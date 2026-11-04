//! System Hardware Abstraction, Dirty Config Synchronization & Context Building.

use crate::services::{
    DisplayHardware, FirmwareContext as SysContext, PowerService, PowerServiceConfigExt,
    RadioMelodyExt, SystemStorage,
};
use crate::{CALIBRATION, PASSCODE_STATE, POWER_SERVICE};
use drivers::audio::AudioAmplifier;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::keyboard::KeypadScanner;
use drivers::st7565::St7565;
use kernel::event::{Event, KeyCode, KeyState};

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

impl kernel::service::SerialPort for SystemSerialPort {
    fn write(&mut self, data: &[u8]) {
        #[cfg(feature = "usb_cdc")]
        if drivers::usb::is_active_usb_configured()
            && (self.active_interface == 1 || drivers::usb::is_active_usb_dtr())
        {
            let _ = drivers::usb::active_usb_write(data);
            return;
        }

        #[cfg(feature = "uart")]
        bsp::Board::uart_write_bytes(data);
    }

    fn read(&mut self, buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb_cdc")]
        {
            if drivers::usb::is_active_usb_connected() {
                let count = drivers::usb::active_usb_read(buf);
                if count > 0 {
                    self.active_interface = 1;
                    return count;
                }
            }
        }

        #[cfg(feature = "uart")]
        {
            let mut count = 0;
            for byte in buf.iter_mut() {
                if let Some(b) = bsp::Board::uart_try_read_byte() {
                    *byte = b;
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 {
                self.active_interface = 0;
                return count;
            }
        }

        0
    }
}

pub struct SystemHardware<'a> {
    pub orchestrator: crate::orchestrator::Orchestrator,
    pub lcd: St7565<'a>,
    pub backlight: BacklightController,
    pub key_scanner: KeypadScanner,
    pub audio: AudioAmplifier,
    pub radio: Bk4829<'a, drivers::bk4829::Disabled>,
    pub ipc_queue: kernel::ipc::MessageQueue<kernel::ipc::SysEvent, 8>,
    pub filesystem: &'a mut SystemStorage<'a>,
    #[cfg(feature = "usb")]
    pub usb: drivers::usb::UsbDriver,
    #[cfg(feature = "uart")]
    pub uart: drivers::uart::HardwareUart,
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    pub wire: drivers::wire::WireDriver<SystemSerialPort>,
}

pub fn apply_audio_hardware(
    audio_cfg: &crate::config::AudioConfig,
    audio: &mut AudioAmplifier,
    radio: &mut Bk4829<drivers::bk4829::Disabled>,
) {
    audio.set_volume(audio_cfg.volume);
    radio.set_volume_level(audio_cfg.volume);
    radio.set_tone_gain((audio_cfg.beep_gain as u16 * 12).min(127) as u8);
    radio.set_mic_gain(audio_cfg.mic_gain);
}

#[inline(always)]
pub fn apply_lcd_hardware(disp_cfg: &crate::config::DisplayConfig, lcd: &mut St7565) {
    lcd.set_contrast(disp_cfg.contrast);
    lcd.set_inverted(disp_cfg.inverted != 0);
    lcd.shading_mode = disp_cfg.shading_mode;
}

pub fn apply_display_hardware(
    disp_cfg: &crate::config::DisplayConfig,
    lcd: &mut St7565,
    backlight: &mut BacklightController,
) {
    apply_lcd_hardware(disp_cfg, lcd);
    disp_cfg.apply_to_backlight(backlight);
}

#[inline(never)]
pub fn build_sys_context<'a, 'hw>(
    radio: &'a mut crate::services::SystemRadio<'hw>,
    filesystem: &'a mut crate::services::SystemStorage<'hw>,
    audio: &'a mut crate::services::SystemAudio,
    #[cfg(feature = "usb")] usb: Option<&'a mut crate::services::SystemUsb>,
    #[cfg(not(feature = "usb"))] _usb: Option<&'a mut ()>,
    #[cfg(feature = "uart")] uart: &'a mut crate::services::SystemUart,
    display: Option<&'a mut DisplayHardware<'a, 'hw>>,
    ipc: Option<&'a mut kernel::ipc::MessageQueue<kernel::ipc::SysEvent, 8>>,
    is_locked: bool,
    is_f: bool,
) -> SysContext<'a, 'hw> {
    SysContext {
        state: crate::radio_state(),
        radio,
        power: unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) },
        audio: Some(audio),
        display,
        ipc,
        config: crate::master_config(),
        is_keypad_locked: is_locked,
        is_f_active: is_f,
        #[cfg(feature = "usb")]
        usb,
        #[cfg(not(feature = "usb"))]
        usb: None,
        storage: filesystem,
        calibration: unsafe { &*core::ptr::addr_of!(CALIBRATION) },
        passcode: unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) },
        #[cfg(feature = "uart")]
        uart,
    }
}

impl<'a> SystemHardware<'a> {
    #[inline(never)]
    pub fn apply_dirty_configs(&mut self) {
        let config = crate::master_config();
        if config.hw_dirty == 0 {
            return;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_KEYS) != 0 {
            self.key_scanner.configure(
                config.keys.hold_delay_ms(),
                config.keys.repeat_rate_ms(),
                config.keys.fast_scroll != 0,
            );
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
            let power = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
            power.load_battery_config(&config.battery);
            config.hw_dirty &= !crate::config::HW_DIRTY_BATTERY;
        }
        if (config.hw_dirty & crate::config::HW_DIRTY_SYSTEM) != 0 {
            if config.system.cpu_clock == 1 {
                py32_hal::rcc::set_sysclk_72mhz();
            } else {
                py32_hal::rcc::set_sysclk_48mhz();
            }
            bsp::Board::uart_set_baud(config.system.baud_rate());
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
        let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
        usb.update_power_telemetry(
            pwr.battery_voltage_mv(),
            pwr.battery_percentage(),
            pwr.is_charging(),
        );
    }

    #[inline(never)]
    pub fn dispatch_event_and_actions(&mut self, ev: Event) -> bool {
        let key_beep = crate::master_config_ref().audio.key_beep;
        let key_press = if let Event::Key { code, state } = ev {
            if state == KeyState::Pressed
                && code != KeyCode::Ptt
                && code != KeyCode::KeyF
                && key_beep != 0
            {
                Some(code)
            } else {
                None
            }
        } else {
            None
        };

        let is_locked = self.orchestrator.is_keypad_locked();
        let is_f = self.orchestrator.is_f_active();

        // 1. Play immediate key feedback beep before UI transitions if key is valid and keypad unlocked
        if key_beep != 0 && key_press.is_some() && !is_locked {
            self.radio
                .play_beep(crate::services::melody::BeepType::from_u8(key_beep));
        }

        let consumed = {
            let mut display_hw = DisplayHardware {
                lcd: &mut self.lcd,
                canvas: crate::system_canvas(),
                backlight: &mut self.backlight,
            };
            let mut ctx = build_sys_context(
                &mut self.radio,
                self.filesystem,
                &mut self.audio,
                #[cfg(feature = "usb")]
                Some(&mut self.usb),
                #[cfg(not(feature = "usb"))]
                None,
                #[cfg(feature = "uart")]
                &mut self.uart,
                Some(&mut display_hw),
                Some(&mut self.ipc_queue),
                is_locked,
                is_f,
            );
            self.orchestrator.dispatch_event(&mut ctx, ev)
        };

        let cfg = crate::master_config_ref();
        let audio_enabled = cfg.audio.volume > 0;
        if audio_enabled {
            let sound = match ev {
                Event::Charging(true) => Some(crate::services::melody::DialogSoundType::Charging),
                #[cfg(feature = "usb")]
                Event::Usb(
                    kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed,
                ) => Some(crate::services::melody::DialogSoundType::UsbConnect),
                #[cfg(feature = "usb")]
                Event::Usb(
                    kernel::event::UsbEvent::Disconnected | kernel::event::UsbEvent::Suspended,
                ) => Some(crate::services::melody::DialogSoundType::UsbDisconnect),
                _ => None,
            };
            if let Some(snd) = sound {
                self.radio.play_dialog_sound(snd);
            }
        }

        self.apply_dirty_configs();
        consumed
    }

    #[inline(never)]
    pub fn init_startup_app(&mut self) {
        let is_locked = self.orchestrator.is_keypad_locked();
        let is_f = self.orchestrator.is_f_active();
        let mut display_hw = DisplayHardware {
            lcd: &mut self.lcd,
            canvas: crate::system_canvas(),
            backlight: &mut self.backlight,
        };
        let mut ctx = build_sys_context(
            &mut self.radio,
            self.filesystem,
            &mut self.audio,
            #[cfg(feature = "usb")]
            Some(&mut self.usb),
            #[cfg(not(feature = "usb"))]
            None,
            #[cfg(feature = "uart")]
            &mut self.uart,
            Some(&mut display_hw),
            Some(&mut self.ipc_queue),
            is_locked,
            is_f,
        );
        crate::AppRegistry::on_start(self.orchestrator.active_app_id(), &mut ctx);
    }

    #[inline(never)]
    pub fn render_frame(&mut self) {
        let is_locked = self.orchestrator.is_keypad_locked();
        let is_f = self.orchestrator.is_f_active();
        let ctx = build_sys_context(
            &mut self.radio,
            self.filesystem,
            &mut self.audio,
            #[cfg(feature = "usb")]
            Some(&mut self.usb),
            #[cfg(not(feature = "usb"))]
            None,
            #[cfg(feature = "uart")]
            &mut self.uart,
            None,
            None,
            is_locked,
            is_f,
        );
        let canvas = crate::system_canvas();
        self.orchestrator.render(canvas, &ctx);
        canvas.mark_all_dirty();
        self.lcd.blit(canvas);
    }

    #[inline(always)]
    pub fn refresh_subframe(&mut self) {
        #[cfg(feature = "grayscale")]
        {
            if self.lcd.shading_mode != 0 {
                return;
            }

            let canvas = crate::system_canvas();
            canvas.mark_all_dirty();
            self.lcd.blit(canvas);
        }
    }
}
