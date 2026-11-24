//! Complete ARMv6-M (Cortex-M0+) CPU core for `kern-emu`.
//!
//! Implements the full ARMv6-M Thumb instruction set (all 16-bit Thumb-1 instructions +
//! 32-bit `BL`, `MRS`, `MSR`, `DSB`, `DMB`, `ISB`), NVIC exception entry/tail-return,
//! `MSP`/`PSP` stack selection, `VTOR`, `PRIMASK`, and `HardFault` vectoring.

use crate::soc::Py32Soc;

const FLAG_N: u32 = 1 << 31;
const FLAG_Z: u32 = 1 << 30;
const FLAG_C: u32 = 1 << 29;
const FLAG_V: u32 = 1 << 28;
const FLAG_T: u32 = 1 << 24;

pub struct CortexM0Plus {
    pub r: [u32; 13],
    pub msp: u32,
    pub psp: u32,
    pub lr: u32,
    pub pc: u32,
    pub xpsr: u32,
    pub primask: u32,
    pub control: u32,
    pub halted_wfi: bool,
    pub total_insns: u64,
}

impl CortexM0Plus {
    pub fn new(soc: &mut Py32Soc) -> Self {
        let mut cpu = Self {
            r: [0; 13],
            msp: 0,
            psp: 0,
            lr: 0xFFFF_FFFF,
            pc: 0,
            xpsr: FLAG_T,
            primask: 0,
            control: 0,
            halted_wfi: false,
            total_insns: 0,
        };
        cpu.reset(soc);
        cpu
    }

    pub fn reset(&mut self, soc: &mut Py32Soc) {
        let vtor = soc.boot_vector_base;
        soc.scb_vtor = vtor;
        self.r = [0; 13];
        self.msp = soc.read_u32(vtor) & !3;
        self.psp = 0;
        self.lr = 0xFFFF_FFFF;
        self.pc = soc.read_u32(vtor + 4) & !1;
        self.xpsr = FLAG_T;
        self.primask = 0;
        self.control = 0;
        self.halted_wfi = false;
    }

    #[inline(always)]
    fn ipsr(&self) -> u32 {
        self.xpsr & 0x3F
    }

    #[inline(always)]
    fn use_psp(&self) -> bool {
        self.ipsr() == 0 && (self.control & 2) != 0
    }

    #[inline(always)]
    fn sp(&self) -> u32 {
        if self.use_psp() { self.psp } else { self.msp }
    }

    #[inline(always)]
    fn set_sp(&mut self, val: u32) {
        let aligned = val & !3;
        if self.use_psp() {
            self.psp = aligned;
        } else {
            self.msp = aligned;
        }
    }

    #[inline(always)]
    fn reg(&self, idx: usize, inst_pc: u32) -> u32 {
        match idx {
            0..=12 => self.r[idx],
            13 => self.sp(),
            14 => self.lr,
            15 => (inst_pc.wrapping_add(4)) & !1,
            _ => 0,
        }
    }

    #[inline(always)]
    fn set_reg(&mut self, idx: usize, val: u32) {
        match idx {
            0..=12 => self.r[idx] = val,
            13 => self.set_sp(val),
            14 => self.lr = val,
            15 => self.pc = val & !1,
            _ => {}
        }
    }

    #[inline(always)]
    fn set_nz(&mut self, res: u32) {
        self.xpsr &= !(FLAG_N | FLAG_Z);
        if (res & 0x8000_0000) != 0 {
            self.xpsr |= FLAG_N;
        }
        if res == 0 {
            self.xpsr |= FLAG_Z;
        }
    }

    #[inline(always)]
    fn set_nzc(&mut self, res: u32, carry: bool) {
        self.xpsr &= !(FLAG_N | FLAG_Z | FLAG_C);
        if (res & 0x8000_0000) != 0 {
            self.xpsr |= FLAG_N;
        }
        if res == 0 {
            self.xpsr |= FLAG_Z;
        }
        if carry {
            self.xpsr |= FLAG_C;
        }
    }

    #[inline(always)]
    fn add_with_carry(&mut self, a: u32, b: u32, cin: bool, update_flags: bool) -> u32 {
        let c = u64::from(cin);
        let usum = (a as u64) + (b as u64) + c;
        let res = usum as u32;
        if update_flags {
            let carry = usum > 0xFFFF_FFFF;
            let overflow = ((!(a ^ b) & (a ^ res)) & 0x8000_0000) != 0;
            self.xpsr &= !(FLAG_N | FLAG_Z | FLAG_C | FLAG_V);
            if (res & 0x8000_0000) != 0 {
                self.xpsr |= FLAG_N;
            }
            if res == 0 {
                self.xpsr |= FLAG_Z;
            }
            if carry {
                self.xpsr |= FLAG_C;
            }
            if overflow {
                self.xpsr |= FLAG_V;
            }
        }
        res
    }

    #[inline(always)]
    fn cond_passed(&self, cond: u16) -> bool {
        let n = (self.xpsr & FLAG_N) != 0;
        let z = (self.xpsr & FLAG_Z) != 0;
        let c = (self.xpsr & FLAG_C) != 0;
        let v = (self.xpsr & FLAG_V) != 0;
        match cond & 0xF {
            0x0 => z,
            0x1 => !z,
            0x2 => c,
            0x3 => !c,
            0x4 => n,
            0x5 => !n,
            0x6 => v,
            0x7 => !v,
            0x8 => c && !z,
            0x9 => !c || z,
            0xA => n == v,
            0xB => n != v,
            0xC => !z && (n == v),
            0xD => z || (n != v),
            0xE => true,
            _ => false,
        }
    }

    fn branch_write_pc(&mut self, soc: &mut Py32Soc, target: u32) {
        if (target & 0xF000_0000) == 0xF000_0000 && self.ipsr() != 0 {
            self.exception_return(soc, target);
            return;
        }
        if (target & 1) == 0 {
            self.enter_exception(soc, 3); // HardFault on ARM-state branch
            return;
        }
        self.pc = target & !1;
    }

    pub fn enter_exception(&mut self, soc: &mut Py32Soc, exc_num: u32) {
        self.halted_wfi = false;
        let from_handler = self.ipsr() != 0;
        let frame_use_psp = !from_handler && (self.control & 2) != 0;
        let sp_before = if frame_use_psp { self.psp } else { self.msp };
        let frame_ptr = sp_before.wrapping_sub(32) & !3;
        if frame_use_psp {
            self.psp = frame_ptr;
        } else {
            self.msp = frame_ptr;
        }

        let ret_xpsr = self.xpsr | FLAG_T;
        let words = [
            self.r[0],
            self.r[1],
            self.r[2],
            self.r[3],
            self.r[12],
            self.lr,
            self.pc & !1,
            ret_xpsr,
        ];
        for (i, &w) in words.iter().enumerate() {
            soc.write_u32(frame_ptr.wrapping_add((i * 4) as u32), w);
        }

        self.lr = if from_handler {
            0xFFFF_FFF1
        } else if frame_use_psp {
            0xFFFF_FFFD
        } else {
            0xFFFF_FFF9
        };
        self.xpsr = (self.xpsr & !0x3F) | (exc_num & 0x3F) | FLAG_T;
        let handler = soc.read_u32(soc.scb_vtor.wrapping_add(exc_num * 4));
        self.pc = handler & !1;
    }

    fn exception_return(&mut self, soc: &mut Py32Soc, exc_return: u32) {
        let (to_handler, use_psp) = match exc_return & 0xF {
            0x1 => (true, false),
            0x9 => (false, false),
            0xD => (false, true),
            _ => (false, false),
        };
        let frame_ptr = if use_psp { self.psp } else { self.msp };
        self.r[0] = soc.read_u32(frame_ptr);
        self.r[1] = soc.read_u32(frame_ptr.wrapping_add(4));
        self.r[2] = soc.read_u32(frame_ptr.wrapping_add(8));
        self.r[3] = soc.read_u32(frame_ptr.wrapping_add(12));
        self.r[12] = soc.read_u32(frame_ptr.wrapping_add(16));
        self.lr = soc.read_u32(frame_ptr.wrapping_add(20));
        let ret_pc = soc.read_u32(frame_ptr.wrapping_add(24));
        let saved_xpsr = soc.read_u32(frame_ptr.wrapping_add(28));

        let new_sp = frame_ptr.wrapping_add(32);
        if use_psp {
            self.psp = new_sp;
            self.control |= 2;
        } else {
            self.msp = new_sp;
            if !to_handler {
                self.control &= !2;
            }
        }
        self.xpsr = if to_handler {
            saved_xpsr | FLAG_T
        } else {
            (saved_xpsr & !0x3F) | FLAG_T
        };
        self.pc = ret_pc & !1;
    }

    #[inline]
    fn check_pending_interrupts(&mut self, soc: &mut Py32Soc) {
        let active_exc = self.ipsr();
        // HardFault (3) cannot be preempted by configurable IRQs
        if active_exc == 3 {
            return;
        }
        if (self.primask & 1) != 0 {
            // Even with PRIMASK=1, a newly pended enabled IRQ wakes WFI without entering handler!
            if self.halted_wfi
                && (soc.systick_pending || soc.pendsv_pending || (soc.nvic_ispr & soc.nvic_iser) != 0)
            {
                self.halted_wfi = false;
            }
            return;
        }
        // Only enter configurable exceptions from Thread mode (active_exc == 0) for simplicity & speed
        if active_exc != 0 {
            return;
        }
        if soc.systick_pending {
            soc.systick_pending = false;
            self.enter_exception(soc, 15);
            return;
        }
        if soc.pendsv_pending {
            soc.pendsv_pending = false;
            self.enter_exception(soc, 14);
            return;
        }
        let active_irqs = soc.nvic_ispr & soc.nvic_iser;
        if active_irqs != 0 {
            let irq = active_irqs.trailing_zeros();
            soc.nvic_ispr &= !(1 << irq);
            self.enter_exception(soc, 16 + irq);
        }
    }

    pub fn step(&mut self, soc: &mut Py32Soc, turbo: bool) {
        if soc.warm_reset_requested {
            soc.warm_reset_peripherals();
            self.reset(soc);
        }

        self.check_pending_interrupts(soc);
        if self.halted_wfi {
            soc.fast_forward_wfi();
            self.check_pending_interrupts(soc);
            return;
        }

        let inst_pc = self.pc;
        let hw1 = soc.read_u16(inst_pc);
        self.total_insns = self.total_insns.wrapping_add(1);

        // 32-bit Thumb-2 subset on ARMv6-M: upper 5 bits in {0b11101, 0b11110, 0b11111}
        if (hw1 & 0xF800) >= 0xE800 {
            let hw2 = soc.read_u16(inst_pc.wrapping_add(2));
            self.pc = inst_pc.wrapping_add(4);
            self.exec_32(soc, inst_pc, hw1, hw2);
            soc.step_cycles(2);
            return;
        }

        self.pc = inst_pc.wrapping_add(2);
        self.exec_16(soc, inst_pc, hw1, turbo);
        soc.step_cycles(1);
    }

    fn exec_32(&mut self, soc: &mut Py32Soc, inst_pc: u32, hw1: u16, hw2: u16) {
        // BL <label>: hw1 = 11110 S imm10, hw2 = 11 J1 1 J2 imm11
        if (hw1 & 0xF800) == 0xF000 && (hw2 & 0xD000) == 0xD000 {
            let s = ((hw1 >> 10) & 1) as u32;
            let j1 = ((hw2 >> 13) & 1) as u32;
            let j2 = ((hw2 >> 11) & 1) as u32;
            let i1 = (!(j1 ^ s)) & 1;
            let i2 = (!(j2 ^ s)) & 1;
            let imm10 = (hw1 & 0x03FF) as u32;
            let imm11 = (hw2 & 0x07FF) as u32;
            let mut off = (s << 24) | (i1 << 23) | (i2 << 22) | (imm10 << 12) | (imm11 << 1);
            if s != 0 {
                off |= 0xFE00_0000;
            }
            self.lr = inst_pc.wrapping_add(4) | 1;
            self.pc = inst_pc.wrapping_add(4).wrapping_add(off) & !1;
            return;
        }

        // MRS Rd, spec_reg: hw1 = 0xF3EF, hw2 = 1000 Rd[11:8] SYSm[7:0]
        if hw1 == 0xF3EF && (hw2 & 0xF000) == 0x8000 {
            let rd = ((hw2 >> 8) & 0xF) as usize;
            let sysm = (hw2 & 0xFF) as u8;
            let val = match sysm {
                0 => self.xpsr & 0xF000_003F,
                1 => self.ipsr(),
                2 => 0,
                3 => self.xpsr & 0xF000_0000,
                5 => self.ipsr(),
                8 => self.msp,
                9 => self.psp,
                16 => self.primask & 1,
                20 => self.control & 3,
                _ => 0,
            };
            self.set_reg(rd, val);
            return;
        }

        // MSR spec_reg, Rn: hw1 = 0xF380 | Rn, hw2 = 1000 1000 SYSm[7:0]
        if (hw1 & 0xFFF0) == 0xF380 && (hw2 & 0xFF00) == 0x8800 {
            let rn = (hw1 & 0xF) as usize;
            let val = self.reg(rn, inst_pc);
            let sysm = (hw2 & 0xFF) as u8;
            match sysm {
                0..=3 => {
                    if (sysm & 1) == 0 {
                        self.xpsr = (self.xpsr & !0xF000_0000) | (val & 0xF000_0000);
                    }
                }
                8 => self.msp = val & !3,
                9 => self.psp = val & !3,
                16 => self.primask = val & 1,
                20 => {
                    if self.ipsr() == 0 {
                        self.control = val & 3;
                    }
                }
                _ => {}
            }
            return;
        }

        // DSB / DMB / ISB barriers: 0xF3BF 0x8F4x / 0x8F5x / 0x8F6x -> NOP in single-core order
        if hw1 == 0xF3BF && (hw2 & 0xFF00) == 0x8F00 {
            return;
        }

        // Undefined 32-bit instruction (e.g. intentional HardFault test) -> HardFault
        self.enter_exception(soc, 3);
    }

    fn exec_16(&mut self, soc: &mut Py32Soc, inst_pc: u32, op: u16, turbo: bool) {
        match op >> 11 {
            // 00000: LSLS Rd, Rm, #imm5
            0b00000 => {
                let imm5 = ((op >> 6) & 0x1F) as u32;
                let rm = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let v = self.r[rm];
                if imm5 == 0 {
                    self.r[rd] = v;
                    self.set_nz(v);
                } else {
                    let carry = ((v >> (32 - imm5)) & 1) != 0;
                    let res = v << imm5;
                    self.r[rd] = res;
                    self.set_nzc(res, carry);
                }
            }
            // 00001: LSRS Rd, Rm, #imm5
            0b00001 => {
                let imm5 = ((op >> 6) & 0x1F) as u32;
                let rm = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let v = self.r[rm];
                let (res, carry) = if imm5 == 0 {
                    (0, (v & 0x8000_0000) != 0)
                } else {
                    (v >> imm5, ((v >> (imm5 - 1)) & 1) != 0)
                };
                self.r[rd] = res;
                self.set_nzc(res, carry);
            }
            // 00010: ASRS Rd, Rm, #imm5
            0b00010 => {
                let imm5 = ((op >> 6) & 0x1F) as u32;
                let rm = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let v = self.r[rm] as i32;
                let (res, carry) = if imm5 == 0 {
                    let neg = v < 0;
                    (if neg { 0xFFFF_FFFF } else { 0 }, neg)
                } else {
                    ((v >> imm5) as u32, (((v as u32) >> (imm5 - 1)) & 1) != 0)
                };
                self.r[rd] = res;
                self.set_nzc(res, carry);
            }
            // 00011: ADDS/SUBS 3-reg or imm3
            0b00011 => {
                let rn = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let a = self.r[rn];
                let b = if (op & (1 << 10)) != 0 {
                    ((op >> 6) & 7) as u32
                } else {
                    self.r[((op >> 6) & 7) as usize]
                };
                let is_sub = (op & (1 << 9)) != 0;
                self.r[rd] = if is_sub {
                    self.add_with_carry(a, !b, true, true)
                } else {
                    self.add_with_carry(a, b, false, true)
                };
            }
            // 00100: MOVS Rd, #imm8
            0b00100 => {
                let rd = ((op >> 8) & 7) as usize;
                let imm = (op & 0xFF) as u32;
                self.r[rd] = imm;
                self.set_nz(imm);
            }
            // 00101: CMP Rn, #imm8
            0b00101 => {
                let rn = ((op >> 8) & 7) as usize;
                let imm = (op & 0xFF) as u32;
                let _ = self.add_with_carry(self.r[rn], !imm, true, true);
            }
            // 00110: ADDS Rdn, #imm8
            0b00110 => {
                let rdn = ((op >> 8) & 7) as usize;
                let imm = (op & 0xFF) as u32;
                self.r[rdn] = self.add_with_carry(self.r[rdn], imm, false, true);
            }
            // 00111: SUBS Rdn, #imm8
            0b00111 => {
                let rdn = ((op >> 8) & 7) as usize;
                let imm = (op & 0xFF) as u32;
                self.r[rdn] = self.add_with_carry(self.r[rdn], !imm, true, true);
            }
            // 01000: Data-processing (010000) & Special data / BX / BLX (010001)
            0b01000 => {
                if (op & 0x0400) == 0 {
                    self.exec_alu_dp(op);
                } else {
                    self.exec_special_data(soc, inst_pc, op);
                }
            }
            // 01001: LDR Rt, [PC, #imm8*4]
            0b01001 => {
                let rt = ((op >> 8) & 7) as usize;
                let off = ((op & 0xFF) as u32) << 2;
                let base = inst_pc.wrapping_add(4) & !3;
                self.r[rt] = soc.read_u32(base.wrapping_add(off));
            }
            // 0101x: Load/Store register offset
            0b01010 | 0b01011 => {
                let rm = ((op >> 6) & 7) as usize;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                let addr = self.r[rn].wrapping_add(self.r[rm]);
                match (op >> 9) & 7 {
                    0b000 => soc.write_u32(addr, self.r[rt]),
                    0b001 => soc.write_u16(addr, self.r[rt] as u16),
                    0b010 => soc.write_u8(addr, self.r[rt] as u8),
                    0b011 => self.r[rt] = (soc.read_u8(addr) as i8 as i32) as u32,
                    0b100 => self.r[rt] = soc.read_u32(addr),
                    0b101 => self.r[rt] = soc.read_u16(addr) as u32,
                    0b110 => self.r[rt] = soc.read_u8(addr) as u32,
                    0b111 => self.r[rt] = (soc.read_u16(addr) as i16 as i32) as u32,
                    _ => {}
                }
            }
            // 01100: STR Rt, [Rn, #imm5*4]
            0b01100 => {
                let imm = (((op >> 6) & 0x1F) as u32) << 2;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                soc.write_u32(self.r[rn].wrapping_add(imm), self.r[rt]);
            }
            // 01101: LDR Rt, [Rn, #imm5*4]
            0b01101 => {
                let imm = (((op >> 6) & 0x1F) as u32) << 2;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                self.r[rt] = soc.read_u32(self.r[rn].wrapping_add(imm));
            }
            // 01110: STRB Rt, [Rn, #imm5]
            0b01110 => {
                let imm = ((op >> 6) & 0x1F) as u32;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                soc.write_u8(self.r[rn].wrapping_add(imm), self.r[rt] as u8);
            }
            // 01111: LDRB Rt, [Rn, #imm5]
            0b01111 => {
                let imm = ((op >> 6) & 0x1F) as u32;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                self.r[rt] = soc.read_u8(self.r[rn].wrapping_add(imm)) as u32;
            }
            // 10000: STRH Rt, [Rn, #imm5*2]
            0b10000 => {
                let imm = (((op >> 6) & 0x1F) as u32) << 1;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                soc.write_u16(self.r[rn].wrapping_add(imm), self.r[rt] as u16);
            }
            // 10001: LDRH Rt, [Rn, #imm5*2]
            0b10001 => {
                let imm = (((op >> 6) & 0x1F) as u32) << 1;
                let rn = ((op >> 3) & 7) as usize;
                let rt = (op & 7) as usize;
                self.r[rt] = soc.read_u16(self.r[rn].wrapping_add(imm)) as u32;
            }
            // 10010: STR Rt, [SP, #imm8*4]
            0b10010 => {
                let rt = ((op >> 8) & 7) as usize;
                let imm = ((op & 0xFF) as u32) << 2;
                soc.write_u32(self.sp().wrapping_add(imm), self.r[rt]);
            }
            // 10011: LDR Rt, [SP, #imm8*4]
            0b10011 => {
                let rt = ((op >> 8) & 7) as usize;
                let imm = ((op & 0xFF) as u32) << 2;
                self.r[rt] = soc.read_u32(self.sp().wrapping_add(imm));
            }
            // 10100: ADR Rd, label (ADD Rd, PC, #imm8*4)
            0b10100 => {
                let rd = ((op >> 8) & 7) as usize;
                let imm = ((op & 0xFF) as u32) << 2;
                let base = inst_pc.wrapping_add(4) & !3;
                self.r[rd] = base.wrapping_add(imm);
            }
            // 10101: ADD Rd, SP, #imm8*4
            0b10101 => {
                let rd = ((op >> 8) & 7) as usize;
                let imm = ((op & 0xFF) as u32) << 2;
                self.r[rd] = self.sp().wrapping_add(imm);
            }
            // 1011x: Miscellaneous 16-bit instructions
            0b10110 | 0b10111 => self.exec_misc_16(soc, op, turbo),
            // 11000: STMIA Rn!, {reglist}
            0b11000 => {
                let rn = ((op >> 8) & 7) as usize;
                let list = op & 0xFF;
                let mut addr = self.r[rn];
                for i in 0..8usize {
                    if (list & (1 << i)) != 0 {
                        soc.write_u32(addr, self.r[i]);
                        addr = addr.wrapping_add(4);
                    }
                }
                self.r[rn] = addr;
            }
            // 11001: LDMIA Rn!, {reglist}
            0b11001 => {
                let rn = ((op >> 8) & 7) as usize;
                let list = op & 0xFF;
                let mut addr = self.r[rn];
                for i in 0..8usize {
                    if (list & (1 << i)) != 0 {
                        self.r[i] = soc.read_u32(addr);
                        addr = addr.wrapping_add(4);
                    }
                }
                if (list & (1 << rn)) == 0 {
                    self.r[rn] = addr;
                }
            }
            // 1101x: Conditional branch B<c> / UDF / SVC
            0b11010 | 0b11011 => {
                let cond = (op >> 8) & 0xF;
                if cond == 0xE {
                    // UDF #imm8 -> HardFault (used by `panic_test(hardfault=true)`!)
                    self.enter_exception(soc, 3);
                } else if cond == 0xF {
                    // SVC #imm8 -> SVCall (11)
                    self.enter_exception(soc, 11);
                } else if self.cond_passed(cond) {
                    let off = ((op as i8 as i32) << 1) as u32;
                    self.pc = inst_pc.wrapping_add(4).wrapping_add(off) & !1;
                }
            }
            // 11100: Unconditional B <imm11>
            0b11100 => {
                let raw = (op & 0x07FF) as i32;
                let sext = (raw << 21) >> 20;
                self.pc = inst_pc.wrapping_add(4).wrapping_add(sext as u32) & !1;
            }
            _ => self.enter_exception(soc, 3),
        }
    }

    fn exec_alu_dp(&mut self, op: u16) {
        let sub = (op >> 6) & 0xF;
        let rm = ((op >> 3) & 7) as usize;
        let rdn = (op & 7) as usize;
        let a = self.r[rdn];
        let b = self.r[rm];
        match sub {
            0x0 => {
                let r = a & b;
                self.r[rdn] = r;
                self.set_nz(r);
            }
            0x1 => {
                let r = a ^ b;
                self.r[rdn] = r;
                self.set_nz(r);
            }
            0x2 => {
                let s = b & 0xFF;
                if s == 0 {
                    self.set_nz(a);
                } else if s < 32 {
                    let c = ((a >> (32 - s)) & 1) != 0;
                    let r = a << s;
                    self.r[rdn] = r;
                    self.set_nzc(r, c);
                } else if s == 32 {
                    self.r[rdn] = 0;
                    self.set_nzc(0, (a & 1) != 0);
                } else {
                    self.r[rdn] = 0;
                    self.set_nzc(0, false);
                }
            }
            0x3 => {
                let s = b & 0xFF;
                if s == 0 {
                    self.set_nz(a);
                } else if s < 32 {
                    let c = ((a >> (s - 1)) & 1) != 0;
                    let r = a >> s;
                    self.r[rdn] = r;
                    self.set_nzc(r, c);
                } else if s == 32 {
                    self.r[rdn] = 0;
                    self.set_nzc(0, (a & 0x8000_0000) != 0);
                } else {
                    self.r[rdn] = 0;
                    self.set_nzc(0, false);
                }
            }
            0x4 => {
                let s = b & 0xFF;
                if s == 0 {
                    self.set_nz(a);
                } else if s < 32 {
                    let c = ((a >> (s - 1)) & 1) != 0;
                    let r = ((a as i32) >> s) as u32;
                    self.r[rdn] = r;
                    self.set_nzc(r, c);
                } else {
                    let neg = (a & 0x8000_0000) != 0;
                    let r = if neg { 0xFFFF_FFFF } else { 0 };
                    self.r[rdn] = r;
                    self.set_nzc(r, neg);
                }
            }
            0x5 => {
                let cin = (self.xpsr & FLAG_C) != 0;
                self.r[rdn] = self.add_with_carry(a, b, cin, true);
            }
            0x6 => {
                let cin = (self.xpsr & FLAG_C) != 0;
                self.r[rdn] = self.add_with_carry(a, !b, cin, true);
            }
            0x7 => {
                let s = b & 0xFF;
                if s == 0 {
                    self.set_nz(a);
                } else {
                    let rot = s & 31;
                    let r = a.rotate_right(rot);
                    let c = (r & 0x8000_0000) != 0;
                    self.r[rdn] = r;
                    self.set_nzc(r, c);
                }
            }
            0x8 => self.set_nz(a & b),
            0x9 => self.r[rdn] = self.add_with_carry(!b, 0, true, true),
            0xA => {
                let _ = self.add_with_carry(a, !b, true, true);
            }
            0xB => {
                let _ = self.add_with_carry(a, b, false, true);
            }
            0xC => {
                let r = a | b;
                self.r[rdn] = r;
                self.set_nz(r);
            }
            0xD => {
                let r = a.wrapping_mul(b);
                self.r[rdn] = r;
                self.set_nz(r);
            }
            0xE => {
                let r = a & !b;
                self.r[rdn] = r;
                self.set_nz(r);
            }
            0xF => {
                let r = !b;
                self.r[rdn] = r;
                self.set_nz(r);
            }
            _ => unreachable!(),
        }
    }

    fn exec_special_data(&mut self, soc: &mut Py32Soc, inst_pc: u32, op: u16) {
        let sub = (op >> 8) & 3;
        match sub {
            0b00 => {
                // ADD Rdn, Rm (high regs, no flags)
                let rd = (((op >> 4) & 8) | (op & 7)) as usize;
                let rm = ((op >> 3) & 0xF) as usize;
                let res = self.reg(rd, inst_pc).wrapping_add(self.reg(rm, inst_pc));
                if rd == 15 {
                    self.pc = res & !1;
                } else {
                    self.set_reg(rd, res);
                }
            }
            0b01 => {
                // CMP Rn, Rm (high regs)
                let rn = (((op >> 4) & 8) | (op & 7)) as usize;
                let rm = ((op >> 3) & 0xF) as usize;
                let _ = self.add_with_carry(self.reg(rn, inst_pc), !self.reg(rm, inst_pc), true, true);
            }
            0b10 => {
                // MOV Rd, Rm (high regs, no flags)
                let rd = (((op >> 4) & 8) | (op & 7)) as usize;
                let rm = ((op >> 3) & 0xF) as usize;
                let v = self.reg(rm, inst_pc);
                if rd == 15 {
                    self.pc = v & !1;
                } else {
                    self.set_reg(rd, v);
                }
            }
            0b11 => {
                // BX Rm / BLX Rm
                let rm = ((op >> 3) & 0xF) as usize;
                let target = self.reg(rm, inst_pc);
                if (op & 0x0080) != 0 {
                    self.lr = inst_pc.wrapping_add(2) | 1;
                }
                self.branch_write_pc(soc, target);
            }
            _ => unreachable!(),
        }
    }

    fn exec_misc_16(&mut self, soc: &mut Py32Soc, op: u16, turbo: bool) {
        let sub = (op >> 8) & 0xF;
        match sub {
            0x0 => {
                // ADD SP, #imm7*4 / SUB SP, #imm7*4
                let imm = ((op & 0x7F) as u32) << 2;
                let sp = self.sp();
                if (op & 0x80) != 0 {
                    self.set_sp(sp.wrapping_sub(imm));
                } else {
                    self.set_sp(sp.wrapping_add(imm));
                }
            }
            0x2 => {
                let rm = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let v = self.r[rm];
                self.r[rd] = match (op >> 6) & 3 {
                    0b00 => (v as u16 as i16 as i32) as u32, // SXTH
                    0b01 => (v as u8 as i8 as i32) as u32,   // SXTB
                    0b10 => v & 0xFFFF,                      // UXTH
                    _ => v & 0xFF,                           // UXTB
                };
            }
            0x4 | 0x5 => {
                // PUSH {reglist, LR?}
                let push_lr = (op & 0x0100) != 0;
                let list = op & 0xFF;
                let count = list.count_ones() + u32::from(push_lr);
                let new_sp = self.sp().wrapping_sub(count * 4);
                let mut addr = new_sp;
                for i in 0..8usize {
                    if (list & (1 << i)) != 0 {
                        soc.write_u32(addr, self.r[i]);
                        addr = addr.wrapping_add(4);
                    }
                }
                if push_lr {
                    soc.write_u32(addr, self.lr);
                }
                self.set_sp(new_sp);
            }
            0x6 => {
                // CPSIE i (0xB662) / CPSID i (0xB672)
                if (op & 0xFFE0) == 0xB660 {
                    self.primask = u32::from((op & 0x0010) != 0);
                } else {
                    self.enter_exception(soc, 3);
                }
            }
            0xA => {
                let rm = ((op >> 3) & 7) as usize;
                let rd = (op & 7) as usize;
                let v = self.r[rm];
                match (op >> 6) & 3 {
                    0b00 => self.r[rd] = v.swap_bytes(), // REV
                    0b01 => {
                        let lo = (v as u16).swap_bytes() as u32;
                        let hi = ((v >> 16) as u16).swap_bytes() as u32;
                        self.r[rd] = (hi << 16) | lo; // REV16
                    }
                    0b11 => self.r[rd] = ((v as u16).swap_bytes() as i16 as i32) as u32, // REVSH
                    _ => self.enter_exception(soc, 3),
                }
            }
            0xC | 0xD => {
                // POP {reglist, PC?}
                let pop_pc = (op & 0x0100) != 0;
                let list = op & 0xFF;
                let mut addr = self.sp();
                for i in 0..8usize {
                    if (list & (1 << i)) != 0 {
                        self.r[i] = soc.read_u32(addr);
                        addr = addr.wrapping_add(4);
                    }
                }
                let loaded_pc = if pop_pc {
                    let v = soc.read_u32(addr);
                    addr = addr.wrapping_add(4);
                    Some(v)
                } else {
                    None
                };
                self.set_sp(addr);
                if let Some(target) = loaded_pc {
                    self.branch_write_pc(soc, target);
                }
            }
            0xE => {
                // BKPT #imm8 -> HardFault on Cortex-M0+ when no debugger attached
                self.enter_exception(soc, 3);
            }
            0xF => {
                // Hints: NOP (0xBF00), YIELD (0xBF10), WFE (0xBF20), WFI (0xBF30), SEV (0xBF40)
                if op == 0xBF30 {
                    self.halted_wfi = true;
                } else if turbo
                    && op == 0xBF00
                    && self.ipsr() == 0
                    && (self.primask & 1) == 0
                    && (soc.syst_csr & 3) == 3
                    && soc.uart_rx_fifo.is_empty()
                    && soc.usart1_rx_dr.is_none()
                    && soc.syst_cvr > 64
                {
                    // Fast-forward `delay_ms()` spin-loops cleanly to the next SysTick boundary
                    let jump = soc.syst_cvr;
                    soc.step_cycles(jump);
                }
            }
            _ => self.enter_exception(soc, 3),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::FlashInitMode;

    #[test]
    fn armv6m_push_bl_pop_and_crc32() {
        // Minimal program at 0x0800_2800:
        //   0x0800_2800: SP = 0x2000_4000
        //   0x0800_2804: Reset = 0x0800_2809
        //   0x0800_2808: MOVS R0, #42 (0x202A)
        //   0x0800_280A: ADDS R0, #8  (0x3008) -> R0 = 50
        //   0x0800_280C: WFI          (0xBF30)
        let mut img = vec![0u8; 32];
        img[0..4].copy_from_slice(&0x2000_4000u32.to_le_bytes());
        img[4..8].copy_from_slice(&0x0800_2809u32.to_le_bytes());
        img[8..10].copy_from_slice(&0x202Au16.to_le_bytes());
        img[10..12].copy_from_slice(&0x3008u16.to_le_bytes());
        img[12..14].copy_from_slice(&0xBF30u16.to_le_bytes());

        let mut soc = Py32Soc::new(&img, None, true, FlashInitMode::BlankWithCalib).unwrap();
        let mut cpu = CortexM0Plus::new(&mut soc);
        cpu.step(&mut soc, false);
        cpu.step(&mut soc, false);
        cpu.step(&mut soc, false);
        assert_eq!(cpu.r[0], 50);
        assert!(cpu.halted_wfi);
    }
}
