//! System Hardware Abstraction, Dirty Config Synchronization & Context Building.

#[cfg(test)]
use crate::services::DisplayHardware;
use crate::services::{PowerServiceConfigExt, SysContext, SystemStorage};
use drivers::audio::AudioAmplifier;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::keyboard::KeypadScanner;
use drivers::st7565::St7565;
use kernel::event::Event;
#[cfg(feature = "usb")]
use kernel::service::PowerService;
#[cfg(feature = "radio")]
use kernel::service::radio::RfSquelch;

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

/// Global `SerialCore` port lease & write-fence router (`0 = UsbCdc`, `1 = Uart0`).
pub static mut SERIAL_ROUTER: kernel::service::SerialRouter<2> =
    kernel::service::SerialRouter::new();

#[inline(always)]
pub fn serial_router_mut() -> &'static mut kernel::service::SerialRouter<2> {
    unsafe { &mut *core::ptr::addr_of_mut!(SERIAL_ROUTER) }
}

impl kernel::service::SerialPort for SystemSerialPort {
    fn write(&mut self, _data: &[u8]) {
        #[cfg(feature = "usb_cdc")]
        if drivers::usb::is_active_usb_configured() && self.active_interface != 2 {
            let _ = drivers::usb::active_usb_write(_data);
            return;
        }

        #[cfg(feature = "uart")]
        bsp::Board::uart_write_bytes(_data);
    }

    fn read(&mut self, _buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if drivers::usb::is_active_usb_configured() || drivers::usb::is_active_usb_connected() {
                let count = drivers::usb::active_usb_read(_buf);
                if count > 0 {
                    self.active_interface = 1;
                    return count;
                }
            } else if self.active_interface == 1 {
                self.active_interface = 0;
            }
        }

        #[cfg(feature = "uart")]
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
    pub filesystem: &'a mut SystemStorage<'a>,
    #[cfg(feature = "uart")]
    pub uart: drivers::uart::HardwareUart<bsp::Uart1Serial>,
    pub key_scanner: KeypadScanner,
    pub ipc_queue: kernel::ipc::MessageQueue<kernel::ipc::SysEvent, 4>,
    #[cfg(feature = "usb")]
    pub usb: drivers::usb::UsbDriver,
    pub orchestrator: crate::orchestrator::Orchestrator,
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    pub wire: drivers::wire::WireDriver<SystemSerialPort, bsp::BspTime>,
    pub radio: Bk4829<'a, bsp::RadioBus>,
    pub lcd: St7565<bsp::LcdBus>,
}

#[inline(never)]
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

#[derive(Clone, Copy)]
enum CtxOp<'e> {
    Start,
    Render,
    Dispatch(&'e Event),
    #[cfg(feature = "krx_native")]
    ContainFault,
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
        usb: &mut drivers::usb::UsbDriver,
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

    #[inline(never)]
    fn exec_with_ctx(&mut self, op: CtxOp<'_>) -> bool {
        let is_locked = self.orchestrator.is_keypad_locked();
        let is_f = self.orchestrator.is_f_active();
        #[cfg(test)]
        let with_display = !matches!(op, CtxOp::Render);
        #[cfg(test)]
        let mut display_hw = DisplayHardware {
            lcd: &mut self.lcd,
            canvas: crate::system_canvas(),
            backlight: &mut self.backlight,
        };
        let mut ctx = SysContext {
            is_keypad_locked: is_locked,
            is_f_active: is_f,
            state: crate::radio_state(),
            radio: &mut self.radio,
            power: crate::power_service(),
            config: crate::master_config(),
            storage: self.filesystem,
            audio: &mut self.audio,
            ipc: &mut self.ipc_queue,
            #[cfg(test)]
            display: with_display.then_some(&mut display_hw),
            #[cfg(all(test, feature = "uart"))]
            uart: &mut self.uart,
            #[cfg(all(test, feature = "usb"))]
            usb: Some(&mut self.usb),
            #[cfg(all(test, not(feature = "usb")))]
            usb: None,
        };
        match op {
            CtxOp::Start => {
                crate::AppRegistry::on_start(self.orchestrator.active_app_id(), &mut ctx);
                false
            }
            CtxOp::Render => {
                self.orchestrator.render(crate::system_canvas(), &ctx);
                false
            }
            CtxOp::Dispatch(ev) => self.orchestrator.dispatch_event(&mut ctx, *ev),
            #[cfg(feature = "krx_native")]
            CtxOp::ContainFault => {
                crate::runtime::fault::finish_fault_cleanup(&mut ctx);
                false
            }
        }
    }

    #[cfg(feature = "krx_native")]
    pub fn contain_app_fault(&mut self) {
        self.exec_with_ctx(CtxOp::ContainFault);
    }

    pub fn dispatch_event_and_actions(&mut self, ev: &Event) -> bool {
        let consumed = self.exec_with_ctx(CtxOp::Dispatch(ev));
        self.apply_dirty_configs();
        consumed
    }

    #[inline(never)]
    pub fn tick_radio_services(&mut self) {
        #[cfg(feature = "radio")]
        {
            if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(0xF1) {
                pcb.ledger.record_activation();
                let is_busy = crate::radio_state().coord.is_transmitting
                    || crate::radio_state().coord.carrier_active
                    || self.radio.is_carrier_open();
                pcb.set_state(if is_busy {
                    kernel::proc::ProcState::Running
                } else {
                    kernel::proc::ProcState::ServiceIdle
                });
            }
            let state = crate::radio_state();
            if crate::PVD_TX_QUENCHED.load(core::sync::atomic::Ordering::Acquire) {
                crate::PVD_TX_QUENCHED.store(false, core::sync::atomic::Ordering::Release);
                if state.coord.is_transmitting {
                    state.coord.stop_tx(&mut self.radio);
                }
            }
            let radio_ev = state.coord.tick_10ms(&mut self.radio);
            state.rssi_dbm = state.coord.vfo_rssi(state.coord.listening_vfo);
            if let Some(bus_ev) = radio_ev.to_bus_event(2) {
                crate::system::enqueue_event(kernel::event::Event::Bus(bus_ev));
            }
        }
    }

    pub fn init_startup_app(&mut self) {
        self.exec_with_ctx(CtxOp::Start);
    }

    pub fn render_frame(&mut self) {
        self.exec_with_ctx(CtxOp::Render);
        if crate::system::is_headless() {
            return;
        }
        let canvas = crate::system_canvas();
        canvas.mark_all_dirty();
        self.lcd.blit(canvas);
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
