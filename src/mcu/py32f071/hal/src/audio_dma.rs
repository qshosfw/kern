//! Bidirectional `DMA1` Baseband Audio Streaming (`TIM6`/`ADC1` 24 kHz RX + `TIM7`/`DAC1` 12 kHz TX)
//! & Zero-Heap `4-bit IMA-ADPCM` Codec (`6 KB/s` QSO recording & RigWire `STREAM::AUDI`).
//!
//! Hardware allocation (`docs/silicon-dma-audio-and-k10pro.md §3`):
//! - RX: `PA4` (`BK4829 AF_OUT`, `ADC1_IN4`) <- `TIM6` `TRGO` @ `24 kHz` -> `DMA1_Ch1` circular
//!   ping-pong `2 × 128 × u16` (`512 B`, `HTIF1`/`TCIF1` every `5.33 ms`, `< 1.5%` CPU).
//! - TX: `TIM7` `TRGO` @ `12 kHz` -> `DMA1_Ch7` -> `DAC1_DHR12R1` (`PA4` / `PB1`).

use crate::pac::{adc, dma, rcc, tim};

pub const AUDIO_RX_HALF_SAMPLES: usize = 128;
pub const AUDIO_RX_BUF_SAMPLES: usize = AUDIO_RX_HALF_SAMPLES * 2;
pub const AUDIO_TX_HALF_SAMPLES: usize = 64;
pub const AUDIO_TX_BUF_SAMPLES: usize = AUDIO_TX_HALF_SAMPLES * 2;

/// `DMA1_Ch1` circular RX ping-pong buffer (`2 × 128 × u16 = 512 B`).
#[unsafe(link_section = ".uninit")]
static mut AUDIO_RX_PINGPONG: [u16; AUDIO_RX_BUF_SAMPLES] = [0; AUDIO_RX_BUF_SAMPLES];

/// `DMA1_Ch7` circular TX ping-pong buffer (`2 × 64 × u16 = 256 B`).
#[unsafe(link_section = ".uninit")]
static mut AUDIO_TX_PINGPONG: [u16; AUDIO_TX_BUF_SAMPLES] = [0; AUDIO_TX_BUF_SAMPLES];

/// Configure `TIM6` @ `24 kHz` TRGO + `ADC1` (`CH4` = `PA4`) + `DMA1_Ch1` circular ping-pong RX.
pub fn start_rx_dma_24khz(sysclk_hz: u32) {
    let arr = (sysclk_hz / 24_000).saturating_sub(1).max(1) as u16;
    unsafe {
        // Enable TIM6 (APB1 bit 4) and DMA1 (AHB bit 0)
        rcc::RCC_APB1ENR.write_volatile(rcc::RCC_APB1ENR.read_volatile() | (1 << 4));
        rcc::RCC_AHBENR.write_volatile(rcc::RCC_AHBENR.read_volatile() | (1 << 0));

        // Configure DMA1_Ch1: ADC1_DR -> AUDIO_RX_PINGPONG, 16-bit circular, HTIE | TCIE
        dma::ccr(1).write_volatile(0);
        dma::cndtr(1).write_volatile(AUDIO_RX_BUF_SAMPLES as u32);
        dma::cpar(1).write_volatile(adc::ADC_DR as u32);
        dma::cmar(1).write_volatile(core::ptr::addr_of!(AUDIO_RX_PINGPONG) as u32);
        dma::ccr(1).write_volatile(
            (1 << dma::CCR_MSIZE_SHIFT)
                | (1 << dma::CCR_PSIZE_SHIFT)
                | dma::CCR_MINC
                | dma::CCR_CIRC
                | dma::CCR_HTIE
                | dma::CCR_TCIE
                | dma::CCR_EN,
        );

        // Configure ADC1 regular sequence: CH4 (PA4), DMA enable, external trigger TIM6_TRGO
        adc::ADC_SQR1.write_volatile(0);
        adc::ADC_SQR3.write_volatile(4);
        let cr2 = adc::ADC_CR2.read_volatile();
        adc::ADC_CR2.write_volatile(cr2 | (1 << 8) | adc::CR2_EXTTRIG);

        // Configure TIM6: PSC=0, ARR=arr, MMS=010 (Update -> TRGO), CEN=1
        let t6 = tim::TIM6_BASE;
        ((t6 + 0x28) as *mut u32).write_volatile(0); // PSC
        ((t6 + 0x2C) as *mut u32).write_volatile(arr as u32); // ARR
        ((t6 + 0x04) as *mut u32).write_volatile(0x0020); // CR2: MMS = Update
        ((t6 + 0x14) as *mut u32).write_volatile(1); // EGR: UG
        (t6 as *mut u32).write_volatile(1); // CR1: CEN
    }
}

/// Stop `TIM6` + `DMA1_Ch1` RX audio capture cleanly.
pub fn stop_rx_dma() {
    unsafe {
        (tim::TIM6_BASE as *mut u32).write_volatile(0);
        dma::ccr(1).write_volatile(0);
    }
}

/// Poll `DMA1_Ch1` half/full transfer flags and return the ready `128`-sample RX slice.
pub fn poll_rx_half() -> Option<&'static [u16; AUDIO_RX_HALF_SAMPLES]> {
    unsafe {
        let isr = dma::DMA_ISR.read_volatile();
        if (isr & dma::htif(1)) != 0 {
            dma::DMA_IFCR.write_volatile(dma::htif(1));
            let p = core::ptr::addr_of!(AUDIO_RX_PINGPONG) as *const u16;
            return Some(&*(p as *const [u16; AUDIO_RX_HALF_SAMPLES]));
        }
        if (isr & dma::tcif(1)) != 0 {
            dma::DMA_IFCR.write_volatile(dma::tcif(1));
            let p =
                (core::ptr::addr_of!(AUDIO_RX_PINGPONG) as *const u16).add(AUDIO_RX_HALF_SAMPLES);
            return Some(&*(p as *const [u16; AUDIO_RX_HALF_SAMPLES]));
        }
    }
    None
}

/// Configure `TIM7` @ `12 kHz` TRGO + `DMA1_Ch7` -> `DAC1_DHR12R1` circular ping-pong TX.
pub fn start_tx_dma_12khz(sysclk_hz: u32) {
    const DAC_DHR12R1: u32 = 0x4000_7408;
    let arr = (sysclk_hz / 12_000).saturating_sub(1).max(1) as u16;
    unsafe {
        // Enable TIM7 (APB1 bit 5) and DAC (APB1 bit 29)
        rcc::RCC_APB1ENR.write_volatile(rcc::RCC_APB1ENR.read_volatile() | (1 << 5) | (1 << 29));
        rcc::RCC_AHBENR.write_volatile(rcc::RCC_AHBENR.read_volatile() | (1 << 0));

        dma::ccr(7).write_volatile(0);
        dma::cndtr(7).write_volatile(AUDIO_TX_BUF_SAMPLES as u32);
        dma::cpar(7).write_volatile(DAC_DHR12R1);
        dma::cmar(7).write_volatile(core::ptr::addr_of!(AUDIO_TX_PINGPONG) as u32);
        dma::ccr(7).write_volatile(
            (1 << dma::CCR_MSIZE_SHIFT)
                | (1 << dma::CCR_PSIZE_SHIFT)
                | dma::CCR_MINC
                | dma::CCR_CIRC
                | dma::CCR_DIR
                | dma::CCR_EN,
        );

        let t7 = tim::TIM7_BASE;
        ((t7 + 0x28) as *mut u32).write_volatile(0);
        ((t7 + 0x2C) as *mut u32).write_volatile(arr as u32);
        ((t7 + 0x04) as *mut u32).write_volatile(0x0020);
        ((t7 + 0x14) as *mut u32).write_volatile(1);
        (t7 as *mut u32).write_volatile(1);
    }
}

pub fn stop_tx_dma() {
    unsafe {
        (tim::TIM7_BASE as *mut u32).write_volatile(0);
        dma::ccr(7).write_volatile(0);
    }
}

/// Zero-heap `4-bit IMA-ADPCM` predictor state (`4 B` RAM) for `6 KB/s` voice recording & `STREAM::AUDI`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImaAdpcmState {
    pub predictor: i16,
    pub index: u8,
}

const IMA_INDEX_TABLE: [i8; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];
const IMA_STEP_TABLE: [u16; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

impl ImaAdpcmState {
    pub const fn new() -> Self {
        Self {
            predictor: 0,
            index: 0,
        }
    }

    pub fn encode_nibble(&mut self, sample: i16) -> u8 {
        let step = IMA_STEP_TABLE[self.index as usize] as i32;
        let mut diff = (sample as i32) - (self.predictor as i32);
        let mut nib = if diff < 0 {
            diff = -diff;
            8u8
        } else {
            0u8
        };
        let mut vpdiff = step >> 3;
        if diff >= step {
            nib |= 4;
            diff -= step;
            vpdiff += step;
        }
        if diff >= (step >> 1) {
            nib |= 2;
            diff -= step >> 1;
            vpdiff += step >> 1;
        }
        if diff >= (step >> 2) {
            nib |= 1;
            vpdiff += step >> 2;
        }
        let pred = if (nib & 8) != 0 {
            (self.predictor as i32) - vpdiff
        } else {
            (self.predictor as i32) + vpdiff
        };
        self.predictor = pred.clamp(-32768, 32767) as i16;
        self.index =
            ((self.index as i32) + (IMA_INDEX_TABLE[nib as usize] as i32)).clamp(0, 88) as u8;
        nib
    }

    pub fn decode_nibble(&mut self, nib: u8) -> i16 {
        let step = IMA_STEP_TABLE[self.index as usize] as i32;
        let mut vpdiff = step >> 3;
        if (nib & 4) != 0 {
            vpdiff += step;
        }
        if (nib & 2) != 0 {
            vpdiff += step >> 1;
        }
        if (nib & 1) != 0 {
            vpdiff += step >> 2;
        }
        let pred = if (nib & 8) != 0 {
            (self.predictor as i32) - vpdiff
        } else {
            (self.predictor as i32) + vpdiff
        };
        self.predictor = pred.clamp(-32768, 32767) as i16;
        self.index = ((self.index as i32) + (IMA_INDEX_TABLE[(nib & 0x0F) as usize] as i32))
            .clamp(0, 88) as u8;
        self.predictor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ima_adpcm_roundtrip_tracks_waveform() {
        let mut enc = ImaAdpcmState::new();
        let mut dec = ImaAdpcmState::new();
        for i in 0..32i16 {
            let sample = i * 250;
            let nib = enc.encode_nibble(sample);
            let out = dec.decode_nibble(nib);
            assert_eq!(enc.predictor, dec.predictor);
            assert_eq!(enc.index, dec.index);
            if i >= 10 {
                assert!((sample as i32 - out as i32).abs() < 400);
            }
        }
    }
}
