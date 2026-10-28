//! BOOT Record (Persistent Crash & Diagnostics Log)
//!
//! Microkernel double-buffered crash report and boot diagnostic record.
//! Survives warm resets in `.uninit` SRAM (256 bytes = Slot A + Slot B).
//! Double-buffered A/B flip with CRC32 integrity verification.

use crypto::crc::CrcEngine;

pub const BOOT_RECORD_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x02, 0xFD];

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
    /// Memory magic-tag corruption (§13.2)
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
        match self {
            Self::Hard => "HARD",
            Self::Appf => "APPF",
            Self::Wdog => "WDOG",
            Self::Tags => "TAGS",
            Self::Brow => "BROW",
        }
    }
}

/// Extended low-level fault forensics & execution state (11 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct FaultForensics {
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
    /// Deterministic 32-bit fault fingerprint for automated grouping & deduplication
    pub crash_signature: u32,
}

impl FaultForensics {
    pub const fn empty() -> Self {
        Self {
            inst_pre: 0,
            fault_reg: 0xFF,
            access_flags: 0,
            ipsr_num: 0,
            exec_mode: 0,
            unwind_count: 0,
            crash_signature: 0,
        }
    }
}

/// 128-byte packed crash log slot (§13.4 Listing 13.2)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BootSlot {
    pub reset_reason: u32,
    pub fault_pc: u32,
    pub fault_lr: u32,
    pub fault_sp: u32,
    pub xpsr: u32,
    pub panic_line: u16,
    pub panic_file_hash: u32,
    pub actor_tag: [u8; 16],
    pub stack_dump: [u32; 16],
    pub crash_count: u8,
    pub forensics: FaultForensics,
    pub rsvd: [u8; 2],
    pub magic: [u8; 4],
    pub crc32: u32,
}

const _: () = assert!(core::mem::size_of::<BootSlot>() == 128);

impl BootSlot {
    pub const fn empty() -> Self {
        Self {
            reset_reason: 0,
            fault_pc: 0,
            fault_lr: 0,
            fault_sp: 0,
            xpsr: 0,
            panic_line: 0,
            panic_file_hash: 0,
            actor_tag: [0; 16],
            stack_dump: [0; 16],
            crash_count: 0,
            forensics: FaultForensics::empty(),
            rsvd: [0; 2],
            magic: [0; 4],
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

    /// Returns true if slot contains valid magic and valid CRC32
    pub fn is_valid(&self) -> bool {
        if self.magic != BOOT_RECORD_MAGIC {
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
        let slot = if self.active_slot == 0 {
            &mut self.slot_b
        } else {
            &mut self.slot_a
        };
        *slot = BootSlot::empty();
        slot.crash_count = prev_crashes.saturating_add(1);
        slot
    }

    /// Finalize writing the active slot with magic and CRC32.
    pub fn commit_write(&mut self) {
        let slot = if self.active_slot == 0 {
            &mut self.slot_b
        } else {
            &mut self.slot_a
        };
        slot.magic = BOOT_RECORD_MAGIC;
        slot.crc32 = slot.compute_crc();
        self.active_slot ^= 1;
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
