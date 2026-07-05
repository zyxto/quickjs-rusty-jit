#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct RegInstruction {
    pub op: u8,
    pub dst: u8,
    pub src1: u16,
    pub src2: u16,
}

// Register Opcode Constants
pub const OP_REG_PUSH_I32: u8 = 1;
pub const OP_REG_PUSH_CONST: u8 = 2;
pub const OP_REG_LOAD_LOC: u8 = 3;
pub const OP_REG_STORE_LOC: u8 = 4;
pub const OP_REG_ADD: u8 = 5;
pub const OP_REG_MOV: u8 = 6;
pub const OP_REG_RETURN: u8 = 7;
pub const OP_REG_RETURN_UNDEF: u8 = 8;
pub const OP_REG_UNDEFINED: u8 = 9;
pub const OP_REG_NULL: u8 = 10;
pub const OP_REG_PUSH_FALSE: u8 = 11;
pub const OP_REG_PUSH_TRUE: u8 = 12;
pub const OP_REG_DROP: u8 = 13;
pub const OP_REG_LT: u8 = 14;
pub const OP_REG_LTE: u8 = 15;
pub const OP_REG_GT: u8 = 16;
pub const OP_REG_GTE: u8 = 17;
pub const OP_REG_IF_FALSE: u8 = 18;
pub const OP_REG_IF_TRUE: u8 = 19;
pub const OP_REG_JMP: u8 = 20;
pub const OP_REG_SET_LOC: u8 = 21;
pub const OP_REG_MUL: u8 = 22;
pub const OP_REG_SUB: u8 = 23;
pub const OP_REG_MOD: u8 = 24;
pub const OP_REG_SHL: u8 = 25;
pub const OP_REG_SAR: u8 = 26;
pub const OP_REG_SHR: u8 = 27;
pub const OP_REG_BIT_AND: u8 = 28;
pub const OP_REG_BIT_XOR: u8 = 29;
pub const OP_REG_BIT_OR: u8 = 30;
pub const OP_REG_STRICT_EQ: u8 = 31;

// Stack-based opcode values from QuickJS
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
const OP_SET_LOC: u8 = 89;
const OP_IF_FALSE: u8 = 104;
const OP_DEC_LOC: u8 = 145;
const OP_INC_LOC: u8 = 146;
const OP_ADD_LOC: u8 = 147;
const OP_PUSH_MINUS1: u8 = 185;
const OP_PUSH_0: u8 = 186;
const OP_PUSH_7: u8 = 193;
const OP_PUSH_I8: u8 = 194;
const OP_PUSH_I16: u8 = 195;
const OP_PUSH_CONST8: u8 = 196;
const OP_GET_LOC8: u8 = 199;
const OP_PUT_LOC8: u8 = 200;
const OP_SET_LOC8: u8 = 201;
const OP_GET_LOC0_LOC1: u8 = 202;
const OP_GET_LOC0: u8 = 203;
const OP_GET_LOC3: u8 = 206;
const OP_PUT_LOC0: u8 = 207;
const OP_PUT_LOC3: u8 = 210;
const OP_SET_LOC0: u8 = 211;
const OP_SET_LOC3: u8 = 214;
const OP_IF_TRUE: u8 = 105;
const OP_GOTO: u8 = 106;
const OP_MUL: u8 = 153;
const OP_MOD: u8 = 155;
const OP_ADD: u8 = 156;
const OP_SUB: u8 = 157;
const OP_SHL: u8 = 158;
const OP_SAR: u8 = 159;
const OP_SHR: u8 = 160;
const OP_AND: u8 = 161;
const OP_XOR: u8 = 162;
const OP_OR: u8 = 163;
const OP_LT: u8 = 165;
const OP_LTE: u8 = 166;
const OP_GT: u8 = 167;
const OP_GTE: u8 = 168;
const OP_STRICT_EQ: u8 = 173;
const OP_IF_FALSE8: u8 = 240;
const OP_IF_TRUE8: u8 = 241;
const OP_GOTO8: u8 = 242;
const OP_GOTO16: u8 = 243;

pub fn compile_bytecode(bytecode: &[u8]) -> Result<Vec<RegInstruction>, &'static str> {
    let mut reg_instructions = Vec::new();
    let mut pc = 0;
    let mut stack_height: usize = 0;

    // Track the mapping from stack-based bytecode PC to register-based instruction index
    let mut pc_to_inst_idx = vec![None; bytecode.len() + 1];
    let mut jumps_to_patch = Vec::new(); // stores (inst_idx_in_vector, target_bytecode_pc)

    while pc < bytecode.len() {
        pc_to_inst_idx[pc] = Some(reg_instructions.len());

        let opcode = bytecode[pc];
        match opcode {
            OP_PUSH_I32 => {
                if pc + 5 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_i32 operand missing");
                }
                let val = read_i32(bytecode, pc + 1);
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: stack_height as u8,
                    src1,
                    src2,
                });
                stack_height += 1;
                pc += 5;
            }
            OP_PUSH_MINUS1 => {
                let val = -1i32;
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: stack_height as u8,
                    src1,
                    src2,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_PUSH_0..=OP_PUSH_7 => {
                let val = (opcode - OP_PUSH_0) as i32;
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: stack_height as u8,
                    src1,
                    src2,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_PUSH_I8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_i8 operand missing");
                }
                let val = bytecode[pc + 1] as i8 as i32;
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: stack_height as u8,
                    src1,
                    src2,
                });
                stack_height += 1;
                pc += 2;
            }
            OP_PUSH_I16 => {
                if pc + 3 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_i16 operand missing");
                }
                let val = read_i16(bytecode, pc + 1) as i32;
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: stack_height as u8,
                    src1,
                    src2,
                });
                stack_height += 1;
                pc += 3;
            }
            OP_PUSH_CONST => {
                if pc + 5 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_const operand missing");
                }
                let idx = read_u32(bytecode, pc + 1);
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_CONST,
                    dst: stack_height as u8,
                    src1: idx as u16,
                    src2: 0,
                });
                stack_height += 1;
                pc += 5;
            }
            OP_PUSH_CONST8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_const8 operand missing");
                }
                let idx = bytecode[pc + 1] as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_CONST,
                    dst: stack_height as u8,
                    src1: idx,
                    src2: 0,
                });
                stack_height += 1;
                pc += 2;
            }
            OP_UNDEFINED => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_UNDEFINED,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_NULL => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_NULL,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_PUSH_FALSE => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_FALSE,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_PUSH_TRUE => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_TRUE,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_DROP => {
                if stack_height == 0 {
                    return Err("Stack underflow on OP_drop");
                }
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_DROP,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                pc += 1;
            }
            OP_DUP => {
                if stack_height == 0 {
                    return Err("Stack underflow on OP_dup");
                }
                reg_instructions.push(RegInstruction {
                    op: OP_REG_MOV,
                    dst: stack_height as u8,
                    src1: (stack_height - 1) as u16,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_GET_LOC0_LOC1 => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                stack_height += 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: stack_height as u8,
                    src1: 1,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_GET_LOC0..=OP_GET_LOC3 => {
                let idx = (opcode - OP_GET_LOC0) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: stack_height as u8,
                    src1: idx,
                    src2: 0,
                });
                stack_height += 1;
                pc += 1;
            }
            OP_GET_LOC8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed bytecode: OP_get_loc8 operand missing");
                }
                let idx = bytecode[pc + 1] as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: stack_height as u8,
                    src1: idx,
                    src2: 0,
                });
                stack_height += 1;
                pc += 2;
            }
            OP_GET_LOC => {
                if pc + 3 > bytecode.len() {
                    return Err("Malformed bytecode: OP_get_loc operand missing");
                }
                let idx = read_u16(bytecode, pc + 1);
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: stack_height as u8,
                    src1: idx,
                    src2: 0,
                });
                stack_height += 1;
                pc += 3;
            }
            OP_PUT_LOC0..=OP_PUT_LOC3 => {
                if stack_height == 0 {
                    return Err("Stack underflow on OP_put_loc_opt");
                }
                let idx = (opcode - OP_PUT_LOC0) as u16;
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_STORE_LOC,
                    dst: idx as u8,
                    src1: stack_height as u16,
                    src2: 0,
                });
                pc += 1;
            }
            OP_PUT_LOC8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed bytecode: OP_put_loc8 operand missing");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on OP_put_loc8");
                }
                let idx = bytecode[pc + 1] as u16;
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_STORE_LOC,
                    dst: idx as u8,
                    src1: stack_height as u16,
                    src2: 0,
                });
                pc += 2;
            }
            OP_PUT_LOC => {
                if pc + 3 > bytecode.len() {
                    return Err("Malformed bytecode: OP_put_loc operand missing");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on OP_put_loc");
                }
                let idx = read_u16(bytecode, pc + 1);
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_STORE_LOC,
                    dst: idx as u8,
                    src1: stack_height as u16,
                    src2: 0,
                });
                pc += 3;
            }
            OP_SET_LOC0..=OP_SET_LOC3 => {
                if stack_height == 0 {
                    return Err("Stack underflow on OP_set_loc_opt");
                }
                let idx = (opcode - OP_SET_LOC0) as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_SET_LOC,
                    dst: idx as u8,
                    src1: (stack_height - 1) as u16,
                    src2: 0,
                });
                pc += 1;
            }
            OP_SET_LOC8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed bytecode: OP_set_loc8 operand missing");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on OP_set_loc8");
                }
                let idx = bytecode[pc + 1] as u16;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_SET_LOC,
                    dst: idx as u8,
                    src1: (stack_height - 1) as u16,
                    src2: 0,
                });
                pc += 2;
            }
            OP_SET_LOC => {
                if pc + 3 > bytecode.len() {
                    return Err("Malformed bytecode: OP_set_loc operand missing");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on OP_set_loc");
                }
                let idx = read_u16(bytecode, pc + 1);
                reg_instructions.push(RegInstruction {
                    op: OP_REG_SET_LOC,
                    dst: idx as u8,
                    src1: (stack_height - 1) as u16,
                    src2: 0,
                });
                pc += 3;
            }
            OP_ADD_LOC => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed OP_add_loc");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on OP_add_loc");
                }
                let idx = bytecode[pc + 1] as u16;
                let temp_reg = stack_height as u8;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: temp_reg,
                    src1: idx,
                    src2: 0,
                });
                reg_instructions.push(RegInstruction {
                    op: OP_REG_ADD,
                    dst: temp_reg,
                    src1: temp_reg as u16,
                    src2: (stack_height - 1) as u16,
                });
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_STORE_LOC,
                    dst: idx as u8,
                    src1: temp_reg as u16,
                    src2: 0,
                });
                pc += 2;
            }
            OP_INC_LOC | OP_DEC_LOC => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed OP_inc_dec_loc");
                }
                let idx = bytecode[pc + 1] as u16;
                let val = if opcode == OP_INC_LOC { 1 } else { -1 };
                let src1 = (val & 0xFFFF) as u16;
                let src2 = ((val >> 16) & 0xFFFF) as u16;
                let temp_reg = stack_height as u8;
                let temp_reg2 = (stack_height + 1) as u8;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_PUSH_I32,
                    dst: temp_reg,
                    src1,
                    src2,
                });
                reg_instructions.push(RegInstruction {
                    op: OP_REG_LOAD_LOC,
                    dst: temp_reg2,
                    src1: idx,
                    src2: 0,
                });
                reg_instructions.push(RegInstruction {
                    op: OP_REG_ADD,
                    dst: temp_reg2,
                    src1: temp_reg2 as u16,
                    src2: temp_reg as u16,
                });
                reg_instructions.push(RegInstruction {
                    op: OP_REG_STORE_LOC,
                    dst: idx as u8,
                    src1: temp_reg2 as u16,
                    src2: 0,
                });
                pc += 2;
            }
            OP_MUL | OP_MOD | OP_ADD | OP_SUB | OP_SHL | OP_SAR | OP_SHR | OP_AND | OP_XOR
            | OP_OR | OP_STRICT_EQ => {
                if stack_height < 2 {
                    return Err("Stack underflow on binary arithmetic");
                }
                let src1 = (stack_height - 2) as u16;
                let src2 = (stack_height - 1) as u16;
                stack_height -= 1;
                let reg_op = match opcode {
                    OP_MUL => OP_REG_MUL,
                    OP_MOD => OP_REG_MOD,
                    OP_ADD => OP_REG_ADD,
                    OP_SUB => OP_REG_SUB,
                    OP_SHL => OP_REG_SHL,
                    OP_SAR => OP_REG_SAR,
                    OP_SHR => OP_REG_SHR,
                    OP_AND => OP_REG_BIT_AND,
                    OP_XOR => OP_REG_BIT_XOR,
                    OP_OR => OP_REG_BIT_OR,
                    _ => OP_REG_STRICT_EQ,
                };
                reg_instructions.push(RegInstruction {
                    op: reg_op,
                    dst: (stack_height - 1) as u8,
                    src1,
                    src2,
                });
                pc += 1;
            }
            OP_LT | OP_LTE | OP_GT | OP_GTE => {
                if stack_height < 2 {
                    return Err("Stack underflow on comparison");
                }
                let src1 = (stack_height - 2) as u16;
                let src2 = (stack_height - 1) as u16;
                stack_height -= 1;
                let reg_op = match opcode {
                    OP_LT => OP_REG_LT,
                    OP_LTE => OP_REG_LTE,
                    OP_GT => OP_REG_GT,
                    _ => OP_REG_GTE,
                };
                reg_instructions.push(RegInstruction {
                    op: reg_op,
                    dst: (stack_height - 1) as u8,
                    src1,
                    src2,
                });
                pc += 1;
            }
            OP_RETURN => {
                if stack_height == 0 {
                    return Err("Stack underflow on OP_return");
                }
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_RETURN,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                pc += 1;
            }
            OP_RETURN_UNDEF => {
                reg_instructions.push(RegInstruction {
                    op: OP_REG_RETURN_UNDEF,
                    dst: 0,
                    src1: 0,
                    src2: 0,
                });
                pc += 1;
            }
            OP_GOTO => {
                if pc + 5 > bytecode.len() {
                    return Err("Malformed OP_goto");
                }
                let offset = read_i32(bytecode, pc + 1);
                let target_pc = (pc as isize + 1 + offset as isize) as usize;
                jumps_to_patch.push((reg_instructions.len(), target_pc));
                reg_instructions.push(RegInstruction {
                    op: OP_REG_JMP,
                    dst: 0,
                    src1: 0,
                    src2: 0,
                });
                pc += 5;
            }
            OP_GOTO8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed OP_goto8");
                }
                let offset = bytecode[pc + 1] as i8 as i32;
                let target_pc = (pc as isize + 1 + offset as isize) as usize;
                jumps_to_patch.push((reg_instructions.len(), target_pc));
                reg_instructions.push(RegInstruction {
                    op: OP_REG_JMP,
                    dst: 0,
                    src1: 0,
                    src2: 0,
                });
                pc += 2;
            }
            OP_GOTO16 => {
                if pc + 3 > bytecode.len() {
                    return Err("Malformed OP_goto16");
                }
                let offset = read_i16(bytecode, pc + 1) as i32;
                let target_pc = (pc as isize + 1 + offset as isize) as usize;
                jumps_to_patch.push((reg_instructions.len(), target_pc));
                reg_instructions.push(RegInstruction {
                    op: OP_REG_JMP,
                    dst: 0,
                    src1: 0,
                    src2: 0,
                });
                pc += 3;
            }
            OP_IF_FALSE | OP_IF_TRUE => {
                if pc + 5 > bytecode.len() {
                    return Err("Malformed conditional jump");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on conditional jump");
                }
                let offset = read_i32(bytecode, pc + 1);
                let target_pc = (pc as isize + 1 + offset as isize) as usize;
                stack_height -= 1;
                let reg_op = if opcode == OP_IF_FALSE {
                    OP_REG_IF_FALSE
                } else {
                    OP_REG_IF_TRUE
                };
                jumps_to_patch.push((reg_instructions.len(), target_pc));
                reg_instructions.push(RegInstruction {
                    op: reg_op,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                pc += 5;
            }
            OP_IF_FALSE8 | OP_IF_TRUE8 => {
                if pc + 2 > bytecode.len() {
                    return Err("Malformed conditional jump8");
                }
                if stack_height == 0 {
                    return Err("Stack underflow on conditional jump8");
                }
                let offset = bytecode[pc + 1] as i8 as i32;
                let target_pc = (pc as isize + 1 + offset as isize) as usize;
                stack_height -= 1;
                let reg_op = if opcode == OP_IF_FALSE8 {
                    OP_REG_IF_FALSE
                } else {
                    OP_REG_IF_TRUE
                };
                jumps_to_patch.push((reg_instructions.len(), target_pc));
                reg_instructions.push(RegInstruction {
                    op: reg_op,
                    dst: stack_height as u8,
                    src1: 0,
                    src2: 0,
                });
                pc += 2;
            }
            _ => {
                return Err("Unsupported opcode for register-based VM compilation");
            }
        }
    }

    pc_to_inst_idx[pc] = Some(reg_instructions.len());

    // Second Pass: Resolve target PCs to instruction offsets
    for (inst_idx, target_pc) in jumps_to_patch {
        if target_pc >= pc_to_inst_idx.len() {
            reg_instructions[inst_idx].src1 = reg_instructions.len() as u16;
        } else if let Some(resolved_idx) = pc_to_inst_idx[target_pc] {
            reg_instructions[inst_idx].src1 = resolved_idx as u16;
        } else {
            return Err("Jump target points to invalid instruction boundary");
        }
    }

    Ok(reg_instructions)
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn read_i16(buf: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn read_i32(buf: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}
