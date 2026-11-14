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
        MetricId::BatteryStatus
        | MetricId::SystemFirmware
        | MetricId::SystemBootloader
        | MetricId::DeviceOwner
        | MetricId::DeviceTag => {
            let s = match id {
                MetricId::BatteryStatus => {
                    if ctx.power.is_charging {
                        "Charging"
                    } else {
                        "Discharging"
                    }
                }
                MetricId::SystemFirmware => crate::build_info::BUILD_STAMP,
                MetricId::SystemBootloader => bsp::uid::bootloader_version_str(),
                MetricId::DeviceOwner => ctx.config.system.owner_str(),
                _ => ctx.config.system.tag_str(),
            };
            out.set_str(s);
            return;
        }
        MetricId::SystemSerial | MetricId::SystemFactoryId => {
            let serial = if id == MetricId::SystemSerial {
                Some(bsp::uid::get_crockford_serial())
            } else {
                bsp::uid::get_factory_serial()
            };
            let s = serial
                .as_ref()
                .and_then(|b| utils::format::ascii_str(&b[..14]))
                .unwrap_or("None");
            out.set_str(s);
            return;
        }
        MetricId::SystemMac => {
            let mac = bsp::uid::read_mac_address();
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 17]) };
            let _ = utils::format::format_mac_address(&mac, arr);
            out.len = 17;
            return;
        }
        MetricId::SystemUptime => {
            let arr = unsafe { &mut *(out.bytes.as_mut_ptr() as *mut [u8; 8]) };
            let _ = utils::format::format_uptime_hms(ctx.power.uptime_secs, arr);
            out.len = 8;
            return;
        }
        #[cfg(feature = "settings_calib")]
        MetricId::CalibSig => {
            out.set_str(if ctx.calibration.is_valid() {
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
                    MetricId::DebugRdp => bsp::uid::get_mcu_rdp_level().short_str(),
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
        MetricId::BatteryCalibRef => (ctx.calibration.battery_calib_const() as i32, 0, ""),
        MetricId::BatteryCoreTemp => (ctx.power.mcu_temperature_deci_c() as i32, 1, " C"),

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
                _ => (s.deleted_sectors as i32, 0, " sec"),
            }
        }

        MetricId::SystemFlashSize => (py32_hal::uid::SystemInfo::flash_size_kb() as i32, 0, " KB"),
        MetricId::DeviceRadioId => (ctx.config.system.radio_id as i32, 0, ""),

        #[cfg(feature = "settings_calib")]
        MetricId::CalibXtal => (ctx.calibration.xtal_freq_low() as i32, 0, " Hz"),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibBatt => (ctx.calibration.battery_calib_const() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibLna => (ctx.calibration.lna_calib() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibMix => (ctx.calibration.mix_calib() as i32, 0, ""),
        #[cfg(feature = "settings_calib")]
        MetricId::CalibVol => (ctx.calibration.volume_gain() as i32, 0, ""),

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
            #[cfg(feature = "flight_recorder")]
            let c = crate::panic::BOOT_RECORD
                .get()
                .latest_valid_slot()
                .map_or(0, |s| s.crash_count as i32);
            #[cfg(not(feature = "flight_recorder"))]
            let c = 0;
            (c, 0, "")
        }
        _ => return,
    };

    if val >= 0 {
        out.push_int_fixed(val, dec, "", unit);
    }
}
