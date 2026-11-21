//! Shared Live Metrics Formatting Utilities for OS Settings.
//!
//! Provides zero-alloc, shift-only integer and unit formatting routines
//! shared across settings domain providers (battery, device, system, storage).

use super::page::MetricId;
use crate::SysContext;
use gfx::gui::modules::menu::ShortString;
use kernel::service::PowerService;

/// Centralized zero-allocation metric dispatch for the Settings application.
#[allow(unreachable_patterns)]
#[inline(never)]
pub fn format_metric(id: MetricId, ctx: &mut SysContext, out: &mut ShortString) {
    #[allow(unreachable_patterns)]
    let (val, dec, unit) = match id {
        // 1. Direct string & buffer metrics that format directly into `out` and return:
        MetricId::BatteryStatus | MetricId::DeviceOwner | MetricId::DeviceTag => {
            let s = match id {
                MetricId::BatteryStatus => {
                    utils::format::nth_cstr(b"Discharging\0Charging\0", ctx.power.is_charging as u8)
                }
                MetricId::DeviceOwner => ctx.config.system.owner_str(),
                _ => ctx.config.system.tag_str(),
            };
            out.set_str(s);
            return;
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibSig => {
            out.set_str(if ctx.calibration().is_valid() {
                "Valid"
            } else {
                "None"
            });
            return;
        }
        #[cfg(feature = "settings_debug")]
        MetricId::DebugSwd | MetricId::DebugRdp | MetricId::DebugDevKey | MetricId::DebugMkHash => {
            #[cfg(feature = "passcode")]
            {
                let sec =
                    crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
                match id {
                    MetricId::DebugSwd => out.set_str(if sec.swd_open { "Open" } else { "Locked" }),
                    MetricId::DebugRdp => out.set_str(sec.rdp.short_str()),
                    MetricId::DebugDevKey => match sec.dev_key_hash {
                        Some(h) => *out = ShortString::from_hex32(h),
                        None => out.set_str("None"),
                    },
                    _ => match sec.master_key_hash {
                        Some(h) => *out = ShortString::from_hex32(h),
                        None => out.set_str("None"),
                    },
                }
            }
            #[cfg(not(feature = "passcode"))]
            {
                let s = match id {
                    MetricId::DebugSwd => {
                        if crate::security::port_lock::is_swd_enabled() {
                            "Open"
                        } else {
                            "Locked"
                        }
                    }
                    MetricId::DebugRdp => crate::services::primitives::mcu_rdp_short_str(),
                    _ => "None",
                };
                out.set_str(s);
            }
            return;
        }

        // 2. Numeric metrics dispatched through a single unified integer formatter:
        MetricId::BatteryVoltage => {
            let mv = ctx.power.battery_voltage_mv() as u32;
            (((mv * 52429) >> 19) as i32, 2, "V")
        }
        MetricId::BatteryPercentage => (ctx.power.battery_percentage() as i32, 0, "%"),
        MetricId::BatteryVdda => (ctx.power.vdda_voltage_mv() as i32, 0, " mV"),
        MetricId::BatteryCalibRef => (ctx.calibration().battery_calib_const() as i32, 0, ""),
        MetricId::BatteryCoreTemp => (ctx.power.mcu_temperature_deci_c() as i32, 1, " C"),

        #[cfg(feature = "filesystem")]
        MetricId::StorageUsed
        | MetricId::StorageFree
        | MetricId::StorageFiles
        | MetricId::StorageWear
        | MetricId::StorageDeleted => {
            let s = ctx.storage.stats();
            match id {
                MetricId::StorageUsed => (s.used_sectors as i32 * 4, 0, " KB"),
                MetricId::StorageFree => (s.free_sectors as i32 * 4, 0, " KB"),
                MetricId::StorageFiles => (s.active_files as i32, 0, ""),
                MetricId::StorageWear => (s.wear_cycles as i32, 0, ""),
                _ => (s.deleted_sectors as i32 * 4, 0, " KB"),
            }
        }

        MetricId::DeviceRadioId => (ctx.config.system.radio_id as i32, 0, ""),

        #[cfg(feature = "settings_calib")]
        MetricId::CalibXtal => (ctx.calibration().xtal_freq_low() as i32, 0, " Hz"),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibBatt => (ctx.calibration().battery_calib_const() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibLna => (ctx.calibration().lna_calib() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibMix => (ctx.calibration().mix_calib() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibVol => (ctx.calibration().volume_gain() as i32, 0, ""),

        #[cfg(feature = "settings_debug")]
        MetricId::DebugStackMargin => (
            kernel::sys::SystemDiagnostics::stack_margin_bytes() as i32,
            0,
            " B",
        ),
        #[cfg(feature = "settings_debug")]
        MetricId::DebugStackPeak => (
            kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as i32,
            0,
            " B",
        ),
        #[cfg(feature = "settings_debug")]
        MetricId::DebugCrashes => {
            let c = crate::panic::BOOT_RECORD
                .get()
                .latest_valid_slot()
                .map_or(0, |s| s.crash_count as i32);
            (c, 0, "")
        }
        _ => return,
    };

    if val >= 0 {
        out.push_int_fixed(val, dec, "", unit);
    }
}
