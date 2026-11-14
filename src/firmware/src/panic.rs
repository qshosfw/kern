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

pub use crate::BOOT_RECORD;
#[cfg(not(test))]
pub use crate::FAULT_EXTRA;

#[cfg(not(test))]
static FAULT_REENTRY_LOCK: AtomicBool = AtomicBool::new(false);

/// Bare-metal naked HardFault exception entry.
/// Captures `r4..r11`, `msp`, `psp`, `RCC_CSR`, and `SCB_ICSR` before rewinding `MSP`.
#[cfg(all(target_arch = "arm", not(test)))]
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
    let tag = BackupCrashTag {
        strikes,
        flags: BackupCrashTag::FLAG_DIRTY_CRASH
            | if strikes >= 2 {
                BackupCrashTag::FLAG_SAFE_MODE_REQ
            } else {
                0
            },
        reason_code,
    };
    hal::rtc::RealTimeClock::write_alarm_raw(tag.encode());
}

#[cfg(not(test))]
#[inline(never)]
fn enter_hardware_safety_gate() {
    bsp::bk4829_fault_quench();
    bsp::pins::BACKLIGHT.set_high();
    bsp::set_status_led(bsp::StatusLed::Red);
    if FAULT_REENTRY_LOCK.load(Ordering::Acquire) {
        loop {
            hal::wdt::Watchdog::feed();
        }
    }
    FAULT_REENTRY_LOCK.store(true, Ordering::Release);
    hal::wdt::Watchdog::feed();
    hal::spi::init();
}

#[cfg(not(test))]
#[inline(never)]
unsafe extern "C" fn hard_fault_inner(fault_sp: u32, exc_return: u32) -> ! {
    enter_hardware_safety_gate();

    // 2. STAGE 1: IMMEDIATE INVARIANT EXTRACTION
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
        lr = slot.stack_dump[5];
        pc = slot.stack_dump[6];
        xpsr = slot.stack_dump[7];
    } else {
        for i in 0..8 {
            slot.stack_dump[i] = 0;
        }
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

    let (unwind_count, unwound_lrs) = unwind_caller_stack(ef_valid, thread_sp, pc, lr);
    if pc == 0 && unwind_count > 0 {
        pc = unwound_lrs[0];
    }

    slot.fault_pc = pc;
    slot.fault_lr = lr;
    slot.xpsr = xpsr;
    for i in 0..4 {
        slot.stack_dump[8 + i] = unwound_lrs[i];
    }

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

    let extra = unsafe {
        let ptr = core::ptr::addr_of!(FAULT_EXTRA);
        if (*ptr).valid == FaultExtraRegs::FAULT_EXTRA_IDENT {
            Some(&*ptr)
        } else {
            None
        }
    };

    analyze_fault_cause(slot, ef_valid, thread_sp, extra);

    if (pc >= 0x0800_2800 && pc < 0x0802_0000) || (pc >= 0x2000_0000 && pc < 0x2000_4000) {
        let inst = unsafe { core::ptr::read_volatile((pc & !1) as *const u16) };
        if (inst & 0xFF00) == 0xDE00 {
            let code = (inst & 0xFF) as u8;
            slot.trap_code = code;
            if slot.actor_tag[0] == 0 {
                if let Some(t) = TrapCode::from_u8(code) {
                    slot.set_actor(t.as_str().as_bytes());
                } else {
                    slot.set_actor(b"UDF_TRAP");
                }
            }
        }
    }

    let ipsr_num = (xpsr & 0x3F) as u8;
    let psp_flag = if (exc_return & (1 << 2)) != 0 {
        1u8
    } else {
        0u8
    };
    let thumb_flag = if (xpsr & (1 << 24)) != 0 { 2u8 } else { 0u8 };
    let align_flag = if (xpsr & (1 << 9)) != 0 { 4u8 } else { 0u8 };
    slot.forensics.exec_mode = psp_flag | thumb_flag | align_flag;
    slot.forensics.ipsr_num = ipsr_num;
    slot.forensics.unwind_count = unwind_count;

    let sig_data = [
        pc,
        lr,
        slot.panic_file_hash,
        slot.panic_line as u32,
        ResetReason::Hard as u32,
    ];
    let sig_bytes =
        unsafe { core::slice::from_raw_parts(sig_data.as_ptr() as *const u8, sig_data.len() * 4) };
    slot.forensics.crash_signature = crypto::crc::CrcEngine::crc32(sig_bytes);

    let strikes = slot.crash_count;
    BOOT_RECORD.get_mut().commit_write();
    record_backup_crash(strikes, 1);

    hal::wdt::Watchdog::feed();

    #[cfg(target_arch = "arm")]
    if unsafe { (py32_pac::COREDEBUG_DHCSR.read_volatile() & py32_pac::COREDEBUG_C_DEBUGEN) != 0 } {
        unsafe {
            core::arch::asm!("bkpt #0");
        }
    }

    recovery_loop();
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

    let line = info
        .location()
        .map_or(0, |loc| loc.line().min(0xFFFF) as u16);
    let file_hash = info.location().map_or(0, |loc| {
        crypto::crc::CrcEngine::crc32(loc.file().as_bytes())
    });

    let slot = BOOT_RECORD.get_mut().begin_write();
    slot.reset_reason = ResetReason::Appf as u32;
    slot.fault_pc = lr;
    slot.fault_lr = lr;
    slot.fault_sp = sp;
    slot.panic_line = line;
    slot.panic_file_hash = file_hash;

    if let Some(loc) = info.location() {
        let f = loc.file().as_bytes();
        let start = f.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
        copy_tag(&mut slot.actor_tag, &f[start..]);
    } else {
        copy_tag(&mut slot.actor_tag, b"PANIC");
    }

    let sp_ptr = sp as *const u32;
    if (sp & 3) == 0 && sp >= 0x2000_0000 && sp <= (0x2000_4000 - 32) {
        for i in 0..8 {
            slot.stack_dump[i] = unsafe { core::ptr::read_volatile(sp_ptr.add(i)) };
        }
    }

    let (unwind_count, unwound_lrs) = unwind_caller_stack(true, sp, lr, lr);
    for i in 0..4 {
        slot.stack_dump[8 + i] = unwound_lrs[i];
    }

    let ipsr: u32;
    #[cfg(target_arch = "arm")]
    unsafe {
        core::arch::asm!("mrs {}, IPSR", out(reg) ipsr);
    }
    #[cfg(not(target_arch = "arm"))]
    {
        ipsr = 0;
    }

    let sig_data = [file_hash, line as u32, lr, sp, ResetReason::Appf as u32];
    let sig_bytes =
        unsafe { core::slice::from_raw_parts(sig_data.as_ptr() as *const u8, sig_data.len() * 4) };
    let crash_signature = crypto::crc::CrcEngine::crc32(sig_bytes);

    slot.forensics = kernel::boot::FaultForensics {
        inst_pre: 0,
        fault_reg: 0xFF,
        access_flags: 0,
        ipsr_num: (ipsr & 0x3F) as u8,
        exec_mode: 0x02, // Thumb mode
        unwind_count,
        _pad: 0,
        crash_signature,
    };

    let strikes = slot.crash_count;
    BOOT_RECORD.get_mut().commit_write();
    record_backup_crash(strikes, 2);

    hal::wdt::Watchdog::feed();

    recovery_loop();
}
