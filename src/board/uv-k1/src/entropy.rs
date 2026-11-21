//! Hardware Entropy Collection Pipeline for KERN TRNG Seeding.
//!
//! Gathers true physical entropy across multiple hardware domains:
//! 1. SysTick Micro-Jitter: High-frequency 24-bit countdown register (`pac::SYST_CVR`)
//!    capturing CPU execution timing variations and bus arbitration jitter at 48/72 MHz.
//! 2. TIM2 Hardware Timer: 32-bit 1 MHz microsecond counter (`pac::tim::TIM2_CNT`).
//! 3. ADC Thermal Noise: Low-order bits of internal temperature sensor (Channel 16)
//!    and bandgap reference (Channel 17) displaying Johnson-Nyquist thermal fluctuations.
//! 4. 256-byte Silicon Fingerprint: Hardware lot, wafer ID, and silicon coordinates.

use core::sync::atomic::{AtomicU32, Ordering};
use py32_hal as hal;
use py32_pac as pac;

static ENTROPY_ROT: AtomicU32 = AtomicU32::new(0);

/// Samples physical hardware noise and mixes it into a high-entropy 32-bit word.
pub fn sample_hardware_noise() -> u32 {
    let mut entropy: u32 = 0;

    // 1. SysTick sub-microsecond cycle countdown register (24-bit, 48/16 MHz clock)
    let systick_val = unsafe { pac::SYST_CVR.read_volatile() & 0x00FF_FFFF };
    entropy ^= systick_val;

    // 2. TIM2 32-bit free-running microsecond timer
    let tim2_val = hal::time::now_us();
    entropy ^= tim2_val.rotate_left(13);

    // 3. Mix live ADC thermal/quantization noise and RTC low counter when clocked
    unsafe {
        if (pac::rcc::RCC_APB2ENR.read_volatile() & pac::rcc::APB2ENR_ADC1) != 0 {
            let adc_dr = pac::adc::ADC_DR.read_volatile();
            entropy ^= adc_dr.rotate_left(7);
        }
        if (pac::rcc::RCC_APB1ENR.read_volatile() & pac::rcc::APB1ENR_RTCAPB) != 0 {
            let rtc_lo = pac::rtc::RTC_CNTL.read_volatile();
            entropy ^= rtc_lo.rotate_left(19);
        }
    }

    // 4. Dynamic bit-rotation across sequential calls to prevent cancellation
    let rot = (ENTROPY_ROT.load(Ordering::Relaxed).wrapping_add(7)) & 31;
    ENTROPY_ROT.store(rot, Ordering::Relaxed);
    entropy.rotate_left(rot)
}

/// Gathers a composite 32-byte physical seed from 256-byte silicon UID and hardware noise.
pub fn gather_hardware_seed(out_seed: &mut [u8; 32]) {
    let uid_raw = crate::uid::read_uid_256();
    // Fold 256-byte UID into 32-byte base seed
    for (i, b) in uid_raw.iter().enumerate() {
        out_seed[i & 31] ^= *b;
    }

    // Inject high-resolution timer and cycle jitter into seed
    let noise1 = sample_hardware_noise().to_le_bytes();
    let noise2 = sample_hardware_noise().to_le_bytes();
    for i in 0..4 {
        out_seed[i] ^= noise1[i];
        out_seed[16 + i] ^= noise2[i];
    }
}
