//! Declarative Orthogonal Kernel Operation Table (`abi!`, 22 operations across 9 domains).
//!
//! Single source of truth for KRX `kern_call(op, a0, a1, a2)` operation IDs,
//! sub-opcode constants (`sub::*`), argument signatures (`Sig`), and capability tokens (`Cap`).
//! All scalar preferences, live telemetry, and single-register hardware/RF controls route
//! through `Op::SysProp` (`abi::prop`), eliminating duplicate per-domain getter/setter trees.
//! Run `./kern abi` after edits to regenerate `sdk/c/krx_ops.h` from these tables.

use crate::Cap;

/// Argument kind encoded in 2 bits per parameter slot inside `Sig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArgKind {
    Val = 0,
    In = 1,
    Out = 2,
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

/// Bitflags packed into `OpDesc::flags` (`1 B`, zero extra `.rodata` cost).
pub mod op_flags {
    pub const NONE: u8 = 0;
    /// Requires extended Cortex-M0+ pre-flight stack margin (`304 B` vs `176 B`).
    pub const HIGH_STACK: u8 = 1 << 0;
    /// Return register carries an unconstrained 32-bit unsigned bitpattern (e.g. CRC32/RNG/ticks).
    pub const RAW_U32: u8 = 1 << 1;
}

/// Compact 4-byte operation metadata entry in `.rodata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct OpDesc {
    pub id: u8,
    pub sig: Sig,
    pub cap_bit: u8,
    pub flags: u8,
}

const _: () = assert!(core::mem::size_of::<OpDesc>() == 4);

impl OpDesc {
    pub const NO_CAP: u8 = 0xFF;

    #[inline(always)]
    pub const fn cap(self) -> Option<Cap> {
        Cap::from_bit(self.cap_bit)
    }

    #[inline(always)]
    pub const fn cap_mask(self) -> u32 {
        if self.cap_bit < 32 {
            1u32 << self.cap_bit
        } else {
            0
        }
    }
}

#[cfg(feature = "describe")]
pub struct DomainInfo {
    pub name: &'static str,
    pub id: u8,
    pub ops: &'static [OpInfo],
}

#[cfg(feature = "describe")]
pub struct OpInfo {
    pub name: &'static str,
    pub id: u8,
    pub cap: Option<&'static str>,
}

#[cfg(feature = "describe")]
pub struct SubGroupInfo {
    pub name: &'static str,
    pub items: &'static [(&'static str, u8)],
}

macro_rules! abi {
    (
        $(
            $domain:ident $dom_id:literal {
                $( $name:ident = $idx:literal ( $a0:ident, $a1:ident, $a2:ident ) $( [ $cap:ident ] )? $( @ $flags:expr )? ),* $(,)?
            }
        )*
    ) => {
        pub mod domain {
            $( pub const $domain: u8 = $dom_id; )*
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
                        cap_bit: abi!(@cap $($cap)?),
                        flags: abi!(@flags $($flags)?),
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

            #[inline(always)]
            pub fn lookup(id: u8) -> Option<(usize, OpDesc)> {
                let dom = (id >> 4) as usize;
                if dom >= DOMAIN_COUNT {
                    return None;
                }
                let at = DOMAIN_BASE[dom] + (id & 0x0F);
                if at < DOMAIN_BASE[dom + 1] {
                    let idx = at as usize;
                    Some((idx, OP_TABLE[idx]))
                } else {
                    None
                }
            }

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

        #[inline(always)]
        pub fn missing_caps(id: u8, granted: u32) -> Option<u32> {
            let (_, desc) = Op::lookup(id)?;
            Some(desc.cap_mask() & !granted)
        }

        const DOMAIN_COUNT: usize = [$($dom_id),*].len();

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
    (@cap) => { OpDesc::NO_CAP };
    (@cap $cap:ident) => { Cap::$cap.bit() };
    (@cap_ident) => { None };
    (@cap_ident $cap:ident) => { Some(stringify!($cap)) };
    (@flags) => { op_flags::NONE };
    (@flags $flags:expr) => { $flags };
}

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
        SysProp   = 0 (Val, Val, Val) @ op_flags::RAW_U32,
        SysQuery  = 1 (Val, Val, Val) @ op_flags::HIGH_STACK | op_flags::RAW_U32,
        SysCtl    = 2 (Val, Val, Val) @ op_flags::RAW_U32,
        SysDiv    = 3 (Val, Val, Val) @ op_flags::RAW_U32,
        SysSec    = 4 (Val, Val, Val) [Preferences] @ op_flags::HIGH_STACK,
    }
    PROC 0x1 {
        ProcCtl   = 0 (Val, Val, Val) @ op_flags::HIGH_STACK,
        ProcExec  = 1 (Val, Val, Val) @ op_flags::HIGH_STACK,
    }
    IPC 0x2 {
        Ipc       = 0 (Val, Val, Val),
    }
    FS 0x3 {
        FsRead    = 0 (Val, In,  Val) [StorageRead] @ op_flags::HIGH_STACK,
        FsWrite   = 1 (Val, In,  Val) [StorageWrite] @ op_flags::HIGH_STACK,
        FsMeta    = 2 (Val, Val, Val) [StorageRead] @ op_flags::HIGH_STACK,
    }
    RF 0x4 {
        RfTune    = 0 (Val, Val, Val) [RadioRx],
        RfVfo     = 1 (Val, Val, Val) [RadioRx],
        RfBulk    = 2 (Val, Val, Val) [RadioRx],
    }
    AUD 0x5 {
        Aud       = 0 (Val, Val, Val) [AudioOutput],
    }
    GFX 0x6 {
        GfxDraw   = 0 (Val, Val, Val) [DisplayDirect] @ op_flags::RAW_U32,
        GfxText   = 1 (In,  Val, Val) [DisplayDirect],
    }
    UI 0x7 {
        UiModal   = 0 (Val, Val, Val) [DisplayDirect] @ op_flags::HIGH_STACK,
        UiWidget  = 1 (Val, In,  Val) [DisplayDirect] @ op_flags::HIGH_STACK,
    }
    IO 0x8 {
        IoKey     = 0 (Val, Val, Val) [Keypad] @ op_flags::RAW_U32,
        IoSerial  = 1 (Val, Val, Val) [SerialComm],
        IoBus     = 2 (Val, Val, Val) [RawPeripheral],
    }
}

abi_sub! {
    sys_prop {
        GET        = 0,
        SET        = 1,
        PREF       = 2,
    }
    sys_query {
        INFO       = 0,
        DEVICE_UID = 1,
        CRC32      = 2,
        CRC16      = 3,
        RTC_GET    = 4,
        FLUSH      = 5,
        RELOAD     = 6,
        RESET      = 7,
    }
    sys_ctl {
        DELAY_MS   = 0,
        DELAY_US   = 1,
        EVENT      = 2,
        REBOOT     = 3,
    }
    sys_sec {
        STATUS     = 0,
        SET_OPTS   = 1,
        VERIFY     = 2,
        SET        = 3,
    }
    proc_ctl {
        INFO       = 0,
        MEM        = 1,
        LEASE      = 2,
        KILL       = 3,
        LAUNCH_PKG = 4,
        FIND       = 5,
        SVC_REG    = 6,
        SVC_RES    = 7,
        SVC_REL    = 8,
    }
    proc_exec {
        LEAF        = 0,
        PAGED_CALL  = 1,
        PAGED_RET   = 2,
        PAGED_FAULT = 3,
        CALL_VFP    = 4,
    }
    ipc {
        SEND       = 0,
        RECV       = 1,
    }
    fs_read {
        FILE_AT    = 0,
        LOAD_CFG   = 1,
        ASSET      = 2,
        LIST_DIR   = 3,
        EXTENTS    = 4,
        STATS      = 5,
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
        RENAME     = 5,
    }
    rf_tune {
        SUBAUDIBLE = 1,
        DTMF       = 2,
        BURST      = 3,
        TONES_RAW  = 4,
        TONES_HZ   = 5,
    }
    rf_vfo {
        GET_CHANNEL = 0,
        SET_CHANNEL = 1,
        STATE       = 2,
        LOCK        = 3,
    }
    rf_bulk {
        SWEEP      = 0,
        SYMBOLS    = 1,
        FSK_INIT   = 2,
        FSK_SEND   = 3,
        FSK_RECV   = 4,
    }
    aud {
        TONE       = 0,
        CAP_START  = 1,
        CAP_READ   = 2,
        CAP_STOP   = 3,
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
        CLEAR      = 9,
        FB         = 10,
        BITMAP     = 11,
        COMMIT     = 12,
    }
    ui_modal {
        SHOW       = 0,
        TOAST      = 1,
        PROMPT_NUM = 2,
        GET_TEXT   = 3,
        SET_BAR    = 4,
        GET_BAR    = 5,
    }
    io_key {
        POLL       = 0,
        MATRIX     = 1,
        IS_PRESSED = 2,
    }
    io_serial {
        WRITE      = 0,
        READ       = 1,
        FLUSH      = 2,
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
        assert_eq!(OP_TABLE.len(), 22);
        for w in OP_TABLE.windows(2) {
            assert!(
                w[0].id < w[1].id,
                "OP_TABLE must be strictly sorted: {} vs {}",
                w[0].id,
                w[1].id
            );
        }
        assert_eq!(Op::SysProp.domain(), domain::SYS);
        assert_eq!(Op::RfVfo.domain(), domain::RF);
        assert_eq!(Op::RfVfo.index(), 1);
        assert_eq!(
            Op::find_desc(Op::SysSec.id()).unwrap().cap(),
            Some(Cap::Preferences)
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
