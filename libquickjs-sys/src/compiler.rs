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
const OP_ADD: u8 = 156;

pub fn compile_bytecode(bytecode: &[u8]) -> Result<Vec<RegInstruction>, &'static str> {
    let mut reg_instructions = Vec::new();
    let mut pc = 0;
    let mut stack_height: usize = 0;

    while pc < bytecode.len() {
        let opcode = bytecode[pc];
        match opcode {
            OP_PUSH_I32 => {
                if pc + 5 > bytecode.len() {
                    return Err("Malformed bytecode: OP_push_i32 operand missing");
                }
                let val = read_i32(bytecode, pc + 1);
                // Store 32-bit value in src1 and src2
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
            OP_ADD => {
                if stack_height < 2 {
                    return Err("Stack underflow on OP_add");
                }
                let src1 = (stack_height - 2) as u16;
                let src2 = (stack_height - 1) as u16;
                stack_height -= 1;
                reg_instructions.push(RegInstruction {
                    op: OP_REG_ADD,
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
            _ => {
                // Encountered an unsupported opcode, abort translation
                return Err("Unsupported opcode for register-based VM compilation");
            }
        }
    }

    Ok(reg_instructions)
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
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
