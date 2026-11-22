#![no_std]
#![cfg_attr(not(test), no_main)]

use crate::config::MasterConfig;
#[cfg_attr(test, allow(unused_imports))]
use crate::hardware::SystemHardware;
use crate::services::RadioState;
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
#[cfg_attr(test, allow(unused_imports))]
use cortex_m_rt::{entry, exception};
use drivers::battery::SystemPower;
#[cfg_attr(test, allow(unused_imports))]
use drivers::bk4829::Bk4829;
#[cfg_attr(test, allow(unused_imports))]
use drivers::st7565::St7565;
use heapless::Deque;
use kernel::event::Event;

#[cfg_attr(test, allow(unused_imports))]
use bsp::Board;
use py32_hal as hal;
#[cfg_attr(test, allow(unused_imports))]
use py32_pac as pac;

pub mod apps;
pub mod boot;
pub mod build_info;
pub mod config;
pub mod data;
pub mod hardware;
mod intrinsics;
pub mod orchestrator;
mod panic;
pub mod runtime;
pub mod security;
pub mod services;
pub mod system;

pub use crate::apps::AppRegistry;
pub use crate::services::SysContext;

/// Global event queue for zero-alloc event-driven kernel dispatch
pub static EVENT_QUEUE: Mutex<RefCell<Deque<Event, 8>>> = Mutex::new(RefCell::new(Deque::new()));

/// Global memory buffer allocator for zero-copy IPC
pub static MBUF_POOL: utils::mbuf::MBufPool = utils::mbuf::MBufPool::new();

#[used]
#[unsafe(link_section = ".uninit")]
pub static BOOT_RECORD: panic::BootRecordCell = panic::BootRecordCell::new();

#[used]
#[unsafe(link_section = ".uninit")]
pub static mut FAULT_EXTRA: kernel::boot::FaultExtraRegs = kernel::boot::FaultExtraRegs::empty();

pub static CONFIG_INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[unsafe(link_section = ".uninit")]
pub static mut MASTER_CONFIG: core::mem::MaybeUninit<MasterConfig> =
    core::mem::MaybeUninit::uninit();
pub static mut CALIBRATION: services::calibration::SystemCalibration =
    services::calibration::SystemCalibration::empty();
pub static mut PASSCODE_STATE: crate::security::PasscodeState =
    crate::security::PasscodeState::empty();
#[unsafe(link_section = ".uninit")]
pub static mut RADIO_STATE: core::mem::MaybeUninit<RadioState> = core::mem::MaybeUninit::uninit();
pub static mut POWER_SERVICE: SystemPower = SystemPower::empty();
pub static mut CANVAS: gfx::Canvas = gfx::Canvas::new();
pub static mut AUDIO_CAPTURE: drivers::audio::AudioCaptureDriver<bsp::BspVoiceAdc> =
    drivers::audio::AudioCaptureDriver::new(bsp::BspVoiceAdc);

#[inline(always)]
pub fn master_config() -> &'static mut MasterConfig {
    unsafe { (*core::ptr::addr_of_mut!(MASTER_CONFIG)).assume_init_mut() }
}

#[inline(always)]
pub fn master_config_ref() -> &'static MasterConfig {
    unsafe { (*core::ptr::addr_of!(MASTER_CONFIG)).assume_init_ref() }
}

#[inline(always)]
pub fn radio_state() -> &'static mut RadioState {
    unsafe { (*core::ptr::addr_of_mut!(RADIO_STATE)).assume_init_mut() }
}

#[inline(always)]
pub fn radio_state_ref() -> &'static RadioState {
    unsafe { (*core::ptr::addr_of!(RADIO_STATE)).assume_init_ref() }
}

#[inline(always)]
pub fn system_canvas() -> &'static mut gfx::Canvas {
    unsafe { &mut *core::ptr::addr_of_mut!(CANVAS) }
}

#[inline(always)]
pub fn audio_capture_mut() -> &'static mut drivers::audio::AudioCaptureDriver<bsp::BspVoiceAdc> {
    unsafe { &mut *core::ptr::addr_of_mut!(AUDIO_CAPTURE) }
}

#[cfg(not(test))]
#[inline(always)]
pub fn kern_hw() -> &'static mut SystemHardware<'static> {
    unsafe { (*core::ptr::addr_of_mut!(KERN_HW)).assume_init_mut() }
}

#[cfg(not(test))]
#[inline(always)]
pub fn kern_fs() -> &'static mut crate::services::SystemStorage<'static> {
    unsafe { (*core::ptr::addr_of_mut!(KERN_FS)).assume_init_mut() }
}

/// SysTick interrupt handler for system uptime tracking
#[exception]
fn SysTick() {
    hal::delay::on_systick();
    drivers::backlight::systick_pwm_tick(&mut bsp::BspBacklight);
}

/// EXTI lines 4..=15 interrupt handler (deep-sleep Exit key PB15 & UART RX PA10 wake).
#[cfg(all(not(test), target_arch = "arm"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn EXTI4_15() {
    let pr = unsafe { pac::exti::EXTI_PR.read_volatile() };
    let mask = pr & 0xFFF0;
    if mask != 0 {
        let prev = crate::system::WAKE_EXTI_FLAGS.load(core::sync::atomic::Ordering::Relaxed);
        crate::system::WAKE_EXTI_FLAGS.store(prev | mask, core::sync::atomic::Ordering::Relaxed);
        unsafe {
            pac::exti::EXTI_PR.write_volatile(mask);
        }
    }
}

/// TIM6 / LPTIM1 / DAC interrupt handler (deep-sleep 4s watchdog cadence wakeup).
#[cfg(all(not(test), target_arch = "arm"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn TIM6_LPTIM1_DAC() {
    hal::lptim::LowPowerTimer::clear_expired();
    hal::exti::clear_pending(hal::exti::LINE_LPTIM1);
}

/// PVD (Programmable Voltage Detector) interrupt handler (IRQ 1, EXTI line 16).
/// Immediately quenches BK4829 PA bias if VDD droops below ~2.8V during high-power TX,
/// preventing MCU brownout resets and SPI Flash corruption.
#[cfg(all(not(test), target_arch = "arm"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn PVD() {
    hal::exti::clear_pending(hal::exti::LINE_PVD);
    if hal::pwr::pvd_below_threshold() {
        #[cfg(feature = "radio")]
        {
            let coord = &mut radio_state().coord;
            if coord.is_transmitting {
                bsp::bk4829_fault_quench();
                coord.is_transmitting = false;
                coord.tx_state = kernel::service::radio::TxState::Idle;
            }
        }
    }
}

/// USART1 interrupt handler (IRQ 27) — drains RX bytes into the HAL 128 B SPSC ring buffer.
#[cfg(all(not(test), target_arch = "arm", feature = "uart"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn USART1() {
    hal::uart::on_usart1_irq();
}

/// USBD (USB 2.0 Full-Speed Device) interrupt handler (IRQ 31).
/// Services bus resets, EP0 control transfers (`SET_ADDRESS` within 2 ms), and CDC endpoints
/// immediately so LCD blits, flash I/O, and BK4829 PA ramps never starve USB enumeration or I/O.
#[cfg(all(not(test), target_arch = "arm", feature = "usb"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn USB() {
    drivers::usb::on_usb_irq();
}

#[cfg(not(test))]
#[unsafe(export_name = "__pre_init")]
unsafe extern "C" fn pre_init() {
    // Stop SysTick and restore application VTOR before crt0 stack/BSS setup touches upper SRAM,
    // ensuring a hooked DFU session cannot vector into overwritten SRAM during jumpback.
    unsafe {
        (0xE000_E010 as *mut u32).write_volatile(0);
        (0xE000_ED04 as *mut u32).write_volatile(1 << 25);
        pac::SCB_VTOR.write_volatile(hal::flash::FIRMWARE_MIN_ADDR);

        // Keep PF8 backlight solidly lit across warm DFU/Recovery or Crash SYSRESETREQ before crt0 runs.
        let apb1 = 0x4002_103C as *mut u32;
        apb1.write_volatile(apb1.read_volatile() | (1 << 28) | (1 << 10));
        let hi = (0x4000_2820 as *const u32).read_volatile() & 0xFFFF;
        let lo = (0x4000_2824 as *const u32).read_volatile() & 0xFFFF;
        let bkp_flags = (lo >> 8) as u8;
        if ((hi >> 8) as u8) == kernel::boot::BackupCrashTag::IDENT_BYTE
            && ((bkp_flags & 0x48) == 0x48
                || (bkp_flags & kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH) != 0)
        {
            let iopenr = 0x4002_1034 as *mut u32;
            iopenr.write_volatile(iopenr.read_volatile() | (1 << 5));
            let gpiof_moder = 0x5000_1400 as *mut u32;
            (0x5000_1418 as *mut u32).write_volatile(1 << 8);
            gpiof_moder.write_volatile((gpiof_moder.read_volatile() & !(3 << 16)) | (1 << 16));
        }
    }
}

/// Bind active factory/user calibration bytes and battery discharge curve to runtime drivers.
#[inline(never)]
pub(crate) fn apply_active_calibration<'a>(radio: &mut Bk4829<'a, bsp::RadioBus>) {
    let calib = unsafe { &*core::ptr::addr_of!(CALIBRATION) };
    radio.attach_calibration(calib.raw_bytes());
    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
    power_service
        .battery_driver
        .load_calibration_curve(calib.battery_curve());
}

/// Boot stage 0 — early hardware: SRAM state in place, VTOR, interrupts, board GPIO and
/// reset-cause classification (stage 1) before any peripheral is driven.
#[cfg(not(test))]
#[inline(never)]
fn early_init() -> Board {
    kernel::sys::SystemDiagnostics::paint_stack();

    unsafe {
        MasterConfig::init_in_place(core::ptr::addr_of_mut!(MASTER_CONFIG) as *mut MasterConfig);
        CONFIG_INITIALIZED.store(true, core::sync::atomic::Ordering::Release);
        RadioState::init_in_place(core::ptr::addr_of_mut!(RADIO_STATE) as *mut RadioState);
        (*core::ptr::addr_of_mut!(CALIBRATION)).init_defaults();
        pac::SCB_VTOR.write_volatile(hal::flash::FIRMWARE_MIN_ADDR);
        cortex_m::interrupt::enable();
    }

    #[cfg(feature = "krx_native")]
    let warm_dfu_tools = boot::recovery::is_warm_tools_boot();
    #[cfg(not(feature = "krx_native"))]
    let warm_dfu_tools = false;
    let warm_crash =
        (boot::backup::read().flags & kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH) != 0;
    if warm_dfu_tools || warm_crash {
        drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
    }

    let board = Board::init();
    #[cfg(target_arch = "arm")]
    {
        // Arm hardware PVD at ~2.8V with digital glitch filter for instant TX brownout quench.
        hal::pwr::enable_pvd(4, true);
        unsafe {
            pac::nvic_clear_pending(pac::Interrupt::PVD as u8);
            pac::nvic_enable(pac::Interrupt::PVD as u8);
        }
    }
    boot::reset::check_reset_cause();
    if warm_dfu_tools || warm_crash {
        bsp::pins::BACKLIGHT.set_high();
        if warm_crash {
            bsp::set_status_led(bsp::StatusLed::Red);
        }
    } else {
        bsp::pins::BACKLIGHT.set_low();
        let _ = hal::uart::write_line("KERN v0.1.0");
        let _ = hal::uart::write_line(build_info::BUILD_STAMP);
    }
    board
}

/// Final boot stage — handoff: bind calibration, apply the durable headless choice, select
/// the startup app and cache the package catalogue before the system loop takes over.
#[cfg(not(test))]
#[inline(never)]
fn handoff(hw: &mut SystemHardware) {
    apply_active_calibration(&mut hw.radio);
    system::restore_from_config(hw);

    let startup_app = if master_config_ref().apps.startup_app == 1 {
        crate::apps::APP_ID_VFO
    } else {
        crate::apps::APP_ID_LAUNCHER
    };
    hw.orchestrator.set_startup_app(startup_app);

    #[cfg(feature = "uart")]
    bsp::Board::uart_set_baud(master_config_ref().system.baud_rate());

    #[cfg(feature = "filesystem")]
    crate::apps::launcher::catalog::get_catalog().scan(hw.filesystem);

    hw.init_startup_app();

    // Re-paint stack watermark after boot provisioning so steady-state introspection is clean
    kernel::sys::SystemDiagnostics::paint_stack();
}

#[cfg(not(test))]
#[unsafe(link_section = ".uninit")]
static mut KERN_BOARD: core::mem::MaybeUninit<Board> = core::mem::MaybeUninit::uninit();

#[cfg(all(not(test), feature = "filesystem"))]
#[unsafe(link_section = ".uninit")]
static mut KERN_FLASH: core::mem::MaybeUninit<drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>> =
    core::mem::MaybeUninit::uninit();

#[cfg(not(test))]
#[unsafe(link_section = ".uninit")]
static mut KERN_FS: core::mem::MaybeUninit<crate::services::SystemStorage<'static>> =
    core::mem::MaybeUninit::uninit();

#[cfg(not(test))]
#[unsafe(link_section = ".uninit")]
static mut KERN_HW: core::mem::MaybeUninit<SystemHardware<'static>> =
    core::mem::MaybeUninit::uninit();

#[cfg(not(test))]
#[entry]
fn main() -> ! {
    let _board = unsafe {
        let p = core::ptr::addr_of_mut!(KERN_BOARD) as *mut Board;
        core::ptr::write(p, early_init());
        &mut *p
    };
    let boot_start_ms = hal::delay::millis();

    #[cfg(feature = "filesystem")]
    let filesystem = unsafe {
        let flash_p = core::ptr::addr_of_mut!(KERN_FLASH)
            as *mut drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>;
        core::ptr::write(
            flash_p,
            drivers::py25q16::Py25q16FlashDriver::new(bsp::FlashBus),
        );
        let flash = &mut *flash_p;
        flash.init();
        let fs_p = core::ptr::addr_of_mut!(KERN_FS) as *mut crate::services::SystemStorage<'static>;
        drivers::filesystem::FileSystemDriver::init_in_place(fs_p, flash);
        &mut *fs_p
    };
    #[cfg(not(feature = "filesystem"))]
    let filesystem = unsafe {
        let fs_p = core::ptr::addr_of_mut!(KERN_FS) as *mut crate::services::SystemStorage<'static>;
        core::ptr::write(fs_p, crate::services::storage::NoStorage);
        &mut *fs_p
    };

    let hw = unsafe {
        let p = core::ptr::addr_of_mut!(KERN_HW) as *mut SystemHardware<'static>;
        core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<SystemHardware>());
        orchestrator::Orchestrator::init_in_place(
            core::ptr::addr_of_mut!((*p).orchestrator),
            crate::apps::APP_ID_LAUNCHER,
        );
        core::ptr::write(core::ptr::addr_of_mut!((*p).lcd), St7565::new(bsp::LcdBus));
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).backlight),
            drivers::backlight::BacklightController::new(bsp::BspBacklight),
        );
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).key_scanner),
            drivers::keyboard::KeypadScanner::new(),
        );
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).audio),
            drivers::audio::AudioAmplifier::new(bsp::BspAudioPa),
        );
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).radio),
            Bk4829::new(bsp::RadioBus),
        );
        core::ptr::write(core::ptr::addr_of_mut!((*p).filesystem), filesystem);
        #[cfg(feature = "usb")]
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).usb),
            drivers::usb::UsbDriver::new(),
        );
        #[cfg(feature = "uart")]
        core::ptr::write(
            core::ptr::addr_of_mut!((*p).uart),
            drivers::uart::HardwareUart::new(bsp::Uart1Serial),
        );
        #[cfg(any(feature = "uart", feature = "usb_cdc"))]
        drivers::wire::WireDriver::init_in_place(
            core::ptr::addr_of_mut!((*p).wire),
            hardware::SystemSerialPort::new(),
            bsp::BspTime,
        );
        &mut *p
    };

    #[cfg(feature = "usb")]
    drivers::usb::register_active_usb_driver(&mut hw.usb as *mut _);

    let adc = boot::run(hw);
    handoff(hw);

    system::run_system_loop(hw, adc, boot_start_ms);
}
