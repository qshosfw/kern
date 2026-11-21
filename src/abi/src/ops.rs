//! Declarative Unified Kernel Operation Table (`abi!`).
//!
//! Single source of truth for KRX `kern_call(op, a0, a1, a2)` and RigWire operation IDs,
//! sub-opcode constants (`sub::*`), argument signatures (`Sig`), and required capability tokens (`Cap`).
//! Operation IDs are packed as `(domain << 4) | index` (16 domains x 16 operations).
//! Run `./kern abi` after edits to regenerate `sdk/c/krx_ops.h` from these tables.

use crate::Cap;

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

/// One operation domain with its operations, for host-side generators.
#[cfg(feature = "describe")]
pub struct DomainInfo {
    pub name: &'static str,
    pub id: u8,
    pub ops: &'static [OpInfo],
}

/// One operation: Rust identifier, packed ID and required capability identifier.
#[cfg(feature = "describe")]
pub struct OpInfo {
    pub name: &'static str,
    pub id: u8,
    pub cap: Option<&'static str>,
}

/// One sub-opcode namespace with its `(name, value)` entries.
#[cfg(feature = "describe")]
pub struct SubGroupInfo {
    pub name: &'static str,
    pub items: &'static [(&'static str, u8)],
}

/// Declares the sorted kernel operation table (`Op` enum, `DOMAIN_*` IDs, and `OP_TABLE`).
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
                        cap: abi!(@cap $($cap)?),
                        _pad: 0,
                    },
                )*
            )*
        ];

        #[cfg(feature = "describe")]
        pub const DOMAINS: &[DomainInfo] = &[
            $(
                DomainInfo {
                    name: stringify!($domain),
                    id: $dom_id,
                    ops: &[
                        $(
                            OpInfo {
                                name: stringify!($name),
                                id: ($dom_id << 4) | $idx,
                                cap: abi!(@cap_ident $($cap)?),
                            },
                        )*
                    ],
                },
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

            /// `OpDesc` of a numeric operation ID: one table load, no scan.
            #[inline(always)]
            pub fn find_desc(id: u8) -> Option<&'static OpDesc> {
                let dom = (id >> 4) as usize;
                if dom >= DOMAIN_COUNT {
                    return None;
                }
                let at = DOMAIN_BASE[dom] + (id & 0x0F);
                if at < DOMAIN_BASE[dom + 1] {
                    Some(&OP_TABLE[at as usize])
                } else {
                    None
                }
            }
        }

        /// Capability bit each operation requires (`Cap::COUNT` = none), in `OP_TABLE` order.
        const OP_CAP_BIT: [u8; OP_TABLE.len()] = {
            let mut bits = [Cap::COUNT; OP_TABLE.len()];
            let mut i = 0;
            while i < bits.len() {
                if let Some(cap) = OP_TABLE[i].cap {
                    bits[i] = cap.bit();
                }
                i += 1;
            }
            bits
        };

        /// Capabilities operation `id` needs beyond `granted`: `Some(0)` when it may run,
        /// `None` when no such operation exists. Two byte loads, no scan.
        #[inline(always)]
        pub fn missing_caps(id: u8, granted: u32) -> Option<u32> {
            let base = DOMAIN_BASE.get((id >> 4) as usize + 1)?;
            let at = DOMAIN_BASE[(id >> 4) as usize] + (id & 0x0F);
            if at >= *base {
                return None;
            }
            let bit = *OP_CAP_BIT.get(at as usize)?;
            Some(((1u32 << (bit & 31)) & !granted) & ((bit < Cap::COUNT) as u32).wrapping_neg())
        }

        const DOMAIN_COUNT: usize = [$($dom_id),*].len();

        /// Index of each domain's first operation in `OP_TABLE`, plus the table length.
        /// Domains are contiguous from 0 and indices dense from 0, so
        /// `OP_TABLE[DOMAIN_BASE[domain] + index]` is the operation.
        const DOMAIN_BASE: [u8; DOMAIN_COUNT + 1] = {
            let mut base = [OP_TABLE.len() as u8; DOMAIN_COUNT + 1];
            let mut i = OP_TABLE.len();
            while i > 0 {
                i -= 1;
                base[(OP_TABLE[i].id >> 4) as usize] = i as u8;
            }
            i = 0;
            while i < OP_TABLE.len() {
                let id = OP_TABLE[i].id;
                assert!(base[(id >> 4) as usize] + (id & 0x0F) == i as u8, "operation indices must be dense");
                i += 1;
            }
            base
        };
    };
    (@cap) => { None };
    (@cap $cap:ident) => { Some(Cap::$cap) };
    (@cap_ident) => { None };
    (@cap_ident $cap:ident) => { Some(stringify!($cap)) };
}

/// Declares typed sub-opcode namespaces mirrored into `sdk/c/krx_ops.h`.
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

        #[cfg(feature = "describe")]
        pub const SUB_GROUPS: &[SubGroupInfo] = &[
            $(
                SubGroupInfo {
                    name: stringify!($group),
                    items: &[ $( (stringify!($name), $val) ),* ],
                },
            )*
        ];
    };
}

abi! {
    SYS 0x0 {
        SysInfo    = 0 (Out, Val, Val),
        SysQuery   = 1 (Val, Val, Val),
        SysDiv     = 2 (Val, Val, Val),
        SysReboot  = 3 (Val, Val, Val) [PowerManage],
        SysEvent   = 4 (Val, Val, Val),
        SysFlush   = 5 (Val, Val, Val),
        PropGet    = 6 (Val, Val, Val),
        PropSet    = 7 (Val, Val, Val),
        PrefSet    = 8 (Val, Val, Val) [Preferences],
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
        FsRead     = 0 (Val, In,  Val) [StorageRead],
        FsWrite    = 1 (Val, In,  Val) [StorageWrite],
        FsMeta     = 2 (Val, Val, Val) [StorageRead],
        FsRename   = 3 (In,  Val, Val) [StorageWrite],
    }
    RF 0x4 {
        RfTune     = 0 (Val, Val, Val) [RadioRx],
        RfGet      = 1 (Val, Val, Val) [RadioRx],
        RfSet      = 2 (Val, Val, Val) [RadioRx],
        RfTones    = 3 (Val, Val, Val) [RadioRx],
        RfVfo      = 4 (Val, Val, Val) [RadioRx],
        RfBulk     = 5 (Val, Val, Val) [RadioRx],
    }
    AUD 0x5 {
        AudTone    = 0 (Val, Val, Val) [AudioOutput],
        AudCtl     = 1 (Val, Val, Val) [AudioOutput],
    }
    GFX 0x6 {
        GfxClear   = 0 (Val, Val, Val) [DisplayDirect],
        GfxDraw    = 1 (Val, Val, Val) [DisplayDirect],
        GfxText    = 2 (In,  Val, Val) [DisplayDirect],
        GfxCommit  = 3 (Val, Val, Val) [DisplayDirect],
        GfxFb      = 4 (Val, Val, Val) [DisplayDirect],
        GfxCtl     = 5 (Val, Val, Val) [DisplayDirect],
    }
    UI 0x7 {
        UiModal    = 0 (Val, Val, Val) [DisplayDirect],
        UiStatus   = 1 (Val, Val, Val) [DisplayDirect],
        UiWidget   = 2 (Val, In,  Val) [DisplayDirect],
    }
    IO 0x8 {
        IoKey      = 0 (Val, Val, Val) [Keypad],
        IoPower    = 1 (Val, Val, Val),
        IoSerial   = 2 (Val, Val, Val) [SerialComm],
        IoTime     = 3 (Val, Val, Val),
        IoBus      = 4 (Val, Val, Val) [RawPeripheral],
    }
}

abi_sub! {
    sys_query {
        OS_VERSION = 0,
        STACK_FREE = 1,
        CPU_HZ     = 2,
        DEVICE_UID = 3,
        RAND32     = 4,
        CRC32      = 5,
        CRC16      = 6,
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
        GET_CHANNEL = 0,
        SET_CHANNEL = 1,
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
        IS_PLAYING = 1,
        MIC_SAMPLE = 2,
        CAP_START  = 3,
        CAP_READ   = 4,
        CAP_STOP   = 5,
        IS_CAP     = 6,
    }
    gfx_draw {
        LINE       = 0,
        RECT       = 1,
        FILL_RECT  = 2,
        CLR_CORNER = 3,
        DITHER     = 4,
        CHAR       = 5,
        ICON       = 6,
        SCROLL     = 7,
        DOTTED_H   = 8,
    }
    gfx_ctl {
        BITMAP     = 0,
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
        MATRIX     = 1,
        IS_PRESSED = 2,
        IS_F       = 3,
        IS_LOCKED  = 4,
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
        BL_WAKE    = 9,
        FLASHLIGHT = 10,
    }
    io_serial {
        WRITE      = 0,
        READ       = 1,
        CONNECTED  = 2,
        FLUSH      = 3,
    }
    io_time {
        TICKS_MS   = 0,
        DELAY_MS   = 1,
        DELAY_US   = 2,
        RTC_GET    = 3,
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
