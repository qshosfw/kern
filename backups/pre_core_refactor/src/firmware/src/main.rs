#![no_std]
#![no_main]

use crate::config::MasterConfig;
use crate::hardware::SystemHardware;
use crate::services::RadioState;
use core::cell::RefCell;
use cortex_m::interrupt::Mutex;
use cortex_m_rt::{entry, exception};
use drivers::battery::SystemPower;
use drivers::bk4829::Bk4829;
use drivers::st7565::St7565;
use heapless::Deque;
use kernel::event::Event;

use bsp::Board;
use py32_hal as hal;
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
pub use crate::services::FirmwareContext as SysContext;

/// Global event queue for zero-alloc event-driven kernel dispatch
pub static EVENT_QUEUE: Mutex<RefCell<Deque<Event, 16>>> = Mutex::new(RefCell::new(Deque::new()));

/// Global memory buffer allocator for zero-copy IPC
pub static MBUF_POOL: utils::mbuf::MBufPool = utils::mbuf::MBufPool::new();

#[unsafe(link_section = ".uninit")]
pub static mut MASTER_CONFIG: core::mem::MaybeUninit<MasterConfig> =
    core::mem::MaybeUninit::uninit();
pub static mut CALIBRATION: services::calibration::SystemCalibration =
    services::calibration::SystemCalibration::new();
pub static mut PASSCODE_STATE: crate::security::PasscodeState =
    crate::security::PasscodeState::empty();
#[unsafe(link_section = ".uninit")]
pub static mut RADIO_STATE: core::mem::MaybeUninit<RadioState> = core::mem::MaybeUninit::uninit();
pub static mut POWER_SERVICE: SystemPower = SystemPower::empty();
pub static mut CANVAS: gfx::Canvas = gfx::Canvas::new();

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

/// SysTick interrupt handler for system uptime tracking
#[exception]
fn SysTick() {
    hal::delay::on_systick();
    drivers::backlight::systick_pwm_tick();
}

#[entry]
fn main() -> ! {
    // High-Assurance Stack Painting for memory introspection & watermarking (§13.1)
    // Painted immediately upon entry before deep call frames are allocated.
    kernel::sys::SystemDiagnostics::paint_stack();

    unsafe {
        (*core::ptr::addr_of_mut!(MASTER_CONFIG)).write(MasterConfig::new());
        (*core::ptr::addr_of_mut!(RADIO_STATE)).write(RadioState::new());
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

    // 2. Initialize Display & Keypad (screen remains blanked until passcode or splash)
    let mut lcd = St7565::new(&mut board.spi_lcd, &mut board.lcd_cs, &mut board.lcd_a0);
    lcd.init();
    bsp::pins::BACKLIGHT.set_low();
    let canvas = system_canvas();
    canvas.clear();
    lcd.blit(canvas);

    let boot_start_ms = hal::delay::millis();

    let mut key_scanner = drivers::keyboard::KeypadScanner::new();
    let mut backlight = drivers::backlight::BacklightController::new();
    backlight.init();

    // 3. Storage & Drivers (Fast mount without heavy provisioning stalls)
    let mut radio = Bk4829::new(&mut board.radio_bus);
    let mut audio_service = drivers::audio::AudioAmplifier::new();

    #[cfg(feature = "filesystem")]
    let mut storage_service =
        drivers::py25q16::Py25q16FlashDriver::new(&mut board.spi_flash, &mut board.flash_cs);
    #[cfg(feature = "filesystem")]
    storage_service.init();

    #[cfg(feature = "filesystem")]
    let mut vfs =
        boot::mount_or_format(&mut lcd, &mut key_scanner, &mut storage_service, &mut radio);
    #[cfg(not(feature = "filesystem"))]
    let mut vfs = boot::mount_or_format(&mut lcd, &mut key_scanner, &mut radio);

    bsp::Board::wdt_feed();

    #[cfg(feature = "uart")]
    let uart_service = drivers::uart::HardwareUart::new();

    #[cfg(feature = "usb")]
    let mut usb_driver = drivers::usb::UsbDriver::new();

    // 4. Execute Instant Boot Pipeline (Passcode Prompt -> Splash -> Concurrent Hardware Init)
    let mut boot_hw = boot::BootHardware {
        lcd: &mut lcd,
        backlight: &mut backlight,
        key_scanner: &mut key_scanner,
        audio: &mut audio_service,
        radio: &mut radio,
        vfs: &mut vfs,
        #[cfg(feature = "usb")]
        usb: &mut usb_driver,
    };
    let adc = boot::boot_orchestration(&mut boot_hw);

    let startup_app = if master_config_ref().apps.startup_app == 1 {
        crate::apps::APP_ID_VFO
    } else {
        crate::apps::APP_ID_LAUNCHER
    };
    let orchestrator = orchestrator::Orchestrator::new(startup_app);
    let sys_ipc_queue = kernel::ipc::MessageQueue::<kernel::ipc::SysEvent, 8>::new();

    let mut hw = SystemHardware {
        orchestrator,
        lcd,
        backlight,
        key_scanner,
        audio: audio_service,
        radio,
        ipc_queue: sys_ipc_queue,
        vfs: &mut vfs,
        #[cfg(feature = "usb")]
        usb: usb_driver,
        #[cfg(feature = "uart")]
        uart: uart_service,
        #[cfg(feature = "uart")]
        wire: drivers::wire::WireDriver::new(hardware::SystemSerialPort::new()),
    };

    #[cfg(feature = "usb")]
    drivers::usb::register_active_usb_driver(&mut hw.usb as *mut _);

    #[cfg(feature = "uart")]
    bsp::Board::uart_set_baud(master_config_ref().system.baud_rate());

    #[cfg(feature = "filesystem")]
    crate::apps::launcher::catalog::get_catalog().scan(hw.vfs);

    hw.init_startup_app();

    // 7. Transition to System Event Loop
    system::run_system_loop(&mut hw, adc, boot_start_ms);
}
