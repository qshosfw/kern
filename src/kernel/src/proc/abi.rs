//! Unified Single-Entry KRX C-ABI Specification (`kern_call`).
//!
//! Exposes one 16-byte (ARM32) `KrxApiTable` descriptor carrying a single AAPCS-Thumb
//! function pointer `call(op, a0, a1, a2) -> usize` routed through the declarative
//! `::abi::Op` table. Multi-argument operations pass compact `#[repr(C)]`
//! stack descriptors (`KrxTextReq`, `KrxFsReq`, `KrxModalReq`, `KrxBusReq`,
//! `KrxIpcSendReq`, `KrxSweepReq`, `KrxWidget`) so every call stays strictly
//! within the 4 AAPCS argument registers (`r0..r3`).

pub use crate::ipc::{KrxIpcMessage, ipc_flags, ipc_msg_type, ipc_target};
use ::abi::Op;

/// `KrxApiTable::abi_version`: `abi::VERSION` in the major half-word.
pub const KRX_ABI_VERSION: u32 = (::abi::VERSION as u32) << 16;

// Standard Service Package Interface IDs (0x0100+ for package services).
pub const INTERFACE_SVC_AX25: u16 = 0x0100;
pub const INTERFACE_SVC_CW: u16 = 0x0101;
pub const INTERFACE_SVC_DTMF: u16 = 0x0102;
pub const INTERFACE_SVC_SPECTRUM: u16 = 0x0103;
pub const INTERFACE_SVC_DATA: u16 = 0x0104;
pub const INTERFACE_SVC_TRIGGER: u16 = 0x0105;

// ── Lifecycle & Event Descriptors ────────────────────────────────────────────

pub const KRX_OP_START: u32 = 0;
pub const KRX_OP_EVENT: u32 = 1;
pub const KRX_OP_RENDER: u32 = 2;
pub const KRX_OP_EXIT: u32 = 3;

pub const KRX_EV_NONE: u8 = 0;
pub const KRX_EV_KEY: u8 = 1;
pub const KRX_EV_ENCODER: u8 = 2;
pub const KRX_EV_TICK: u8 = 3;
pub const KRX_EV_BUS: u8 = 4;
pub const KRX_EV_USB: u8 = 5;
pub const KRX_EV_POWER: u8 = 6;
pub const KRX_EV_MODAL: u8 = 7;
pub const KRX_EV_IPC: u8 = 8;
pub const KRX_IPC_ACTION_FLAG: u8 = 0xAC;

pub const MODAL_RESULT_DISMISSED: u8 = 0;
pub const MODAL_RESULT_CONFIRMED: u8 = 1;
pub const MODAL_RESULT_TEXT_DONE: u8 = 2;
pub const MODAL_RESULT_SHEET_SELECTED: u8 = 3;
pub const MODAL_RESULT_CANCELED: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxEvent {
    pub kind: u8,
    pub key: u8,
    pub flags: u8,
    pub reserved: u8,
    pub payload: u32,
}

impl KrxEvent {
    #[inline(always)]
    pub const fn is_key(&self) -> bool {
        self.kind == KRX_EV_KEY
    }

    #[inline(always)]
    pub const fn is_encoder(&self) -> bool {
        self.kind == KRX_EV_ENCODER
    }

    #[inline(always)]
    pub const fn is_tick(&self) -> bool {
        self.kind == KRX_EV_TICK
    }

    #[inline(always)]
    pub const fn is_bus(&self) -> bool {
        self.kind == KRX_EV_BUS
    }

    #[inline(always)]
    pub const fn is_radio_bus(&self) -> bool {
        self.kind == KRX_EV_BUS && self.flags == (crate::ipc::Topic::RADIO.0 & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn is_usb(&self) -> bool {
        self.kind == KRX_EV_USB
    }

    #[inline(always)]
    pub const fn is_power(&self) -> bool {
        self.kind == KRX_EV_POWER
    }

    #[inline(always)]
    pub const fn is_modal(&self) -> bool {
        self.kind == KRX_EV_MODAL
    }

    #[inline(always)]
    pub const fn is_ipc(&self) -> bool {
        self.kind == KRX_EV_IPC
    }
}

// ── Single-Entry System Call Table ───────────────────────────────────────────

/// Unified AAPCS-Thumb system call entry point (`r0..r3 -> r0` on Cortex-M0+).
pub type KernCallFn = unsafe extern "C" fn(op: u32, a0: usize, a1: usize, a2: usize) -> usize;

/// Standard native KRX application entry point signature.
pub type KrxNativeEntryFn =
    unsafe extern "C" fn(op: u32, api: *const KrxApiTable, arg: *const ()) -> u32;

/// Compact 16-byte ($2^4$ on ARM32) KRX API handle passed in `r1` to `KrxNativeEntryFn`.
#[repr(C)]
pub struct KrxApiTable {
    pub abi_version: u32,
    pub table_size: u16,
    pub _pad0: u16,
    pub granted_caps: u32,
    pub call: KernCallFn,
}

unsafe impl Sync for KrxApiTable {}
unsafe impl Send for KrxApiTable {}

impl KrxApiTable {
    /// Constructs a safe no-op stub table for host unit tests and dry runs.
    pub const fn safe_stub(granted_caps: u32) -> Self {
        Self {
            abi_version: KRX_ABI_VERSION,
            table_size: core::mem::size_of::<Self>() as u16,
            _pad0: 0,
            granted_caps,
            call: stub_kern_call,
        }
    }

    /// Invokes a typed `Op` through this table's single `kern_call` entry point.
    #[inline(always)]
    pub unsafe fn invoke(&self, op: Op, a0: usize, a1: usize, a2: usize) -> usize {
        unsafe { (self.call)(op.u32_id(), a0, a1, a2) }
    }
}

unsafe extern "C" fn stub_kern_call(op: u32, a0: usize, _a1: usize, _a2: usize) -> usize {
    match op as u8 {
        x if x == Op::SysQuery.id() => match a0 {
            0 => KRX_ABI_VERSION as usize,
            1 => 1024,
            2 => 48_000_000,
            _ => 0,
        },
        x if x == Op::ProcLeaf.id() => 0xFFFF_FFFF,
        x if x == Op::ProcSvc.id() => usize::from((a0 & 0xFF) != 1),
        x if x == Op::IpcPoll.id() => 0xFF,
        x if x == Op::FsMeta.id() => usize::from(matches!(a0, 2 | 3 | 4)),
        x if x == Op::RfGet.id() => {
            if a0 == 1 {
                (-140i16 as i32 as u32) as usize
            } else {
                0
            }
        }
        x if x == Op::RfSet.id() => usize::from(matches!(a0, 6 | 8 | 10 | 11)),
        x if x == Op::AudCtl.id() => usize::from(a0 == 3),
        x if x == Op::GfxDraw.id() => {
            if (a0 & 0xFF) == 5 {
                6
            } else {
                0
            }
        }
        x if x == Op::IoPower.id() => match a0 {
            0 => 4000,
            5 => 250,
            _ => 0,
        },
        x if x == Op::IoTime.id() => usize::from(a0 == 3),
        x if x == Op::IoBus.id() => usize::from(matches!(a0, 0 | 1 | 6)),
        x if x == Op::SysInfo.id()
            || x == Op::ProcInfo.id()
            || x == Op::ProcCtl.id()
            || x == Op::ProcLease.id()
            || x == Op::ProcMem.id()
            || x == Op::IpcSend.id()
            || x == Op::IpcRecv.id()
            || x == Op::IpcReply.id()
            || x == Op::IpcSendStr.id()
            || x == Op::FsWrite.id()
            || x == Op::FsRename.id()
            || x == Op::RfTune.id()
            || x == Op::RfVfo.id()
            || x == Op::UiWidget.id() =>
        {
            1
        }
        _ => 0,
    }
}

// ── Compact Request Descriptors (Pointers-First, Zero Hidden Padding) ────────

/// Text draw / measure request (`Op::GfxText`).
/// `flags`: bit 0 = measure only, bit 1 = clip to `arg` (`max_w`), otherwise `arg` = align.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxTextReq {
    pub text: *const u8,
    pub x: i16,
    pub y: i16,
    pub len: u16,
    pub arg: i16,
    pub font: u8,
    pub color: u8,
    pub flags: u8,
    pub _pad: u8,
}

/// Filesystem read/write/rename/list request (`Op::FsRead`, `Op::FsWrite`, `Op::FsRename`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxFsReq {
    pub path: *const u8,
    pub buf: *mut u8,
    pub offset: u32,
    pub len: u16,
    pub path_len: u8,
    pub extra: u8,
}

/// Modal popup/confirm/text-prompt request (`Op::UiModal`, sub = 5).
/// `kind`: 0 = popup, 1 = confirm, 2 = text prompt.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxModalReq {
    pub title: *const u8,
    pub msg: *const u8,
    pub ok: *const u8,
    pub cancel: *const u8,
    pub tag: u8,
    pub kind: u8,
    pub t_len: u8,
    pub m_len: u8,
    pub o_len: u8,
    pub c_len: u8,
    pub max_len: u8,
    pub _pad: u8,
}

/// Raw peripheral bus request (`Op::IoBus`, sub = 0 I2C / 1 SPI / 6 flash_read).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxBusReq {
    pub tx: *const u8,
    pub rx: *mut u8,
    pub addr: u32,
    pub tx_len: u16,
    pub rx_len: u16,
}

/// Name-addressed IPC send request (`Op::IpcSendStr`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxIpcSendReq {
    pub target: *const u8,
    pub msg: *const u8,
    pub target_len: u8,
    pub msg_len: u8,
    pub _pad: [u8; 2],
}

/// Frequency sweep request (`Op::RfBulk`, sub = 0).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxSweepReq {
    pub out_rssi: *mut i16,
    pub start_hz: u32,
    pub step_hz: u32,
    pub count: u16,
    pub _pad: u16,
}

// ── UI Widget & Status Bar Descriptors ───────────────────────────────────────

pub const KRX_STATUS_VISIBLE: u8 = 1 << 0;
pub const KRX_STATUS_DIVIDER: u8 = 1 << 1;
pub const KRX_STATUS_HAS_SIGNAL: u8 = 1 << 2;
pub const KRX_STATUS_CUSTOM_TITLE: u8 = 1 << 3;
pub const KRX_STATUS_PARTIAL: u8 = 1 << 4;

pub const KRX_ICON_CUSTOM_0: u8 = 0x80;
pub const KRX_ICON_CUSTOM_1: u8 = 0x81;
pub const KRX_ICON_NONE: u8 = 0xFF;

pub const KRX_CUSTOM_TRAY_PERSIST: u8 = 1 << 0;
pub const KRX_CUSTOM_TRAY_ACTIVE: u8 = 1 << 1;

/// 64-byte ($2^6$) Power-of-Two Aligned C-ABI Status Bar Descriptor.
#[repr(C, align(4))]
#[derive(Debug, Clone, Copy)]
pub struct KrxStatusBar {
    pub flags: u8,
    pub signal_level: u8,
    pub left_icon_count: u8,
    pub center_icon_count: u8,
    pub left_icons: [u8; 4],
    pub center_icons: [u8; 4],
    pub title: [u8; 16],
    pub badge: [u8; 8],
    pub center: [u8; 16],
    pub custom_icon_0: [u8; 5],
    pub custom_icon_1: [u8; 5],
    pub tray_icon_id: u8,
    pub custom_flags: u8,
}

const _: () = assert!(core::mem::size_of::<KrxStatusBar>() == 64);

impl Default for KrxStatusBar {
    fn default() -> Self {
        Self::empty()
    }
}

impl KrxStatusBar {
    pub const EMPTY: Self = Self::empty();

    pub const fn empty() -> Self {
        Self {
            flags: KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER,
            signal_level: 0,
            left_icon_count: 0,
            center_icon_count: 0,
            left_icons: [0; 4],
            center_icons: [0; 4],
            title: [0; 16],
            badge: [0; 8],
            center: [0; 16],
            custom_icon_0: [0; 5],
            custom_icon_1: [0; 5],
            tray_icon_id: 0,
            custom_flags: 0,
        }
    }
}

pub const KRX_WIDGET_ROUNDED_BOX: u8 = 2;
pub const KRX_WIDGET_NEGATED_HEADER: u8 = 3;
pub const KRX_WIDGET_SHEET_FRAME: u8 = 4;
pub const KRX_WIDGET_PROGRESS_BAR: u8 = 5;
pub const KRX_WIDGET_S_METER: u8 = 6;
pub const KRX_WIDGET_TAG_BADGE: u8 = 7;
pub const KRX_WIDGET_KEY_BADGE: u8 = 8;
pub const KRX_WIDGET_CAPSULE_BUTTON: u8 = 9;
pub const KRX_WIDGET_SCROLLBAR: u8 = 10;
pub const KRX_WIDGET_MENU_ITEM: u8 = 11;
pub const KRX_WIDGET_BIG_DIGIT: u8 = 12;
pub const KRX_WIDGET_SMALL_DIGIT: u8 = 13;
pub const KRX_WIDGET_BATTERY: u8 = 14;
pub const KRX_WIDGET_SIGNAL: u8 = 15;
pub const KRX_WIDGET_MODAL_BOX: u8 = 16;

/// Unified widget / bitmap draw descriptor (`Op::UiWidget`, `Op::GfxCtl` bitmap).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxWidget {
    pub p0: *const u8,
    pub p1: *const u8,
    pub x: i16,
    pub y: i16,
    pub len0: u16,
    pub len1: u16,
    pub extra: u16,
    pub w: u8,
    pub h: u8,
    pub a: u8,
    pub b: u8,
    pub _pad: [u8; 2],
}

unsafe impl Sync for KrxWidget {}
unsafe impl Send for KrxWidget {}

// ── Domain Data Structures (ORDS-01 Power-of-Two Geometries) ─────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxSymbol {
    pub freq_word: u16,
    pub duration_us: u16,
}

/// Directory listing entry record with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxDirEntry {
    pub name: [u8; 24],
    pub size: u32,
    pub is_dir: u8,
    pub reserved: [u8; 3],
}

const _: () = assert!(core::mem::size_of::<KrxDirEntry>() == 32);

/// `KrxProcInfo::flags` bitmasks.
pub const KRX_PROC_FLAG_PERSISTENT: u8 = 1 << 0;
pub const KRX_PROC_FLAG_SUSPENDED: u8 = 1 << 1;
pub const KRX_PROC_FLAG_FOREGROUND: u8 = 1 << 2;
pub const KRX_PROC_FLAG_HAS_LEASE: u8 = 1 << 3;
pub const KRX_PROC_FLAG_KILLABLE: u8 = 1 << 4;

/// Process state descriptor record with $O(1)$ power-of-two geometry (32 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxProcInfo {
    pub pid: u8,
    pub app_id: u8,
    pub state: u8,
    pub priority: u8,
    pub runtime_ticks: u32,
    pub memory_bytes: u16,
    pub name: [u8; 12],
    pub exec_type: u8,
    pub flags: u8,
    pub stack_watermark: u16,
    pub ipc_events: u16,
    pub page_faults: u8,
    pub slice_overruns: u8,
    pub hibernations: u8,
    pub fault_count: u8,
}

const _: () = assert!(core::mem::size_of::<KrxProcInfo>() == 32);

/// `Op::ProcCtl` sub-opcodes (`op = a0 & 0xFF`, `target = (a0 >> 8) & 0xFF`, `len = a0 >> 16`).
pub const PROC_CTL_KILL: u8 = 0;
pub const PROC_CTL_SET_STATE: u8 = 1;
pub const PROC_CTL_SET_PRIORITY: u8 = 2;
pub const PROC_CTL_FOCUS_APP: u8 = 3;
pub const PROC_CTL_REVOKE_LEASE: u8 = 4;
pub const PROC_CTL_RESET_STATS: u8 = 5;

/// 16-byte power-of-two record readers (`buf != NULL && len >= 16`).
pub const PROC_CTL_SYS_OVERVIEW: u8 = 0x80;
pub const PROC_CTL_LEASE_ENTRY: u8 = 0x81;
pub const PROC_CTL_SVC_ENTRY: u8 = 0x82;
pub const PROC_CTL_IPC_ENTRY: u8 = 0x83;
pub const PROC_CTL_PROC_EXT: u8 = 0x84;

/// C-ABI live coordinator telemetry snapshot.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KrxRadioState {
    pub active_vfo: u8,
    pub listening_vfo: u8,
    pub dual_watch: u8,
    pub cross_band: u8,
    pub bclo_policy: u8,
    pub ptt_active: u8,
    pub squelch_open: u8,
    pub subaudible_matched: u8,
    pub tot_locked_out: u8,
    pub lock_active: u8,
    pub tx_perm: u8,
    pub _reserved: u8,
    pub tot_remaining_s: u16,
    pub rssi_dbm: i16,
}

pub const KRX_SYS_INFO_FLAG_HAS_CERT: u16 = 1 << 0;
pub const KRX_SYS_INFO_FLAG_RDP_PROTECTED: u16 = 1 << 1;
pub const KRX_SYS_INFO_FLAG_CALIB_VALID: u16 = 1 << 2;

pub const KRX_QS1_NONE: u8 = 0;
pub const KRX_QS1_EMPTY: u8 = 1;
pub const KRX_QS1_INVALID: u8 = 2;
pub const KRX_QS1_UNVERIFIED: u8 = 3;
pub const KRX_QS1_VERIFIED: u8 = 4;

/// Canonical hardware, firmware build, and provisioning identity (`2^7 = 128` bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KrxSysInfo {
    pub flash_kb: u16,
    pub sram_kb: u16,
    pub flags: u16,
    pub mac: [u8; 6],
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub target_id: u8,
    pub model: [u8; 12],
    pub mcu: [u8; 12],
    pub boot_ver: [u8; 8],
    pub git_hash: [u8; 22],
    pub build_date: [u8; 12],
    pub serial: [u8; 15],
    pub qs1_len: u8,
    pub qs1: [u8; 17],
    pub mcu_uid: [u8; 12],
    pub qs1_status: u8,
}

const _: () = assert!(core::mem::size_of::<KrxSysInfo>() == 128);

unsafe impl Sync for KrxSysInfo {}
unsafe impl Send for KrxSysInfo {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_modal_events_and_math() {
        let ev = KrxEvent {
            kind: KRX_EV_MODAL,
            key: 42,
            flags: MODAL_RESULT_CONFIRMED,
            reserved: 0,
            payload: 0,
        };
        assert!(ev.is_modal());
        assert!(!ev.is_key());
    }

    #[test]
    fn test_abi_geometries() {
        assert_eq!(core::mem::size_of::<KrxDirEntry>(), 32);
        assert_eq!(core::mem::align_of::<KrxDirEntry>(), 4);
        assert_eq!(core::mem::size_of::<KrxProcInfo>(), 32);
        assert_eq!(core::mem::align_of::<KrxProcInfo>(), 4);
        assert_eq!(core::mem::size_of::<KrxSysInfo>(), 128);
        assert_eq!(core::mem::align_of::<KrxSysInfo>(), 2);
        assert_eq!(core::mem::size_of::<KrxStatusBar>(), 64);

        #[cfg(target_pointer_width = "32")]
        {
            assert_eq!(core::mem::size_of::<KrxApiTable>(), 16);
            assert_eq!(core::mem::size_of::<KrxTextReq>(), 16);
            assert_eq!(core::mem::size_of::<KrxFsReq>(), 16);
            assert_eq!(core::mem::size_of::<KrxBusReq>(), 16);
            assert_eq!(core::mem::size_of::<KrxSweepReq>(), 16);
            assert_eq!(core::mem::size_of::<KrxWidget>(), 24);
        }
        #[cfg(target_pointer_width = "64")]
        assert_eq!(core::mem::size_of::<KrxApiTable>(), 24);
    }

    #[test]
    fn test_abi_safe_stub_dispatch() {
        let table = KrxApiTable::safe_stub(0x1234);
        assert_eq!(table.abi_version, KRX_ABI_VERSION);
        assert_eq!(table.granted_caps, 0x1234);

        unsafe {
            assert_eq!(
                table.invoke(Op::SysQuery, 0, 0, 0),
                KRX_ABI_VERSION as usize
            );
            assert_eq!(table.invoke(Op::SysQuery, 1, 0, 0), 1024);
            assert_eq!(table.invoke(Op::SysQuery, 2, 0, 0), 48_000_000);
            assert_eq!(table.invoke(Op::RfTune, 145_000_000, 1, 12_500), 1);
            assert_eq!(table.invoke(Op::RfGet, 1, 0, 0) as u32 as i32, -140);
            assert_eq!(table.invoke(Op::IoPower, 0, 0, 0), 4000);
            assert_eq!(table.invoke(Op::IoPower, 5, 0, 0), 250);
            assert_eq!(table.invoke(Op::ProcLeaf, 1, 0, 0), 0xFFFF_FFFF);
            assert_eq!(table.invoke(Op::IpcPoll, 0, 0, 0), 0xFF);
            assert_eq!(table.invoke(Op::UiWidget, 0, 0, 0), 1);
        }
    }

    #[test]
    fn test_krx_event_methods_and_kinds() {
        let ev_key = KrxEvent {
            kind: KRX_EV_KEY,
            key: 15,
            flags: 0,
            reserved: 0,
            payload: 0,
        };
        assert!(ev_key.is_key());
        assert!(!ev_key.is_bus());

        let ev_bus = KrxEvent {
            kind: KRX_EV_BUS,
            key: 1,
            flags: (crate::ipc::Topic::RADIO.0 & 0xFF) as u8,
            reserved: 0,
            payload: 145_000_000,
        };
        assert!(ev_bus.is_bus());
        assert!(ev_bus.is_radio_bus());
        assert!(!ev_bus.is_key());
    }
}
