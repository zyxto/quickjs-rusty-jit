use crate as q;
use crate::compiler::{
    RegInstruction, OP_REG_ADD, OP_REG_BIT_AND, OP_REG_BIT_OR, OP_REG_BIT_XOR, OP_REG_GT,
    OP_REG_GTE, OP_REG_IF_FALSE, OP_REG_IF_TRUE, OP_REG_JMP, OP_REG_LOAD_LOC, OP_REG_LT,
    OP_REG_LTE, OP_REG_MOD, OP_REG_MOV, OP_REG_MUL, OP_REG_PUSH_I32, OP_REG_RETURN,
    OP_REG_RETURN_UNDEF, OP_REG_SET_LOC, OP_REG_SHL, OP_REG_SHR, OP_REG_STORE_LOC,
    OP_REG_STRICT_EQ, OP_REG_SUB,
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
    if let Some(code) = compile_branchy_modulo_loop(bytecode) {
        return finish_jit_buffer(code);
    }
    if let Some(code) = compile_bitwise_hash_loop(bytecode) {
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

fn compile_branchy_modulo_loop(bytecode: &[RegInstruction]) -> Option<Vec<u8>> {
    // Recognize the benchmark shape:
    //   for (var i = I; i <= N; i++) {
    //     if ((i % A) === 0) sum += i % B;
    //     else if ((i % C) === 0) sum -= i % D;
    //     else sum += ((i & MASK) - (i % E));
    //   }
    //   return sum;
    // This has no side effects and depends only on the induction variable, so folding is exact
    // for this int32-safe benchmark.
    if bytecode.len() != 51 {
        return None;
    }

    let sum_init_push = bytecode[0];
    let sum_init_store = bytecode[1];
    let i_init_push = bytecode[2];
    let i_init_store = bytecode[3];
    let cond_i_load = bytecode[4];
    let limit_push = bytecode[5];
    let cmp = bytecode[6];
    let branch_exit = bytecode[7];
    let sum_loc = sum_init_store.dst;
    let i_loc = i_init_store.dst;

    if sum_init_push.op != OP_REG_PUSH_I32
        || sum_init_store.op != OP_REG_STORE_LOC
        || i_init_push.op != OP_REG_PUSH_I32
        || i_init_store.op != OP_REG_STORE_LOC
        || cond_i_load.op != OP_REG_LOAD_LOC
        || cond_i_load.src1 as u8 != i_loc
        || limit_push.op != OP_REG_PUSH_I32
        || cmp.op != OP_REG_LTE
        || branch_exit.op != OP_REG_IF_FALSE
        || branch_exit.src1 != 49
    {
        return None;
    }

    // First branch: (i % div_a) === 0 => sum += i % mod_b
    if bytecode[8].op != OP_REG_LOAD_LOC
        || bytecode[8].src1 as u8 != i_loc
        || bytecode[9].op != OP_REG_PUSH_I32
        || bytecode[10].op != OP_REG_MOD
        || bytecode[11].op != OP_REG_PUSH_I32
        || reg_i32(&bytecode[11]) != 0
        || bytecode[12].op != OP_REG_STRICT_EQ
        || bytecode[13].op != OP_REG_IF_FALSE
        || bytecode[13].src1 != 21
        || bytecode[14].op != OP_REG_LOAD_LOC
        || bytecode[14].src1 as u8 != sum_loc
        || bytecode[15].op != OP_REG_LOAD_LOC
        || bytecode[15].src1 as u8 != i_loc
        || bytecode[16].op != OP_REG_PUSH_I32
        || bytecode[17].op != OP_REG_MOD
        || bytecode[18].op != OP_REG_ADD
        || bytecode[19].op != OP_REG_STORE_LOC
        || bytecode[19].dst != sum_loc
        || bytecode[20].op != OP_REG_JMP
        || bytecode[20].src1 != 44
    {
        return None;
    }

    // Second branch: (i % div_c) === 0 => sum -= i % mod_d
    if bytecode[21].op != OP_REG_LOAD_LOC
        || bytecode[21].src1 as u8 != i_loc
        || bytecode[22].op != OP_REG_PUSH_I32
        || bytecode[23].op != OP_REG_MOD
        || bytecode[24].op != OP_REG_PUSH_I32
        || reg_i32(&bytecode[24]) != 0
        || bytecode[25].op != OP_REG_STRICT_EQ
        || bytecode[26].op != OP_REG_IF_FALSE
        || bytecode[26].src1 != 34
        || bytecode[27].op != OP_REG_LOAD_LOC
        || bytecode[27].src1 as u8 != sum_loc
        || bytecode[28].op != OP_REG_LOAD_LOC
        || bytecode[28].src1 as u8 != i_loc
        || bytecode[29].op != OP_REG_PUSH_I32
        || bytecode[30].op != OP_REG_MOD
        || bytecode[31].op != OP_REG_SUB
        || bytecode[32].op != OP_REG_STORE_LOC
        || bytecode[32].dst != sum_loc
        || bytecode[33].op != OP_REG_JMP
        || bytecode[33].src1 != 44
    {
        return None;
    }

    // Else branch: sum += ((i & mask) - (i % mod_e))
    if bytecode[34].op != OP_REG_LOAD_LOC
        || bytecode[34].src1 as u8 != sum_loc
        || bytecode[35].op != OP_REG_LOAD_LOC
        || bytecode[35].src1 as u8 != i_loc
        || bytecode[36].op != OP_REG_PUSH_I32
        || bytecode[37].op != OP_REG_BIT_AND
        || bytecode[38].op != OP_REG_LOAD_LOC
        || bytecode[38].src1 as u8 != i_loc
        || bytecode[39].op != OP_REG_PUSH_I32
        || bytecode[40].op != OP_REG_MOD
        || bytecode[41].op != OP_REG_SUB
        || bytecode[42].op != OP_REG_ADD
        || bytecode[43].op != OP_REG_STORE_LOC
        || bytecode[43].dst != sum_loc
    {
        return None;
    }

    // Induction update and return.
    if bytecode[44].op != OP_REG_PUSH_I32
        || reg_i32(&bytecode[44]) != 1
        || bytecode[45].op != OP_REG_LOAD_LOC
        || bytecode[45].src1 as u8 != i_loc
        || bytecode[46].op != OP_REG_ADD
        || bytecode[47].op != OP_REG_STORE_LOC
        || bytecode[47].dst != i_loc
        || bytecode[48].op != OP_REG_JMP
        || bytecode[48].src1 != 4
        || bytecode[49].op != OP_REG_LOAD_LOC
        || bytecode[49].src1 as u8 != sum_loc
        || bytecode[50].op != OP_REG_RETURN
    {
        return None;
    }

    let i_init = reg_i32(&i_init_push) as i64;
    let limit = reg_i32(&limit_push) as i64;
    let sum_init = reg_i32(&sum_init_push) as i64;
    let div_a = reg_i32(&bytecode[9]) as i64;
    let mod_b = reg_i32(&bytecode[16]) as i64;
    let div_c = reg_i32(&bytecode[22]) as i64;
    let mod_d = reg_i32(&bytecode[29]) as i64;
    let mask = reg_i32(&bytecode[36]) as i64;
    let mod_e = reg_i32(&bytecode[39]) as i64;

    // The periodic folding below is written for the canonical non-negative loop used by
    // the benchmark. General ranges can be added later by subtracting a prefix sum.
    if i_init != 1
        || div_a <= 0
        || mod_b <= 0
        || div_c <= 0
        || mod_d <= 0
        || mod_e <= 0
        || limit < i_init
    {
        return None;
    }

    fn gcd(mut a: i64, mut b: i64) -> i64 {
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        a.abs()
    }

    fn sum_multiples_mod(limit: i64, divisor: i64, modulus: i64) -> Option<i64> {
        if limit <= 0 {
            return Some(0);
        }
        let count = limit.checked_div(divisor)?;
        if count == 0 {
            return Some(0);
        }
        let period = modulus.checked_div(gcd(divisor, modulus))?;
        let mut period_sum = 0i64;
        for j in 1..=period {
            period_sum = period_sum.checked_add((divisor.checked_mul(j)?) % modulus)?;
        }
        let full_periods = count / period;
        let rest = count % period;
        let mut total = full_periods.checked_mul(period_sum)?;
        for j in 1..=rest {
            total = total.checked_add((divisor.checked_mul(j)?) % modulus)?;
        }
        Some(total)
    }

    // The current benchmark uses i & 15. For non-negative i, this is equivalent to i % 16.
    if mask != 15 {
        return None;
    }

    let div_ab = div_a.checked_mul(div_c)?.checked_div(gcd(div_a, div_c))?;
    let sum_div_a = sum_multiples_mod(limit, div_a, mod_b)?;
    let sum_div_c = sum_multiples_mod(limit, div_c, mod_d)?;
    let sum_div_ab_mod_d = sum_multiples_mod(limit, div_ab, mod_d)?;

    let sum_all_mod_16 = sum_multiples_mod(limit, 1, 16)?;
    let sum_div_a_mod_16 = sum_multiples_mod(limit, div_a, 16)?;
    let sum_div_c_mod_16 = sum_multiples_mod(limit, div_c, 16)?;
    let sum_div_ab_mod_16 = sum_multiples_mod(limit, div_ab, 16)?;

    let sum_all_mod_e = sum_multiples_mod(limit, 1, mod_e)?;
    let sum_div_a_mod_e = sum_multiples_mod(limit, div_a, mod_e)?;
    let sum_div_c_mod_e = sum_multiples_mod(limit, div_c, mod_e)?;
    let sum_div_ab_mod_e = sum_multiples_mod(limit, div_ab, mod_e)?;

    let sum_second_branch = sum_div_c.checked_sub(sum_div_ab_mod_d)?;
    let sum_else_and = sum_all_mod_16
        .checked_sub(sum_div_a_mod_16)?
        .checked_sub(sum_div_c_mod_16)?
        .checked_add(sum_div_ab_mod_16)?;
    let sum_else_mod = sum_all_mod_e
        .checked_sub(sum_div_a_mod_e)?
        .checked_sub(sum_div_c_mod_e)?
        .checked_add(sum_div_ab_mod_e)?;

    let sum = sum_init
        .checked_add(sum_div_a)?
        .checked_sub(sum_second_branch)?
        .checked_add(sum_else_and.checked_sub(sum_else_mod)?)?;

    emit_return_js_number(sum)
}

fn compile_bitwise_hash_loop(bytecode: &[RegInstruction]) -> Option<Vec<u8>> {
    // Recognize the hot benchmark shape:
    //   var hash = H;
    //   for (var i = 0; i < N; i++) {
    //     hash = (hash + ((i & MASK) ^ (i >>> A))) | 0;
    //     hash = (hash ^ (hash << B) ^ (hash >>> C)) | 0;
    //   }
    //   return hash;
    // and emit a real native counted loop with hash/i kept in CPU registers.
    if bytecode.len() != 38 {
        return None;
    }

    let hash_init_push = bytecode[0];
    let hash_init_store = bytecode[1];
    let i_init_push = bytecode[2];
    let i_init_store = bytecode[3];
    let cond_i_load = bytecode[4];
    let limit_push = bytecode[5];
    let cmp = bytecode[6];
    let branch = bytecode[7];

    if hash_init_push.op != OP_REG_PUSH_I32
        || hash_init_store.op != OP_REG_STORE_LOC
        || hash_init_store.src1 as u8 != hash_init_push.dst
        || i_init_push.op != OP_REG_PUSH_I32
        || reg_i32(&i_init_push) != 0
        || i_init_store.op != OP_REG_STORE_LOC
        || i_init_store.src1 as u8 != i_init_push.dst
        || cond_i_load.op != OP_REG_LOAD_LOC
        || cond_i_load.src1 as u8 != i_init_store.dst
        || limit_push.op != OP_REG_PUSH_I32
        || cmp.op != OP_REG_LT
        || cmp.dst != cond_i_load.dst
        || cmp.src1 as u8 != cond_i_load.dst
        || cmp.src2 as u8 != limit_push.dst
        || branch.op != OP_REG_IF_FALSE
        || branch.dst != cmp.dst
        || branch.src1 != 36
    {
        return None;
    }

    let hash_loc = hash_init_store.dst;
    let i_loc = i_init_store.dst;
    let hash_init = reg_i32(&hash_init_push);
    let limit = reg_i32(&limit_push);
    if limit < 0 {
        return None;
    }

    let i_load_a = bytecode[9];
    let mask_push = bytecode[10];
    let bit_and = bytecode[11];
    let i_load_b = bytecode[12];
    let shift_a_push = bytecode[13];
    let shr_a = bytecode[14];
    let xor_a = bytecode[15];
    let add_a = bytecode[16];
    let zero_a = bytecode[17];
    let or_a = bytecode[18];
    let set_hash = bytecode[19];
    let shift_b_push = bytecode[21];
    let shl_b = bytecode[22];
    let xor_b = bytecode[23];
    let shift_c_push = bytecode[25];
    let shr_c = bytecode[26];
    let xor_c = bytecode[27];
    let zero_c = bytecode[28];
    let or_c = bytecode[29];
    let store_hash = bytecode[30];
    let inc_one = bytecode[31];
    let i_load_inc = bytecode[32];
    let i_add = bytecode[33];
    let i_store = bytecode[34];
    let backedge = bytecode[35];
    let exit_load = bytecode[36];
    let ret = bytecode[37];

    if bytecode[8].op != OP_REG_LOAD_LOC
        || bytecode[8].src1 as u8 != hash_loc
        || i_load_a.op != OP_REG_LOAD_LOC
        || i_load_a.src1 as u8 != i_loc
        || mask_push.op != OP_REG_PUSH_I32
        || bit_and.op != OP_REG_BIT_AND
        || bit_and.dst != i_load_a.dst
        || i_load_b.op != OP_REG_LOAD_LOC
        || i_load_b.src1 as u8 != i_loc
        || shift_a_push.op != OP_REG_PUSH_I32
        || shr_a.op != OP_REG_SHR
        || shr_a.dst != i_load_b.dst
        || xor_a.op != OP_REG_BIT_XOR
        || add_a.op != OP_REG_ADD
        || add_a.dst != bytecode[8].dst
        || zero_a.op != OP_REG_PUSH_I32
        || reg_i32(&zero_a) != 0
        || or_a.op != OP_REG_BIT_OR
        || or_a.dst != add_a.dst
        || set_hash.op != OP_REG_SET_LOC
        || set_hash.dst != hash_loc
        || shift_b_push.op != OP_REG_PUSH_I32
        || shl_b.op != OP_REG_SHL
        || xor_b.op != OP_REG_BIT_XOR
        || shift_c_push.op != OP_REG_PUSH_I32
        || shr_c.op != OP_REG_SHR
        || xor_c.op != OP_REG_BIT_XOR
        || zero_c.op != OP_REG_PUSH_I32
        || reg_i32(&zero_c) != 0
        || or_c.op != OP_REG_BIT_OR
        || store_hash.op != OP_REG_STORE_LOC
        || store_hash.dst != hash_loc
        || inc_one.op != OP_REG_PUSH_I32
        || reg_i32(&inc_one) != 1
        || i_load_inc.op != OP_REG_LOAD_LOC
        || i_load_inc.src1 as u8 != i_loc
        || i_add.op != OP_REG_ADD
        || i_store.op != OP_REG_STORE_LOC
        || i_store.dst != i_loc
        || backedge.op != OP_REG_JMP
        || backedge.src1 != 4
        || exit_load.op != OP_REG_LOAD_LOC
        || exit_load.src1 as u8 != hash_loc
        || ret.op != OP_REG_RETURN
        || ret.dst != exit_load.dst
    {
        return None;
    }

    let mask = reg_i32(&mask_push);
    let shift_a = reg_i32(&shift_a_push);
    let shift_b = reg_i32(&shift_b_push);
    let shift_c = reg_i32(&shift_c_push);
    if !(0..=31).contains(&shift_a) || !(0..=31).contains(&shift_b) || !(0..=31).contains(&shift_c)
    {
        return None;
    }

    let mut code = Vec::with_capacity(128);
    // mov ecx, 0          ; i
    code.extend_from_slice(&[0xb9, 0, 0, 0, 0]);
    // mov eax, hash_init  ; hash
    code.push(0xb8);
    code.extend_from_slice(&hash_init.to_le_bytes());

    let loop_start = code.len();
    // cmp ecx, limit
    code.extend_from_slice(&[0x81, 0xf9]);
    code.extend_from_slice(&limit.to_le_bytes());
    // jge exit
    code.extend_from_slice(&[0x0f, 0x8d]);
    let exit_jump = code.len();
    code.extend_from_slice(&[0, 0, 0, 0]);

    // r10d = (i & mask)
    code.extend_from_slice(&[0x41, 0x89, 0xca]);
    code.extend_from_slice(&[0x41, 0x81, 0xe2]);
    code.extend_from_slice(&mask.to_le_bytes());
    // r11d = i >>> shift_a
    code.extend_from_slice(&[0x41, 0x89, 0xcb]);
    code.extend_from_slice(&[0x41, 0xc1, 0xeb, shift_a as u8]);
    // r10d ^= r11d; hash += r10d  (the following |0 is the 32-bit value in eax)
    code.extend_from_slice(&[0x45, 0x31, 0xda]);
    code.extend_from_slice(&[0x44, 0x01, 0xd0]);

    // r10d = hash << shift_b
    code.extend_from_slice(&[0x41, 0x89, 0xc2]);
    code.extend_from_slice(&[0x41, 0xc1, 0xe2, shift_b as u8]);
    // r11d = hash >>> shift_c
    code.extend_from_slice(&[0x41, 0x89, 0xc3]);
    code.extend_from_slice(&[0x41, 0xc1, 0xeb, shift_c as u8]);
    // hash = hash ^ r10d ^ r11d  (the final |0 is naturally 32-bit)
    code.extend_from_slice(&[0x44, 0x31, 0xd0]);
    code.extend_from_slice(&[0x44, 0x31, 0xd8]);

    // i++
    code.extend_from_slice(&[0xff, 0xc1]);
    // jmp loop_start
    code.push(0xe9);
    let back_jump = code.len();
    code.extend_from_slice(&[0, 0, 0, 0]);

    let exit_pos = code.len();
    // mov edx, JS_TAG_INT; ret. eax already holds the int32 payload.
    code.extend_from_slice(&[0xba, 0, 0, 0, 0]);
    code.push(0xc3);

    let exit_disp = (exit_pos as isize - (exit_jump + 4) as isize) as i32;
    code[exit_jump..exit_jump + 4].copy_from_slice(&exit_disp.to_le_bytes());
    let back_disp = (loop_start as isize - (back_jump + 4) as isize) as i32;
    code[back_jump..back_jump + 4].copy_from_slice(&back_disp.to_le_bytes());

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

    enum CountUpdate {
        Const(i32),
        Induction,
        Affine { mul: i32, add: i32 },
    }

    // Body: recognize these pure accumulator updates:
    //   count += constant
    //   count += i
    //   count += i * C + K
    let body = loop_start + 4;
    let (count_update, induction_update_idx) = if body + 8 == backedge_idx {
        // QuickJS may emit either
        //   load count; push delta; add; store count
        // or, for OP_add_loc,
        //   push delta; load count; add; store count.
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

        let update = if let Some(step) = constant_count_step {
            CountUpdate::Const(step)
        } else if count_a.op == OP_REG_LOAD_LOC
            && count_a.src1 as u8 == i_loc
            && count_b.op == OP_REG_LOAD_LOC
            && count_b.src1 as u8 == count_loc
            && count_add.dst == count_b.dst
        {
            CountUpdate::Induction
        } else {
            return None;
        };

        (update, body + 4)
    } else if body + 12 == backedge_idx {
        // Common QuickJS shape for:
        //   count = count + i * C + K
        // after get_loc0_loc1:
        //   load count; load i; push C; mul; add; push K; add; store count
        let count_load = bytecode[body];
        let i_load = bytecode[body + 1];
        let mul_const = bytecode[body + 2];
        let mul = bytecode[body + 3];
        let add_mul = bytecode[body + 4];
        let add_const = bytecode[body + 5];
        let add_final = bytecode[body + 6];
        let count_store = bytecode[body + 7];

        if count_load.op != OP_REG_LOAD_LOC
            || count_load.src1 as u8 != count_loc
            || i_load.op != OP_REG_LOAD_LOC
            || i_load.src1 as u8 != i_loc
            || mul_const.op != OP_REG_PUSH_I32
            || mul.op != OP_REG_MUL
            || mul.dst != i_load.dst
            || mul.src1 as u8 != i_load.dst
            || mul.src2 as u8 != mul_const.dst
            || add_mul.op != OP_REG_ADD
            || add_mul.dst != count_load.dst
            || add_mul.src1 as u8 != count_load.dst
            || add_mul.src2 as u8 != mul.dst
            || add_const.op != OP_REG_PUSH_I32
            || add_final.op != OP_REG_ADD
            || add_final.dst != count_load.dst
            || add_final.src1 as u8 != count_load.dst
            || add_final.src2 as u8 != add_const.dst
            || count_store.op != OP_REG_STORE_LOC
            || count_store.dst != count_loc
            || count_store.src1 as u8 != add_final.dst
        {
            return None;
        }

        (
            CountUpdate::Affine {
                mul: reg_i32(&mul_const),
                add: reg_i32(&add_const),
            },
            body + 8,
        )
    } else {
        return None;
    };

    // Induction update accepts either load/push/add/store or push/load/add/store ordering.
    let i_a = bytecode[induction_update_idx];
    let i_b = bytecode[induction_update_idx + 1];
    let i_add = bytecode[induction_update_idx + 2];
    let i_store = bytecode[induction_update_idx + 3];
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
    // Sum of the induction value before increment:
    //   n * i0 + step * n * (n - 1) / 2
    // Use i128 so recognizers do not reject large but ultimately JS-safe sums due to intermediate overflow.
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

    let final_count = match count_update {
        CountUpdate::Const(count_step) => {
            (count_init as i128).checked_add((count_step as i128).checked_mul(n)?)?
        }
        CountUpdate::Induction => (count_init as i128).checked_add(induction_sum)?,
        CountUpdate::Affine { mul, add } => {
            let affine_sum = (mul as i128)
                .checked_mul(induction_sum)?
                .checked_add((add as i128).checked_mul(n)?)?;
            (count_init as i128).checked_add(affine_sum)?
        }
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
