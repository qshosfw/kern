//! System Panic & HardFault Handler with Bare-Metal Fault Forensics.
//!
//! Captures comprehensive CPU context into the persistent BOOT Record, ensures RF PA
//! transmission safety, and presents interactive bare-metal recovery options on the LCD.

pub mod baremetal;
pub mod forensics;

pub use baremetal::*;
pub use forensics::*;

use kernel::boot::{BootRecord, ResetReason};
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

#[used]
#[unsafe(link_section = ".uninit")]
pub static BOOT_RECORD: BootRecordCell = BootRecordCell::new();

static mut FAULT_REENTRY_LOCK: bool = false;

/// Bare-metal naked HardFault exception entry.
#[cfg(target_arch = "arm")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    core::arch::naked_asm!(
        "cpsid i",
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
        inner = sym hard_fault_inner,
    );
}

#[cfg(not(target_arch = "arm"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HardFault() -> ! {
    loop {}
}

#[inline(never)]
fn enter_hardware_safety_gate() {
    bsp::pins::AUDIO_PA.set_low();
    bsp::pins::BACKLIGHT.set_high();
    bsp::pins::FLASHLIGHT.set_low();
    bsp::set_status_led(bsp::StatusLed::Red);
    if unsafe { *core::ptr::addr_of!(FAULT_REENTRY_LOCK) } {
        loop {
            hal::wdt::Watchdog::feed();
        }
    }
    unsafe {
        *core::ptr::addr_of_mut!(FAULT_REENTRY_LOCK) = true;
    }
    hal::wdt::Watchdog::feed();
    hal::spi::init();
}

#[inline(never)]
unsafe extern "C" fn hard_fault_inner(fault_sp: u32, exc_return: u32) -> ! {
    enter_hardware_safety_gate();

    // 2. STAGE 1: IMMEDIATE INVARIANT EXTRACTION
    let ef_valid =
        (fault_sp >= 0x2000_0000) && (fault_sp <= (0x2000_4000 - 32)) && ((fault_sp & 3) == 0);

    let (r0, r1, r2, r3, r12, lr, mut pc, xpsr) = if ef_valid {
        let p = fault_sp as *const u32;
        unsafe {
            (
                core::ptr::read_volatile(p.add(0)),
                core::ptr::read_volatile(p.add(1)),
                core::ptr::read_volatile(p.add(2)),
                core::ptr::read_volatile(p.add(3)),
                core::ptr::read_volatile(p.add(4)),
                core::ptr::read_volatile(p.add(5)),
                core::ptr::read_volatile(p.add(6)),
                core::ptr::read_volatile(p.add(7)),
            )
        }
    } else {
        (0, 0, 0, 0, 0, exc_return, 0, 0)
    };

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

    let slot = BOOT_RECORD.get_mut().begin_write();
    slot.reset_reason = ResetReason::Hard as u32;
    slot.fault_pc = pc;
    slot.fault_lr = lr;
    slot.fault_sp = fault_sp;
    slot.xpsr = xpsr;

    slot.stack_dump[0] = r0;
    slot.stack_dump[1] = r1;
    slot.stack_dump[2] = r2;
    slot.stack_dump[3] = r3;
    slot.stack_dump[4] = r12;
    slot.stack_dump[5] = lr;
    slot.stack_dump[6] = pc;
    slot.stack_dump[7] = xpsr;
    slot.stack_dump[8] = unwound_lrs[0];
    slot.stack_dump[9] = unwound_lrs[1];
    slot.stack_dump[10] = unwound_lrs[2];
    slot.stack_dump[11] = unwound_lrs[3];

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

    analyze_fault_cause(slot, ef_valid, thread_sp);

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

    BOOT_RECORD.get_mut().commit_write();

    hal::wdt::Watchdog::feed();

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

    copy_tag(&mut slot.actor_tag, b"PANIC");
    if let Some(loc) = info.location() {
        let f = loc.file().as_bytes();
        let start = f.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1);
        let name = &f[start..];
        let len = name.len().min(16);
        slot.actor_tag[..len].copy_from_slice(&name[..len]);
    }

    let sp_ptr = sp as *const u32;
    if (sp & 3) == 0 && sp >= 0x2000_0000 && sp <= (0x2000_4000 - 32) {
        for i in 0..8 {
            slot.stack_dump[i] = unsafe { core::ptr::read_volatile(sp_ptr.add(i)) };
        }
    }

    let (unwind_count, unwound_lrs) = unwind_caller_stack(true, sp, lr, lr);
    slot.stack_dump[8] = unwound_lrs[0];
    slot.stack_dump[9] = unwound_lrs[1];
    slot.stack_dump[10] = unwound_lrs[2];
    slot.stack_dump[11] = unwound_lrs[3];

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
        crash_signature,
    };

    BOOT_RECORD.get_mut().commit_write();

    hal::wdt::Watchdog::feed();

    recovery_loop();
}
