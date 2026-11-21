//! Unified Single-Entry KRX C-ABI Specification (`kern_call`).
//!
//! Re-exports the canonical C-ABI request descriptors, record layouts, and constants
//! from [`::abi::types`] alongside kernel IPC message descriptors.

pub use crate::ipc::{KrxIpcMessage, ipc_flags, ipc_msg_type, ipc_target};
pub use ::abi::types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::Op;

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
