//! Shared Live Metrics Formatting Utilities for OS Settings.
//!
//! Provides zero-alloc, shift-only integer and unit formatting routines
//! shared across settings domain providers (battery, device, system, storage).

use super::page::MetricId;
use crate::SysContext;
use gfx::gui::modules::menu::ShortString;
use kernel::service::PowerService;

/// Shared utility for formatting integer metrics with units and decimals without dynamic allocation.
#[inline(never)]
pub fn format_metric_int(out: &mut ShortString, val: i32, dec: u8, unit: &'static str) {
    if val >= 0 {
        out.push_int_fixed(val, dec, "", unit);
    }
}

/// Centralized zero-allocation metric dispatch for the Settings application.
#[inline(never)]
pub fn format_metric(id: MetricId, ctx: &mut SysContext, out: &mut ShortString) {
    match id {
        MetricId::BatteryVoltage => {
            let mv = ctx.power.battery_voltage_mv() as u32;
            let v = ((mv * 52429) >> 19) as i32;
            format_metric_int(out, v, 2, "V");
        }
        MetricId::BatteryPercentage => {
            format_metric_int(out, ctx.power.battery_percentage() as i32, 0, "%");
        }
        MetricId::BatteryStatus => {
            out.set_str(if ctx.power.is_charging {
                "Charging"
            } else {
                "Discharging"
            });
        }
        MetricId::BatteryVdda => {
            format_metric_int(out, ctx.power.vdda_voltage_mv() as i32, 0, " mV");
        }
        MetricId::BatteryCalibRef => {
            format_metric_int(out, ctx.calibration.battery_calib_const() as i32, 0, "");
        }
        MetricId::BatteryCoreTemp => {
            format_metric_int(out, ctx.power.mcu_temperature_deci_c() as i32, 1, " C");
        }

        MetricId::StorageUsed => {
            let stats = ctx.storage.stats();
            format_metric_int(out, stats.used_sectors as i32 * 4, 0, " KB");
        }
        MetricId::StorageFree => {
            let stats = ctx.storage.stats();
            format_metric_int(out, stats.free_sectors as i32 * 4, 0, " KB");
        }
        MetricId::StorageFiles => {
            let stats = ctx.storage.stats();
            format_metric_int(out, stats.active_files as i32, 0, "");
        }
        MetricId::StorageWear => {
            let stats = ctx.storage.stats();
            format_metric_int(out, stats.wear_cycles as i32, 0, "");
        }
        MetricId::StorageDeleted => {
            let stats = ctx.storage.stats();
            format_metric_int(out, stats.deleted_sectors as i32, 0, " sec");
        }

        MetricId::SystemFirmware => {
            out.set_str(crate::build_info::BUILD_STAMP);
        }
        MetricId::SystemSerial => {
            let serial = bsp::uid::get_crockford_serial();
            out.set_str(unsafe { core::str::from_utf8_unchecked(&serial[..14]) });
        }
        MetricId::SystemMac => {
            let mac = bsp::uid::read_mac_address();
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 17]) };
            let _ = utils::format::format_mac_address(&mac, arr);
            out.len = 17;
        }
        MetricId::SystemUptime => {
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 8]) };
            let _ = utils::format::format_uptime_hms(ctx.power.uptime_secs, arr);
            out.len = 8;
        }

        #[cfg(feature = "settings_calib")]
        MetricId::CalibSig => {
            out.set_str(if ctx.calibration.is_valid() {
                "Valid"
            } else {
                "None"
            });
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibXtal => {
            format_metric_int(out, ctx.calibration.xtal_freq_low() as i32, 0, " Hz");
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibBatt => {
            format_metric_int(out, ctx.calibration.battery_calib_const() as i32, 0, "");
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibLna => {
            format_metric_int(out, ctx.calibration.lna_calib() as i32, 0, "");
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibMix => {
            format_metric_int(out, ctx.calibration.mix_calib() as i32, 0, "");
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibVol => {
            format_metric_int(out, ctx.calibration.volume_gain() as i32, 0, "");
        }

        #[cfg(feature = "settings_debug")]
        MetricId::DebugStackMargin => {
            format_metric_int(
                out,
                kernel::sys::SystemDiagnostics::stack_margin_bytes() as i32,
                0,
                " B",
            );
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugStackPeak => {
            format_metric_int(
                out,
                kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as i32,
                0,
                " B",
            );
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugCrashes => {
            #[cfg(feature = "flight_recorder")]
            let c = crate::panic::BOOT_RECORD
                .get()
                .latest_valid_slot()
                .map_or(0, |s| s.crash_count as i32);
            #[cfg(not(feature = "flight_recorder"))]
            let c = 0;
            format_metric_int(out, c, 0, "");
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugSwd => {
            #[cfg(feature = "passcode")]
            let open =
                crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode).swd_open;
            #[cfg(not(feature = "passcode"))]
            let open = crate::security::port_lock::is_swd_enabled();
            out.set_str(if open { "Open" } else { "Locked" });
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugRdp => {
            #[cfg(feature = "passcode")]
            let s = crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode)
                .rdp
                .short_str();
            #[cfg(not(feature = "passcode"))]
            let s = py32_hal::flash::InternalFlash::rdp_status().short_str();
            out.set_str(s);
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugDevKey => {
            #[cfg(feature = "passcode")]
            {
                let sec =
                    crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
                match sec.dev_key_hash {
                    Some(h) => *out = ShortString::from_hex32(h),
                    None => out.set_str("None"),
                }
            }
            #[cfg(not(feature = "passcode"))]
            out.set_str("None");
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugMkHash => {
            #[cfg(feature = "passcode")]
            {
                let sec =
                    crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
                match sec.master_key_hash {
                    Some(h) => *out = ShortString::from_hex32(h),
                    None => out.set_str("None"),
                }
            }
            #[cfg(not(feature = "passcode"))]
            out.set_str("None");
        }

        MetricId::DeviceOwner => {
            out.set_str(ctx.config.system.owner_str());
        }
        MetricId::DeviceTag => {
            out.set_str(ctx.config.system.tag_str());
        }
        MetricId::DeviceRadioId => {
            format_metric_int(out, ctx.config.system.radio_id as i32, 0, "");
        }
    }
}
