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
    /// Acquire a DMA1 channel (`1..=7`) and ensure the DMA1 AHB clock is enabled.
    pub fn new(channel_idx: u8) -> Self {
        assert!((1..=7).contains(&channel_idx));
        crate::rcc::enable_dma1();
        Self { channel_idx }
    }

    /// Route a PY32F071 peripheral DMA request ID (`0..=63`) to this channel via `SYSCFG_CFGR3/4`.
    pub fn remap(&mut self, request_id: u8) {
        crate::rcc::enable_syscfg();
        let (reg, shift) = pac::syscfg::dma_map_reg_and_shift(self.channel_idx);
        let mask = pac::syscfg::DMA_MAP_MASK << shift;
        let bits = ((request_id as u32) & pac::syscfg::DMA_MAP_MASK) << shift;
        cortex_m::interrupt::free(|_| unsafe {
            let val = reg.read_volatile();
            reg.write_volatile((val & !mask) | bits);
        });
    }

    /// Build a `CCR` control word from `DmaConfig` without mutating hardware registers.
    pub const fn encode_ccr(config: &DmaConfig) -> u32 {
        let mut ccr = match config.direction {
            Direction::MemToPeriph => dma::CCR_DIR,
            Direction::PeriphToMem => 0,
            Direction::MemToMem => dma::CCR_MEM2MEM,
        };
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
        ccr
    }

    pub fn configure(&mut self, config: DmaConfig) {
        let ccr = Self::encode_ccr(&config);
        unsafe {
            dma::ccr(self.channel_idx).write_volatile(ccr);
        }
    }

    /// Start a scoped RAII DMA transfer whose `Drop` unconditionally disarms `CCR.EN`
    /// so DMA can never continue accessing a dropped or out-of-scope buffer.
    pub fn start_scoped<'a>(
        &'a mut self,
        config: &DmaConfig,
        periph_addr: u32,
        mem_addr: u32,
        count: u16,
    ) -> DmaTransfer<'a> {
        let ccr = Self::encode_ccr(config);
        unsafe {
            dma::ccr(self.channel_idx).write_volatile(0);
            dma::DMA_IFCR.write_volatile(dma::gif(self.channel_idx));
            dma::cpar(self.channel_idx).write_volatile(periph_addr);
            dma::cmar(self.channel_idx).write_volatile(mem_addr);
            dma::cndtr(self.channel_idx).write_volatile(count as u32);
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            dma::ccr(self.channel_idx).write_volatile(ccr | dma::CCR_EN);
        }
        DmaTransfer { ch: self }
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

/// Scoped RAII guard for an active DMA1 transfer; clears `CCR.EN` and status flags on `Drop`.
pub struct DmaTransfer<'a> {
    ch: &'a mut DmaChannel,
}

impl<'a> DmaTransfer<'a> {
    #[inline(always)]
    pub fn is_complete(&self) -> bool {
        self.ch.is_transfer_complete()
    }

    /// Poll until transfer complete or `max_polls` expires; returns `true` if completed.
    pub fn wait(self, mut max_polls: u32) -> bool {
        while !self.ch.is_transfer_complete() && max_polls > 0 {
            max_polls -= 1;
        }
        self.ch.is_transfer_complete()
    }
}

impl<'a> Drop for DmaTransfer<'a> {
    fn drop(&mut self) {
        self.ch.disable();
        self.ch.clear_flags();
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}
