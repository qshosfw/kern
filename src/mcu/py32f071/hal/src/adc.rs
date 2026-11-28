//! 12-bit Analog-to-Digital Converter (ADC1) driver for PY32F071.
//!
//! Provides battery voltage (VBAT on Channel 8 / PB0), VDDA reference, and MCU temperature.

use crate::rcc;
use pac::adc;

/// Factory calibration values
const TS_CAL1: *const u16 = 0x1FFF_3228 as *const u16; // 30°C
const TS_CAL2: *const u16 = 0x1FFF_3230 as *const u16; // 105°C
const VREFINT_MV: u16 = 1200; // Typical 1.2V

/// 12-bit ADC handle for single-ended analog measurements.
pub struct Adc;

impl Adc {
    /// Initialize ADC1 peripheral clock and basic configuration.
    pub fn init() -> Self {
        rcc::enable_gpio_clocks();
        crate::gpio::set_pins_analog(pac::gpio::GPIOB_BASE, pac::gpio::PIN_0 | pac::gpio::PIN_1);
        rcc::enable_adc1();

        unsafe {
            // Wait for ADC clock to stabilize
            crate::delay::delay_us(100);

            // 1. Reset control registers: 12-bit, single-conversion, scan disabled
            adc::ADC_CR1.write_volatile(0);
            adc::ADC_SQR1.write_volatile(0);

            // 2. Set 239.5 cycle sampling time across all channels by default (SMPR1, SMPR2, SMPR3)
            adc::ADC_SMPR1.write_volatile(0x00FF_FFFF);
            adc::ADC_SMPR2.write_volatile(0x3FFF_FFFF);
            adc::ADC_SMPR3.write_volatile(0x3FFF_FFFF);

            // 3. Configure Trigger Source as SWSTART (EXTSEL = 0b111)
            adc::ADC_CR2.write_volatile(adc::CR2_EXTSEL_SWSTART);

            // 4. Start ADC Calibration (ADC must be disabled / ADON=0)
            adc::ADC_CR2.write_volatile(adc::CR2_CAL | adc::CR2_EXTSEL_SWSTART);
            let mut cal_timeout = 100_000;
            while (adc::ADC_CR2.read_volatile() & adc::CR2_CAL) != 0 && cal_timeout > 0 {
                cal_timeout -= 1;
            }

            // 5. Enable ADC (ADON = 1) and wait tSTAB (~150us)
            adc::ADC_CR2.write_volatile(adc::CR2_ADON | adc::CR2_EXTSEL_SWSTART);
            crate::delay::delay_us(150);
        }

        Self
    }

    /// Power down ADC1 analog bias (`ADON = 0`, `TSVREFE = 0`) to eliminate ~150-300 uA
    /// standby leakage during STOP or idle periods. `sample_injected_telemetry()` powers it back on lazily.
    pub fn power_down() {
        unsafe {
            if (pac::rcc::RCC_APB2ENR.read_volatile() & pac::rcc::APB2ENR_ADC1) != 0 {
                let cr2 = adc::ADC_CR2.read_volatile();
                adc::ADC_CR2.write_volatile(cr2 & !(adc::CR2_ADON | adc::CR2_TSVREFE));
            }
        }
    }

    /// Configure single-channel analog watchdog thresholds (12-bit counts).
    pub fn configure_watchdog(&self, channel: u8, low: u16, high: u16, enable_irq: bool) {
        unsafe {
            adc::ADC_LTR.write_volatile((low & 0x0FFF) as u32);
            adc::ADC_HTR.write_volatile((high & 0x0FFF) as u32);
            let mut cr1 = adc::ADC_CR1.read_volatile() & !(0x1F | adc::CR1_AWDIE);
            cr1 |= (channel as u32 & 0x1F) | adc::CR1_AWDSGL | adc::CR1_AWDEN;
            if enable_irq {
                cr1 |= adc::CR1_AWDIE;
            }
            adc::ADC_CR1.write_volatile(cr1);
            adc::ADC_SR.write_volatile(!adc::SR_AWD);
        }
    }

    /// Returns true if the analog watchdog threshold flag is latched.
    #[inline]
    pub fn watchdog_triggered(&self) -> bool {
        unsafe { (adc::ADC_SR.read_volatile() & adc::SR_AWD) != 0 }
    }

    /// Clear analog watchdog threshold flag.
    #[inline]
    pub fn clear_watchdog(&self) {
        unsafe {
            adc::ADC_SR.write_volatile(!adc::SR_AWD);
        }
    }

    /// Read latest 12-bit injected channel result from JDR1.
    #[inline]
    pub fn read_jdr1_raw(&self) -> u16 {
        unsafe { (adc::ADC_JDR1.read_volatile() & 0x0FFF) as u16 }
    }

    fn vref_raw_to_mv(v_raw: u16) -> u16 {
        if v_raw == 0 {
            3300
        } else {
            crate::div::udiv(VREFINT_MV as u32 * 4095, v_raw as u32) as u16
        }
    }

    fn temp_raw_to_deci_c(ts_raw: u16, vdda_mv: u16) -> i16 {
        unsafe {
            let vdda = if vdda_mv < 1800 || vdda_mv > 3800 {
                3300u32
            } else {
                vdda_mv as u32
            };
            let c1 = TS_CAL1.read_volatile() as u32;
            let c2 = TS_CAL2.read_volatile() as u32;
            let ts_data = ts_raw as u32;
            let ts_data_norm = (ts_data * ((vdda * 0x4F70) >> 12)) >> 14;

            let deci_c = if c1 >= 200 && c1 <= 3800 && c2 <= 4000 && c2 >= c1 + 50 {
                let cal_span = c2 - c1;
                let delta = crate::div::udiv(ts_data_norm.abs_diff(c1) * 750, cal_span) as i16;
                if ts_data_norm >= c1 {
                    300 + delta
                } else {
                    300 - delta
                }
            } else {
                let prod = ts_data * vdda;
                let mv = (prod + (prod >> 12)) >> 12;
                300 + ((mv as i32 - 750) * 4) as i16
            };
            deci_c.clamp(-400, 1250)
        }
    }

    /// Sample `(vbat_raw, vdda_mv, mcu_temp_deci_c)` via the hardware Injected Group (`JSQR` -> `JDR1..3`),
    /// leaving `ADC_SQR1..3` and `ADC_DR` completely untouched so `DMA1_Ch1` regular audio never stalls.
    pub fn sample_injected_telemetry(&self) -> (u16, u16, i16) {
        // JL = 2 (3 conversions, left-aligned in JSQR: JSQ2=CH8 VBAT, JSQ3=CH17 VREFINT, JSQ4=CH16 TEMP)
        const JSQR_TELEMETRY: u32 = (2 << 20) | (8 << 5) | (17 << 10) | (16 << 15);
        unsafe {
            let mut cr2 = adc::ADC_CR2.read_volatile();
            if (cr2 & adc::CR2_ADON) == 0 {
                cr2 |= adc::CR2_ADON | adc::CR2_EXTSEL_SWSTART;
                adc::ADC_CR2.write_volatile(cr2);
                crate::delay::delay_us(150);
            }
            if (cr2 & adc::CR2_TSVREFE) == 0 {
                cr2 |= adc::CR2_TSVREFE;
                adc::ADC_CR2.write_volatile(cr2);
                crate::delay::delay_us(20);
            }
            let prev_cr1 = adc::ADC_CR1.read_volatile();
            adc::ADC_CR1.write_volatile(prev_cr1 | adc::CR1_SCAN);
            adc::ADC_JSQR.write_volatile(JSQR_TELEMETRY);

            let mut vbat_sum = 0u32;
            let mut vref_last = 0u16;
            let mut temp_last = 0u16;
            for _ in 0..4u8 {
                adc::ADC_SR.write_volatile(!adc::SR_JEOC);
                adc::ADC_CR2.write_volatile(
                    cr2 | adc::CR2_JEXTSEL_JSWSTART | adc::CR2_JEXTTRIG | adc::CR2_JSWSTART,
                );
                let mut timeout = 50_000u32;
                while (adc::ADC_SR.read_volatile() & adc::SR_JEOC) == 0 && timeout > 0 {
                    timeout -= 1;
                }
                if timeout == 0 {
                    break;
                }
                vbat_sum += adc::ADC_JDR1.read_volatile() & 0x0FFF;
                vref_last = (adc::ADC_JDR2.read_volatile() & 0x0FFF) as u16;
                temp_last = (adc::ADC_JDR3.read_volatile() & 0x0FFF) as u16;
            }
            adc::ADC_SR.write_volatile(!adc::SR_JEOC);
            adc::ADC_CR1.write_volatile(prev_cr1);

            let vbat_raw = (vbat_sum >> 2) as u16;
            let vdda_mv = Self::vref_raw_to_mv(vref_last);
            let temp_deci_c = Self::temp_raw_to_deci_c(temp_last, vdda_mv);
            (vbat_raw, vdda_mv, temp_deci_c)
        }
    }
}
