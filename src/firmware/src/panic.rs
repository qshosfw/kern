//! System Panic & HardFault Handler with Bare-Metal Fault Forensics.
//!
//! Captures comprehensive CPU context into the persistent BOOT Record, ensures RF PA
//! transmission safety, and presents interactive bare-metal recovery options on the LCD.

pub mod baremetal;
pub mod forensics;

#[cfg(not(test))]
pub use baremetal::*;
#[cfg(not(test))]
pub use forensics::*;

use kernel::boot::BootRecord;
#[cfg(not(test))]
use kernel::boot::ResetReason;
#[cfg(not(test))]
use py32_hal as hal;

#[repr(C)]
pub struct BootRecordCell(core::cell::UnsafeCell<BootRecord>);
unsafe impl Sync for BootRecordCell {}

impl BootRecordCell {
    pub const fn new() -> Self {
        Self(core::cell::UnsafeCell::new(BootRecord::new()))
    }

    #[allow(clippy::mut_from_ref)]
    #[inline(always)]
    pub fn get_mut(&self) -> &mut BootRecord {
        unsafe { &mut *self.0.get() }
    }

    #[inline(always)]
    pub fn get(&self) -> &BootRecord {
        unsafe { &*self.0.get() }
    }
}

#[cfg(not(test))]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(test))]
use kernel::boot::{BackupCrashTag, FaultExtraRegs, TrapCode};

#[cfg_attr(test, allow(unused_imports))]
pub use crate::{BOOT_RECORD, FAULT_EXTRA};

#[cfg(not(test))]
static FAULT_REENTRY_LOCK: AtomicBool = AtomicBool::new(false);

/// Bare-metal naked HardFault exception entry.
#[cfg(all(target_arch = "arm", not(test), feature = "krx_native"))]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    core::arch::naked_asm!(
        "cpsid i",
        "ldr r0, ={in_flight}",
        "ldrb r0, [r0]",
        "cmp r0, #0",
        "beq 3f",
        "mov r4, lr",
        "mrs r0, msp",
        "ldr r1, ={try_contain}",
        "blx r1",
        "cmp r0, #0",
        "beq 3f",
        "bx r4",
        "3:",
        "ldr r3, ={extra}",
        "stm r3!, {{r4-r7}}",
        "mov r4, r8",
        "mov r5, r9",
        "mov r6, r10",
        "mov r7, r11",
        "stm r3!, {{r4-r7}}",
        "mrs r4, msp",
        "mrs r5, psp",
        "ldr r6, =0x40021060",
        "ldr r6, [r6]",
        "ldr r7, =0xE000ED04",
        "ldr r7, [r7]",
        "stm r3!, {{r4-r7}}",
        "ldr r4, ={extra_magic}",
        "str r4, [r3]",
        "mov r0, lr",
        "movs r1, #4",
        "tst r0, r1",
        "bne 1f",
        "mrs r0, msp",
        "b 2f",
        "1:",
        "mrs r0, psp",
        "2:",
        "mov r1, lr",
        "ldr r2, =0x20004000",
        "msr msp, r2",
        "ldr r2, ={inner}",
        "bx r2",
        in_flight = sym crate::runtime::fault::GUEST_IN_FLIGHT,
        try_contain = sym crate::runtime::fault::try_contain_app_fault,
        extra = sym crate::FAULT_EXTRA,
        extra_magic = const FaultExtraRegs::FAULT_EXTRA_IDENT,
        inner = sym hard_fault_inner,
    );
}

#[cfg(all(target_arch = "arm", not(test), not(feature = "krx_native")))]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    core::arch::naked_asm!(
        "cpsid i",
        "ldr r3, ={extra}",
        "stm r3!, {{r4-r7}}",
        "mov r4, r8",
        "mov r5, r9",
        "mov r6, r10",
        "mov r7, r11",
        "stm r3!, {{r4-r7}}",
        "mrs r4, msp",
        "mrs r5, psp",
        "ldr r6, =0x40021060",
        "ldr r6, [r6]",
        "ldr r7, =0xE000ED04",
        "ldr r7, [r7]",
        "stm r3!, {{r4-r7}}",
        "ldr r4, ={extra_magic}",
        "str r4, [r3]",
        "mov r0, lr",
        "movs r1, #4",
        "tst r0, r1",
        "bne 1f",
        "mrs r0, msp",
        "b 2f",
        "1:",
        "mrs r0, psp",
        "2:",
        "mov r1, lr",
        "ldr r2, =0x20004000",
        "msr msp, r2",
        "ldr r2, ={inner}",
        "bx r2",
        extra = sym crate::FAULT_EXTRA,
        extra_magic = const FaultExtraRegs::FAULT_EXTRA_IDENT,
        inner = sym hard_fault_inner,
    );
}

#[cfg(all(not(target_arch = "arm"), not(test)))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    loop {}
}

#[cfg(not(test))]
fn record_backup_crash(strikes: u8, reason_code: u8) {
    // A crashed headless node must come back headless (no panel prompts, no keypad).
    let kept = crate::boot::backup::read().flags & BackupCrashTag::FLAG_HEADLESS;
    let tag = BackupCrashTag {
        strikes,
        flags: kept
            | BackupCrashTag::FLAG_DIRTY_CRASH
            | if strikes >= 2 {
                BackupCrashTag::FLAG_SAFE_MODE_REQ
            } else {
                0
            },
        reason_code,
    };
    crate::boot::backup::write(tag);
}

#[cfg(not(test))]
#[inline(never)]
fn enter_hardware_safety_gate() {
    // Halt active DMA1 channels immediately so stalled transfers cannot corrupt SRAM or SPI1.
    unsafe {
        if (hal::pac::rcc::RCC_AHBENR.read_volatile() & hal::pac::rcc::AHBENR_DMA1) != 0 {
            for ch in 1..=7 {
                hal::pac::dma::ccr(ch).write_volatile(0);
            }
            hal::pac::dma::DMA_IFCR.write_volatile(0x0FFF_FFFF);
        }
    }

    bsp::bk4829_fault_quench();
    #[cfg(feature = "usb")]
    hal::rcc::disable_usbd();
    bsp::set_status_led(bsp::StatusLed::Red);

    // Re-entrant fault or secondary panic inside diagnostics must reset immediately
    // rather than looping forever with watchdog feeds.
    if FAULT_REENTRY_LOCK.load(Ordering::Acquire) {
        cortex_m::peripheral::SCB::sys_reset();
    }
    FAULT_REENTRY_LOCK.store(true, Ordering::Release);

    hal::wdt::Watchdog::feed();
}

#[cfg(not(test))]
#[inline(never)]
fn finish_fault_record(strikes: u8, reason_code: u8) -> ! {
    BOOT_RECORD.get_mut().commit_write();
    record_backup_crash(strikes, reason_code);
    recovery_loop()
}

#[cfg(not(test))]
#[inline(never)]
unsafe extern "C" fn hard_fault_inner(fault_sp: u32, exc_return: u32) -> ! {
    enter_hardware_safety_gate();

    let ef_valid =
        (fault_sp >= 0x2000_0000) && (fault_sp <= (0x2000_4000 - 32)) && ((fault_sp & 3) == 0);

    let slot = BOOT_RECORD.get_mut().begin_write();
    slot.reset_reason = ResetReason::Hard as u32;
    slot.fault_sp = fault_sp;

    let mut pc = 0u32;
    let mut lr = exc_return;
    let mut xpsr = 0u32;

    if ef_valid {
        let p = fault_sp as *const u32;
        for i in 0..8 {
            slot.stack_dump[i] = unsafe { core::ptr::read_volatile(p.add(i)) };
        }
        lr = unsafe { core::ptr::read_volatile(p.add(5)) };
        pc = unsafe { core::ptr::read_volatile(p.add(6)) };
        xpsr = unsafe { core::ptr::read_volatile(p.add(7)) };
    } else {
        slot.stack_dump[5] = exc_return;
    }

    let align_pad = if ef_valid && (xpsr & (1 << 9)) != 0 {
        4u32
    } else {
        0u32
    };
    let thread_sp = if ef_valid {
        fault_sp.wrapping_add(32).wrapping_add(align_pad)
    } else {
        fault_sp
    };

    let unwind_count =
        unwind_caller_stack(ef_valid, thread_sp, pc, lr, &mut slot.stack_dump[8..12]);
    if pc == 0 && unwind_count > 0 {
        pc = slot.stack_dump[8];
    }

    slot.fault_pc = pc;
    slot.fault_lr = lr;
    slot.xpsr = xpsr;

    let caller_sp = if ef_valid
        && (thread_sp & 3) == 0
        && thread_sp >= 0x2000_0000
        && thread_sp <= (0x2000_4000 - 16)
    {
        thread_sp as *const u32
    } else if !ef_valid {
        0x2000_0000 as *const u32
    } else {
        core::ptr::null()
    };
    if !caller_sp.is_null() {
        for i in 0..4 {
            slot.stack_dump[12 + i] = unsafe { core::ptr::read_volatile(caller_sp.add(i)) };
        }
    }

    let mut inst = 0u16;
    let pc_even = pc & !1;
    let pc_mapped = (pc_even >= 0x0800_0000 && pc_even < 0x0802_0000)
        || (pc_even >= 0x2000_0000 && pc_even < 0x2000_4000);
    if pc_mapped {
        inst = unsafe { core::ptr::read_volatile(pc_even as *const u16) };
        slot.panic_line = inst;
        if pc_even >= 0x0800_0002 && pc_even != 0x2000_0000 {
            slot.forensics.inst_pre =
                unsafe { core::ptr::read_volatile((pc_even - 2) as *const u16) };
        }
    }

    if !ef_valid {
        slot.set_actor(b"STACK_OVF");
    } else if pc_mapped && (inst & 0xFF00) == 0xDE00 {
        let code = (inst & 0xFF) as u8;
        slot.trap_code = code;
        if let Some(t) = TrapCode::from_u8(code) {
            slot.set_actor(t.as_str().as_bytes());
        } else {
            slot.set_actor(b"UDF_TRAP");
        }
    } else {
        slot.set_actor(b"HARDFAULT");
    }

    let ipsr_num = (xpsr & 0x3F) as u8;
    let psp_flag = u8::from((exc_return & (1 << 2)) != 0);
    let thumb_flag = if (xpsr & (1 << 24)) != 0 { 2u8 } else { 0u8 };
    let align_flag = if (xpsr & (1 << 9)) != 0 { 4u8 } else { 0u8 };
    slot.forensics.exec_mode = psp_flag | thumb_flag | align_flag;
    slot.forensics.ipsr_num = ipsr_num;
    slot.forensics.unwind_count = unwind_count;
    slot.forensics.crash_signature = 0;

    let strikes = slot.crash_count;
    finish_fault_record(strikes, 1);
}

// ── Production & Graphical Panic Handler ────────────────────────────────────

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!(
            "cpsid i",
            "mov r1, sp",
            "mov r2, lr",
            "ldr r3, =0x20004000",
            "msr msp, r3",
            "ldr r3, ={inner}",
            "bx r3",
            inner = sym panic_inner,
            in("r0") info as *const _ as usize,
            options(noreturn)
        );
    }
    #[cfg(not(target_arch = "arm"))]
    loop {}
}

#[cfg(not(test))]
#[inline(never)]
unsafe extern "C" fn panic_inner(info_ptr: usize, sp: u32, lr: u32) -> ! {
    enter_hardware_safety_gate();

    let info = unsafe { &*(info_ptr as *const core::panic::PanicInfo) };
    unsafe {
        (*core::ptr::addr_of_mut!(FAULT_EXTRA)).valid = 0;
    }

    let slot = BOOT_RECORD.get_mut().begin_write();
    slot.reset_reason = ResetReason::Appf as u32;
    slot.trap_code = 0xFE;
    slot.fault_pc = lr;
    slot.fault_lr = lr;
    slot.fault_sp = sp;

    let (line, file_hash) = if let Some(loc) = info.location() {
        let f = loc.file().as_bytes();
        let start = f.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
        slot.set_actor(&f[start..]);
        (
            loc.line().min(0xFFFF) as u16,
            crypto::crc::CrcEngine::crc32(f),
        )
    } else {
        slot.set_actor(b"PANIC");
        (0, 0)
    };
    slot.panic_line = line;
    slot.panic_file_hash = file_hash;

    let sp_ptr = sp as *const u32;
    if (sp & 3) == 0 && sp >= 0x2000_0000 && sp <= (0x2000_4000 - 32) {
        for i in 0..8 {
            slot.stack_dump[i] = unsafe { core::ptr::read_volatile(sp_ptr.add(i)) };
        }
    }

    let unwind_count = unwind_caller_stack(true, sp, lr, lr, &mut slot.stack_dump[8..12]);

    let ipsr: u32;
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("mrs {}, IPSR", out(reg) ipsr);
    }
    #[cfg(not(target_arch = "arm"))]
    {
        ipsr = 0;
    }

    slot.forensics = kernel::boot::FaultForensics {
        inst_pre: 0,
        fault_reg: 0xFF,
        access_flags: 0,
        ipsr_num: (ipsr & 0x3F) as u8,
        exec_mode: 0x02, // Thumb mode
        unwind_count,
        _pad: 0,
        crash_signature: 0,
    };

    let strikes = slot.crash_count;
    finish_fault_record(strikes, 2);
}
