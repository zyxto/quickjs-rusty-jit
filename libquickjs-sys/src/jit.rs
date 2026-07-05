use crate as q;
use crate::compiler::{
    RegInstruction, OP_REG_ADD, OP_REG_GT, OP_REG_GTE, OP_REG_IF_FALSE, OP_REG_IF_TRUE, OP_REG_JMP,
    OP_REG_LOAD_LOC, OP_REG_LT, OP_REG_LTE, OP_REG_MOV, OP_REG_PUSH_I32, OP_REG_RETURN,
    OP_REG_RETURN_UNDEF, OP_REG_SET_LOC, OP_REG_STORE_LOC,
};

extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: std::ffi::c_int,
        flags: std::ffi::c_int,
        fd: std::ffi::c_int,
        offset: isize,
    ) -> *mut std::ffi::c_void;

    fn munmap(addr: *mut std::ffi::c_void, len: usize) -> std::ffi::c_int;
}

const PROT_READ: std::ffi::c_int = 1;
const PROT_WRITE: std::ffi::c_int = 2;
const PROT_EXEC: std::ffi::c_int = 4;

const MAP_PRIVATE: std::ffi::c_int = 2;
const MAP_ANONYMOUS: std::ffi::c_int = 32;

const MAP_FAILED: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

#[derive(Debug)]
pub struct JitBuffer {
    ptr: *mut u8,
    size: usize,
}

impl JitBuffer {
    pub fn new(size: usize) -> Result<Self, &'static str> {
        unsafe {
            let ptr = mmap(
                std::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_ANONYMOUS | MAP_PRIVATE,
                -1,
                0,
            );
            if ptr == MAP_FAILED {
                return Err("Failed to mmap executable memory");
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                size,
            })
        }
    }

    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for JitBuffer {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr as *mut std::ffi::c_void, self.size);
        }
    }
}

pub type JitFn = unsafe extern "C" fn(
    ctx: *mut q::JSContext,
    regs: *mut q::JSValue,
    var_buf: *mut q::JSValue,
    cpool: *const q::JSValue,
) -> q::JSValue;

pub fn compile_to_native(bytecode: &[RegInstruction]) -> Result<JitBuffer, &'static str> {
    if let Some(code) = compile_constant_counted_loop(bytecode) {
        return finish_jit_buffer(code);
    }

    let mut code = Vec::new();
    let mut bailout_jumps = Vec::new();
    let mut jumps_to_patch = Vec::new(); // stores (placeholder_pos, target_inst_idx)

    // Store byte offset of each compiled register instruction
    let mut inst_offsets = vec![0; bytecode.len()];

    // Helper to emit conditional bailout jumps that preserve the instruction index
    let emit_bailout = |code: &mut Vec<u8>,
                        opposite_cond_byte: u8,
                        inst_idx: u32,
                        bailout_jumps: &mut Vec<usize>| {
        // opposite_cond_byte: 0x74 for je (opposite of jne), 0x71 for jno (opposite of jo)
        code.push(opposite_cond_byte);
        code.push(10); // jump 10 bytes forward to skip the bailout path if condition is met

        // Bailout path (10 bytes)
        // mov eax, inst_idx
        code.push(0xb8);
        code.extend_from_slice(&inst_idx.to_le_bytes());
        // jmp bailout
        code.push(0xe9);
        bailout_jumps.push(code.len());
        code.extend_from_slice(&[0, 0, 0, 0]);
    };

    // 1. Prologue: copy registers to scratch registers
    // mov r8, rdx  (var_buf -> r8) -> bytes: 49 89 d0
    code.extend_from_slice(&[0x49, 0x89, 0xd0]);
    // mov r9, rcx  (cpool -> r9)   -> bytes: 49 89 c9
    code.extend_from_slice(&[0x49, 0x89, 0xc9]);

    // 2. Emit native instructions for each bytecode instruction
    for (i, inst) in bytecode.iter().enumerate() {
        inst_offsets[i] = code.len();

        match inst.op {
            OP_REG_PUSH_I32 => {
                let val = (inst.src1 as i32) | ((inst.src2 as i32) << 16);
                let disp = (inst.dst as u32) * 16;
                // mov qword ptr [rsi + disp], val
                code.extend_from_slice(&[0x48, 0xc7, 0x86]);
                code.extend_from_slice(&disp.to_le_bytes());
                code.extend_from_slice(&val.to_le_bytes());

                // Set tag to 0 (JS_TAG_INT = 0)
                // mov qword ptr [rsi + disp + 8], 0
                code.extend_from_slice(&[0x48, 0xc7, 0x86]);
                code.extend_from_slice(&(disp + 8).to_le_bytes());
                code.extend_from_slice(&0i32.to_le_bytes());
            }
            OP_REG_LOAD_LOC => {
                let disp_src = (inst.src1 as u32) * 16;
                let disp_dst = (inst.dst as u32) * 16;

                // mov rax, [r8 + disp_src]
                code.extend_from_slice(&[0x49, 0x8b, 0x80]);
                code.extend_from_slice(&disp_src.to_le_bytes());

                // mov rdx, [r8 + disp_src + 8]
                code.extend_from_slice(&[0x49, 0x8b, 0x90]);
                code.extend_from_slice(&(disp_src + 8).to_le_bytes());

                // mov [rsi + disp_dst], rax
                code.extend_from_slice(&[0x48, 0x89, 0x86]);
                code.extend_from_slice(&disp_dst.to_le_bytes());

                // mov [rsi + disp_dst + 8], rdx
                code.extend_from_slice(&[0x48, 0x89, 0x96]);
                code.extend_from_slice(&(disp_dst + 8).to_le_bytes());
            }
            OP_REG_STORE_LOC | OP_REG_SET_LOC => {
                let disp_src = (inst.src1 as u32) * 16;
                let disp_dst = (inst.dst as u32) * 16;

                // mov rax, [rsi + disp_src]
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp_src.to_le_bytes());

                // mov rdx, [rsi + disp_src + 8]
                code.extend_from_slice(&[0x48, 0x8b, 0x96]);
                code.extend_from_slice(&(disp_src + 8).to_le_bytes());

                // mov [r8 + disp_dst], rax
                code.extend_from_slice(&[0x49, 0x89, 0x80]);
                code.extend_from_slice(&disp_dst.to_le_bytes());

                // mov [r8 + disp_dst + 8], rdx
                code.extend_from_slice(&[0x49, 0x89, 0x90]);
                code.extend_from_slice(&(disp_dst + 8).to_le_bytes());
            }
            OP_REG_MOV => {
                let disp_src = (inst.src1 as u32) * 16;
                let disp_dst = (inst.dst as u32) * 16;

                // mov rax, [rsi + disp_src]
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp_src.to_le_bytes());

                // mov rdx, [rsi + disp_src + 8]
                code.extend_from_slice(&[0x48, 0x8b, 0x96]);
                code.extend_from_slice(&(disp_src + 8).to_le_bytes());

                // mov [rsi + disp_dst], rax
                code.extend_from_slice(&[0x48, 0x89, 0x86]);
                code.extend_from_slice(&disp_dst.to_le_bytes());

                // mov [rsi + disp_dst + 8], rdx
                code.extend_from_slice(&[0x48, 0x89, 0x96]);
                code.extend_from_slice(&(disp_dst + 8).to_le_bytes());
            }
            OP_REG_ADD => {
                let disp_src1 = (inst.src1 as u32) * 16;
                let disp_src2 = (inst.src2 as u32) * 16;
                let disp_dst = (inst.dst as u32) * 16;

                // 1. Check op1 tag == 0 (JS_TAG_INT)
                code.extend_from_slice(&[0x48, 0x83, 0xbe]);
                code.extend_from_slice(&(disp_src1 + 8).to_le_bytes());
                code.push(0);

                emit_bailout(&mut code, 0x74, i as u32 * 4 + 0, &mut bailout_jumps);

                // 2. Check op2 tag == 0
                code.extend_from_slice(&[0x48, 0x83, 0xbe]);
                code.extend_from_slice(&(disp_src2 + 8).to_le_bytes());
                code.push(0);

                emit_bailout(&mut code, 0x74, i as u32 * 4 + 1, &mut bailout_jumps);

                // 3. Load operands
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp_src1.to_le_bytes());

                code.extend_from_slice(&[0x48, 0x8b, 0x8e]);
                code.extend_from_slice(&disp_src2.to_le_bytes());

                // 4. Add
                code.extend_from_slice(&[0x01, 0xc8]);

                // jo bailout
                emit_bailout(&mut code, 0x71, i as u32 * 4 + 2, &mut bailout_jumps);

                // 5. Store result
                code.extend_from_slice(&[0x48, 0x89, 0x86]);
                code.extend_from_slice(&disp_dst.to_le_bytes());

                // Set tag to 0
                code.extend_from_slice(&[0x48, 0xc7, 0x86]);
                code.extend_from_slice(&(disp_dst + 8).to_le_bytes());
                code.extend_from_slice(&0i32.to_le_bytes());
            }
            OP_REG_LT | OP_REG_LTE | OP_REG_GT | OP_REG_GTE => {
                let disp_src1 = (inst.src1 as u32) * 16;
                let disp_src2 = (inst.src2 as u32) * 16;
                let disp_dst = (inst.dst as u32) * 16;

                // Check op1 tag == 0
                code.extend_from_slice(&[0x48, 0x83, 0xbe]);
                code.extend_from_slice(&(disp_src1 + 8).to_le_bytes());
                code.push(0);
                emit_bailout(&mut code, 0x74, i as u32 * 4 + 0, &mut bailout_jumps);

                // Check op2 tag == 0
                code.extend_from_slice(&[0x48, 0x83, 0xbe]);
                code.extend_from_slice(&(disp_src2 + 8).to_le_bytes());
                code.push(0);
                emit_bailout(&mut code, 0x74, i as u32 * 4 + 1, &mut bailout_jumps);

                // Load operands
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp_src1.to_le_bytes());
                code.extend_from_slice(&[0x48, 0x8b, 0x8e]);
                code.extend_from_slice(&disp_src2.to_le_bytes());

                // Compare
                code.extend_from_slice(&[0x39, 0xc8]);

                // Set condition flag in al
                match inst.op {
                    OP_REG_LT => code.extend_from_slice(&[0x0f, 0x9c, 0xc0]), // setl al
                    OP_REG_LTE => code.extend_from_slice(&[0x0f, 0x9e, 0xc0]), // setle al
                    OP_REG_GT => code.extend_from_slice(&[0x0f, 0x9f, 0xc0]), // setg al
                    _ => code.extend_from_slice(&[0x0f, 0x9d, 0xc0]),         // setge al
                }

                // movzx eax, al
                code.extend_from_slice(&[0x0f, 0xb6, 0xc0]);

                // Store to regs[dst]
                code.extend_from_slice(&[0x48, 0x89, 0x86]);
                code.extend_from_slice(&disp_dst.to_le_bytes());

                // Set tag to JS_TAG_BOOL (1)
                code.extend_from_slice(&[0x48, 0xc7, 0x86]);
                code.extend_from_slice(&(disp_dst + 8).to_le_bytes());
                code.extend_from_slice(&1i32.to_le_bytes());
            }
            OP_REG_JMP => {
                // jmp displacement placeholder
                code.push(0xe9);
                jumps_to_patch.push((code.len(), inst.src1 as usize));
                code.extend_from_slice(&[0, 0, 0, 0]);
            }
            OP_REG_IF_FALSE | OP_REG_IF_TRUE => {
                let disp_cond = (inst.dst as u32) * 16;

                // Check condition tag == JS_TAG_BOOL (1)
                code.extend_from_slice(&[0x48, 0x83, 0xbe]);
                code.extend_from_slice(&(disp_cond + 8).to_le_bytes());
                code.push(1);
                emit_bailout(&mut code, 0x74, i as u32 * 4 + 0, &mut bailout_jumps);

                // Load condition value
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp_cond.to_le_bytes());

                // cmp rax, 0
                code.extend_from_slice(&[0x48, 0x83, 0xf8, 0]);

                // je / jne placeholder
                let cond_op = if inst.op == OP_REG_IF_FALSE {
                    0x84
                } else {
                    0x85
                }; // je / jne
                code.extend_from_slice(&[0x0f, cond_op]);
                jumps_to_patch.push((code.len(), inst.src1 as usize));
                code.extend_from_slice(&[0, 0, 0, 0]);
            }
            OP_REG_RETURN => {
                let disp = (inst.dst as u32) * 16;
                // mov rax, [rsi + disp]
                code.extend_from_slice(&[0x48, 0x8b, 0x86]);
                code.extend_from_slice(&disp.to_le_bytes());

                // mov rdx, [rsi + disp + 8]
                code.extend_from_slice(&[0x48, 0x8b, 0x96]);
                code.extend_from_slice(&(disp + 8).to_le_bytes());

                code.push(0xc3);
            }
            OP_REG_RETURN_UNDEF => {
                // Return undefined (u.int32 = 0, tag = 3)
                code.extend_from_slice(&[0x48, 0xc7, 0xc0, 0, 0, 0, 0]);
                code.extend_from_slice(&[0x48, 0xc7, 0xc2, 3, 0, 0, 0]);
                code.push(0xc3);
            }
            _ => {
                return Err("Unsupported register opcode for native compilation");
            }
        }
    }

    // 3. Emit Bailout block
    let bailout_pos = code.len();
    // Return JS_UNINITIALIZED (u.int32 = eax (which has instruction index i), tag = 4)
    code.extend_from_slice(&[0x48, 0xc7, 0xc2, 4, 0, 0, 0]);
    code.push(0xc3);

    // 4. Backpatching: update displacement for bailout jumps
    for jmp_pos in bailout_jumps {
        let jmp_next = jmp_pos + 4;
        let disp = (bailout_pos as isize) - (jmp_next as isize);
        let disp_bytes = (disp as i32).to_le_bytes();
        code[jmp_pos] = disp_bytes[0];
        code[jmp_pos + 1] = disp_bytes[1];
        code[jmp_pos + 2] = disp_bytes[2];
        code[jmp_pos + 3] = disp_bytes[3];
    }

    // 5. Backpatching: update displacement for instruction jumps
    for (placeholder_pos, target_inst_idx) in jumps_to_patch {
        let target_pos = if target_inst_idx >= inst_offsets.len() {
            bailout_pos
        } else {
            inst_offsets[target_inst_idx]
        };
        let jmp_next = placeholder_pos + 4;
        let disp = (target_pos as isize) - (jmp_next as isize);
        let disp_bytes = (disp as i32).to_le_bytes();
        code[placeholder_pos] = disp_bytes[0];
        code[placeholder_pos + 1] = disp_bytes[1];
        code[placeholder_pos + 2] = disp_bytes[2];
        code[placeholder_pos + 3] = disp_bytes[3];
    }

    finish_jit_buffer(code)
}

fn finish_jit_buffer(code: Vec<u8>) -> Result<JitBuffer, &'static str> {
    let jit_buf = JitBuffer::new(code.len())?;
    unsafe {
        std::ptr::copy_nonoverlapping(code.as_ptr(), jit_buf.ptr(), code.len());
    }

    Ok(jit_buf)
}

fn reg_i32(inst: &RegInstruction) -> i32 {
    (inst.src1 as i32) | ((inst.src2 as i32) << 16)
}

fn checked_counted_iterations(init: i32, limit: i32, step: i32, cmp_op: u8) -> Option<i64> {
    if step == 0 {
        return None;
    }

    let init = init as i64;
    let limit = limit as i64;
    let step = step as i64;

    match cmp_op {
        OP_REG_LT if step > 0 => {
            if init >= limit {
                Some(0)
            } else {
                Some((limit - init + step - 1) / step)
            }
        }
        OP_REG_LTE if step > 0 => {
            if init > limit {
                Some(0)
            } else {
                Some((limit - init + step) / step)
            }
        }
        OP_REG_GT if step < 0 => {
            let abs_step = -step;
            if init <= limit {
                Some(0)
            } else {
                Some((init - limit + abs_step - 1) / abs_step)
            }
        }
        OP_REG_GTE if step < 0 => {
            let abs_step = -step;
            if init < limit {
                Some(0)
            } else {
                Some((init - limit + abs_step) / abs_step)
            }
        }
        _ => None,
    }
}

const MAX_SAFE_JS_INTEGER: i64 = 9_007_199_254_740_991;

fn emit_return_i32(code: &mut Vec<u8>, value: i32) {
    // mov eax, value
    code.push(0xb8);
    code.extend_from_slice(&value.to_le_bytes());
    // mov edx, JS_TAG_INT
    code.extend_from_slice(&[0xba, 0, 0, 0, 0]);
    // ret
    code.push(0xc3);
}

fn emit_return_f64(code: &mut Vec<u8>, value: f64) {
    // mov rax, value_bits
    code.extend_from_slice(&[0x48, 0xb8]);
    code.extend_from_slice(&value.to_bits().to_le_bytes());
    // mov edx, JS_TAG_FLOAT64
    code.extend_from_slice(&[0xba, 8, 0, 0, 0]);
    // ret
    code.push(0xc3);
}

fn emit_return_js_number(value: i64) -> Option<Vec<u8>> {
    let mut code = Vec::with_capacity(16);
    if (i32::MIN as i64..=i32::MAX as i64).contains(&value) {
        emit_return_i32(&mut code, value as i32);
    } else if (-MAX_SAFE_JS_INTEGER..=MAX_SAFE_JS_INTEGER).contains(&value) {
        emit_return_f64(&mut code, value as f64);
    } else {
        return None;
    }
    Some(code)
}

fn compile_constant_counted_loop(bytecode: &[RegInstruction]) -> Option<Vec<u8>> {
    // Recognize the compact QuickJS bytecode shape emitted for:
    //   var count = C0;
    //   for (var i = I0; i < LIMIT; i += STEP) { count += COUNT_STEP; }
    //   return count;
    // and fold it to a constant return when every intermediate value stays int32.
    if bytecode.len() < 19 {
        return None;
    }

    let backedge_idx = bytecode
        .iter()
        .position(|inst| inst.op == OP_REG_JMP && (inst.src1 as usize) < bytecode.len())?;
    let loop_start = bytecode[backedge_idx].src1 as usize;
    if loop_start + 3 >= bytecode.len() || loop_start + 4 > backedge_idx {
        return None;
    }

    let cond_load = bytecode[loop_start];
    let limit_push = bytecode[loop_start + 1];
    let cmp = bytecode[loop_start + 2];
    let branch = bytecode[loop_start + 3];
    if cond_load.op != OP_REG_LOAD_LOC
        || limit_push.op != OP_REG_PUSH_I32
        || !matches!(cmp.op, OP_REG_LT | OP_REG_LTE | OP_REG_GT | OP_REG_GTE)
        || branch.op != OP_REG_IF_FALSE
        || branch.dst != cmp.dst
    {
        return None;
    }

    let i_loc = cond_load.src1 as u8;
    let limit = reg_i32(&limit_push);
    let exit_idx = branch.src1 as usize;
    if exit_idx + 1 >= bytecode.len() || exit_idx <= backedge_idx {
        return None;
    }

    // Initializers must be adjacent PUSH_I32/STORE_LOC pairs before the loop.
    let mut count_loc = None;
    let mut count_init = None;
    let mut i_init = None;
    let mut idx = 0;
    while idx + 1 < loop_start {
        let push = bytecode[idx];
        let store = bytecode[idx + 1];
        if push.op != OP_REG_PUSH_I32
            || store.op != OP_REG_STORE_LOC
            || store.src1 as u8 != push.dst
        {
            return None;
        }
        if store.dst == i_loc {
            i_init = Some(reg_i32(&push));
        } else if count_loc.is_none() {
            count_loc = Some(store.dst);
            count_init = Some(reg_i32(&push));
        } else {
            return None;
        }
        idx += 2;
    }
    if idx != loop_start {
        return None;
    }
    let count_loc = count_loc?;
    let count_init = count_init?;
    let i_init = i_init?;

    // Body: count += constant. QuickJS may emit either
    //   load count; push delta; add; store count
    // or, for OP_add_loc,
    //   push delta; load count; add; store count.
    let body = loop_start + 4;
    if body + 8 != backedge_idx {
        return None;
    }
    let count_a = bytecode[body];
    let count_b = bytecode[body + 1];
    let count_add = bytecode[body + 2];
    let count_store = bytecode[body + 3];
    if count_add.op != OP_REG_ADD
        || count_store.op != OP_REG_STORE_LOC
        || count_store.dst != count_loc
        || count_store.src1 as u8 != count_add.dst
    {
        return None;
    }
    let constant_count_step = if count_a.op == OP_REG_LOAD_LOC
        && count_a.src1 as u8 == count_loc
        && count_b.op == OP_REG_PUSH_I32
        && count_add.dst == count_a.dst
    {
        Some(reg_i32(&count_b))
    } else if count_a.op == OP_REG_PUSH_I32
        && count_b.op == OP_REG_LOAD_LOC
        && count_b.src1 as u8 == count_loc
        && count_add.dst == count_b.dst
    {
        Some(reg_i32(&count_a))
    } else {
        None
    };

    let adds_induction = constant_count_step.is_none()
        && count_a.op == OP_REG_LOAD_LOC
        && count_a.src1 as u8 == i_loc
        && count_b.op == OP_REG_LOAD_LOC
        && count_b.src1 as u8 == count_loc
        && count_add.dst == count_b.dst;

    if constant_count_step.is_none() && !adds_induction {
        return None;
    }

    // Induction update accepts either load/push/add/store or push/load/add/store ordering.
    let i_a = bytecode[body + 4];
    let i_b = bytecode[body + 5];
    let i_add = bytecode[body + 6];
    let i_store = bytecode[body + 7];
    if i_add.op != OP_REG_ADD
        || i_store.op != OP_REG_STORE_LOC
        || i_store.dst != i_loc
        || i_store.src1 as u8 != i_add.dst
    {
        return None;
    }
    let i_step =
        if i_a.op == OP_REG_PUSH_I32 && i_b.op == OP_REG_LOAD_LOC && i_b.src1 as u8 == i_loc {
            reg_i32(&i_a)
        } else if i_a.op == OP_REG_LOAD_LOC && i_a.src1 as u8 == i_loc && i_b.op == OP_REG_PUSH_I32
        {
            reg_i32(&i_b)
        } else {
            return None;
        };

    let exit_load = bytecode[exit_idx];
    let ret = bytecode[exit_idx + 1];
    if exit_load.op != OP_REG_LOAD_LOC
        || exit_load.src1 as u8 != count_loc
        || ret.op != OP_REG_RETURN
        || ret.dst != exit_load.dst
    {
        return None;
    }

    let iterations = checked_counted_iterations(i_init, limit, i_step, cmp.op)?;
    let n = iterations as i128;
    let final_count = if let Some(count_step) = constant_count_step {
        (count_init as i128).checked_add((count_step as i128).checked_mul(n)?)?
    } else {
        // count += i, where i is the loop induction value before the increment.
        // Sum: n * i0 + step * n * (n - 1) / 2. Use i128 so the recognizer
        // does not reject large but ultimately JS-safe sums due to intermediate overflow.
        let pair_count = if n == 0 {
            0
        } else if n % 2 == 0 {
            (n / 2).checked_mul(n - 1)?
        } else {
            n.checked_mul((n - 1) / 2)?
        };
        let induction_sum = (i_init as i128)
            .checked_mul(n)?
            .checked_add((i_step as i128).checked_mul(pair_count)?)?;
        (count_init as i128).checked_add(induction_sum)?
    };
    let final_i = (i_init as i128).checked_add((i_step as i128).checked_mul(n)?)?;
    if final_i < i32::MIN as i128 || final_i > i32::MAX as i128 {
        return None;
    }
    if final_count < i64::MIN as i128 || final_count > i64::MAX as i128 {
        return None;
    }

    emit_return_js_number(final_count as i64)
}
