//! Asynchronous Radio Subsystem Events and Bus Serialization (§1.5 / §13.3).

use super::arbitrator::{ClientToken, RfResourceMask};
use super::channel::VfoSlot;
use crate::ipc::{BusEvent, Topic, radio_events};

/// Asynchronous events emitted by the radio coordinator on 10ms ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioEvent {
    None,
    CarrierOpened(VfoSlot),
    CarrierClosed(VfoSlot),
    DualWatchSwitched(VfoSlot),
    FskSyncDetected(VfoSlot),
    FskFrameFinished(VfoSlot),
    FskLockReleased(VfoSlot),
    LockAcquired {
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
    },
    LockReleased {
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
    },
    LockPreempted {
        victim: ClientToken,
        by: ClientToken,
    },
    /// Stashed background lock restored after higher-priority preemption released.
    LockRestored {
        client: ClientToken,
        resources: RfResourceMask,
        slot: VfoSlot,
    },
    PacketReceived,
    /// Transmission ended and synthesizer returned to RX frequency.
    TxEnded,
    /// Time-Out Timer approaching limit warning (e.g., 10 seconds before cutoff).
    TotWarning {
        remaining_sec: u8,
    },
    /// Time-Out Timer expired: transmitter automatically disengaged.
    TotExpired,
}

impl RadioEvent {
    /// Convert coordinator radio event into a microkernel BusEvent for asynchronous IPC delivery.
    pub fn to_bus_event(&self, sender: u8) -> Option<BusEvent> {
        match *self {
            RadioEvent::None => None,
            RadioEvent::CarrierOpened(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::CARRIER_OPENED,
                slot.index() as u32,
            )),
            RadioEvent::CarrierClosed(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::CARRIER_CLOSED,
                slot.index() as u32,
            )),
            RadioEvent::DualWatchSwitched(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::DUAL_WATCH_SWITCHED,
                slot.index() as u32,
            )),
            RadioEvent::FskSyncDetected(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::FSK_SYNC_DETECTED,
                slot.index() as u32,
            )),
            RadioEvent::FskFrameFinished(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::FSK_FRAME_FINISHED,
                slot.index() as u32,
            )),
            RadioEvent::FskLockReleased(slot) => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::FSK_LOCK_RELEASED,
                slot.index() as u32,
            )),
            RadioEvent::LockAcquired {
                client,
                resources: _,
                slot,
            } => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::LOCK_ACQUIRED,
                (client.id.0 & 0x00FF_FFFF) | ((slot.index() as u32) << 24),
            )),
            RadioEvent::LockReleased {
                client,
                resources: _,
                slot,
            } => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::LOCK_RELEASED,
                (client.id.0 & 0x00FF_FFFF) | ((slot.index() as u32) << 24),
            )),
            RadioEvent::LockPreempted { victim, by } => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::LOCK_PREEMPTED,
                ((victim.id.0 as u16 as u32) << 16) | (by.id.0 as u16 as u32),
            )),
            RadioEvent::LockRestored {
                client,
                resources: _,
                slot,
            } => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::LOCK_RESTORED,
                (client.id.0 & 0x00FF_FFFF) | ((slot.index() as u32) << 24),
            )),
            RadioEvent::PacketReceived => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::PACKET_RECEIVED,
                0,
            )),
            RadioEvent::TxEnded => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::TX_ENDED,
                0,
            )),
            RadioEvent::TotWarning { remaining_sec } => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::TOT_WARNING,
                remaining_sec as u32,
            )),
            RadioEvent::TotExpired => Some(BusEvent::new(
                Topic::RADIO,
                sender,
                radio_events::TOT_EXPIRED,
                0,
            )),
        }
    }

    /// Reconstruct RadioEvent from a microkernel BusEvent.
    pub fn from_bus_event(bus_ev: &BusEvent) -> Option<Self> {
        if !bus_ev.is_radio() {
            return None;
        }

        let slot = match bus_ev.data & 0x03 {
            0 => VfoSlot::VfoA,
            1 => VfoSlot::VfoB,
            2 => VfoSlot::VfoC,
            _ => VfoSlot::VfoD,
        };

        match bus_ev.event_id {
            radio_events::CARRIER_OPENED => Some(Self::CarrierOpened(slot)),
            radio_events::CARRIER_CLOSED => Some(Self::CarrierClosed(slot)),
            radio_events::DUAL_WATCH_SWITCHED => Some(Self::DualWatchSwitched(slot)),
            radio_events::FSK_SYNC_DETECTED => Some(Self::FskSyncDetected(slot)),
            radio_events::FSK_FRAME_FINISHED => Some(Self::FskFrameFinished(slot)),
            radio_events::FSK_LOCK_RELEASED => Some(Self::FskLockReleased(slot)),
            radio_events::PACKET_RECEIVED => Some(Self::PacketReceived),
            radio_events::TX_ENDED => Some(Self::TxEnded),
            radio_events::TOT_WARNING => Some(Self::TotWarning {
                remaining_sec: bus_ev.data as u8,
            }),
            radio_events::TOT_EXPIRED => Some(Self::TotExpired),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radio_event_bus_serialization_roundtrip() {
        let ev1 = RadioEvent::CarrierOpened(VfoSlot::VfoB);
        let bus1 = ev1.to_bus_event(2).expect("bus event expected");
        assert!(bus1.is_radio());
        assert_eq!(bus1.event_id, radio_events::CARRIER_OPENED);
        assert_eq!(bus1.sender, 2);
        assert_eq!(RadioEvent::from_bus_event(&bus1), Some(ev1));

        let ev2 = RadioEvent::CarrierClosed(VfoSlot::VfoA);
        let bus2 = ev2.to_bus_event(2).unwrap();
        assert_eq!(RadioEvent::from_bus_event(&bus2), Some(ev2));

        let ev3 = RadioEvent::DualWatchSwitched(VfoSlot::VfoC);
        let bus3 = ev3.to_bus_event(2).unwrap();
        assert_eq!(RadioEvent::from_bus_event(&bus3), Some(ev3));

        let ev4 = RadioEvent::TotWarning { remaining_sec: 10 };
        let bus4 = ev4.to_bus_event(2).unwrap();
        assert_eq!(bus4.event_id, radio_events::TOT_WARNING);
        assert_eq!(bus4.data, 10);
        assert_eq!(RadioEvent::from_bus_event(&bus4), Some(ev4));

        let ev5 = RadioEvent::TotExpired;
        let bus5 = ev5.to_bus_event(2).unwrap();
        assert_eq!(RadioEvent::from_bus_event(&bus5), Some(ev5));

        assert_eq!(RadioEvent::None.to_bus_event(2), None);
    }
}
