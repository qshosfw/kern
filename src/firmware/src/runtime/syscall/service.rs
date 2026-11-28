//! Unified System, Property/Telemetry, Process & IPC System Call Handlers
//! (`Op::Sys*` 0x0, `Op::Proc*` 0x1, `Op::Ipc` 0x2).
//!
//! `op_sys_prop` (`0x00..=0xFE`) serves as the single scalar state/telemetry/control bus
//! shared across KRX packages (`kern_call`) and RigWire (`services::wire`).

#[cfg(feature = "passcode")]
use super::slice_or_empty;
use super::{parse_str, ptr_ref, ptr_ref_mut, ptr_slice, ptr_slice_mut};
use kernel::ipc::{IpcChannel, KrxIpcMessage};
use kernel::proc::abi::{
    KRX_ABI_VERSION, KRX_QS1_INVALID, KRX_QS1_UNVERIFIED, KRX_SYS_INFO_FLAG_CALIB_VALID,
    KRX_SYS_INFO_FLAG_HAS_CERT, KRX_SYS_INFO_FLAG_RDP_PROTECTED, KrxProcInfo, KrxSysInfo,
};
pub use kernel::proc::service_registry::ServiceRegistry;
use kernel::service::PowerService;

pub static mut SERVICE_REGISTRY: ServiceRegistry<2> = ServiceRegistry::new();
pub static mut KERNEL_IPC_CHANNEL: IpcChannel<2> = IpcChannel::new();

static mut PENDING_LAUNCH: utils::format::InlineStr<31> = utils::format::InlineStr::new();

pub fn take_pending_launch_path() -> Option<&'static str> {
    unsafe {
        let p = &mut *core::ptr::addr_of_mut!(PENDING_LAUNCH);
        if p.is_empty() {
            return None;
        }
        let s = core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            p.buf.as_ptr(),
            p.len().min(31),
        ));
        p.clear();
        Some(s)
    }
}

pub fn ipc_channel_mut() -> &'static mut IpcChannel<2> {
    unsafe { &mut *core::ptr::addr_of_mut!(KERNEL_IPC_CHANNEL) }
}

pub fn service_registry_mut() -> &'static mut ServiceRegistry<2> {
    unsafe { &mut *core::ptr::addr_of_mut!(SERVICE_REGISTRY) }
}

pub fn active_caller_pid() -> u8 {
    if let Some(bg) = super::super::background::BgManager::active_bg_slot() {
        return bg.pid.0;
    }
    let slot = super::super::native_runner::slot_ref();
    if slot.is_loaded && slot.pid.0 != 0 {
        slot.pid.0
    } else {
        for p in &crate::orchestrator::proc_table().slots {
            if p.state == kernel::proc::ProcState::Running {
                return p.pid.0;
            }
        }
        1
    }
}

fn post_sys_event(target: u8, command: u8, arg: u16) {
    crate::system::enqueue_event(kernel::event::Event::Sys(kernel::ipc::SysEvent {
        target,
        command,
        arg,
    }));
}

#[inline(always)]
fn post_shortcut(sc: crate::apps::Shortcut) {
    let ev: kernel::ipc::SysEvent = sc.into();
    post_sys_event(ev.target, ev.command, ev.arg);
}

fn active_slot_scan(target: u8) -> (u8, Option<&'static kernel::proc::Pcb>) {
    let mut count = 0u8;
    for slot in crate::orchestrator::proc_table().slots.iter() {
        if !matches!(
            slot.state,
            kernel::proc::ProcState::Dormant | kernel::proc::ProcState::Dead
        ) {
            if count == target {
                return (count, Some(slot));
            }
            count += 1;
        }
    }
    (count, None)
}

fn copy_cstr(dst: &mut [u8], src: &[u8]) {
    let n = src.len().min(dst.len().saturating_sub(1));
    unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), n) };
}

unsafe fn fill_sys_info(out_info: *mut KrxSysInfo) -> usize {
    let Some(out) = (unsafe { ptr_ref_mut(out_info) }) else {
        return 1;
    };
    unsafe { core::ptr::write_bytes(out as *mut KrxSysInfo, 0, 1) };

    out.flash_kb = py32_hal::uid::SystemInfo::flash_size_kb();
    out.sram_kb = 16;
    if bsp::uid::is_mcu_read_protected() {
        out.flags |= KRX_SYS_INFO_FLAG_RDP_PROTECTED;
    }
    if crate::calibration_mut().is_valid() {
        out.flags |= KRX_SYS_INFO_FLAG_CALIB_VALID;
    }
    let model = crate::services::config().system.model_str().as_bytes();
    bsp::uid::fill_device_identity(&mut out.mac, &mut out.serial);
    out.mcu_uid = bsp::uid::read_mcu_uid();

    let meta = &crate::build_info::FW_METADATA;
    out.fw_major = meta.version_major;
    out.fw_minor = meta.version_minor;
    out.fw_patch = meta.version_patch;
    out.target_id = meta.target_id;

    copy_cstr(&mut out.model, model);
    copy_cstr(&mut out.mcu, b"PY32F071xB");
    copy_cstr(
        &mut out.boot_ver,
        bsp::uid::bootloader_version_str().as_bytes(),
    );
    copy_cstr(&mut out.git_hash, crate::build_info::GIT_HASH.as_bytes());
    copy_cstr(
        &mut out.build_date,
        crate::build_info::BUILD_DATE.as_bytes(),
    );

    if let Some(cert) = py32_hal::uid::SystemInfo::factory_cert_slice() {
        out.flags |= KRX_SYS_INFO_FLAG_HAS_CERT;
        if let Some(qs1) = bsp::uid::extract_qs1_slice(cert) {
            out.qs1_status = KRX_QS1_UNVERIFIED;
            out.qs1_len = qs1.len() as u8;
            copy_cstr(&mut out.qs1, qs1);
        } else {
            out.qs1_status = KRX_QS1_INVALID;
        }
    }
    0
}

fn post_event_id(event_id: u32) -> usize {
    let ev = kernel::ipc::SysEvent {
        target: (event_id >> 16) as u8,
        command: (event_id & 0xFF) as u8,
        arg: ((event_id >> 8) & 0xFF) as u16,
    };
    usize::from(!crate::services::post_event(ev))
}

#[cfg_attr(feature = "passcode", inline(never))]
unsafe fn dispatch_sys_sec(sub: u8, a1: usize, a2: usize) -> usize {
    #[cfg(not(feature = "passcode"))]
    {
        let _ = (sub, a1, a2);
        return if sub == ::abi::sub::sys_sec::STATUS {
            0
        } else {
            1
        };
    }
    #[cfg(feature = "passcode")]
    {
        use ::abi::{SEC_FLAG_AVAILABLE, sub::sys_sec};
        let fs = crate::services::storage_mut();
        let ps = unsafe { &mut *core::ptr::addr_of_mut!(crate::PASSCODE_STATE) };
        #[cfg(not(test))]
        let progress = Some(crate::boot::security::lcd_verify_progress_hook(
            &mut crate::kern_hw().lcd,
        ));
        #[cfg(test)]
        let progress: Option<crate::security::ProgressHook> = None;

        match sub {
            sys_sec::STATUS => {
                let c = crate::security::passcode::get_passcode_config(fs);
                (SEC_FLAG_AVAILABLE
                    | (c.is_set as u32)
                    | ((c.expose_len as u32) << 1)
                    | ((c.stealth as u32) << 2)
                    | ((c.digits_only as u32) << 3)
                    | ((c.passcode_len as u32) << 8)
                    | ((c.max_tries as u32) << 16)
                    | ((c.failed_attempts as u32) << 24)) as usize
            }
            sys_sec::SET_OPTS => {
                let opt_flags = ((a1 as u8) & 0x0E) << 1;
                let max_tries = (a1 >> 8) as u8;
                usize::from(
                    crate::security::passcode::set_passcode_options(fs, opt_flags, max_tries)
                        .is_err(),
                )
            }
            sys_sec::VERIFY => {
                let pin = unsafe { slice_or_empty(a1 as *const u8, (a2 as u8) as usize) };
                match crate::security::passcode::verify_passcode(fs, pin, ps, progress) {
                    Ok(true) => 0,
                    Ok(false) => {
                        let c = crate::security::passcode::get_passcode_config(fs);
                        if c.failed_attempts >= c.max_tries {
                            #[cfg(not(test))]
                            kernel::power::PowerControl::reboot();
                            #[cfg(test)]
                            return 2;
                        }
                        1
                    }
                    Err(_) => 2,
                }
            }
            sys_sec::SET => {
                let len = (a2 & 0xFF).min(16);
                let pin = unsafe { slice_or_empty(a1 as *const u8, len) };
                let opt_flags = (((a2 >> 16) as u8) & 0x0E) << 1;
                let max_tries = (a2 >> 24) as u8;
                usize::from(
                    crate::security::passcode::set_passcode(
                        fs, ps, pin, opt_flags, max_tries, progress,
                    )
                    .is_err(),
                )
            }
            _ => 1,
        }
    }
}

// ── Unified Property & Live Telemetry/Control Bus (`0x00..=0xFE`) ─────────────

fn required_live_cap(id: u8) -> Option<::abi::Cap> {
    use ::abi::{Cap, prop};
    match id {
        prop::KEY_F_ACTIVE | prop::KEY_LOCKED => Some(Cap::Keypad),
        prop::AUD_PATH..=prop::AUD_CAPTURING => Some(Cap::AudioOutput),
        prop::MODAL_STATE..=prop::TRAY_ICON => Some(Cap::DisplayDirect),
        prop::RF_FREQ..=0xFE => Some(Cap::RadioRx),
        _ => None,
    }
}

/// Read any live scalar (`0xC0..=0xFE`) without redundant per-domain boilerplate.
pub fn live_prop_get(id: u8) -> usize {
    use ::abi::prop;
    let p = crate::services::power();
    match id {
        prop::OS_VERSION => KRX_ABI_VERSION as usize,
        prop::TICKS_MS => py32_hal::delay::millis() as usize,
        prop::CPU_HZ => py32_hal::rcc::sysclk_hz() as usize,
        prop::STACK_FREE => kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16 as usize,
        prop::RAND32 => {
            let (_, radio) = crate::services::state_and_radio();
            let noise = radio.read_reg(drivers::bk4829::registers::REG_NOISE_VAL) as u32;
            let seed = py32_hal::delay::millis() ^ (noise << 16);
            crypto::crc::CrcEngine::crc32(&seed.to_le_bytes()) as usize
        }
        prop::BATT_MV => p.battery_voltage_mv() as usize,
        prop::BATT_PCT => p.battery_percentage() as usize,
        prop::BATT_MA => p.battery_current_ma() as usize,
        prop::BATT_TYPE => p.battery_type() as usize,
        prop::BATT_ADC => p.raw_battery_adc() as usize,
        prop::MCU_TEMP => p.mcu_temperature_deci_c() as u16 as usize,
        prop::CHARGING => usize::from(p.is_charging),
        prop::BATT_LOW => usize::from(p.is_low()),
        prop::BATT_CRIT => usize::from(p.is_critical()),
        prop::FLASHLIGHT => bsp::pins::FLASHLIGHT.is_high() as usize,
        prop::KEY_F_ACTIVE => crate::kern_hw().orchestrator.is_f_active() as usize,
        prop::KEY_LOCKED => crate::kern_hw().orchestrator.is_keypad_locked() as usize,
        prop::USB_CONNECTED => {
            #[cfg(feature = "usb")]
            {
                crate::kern_hw().usb.is_connected() as usize
            }
            #[cfg(not(feature = "usb"))]
            0
        }
        prop::AUD_PLAYING => crate::kern_hw().audio.is_active() as usize,
        prop::AUD_MIC_LEVEL => {
            let (_, radio) = crate::services::state_and_radio();
            radio.read_reg(drivers::bk4829::registers::REG_AUDIO_AMP) as usize
        }
        prop::AUD_CAPTURING => crate::audio_capture_mut().is_capturing() as usize,
        prop::MODAL_STATE => {
            let m = crate::orchestrator::compositor::active_modal_mut();
            (m.is_active() as usize) | ((m.is_animating() as usize) << 1)
        }
        prop::SELF_PID => active_caller_pid() as usize,
        prop::PROC_COUNT => active_slot_scan(u8::MAX).0 as usize,
        prop::RF_FREQ..=0xFE => super::radio::radio_prop_get(id),
        _ => 0,
    }
}

/// Write any live control (`0xC0..=0xFE`); returns `0` on success (or status value where specified).
pub fn live_prop_set(id: u8, val: usize) -> usize {
    use ::abi::prop;
    match id {
        prop::BL_WAKE => {
            crate::services::wake_display();
            0
        }
        prop::FLASHLIGHT => {
            let high = if val >= 2 {
                !bsp::pins::FLASHLIGHT.is_high()
            } else {
                val != 0
            };
            if high {
                bsp::pins::FLASHLIGHT.set_high();
            } else {
                bsp::pins::FLASHLIGHT.set_low();
            }
            0
        }
        prop::KEY_LOCKED => {
            let orch = &mut crate::kern_hw().orchestrator;
            if val >= 2 || (val != 0) != orch.is_keypad_locked() {
                orch.toggle_keypad_lock();
            }
            0
        }
        prop::AUD_PATH => {
            let aud = &mut crate::kern_hw().audio;
            if val != 0 {
                aud.enable();
            } else {
                aud.disable();
            }
            0
        }
        prop::MODAL_STATE => {
            if val == 0 {
                crate::orchestrator::sink_enter_keys();
                crate::orchestrator::compositor::active_modal_mut().close();
            }
            0
        }
        prop::MODAL_HOLD_MS => {
            crate::orchestrator::compositor::active_modal_mut().set_hold_ms(val as u16);
            0
        }
        prop::TRAY_ICON => super::ui::set_tray_icon(val as u8) as usize,
        prop::RF_FREQ..=0xFE => super::radio::radio_prop_set(id, val),
        _ => ::abi::Status::Unsupported.to_raw(),
    }
}

pub unsafe fn op_sys_prop(a0: usize, a1: usize, _a2: usize) -> usize {
    use ::abi::{prop, sub::sys_prop};
    let id = a0 as u8;
    let mode = (a0 >> 8) as u8;
    if id >= 0xC0 {
        if let Some(cap) = required_live_cap(id)
            && !super::has_cap(cap)
        {
            return ::abi::Status::PermissionDenied.to_raw();
        }
        if mode != sys_prop::GET
            && id == prop::RF_TX
            && a1 != 0
            && !super::has_cap(::abi::Cap::RadioTx)
        {
            return super::radio::RF_TX_DENIED;
        }
        return match mode {
            sys_prop::GET => live_prop_get(id),
            sys_prop::SET => live_prop_set(id, a1),
            _ => ::abi::Status::InvalidArg.to_raw(),
        };
    }

    let cfg = crate::services::config_mut();
    let val = a1 as u8;
    match mode {
        sys_prop::GET => cfg.prop_get(id) as usize,
        sys_prop::SET => {
            let granted = crate::runtime::native_runner::active_native_caps();
            let allowed = prop::CONTROL_CAP
                .get((id >> 5) as usize)
                .is_some_and(|&cap| granted.contains(cap));
            usize::from(!(allowed && cfg.control_set(id, val)))
        }
        sys_prop::PREF => {
            if !super::has_cap(::abi::Cap::Preferences) {
                return ::abi::Status::PermissionDenied.to_raw();
            }
            usize::from(!cfg.prop_set(id, val))
        }
        _ => ::abi::Status::InvalidArg.to_raw(),
    }
}

pub unsafe fn op_sys_query(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::sys_query;
    match a0 as u8 {
        sys_query::INFO => unsafe { fill_sys_info(a1 as *mut KrxSysInfo) },
        sys_query::DEVICE_UID => {
            if let Some(dst) = unsafe { ptr_slice_mut(a1 as *mut u8, 12) } {
                dst.copy_from_slice(&bsp::uid::read_mcu_uid());
                0
            } else {
                1
            }
        }
        sys_query::CRC32 | sys_query::CRC16 => {
            let Some(s) = (unsafe { ptr_slice(a1 as *const u8, (a2 as u16) as usize) }) else {
                return 0;
            };
            if a0 as u8 == sys_query::CRC32 {
                crypto::crc::CrcEngine::crc32(s) as usize
            } else {
                crypto::crc::CrcEngine::crc16_ccitt(s) as usize
            }
        }
        sys_query::RTC_GET => {
            let Some(dst) = (unsafe { ptr_slice_mut(a1 as *mut u8, 7) }) else {
                return 1;
            };
            let mut rem = crate::services::power().uptime_secs;
            let mut d = 1u8;
            let mut h = 0u8;
            while rem >= 3600 {
                rem -= 3600;
                h += 1;
                if h == 24 {
                    h = 0;
                    d = if d == 28 { 1 } else { d + 1 };
                }
            }
            let (m, s) = utils::math::div60_u16(rem as u16);
            dst.copy_from_slice(&[0xEA, 0x07, 1, d, h, m as u8, s]);
            0
        }
        sys_query::FLUSH => usize::from(
            crate::services::config_mut()
                .flush_dirty(crate::services::storage_mut())
                .is_err(),
        ),
        sys_query::RELOAD => {
            crate::services::config_mut().reload_from_storage(crate::services::storage_mut());
            0
        }
        sys_query::RESET => {
            if !super::has_cap(::abi::Cap::Preferences) {
                return 1;
            }
            usize::from(
                crate::services::config_mut()
                    .reset_all_to_defaults(crate::services::storage_mut())
                    .is_err(),
            )
        }
        _ => 1,
    }
}

pub unsafe fn op_sys_ctl(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::sys_ctl;
    match a0 as u8 {
        sys_ctl::DELAY_MS => {
            let mut rem = a1 as u32;
            while rem > 0 {
                let step = rem.min(50);
                bsp::Board::wdt_feed();
                py32_hal::delay::delay_ms(step);
                rem -= step;
            }
            0
        }
        sys_ctl::DELAY_US => {
            py32_hal::delay::delay_us(a1 as u32);
            0
        }
        sys_ctl::EVENT => {
            if ((a0 >> 8) & 0xFF) == 0 {
                post_event_id(a1 as u32)
            } else {
                let len = (a1 >> 16) as u8;
                let b0 = if len > 0 {
                    unsafe { ptr_slice(a2 as *const u8, 1) }.map_or(0, |s| s[0] as u32)
                } else {
                    0
                };
                let topic = a1 as u16;
                post_event_id(((topic & 0xFF) as u32) | (b0 << 8) | (((topic >> 8) as u32) << 16))
            }
        }
        sys_ctl::REBOOT => {
            if !super::has_cap(::abi::Cap::PowerManage) {
                return ::abi::Status::PermissionDenied.to_raw();
            }
            kernel::power::PowerControl::reboot_with(kernel::power::RebootMode::from_u8(a1 as u8));
        }
        _ => ::abi::Status::Unsupported.to_raw(),
    }
}

pub unsafe fn op_sys_div(a0: usize, a1: usize, a2: usize) -> usize {
    let (q, r) = py32_hal::div::udivmod(a0 as u32, a1 as u32);
    if a2 != 0
        && (a2 & 3) == 0
        && let Some(dst) = unsafe { ptr_slice_mut(a2 as *mut u8, 4) }
    {
        dst.copy_from_slice(&r.to_ne_bytes());
    }
    q as usize
}

pub unsafe fn op_sys_sec(a0: usize, a1: usize, a2: usize) -> usize {
    unsafe { dispatch_sys_sec(a0 as u8, a1, a2) }
}

// ── Lean Process, Service & Execution Dispatch (`Op::ProcCtl`, `Op::ProcExec`) ──

unsafe fn fill_proc_info(index: u8, out_info: *mut KrxProcInfo) -> usize {
    let Some(info) = (unsafe { ptr_ref_mut(out_info) }) else {
        return 1;
    };
    let (_, Some(slot)) = active_slot_scan(index) else {
        return 1;
    };
    unsafe {
        core::ptr::write_bytes(info as *mut KrxProcInfo, 0, 1);
        info.pid = slot.pid.0;
        info.app_id = slot.app_id;
        info.state = slot.state as u8;
        info.priority = slot.priority as u8;
        info.memory_bytes = slot.mem_bytes;
        let src = slot.name.as_bytes();
        let len = src.len().min(info.name.len());
        core::ptr::copy_nonoverlapping(src.as_ptr(), info.name.as_mut_ptr(), len);
        info.exec_type = slot.exec_type as u8;

        let is_core = slot.app_id as usize == crate::apps::APP_ID_LAUNCHER
            || slot.exec_type == kernel::proc::ExecType::StaticFirmware;
        let mut pflags = if is_core {
            kernel::proc::abi::KRX_PROC_FLAG_PERSISTENT
        } else {
            kernel::proc::abi::KRX_PROC_FLAG_KILLABLE
        };
        if slot.state == kernel::proc::ProcState::Suspended {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_SUSPENDED;
        } else if slot.state == kernel::proc::ProcState::Running {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_FOREGROUND;
        }
        if crate::orchestrator::lease_manager().has_owner(slot.pid) {
            pflags |= kernel::proc::abi::KRX_PROC_FLAG_HAS_LEASE;
        }
        info.flags = pflags;
        info.stack_watermark = slot.health.stack_watermark_bytes();
        info.page_faults = slot.health.page_faults.min(255) as u8;
        info.fault_count = slot.health.faults;
    }
    0
}

pub unsafe fn op_proc_ctl(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::proc_ctl;
    use kernel::proc::{LeaseKind, Pid};

    let sub = a0 as u8;
    let target = (a0 >> 8) as u8;
    match sub {
        proc_ctl::INFO => unsafe { fill_proc_info(target, a1 as *mut KrxProcInfo) },
        proc_ctl::MEM => {
            let slot = super::super::native_runner::slot_ref();
            if !slot.is_loaded {
                return 1;
            }
            let Some(out_desc) =
                (unsafe { ptr_ref_mut(a1 as *mut kernel::proc::MemoryRegionDescriptor) })
            else {
                return 1;
            };
            if let Some(rk) = kernel::proc::RegionKind::from_u8(target)
                && let Some(desc) = slot.query_region(rk)
            {
                *out_desc = desc;
                0
            } else {
                1
            }
        }
        proc_ctl::LEASE => {
            let Some(kind) = LeaseKind::from_u8(target & 0x7F) else {
                return 1;
            };
            let pid = Pid(active_caller_pid());
            let mgr = crate::orchestrator::lease_manager_mut();
            let res = if (target & 0x80) != 0 {
                mgr.acquire(pid, kind, a1 as u32, py32_hal::delay::millis())
            } else {
                mgr.release(pid, kind)
            };
            usize::from(res.is_err())
        }
        proc_ctl::FIND => {
            if let Some(name_str) = unsafe { parse_str(a1 as *const u8, (target as usize).min(16)) }
                && let Some(pcb) = crate::orchestrator::proc_table_mut().find_by_name(name_str)
            {
                return pcb.pid.0 as usize;
            }
            0
        }
        proc_ctl::SVC_REG | proc_ctl::SVC_RES | proc_ctl::SVC_REL => {
            let iface = (a0 >> 8) as u16;
            let reg = service_registry_mut();
            match sub {
                proc_ctl::SVC_REG => usize::from(!reg.register(
                    iface,
                    a1 as u16,
                    Pid(active_caller_pid()),
                    a2,
                    false,
                )),
                proc_ctl::SVC_RES => reg.resolve(iface, a1 as u16).unwrap_or(0),
                _ => usize::from(!reg.release(iface)),
            }
        }
        proc_ctl::KILL => {
            if !super::has_cap(::abi::Cap::TaskControl) {
                return 2;
            }
            let is_fg = crate::orchestrator::proc_table()
                .find(Pid(target))
                .is_some_and(|p| p.state == kernel::proc::ProcState::Running);
            if !crate::orchestrator::reap_pid_resources(target) {
                return 1;
            }
            if is_fg {
                post_shortcut(crate::apps::Shortcut::CloseApp);
            }
            0
        }
        proc_ctl::LAUNCH_PKG => {
            if !super::has_cap(::abi::Cap::TaskControl) {
                return 2;
            }
            let plen = target as usize;
            if plen == 0 || plen > 31 {
                return 1;
            }
            let Some(path) = (unsafe { parse_str(a1 as *const u8, plen) }) else {
                return 1;
            };
            if !path.starts_with("/apps/") {
                return 1;
            }
            unsafe { (*core::ptr::addr_of_mut!(PENDING_LAUNCH)).set(path) };
            post_shortcut(crate::apps::Shortcut::LaunchPendingPkg);
            0
        }
        _ => 1,
    }
}

pub unsafe fn op_proc_exec(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::proc_exec;
    let sub = a0 as u8;
    if sub == proc_exec::LEAF {
        return super::super::native_runner::krx_leaf_call((a0 >> 8) as u8, a1 as u32) as usize;
    }
    #[cfg(feature = "krx_paged")]
    {
        match sub {
            proc_exec::PAGED_CALL => {
                super::super::native_runner::krx_call_paged(a1 as u16, (a1 >> 16) as u16, a2 as u16)
            }
            proc_exec::PAGED_RET => super::super::native_runner::krx_return_thunk(),
            proc_exec::PAGED_FAULT => {
                super::super::native_runner::krx_page_fault(a1 as u16, a2 as u16)
            }
            proc_exec::CALL_VFP => super::super::native_runner::krx_call_vfp(a1 as u32, a2 as u16),
            _ => 0,
        }
    }
    #[cfg(not(feature = "krx_paged"))]
    {
        let _ = (a1, a2);
        0
    }
}

pub unsafe fn op_ipc(a0: usize, a1: usize, _a2: usize) -> usize {
    use ::abi::sub::ipc;
    match a0 as u8 {
        ipc::SEND => {
            let Some(m) = (unsafe { ptr_ref(a1 as *const KrxIpcMessage) }) else {
                return 1;
            };
            let sender_pid = if m.sender_pid == 0 {
                active_caller_pid()
            } else {
                m.sender_pid
            };
            if ipc_channel_mut().send_from(m, sender_pid) {
                post_sys_event(m.target_pid, (m.msg_type & 0xFF) as u8, m.seq);
                0
            } else {
                1
            }
        }
        ipc::RECV => {
            let Some(out) = (unsafe { ptr_ref_mut(a1 as *mut KrxIpcMessage) }) else {
                return 1;
            };
            usize::from(!ipc_channel_mut().poll_into(active_caller_pid(), out))
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::abi::{Cap, CapabilitySet, Op, sub};

    #[test]
    fn proc_ctl_launch_pkg_bounds_and_prefix() {
        let _ = take_pending_launch_path();
        crate::runtime::native_runner::init_native_slot();
        let slot = crate::runtime::native_runner::slot_mut();
        slot.is_loaded = true;
        slot.caps = CapabilitySet::empty().with(Cap::TaskControl);

        let empty = b"";
        assert_eq!(
            unsafe {
                op_proc_ctl(
                    sub::proc_ctl::LAUNCH_PKG as usize,
                    empty.as_ptr() as usize,
                    0,
                )
            },
            1
        );
        assert!(take_pending_launch_path().is_none());

        let too_long = b"/apps/abcdefghijklmnopqrstuvwxyz";
        let a0 = (sub::proc_ctl::LAUNCH_PKG as usize) | (too_long.len() << 8);
        assert_eq!(unsafe { op_proc_ctl(a0, too_long.as_ptr() as usize, 0) }, 1);
        assert!(take_pending_launch_path().is_none());

        let bad_prefix = b"/user/settings.krx";
        let a0 = (sub::proc_ctl::LAUNCH_PKG as usize) | (bad_prefix.len() << 8);
        assert_eq!(
            unsafe { op_proc_ctl(a0, bad_prefix.as_ptr() as usize, 0) },
            1
        );
        assert!(take_pending_launch_path().is_none());

        let max_valid = b"/apps/abcdefghijklmnopqrstu.krx";
        let a0 = (sub::proc_ctl::LAUNCH_PKG as usize) | (max_valid.len() << 8);
        assert_eq!(
            unsafe { op_proc_ctl(a0, max_valid.as_ptr() as usize, 0) },
            0
        );
        assert_eq!(
            take_pending_launch_path(),
            Some("/apps/abcdefghijklmnopqrstu.krx")
        );
        assert!(take_pending_launch_path().is_none());
        crate::runtime::native_runner::init_native_slot();
    }

    #[test]
    fn sys_query_reset_and_sys_sec_enforce_preferences_cap() {
        crate::runtime::native_runner::init_native_slot();
        let slot = crate::runtime::native_runner::slot_mut();
        slot.is_loaded = true;
        slot.caps = CapabilitySet::empty();

        assert_eq!(
            ::abi::missing_caps(Op::SysSec.id(), slot.caps.raw()),
            Some(1u32 << Cap::Preferences.bit())
        );
        assert_eq!(
            unsafe {
                crate::runtime::syscall::kern_call(
                    Op::SysQuery.u32_id(),
                    sub::sys_query::RESET as usize,
                    0,
                    0,
                )
            },
            1
        );
        slot.caps = CapabilitySet::empty().with(Cap::Preferences);
        assert_eq!(
            ::abi::missing_caps(Op::SysSec.id(), slot.caps.raw()),
            Some(0)
        );
        crate::runtime::native_runner::init_native_slot();
    }
}
