//! Cooperative Multi-Slot Background Task Runtime (Zone A: Daemons, Services, Drivers, Modules).
//!
//! Hosts up to [`MAX_BG_SLOTS`] concurrent resident background packages in the lower
//! [`ZONE_A_MAX_BYTES`] (`1,024 B`) partition of `TRANSIENT_ARENA`, while foreground
//! applications and P1 demand-paged windows execute cleanly in Zone B above `zone_a_used_bytes()`.

use ::abi::CapabilitySet;
use kernel::proc::Pid;
use kernel::runtime::KrxExecType;
use kernel::sched::Priority;

/// Maximum concurrent resident background packages in Zone A.
pub const MAX_BG_SLOTS: usize = 2;

/// Maximum SRAM reservation for Zone A background daemons, services, drivers, and modules.
pub use crate::data::arena::ZONE_A_MAX_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgError {
    SlotsFull,
    OutOfArena,
    InvalidSize,
}

/// Compact resident background task slot (`28 B` on 32-bit Cortex-M0+).
#[derive(Debug, Clone, Copy)]
#[repr(C, align(4))]
pub struct BgSlot {
    pub pid: Pid,
    pub exec_type: KrxExecType,
    pub priority: Priority,
    pub active: bool,
    pub arena_off: u16,
    pub arena_len: u16,
    pub period_ms: u16,
    pub _pad: u16,
    pub entry_point: usize,
    pub got_base: u32,
    pub caps: CapabilitySet,
    pub last_tick_ms: u32,
}

#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<BgSlot>() <= 32);

impl BgSlot {
    pub const fn empty() -> Self {
        Self {
            pid: Pid(0),
            exec_type: KrxExecType::Daemon,
            priority: Priority::Idle,
            active: false,
            arena_off: 0,
            arena_len: 0,
            period_ms: 0,
            _pad: 0,
            entry_point: 0,
            got_base: 0,
            caps: CapabilitySet::NONE,
            last_tick_ms: 0,
        }
    }

    #[inline(always)]
    pub const fn end_off(&self) -> usize {
        (self.arena_off as usize) + (self.arena_len as usize)
    }

    #[inline(always)]
    pub const fn is_due(&self, now_ms: u32) -> bool {
        self.active
            && self.entry_point != 0
            && (self.period_ms == 0
                || now_ms.wrapping_sub(self.last_tick_ms) >= self.period_ms as u32)
    }
}

static mut BG_SLOTS: [BgSlot; MAX_BG_SLOTS] = [const { BgSlot::empty() }; MAX_BG_SLOTS];
static mut ACTIVE_BG_IDX: u8 = 0xFF;

#[inline(always)]
fn slots_ref() -> &'static [BgSlot; MAX_BG_SLOTS] {
    slots_mut()
}

#[inline(never)]
fn slots_mut() -> &'static mut [BgSlot; MAX_BG_SLOTS] {
    unsafe { &mut *core::ptr::addr_of_mut!(BG_SLOTS) }
}

/// Cooperative multi-slot Zone A background manager for daemons, services, drivers, and modules.
pub struct BgManager;

impl BgManager {
    pub fn reset() {
        unsafe { *core::ptr::addr_of_mut!(ACTIVE_BG_IDX) = 0xFF };
        for s in slots_mut().iter_mut() {
            s.active = false;
            s.pid = Pid(0);
            s.entry_point = 0;
        }
    }

    /// Returns the currently executing Zone A background slot during its start or tick callback.
    #[inline(never)]
    pub fn active_bg_slot() -> Option<&'static BgSlot> {
        let idx = unsafe { *core::ptr::addr_of!(ACTIVE_BG_IDX) } as usize;
        slots_ref().get(idx).filter(|s| s.active)
    }

    /// Number of currently active resident background tasks.
    #[inline(always)]
    pub fn active_count() -> usize {
        let s = slots_ref();
        usize::from(s[0].active) + usize::from(s[1].active)
    }

    /// Byte watermark of Zone A currently reserved by active background tasks (`0` when idle,
    /// always 8-byte aligned and `<= ZONE_A_MAX_BYTES`).
    #[inline(never)]
    pub fn zone_a_used_bytes() -> usize {
        let mut max_end = 0usize;
        for s in slots_ref() {
            if s.active {
                max_end = max_end.max(s.end_off());
            }
        }
        (max_end + 7) & !7
    }

    /// Reserve a disjoint 8-byte-aligned Zone A sub-slice for a resident background task.
    /// Returns `(slot_idx, arena_offset)` on success.
    pub fn alloc_slot(
        pid: Pid,
        exec_type: KrxExecType,
        priority: Priority,
        caps: CapabilitySet,
        size_bytes: usize,
        period_ms: u16,
    ) -> Result<(usize, usize), BgError> {
        if size_bytes == 0 || size_bytes > ZONE_A_MAX_BYTES {
            return Err(BgError::InvalidSize);
        }
        let aligned_len = (size_bytes + 7) & !7;
        let base_off = Self::zone_a_used_bytes();
        if base_off + aligned_len > ZONE_A_MAX_BYTES {
            return Err(BgError::OutOfArena);
        }

        for (idx, slot) in slots_mut().iter_mut().enumerate() {
            if !slot.active {
                slot.pid = pid;
                slot.exec_type = exec_type;
                slot.priority = priority;
                slot.active = true;
                slot.arena_off = base_off as u16;
                slot.arena_len = aligned_len as u16;
                slot.period_ms = period_ms;
                slot.entry_point = 0;
                slot.got_base = 0;
                slot.caps = caps;
                slot.last_tick_ms = 0;
                return Ok((idx, base_off));
            }
        }
        Err(BgError::SlotsFull)
    }

    /// Bind the relocated Thumb-1 entry point and GOT base for an allocated Zone A slot.
    pub fn activate(slot_idx: usize, entry_point: usize, got_base: u32) -> bool {
        let Some(slot) = slots_mut().get_mut(slot_idx) else {
            return false;
        };
        if !slot.active {
            return false;
        }
        slot.entry_point = entry_point;
        slot.got_base = got_base;
        true
    }

    /// Stop and release the Zone A slot owned by `pid`.
    #[inline(never)]
    pub fn stop_pid(pid: Pid) -> bool {
        if pid.0 == 0 {
            return false;
        }
        for slot in slots_mut().iter_mut() {
            if slot.active && slot.pid == pid {
                slot.active = false;
                slot.pid = Pid(0);
                slot.entry_point = 0;
                return true;
            }
        }
        false
    }

    /// Return `[first, second]` slot indices ordered from highest to lowest priority.
    #[inline(always)]
    fn prio_order() -> [usize; MAX_BG_SLOTS] {
        let s = slots_ref();
        if (s[1].priority as u8) > (s[0].priority as u8) {
            [1, 0]
        } else {
            [0, 1]
        }
    }

    /// Cooperatively dispatch all due background tasks in strict priority order
    /// (`Safety > Capture > Radio > Ui > Idle`), returning the number of tasks ticked.
    #[cfg(test)]
    pub fn tick_due<F: FnMut(&BgSlot)>(now_ms: u32, mut run: F) -> usize {
        let mut ran = 0usize;
        for idx in Self::prio_order() {
            let slot = &mut slots_mut()[idx];
            if slot.is_due(now_ms) {
                slot.last_tick_ms = now_ms;
                run(slot);
                ran += 1;
            }
        }
        ran
    }

    /// Invoke an op (`KRX_OP_START` or `KRX_OP_EVENT`) on a resident Zone A slot with its
    /// own `Pid` and `CapabilitySet` bound for `kern_call`.
    #[inline(never)]
    pub fn invoke_slot_op(slot_idx: usize, op: u32, param: *const ()) -> u32 {
        let Some(s) = slots_ref().get(slot_idx) else {
            return 0xFFFF_FFFF;
        };
        if !s.active || s.entry_point == 0 {
            return 0xFFFF_FFFF;
        }
        let api = kernel::proc::abi::KrxApiTable::with_call(s.caps.raw(), super::syscall::kern_call);
        unsafe {
            *core::ptr::addr_of_mut!(ACTIVE_BG_IDX) = slot_idx as u8;
            let res = super::native_runner::invoke_krx_native(
                s.entry_point,
                s.got_base,
                op,
                &raw const api,
                param,
            );
            *core::ptr::addr_of_mut!(ACTIVE_BG_IDX) = 0xFF;
            res
        }
    }

    /// Cooperative OS tick hook called from the orchestrator when background tasks are loaded.
    pub fn tick_system(now_ms: u32) {
        let tick_ev = kernel::proc::abi::KrxEvent {
            kind: kernel::proc::abi::KRX_EV_TICK,
            key: 0,
            flags: 0,
            reserved: 0,
            payload: 0,
        };
        for idx in Self::prio_order() {
            let pid = {
                let s = &mut slots_mut()[idx];
                if !s.is_due(now_ms) {
                    continue;
                }
                s.last_tick_ms = now_ms;
                s.pid
            };
            let low_stack = kernel::sys::SystemDiagnostics::stack_margin_bytes() < 256;
            let res = if low_stack {
                0
            } else {
                Self::invoke_slot_op(
                    idx,
                    kernel::proc::abi::KRX_OP_EVENT,
                    &raw const tick_ev as *const (),
                )
            };
            if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(pid) {
                if low_stack {
                    pcb.ledger.record_slice_overrun();
                } else {
                    pcb.ledger.record_event();
                    if res == kernel::runtime::fault::KRX_FAULT_SENTINEL {
                        pcb.ledger.record_fault();
                    }
                }
            }
            if !low_stack && res == kernel::runtime::fault::KRX_FAULT_SENTINEL {
                Self::stop_pid(pid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_a_coexists_with_zone_b_and_reclaims_on_stop() {
        BgManager::reset();
        assert_eq!(BgManager::active_count(), 0);
        assert_eq!(BgManager::zone_a_used_bytes(), 0);
        assert_eq!(
            crate::data::arena::zone_b_base_ptr(),
            crate::data::arena::arena_base_ptr()
        );

        let (s0, off0) = BgManager::alloc_slot(
            Pid(10),
            KrxExecType::Daemon,
            Priority::Radio,
            CapabilitySet::NONE,
            256,
            100,
        )
        .unwrap();
        assert_eq!((s0, off0), (0, 0));
        assert_eq!(BgManager::zone_a_used_bytes(), 256);

        let (s1, off1) = BgManager::alloc_slot(
            Pid(11),
            KrxExecType::Service,
            Priority::Safety,
            CapabilitySet::NONE,
            252,
            50,
        )
        .unwrap();
        assert_eq!(s1, 1);
        assert_eq!(off1, 256);
        assert_eq!(BgManager::zone_a_used_bytes(), 512);
        assert_eq!(
            crate::data::arena::zone_b_base_ptr() as usize
                - crate::data::arena::arena_base_ptr() as usize,
            512
        );
        assert_eq!(crate::data::arena::zone_b_cache_bytes(), 4096);

        assert_eq!(
            BgManager::alloc_slot(
                Pid(12),
                KrxExecType::Module,
                Priority::Idle,
                CapabilitySet::NONE,
                256,
                100
            ),
            Err(BgError::SlotsFull)
        );
        assert_eq!(
            BgManager::alloc_slot(
                Pid(12),
                KrxExecType::Module,
                Priority::Idle,
                CapabilitySet::NONE,
                600,
                100
            ),
            Err(BgError::OutOfArena)
        );

        assert!(BgManager::activate(s0, 0x2000_0001, 0x2000_0100));
        assert!(BgManager::activate(s1, 0x2000_0201, 0x2000_0300));
        let mut order = [0u8; 2];
        let mut n = 0usize;
        let ran = BgManager::tick_due(200, |slot| {
            order[n] = slot.pid.0;
            n += 1;
        });
        assert_eq!(ran, 2);
        assert_eq!(order, [11, 10]);

        assert_eq!(BgManager::tick_due(240, |_| {}), 0);
        let mut who = 0u8;
        assert_eq!(BgManager::tick_due(260, |s| who = s.pid.0), 1);
        assert_eq!(who, 11);

        assert!(BgManager::stop_pid(Pid(11)));
        assert_eq!(BgManager::zone_a_used_bytes(), 256);
        assert!(BgManager::stop_pid(Pid(10)));
        assert_eq!(BgManager::zone_a_used_bytes(), 0);
        assert_eq!(BgManager::active_count(), 0);
    }
}
