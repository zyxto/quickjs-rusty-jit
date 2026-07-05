#![allow(unused_assignments)]
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use crate as q;
use crate::compiler::{
    RegInstruction, OP_REG_ADD, OP_REG_DROP, OP_REG_GT, OP_REG_GTE, OP_REG_IF_FALSE,
    OP_REG_IF_TRUE, OP_REG_JMP, OP_REG_LOAD_LOC, OP_REG_LT, OP_REG_LTE, OP_REG_MOV, OP_REG_NULL,
    OP_REG_PUSH_CONST, OP_REG_PUSH_FALSE, OP_REG_PUSH_I32, OP_REG_PUSH_TRUE, OP_REG_RETURN,
    OP_REG_RETURN_UNDEF, OP_REG_SET_LOC, OP_REG_STORE_LOC, OP_REG_UNDEFINED,
};

const JS_TAG_INT: i64 = 0;
const JS_TAG_BOOL: i64 = 1;
const JS_TAG_NULL: i64 = 2;
const JS_TAG_UNDEFINED: i64 = 3;
const JS_TAG_UNINITIALIZED: i64 = 4;
const JS_TAG_FLOAT64: i64 = 8;

fn trace_jit_enabled() -> bool {
    static TRACE_JIT: OnceLock<bool> = OnceLock::new();
    *TRACE_JIT.get_or_init(|| std::env::var_os("QJS_JIT_TRACE").is_some())
}

#[cfg(target_arch = "x86_64")]
fn native_cache() -> &'static Mutex<HashMap<(u64, usize), usize>> {
    static CACHE: OnceLock<Mutex<HashMap<(u64, usize), usize>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_arch = "x86_64")]
unsafe fn run_native_entry(
    entry: usize,
    ctx: *mut q::JSContext,
    var_buf: *mut q::JSValue,
    cpool: *const q::JSValue,
) -> q::JSValue {
    let jit_fn: crate::jit::JitFn = std::mem::transmute(entry as *const ());
    let mut regs = [q::JSValue {
        u: q::JSValueUnion { int32: 0 },
        tag: JS_TAG_UNDEFINED,
    }; 256];
    jit_fn(ctx, regs.as_mut_ptr(), var_buf, cpool)
}

#[no_mangle]
pub unsafe extern "C" fn js_register_interpreter(
    ctx: *mut q::JSContext,
    stack_bytecode: *const u8,
    bytecode_len: i32,
    var_buf: *mut q::JSValue,
    cpool: *const q::JSValue,
) -> q::JSValue {
    let bytecode_slice = std::slice::from_raw_parts(stack_bytecode, bytecode_len as usize);

    let trace_jit = trace_jit_enabled();
    let mut hasher = DefaultHasher::new();
    bytecode_slice.hash(&mut hasher);
    let native_cache_key = (hasher.finish(), bytecode_len as usize);

    #[cfg(target_arch = "x86_64")]
    {
        if let Some(entry) = native_cache()
            .lock()
            .ok()
            .and_then(|cache| cache.get(&native_cache_key).copied())
        {
            let ret = run_native_entry(entry, ctx, var_buf, cpool);
            if ret.tag != JS_TAG_UNINITIALIZED {
                return ret;
            }
            if let Ok(mut cache) = native_cache().lock() {
                cache.remove(&native_cache_key);
            }
        }
    }

    // Compile stack-based bytecode to register-based bytecode at runtime
    let reg_bytecode = match crate::compiler::compile_bytecode(bytecode_slice) {
        Ok(code) => code,
        Err(err) => {
            if trace_jit {
                eprintln!(
                    "[qjs-jit] register compile miss: {err}; stack bytecode={bytecode_slice:?}"
                );
            }
            // Abort and fallback to standard C interpreter
            return q::JSValue {
                u: q::JSValueUnion { int32: 0 },
                tag: JS_TAG_UNINITIALIZED,
            };
        }
    };
    if trace_jit {
        eprintln!(
            "[qjs-jit] register bytecode len={}: {reg_bytecode:?}",
            reg_bytecode.len()
        );
    }

    // Try to run JIT compiled native code
    #[cfg(target_arch = "x86_64")]
    {
        match crate::jit::compile_to_native(&reg_bytecode) {
            Ok(jit_buffer) => {
                let entry = jit_buffer.ptr() as usize;
                if let Ok(mut cache) = native_cache().lock() {
                    if cache.len() < 4096 {
                        cache.insert(native_cache_key, entry);
                    }
                }
                std::mem::forget(jit_buffer);

                let ret = run_native_entry(entry, ctx, var_buf, cpool);
                if ret.tag != JS_TAG_UNINITIALIZED {
                    return ret;
                }
                if let Ok(mut cache) = native_cache().lock() {
                    cache.remove(&native_cache_key);
                }
            }
            Err(err) => {
                if trace_jit {
                    eprintln!("[qjs-jit] native compile miss: {err}");
                }
            }
        }
    }

    // Run the register VM interpreter
    run_register_vm(ctx, &reg_bytecode, var_buf, cpool)
}

unsafe fn run_register_vm(
    ctx: *mut q::JSContext,
    bytecode: &[RegInstruction],
    var_buf: *mut q::JSValue,
    cpool: *const q::JSValue,
) -> q::JSValue {
    // 256 virtual registers, initialized to JS_UNDEFINED
    let mut regs = [q::JSValue {
        u: q::JSValueUnion { int32: 0 },
        tag: JS_TAG_UNDEFINED,
    }; 256];

    let mut pc = 0;
    while pc < bytecode.len() {
        let inst = bytecode[pc];
        pc += 1;

        match inst.op {
            OP_REG_PUSH_I32 => {
                let val = (inst.src1 as i32) | ((inst.src2 as i32) << 16);
                regs[inst.dst as usize] = q::JSValue {
                    u: q::JSValueUnion { int32: val },
                    tag: JS_TAG_INT,
                };
            }
            OP_REG_PUSH_CONST => {
                let val = *cpool.add(inst.src1 as usize);
                regs[inst.dst as usize] = js_dup(ctx, val);
            }
            OP_REG_UNDEFINED => {
                regs[inst.dst as usize] = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNDEFINED,
                };
            }
            OP_REG_NULL => {
                regs[inst.dst as usize] = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_NULL,
                };
            }
            OP_REG_PUSH_FALSE => {
                regs[inst.dst as usize] = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_BOOL,
                };
            }
            OP_REG_PUSH_TRUE => {
                regs[inst.dst as usize] = q::JSValue {
                    u: q::JSValueUnion { int32: 1 },
                    tag: JS_TAG_BOOL,
                };
            }
            OP_REG_DROP => {
                js_free(ctx, regs[inst.dst as usize]);
            }
            OP_REG_MOV => {
                let val = regs[inst.src1 as usize];
                regs[inst.dst as usize] = js_dup(ctx, val);
            }
            OP_REG_LOAD_LOC => {
                let val = *var_buf.add(inst.src1 as usize);
                regs[inst.dst as usize] = js_dup(ctx, val);
            }
            OP_REG_STORE_LOC => {
                let val = regs[inst.src1 as usize];
                let loc_ptr = var_buf.add(inst.dst as usize);
                js_free(ctx, *loc_ptr);
                *loc_ptr = val;
            }
            OP_REG_SET_LOC => {
                let val = regs[inst.src1 as usize];
                let loc_ptr = var_buf.add(inst.dst as usize);
                js_free(ctx, *loc_ptr);
                *loc_ptr = js_dup(ctx, val);
            }
            OP_REG_ADD => {
                let op1 = regs[inst.src1 as usize];
                let op2 = regs[inst.src2 as usize];

                if op1.tag == JS_TAG_INT && op2.tag == JS_TAG_INT {
                    let mut sum = std::mem::MaybeUninit::<i32>::uninit();
                    let mut overflow = std::mem::MaybeUninit::<u8>::uninit();
                    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                    core::arch::asm!(
                        "add {val1:e}, {val2:e}",
                        "seto {overflow}",
                        val1 = inout(reg) op1.u.int32 => sum,
                        val2 = in(reg) op2.u.int32,
                        overflow = out(reg_byte) overflow,
                    );
                    let sum = sum.assume_init();
                    let overflow = overflow.assume_init();
                    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
                    let (sum, overflow) = {
                        let (res, ovf) = op1.u.int32.overflowing_add(op2.u.int32);
                        (res, ovf as u8)
                    };

                    if overflow == 0 {
                        regs[inst.dst as usize] = q::JSValue {
                            u: q::JSValueUnion { int32: sum },
                            tag: JS_TAG_INT,
                        };
                        continue;
                    }
                } else if op1.tag == JS_TAG_FLOAT64 && op2.tag == JS_TAG_FLOAT64 {
                    let sum = op1.u.float64 + op2.u.float64;
                    regs[inst.dst as usize] = q::JSValue {
                        u: q::JSValueUnion { float64: sum },
                        tag: JS_TAG_FLOAT64,
                    };
                    continue;
                }

                // Unhandled types (strings, object concat) or int overflow: abort VM
                // Clean up the registers that have ownership to prevent memory leaks
                for r in regs.iter() {
                    js_free(ctx, *r);
                }
                return q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNINITIALIZED,
                };
            }
            OP_REG_RETURN => {
                let ret_val = regs[inst.dst as usize];
                // Free other registers to prevent leak
                for (i, r) in regs.iter().enumerate() {
                    if i != inst.dst as usize {
                        js_free(ctx, *r);
                    }
                }
                return ret_val;
            }
            OP_REG_RETURN_UNDEF => {
                for r in regs.iter() {
                    js_free(ctx, *r);
                }
                return q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNDEFINED,
                };
            }
            OP_REG_LT | OP_REG_LTE | OP_REG_GT | OP_REG_GTE => {
                let op1 = regs[inst.src1 as usize];
                let op2 = regs[inst.src2 as usize];
                if op1.tag == JS_TAG_INT && op2.tag == JS_TAG_INT {
                    let v1 = op1.u.int32;
                    let v2 = op2.u.int32;
                    let res = match inst.op {
                        OP_REG_LT => v1 < v2,
                        OP_REG_LTE => v1 <= v2,
                        OP_REG_GT => v1 > v2,
                        _ => v1 >= v2,
                    };
                    regs[inst.dst as usize] = q::JSValue {
                        u: q::JSValueUnion { int32: res as i32 },
                        tag: JS_TAG_BOOL,
                    };
                } else {
                    for r in regs.iter() {
                        js_free(ctx, *r);
                    }
                    return q::JSValue {
                        u: q::JSValueUnion { int32: 0 },
                        tag: JS_TAG_UNINITIALIZED,
                    };
                }
            }
            OP_REG_JMP => {
                pc = inst.src1 as usize;
            }
            OP_REG_IF_FALSE => {
                let cond = regs[inst.dst as usize];
                if cond.tag == JS_TAG_BOOL {
                    if cond.u.int32 == 0 {
                        pc = inst.src1 as usize;
                    }
                } else {
                    for r in regs.iter() {
                        js_free(ctx, *r);
                    }
                    return q::JSValue {
                        u: q::JSValueUnion { int32: 0 },
                        tag: JS_TAG_UNINITIALIZED,
                    };
                }
            }
            OP_REG_IF_TRUE => {
                let cond = regs[inst.dst as usize];
                if cond.tag == JS_TAG_BOOL {
                    if cond.u.int32 != 0 {
                        pc = inst.src1 as usize;
                    }
                } else {
                    for r in regs.iter() {
                        js_free(ctx, *r);
                    }
                    return q::JSValue {
                        u: q::JSValueUnion { int32: 0 },
                        tag: JS_TAG_UNINITIALIZED,
                    };
                }
            }
            _ => {
                for r in regs.iter() {
                    js_free(ctx, *r);
                }
                return q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNINITIALIZED,
                };
            }
        }
    }

    for r in regs.iter() {
        js_free(ctx, *r);
    }
    q::JSValue {
        u: q::JSValueUnion { int32: 0 },
        tag: JS_TAG_UNINITIALIZED,
    }
}

#[inline(always)]
unsafe fn js_dup(ctx: *mut q::JSContext, v: q::JSValue) -> q::JSValue {
    if v.tag < 0 {
        q::JS_DupValue(ctx, v);
    }
    v
}

#[inline(always)]
unsafe fn js_free(ctx: *mut q::JSContext, v: q::JSValue) {
    if v.tag < 0 {
        q::JS_FreeValue(ctx, v);
    }
}
