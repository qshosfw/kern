//! System Hardware Abstraction & Dirty Config Synchronization.

use crate::services::PowerServiceConfigExt;
use drivers::audio::AudioAmplifier;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::keyboard::KeypadScanner;
use drivers::st7565::St7565;
use kernel::event::Event;
#[cfg(feature = "usb")]
use kernel::service::PowerService;

#[derive(Clone, Copy, Default)]
pub struct SystemSerialPort {
    pub active_interface: u8, // 0 = UART, 1 = USB CDC
}

impl SystemSerialPort {
    pub const fn new() -> Self {
        Self {
            active_interface: 0,
        }
    }
}

/// Global `SerialCore` port lease & write-fence router
/// (`0 = UsbCdc`, `1 = Uart0`, and `2 = Wireless` BLE on `target_k10pro`).
#[cfg(not(feature = "target_k10pro"))]
pub static mut SERIAL_ROUTER: kernel::service::SerialRouter<2> =
    kernel::service::SerialRouter::new();
#[cfg(feature = "target_k10pro")]
pub static mut SERIAL_ROUTER: kernel::service::SerialRouter<3> =
    kernel::service::SerialRouter::new();

#[cfg(feature = "usb")]
static mut ACTIVE_USB_DRIVER: *mut drivers::usb::UsbDriver<bsp::BspUsbBus> = core::ptr::null_mut();

#[cfg(feature = "usb")]
pub fn register_active_usb_driver(driver: *mut drivers::usb::UsbDriver<bsp::BspUsbBus>) {
    unsafe {
        ACTIVE_USB_DRIVER = driver;
    }
}

#[cfg(feature = "usb")]
#[inline(always)]
pub fn on_usb_irq() {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if !ptr.is_null() {
        unsafe {
            (*ptr).handle_irq();
        }
    }
}

#[cfg(feature = "usb")]
fn active_usb_driver() -> Option<&'static drivers::usb::UsbDriver<bsp::BspUsbBus>> {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

#[cfg(feature = "usb")]
pub fn is_active_usb_connected() -> bool {
    active_usb_driver().is_some_and(|d| d.is_connected())
}

#[cfg(feature = "usb")]
pub fn is_active_usb_configured() -> bool {
    active_usb_driver().is_some_and(|d| d.is_configured())
}

#[cfg(feature = "usb")]
pub fn active_usb_read(buf: &mut [u8]) -> usize {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if !ptr.is_null() {
        unsafe { (*ptr).cdc_read(buf) }
    } else {
        0
    }
}

#[cfg(feature = "usb")]
pub fn active_usb_write(data: &[u8]) -> usize {
    let ptr = unsafe { ACTIVE_USB_DRIVER };
    if !ptr.is_null() {
        unsafe { (*ptr).cdc_write(data) }
    } else {
        0
    }
}

#[cfg(not(feature = "target_k10pro"))]
#[inline(always)]
pub fn serial_router_mut() -> &'static mut kernel::service::SerialRouter<2> {
    unsafe { &mut *core::ptr::addr_of_mut!(SERIAL_ROUTER) }
}

#[cfg(feature = "target_k10pro")]
#[inline(always)]
pub fn serial_router_mut() -> &'static mut kernel::service::SerialRouter<3> {
    unsafe { &mut *core::ptr::addr_of_mut!(SERIAL_ROUTER) }
}

impl kernel::service::SerialPort for SystemSerialPort {
    fn write(&mut self, _data: &[u8]) {
        #[cfg(feature = "usb_cdc")]
        if self.active_interface == 1 || (self.active_interface == 0 && is_active_usb_configured())
        {
            if is_active_usb_configured() {
                let _ = active_usb_write(_data);
                return;
            }
        }

        #[cfg(feature = "uart")]
        if serial_router_mut().ports[kernel::service::PortId::Uart0 as usize].mode
            == kernel::service::PortMode::RigWire
        {
            bsp::Board::uart_write_bytes(_data);
        }
    }

    fn read(&mut self, _buf: &mut [u8]) -> usize {
        let router = serial_router_mut();

        #[cfg(feature = "usb_cdc")]
        if router.ports[kernel::service::PortId::UsbCdc as usize].mode
            == kernel::service::PortMode::RigWire
        {
            if is_active_usb_configured() || is_active_usb_connected() {
                let count = active_usb_read(_buf);
                if count > 0 {
                    self.active_interface = 1;
                    return count;
                }
            } else if self.active_interface == 1 {
                self.active_interface = 0;
            }
        }

        #[cfg(feature = "uart")]
        if router.ports[kernel::service::PortId::Uart0 as usize].mode
            == kernel::service::PortMode::RigWire
        {
            let mut count = 0;
            for byte in _buf.iter_mut() {
                if let Some(b) = bsp::Board::uart_try_read_byte() {
                    *byte = b;
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                self.active_interface = 2;
                return count;
            }
        }

        0
    }

    fn flush(&mut self) {
        #[cfg(feature = "uart")]
        py32_hal::uart::wait_tx_complete();
    }
}

#[repr(C)]
pub struct SystemHardware<'a> {
    pub backlight: BacklightController<bsp::BspBacklight>,
    pub audio: AudioAmplifier<bsp::BspAudioPa>,
    pub filesystem: &'a mut kernfs::KernFS<'a, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    pub key_scanner: KeypadScanner<bsp::BspKeyMatrix>,
    #[cfg(feature = "usb")]
    pub usb: drivers::usb::UsbDriver<bsp::BspUsbBus>,
    pub orchestrator: crate::orchestrator::Orchestrator,
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    pub wire: kernel::wire::WireEngine<SystemSerialPort>,
    pub radio: Bk4829<'a, bsp::RadioBus>,
    pub lcd: St7565<bsp::LcdBus>,
}

pub fn apply_audio_hardware(
    audio_cfg: &crate::config::AudioConfig,
    audio: &mut AudioAmplifier<bsp::BspAudioPa>,
    radio: &mut Bk4829<'_, bsp::RadioBus>,
) {
    audio.set_volume(audio_cfg.volume);
    radio.set_volume_level(audio_cfg.volume);
    radio.set_tone_gain((audio_cfg.beep_gain as u16 * 12).min(127) as u8);
    radio.set_mic_gain(audio_cfg.mic_gain);
    radio.set_audio_profile(audio_cfg.audio_profile);
}

#[inline(always)]
pub fn is_torch_on() -> bool {
    #[cfg(target_arch = "arm")]
    {
        bsp::pins::FLASHLIGHT.is_high()
    }
    #[cfg(not(target_arch = "arm"))]
    {
        false
    }
}

#[inline(always)]
pub fn apply_lcd_hardware(disp_cfg: &crate::config::DisplayConfig, lcd: &mut St7565<bsp::LcdBus>) {
    lcd.set_contrast(disp_cfg.contrast);
    lcd.set_inverted(disp_cfg.inverted != 0);
    lcd.shading_mode = disp_cfg.shading_mode;
}

pub fn apply_display_hardware(
    disp_cfg: &crate::config::DisplayConfig,
    lcd: &mut St7565<bsp::LcdBus>,
    backlight: &mut BacklightController<bsp::BspBacklight>,
) {
    apply_lcd_hardware(disp_cfg, lcd);
    disp_cfg.apply_to_backlight(backlight);
}

impl<'a> SystemHardware<'a> {
    #[inline(never)]
    pub fn apply_dirty_configs(&mut self) {
        let config = crate::master_config();
        if config.hw_dirty == 0 {
            return;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_KEYS) != 0 {
            self.key_scanner.configure(config.keys.hold_delay_ms());
            config.hw_dirty &= !crate::config::HW_DIRTY_KEYS;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_AUDIO) != 0 {
            apply_audio_hardware(&config.audio, &mut self.audio, &mut self.radio);
            config.hw_dirty &= !crate::config::HW_DIRTY_AUDIO;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_DISPLAY) != 0 {
            apply_display_hardware(&config.display, &mut self.lcd, &mut self.backlight);
            config.hw_dirty &= !crate::config::HW_DIRTY_DISPLAY;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_BATTERY) != 0 {
            crate::power_service().load_battery_config(&config.battery);
            config.hw_dirty &= !crate::config::HW_DIRTY_BATTERY;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_SYSTEM) != 0 {
            let clk_idx = if cfg!(feature = "overclock_72mhz") {
                config.system.cpu_clock
            } else {
                config.system.cpu_clock.min(3)
            };
            #[cfg(feature = "usb")]
            if self.usb.is_connected() {
                py32_hal::rcc::set_sysclk_48mhz();
            } else {
                py32_hal::rcc::set_sysclk_index(clk_idx);
            }
            #[cfg(not(feature = "usb"))]
            py32_hal::rcc::set_sysclk_index(clk_idx);

            bsp::Board::uart_set_baud(config.system.baud_rate());
            #[cfg(all(not(test), feature = "filesystem"))]
            crate::services::lang::reload_os(crate::kern_fs(), &config.system);
            #[cfg(feature = "usb")]
            self.usb
                .update_device_identity(config.system.model_str(), config.system.tag_str());
            config.hw_dirty &= !crate::config::HW_DIRTY_SYSTEM;
        }
    }

    #[cfg(feature = "usb")]
    pub fn sync_usb_telemetry(
        usb: &mut drivers::usb::UsbDriver<bsp::BspUsbBus>,
        cfg: &crate::config::MasterConfig,
    ) {
        usb.update_device_identity(cfg.system.model_str(), cfg.system.tag_str());
        let pwr = crate::power_service();
        usb.update_power_telemetry(
            pwr.battery_voltage_mv(),
            pwr.battery_percentage(),
            pwr.is_charging,
        );
    }

    #[cfg(feature = "krx_native")]
    pub fn contain_app_fault(&mut self) {
        crate::runtime::fault::finish_fault_cleanup();
    }

    #[inline(never)]
    pub fn dispatch_event_and_actions(&mut self, ev: &Event) -> bool {
        let consumed = self.orchestrator.dispatch_event(*ev);
        self.apply_dirty_configs();
        consumed
    }

    #[inline(never)]
    pub fn tick_radio_services(&mut self) {
        #[cfg(feature = "radio")]
        {
            let state = crate::radio_state();
            if crate::PVD_TX_QUENCHED.load(core::sync::atomic::Ordering::Acquire) {
                crate::PVD_TX_QUENCHED.store(false, core::sync::atomic::Ordering::Release);
                if state.coord.is_transmitting {
                    state.coord.stop_tx(&mut self.radio);
                }
            }
            let radio_ev = state.coord.tick_10ms(&mut self.radio);
            state.rssi_dbm = state.coord.vfo_rssi(state.coord.listening_vfo);
            let mut view = crate::services::radio::RADIO_VIEW.read();
            let vfo_a = state
                .coord
                .vfo_channel(kernel::service::radio::VfoSlot::VfoA);
            let vfo_b = state
                .coord
                .vfo_channel(kernel::service::radio::VfoSlot::VfoB);
            let act = state.coord.vfo_channel(state.coord.active_vfo);
            view.vfo_a_hz = vfo_a.effective_rx_freq().raw();
            view.vfo_b_hz = vfo_b.effective_rx_freq().raw();
            view.active_freq_hz = act.effective_rx_freq().raw();
            view.rssi_dbm = state.rssi_dbm;
            view.active_vfo = state.coord.active_vfo as u8;
            view.listening_vfo = state.coord.listening_vfo as u8;
            view.squelch_level = act.squelch_level;
            view.tx_power = act.power_level() as u8;
            view.flags = if state.coord.is_transmitting {
                kernel::service::radio::RADIO_FLAG_TX
            } else {
                0
            } | if state.coord.is_slot_rx_active(state.coord.listening_vfo) {
                kernel::service::radio::RADIO_FLAG_RX_OPEN
            } else {
                0
            };
            crate::services::radio::RADIO_VIEW.write(view);
            if let Some(bus_ev) = radio_ev.to_bus_event(2) {
                crate::system::enqueue_event(kernel::event::Event::Bus(bus_ev));
            }
        }
    }

    pub fn init_startup_app(&mut self) {
        crate::AppRegistry::on_start(self.orchestrator.active_app_id());
    }

    #[inline(never)]
    pub fn render_frame(&mut self) {
        self.orchestrator.render(crate::system_canvas());
        if crate::system::is_headless() {
            return;
        }
        self.lcd.blit(crate::system_canvas());
    }

    #[inline(always)]
    pub fn refresh_subframe(&mut self) {
        #[cfg(feature = "grayscale")]
        {
            if crate::system::is_headless() || self.lcd.shading_mode != 0 {
                return;
            }

            let canvas = crate::system_canvas();
            canvas.mark_all_dirty();
            self.lcd.blit(canvas);
        }
    }
}
