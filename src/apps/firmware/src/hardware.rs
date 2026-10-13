//! System Hardware Abstraction, Dirty Config Synchronization & Context Building.

use crate::services::{
    DisplayHardware, FirmwareContext as SysContext, PowerServiceConfigExt, RadioMelodyExt,
    RadioService, SystemStorage,
};
use crate::{CALIBRATION, MASTER_CONFIG, PASSCODE_STATE, POWER_SERVICE, RADIO_STATE};
use drivers::audio::AudioAmplifier;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::keyboard::KeypadScanner;
use drivers::st7565::St7565;
use kernel::event::{Event, KeyCode, KeyState};

pub struct SystemHardware<'a> {
    pub orchestrator: crate::orchestrator::Orchestrator,
    pub lcd: St7565<'a>,
    pub backlight: BacklightController,
    pub key_scanner: KeypadScanner,
    pub audio: AudioAmplifier,
    pub radio: Bk4829<'a, drivers::bk4829::Disabled>,
    pub ipc_queue: kernel::ipc::MessageQueue<kernel::ipc::SysEvent, 8>,
    pub vfs: &'a mut SystemStorage<'a>,
    #[cfg(feature = "usb")]
    pub usb: drivers::usb::UsbDriver,
    #[cfg(feature = "uart")]
    pub uart: drivers::uart::HardwareUart,
}

pub fn apply_audio_hardware(
    audio_cfg: &crate::config::AudioConfig,
    audio: &mut AudioAmplifier,
    radio: &mut Bk4829<drivers::bk4829::Disabled>,
) {
    audio.set_volume(audio_cfg.volume);
    radio.set_volume(audio_cfg.volume);
    radio.set_tone_gain((audio_cfg.beep_gain as u16 * 12).min(127) as u8);
    radio.set_mic_gain(audio_cfg.mic_gain);
    radio.set_audio_profile(audio_cfg.audio_profile);
}

pub fn apply_display_hardware(
    disp_cfg: &crate::config::DisplayConfig,
    lcd: &mut St7565,
    backlight: &mut BacklightController,
) {
    lcd.set_contrast(disp_cfg.contrast);
    lcd.set_inverted(disp_cfg.inverted != 0);
    disp_cfg.apply_to_backlight(backlight);
}

#[inline(never)]
pub fn build_sys_context<'a, 'hw>(
    radio: &'a mut crate::services::SystemRadio<'hw>,
    vfs: &'a mut crate::services::SystemStorage<'hw>,
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
        state: unsafe { &mut *core::ptr::addr_of_mut!(RADIO_STATE) },
        radio,
        power: unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) },
        audio: Some(audio),
        display,
        ipc,
        config: unsafe { &mut *core::ptr::addr_of_mut!(MASTER_CONFIG) },
        is_keypad_locked: is_locked,
        is_f_active: is_f,
        #[cfg(feature = "usb")]
        usb,
        #[cfg(not(feature = "usb"))]
        usb: None,
        storage: vfs,
        calibration: unsafe { &*core::ptr::addr_of!(CALIBRATION) },
        passcode: unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) },
        #[cfg(feature = "uart")]
        uart,
    }
}

impl<'a> SystemHardware<'a> {
    pub fn apply_dirty_configs(&mut self) {
        let config = unsafe { &mut *core::ptr::addr_of_mut!(MASTER_CONFIG) };
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
            if config.battery.cpu_clock == 1 {
                py32_hal::rcc::set_sysclk_72mhz();
            } else {
                py32_hal::rcc::set_sysclk_48mhz();
            }
            config.hw_dirty &= !crate::config::HW_DIRTY_BATTERY;
        }
        #[cfg(feature = "usb")]
        if (config.hw_dirty & crate::config::HW_DIRTY_SYSTEM) != 0 {
            self.usb
                .update_device_identity(config.system.model_str(), config.system.tag_str());
            config.hw_dirty &= !crate::config::HW_DIRTY_SYSTEM;
        }
    }

    #[inline(never)]
    pub fn dispatch_event_and_actions(&mut self, ev: Event) {
        let key_beep = unsafe { (*core::ptr::addr_of!(MASTER_CONFIG)).audio.key_beep };
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

        {
            let mut display_hw = DisplayHardware {
                lcd: &mut self.lcd,
                canvas: crate::system_canvas(),
                backlight: &mut self.backlight,
            };
            let mut ctx = build_sys_context(
                &mut self.radio,
                self.vfs,
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
            self.orchestrator.dispatch_event(&mut ctx, ev);
        }

        if key_beep != 0 {
            match ev {
                Event::Charging(true) => {
                    self.radio
                        .play_dialog_sound(crate::services::melody::DialogSoundType::Charging);
                }
                #[cfg(feature = "usb")]
                Event::Usb(
                    kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed,
                ) => {
                    self.radio
                        .play_dialog_sound(crate::services::melody::DialogSoundType::UsbConnect);
                }
                #[cfg(feature = "usb")]
                Event::Usb(kernel::event::UsbEvent::Disconnected) => {
                    self.radio
                        .play_dialog_sound(crate::services::melody::DialogSoundType::UsbDisconnect);
                }
                _ => {}
            }
        }

        self.apply_dirty_configs();
    }

    #[inline(never)]
    pub fn render_frame(&mut self) {
        let is_locked = self.orchestrator.is_keypad_locked();
        let is_f = self.orchestrator.is_f_active();
        let ctx = build_sys_context(
            &mut self.radio,
            self.vfs,
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
}
