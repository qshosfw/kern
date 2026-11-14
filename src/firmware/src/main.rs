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
pub static EVENT_QUEUE: Mutex<RefCell<Deque<Event, 16>>> = Mutex::new(RefCell::new(Deque::new()));

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
pub static mut AUDIO_CAPTURE: drivers::audio::AudioCaptureDriver =
    drivers::audio::AudioCaptureDriver::new();

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
pub fn audio_capture_mut() -> &'static mut drivers::audio::AudioCaptureDriver {
    unsafe { &mut *core::ptr::addr_of_mut!(AUDIO_CAPTURE) }
}

/// SysTick interrupt handler for system uptime tracking
#[exception]
fn SysTick() {
    hal::delay::on_systick();
    drivers::backlight::systick_pwm_tick();
}

#[cfg(not(test))]
#[entry]
fn main() -> ! {
    // High-Assurance Stack Painting for memory introspection & watermarking (§13.1)
    // Painted immediately upon entry before deep call frames are allocated.
    kernel::sys::SystemDiagnostics::paint_stack();

    unsafe {
        MasterConfig::init_in_place(core::ptr::addr_of_mut!(MASTER_CONFIG) as *mut MasterConfig);
        CONFIG_INITIALIZED.store(true, core::sync::atomic::Ordering::Release);
        RadioState::init_in_place(core::ptr::addr_of_mut!(RADIO_STATE) as *mut RadioState);
        (*core::ptr::addr_of_mut!(CALIBRATION)).init_defaults();
    }

    // 0. CRITICAL: Set Vector Table Offset Register (VTOR) to 0x0800_2800!
    unsafe {
        pac::SCB_VTOR.write_volatile(0x0800_2800);
        cortex_m::interrupt::enable();
    }

    // 1. Initialize Board (Clocks, GPIOs, SysTick, TIM2, SPI1, USART1)
    let mut board = Board::init();
    boot::check_reset_cause();

    // Ensure Display Backlight is OFF during bring-up
    bsp::pins::BACKLIGHT.set_low();

    // Start Independent Watchdog with generous 6000ms timeout for reliable boot & flash operations
    hal::wdt::Watchdog::start(6000);
    let _ = hal::uart::write_line("KERN v0.1.0");
    let _ = hal::uart::write_line(build_info::BUILD_STAMP);

    let boot_start_ms = hal::delay::millis();

    #[cfg(feature = "filesystem")]
    let mut storage_service =
        drivers::py25q16::Py25q16FlashDriver::new(&mut board.spi_flash, &mut board.flash_cs);
    #[cfg(feature = "filesystem")]
    storage_service.init();

    #[cfg(feature = "filesystem")]
    let mut filesystem =
        drivers::filesystem::FileSystemDriver::new(kernfs::KernFS::new(&mut storage_service));
    #[cfg(not(feature = "filesystem"))]
    let mut filesystem = crate::services::storage::NoStorage;

    let mut hw = SystemHardware {
        orchestrator: orchestrator::Orchestrator::new(crate::apps::APP_ID_LAUNCHER),
        lcd: St7565::new(&mut board.spi_lcd, &mut board.lcd_cs, &mut board.lcd_a0),
        backlight: drivers::backlight::BacklightController::new(),
        key_scanner: drivers::keyboard::KeypadScanner::new(),
        audio: drivers::audio::AudioAmplifier::new(),
        radio: Bk4829::new(&mut board.radio_bus),
        ipc_queue: kernel::ipc::MessageQueue::<kernel::ipc::SysEvent, 8>::new(),
        filesystem: &mut filesystem,
        #[cfg(feature = "usb")]
        usb: drivers::usb::UsbDriver::new(),
        #[cfg(feature = "uart")]
        uart: drivers::uart::HardwareUart::new(),
        #[cfg(any(feature = "uart", feature = "usb_cdc"))]
        wire: drivers::wire::WireDriver::new(hardware::SystemSerialPort::new()),
    };

    #[cfg(feature = "usb")]
    drivers::usb::register_active_usb_driver(&mut hw.usb as *mut _);

    // 2. Initialize Display & Keypad (screen remains blanked until passcode or splash)
    hw.lcd.init();
    bsp::pins::BACKLIGHT.set_low();
    let canvas = system_canvas();
    canvas.clear();
    hw.lcd.blit(canvas);
    hw.backlight.init();

    // 3. Storage & Drivers (Fast mount without heavy provisioning stalls)
    #[cfg(feature = "filesystem")]
    boot::mount_or_format(
        &mut hw.lcd,
        &mut hw.key_scanner,
        hw.filesystem,
        &mut hw.radio,
    );

    bsp::Board::wdt_feed();

    // 4. Execute Instant Boot Pipeline (Passcode Prompt -> Splash -> Concurrent Hardware Init)
    let adc = {
        let mut boot_hw = boot::BootHardware {
            lcd: &mut hw.lcd,
            backlight: &mut hw.backlight,
            key_scanner: &mut hw.key_scanner,
            audio: &mut hw.audio,
            radio: &mut hw.radio,
            filesystem: hw.filesystem,
            #[cfg(feature = "usb")]
            usb: &mut hw.usb,
        };
        boot::boot_orchestration(&mut boot_hw)
    };

    let calib_ref = unsafe { (*core::ptr::addr_of!(CALIBRATION)).raw_bytes() };
    hw.radio.attach_calibration(calib_ref);
    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
    power_service
        .battery_driver
        .load_calibration_curve(unsafe { (*core::ptr::addr_of!(CALIBRATION)).battery_curve() });

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

    // 7. Transition to System Event Loop
    system::run_system_loop(&mut hw, adc, boot_start_ms);
}
