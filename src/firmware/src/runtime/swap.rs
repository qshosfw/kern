//! Application Swap & State Hibernation Manager (KRX-ABI / ORDS-01)
//!
//! Provides zero-allocation, bounded state hibernation with PackBits RLE compression
//! and 16-byte `KrxSwapHeader` integrity validation.

use kernel::runtime::{KrxSwapHeader, SWAP_FLAG_COMPRESSED_RLE};

/// Maximum concurrent hibernated application slots in volatile memory
pub const MAX_SWAP_SLOTS: usize = 3;

/// Payload size per slot (128 bytes total slot geometry: 16B header + 112B payload)
pub const SWAP_PAYLOAD_SIZE: usize = 112;

/// Exact power-of-two 128-byte swap container (§2 / ORDS-01)
#[derive(Clone, Copy)]
#[repr(C)]
pub struct SwapSlot {
    pub header: KrxSwapHeader,
    pub payload: [u8; SWAP_PAYLOAD_SIZE],
}

const _: () = assert!(core::mem::size_of::<SwapSlot>() == 128);

impl SwapSlot {
    pub const fn empty() -> Self {
        Self {
            header: KrxSwapHeader {
                ident: [0; 4],
                app_id: 0,
                original_len: 0,
                compressed_len: 0,
                flags: 0,
                crc16: 0,
                reserved: [0; 2],
            },
            payload: [0; SWAP_PAYLOAD_SIZE],
        }
    }

    #[inline]
    pub fn is_in_use(&self) -> bool {
        self.header.is_valid()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.header.ident = [0; 4];
        self.header.app_id = 0;
        self.header.compressed_len = 0;
        self.header.original_len = 0;
    }
}

pub static mut SWAP_SLOTS: [SwapSlot; MAX_SWAP_SLOTS] =
    [const { SwapSlot::empty() }; MAX_SWAP_SLOTS];

pub use kernel::runtime::{packbits_compress, packbits_decompress};

/// Zero-allocation Application State Hibernation Manager
pub struct AppSwapManager;

impl AppSwapManager {
    /// Hibernate application state into a swap slot.
    ///
    /// Compresses `src_state` with PackBits RLE, validates with CRC16, and attaches `KrxSwapHeader`.
    /// Returns Ok(compressed_len) on success.
    pub fn swap_out(app_id: usize, src_state: &[u8]) -> Result<u16, ()> {
        let slots = unsafe { &mut *core::ptr::addr_of_mut!(SWAP_SLOTS) };

        // Check if app already occupies a slot, else find first empty slot
        let mut target_idx = None;
        for (i, slot) in slots.iter().enumerate() {
            if slot.is_in_use() && slot.header.app_id == app_id as u16 {
                target_idx = Some(i);
                break;
            }
        }
        if target_idx.is_none() {
            for (i, slot) in slots.iter().enumerate() {
                if !slot.is_in_use() {
                    target_idx = Some(i);
                    break;
                }
            }
        }

        let idx = target_idx.ok_or(())?;
        let slot = &mut slots[idx];

        let compressed_len = packbits_compress(src_state, &mut slot.payload)?;
        let crc = crypto::crc::CrcEngine::crc16_ccitt(&slot.payload[..compressed_len]);

        slot.header = KrxSwapHeader::new(
            app_id as u16,
            src_state.len() as u16,
            compressed_len as u16,
            SWAP_FLAG_COMPRESSED_RLE,
            crc,
        );

        Ok(compressed_len as u16)
    }

    /// Restore hibernated application state from its swap slot.
    ///
    /// Validates `KrxSwapHeader` format identifier and payload CRC16 before decompressing into `dst_state`.
    /// Releases the swap slot upon successful decompression.
    pub fn swap_in(app_id: usize, dst_state: &mut [u8]) -> Result<usize, ()> {
        let slots = unsafe { &mut *core::ptr::addr_of_mut!(SWAP_SLOTS) };

        let slot = slots
            .iter_mut()
            .find(|s| s.is_in_use() && s.header.app_id == app_id as u16)
            .ok_or(())?;

        if !slot.header.is_valid() {
            return Err(());
        }

        let comp_len = slot.header.compressed_len as usize;
        if comp_len > slot.payload.len() {
            return Err(());
        }

        let expected_crc = crypto::crc::CrcEngine::crc16_ccitt(&slot.payload[..comp_len]);
        if expected_crc != slot.header.crc16 {
            return Err(());
        }

        let decomp_len = packbits_decompress(&slot.payload[..comp_len], dst_state)?;
        if decomp_len != slot.header.original_len as usize {
            return Err(());
        }

        slot.clear();
        Ok(decomp_len)
    }

    /// Check if a given application is currently swapped to a hibernation slot.
    #[inline]
    pub fn has_swap(app_id: usize) -> bool {
        let slots = unsafe { &*core::ptr::addr_of!(SWAP_SLOTS) };
        slots
            .iter()
            .any(|s| s.is_in_use() && s.header.app_id == app_id as u16)
    }

    /// Release swap slot without decompressing (e.g. on application kill).
    pub fn clear_swap(app_id: usize) -> bool {
        let slots = unsafe { &mut *core::ptr::addr_of_mut!(SWAP_SLOTS) };
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.is_in_use() && s.header.app_id == app_id as u16)
        {
            slot.clear();
            true
        } else {
            false
        }
    }

    /// Total bytes currently occupied by hibernated swap payloads.
    pub fn total_swap_bytes_used() -> usize {
        let slots = unsafe { &*core::ptr::addr_of!(SWAP_SLOTS) };
        slots
            .iter()
            .filter(|s| s.is_in_use())
            .map(|s| s.header.compressed_len as usize)
            .sum()
    }
}
