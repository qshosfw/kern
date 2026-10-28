//! KVM-16 Format B Virtual Machine Interpreter (KRX-ABI 2.5.0 Section 5.4).
//!
//! Provides lightweight, sandboxed, fuel-metered bytecode execution for
//! untrusted installable apps on Cortex-M0+ (Class 2).

pub const OP_NOP: u8 = 0;
pub const OP_MOV_IMM: u8 = 1;
pub const OP_MOV_REG: u8 = 2;
pub const OP_ADD: u8 = 3;
pub const OP_SUB: u8 = 4;
pub const OP_AND: u8 = 5;
pub const OP_ORR: u8 = 6;
pub const OP_XOR: u8 = 7;
pub const OP_LSL: u8 = 8;
pub const OP_LSR: u8 = 9;
pub const OP_JMP: u8 = 10;
pub const OP_JZ: u8 = 11;
pub const OP_JNZ: u8 = 12;
pub const OP_CALL: u8 = 13;
pub const OP_RET: u8 = 14;
pub const OP_SYS: u8 = 15;
pub const OP_HALT: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmError {
    InvalidInstruction,
    PcOutOfBounds,
    StackOverflow,
    StackUnderflow,
    DivisionByZero,
    FuelExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmStatus {
    Continue,
    Yield,
    Halted,
    SysCall { iface: u16, op: u16, arg: u16 },
}

/// KVM-16 Register File and Virtual Machine State
#[derive(Debug, Clone)]
pub struct Kvm16 {
    pub regs: [u16; 8],
    pub pc: usize,
    pub flags: u16,
    pub fuel: u32,
    pub stack: [u16; 16],
    pub sp: usize,
    pub is_halted: bool,
}

impl Default for Kvm16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Kvm16 {
    pub const fn new() -> Self {
        Self {
            regs: [0; 8],
            pc: 0,
            flags: 0,
            fuel: 0,
            stack: [0; 16],
            sp: 0,
            is_halted: false,
        }
    }

    pub fn reset(&mut self) {
        self.regs = [0; 8];
        self.pc = 0;
        self.flags = 0;
        self.fuel = 0;
        self.sp = 0;
        self.is_halted = false;
    }

    /// Execute a bounded slice of instructions up to `fuel_quota`.
    pub fn run_slice(&mut self, code: &[u16], fuel_quota: u32) -> Result<VmStatus, VmError> {
        self.fuel = fuel_quota;

        while self.fuel > 0 && !self.is_halted {
            self.fuel -= 1;

            if self.pc >= code.len() {
                return Err(VmError::PcOutOfBounds);
            }

            let insn = code[self.pc];
            self.pc += 1;

            let opcode = ((insn >> 10) & 0x3F) as u8;
            let vd = ((insn >> 7) & 0x07) as usize;
            let vs = ((insn >> 4) & 0x07) as usize;
            let vt = ((insn >> 1) & 0x07) as usize;
            let _flag = insn & 0x01;

            match opcode {
                OP_NOP => {}
                OP_MOV_IMM => {
                    let imm8 = (insn & 0xFF) as u16;
                    self.regs[vd] = imm8;
                }
                OP_MOV_REG => {
                    self.regs[vd] = self.regs[vs];
                }
                OP_ADD => {
                    self.regs[vd] = self.regs[vs].wrapping_add(self.regs[vt]);
                }
                OP_SUB => {
                    self.regs[vd] = self.regs[vs].wrapping_sub(self.regs[vt]);
                }
                OP_AND => {
                    self.regs[vd] = self.regs[vs] & self.regs[vt];
                }
                OP_ORR => {
                    self.regs[vd] = self.regs[vs] | self.regs[vt];
                }
                OP_XOR => {
                    self.regs[vd] = self.regs[vs] ^ self.regs[vt];
                }
                OP_LSL => {
                    let shift = (insn & 0x0F) as u32;
                    self.regs[vd] = self.regs[vs] << shift;
                }
                OP_LSR => {
                    let shift = (insn & 0x0F) as u32;
                    self.regs[vd] = self.regs[vs] >> shift;
                }
                OP_JMP => {
                    let target = (insn & 0x3FF) as usize;
                    if target >= code.len() {
                        return Err(VmError::PcOutOfBounds);
                    }
                    self.pc = target;
                }
                OP_JZ => {
                    if self.regs[vs] == 0 {
                        let target = (insn & 0x3FF) as usize;
                        if target >= code.len() {
                            return Err(VmError::PcOutOfBounds);
                        }
                        self.pc = target;
                    }
                }
                OP_JNZ => {
                    if self.regs[vs] != 0 {
                        let target = (insn & 0x3FF) as usize;
                        if target >= code.len() {
                            return Err(VmError::PcOutOfBounds);
                        }
                        self.pc = target;
                    }
                }
                OP_CALL => {
                    if self.sp >= self.stack.len() {
                        return Err(VmError::StackOverflow);
                    }
                    self.stack[self.sp] = self.pc as u16;
                    self.sp += 1;
                    let target = (insn & 0x3FF) as usize;
                    if target >= code.len() {
                        return Err(VmError::PcOutOfBounds);
                    }
                    self.pc = target;
                }
                OP_RET => {
                    if self.sp == 0 {
                        return Err(VmError::StackUnderflow);
                    }
                    self.sp -= 1;
                    self.pc = self.stack[self.sp] as usize;
                }
                OP_SYS => {
                    let iface = self.regs[vs];
                    let op = self.regs[vt];
                    let arg = self.regs[vd];
                    return Ok(VmStatus::SysCall { iface, op, arg });
                }
                OP_HALT => {
                    self.is_halted = true;
                    return Ok(VmStatus::Halted);
                }
                _ => return Err(VmError::InvalidInstruction),
            }
        }

        if self.is_halted {
            Ok(VmStatus::Halted)
        } else if self.fuel == 0 {
            Ok(VmStatus::Yield)
        } else {
            Ok(VmStatus::Continue)
        }
    }
}
