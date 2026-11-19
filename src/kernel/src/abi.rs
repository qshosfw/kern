//! Declarative Unified Kernel Operation Table (`abi!`).
//!
//! Single source of truth for KRX `kern_call(op, a0, a1, a2)` and RigWire operation IDs,
//! sub-opcode constants (`sub::*`), argument signatures (`Sig`), and required capability tokens (`Cap`).
//! Operation IDs are packed as `(domain << 4) | index` (16 domains x 16 operations).
//! Run `python3 tools/gen_abi.py` after edits to regenerate `packages/sdk/krx_ops.h`.

use crate::caps::Cap;

/// Argument kind encoded in 2 bits per parameter slot inside `Sig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArgKind {
    /// Immediate scalar value (`usize` / `u32`).
    Val = 0,
    /// Read-only guest buffer or request struct pointer (`*const u8`).
    In = 1,
    /// Writable guest buffer pointer (`*mut u8`).
    Out = 2,
    /// Read-only UTF-8/ASCII string pointer (`*const u8`).
    Str = 3,
}

/// Compact 1-byte argument signature for 3 `kern_call` parameters (`a0`, `a1`, `a2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Sig(pub u8);

impl Sig {
    pub const VAL: Self = Self(0);

    #[inline(always)]
    pub const fn pack(a0: ArgKind, a1: ArgKind, a2: ArgKind) -> Self {
        Self((a0 as u8) | ((a1 as u8) << 2) | ((a2 as u8) << 4))
    }

    #[inline(always)]
    pub const fn arg(self, idx: u8) -> ArgKind {
        match (self.0 >> (idx * 2)) & 0x03 {
            0 => ArgKind::Val,
            1 => ArgKind::In,
            2 => ArgKind::Out,
            _ => ArgKind::Str,
        }
    }

    /// Validates that any pointer-typed argument (`In`, `Out`, `Str`) is non-null.
    #[inline(always)]
    pub const fn check_ptrs(self, a0: usize, a1: usize, a2: usize) -> bool {
        let args = [a0, a1, a2];
        let mut i = 0u8;
        while i < 3 {
            if !matches!(self.arg(i), ArgKind::Val) && args[i as usize] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Compact 4-byte operation metadata entry in `.rodata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OpDesc {
    pub id: u8,
    pub sig: Sig,
    pub cap: Option<Cap>,
    pub _pad: u8,
}

/// Declares the sorted kernel operation table (`Op` enum, `DOMAIN_*` IDs, and `OP_TABLE`).
#[macro_export]
macro_rules! abi {
    (
        $(
            $domain:ident $dom_id:literal {
                $( $name:ident = $idx:literal ( $a0:ident, $a1:ident, $a2:ident ) $( [ $cap:ident ] )? ),* $(,)?
            }
        )*
    ) => {
        pub mod domain {
            $(
                pub const $domain: u8 = $dom_id;
            )*
        }

        pub mod idx {
            $(
                $(
                    #[allow(non_upper_case_globals)]
                    pub const $name: u8 = $idx;
                )*
            )*
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        #[allow(non_camel_case_types)]
        pub enum Op {
            $(
                $(
                    $name = ($dom_id << 4) | $idx,
                )*
            )*
        }

        pub const OP_TABLE: &[OpDesc] = &[
            $(
                $(
                    OpDesc {
                        id: ($dom_id << 4) | $idx,
                        sig: Sig::pack(ArgKind::$a0, ArgKind::$a1, ArgKind::$a2),
                        cap: $crate::abi!(@cap $($cap)?),
                        _pad: 0,
                    },
                )*
            )*
        ];

        impl Op {
            #[inline(always)]
            pub const fn id(self) -> u8 {
                self as u8
            }

            #[inline(always)]
            pub const fn u32_id(self) -> u32 {
                self as u8 as u32
            }

            #[inline(always)]
            pub const fn domain(self) -> u8 {
                (self as u8) >> 4
            }

            #[inline(always)]
            pub const fn index(self) -> u8 {
                (self as u8) & 0x0F
            }

            /// Lookup `OpDesc` by numeric operation ID via linear scan over sorted compact table.
            #[inline(never)]
            pub fn find_desc(id: u8) -> Option<&'static OpDesc> {
                for desc in OP_TABLE {
                    if desc.id == id {
                        return Some(desc);
                    }
                }
                None
            }
        }
    };
    (@cap) => { None };
    (@cap $cap:ident) => { Some(Cap::$cap) };
}

/// Declares typed sub-opcode namespaces mirrored into `packages/sdk/krx_ops.h`.
macro_rules! abi_sub {
    (
        $(
            $group:ident {
                $( $name:ident = $val:literal ),* $(,)?
            }
        )*
    ) => {
        pub mod sub {
            $(
                pub mod $group {
                    $( pub const $name: u8 = $val; )*
                }
            )*
        }
    };
}

abi! {
    SYS 0x0 {
        SysInfo    = 0 (Out, Val, Val),
        SysQuery   = 1 (Val, Val, Val),
        SysDiv     = 2 (Val, Val, Val),
        SysIdiv    = 3 (Val, Val, Val),
        SysReboot  = 4 (Val, Val, Val) [PowerManage],
        SysEvent   = 5 (Val, Val, Val),
        SysFlush   = 6 (Val, Val, Val),
    }
    PROC 0x1 {
        ProcSelf   = 0 (Val, Val, Val),
        ProcFind   = 1 (Str, Val, Val),
        ProcCount  = 2 (Val, Val, Val),
        ProcInfo   = 3 (Val, Out, Val),
        ProcCtl    = 4 (Val, Val, Val),
        ProcLeaf   = 5 (Val, Val, Val),
        ProcPaged  = 6 (Val, Val, Val),
        ProcLease  = 7 (Val, Val, Val),
        ProcMem    = 8 (Val, Out, Val),
        ProcSvc    = 9 (Val, Val, Val),
    }
    IPC 0x2 {
        IpcSend    = 0 (In,  Val, Val),
        IpcRecv    = 1 (Out, Val, Val),
        IpcReply   = 2 (In,  In,  Val),
        IpcPoll    = 3 (Out, Val, Val),
        IpcSendStr = 4 (In,  Val, Val),
    }
    FS 0x3 {
        FsRead     = 0 (Val, In,  Val),
        FsWrite    = 1 (Val, In,  Val),
        FsMeta     = 2 (Val, Val, Val),
        FsRename   = 3 (In,  Val, Val),
    }
    RF 0x4 {
        RfTune     = 0 (Val, Val, Val),
        RfGet      = 1 (Val, Val, Val),
        RfSet      = 2 (Val, Val, Val),
        RfTones    = 3 (Val, Val, Val),
        RfVfo      = 4 (Val, Val, Val),
        RfBulk     = 5 (Val, Val, Val),
    }
    AUD 0x5 {
        AudTone    = 0 (Val, Val, Val),
        AudCtl     = 1 (Val, Val, Val),
    }
    GFX 0x6 {
        GfxClear   = 0 (Val, Val, Val),
        GfxDraw    = 1 (Val, Val, Val),
        GfxText    = 2 (In,  Val, Val),
        GfxCommit  = 3 (Val, Val, Val),
        GfxFb      = 4 (Val, Val, Val),
        GfxCtl     = 5 (Val, Val, Val),
    }
    UI 0x7 {
        UiModal    = 0 (Val, Val, Val),
        UiStatus   = 1 (Val, Val, Val),
        UiWidget   = 2 (Val, In,  Val),
    }
    IO 0x8 {
        IoKey      = 0 (Val, Val, Val),
        IoPower    = 1 (Val, Val, Val),
        IoSerial   = 2 (Val, Val, Val),
        IoTime     = 3 (Val, Val, Val),
        IoBus      = 4 (Val, Val, Val),
    }
}

abi_sub! {
    sys_query {
        OS_VERSION = 0,
        STACK_FREE = 1,
        CPU_HZ     = 2,
        DEVICE_UID = 3,
        RAND32     = 4,
        ISQRT      = 5,
        CRC32      = 6,
        CRC16      = 7,
    }
    proc_paged {
        CALL       = 0,
        RETURN     = 1,
        FAULT      = 2,
    }
    proc_svc {
        REGISTER   = 0,
        RESOLVE    = 1,
        RELEASE    = 2,
    }
    fs_read {
        FILE_AT    = 0,
        LOAD_CFG   = 1,
        ASSET      = 2,
        LIST_DIR   = 3,
        EXTENTS    = 4,
    }
    fs_write {
        FILE       = 0,
        APPEND     = 1,
        SAVE_CFG   = 2,
    }
    fs_meta {
        SIZE       = 0,
        EXISTS     = 1,
        REMOVE     = 2,
        MKDIR      = 3,
        FORMAT     = 4,
    }
    rf_get {
        FREQ       = 0,
        RSSI       = 1,
        CARRIER    = 2,
        BANDWIDTH  = 3,
        MODULATION = 4,
        TX_STATE   = 5,
        GLITCH     = 6,
        SNR        = 7,
        NOISE      = 8,
        FM_RSSI    = 9,
        AFC        = 10,
        AF_FREQ    = 11,
        AF_AMP     = 12,
        CTCSS_SCAN = 13,
    }
    rf_set {
        MODULATION = 0,
        SQUELCH    = 1,
        TX_POWER   = 2,
        AF_MODE    = 3,
        MIC_MUTE   = 4,
        AGC        = 5,
        TX         = 6,
        TAIL       = 7,
        SUBAUDIBLE = 8,
        DTMF       = 9,
        BURST      = 10,
        UNLOCK_VFO = 11,
        FM_TUNE    = 12,
        FM_VOLUME  = 13,
        FM_MUTE    = 14,
    }
    rf_vfo {
        GET_CFG    = 0,
        SET_CFG    = 1,
        STATE      = 2,
        WATCH_MODE = 3,
        LOCK       = 4,
        MONITOR    = 5,
    }
    rf_bulk {
        SWEEP      = 0,
        SYMBOLS    = 1,
        FSK_INIT   = 2,
        FSK_SEND   = 3,
        FSK_RECV   = 4,
    }
    aud_ctl {
        PATH       = 0,
        SET_VOL    = 1,
        GET_VOL    = 2,
        IS_PLAYING = 3,
        MIC_GAIN   = 4,
        MIC_SAMPLE = 5,
        CAP_START  = 6,
        CAP_READ   = 7,
        CAP_STOP   = 8,
        IS_CAP     = 9,
    }
    gfx_draw {
        PIXEL      = 0,
        LINE       = 1,
        RECT       = 2,
        FILL_RECT  = 3,
        HLINE      = 4,
        VLINE      = 5,
        CLR_CORNER = 6,
        DITHER     = 7,
        INVERT     = 8,
        CHAR       = 9,
        ICON       = 10,
        SCROLL     = 11,
        DOTTED_H   = 12,
    }
    gfx_ctl {
        INVERTED   = 0,
        CONTRAST   = 1,
        BITMAP     = 2,
    }
    ui_modal {
        CLOSE      = 0,
        ACTIVE     = 1,
        ANIMATING  = 2,
        SET_HOLD   = 3,
        GET_TEXT   = 4,
        SHOW       = 5,
        TOAST      = 6,
    }
    ui_status {
        SET_BAR    = 0,
        GET_BAR    = 1,
        TRAY_ICON  = 2,
    }
    io_key {
        POLL       = 0,
        NAV_DIR    = 1,
        MATRIX     = 2,
        IS_PRESSED = 3,
        IS_F       = 4,
        CLEAR      = 5,
        IS_LOCKED  = 6,
    }
    io_power {
        BATT_MV    = 0,
        BATT_PCT   = 1,
        BATT_MA    = 2,
        BATT_TYPE  = 3,
        BATT_ADC   = 4,
        MCU_TEMP   = 5,
        CHARGING   = 6,
        LOW        = 7,
        CRITICAL   = 8,
        BL_MODE    = 9,
        BL_LEVELS  = 10,
        BL_WAKE    = 11,
        FLASHLIGHT = 12,
    }
    io_serial {
        WRITE      = 0,
        READ       = 1,
        CONNECTED  = 2,
        FLUSH      = 3,
    }
    io_time {
        TICKS_MS   = 0,
        UPTIME_S   = 1,
        DELAY_MS   = 2,
        DELAY_US   = 3,
        RTC_GET    = 4,
    }
    io_bus {
        I2C        = 0,
        SPI        = 1,
        GPIO_SET   = 2,
        GPIO_GET   = 3,
        RF_RD      = 4,
        RF_WR      = 5,
        FLASH_RD   = 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_table_ids_are_unique_and_sorted() {
        for w in OP_TABLE.windows(2) {
            assert!(
                w[0].id < w[1].id,
                "OP_TABLE must be strictly sorted: {} vs {}",
                w[0].id,
                w[1].id
            );
        }
        assert_eq!(Op::SysInfo.domain(), domain::SYS);
        assert_eq!(Op::RfSet.domain(), domain::RF);
        assert_eq!(Op::RfSet.index(), 2);
        assert_eq!(
            Op::find_desc(Op::SysReboot.id()).unwrap().cap,
            Some(Cap::PowerManage)
        );
        assert_eq!(Op::find_desc(0xFF), None);
    }

    #[test]
    fn sig_packs_and_validates_pointer_args() {
        let sig = Sig::pack(ArgKind::Val, ArgKind::Out, ArgKind::Str);
        assert_eq!(sig.arg(0), ArgKind::Val);
        assert_eq!(sig.arg(1), ArgKind::Out);
        assert_eq!(sig.arg(2), ArgKind::Str);
        assert!(sig.check_ptrs(0, 0x2000_0100, 0x2000_0200));
        assert!(!sig.check_ptrs(0, 0, 0x2000_0200));
        assert!(!sig.check_ptrs(0, 0x2000_0100, 0));
    }
}
