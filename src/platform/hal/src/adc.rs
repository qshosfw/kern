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

    /// Read a raw 12-bit conversion value (0 - 4095) for the specified channel.
    #[inline(never)]
    pub fn read_channel(&self, channel: u8) -> u16 {
        unsafe {
            // 1. Ensure ADC is powered ON with SWSTART trigger selected
            let mut cr2 = adc::ADC_CR2.read_volatile();
            if (cr2 & adc::CR2_ADON) == 0 {
                cr2 |= adc::CR2_ADON | adc::CR2_EXTSEL_SWSTART;
                adc::ADC_CR2.write_volatile(cr2);
                crate::delay::delay_us(150);
            }

            // 2. Configure sequence: single conversion on specified channel (Rank 1 in SQR3)
            adc::ADC_SQR1.write_volatile(0);
            adc::ADC_SQR3.write_volatile((channel as u32) & 0x1F);

            // 3. Configure internal sensor path for TempSensor (Ch 16) or VREFINT (Ch 17)
            if channel >= 16 {
                cr2 |= adc::CR2_TSVREFE;
                adc::ADC_CR2.write_volatile(cr2);
                crate::delay::delay_us(20); // Internal reference buffer stabilization
            }

            // 4. Pre-clear any stale EOC/EOS flag in SR before starting new conversion
            adc::ADC_SR.write_volatile(!adc::SR_EOC);

            // Analog multiplexer settling delay (10us)
            crate::delay::delay_us(10);

            // 5. Start conversion: set SWSTART and EXTTRIG
            adc::ADC_CR2.write_volatile(cr2 | adc::CR2_EXTTRIG | adc::CR2_SWSTART);

            // 6. Poll for End Of Conversion (EOC in SR)
            let mut timeout = 50_000;
            while (adc::ADC_SR.read_volatile() & adc::SR_EOC) == 0 && timeout > 0 {
                timeout -= 1;
            }

            if timeout == 0 {
                return 0;
            }

            // 7. Read 12-bit conversion result from DR (reading DR clears EOC)
            let data = (adc::ADC_DR.read_volatile() & 0x0FFF) as u16;

            // 8. Explicitly clear EOC flag in SR (rc_w0: write 0 to EOC)
            adc::ADC_SR.write_volatile(!adc::SR_EOC);

            data
        }
    }

    /// Read a software-averaged 12-bit ADC value across `samples` iterations.
    #[inline(never)]
    pub fn read_channel_averaged(&self, channel: u8, samples: u8) -> u16 {
        let count = samples.max(1);
        let mut sum = 0u32;
        for _ in 0..count {
            sum += self.read_channel(channel) as u32;
        }
        let mut s = count;
        while s > 1 {
            sum >>= 1;
            s >>= 1;
        }
        sum as u16
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

    /// Read internal VREFINT (Channel 17 on PY32F071) and return calculated VDDA voltage in millivolts.
    pub fn read_vref_mv(&self) -> u16 {
        let v_raw = self.read_channel_averaged(17, 8);
        if v_raw == 0 {
            return 3300;
        }
        ((VREFINT_MV as u32 * 4095) / v_raw as u32) as u16
    }

    /// Read internal temperature sensor (Channel 16 on PY32F071).
    /// Returns temperature in deci-Celsius (e.g. 250 = 25.0°C).
    pub fn read_mcu_temp_deci_c(&self, vdda_mv: u16) -> i16 {
        unsafe {
            // Validate VDDA reference: if out of range, assume typical 3.3V
            let vdda = if vdda_mv < 1800 || vdda_mv > 3800 {
                3300u32
            } else {
                vdda_mv as u32
            };

            // 1. Primary factory calibration at 0x1FFF_3228 (30°C) and 0x1FFF_3230 (105°C)
            let c1 = TS_CAL1.read_volatile() as u32;
            let c2 = TS_CAL2.read_volatile() as u32;

            // 8-sample software average for high thermal resolution
            let ts_data = self.read_channel_averaged(16, 8) as u32;

            // Normalize raw counts to 3.3V factory calibration baseline:
            // (vdda * 0x4F70) >> 26 == vdda / 3300 in 32-bit math (exact across 1800..=3800 mV)
            let ts_data_norm = (ts_data * ((vdda * 0x4F70) >> 12)) >> 14;

            let deci_c = if c1 >= 200 && c1 <= 3800 && c2 <= 4000 && c2 >= c1 + 50 {
                // Two-point linear interpolation between 30°C (300 dC) and 105°C (1050 dC):
                // delta_T = 1050 - 300 = 750 deci-Celsius
                let cal_span = c2 - c1;
                let delta = ((ts_data_norm.abs_diff(c1) * 750) / cal_span) as i16;
                if ts_data_norm >= c1 {
                    300 + delta
                } else {
                    300 - delta
                }
            } else {
                // Unprogrammed fallback to typical silicon values:
                // V_30 = 0.75V (750 mV), Slope = 2.5 mV/°C (4 deci-°C per mV)
                // mv = (ts_data * vdda) / 4095 via 32-bit (prod + (prod >> 12)) >> 12
                let prod = ts_data * vdda;
                let mv = (prod + (prod >> 12)) >> 12;
                300 + ((mv as i32 - 750) * 4) as i16
            };

            // Clamp to physical silicon operating bounds (-40.0°C to +125.0°C)
            deci_c.clamp(-400, 1250)
        }
    }
}
