//! Declarative Unified Kernel Operation Table (`abi!`).
//!
//! Single source of truth for KRX `kern_call(op, a0, a1, a2)` and RigWire operation IDs,
//! argument signatures (`Sig`), and required capability tokens (`Cap`).
//! Operation IDs are packed as `(domain << 4) | index` (16 domains x 16 operations).

use crate::caps::Cap;

/// Argument kind encoded in 2 bits per parameter slot inside `Sig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ArgKind {
    /// Immediate scalar value (`u32`).
    Val = 0,
    /// Read-only guest buffer pointer (`*const u8`, length in next arg or fixed).
    In = 1,
    /// Writable guest buffer pointer (`*mut u8`, length in next arg or fixed).
    Out = 2,
    /// Read-only UTF-8/ASCII string pointer (`*const u8`, length in next arg).
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
    pub const fn check_ptrs(self, a0: u32, a1: u32, a2: u32) -> bool {
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

/// Declares the sorted kernel operation table (`Op` enum + `OP_TABLE` slice).
#[macro_export]
macro_rules! abi {
    (
        $(
            $domain:ident $dom_id:literal {
                $( $name:ident = $idx:literal ( $a0:ident, $a1:ident, $a2:ident ) $( [ $cap:ident ] )? ),* $(,)?
            }
        )*
    ) => {
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

abi! {
    SYS 0x0 {
        SysInfo    = 0 (Out, Val, Val),
        SysVersion = 1 (Val, Val, Val),
        SysUid     = 2 (Out, Val, Val),
        SysReboot  = 3 (Val, Val, Val) [PowerManage],
        SysSleep   = 4 (Val, Val, Val),
    }
    PROC 0x1 {
        ProcSelf   = 0 (Val, Val, Val),
        ProcFind   = 1 (Str, Val, Val),
        ProcCount  = 2 (Val, Val, Val),
        ProcInfo   = 3 (Val, Out, Val),
        ProcKill   = 4 (Val, Val, Val) [TaskControl],
        ProcCtl    = 5 (Val, Val, Out) [TaskControl],
    }
    IPC 0x2 {
        IpcSend    = 0 (In,  Val, Val),
        IpcRecv    = 1 (Out, Val, Val),
        IpcPoll    = 2 (Out, Val, Val),
        IpcReply   = 3 (In,  In,  Val),
    }
    FS 0x3 {
        FsRead     = 0 (Str, Val, Out) [StorageRead],
        FsWrite    = 1 (Str, Val, In)  [StorageWrite],
        FsStat     = 2 (Str, Val, Val) [StorageRead],
        FsList     = 3 (Str, Val, Out) [StorageRead],
        FsRemove   = 4 (Str, Val, Val) [StorageWrite],
        FsRename   = 5 (Str, Str, Val) [StorageWrite],
        FsFormat   = 6 (Val, Val, Val) [StorageAdmin],
    }
    RF 0x4 {
        RfTune     = 0 (Val, Val, Val) [RadioRx],
        RfGetFreq  = 1 (Val, Val, Val) [RadioRx],
        RfGetRssi  = 2 (Val, Val, Val) [RadioRx],
        RfVfoGet   = 3 (Val, Out, Val) [RadioRx],
        RfVfoSet   = 4 (Val, In,  Val) [RadioRx],
        RfState    = 5 (Out, Val, Val) [RadioRx],
        RfSetTx    = 6 (Val, Val, Val) [RadioTx],
        RfSweep    = 7 (Val, Val, Out) [RadioRx],
    }
    AUD 0x5 {
        AudTone    = 0 (Val, Val, Val) [AudioOutput],
        AudCapture = 1 (Val, Out, Val) [AudioCapture],
        AudVolume  = 2 (Val, Val, Val) [AudioOutput],
    }
    GFX 0x6 {
        GfxPixel   = 0 (Val, Val, Val),
        GfxRect    = 1 (Val, Val, Val),
        GfxFill    = 2 (Val, Val, Val),
        GfxText    = 3 (Val, Val, Str),
        GfxCommit  = 4 (Val, Val, Val),
        GfxFb      = 5 (Val, Val, Val) [DisplayDirect],
    }
    UI 0x7 {
        UiModal    = 0 (Val, Str, Str),
        UiToast    = 1 (Str, Val, Val),
        UiTray     = 2 (Val, Val, Val),
        UiStatus   = 3 (In,  Val, Val),
    }
    IO 0x8 {
        IoKey      = 0 (Val, Val, Val) [Keypad],
        IoTimer    = 1 (Val, Val, Val),
        IoPower    = 2 (Val, Val, Val),
        IoSpi      = 3 (In,  Out, Val) [BusAccess],
        IoI2c      = 4 (Val, In,  Out) [BusAccess],
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
        assert_eq!(Op::SysInfo.domain(), 0);
        assert_eq!(Op::RfSetTx.domain(), 4);
        assert_eq!(Op::RfSetTx.index(), 6);
        assert_eq!(
            Op::find_desc(Op::RfSetTx.id()).unwrap().cap,
            Some(Cap::RadioTx)
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
