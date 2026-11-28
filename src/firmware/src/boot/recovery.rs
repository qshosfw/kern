//! Boot stage 2 — recovery entry and the cooperative boot-module chain.
//!
//! Recovery is entered *before* storage mount / watchdog start when either the backup
//! domain carries a bootloader-hold request (set by a prior `RebootMode::Bootloader`) or
//! the attended operator holds `PTT + Side 2` at power-on. Headless nodes never honour the
//! chord: their only path into recovery is an authenticated RigWire reboot request.
//!
//! The rich tooling lives in installable boot modules (`/apps/*.mod`); each module returns
//! a [`Next`] code that selects the following module, forming a small total state machine
//! ([`next_module`]) that always terminates in the OS boot or the raw DFU handoff.

use super::backup;
use crate::hardware::SystemHardware;
use kernel::boot::BackupCrashTag;

/// Warm DFU → Recovery transition: panel and backlight stay live across the reset.
pub const WARM_TOOLS_MASK: u8 =
    BackupCrashTag::FLAG_BOOTLOADER_HOLD | BackupCrashTag::FLAG_DFU_TOOLS_REQ;

/// `true` when this boot is a warm hand-back from the DFU tool into recovery.
#[inline(always)]
pub fn is_warm_tools_boot() -> bool {
    backup::has(WARM_TOOLS_MASK)
}

/// Boot modules forming the recovery chain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Module {
    Dfu,
    Multiboot,
    Calibration,
    Recovery,
}

impl Module {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Dfu => "/apps/dfu.mod",
            Self::Multiboot => "/apps/multiboot.mod",
            Self::Calibration => "/apps/calib.mod",
            Self::Recovery => "/apps/recovery.mod",
        }
    }
}

/// Exit codes returned by a boot module's `start` entry.
pub mod next {
    pub const BOOT_OS: u32 = 0;
    pub const DFU: u32 = 2;
    pub const MULTIBOOT: u32 = 3;
    pub const CALIBRATION: u32 = 4;
    pub const RECOVERY: u32 = 5;
}

/// One hop of the chain: the module to run and whether the recovery menu launched it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    pub module: Module,
    /// Launched as a sub-tool of the recovery menu: "back" (`BOOT_OS`) returns to the menu.
    pub from_menu: bool,
}

impl Step {
    pub const fn root(module: Module) -> Self {
        Self {
            module,
            from_menu: false,
        }
    }

    /// Transition after this step finished with `outcome` (`Err` = module failed to load).
    ///
    /// Sub-tools return to the menu that launched them; a missing module degrades along
    /// recovery → multiboot → DFU → OS, so a flash without any boot module still boots.
    pub const fn next(self, outcome: Result<u32, ()>) -> Option<Step> {
        let from_menu = matches!(self.module, Module::Recovery);
        match outcome {
            Ok(next::DFU) => Some(Step::root(Module::Dfu)),
            Ok(next::RECOVERY) => Some(Step::root(Module::Recovery)),
            Ok(next::MULTIBOOT) => Some(Step {
                module: Module::Multiboot,
                from_menu,
            }),
            Ok(next::CALIBRATION) => Some(Step {
                module: Module::Calibration,
                from_menu,
            }),
            _ if self.from_menu => Some(Step::root(Module::Recovery)),
            Ok(_) => None,
            Err(()) => match self.module {
                Module::Recovery => Some(Step::root(Module::Multiboot)),
                Module::Multiboot => Some(Step::root(Module::Dfu)),
                Module::Calibration | Module::Dfu => None,
            },
        }
    }
}

/// Physical recovery chord (`PTT + Side 2`), honoured only on an attended radio.
#[inline(always)]
fn chord_held() -> bool {
    #[cfg(all(not(test), target_arch = "arm"))]
    {
        const CHORD: u32 = (1 << 18) | (1 << 1);
        crate::system::admits_physical_input()
            && (drivers::keyboard::KeypadScanner::poll_raw_mask() & CHORD) == CHORD
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    false
}

/// Run the recovery chain when requested. Returns only when the OS should keep booting.
pub fn run(hw: &mut SystemHardware, warm_tools: bool) {
    let hold = backup::has(BackupCrashTag::FLAG_BOOTLOADER_HOLD);
    if !hold && !chord_held() {
        return;
    }
    if hold {
        backup::update(BackupCrashTag::FLAG_CLEAN_REBOOT, WARM_TOOLS_MASK);
    }

    #[cfg(feature = "krx_native")]
    if hw.filesystem.fs.mount().is_ok() {
        hw.lcd.turn_on();
        drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
        bsp::pins::BACKLIGHT.set_high();

        let mut step = Some(Step::root(if warm_tools {
            Module::Recovery
        } else {
            Module::Dfu
        }));
        while let Some(cur) = step {
            let path = cur.module.path();
            let outcome =
                crate::runtime::try_launch_boot_module(&mut hw.lcd, hw.filesystem, path, 1);
            step = cur.next(outcome.map_err(|_| ()));
        }

        drivers::backlight::BACKLIGHT_DUTY.store(0, core::sync::atomic::Ordering::Relaxed);
        bsp::pins::BACKLIGHT.set_low();
        hw.lcd.turn_off();
    }
    #[cfg(not(feature = "krx_native"))]
    let _ = (&hw, warm_tools);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(module: Module) -> Step {
        Step {
            module,
            from_menu: true,
        }
    }

    #[test]
    fn failure_chain_always_terminates() {
        for module in [
            Module::Dfu,
            Module::Multiboot,
            Module::Calibration,
            Module::Recovery,
        ] {
            let mut cur = Some(Step::root(module));
            let mut hops = 0;
            while let Some(step) = cur {
                cur = step.next(Err(()));
                hops += 1;
                assert!(
                    hops <= 4,
                    "failure chain from {module:?} does not terminate"
                );
            }
        }
    }

    #[test]
    fn recovery_menu_tools_return_to_the_menu() {
        let rec = Step::root(Module::Recovery);
        assert_eq!(rec.next(Ok(next::MULTIBOOT)), Some(menu(Module::Multiboot)));
        assert_eq!(
            rec.next(Ok(next::CALIBRATION)),
            Some(menu(Module::Calibration))
        );
        // "Back" from a sub-tool, or a sub-tool that fails to load, re-opens the menu.
        assert_eq!(menu(Module::Multiboot).next(Ok(next::BOOT_OS)), Some(rec));
        assert_eq!(menu(Module::Calibration).next(Err(())), Some(rec));
        // A sub-tool may still hand off to DFU directly.
        assert_eq!(
            menu(Module::Multiboot).next(Ok(next::DFU)),
            Some(Step::root(Module::Dfu))
        );
        assert_eq!(rec.next(Ok(next::BOOT_OS)), None);
    }

    #[test]
    fn missing_modules_degrade_to_os_boot() {
        assert_eq!(
            Step::root(Module::Recovery).next(Err(())),
            Some(Step::root(Module::Multiboot))
        );
        assert_eq!(
            Step::root(Module::Multiboot).next(Err(())),
            Some(Step::root(Module::Dfu))
        );
        assert_eq!(Step::root(Module::Dfu).next(Err(())), None);
        // A root multiboot picker (reached by degradation) exits to the OS on "back".
        assert_eq!(Step::root(Module::Multiboot).next(Ok(next::BOOT_OS)), None);
        assert_eq!(
            Step::root(Module::Dfu).next(Ok(next::RECOVERY)),
            Some(Step::root(Module::Recovery))
        );
    }
}
