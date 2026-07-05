#![allow(unused_assignments)]
use crate as q;

// Opcode constants
const OP_PUSH_I32: u8 = 1;
const OP_PUSH_CONST: u8 = 2;
const OP_UNDEFINED: u8 = 6;
const OP_NULL: u8 = 7;
const OP_PUSH_FALSE: u8 = 9;
const OP_PUSH_TRUE: u8 = 10;
const OP_DROP: u8 = 14;
const OP_DUP: u8 = 17;
const OP_RETURN: u8 = 40;
const OP_RETURN_UNDEF: u8 = 41;
const OP_GET_LOC: u8 = 87;
const OP_PUT_LOC: u8 = 88;
const OP_ADD: u8 = 156;

// JSValue tags (MSB on 64-bit systems)
const JS_TAG_INT: i64 = 0;
const JS_TAG_BOOL: i64 = 1;
const JS_TAG_NULL: i64 = 2;
const JS_TAG_UNDEFINED: i64 = 3;
const JS_TAG_UNINITIALIZED: i64 = 4;

#[no_mangle]
pub unsafe extern "C" fn js_fast_interpreter(
    ctx: *mut q::JSContext,
    sp_ptr: *mut *mut q::JSValue,
    pc_ptr: *mut *const u8,
    var_buf: *mut q::JSValue,
    cpool: *const q::JSValue,
) -> q::JSValue {
    let mut sp = *sp_ptr;
    let mut pc = *pc_ptr;

    loop {
        let opcode = *pc;
        pc = pc.add(1);

        match opcode {
            OP_PUSH_I32 => {
                let val = std::ptr::read_unaligned(pc as *const i32);
                pc = pc.add(4);
                *sp = q::JSValue {
                    u: q::JSValueUnion { int32: val },
                    tag: JS_TAG_INT,
                };
                sp = sp.add(1);
            }
            OP_PUSH_CONST => {
                let const_idx = std::ptr::read_unaligned(pc as *const u32);
                pc = pc.add(4);
                let val = *cpool.add(const_idx as usize);
                *sp = js_dup(ctx, val);
                sp = sp.add(1);
            }
            OP_UNDEFINED => {
                *sp = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNDEFINED,
                };
                sp = sp.add(1);
            }
            OP_NULL => {
                *sp = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_NULL,
                };
                sp = sp.add(1);
            }
            OP_PUSH_FALSE => {
                *sp = q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_BOOL,
                };
                sp = sp.add(1);
            }
            OP_PUSH_TRUE => {
                *sp = q::JSValue {
                    u: q::JSValueUnion { int32: 1 },
                    tag: JS_TAG_BOOL,
                };
                sp = sp.add(1);
            }
            OP_DROP => {
                sp = sp.sub(1);
                js_free(ctx, *sp);
            }
            OP_DUP => {
                let val = *sp.sub(1);
                *sp = js_dup(ctx, val);
                sp = sp.add(1);
            }
            OP_GET_LOC => {
                let idx = std::ptr::read_unaligned(pc as *const u16) as usize;
                pc = pc.add(2);
                let val = *var_buf.add(idx);
                *sp = js_dup(ctx, val);
                sp = sp.add(1);
            }
            OP_PUT_LOC => {
                let idx = std::ptr::read_unaligned(pc as *const u16) as usize;
                pc = pc.add(2);
                sp = sp.sub(1);
                let val = *sp;
                let loc_ptr = var_buf.add(idx);
                js_free(ctx, *loc_ptr);
                *loc_ptr = val;
            }
            OP_ADD => {
                let op1 = *sp.sub(2);
                let op2 = *sp.sub(1);
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
                        sp = sp.sub(1);
                        (*sp.sub(1)).u.int32 = sum;
                        continue;
                    }
                }
                pc = pc.sub(1);
                break;
            }
            OP_RETURN => {
                sp = sp.sub(1);
                let ret_val = *sp;
                *sp_ptr = sp;
                *pc_ptr = pc;
                return ret_val;
            }
            OP_RETURN_UNDEF => {
                *sp_ptr = sp;
                *pc_ptr = pc;
                return q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: JS_TAG_UNDEFINED,
                };
            }
            _ => {
                pc = pc.sub(1);
                break;
            }
        }
    }

    *sp_ptr = sp;
    *pc_ptr = pc;
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
