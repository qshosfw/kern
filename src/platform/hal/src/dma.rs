//! DMA Driver for PY32F071.

use pac::dma;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    MemToPeriph,
    PeriphToMem,
    MemToMem,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Medium,
    High,
    VeryHigh,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataSize {
    Byte8,
    HalfWord16,
    Word32,
}

pub struct DmaConfig {
    pub direction: Direction,
    pub priority: Priority,
    pub mem_size: DataSize,
    pub periph_size: DataSize,
    pub mem_inc: bool,
    pub periph_inc: bool,
    pub circular: bool,
}

pub struct DmaChannel {
    channel_idx: u8,
}

impl DmaChannel {
    pub fn new(channel_idx: u8) -> Self {
        assert!((1..=7).contains(&channel_idx));
        Self { channel_idx }
    }

    pub fn configure(&mut self, config: DmaConfig) {
        let mut ccr = 0u32;

        match config.direction {
            Direction::MemToPeriph => {
                ccr |= dma::CCR_DIR;
            }
            Direction::PeriphToMem => { /* default */ }
            Direction::MemToMem => {
                ccr |= dma::CCR_MEM2MEM;
            }
        }

        let prio_bits = match config.priority {
            Priority::Low => 0b00,
            Priority::Medium => 0b01,
            Priority::High => 0b10,
            Priority::VeryHigh => 0b11,
        };
        ccr |= prio_bits << dma::CCR_PL_SHIFT;

        let msize_bits = match config.mem_size {
            DataSize::Byte8 => 0b00,
            DataSize::HalfWord16 => 0b01,
            DataSize::Word32 => 0b10,
        };
        ccr |= msize_bits << dma::CCR_MSIZE_SHIFT;

        let psize_bits = match config.periph_size {
            DataSize::Byte8 => 0b00,
            DataSize::HalfWord16 => 0b01,
            DataSize::Word32 => 0b10,
        };
        ccr |= psize_bits << dma::CCR_PSIZE_SHIFT;

        if config.mem_inc {
            ccr |= dma::CCR_MINC;
        }
        if config.periph_inc {
            ccr |= dma::CCR_PINC;
        }
        if config.circular {
            ccr |= dma::CCR_CIRC;
        }

        unsafe {
            dma::ccr(self.channel_idx).write_volatile(ccr);
        }
    }

    pub fn set_memory_address(&mut self, addr: u32) {
        unsafe {
            dma::cmar(self.channel_idx).write_volatile(addr);
        }
    }

    pub fn set_peripheral_address(&mut self, addr: u32) {
        unsafe {
            dma::cpar(self.channel_idx).write_volatile(addr);
        }
    }

    pub fn set_transfer_count(&mut self, count: u16) {
        unsafe {
            dma::cndtr(self.channel_idx).write_volatile(count as u32);
        }
    }

    pub fn enable(&mut self) {
        unsafe {
            let p = dma::ccr(self.channel_idx);
            p.write_volatile(p.read_volatile() | dma::CCR_EN);
        }
    }

    pub fn disable(&mut self) {
        unsafe {
            let p = dma::ccr(self.channel_idx);
            p.write_volatile(p.read_volatile() & !dma::CCR_EN);
        }
    }

    pub fn is_transfer_complete(&self) -> bool {
        unsafe { (dma::DMA_ISR.read_volatile() & dma::tcif(self.channel_idx)) != 0 }
    }

    pub fn clear_flags(&mut self) {
        unsafe {
            dma::DMA_IFCR.write_volatile(dma::gif(self.channel_idx));
        }
    }
}
