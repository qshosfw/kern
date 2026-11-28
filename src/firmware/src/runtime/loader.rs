//! KRX Application Package Loader & Binder (KRX-ABI 2.5.0 Appendix C).
//!
//! Provides end-to-end binary package verification, capability token checks,
//! `CapImports` GOT binding, `BlockMap` paged configuration, process table
//! registration, and execution class dispatch for installable packages.

use crate::apps::registry::{AppRegistry, DynamicAppRecord};
use kernel::app::AppMetadata;
use kernel::runtime::KrxClass;
use utils::format::InlineStr;

pub use kernel::runtime::KrxLaunchError;

static mut DYNAMIC_APP_NAME: InlineStr<19> = InlineStr::new();

pub(crate) fn dynamic_app_name() -> &'static str {
    unsafe { (&*core::ptr::addr_of!(DYNAMIC_APP_NAME)).as_str() }
}

fn store_dynamic_app_name(name: &str) -> &'static str {
    unsafe {
        let slot = &mut *core::ptr::addr_of_mut!(DYNAMIC_APP_NAME);
        slot.set(name);
        slot.as_str()
    }
}

enum LoadedSlot {
    Foreground(::abi::CapabilitySet, &'static str),
    Background(usize),
}

fn reap_err(pid: kernel::proc::Pid, err: KrxLaunchError) -> KrxLaunchError {
    crate::orchestrator::reap_pid_only(pid);
    err
}

fn validate_krx_layout<'a>(
    summary: &'a kernel::runtime::KrxSummary<'_>,
    enforce_caps: bool,
    is_bg: bool,
    zone_a_off: usize,
) -> Result<
    (
        &'a kernel::runtime::SectionExecution,
        kernel::runtime::KrxProfile,
    ),
    KrxLaunchError,
> {
    use super::background::ZONE_A_MAX_BYTES;
    let exec_class = summary
        .preamble
        .execution_class()
        .ok_or(KrxLaunchError::UnsupportedProfile)?;
    if summary.abi != ::abi::VERSION {
        return Err(KrxLaunchError::ImportBindingFailed);
    }
    let required_caps = summary.caps;
    let granted = crate::services::system_caps();
    if enforce_caps && !granted.contains_all(required_caps) {
        return Err(KrxLaunchError::CapabilityDenied(
            required_caps.without(granted).first_cap(),
        ));
    }
    let exec = summary.exec().ok_or(KrxLaunchError::MissingExecution)?;
    // Two-Tier Capability Security: packages requesting Signed System Tier capabilities
    // (`FactoryCalib`, `RawPeripheral`, `StorageAdmin`) must carry a valid `SECTION_SEAL`.
    if enforce_caps
        && (required_caps.raw() & kernel::runtime::PRIVILEGED_CAP_MASK) != 0
        && !kernel::runtime::KrxSeal::verify_summary(summary, exec)
    {
        let priv_missing = ::abi::CapabilitySet::from_raw(
            required_caps.raw() & kernel::runtime::PRIVILEGED_CAP_MASK,
        );
        return Err(KrxLaunchError::CapabilityDenied(priv_missing.first_cap()));
    }
    if enforce_caps
        && kernel::sys::SystemDiagnostics::stack_margin_bytes()
            < (exec.min_stack_bytes as usize).min(96).saturating_add(32)
    {
        return Err(KrxLaunchError::InsufficientStack);
    }
    let profile = summary.profile(exec);
    let is_paged = matches!(exec_class, KrxClass::Paged);
    let is_p0 = profile.is_structured_paged();
    let is_p1 = profile == kernel::runtime::KrxProfile::P1TransparentPaged;
    #[cfg(not(feature = "krx_paged"))]
    if is_p1 {
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    if is_bg && is_paged {
        return Err(KrxLaunchError::UnsupportedProfile);
    }
    let arena_cap = if is_bg {
        ZONE_A_MAX_BYTES
    } else {
        crate::data::arena::TRANSIENT_ARENA_BYTES
    };
    let avail = arena_cap.saturating_sub(zone_a_off);
    let load_base = (unsafe { super::native_runner::arena_base_ptr().add(zone_a_off) }) as u32;
    if profile == kernel::runtime::KrxProfile::N0NativeFixed
        && summary.preamble.link_base != load_base
    {
        return Err(KrxLaunchError::RelocationFailed);
    }
    if !is_bg && !exec.can_execute(avail, 64) {
        return Err(KrxLaunchError::InsufficientRam);
    }
    let code_limit = if is_p0 {
        avail.saturating_sub(super::native_runner::p0_overlay_size(exec))
    } else {
        avail
    };
    let pay_sz = exec.payload_size as usize;
    if is_p1 {
        if exec.payload_size > kernel::runtime::MAX_STREAMED_IMAGE_BYTES {
            return Err(KrxLaunchError::PayloadTooLarge);
        }
    } else if (is_p0 && pay_sz > code_limit) || (!is_paged && pay_sz > avail) {
        return Err(KrxLaunchError::PayloadTooLarge);
    }
    let resident_prefix = if is_p1 {
        (exec.got_offset as usize).min(code_limit)
    } else {
        exec.got_offset as usize
    };
    if (exec.got_offset & 3) != 0 || (!is_p1 && resident_prefix >= avail) {
        return Err(KrxLaunchError::ImportBindingFailed);
    }
    if exec.payload_size > 0 && !is_p1 {
        let code_size = if exec.got_offset > 0 {
            (exec.got_offset as u32).min(code_limit as u32)
        } else {
            exec.payload_size.min(code_limit as u32)
        };
        if (exec.entry_point & !1) >= code_size {
            return Err(KrxLaunchError::InvalidEntryPoint);
        }
    } else if is_p1 && (exec.entry_point & !1) >= exec.payload_size.max(2) {
        return Err(KrxLaunchError::InvalidEntryPoint);
    }
    Ok((exec, profile))
}

fn activate_bg_slot(
    bg_slot_idx: usize,
    dst_off: usize,
    dst_cap: usize,
    payload_read: usize,
    summary: &kernel::runtime::KrxSummary<'_>,
    exec: &kernel::runtime::SectionExecution,
    pid: kernel::proc::Pid,
    start_arg: u32,
) -> Result<LoadedSlot, KrxLaunchError> {
    use super::background::BgManager;
    let (entry_thumb, got_base) = super::native_runner::bind_krx_payload_at(
        dst_off,
        dst_cap,
        payload_read,
        summary,
        exec,
        pid,
        false,
    )?;
    BgManager::activate(bg_slot_idx, entry_thumb, got_base);
    if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(pid) {
        pcb.mem_bytes = dst_cap as u16;
        pcb.set_state(kernel::proc::ProcState::ServiceIdle);
    }
    let rc = BgManager::invoke_slot_op(
        bg_slot_idx,
        kernel::proc::abi::KRX_OP_START,
        start_arg as usize as *const (),
    );
    if rc == 0xFFFF_FFFF || rc == kernel::runtime::fault::KRX_FAULT_SENTINEL {
        return Err(KrxLaunchError::ExecutionFailed);
    }
    Ok(LoadedSlot::Background(bg_slot_idx))
}

/// Shared header validation, capability/RAM check, and 3-Zone package loader.
#[inline(never)]
fn load_krx_into_slot(
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    enforce_caps: bool,
    spawn: bool,
    start_arg: u32,
) -> Result<LoadedSlot, KrxLaunchError> {
    use super::background::BgManager;
    const HDR_RESERVE: usize = kernel::runtime::KrxPreamble::MAX_HEADER_SIZE;
    const SUMMARY_RESERVE: usize =
        (core::mem::size_of::<kernel::runtime::KrxSummary<'static>>() + 7) & !7;
    let (hdr_buf, summary) = super::read_krx_summary_in_scratch(storage, path)?;
    let header_len = summary.preamble.header_len as usize;
    let is_bg = spawn && summary.exec_type.is_service_or_daemon();
    let zone_a_off = crate::data::arena::zone_a_used_bytes();
    let (exec, profile) = validate_krx_layout(summary, enforce_caps, is_bg, zone_a_off)?;
    let required_caps = summary.caps;
    let pay_sz = exec.payload_size as usize;

    let raw_name = summary.title.unwrap_or("App");

    if spawn && !is_bg && super::native_runner::slot_ref().is_loaded {
        let old_pid = super::native_runner::slot_ref().pid;
        unsafe { super::native_runner::native_on_exit() };
        if old_pid.0 != 0 {
            crate::orchestrator::reap_pid_only(old_pid);
        }
    }

    let (app_name, app_id, prio) = if is_bg {
        (
            raw_name,
            0xE0 + BgManager::active_count(),
            summary.exec_type.default_priority(),
        )
    } else {
        (
            store_dynamic_app_name(raw_name),
            AppRegistry::count(),
            kernel::sched::Priority::Ui,
        )
    };

    let pid = if spawn {
        crate::orchestrator::proc_table_mut()
            .spawn_or_recycle(
                app_name,
                app_id,
                prio,
                kernel::proc::ExecType::RelocatableNative,
                required_caps,
            )
            .map_err(|_| KrxLaunchError::InsufficientRam)?
    } else {
        kernel::proc::Pid(0)
    };

    let mut bg_slot_idx = 0usize;
    let (dst_off, dst_cap) = if is_bg {
        let slot_bytes = pay_sz
            .max((exec.got_offset as usize).saturating_add(16))
            .saturating_add(exec.min_heap_bytes as usize);
        let Ok((idx, off)) =
            BgManager::alloc_slot(pid, summary.exec_type, prio, required_caps, slot_bytes, 100)
        else {
            return Err(reap_err(pid, KrxLaunchError::InsufficientRam));
        };
        bg_slot_idx = idx;
        let aligned = (slot_bytes + 7) & !7;
        super::native_runner::transient_arena_bytes_mut()[off..off + aligned].fill(0);
        (off, aligned)
    } else {
        let scratch_start =
            crate::data::arena::TRANSIENT_ARENA_BYTES - HDR_RESERVE - SUMMARY_RESERVE;
        (zone_a_off, scratch_start.saturating_sub(zone_a_off))
    };

    let to_read = if pay_sz > 0 {
        pay_sz.min(dst_cap)
    } else {
        dst_cap
    };
    let arena_buf = super::native_runner::transient_arena_bytes_mut();
    let Ok(payload_read) =
        storage.read_file_at(path, header_len, &mut arena_buf[dst_off..dst_off + to_read])
    else {
        return Err(reap_err(pid, KrxLaunchError::ReadFailed));
    };

    if is_bg {
        return activate_bg_slot(
            bg_slot_idx,
            dst_off,
            dst_cap,
            payload_read,
            summary,
            &exec,
            pid,
            start_arg,
        )
        .map_err(|e| reap_err(pid, e));
    }

    let hdr_base = hdr_buf.as_ptr() as usize;
    let max_arena = crate::data::arena::TRANSIENT_ARENA_BYTES.saturating_sub(zone_a_off);
    let load_base = (unsafe { super::native_runner::arena_base_ptr().add(zone_a_off) }) as u32;
    let is_p0 = profile.is_structured_paged();
    let is_paged = is_p0 || profile == kernel::runtime::KrxProfile::P1TransparentPaged;

    super::native_runner::init_native_slot();
    let (mut entry_thumb, got_base) = match super::native_runner::bind_krx_payload_at(
        zone_a_off,
        max_arena,
        payload_read,
        summary,
        exec,
        pid,
        is_paged,
    ) {
        Ok(v) => v,
        Err(e) => return Err(reap_err(pid, e)),
    };

    let slot = super::native_runner::slot_mut();
    slot.pid = pid;
    slot.is_paged = is_paged;
    slot.profile = profile;
    slot.got_base = got_base;
    slot.caps = required_caps;
    slot.set_pkg_path(path);
    slot.pkg_ino = storage.lookup_ino(path).unwrap_or(0);

    let overlay_size = super::native_runner::p0_overlay_size(exec) as u32;
    let code_limit = max_arena as u32 - if is_p0 { overlay_size } else { 0 };

    if is_p0 {
        let leaf_mgr = unsafe { &mut *slot.overlay.leaf };
        let (tbl_off, cnt) = summary.aux.leaf_map.map_or((0, 0), |map| {
            (
                (map.as_ptr() as usize).saturating_sub(hdr_base) as u32,
                (map.len() / kernel::runtime::LeafEntry::SIZE).min(255) as u8,
            )
        });
        leaf_mgr.configure(load_base + code_limit, overlay_size as u16, tbl_off, cnt);
    } else if is_paged {
        #[cfg(feature = "krx_paged")]
        {
            let win = unsafe { &mut *slot.overlay.window };
            win.shadow_stack.clear();
            let pay_size = if exec.payload_size > 0 {
                exec.payload_size
            } else {
                payload_read as u32
            };
            let cache_base_off =
                if exec.got_offset > 0 && (exec.got_offset as usize) < max_arena / 2 {
                    (exec.got_offset + 7) & !7
                } else {
                    0
                };
            let avail_cache =
                crate::data::arena::zone_b_cache_bytes().saturating_sub(cache_base_off as usize);
            let (bmap_off, bmap_cnt) = summary.aux.block_map.map_or((0, 0), |map| {
                (
                    (map.as_ptr() as usize).saturating_sub(hdr_base) as u32,
                    (map.len() / kernel::runtime::BlockEntry::SIZE) as u16,
                )
            });
            win.cache.configure(
                exec.page_size,
                exec.cache_blocks,
                avail_cache,
                cache_base_off,
                exec.rt_pin_mask,
                header_len as u32 + cache_base_off as u32,
                pay_size.saturating_sub(cache_base_off as u32),
                bmap_off,
                bmap_cnt,
            );
            if win.cache.page_count > 0 && (exec.entry_point & !1) >= cache_base_off as u32 {
                let rel_ep = (exec.entry_point & !1) - cache_base_off as u32;
                let init_page = (rel_ep >> win.cache.page_shift) as u16;
                let init_off = (rel_ep & ((1u32 << win.cache.page_shift) - 1)) as u16;
                if cache_base_off == 0 && bmap_off == 0 && init_page == 0 {
                    let p0_sz = win.cache.code_total_size.min(win.cache.page_size() as u32) as u16;
                    win.cache.access_clock = 1;
                    win.cache.ways[0] = kernel::runtime::CacheWayMeta {
                        resident_page: 0,
                        loaded_size: p0_sz,
                        last_access_tick: 1,
                        flags: if (exec.rt_pin_mask & 1) != 0 {
                            kernel::runtime::BLOCK_FLAG_PINNED
                        } else {
                            0
                        },
                        _pad: 0,
                    };
                    slot.current_page = 0;
                    slot.current_slot = 0;
                } else {
                    slot.is_loaded = true;
                    let resolved = super::native_runner::krx_page_fault(init_page, init_off);
                    if resolved == 0 {
                        super::native_runner::slot_mut().is_loaded = false;
                        return Err(reap_err(pid, KrxLaunchError::InvalidEntryPoint));
                    }
                    entry_thumb = resolved;
                }
            }
        }
    }

    let slot = super::native_runner::slot_mut();
    #[cfg(feature = "krx_paged")]
    let code_size = if is_paged && !is_p0 {
        let win = unsafe { &*slot.overlay.window };
        ((win.cache.cache_base_off as u32)
            + (win.cache.active_slots as u32 * win.cache.page_size() as u32))
            .min(max_arena as u32)
    } else if exec.got_offset > 0 {
        (exec.got_offset as u32).min(code_limit)
    } else {
        (payload_read as u32).min(code_limit)
    };
    #[cfg(not(feature = "krx_paged"))]
    let code_size = if exec.got_offset > 0 {
        (exec.got_offset as u32).min(code_limit)
    } else {
        (payload_read as u32).min(code_limit)
    };
    slot.code_size = code_size as u16;
    slot.entry_point = entry_thumb;
    super::native_runner::set_live_api_caps(required_caps.raw());
    slot.is_loaded = true;
    super::fault::note_loaded_pkg(pid, path);

    Ok(LoadedSlot::Foreground(required_caps, app_name))
}

/// Load a verified KRX package into Zone A (background daemon/service) or Zone B (foreground app).
///
/// `start_arg` reaches the package's `on_start` (shortcut action ID; `0` for a plain launch).
pub fn launch_krx_package(path: &str, start_arg: u32) -> Result<usize, KrxLaunchError> {
    match load_krx_into_slot(crate::services::storage_mut(), path, true, true, start_arg)? {
        LoadedSlot::Background(slot_idx) => Ok(0xE0 + slot_idx),
        LoadedSlot::Foreground(required_caps, app_name) => {
            #[cfg(feature = "filesystem")]
            crate::services::lang::bind_app(crate::services::storage_mut(), path);
            super::native_runner::set_pending_start_arg(start_arg);
            let dyn_id = AppRegistry::register_dynamic(DynamicAppRecord {
                metadata: AppMetadata { name: app_name },
                required_capabilities: required_caps,
            });
            let _ =
                crate::services::post_event(crate::apps::Shortcut::LaunchApp(dyn_id as u8).into());
            Ok(dyn_id)
        }
    }
}

/// Load and execute a standalone KRX module directly during early boot.
pub fn try_launch_boot_module(
    _lcd: &mut drivers::st7565::St7565<bsp::LcdBus>,
    storage: &mut crate::services::SystemStorage<'_>,
    path: &str,
    param: usize,
) -> Result<u32, KrxLaunchError> {
    load_krx_into_slot(storage, path, false, false, 0)?;

    let res = unsafe {
        let rc = super::native_runner::invoke_boot_start(param);
        super::native_runner::slot_mut().is_loaded = false;
        rc
    };
    Ok(res)
}

/// Display a detailed, actionable error popup for KRX package launch failures.
pub fn show_launch_error_dialog(path: &str, err: KrxLaunchError) {
    let title = crate::apps::launcher::catalog::extract_filename(path);
    let mut msg_s = utils::format::InlineStr::<40>::new();
    let msg = if let KrxLaunchError::CapabilityDenied(Some(cap)) = err {
        msg_s.set("Missing cap:\n");
        let _ = msg_s.push_str(kernel::caps::name(cap));
        msg_s.as_str()
    } else {
        err.describe()
    };

    crate::services::show_confirm(
        crate::orchestrator::compositor::TAG_SYSTEM_ALERT,
        title,
        msg,
        "OK",
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::background::BgManager;
    use kernel::runtime::{
        KrxClass, KrxExecType, KrxPreamble, SECTION_EXECUTION, SECTION_IDENTITY,
        SECTION_TYPE_LIFECYCLE, SectionExecution, SectionHeader,
    };

    extern "C" fn dummy_bg_entry(
        op: u32,
        _api: *const kernel::proc::abi::KrxApiTable,
        arg: *const (),
    ) -> u32 {
        if op == kernel::proc::abi::KRX_OP_START {
            let pid = crate::runtime::syscall::service::active_caller_pid();
            let caps = crate::runtime::native_runner::active_native_caps();
            assert!(pid > 0, "Zone A bg task must report its own PID");
            assert!(caps.contains(::abi::Cap::ServiceRegistry));
            return (arg as usize as u32).wrapping_add(1);
        }
        0
    }

    fn build_bg_krx_package(
        exec_type: KrxExecType,
        caps: ::abi::CapabilitySet,
    ) -> heapless::Vec<u8, 512> {
        let mut payload = [0u8; 16];
        // Host test trampoline pattern recognized by `invoke_krx_native` on x86_64:
        // [0x00, 0x4b, 0x18, 0x47, <usize fn_ptr>]
        payload[0..4].copy_from_slice(&[0x00, 0x4b, 0x18, 0x47]);
        let fn_addr = dummy_bg_entry as *const () as usize;
        let fn_bytes = fn_addr.to_ne_bytes();
        payload[4..4 + fn_bytes.len()].copy_from_slice(&fn_bytes);

        let mut secs = heapless::Vec::<u8, 256>::new();
        let push_sec = |out: &mut heapless::Vec<u8, 256>, id: u16, body: &[u8]| {
            let hdr = SectionHeader {
                id,
                length: (SectionHeader::SIZE + body.len()) as u16,
            };
            let _ = out.extend_from_slice(&hdr.id.to_le_bytes());
            let _ = out.extend_from_slice(&hdr.length.to_le_bytes());
            let _ = out.extend_from_slice(body);
            while (out.len() & 3) != 0 {
                let _ = out.push(0);
            }
        };

        let mut id_body = [0u8; 40];
        id_body[5..7].copy_from_slice(&::abi::VERSION.to_le_bytes());
        push_sec(&mut secs, SECTION_IDENTITY, &id_body);

        let mut type_body = [0u8; 12];
        type_body[0] = exec_type as u8;
        type_body[8..12].copy_from_slice(&caps.raw().to_le_bytes());
        push_sec(&mut secs, SECTION_TYPE_LIFECYCLE, &type_body);

        let exec = SectionExecution {
            min_stack_bytes: 64,
            min_heap_bytes: 0,
            page_size: 0,
            cache_blocks: 0,
            dma_burst_size: 0,
            entry_point: 0,
            payload_size: payload.len() as u32,
            reloc_offset: 0,
            reloc_count: 0,
            got_offset: 0,
            rt_pin_mask: 0,
            max_fault_latency_us: 0,
            reserved: [0; 2],
        };
        push_sec(&mut secs, SECTION_EXECUTION, unsafe {
            core::slice::from_raw_parts(&exec as *const _ as *const u8, SectionExecution::SIZE)
        });

        let header_len = (KrxPreamble::SIZE + secs.len()) as u16;
        let payload_crc = crypto::crc::CrcEngine::crc32(&payload);
        let mut preamble = KrxPreamble {
            ident: kernel::runtime::KRX_IDENT,
            app_type: KrxClass::Native as u8,
            format_version: 2,
            section_count: 3,
            header_len,
            flags: 0,
            payload_crc32: payload_crc,
            header_crc32: 0,
            link_base: 0,
        };

        let mut pkg = heapless::Vec::<u8, 512>::new();
        let _ = pkg.extend_from_slice(unsafe {
            core::slice::from_raw_parts(&preamble as *const _ as *const u8, KrxPreamble::SIZE)
        });
        let _ = pkg.extend_from_slice(&secs);
        preamble.header_crc32 = crypto::crc::CrcEngine::crc32(&pkg[KrxPreamble::SIZE..]);
        let pre_bytes = unsafe {
            core::slice::from_raw_parts(&preamble as *const _ as *const u8, KrxPreamble::SIZE)
        };
        pkg[..KrxPreamble::SIZE].copy_from_slice(pre_bytes);
        let _ = pkg.extend_from_slice(&payload);
        pkg
    }

    #[test]
    fn zone_a_daemon_loads_with_own_pid_and_caps_and_coexists_with_zone_b() {
        BgManager::reset();
        crate::runtime::native_runner::init_native_slot();

        let caps = ::abi::CapabilitySet::empty().with(::abi::Cap::ServiceRegistry);
        let pkg = build_bg_krx_package(KrxExecType::Daemon, caps);
        let summary = kernel::runtime::KrxPreamble::summarize(&pkg).expect("valid KRX summary");
        let (exec, _) = validate_krx_layout(&summary, true, true, 0)
            .expect("Zone A daemon layout must validate");

        let pid = crate::orchestrator::proc_table_mut()
            .spawn_or_recycle(
                "BgTest",
                0xE0,
                summary.exec_type.default_priority(),
                kernel::proc::ExecType::RelocatableNative,
                caps,
            )
            .expect("spawn daemon PCB");
        let payload = &pkg[summary.preamble.header_len as usize..];
        let (slot_idx, dst_off) = BgManager::alloc_slot(
            pid,
            summary.exec_type,
            summary.exec_type.default_priority(),
            caps,
            payload.len(),
            100,
        )
        .expect("alloc Zone A slot");
        let dst_cap = (payload.len() + 7) & !7;
        let arena = super::super::native_runner::transient_arena_bytes_mut();
        arena[dst_off..dst_off + payload.len()].copy_from_slice(payload);

        let loaded = activate_bg_slot(
            slot_idx,
            dst_off,
            dst_cap,
            payload.len(),
            &summary,
            exec,
            pid,
            41,
        )
        .expect("Zone A daemon must bind and run KRX_OP_START");

        assert!(matches!(loaded, LoadedSlot::Background(idx) if idx == slot_idx));
        assert_eq!(BgManager::active_count(), 1);
        let zone_a_bytes = crate::data::arena::zone_a_used_bytes();
        assert!(zone_a_bytes > 0);
        assert_eq!(
            crate::data::arena::zone_b_base_ptr() as usize,
            crate::data::arena::arena_base_ptr() as usize + zone_a_bytes
        );

        let pcb = crate::orchestrator::proc_table()
            .find(pid)
            .expect("daemon PCB must exist");
        assert_eq!(pcb.state, kernel::proc::ProcState::ServiceIdle);

        assert!(BgManager::stop_pid(pid));
        assert_eq!(BgManager::active_count(), 0);
        assert_eq!(crate::data::arena::zone_a_used_bytes(), 0);
        crate::orchestrator::reap_pid_only(pid);
    }

    #[test]
    fn privileged_capabilities_require_valid_section_seal() {
        let priv_caps = ::abi::CapabilitySet::empty()
            .with(::abi::Cap::DisplayDirect)
            .with(::abi::Cap::StorageAdmin);
        let unsigned_pkg = build_bg_krx_package(KrxExecType::App, priv_caps);
        let summary =
            kernel::runtime::KrxPreamble::summarize(&unsigned_pkg).expect("valid summary");
        assert_eq!(
            validate_krx_layout(&summary, true, false, 0).unwrap_err(),
            KrxLaunchError::CapabilityDenied(Some(::abi::Cap::StorageAdmin))
        );
    }
}
