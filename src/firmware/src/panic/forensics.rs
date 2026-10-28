//! ARM Cortex-M0+ CPU Fault Opcode Disassembly & Caller Stack Unwinder.

#[inline(always)]
pub fn copy_tag(dst: &mut [u8; 16], src: &[u8]) {
    let len = src.len().min(16);
    dst[..len].copy_from_slice(&src[..len]);
}

#[cfg(feature = "flight_recorder")]
#[inline(never)]
pub fn unwind_caller_stack(ef_valid: bool, thread_sp: u32, pc: u32, lr: u32) -> (u8, [u32; 4]) {
    let mut unwind_count = 0u8;
    let mut unwound_lrs = [0u32; 4];

    let (start_sp, max_words) = if ef_valid
        && (thread_sp & 3) == 0
        && thread_sp >= 0x2000_0000
        && thread_sp <= (0x2000_4000 - 32)
    {
        (
            thread_sp,
            (((0x2000_4000 - thread_sp) >> 2) as usize).min(64),
        )
    } else {
        (0x2000_0000, 64)
    };

    let sp_slice = unsafe { core::slice::from_raw_parts(start_sp as *const u32, max_words) };
    for &w in sp_slice {
        if w >= 0x0800_2800 && w < 0x0802_0000 && (w & 1) != 0 && w != pc && w != lr {
            if (unwind_count == 0 || unwound_lrs[(unwind_count - 1) as usize] != w)
                && (unwind_count < 4)
            {
                unwound_lrs[unwind_count as usize] = w;
                unwind_count += 1;
            }
        }
    }

    (unwind_count, unwound_lrs)
}

#[cfg(not(feature = "flight_recorder"))]
#[inline(always)]
pub fn unwind_caller_stack(_ef_valid: bool, _thread_sp: u32, _pc: u32, _lr: u32) -> (u8, [u32; 4]) {
    (0, [0u32; 4])
}

#[cfg(feature = "flight_recorder")]
#[inline(never)]
pub fn analyze_fault_cause(slot: &mut kernel::boot::BootSlot, ef_valid: bool, thread_sp: u32) {
    let mut actor: &'static [u8] = b"FORCED";
    let mut fault_opcode = 0u16;
    let mut fault_addr = 0xFFFF_FFFFu32;
    let mut fault_reg = 0xFFu8;
    let mut access_flags = 0u8;

    let pc = slot.fault_pc;
    let fault_sp = slot.fault_sp;
    let xpsr = slot.xpsr;
    let pc_in_flash = pc >= 0x0800_2800 && pc < 0x0802_0000;
    let pc_in_sram = pc >= 0x2000_0000 && pc < 0x2000_4000;

    if !ef_valid || thread_sp < 0x2000_0080 || thread_sp > 0x2000_4000 {
        actor = b"STACK_OVF";
        fault_addr = fault_sp;
    } else if (xpsr & (1 << 24)) == 0 {
        actor = b"INVSTATE";
    } else if !pc_in_flash && !pc_in_sram {
        actor = b"IBUSERR";
        fault_addr = pc;
    } else if (pc & 1) != 0 {
        actor = b"UNALIGNED";
        fault_addr = pc;
    } else {
        let inst = unsafe { core::ptr::read_volatile((pc & !1) as *const u16) };
        fault_opcode = inst;

        if (inst & 0xFF00) == 0xBE00 {
            actor = b"DEBUGEVT";
        } else if (inst & 0xFF00) == 0xDE00 {
            actor = b"UNDEFINSTR";
        } else if (inst & 0xFE00) == 0xB400 {
            actor = b"STKERR";
            fault_addr = fault_sp;
            fault_reg = 13;
            access_flags = 0b0101;
        } else if (inst & 0xFE00) == 0xBC00 {
            actor = b"UNSTKERR";
            fault_addr = fault_sp;
            fault_reg = 13;
            access_flags = 0b0100;
        } else if (inst & 0xF000) == 0x8000 || (inst & 0xE000) == 0x6000 {
            let is_write = (inst & 0x0800) == 0;
            let sz = if (inst & 0xF000) == 0x8000 {
                1
            } else if (inst & 0x1000) != 0 {
                0
            } else {
                2
            };
            fault_reg = (inst & 7) as u8;
            access_flags = (sz << 1) | (is_write as u8);

            let rn = ((inst >> 3) & 7) as usize;
            let reg_val = if rn < 4 { slot.stack_dump[rn] } else { 0 };
            let imm = (((inst >> 6) & 0x1F) as u32) << sz;
            fault_addr = reg_val.wrapping_add(imm);
            if fault_addr < 0x0000_0100 {
                actor = b"DACCVIOL";
            } else {
                actor = b"PRECISERR";
            }
        } else if (inst & 0xF200) == 0x5000 {
            fault_reg = (inst & 7) as u8;
            let is_write = (inst & 0x0800) == 0;
            let sz = match (inst >> 9) & 7 {
                0 | 4 => 2,
                1 | 5 => 1,
                _ => 0,
            };
            access_flags = (sz << 1) | (is_write as u8);
            actor = b"PRECISERR";
        } else if (inst & 0xFF87) == 0x4700 {
            actor = b"INVPC";
        }
    }

    let mut inst_pre = 0u16;
    if (pc >= 0x0800_2802 && pc < 0x0802_0000) || (pc >= 0x2000_0002 && pc < 0x2000_4000) {
        if (pc & 1) == 0 {
            inst_pre = unsafe { core::ptr::read_volatile(((pc - 2) & !1) as *const u16) };
        }
    }

    if fault_addr != 0xFFFF_FFFF {
        let width = (access_flags >> 1) & 3;
        if (width == 2 && (fault_addr & 3) != 0) || (width == 1 && (fault_addr & 1) != 0) {
            access_flags |= 0x08;
        }
    }

    slot.set_actor(actor);
    slot.panic_line = fault_opcode;
    slot.panic_file_hash = fault_addr;
    slot.forensics.inst_pre = inst_pre;
    slot.forensics.fault_reg = fault_reg;
    slot.forensics.access_flags = access_flags;
}

#[cfg(not(feature = "flight_recorder"))]
#[inline(always)]
pub fn analyze_fault_cause(_slot: &mut kernel::boot::BootSlot, _ef_valid: bool, _thread_sp: u32) {}
