//! BOOT Record (Persistent Crash & Diagnostics Log)
//!
//! Microkernel double-buffered crash report and boot diagnostic record.
//! Survives warm resets in `.uninit` SRAM (256 bytes = Slot A + Slot B).
//! Double-buffered A/B flip with CRC32 integrity verification.

use crypto::crc::CrcEngine;

pub const BOOT_RECORD_IDENT: [u8; 4] = [0xC0, 0xDE, 0x02, 0xFD];

/// Semantic Thumb-16 `udf #imm8` trap codes (`0 B` `.rodata` per trap site).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrapCode {
    AssertFail = 0x01,
    KernelLock = 0x02,
    StackCanary = 0x03,
    PagerIo = 0x04,
    AllocInvariant = 0x05,
    RadioLock = 0x06,
    RustPanic = 0xFE,
}

const TRAP_NAMES: &[u8] = b"ASSERT\0KLOCK\0CANARY\0PAGER_IO\0ALLOC\0RFLOCK\0PANIC\0";

impl TrapCode {
    pub const fn as_str(&self) -> &'static str {
        let idx = match self {
            Self::RustPanic => 6,
            _ => (*self as u8) - 1,
        };
        utils::format::nth_cstr(TRAP_NAMES, idx)
    }

    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::AssertFail),
            0x02 => Some(Self::KernelLock),
            0x03 => Some(Self::StackCanary),
            0x04 => Some(Self::PagerIo),
            0x05 => Some(Self::AllocInvariant),
            0x06 => Some(Self::RadioLock),
            0xFE => Some(Self::RustPanic),
            _ => None,
        }
    }
}

/// Callee-saved and silicon control registers captured in `.uninit` prior to `MSP` rewind.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FaultExtraRegs {
    pub callee: [u32; 8], // r4..r11
    pub msp: u32,
    pub psp: u32,
    pub rcc_csr: u32,
    pub icsr: u32,
    pub valid: u32,
}

impl FaultExtraRegs {
    pub const FAULT_EXTRA_IDENT: u32 = 0xF007_CAFE;

    pub const fn empty() -> Self {
        Self {
            callee: [0; 8],
            msp: 0,
            psp: 0,
            rcc_csr: 0,
            icsr: 0,
            valid: 0,
        }
    }
}

/// Trigger an immediate semantic Thumb-16 `udf` trap (`0 B` `.rodata`).
#[inline(always)]
pub fn ktrap(code: TrapCode) -> ! {
    #[cfg(target_arch = "arm")]
    unsafe {
        match code {
            TrapCode::AssertFail => core::arch::asm!("udf #0x01", options(noreturn)),
            TrapCode::KernelLock => core::arch::asm!("udf #0x02", options(noreturn)),
            TrapCode::StackCanary => core::arch::asm!("udf #0x03", options(noreturn)),
            TrapCode::PagerIo => core::arch::asm!("udf #0x04", options(noreturn)),
            TrapCode::AllocInvariant => core::arch::asm!("udf #0x05", options(noreturn)),
            TrapCode::RadioLock => core::arch::asm!("udf #0x06", options(noreturn)),
            TrapCode::RustPanic => core::arch::asm!("udf #0xFE", options(noreturn)),
        }
    }
    #[cfg(not(target_arch = "arm"))]
    {
        panic!("ktrap({:?})", code);
    }
}

#[macro_export]
macro_rules! kassert {
    ($cond:expr) => {
        if !$cond {
            $crate::boot::ktrap($crate::boot::TrapCode::AssertFail);
        }
    };
    ($cond:expr, $code:expr) => {
        if !$cond {
            $crate::boot::ktrap($code);
        }
    };
}

/// Reset reason FourCC tags (§13.4 normative)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ResetReason {
    /// Kernel fault / panic / CPU exception
    Hard = u32::from_le_bytes(*b"HARD"),
    /// Package / Userland application or driver fault
    Appf = u32::from_le_bytes(*b"APPF"),
    /// Hardware Independent Watchdog timeout
    Wdog = u32::from_le_bytes(*b"WDOG"),
    /// Memory format-tag corruption (§13.2)
    Tags = u32::from_le_bytes(*b"TAGS"),
    /// Supply voltage brownout reset
    Brow = u32::from_le_bytes(*b"BROW"),
}

impl ResetReason {
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw == Self::Hard as u32 {
            Some(Self::Hard)
        } else if raw == Self::Appf as u32 {
            Some(Self::Appf)
        } else if raw == Self::Wdog as u32 {
            Some(Self::Wdog)
        } else if raw == Self::Tags as u32 {
            Some(Self::Tags)
        } else if raw == Self::Brow as u32 {
            Some(Self::Brow)
        } else {
            None
        }
    }

    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"HARD\0APPF\0WDOG\0TAGS\0BROW\0";
        let idx = match self {
            Self::Hard => 0,
            Self::Appf => 1,
            Self::Wdog => 2,
            Self::Tags => 3,
            Self::Brow => 4,
        };
        utils::format::nth_cstr(NAMES, idx)
    }
}

/// Extended low-level fault forensics & execution state (12 bytes, naturally aligned)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FaultForensics {
    /// Deterministic 32-bit fault fingerprint for automated grouping & deduplication
    pub crash_signature: u32,
    /// Preceding 16-bit instruction at PC - 2 (disassembly stream window)
    pub inst_pre: u16,
    /// Target/source register index (0..15, or 0xFF if not applicable)
    pub fault_reg: u8,
    /// Access semantics: bit 0: Read(0)/Write(1); bits 1..2: Byte(0)/Halfword(1)/Word(2); bit 3: Unaligned
    pub access_flags: u8,
    /// Active interrupt/exception vector number (0 = Thread, 15 = SysTick, 3 = HardFault)
    pub ipsr_num: u8,
    /// Execution mode: bit 0: 0=MSP/1=PSP; bit 1: Thumb; bit 2: 8-byte stack alignment pad
    pub exec_mode: u8,
    /// Number of valid caller return addresses recovered from the stack
    pub unwind_count: u8,
    /// Alignment padding for natural 4-byte struct boundary
    pub _pad: u8,
}

const _: () = assert!(core::mem::size_of::<FaultForensics>() == 12);
const _: () = assert!(core::mem::align_of::<FaultForensics>() == 4);

impl FaultForensics {
    pub const fn empty() -> Self {
        Self {
            crash_signature: 0,
            inst_pre: 0,
            fault_reg: 0xFF,
            access_flags: 0,
            ipsr_num: 0,
            exec_mode: 0,
            unwind_count: 0,
            _pad: 0,
        }
    }
}

/// 128-byte naturally aligned crash log slot (§13.4 Listing 13.2)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BootSlot {
    pub reset_reason: u32,
    pub fault_pc: u32,
    pub fault_lr: u32,
    pub fault_sp: u32,
    pub xpsr: u32,
    pub panic_file_hash: u32,
    pub stack_dump: [u32; 16],
    pub actor_tag: [u8; 16],
    pub forensics: FaultForensics,
    pub ident: [u8; 4],
    pub panic_line: u16,
    pub crash_count: u8,
    pub trap_code: u8,
    pub crc32: u32,
}

const _: () = assert!(core::mem::size_of::<BootSlot>() == 128);
const _: () = assert!(core::mem::align_of::<BootSlot>() == 4);

impl BootSlot {
    pub const fn empty() -> Self {
        Self {
            reset_reason: 0,
            fault_pc: 0,
            fault_lr: 0,
            fault_sp: 0,
            xpsr: 0,
            panic_file_hash: 0,
            stack_dump: [0; 16],
            actor_tag: [0; 16],
            forensics: FaultForensics::empty(),
            ident: [0; 4],
            panic_line: 0,
            crash_count: 0,
            trap_code: 0,
            crc32: 0,
        }
    }

    /// Calculate CRC32 of slot bytes excluding the crc32 field itself
    pub fn compute_crc(&self) -> u32 {
        let bytes = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                core::mem::size_of::<Self>() - 4,
            )
        };
        CrcEngine::crc32(bytes)
    }

    /// Returns true if slot contains valid format identifier and valid CRC32
    pub fn is_valid(&self) -> bool {
        if self.ident != BOOT_RECORD_IDENT {
            return false;
        }
        self.compute_crc() == self.crc32
    }

    pub fn reason(&self) -> Option<ResetReason> {
        ResetReason::from_raw(self.reset_reason)
    }

    pub fn set_actor(&mut self, actor: &[u8]) {
        let copy_actor_len = actor.len().min(16);
        self.actor_tag[..copy_actor_len].copy_from_slice(&actor[..copy_actor_len]);
    }

    /// Returns true if slot records an active crash or fault condition.
    pub fn is_crash(&self) -> bool {
        self.is_valid() && self.reason().is_some()
    }

    /// Extract actor tag as a UTF-8 string slice (trimmed at first NUL byte).
    pub fn actor_str(&self) -> &str {
        let len = self
            .actor_tag
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.actor_tag.len());
        utils::format::ascii_str(&self.actor_tag[..len]).unwrap_or("Unknown")
    }

    /// Returns natural-language clause explaining why the crash occurred.
    pub fn natural_cause(&self) -> &'static str {
        match self.reason() {
            Some(ResetReason::Wdog) => "because the system froze or",
            Some(ResetReason::Tags) => "due to corrupted data",
            Some(ResetReason::Brow) => "due to a sudden drop in",
            Some(ResetReason::Appf) => "due to a software panic in",
            Some(ResetReason::Hard) => {
                let actor = self.actor_str();
                if actor.starts_with("STACK_OVF") {
                    "because call stack memory"
                } else if actor.starts_with("DACCVIOL") {
                    "due to an invalid data access"
                } else if actor.starts_with("PRECISERR") {
                    "due to a hardware bus error"
                } else if actor.starts_with("UNDEFINSTR") {
                    "due to an illegal instruction"
                } else if actor.starts_with("UNALIGNED") {
                    "due to an unaligned memory access"
                } else if actor.starts_with("IBUSERR") {
                    "due to an instruction fetch failure"
                } else if actor.starts_with("INVSTATE") {
                    "due to an invalid CPU execution state"
                } else if actor.starts_with("INVPC") {
                    "due to an invalid program counter"
                } else if actor.starts_with("STKERR") || actor.starts_with("UNSTKERR") {
                    "due to an exception stack error"
                } else if actor.starts_with("DEBUGEVT") {
                    "after a debug breakpoint halted"
                } else if actor.starts_with("WATCHDOG") {
                    "because the system froze and"
                } else {
                    "due to an unexpected CPU fault"
                }
            }
            None => "due to an unexpected hardware",
        }
    }

    /// Formats natural-language detail clause into `out`.
    pub fn natural_detail<'a>(&'a self, out: &'a mut [u8]) -> &'a str {
        match self.reason() {
            Some(ResetReason::Appf) => {
                let actor = self.actor_str();
                let mut pos = 0;
                let a_bytes = actor.as_bytes();
                let a_len = a_bytes.len().min(out.len());
                out[..a_len].copy_from_slice(&a_bytes[..a_len]);
                pos += a_len;

                if self.panic_line > 0 {
                    const MID: &[u8] = b" at line ";
                    let m_len = MID.len().min(out.len().saturating_sub(pos));
                    out[pos..pos + m_len].copy_from_slice(&MID[..m_len]);
                    pos += m_len;

                    let l_str = utils::format::format_u32(self.panic_line as u32, &mut out[pos..]);
                    pos += l_str.len();

                    if pos < out.len() {
                        out[pos] = b'.';
                        pos += 1;
                    }
                    unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
                } else {
                    const SUFFIX: &[u8] = b" module.";
                    let s_len = SUFFIX.len().min(out.len().saturating_sub(pos));
                    out[pos..pos + s_len].copy_from_slice(&SUFFIX[..s_len]);
                    pos += s_len;
                    unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
                }
            }
            Some(ResetReason::Hard) => {
                let actor = self.actor_str();
                if actor.starts_with("STACK_OVF") {
                    "overflowed available RAM."
                } else {
                    let (addr, prefix): (u32, &[u8]) =
                        if self.panic_file_hash != 0 && self.panic_file_hash != 0xFFFF_FFFF {
                            (self.panic_file_hash, b"at memory address 0x")
                        } else if self.fault_pc > 0 {
                            (self.fault_pc, b"at program address 0x")
                        } else {
                            (0, b"")
                        };

                    if addr > 0 {
                        let mut pos = 0;
                        let p_len = prefix.len().min(out.len());
                        out[..p_len].copy_from_slice(&prefix[..p_len]);
                        pos += p_len;
                        let mut hbuf = [0u8; 8];
                        let h_str = utils::format::format_hex32(addr, &mut hbuf);
                        let h_len = h_str.len().min(out.len().saturating_sub(pos));
                        out[pos..pos + h_len].copy_from_slice(h_str.as_bytes());
                        pos += h_len;
                        if pos < out.len() {
                            out[pos] = b'.';
                            pos += 1;
                        }
                        unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
                    } else {
                        "during system execution."
                    }
                }
            }
            Some(ResetReason::Wdog) => "stopped responding.",
            Some(ResetReason::Tags) => "detected in system memory.",
            Some(ResetReason::Brow) => "battery supply voltage.",
            None => "reset during operation.",
        }
    }
}

/// Compact 32-bit backup-domain crash/reboot tag stored in PY32F071 `RTC_ALRH/ALRL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupCrashTag {
    pub strikes: u8,
    pub flags: u8,
    pub reason_code: u8,
}

impl BackupCrashTag {
    pub const IDENT_BYTE: u8 = 0xA5;
    pub const FLAG_CLEAN_REBOOT: u8 = 1 << 0;
    pub const FLAG_DIRTY_CRASH: u8 = 1 << 1;
    pub const FLAG_SAFE_MODE_REQ: u8 = 1 << 2;
    pub const FLAG_BOOTLOADER_HOLD: u8 = 1 << 3;
    /// Node runs headless: physical I/O locked, RigWire is the sole control plane.
    pub const FLAG_HEADLESS: u8 = 1 << 4;
    pub const FLAG_DFU_TOOLS_REQ: u8 = 1 << 6;

    pub const fn empty() -> Self {
        Self {
            strikes: 0,
            flags: 0,
            reason_code: 0,
        }
    }

    pub const fn decode(raw: u32) -> Option<Self> {
        if ((raw >> 24) as u8) != Self::IDENT_BYTE {
            return None;
        }
        Some(Self {
            strikes: ((raw >> 16) & 0xFF) as u8,
            flags: ((raw >> 8) & 0xFF) as u8,
            reason_code: (raw & 0xFF) as u8,
        })
    }

    pub const fn encode(&self) -> u32 {
        ((Self::IDENT_BYTE as u32) << 24)
            | ((self.strikes as u32) << 16)
            | ((self.flags as u32) << 8)
            | (self.reason_code as u32)
    }
}

/// 256-byte double-buffered BOOT Record stored in `.uninit` SRAM.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BootRecord {
    pub slot_a: BootSlot,
    pub slot_b: BootSlot,
    pub active_slot: u8,
}

impl BootRecord {
    pub const fn new() -> Self {
        Self {
            slot_a: BootSlot::empty(),
            slot_b: BootSlot::empty(),
            active_slot: 0,
        }
    }

    /// Read the latest valid slot (if any).
    pub fn latest_valid_slot(&self) -> Option<&BootSlot> {
        let a_valid = self.slot_a.is_valid();
        let b_valid = self.slot_b.is_valid();

        match (a_valid, b_valid) {
            (true, true) => {
                if self.active_slot == 1 {
                    Some(&self.slot_b)
                } else if self.active_slot == 0 {
                    Some(&self.slot_a)
                } else if self.slot_b.crash_count > self.slot_a.crash_count {
                    Some(&self.slot_b)
                } else {
                    Some(&self.slot_a)
                }
            }
            (true, false) => Some(&self.slot_a),
            (false, true) => Some(&self.slot_b),
            (false, false) => None,
        }
    }

    /// Prepare next slot for in-place writing using A/B flip.
    pub fn begin_write(&mut self) -> &mut BootSlot {
        let prev_crashes = self.latest_valid_slot().map_or(0, |s| s.crash_count);
        if self.active_slot > 1 {
            self.active_slot = if self.slot_a.is_valid() && !self.slot_b.is_valid() {
                0
            } else if self.slot_b.is_valid() && !self.slot_a.is_valid() {
                1
            } else {
                0
            };
        }
        let slot = if self.active_slot == 0 {
            &mut self.slot_b
        } else {
            &mut self.slot_a
        };
        *slot = BootSlot::empty();
        slot.crash_count = prev_crashes.saturating_add(1);
        slot
    }

    /// Finalize writing the active slot with format identifier and CRC32.
    pub fn commit_write(&mut self) {
        let slot = if self.active_slot == 0 {
            &mut self.slot_b
        } else {
            &mut self.slot_a
        };
        slot.ident = BOOT_RECORD_IDENT;
        slot.crc32 = slot.compute_crc();
        self.active_slot = if self.active_slot == 0 { 1 } else { 0 };
    }

    /// Clear crash loop count on stable boot execution.
    pub fn clear_crash_loop(&mut self) {
        if self.slot_a.is_valid() {
            self.slot_a.crash_count = 0;
            self.slot_a.crc32 = self.slot_a.compute_crc();
        }
        if self.slot_b.is_valid() {
            self.slot_b.crash_count = 0;
            self.slot_b.crc32 = self.slot_b.compute_crc();
        }
    }

    /// Wipe all slots (e.g. factory reset).
    pub fn reset_all(&mut self) {
        self.slot_a = BootSlot::empty();
        self.slot_b = BootSlot::empty();
        self.active_slot = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_slot_empty_is_invalid() {
        let slot = BootSlot::empty();
        assert!(!slot.is_valid());
        assert!(!slot.is_crash());
        assert_eq!(slot.reason(), None);
        assert_eq!(slot.natural_cause(), "due to an unexpected hardware");
    }

    #[test]
    fn test_boot_slot_commit_and_crc_validation() {
        let mut record = BootRecord::new();
        assert!(record.latest_valid_slot().is_none());

        {
            let slot = record.begin_write();
            slot.reset_reason = ResetReason::Appf as u32;
            slot.set_actor(b"tasks.rs");
            slot.panic_line = 42;
            slot.fault_pc = 0x0800_1234;
            assert_eq!(slot.crash_count, 1);
        }
        record.commit_write();

        let valid = record.latest_valid_slot().expect("Slot must be valid");
        assert!(valid.is_valid());
        assert!(valid.is_crash());
        assert_eq!(valid.reason(), Some(ResetReason::Appf));
        assert_eq!(valid.actor_str(), "tasks.rs");
        assert_eq!(valid.natural_cause(), "due to a software panic in");

        let mut buf = [0u8; 32];
        assert_eq!(valid.natural_detail(&mut buf), "tasks.rs at line 42.");
    }

    #[test]
    fn test_boot_record_double_buffering_and_crash_count() {
        let mut record = BootRecord::new();

        // 1st Crash
        {
            let slot = record.begin_write();
            slot.reset_reason = ResetReason::Hard as u32;
            slot.set_actor(b"DACCVIOL");
            slot.fault_pc = 0x0800_4568;
        }
        record.commit_write();

        assert_eq!(record.latest_valid_slot().unwrap().crash_count, 1);
        assert_eq!(
            record.latest_valid_slot().unwrap().natural_cause(),
            "due to an invalid data access"
        );

        let mut buf = [0u8; 32];
        assert_eq!(
            record.latest_valid_slot().unwrap().natural_detail(&mut buf),
            "at program address 0x08004568."
        );

        // 2nd Crash (reboot loop)
        {
            let slot = record.begin_write();
            slot.reset_reason = ResetReason::Wdog as u32;
            slot.set_actor(b"WATCHDOG");
        }
        record.commit_write();

        let slot2 = record.latest_valid_slot().unwrap();
        assert_eq!(slot2.crash_count, 2);
        assert_eq!(slot2.reason(), Some(ResetReason::Wdog));
        assert_eq!(slot2.natural_cause(), "because the system froze or");
        assert_eq!(slot2.natural_detail(&mut buf), "stopped responding.");

        // Clear crash loop preserves slot forensic data
        record.clear_crash_loop();
        let slot_cleared = record.latest_valid_slot().unwrap();
        assert!(slot_cleared.is_valid());
        assert_eq!(slot_cleared.crash_count, 0);
        assert_eq!(slot_cleared.reason(), Some(ResetReason::Wdog));
    }

    #[test]
    fn test_boot_slot_hardfault_actor_mapping() {
        let actors: &[(&[u8], &str)] = &[
            (b"STACK_OVF", "because call stack memory"),
            (b"DACCVIOL", "due to an invalid data access"),
            (b"PRECISERR", "due to a hardware bus error"),
            (b"UNDEFINSTR", "due to an illegal instruction"),
            (b"UNALIGNED", "due to an unaligned memory access"),
            (b"IBUSERR", "due to an instruction fetch failure"),
            (b"INVSTATE", "due to an invalid CPU execution state"),
            (b"INVPC", "due to an invalid program counter"),
            (b"STKERR", "due to an exception stack error"),
            (b"UNSTKERR", "due to an exception stack error"),
            (b"DEBUGEVT", "after a debug breakpoint halted"),
            (b"WATCHDOG", "because the system froze and"),
            (b"FORCED", "due to an unexpected CPU fault"),
            (b"CUSTOM_ACTOR", "due to an unexpected CPU fault"),
        ];

        for &(tag, expected_cause) in actors {
            let mut slot = BootSlot::empty();
            slot.reset_reason = ResetReason::Hard as u32;
            slot.set_actor(tag);
            slot.ident = BOOT_RECORD_IDENT;
            slot.crc32 = slot.compute_crc();

            assert_eq!(slot.natural_cause(), expected_cause);
        }
    }

    #[test]
    fn test_boot_slot_all_reset_reasons() {
        let mut slot = BootSlot::empty();
        slot.ident = BOOT_RECORD_IDENT;

        slot.reset_reason = ResetReason::Tags as u32;
        slot.crc32 = slot.compute_crc();
        assert_eq!(slot.natural_cause(), "due to corrupted data");

        slot.reset_reason = ResetReason::Brow as u32;
        slot.crc32 = slot.compute_crc();
        assert_eq!(slot.natural_cause(), "due to a sudden drop in");

        slot.reset_reason = ResetReason::Wdog as u32;
        slot.crc32 = slot.compute_crc();
        assert_eq!(slot.natural_cause(), "because the system froze or");
    }

    #[test]
    fn test_corrupted_slot_rejection() {
        let mut record = BootRecord::new();
        {
            let slot = record.begin_write();
            slot.reset_reason = ResetReason::Hard as u32;
            slot.set_actor(b"DACCVIOL");
        }
        record.commit_write();

        assert!(record.latest_valid_slot().is_some());

        // Corrupt active slot CRC
        record.slot_b.crc32 ^= 0x1234_5678;
        assert!(!record.slot_b.is_valid());
        assert!(record.latest_valid_slot().is_none());
    }

    #[test]
    fn test_boot_slot_natural_language_formatting() {
        let mut slot = BootSlot::empty();
        slot.ident = BOOT_RECORD_IDENT;

        // 1. Panic
        slot.reset_reason = ResetReason::Appf as u32;
        slot.set_actor(b"tasks.rs");
        slot.panic_line = 42;
        assert_eq!(slot.natural_cause(), "due to a software panic in");
        let mut buf = [0u8; 40];
        assert_eq!(slot.natural_detail(&mut buf), "tasks.rs at line 42.");

        // 2. Memory access fault
        slot.reset_reason = ResetReason::Hard as u32;
        slot.set_actor(b"DACCVIOL");
        slot.panic_file_hash = 0x2000_3FE0;
        assert_eq!(slot.natural_cause(), "due to an invalid data access");
        assert_eq!(
            slot.natural_detail(&mut buf),
            "at memory address 0x20003FE0."
        );

        // 3. Stack overflow
        slot.set_actor(b"STACK_OVF");
        slot.panic_file_hash = 0;
        assert_eq!(slot.natural_cause(), "because call stack memory");
        assert_eq!(slot.natural_detail(&mut buf), "overflowed available RAM.");

        // 4. Watchdog
        slot.reset_reason = ResetReason::Wdog as u32;
        slot.set_actor(b"WATCHDOG");
        assert_eq!(slot.natural_cause(), "because the system froze or");
        assert_eq!(slot.natural_detail(&mut buf), "stopped responding.");

        // 5. Brownout
        slot.reset_reason = ResetReason::Brow as u32;
        assert_eq!(slot.natural_cause(), "due to a sudden drop in");
        assert_eq!(slot.natural_detail(&mut buf), "battery supply voltage.");
    }
}
