//! Two-Tier Application Swap & State Hibernation Manager (`app_hibernation`).
//!
//! - **Tier 1 (Volatile RAM RLE)**: `MAX_SWAP_SLOTS` × `64 B` (`128 B` SRAM) PackBits RLE
//!   slots with `16 B` `KrxSwapHeader` + CRC-16 for instant sub-millisecond resume.
//! - **Tier 2 (KernFS `/sys/swap/` Spill)**: Automatically checkpoints larger states
//!   (`> 48 B` compressed) or Tier-1 overflows into `/sys/swap/aXX.swp` using `staging_buf()`.

use kernel::runtime::{KrxSwapHeader, SWAP_FLAG_COMPRESSED_RLE};

/// Maximum concurrent Tier-1 hibernated application slots in volatile SRAM (`2 × 64 B = 128 B`).
pub const MAX_SWAP_SLOTS: usize = 2;

/// Tier-1 payload size per slot (`64 B` power-of-two slot geometry: `16 B` header + `48 B` payload).
pub const SWAP_PAYLOAD_SIZE: usize = 48;

/// Hibernation placement policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SwapPolicy {
    RamOnly = 0,
    RamThenFlash = 1,
    FlashOnly = 2,
}

/// Fixed 64-byte power-of-two Tier-1 swap container.
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct SwapSlot {
    pub header: KrxSwapHeader,
    pub payload: [u8; SWAP_PAYLOAD_SIZE],
}

const _: () = assert!(core::mem::size_of::<SwapSlot>() == 64);

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

    #[inline(always)]
    pub fn is_in_use(&self) -> bool {
        self.header.is_valid()
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.header.ident = [0; 4];
    }
}

pub static mut SWAP_SLOTS: [SwapSlot; MAX_SWAP_SLOTS] =
    [const { SwapSlot::empty() }; MAX_SWAP_SLOTS];

pub use kernel::runtime::{packbits_compress, packbits_decompress};

#[inline(never)]
fn format_swap_path(app_id: usize, buf: &mut [u8; 17]) -> &str {
    let p = buf.as_mut_ptr();
    unsafe {
        core::ptr::copy_nonoverlapping(b"/sys/swap/a00.swp".as_ptr(), p, 17);
        for i in 0..2usize {
            let nib = ((app_id >> ((1 - i) << 2)) & 0x0F) as u8;
            *p.add(11 + i) = if nib < 10 {
                b'0' + nib
            } else {
                b'a' + nib - 10
            };
        }
        core::str::from_utf8_unchecked(buf)
    }
}

#[inline(never)]
fn compress_with_header(
    app_id: usize,
    src_state: &[u8],
    dst_hdr: &mut KrxSwapHeader,
    dst_payload: &mut [u8],
) -> Result<u16, ()> {
    let comp_len = packbits_compress(src_state, dst_payload)?;
    let crc = crypto::crc::CrcEngine::crc16_ccitt(unsafe { dst_payload.get_unchecked(..comp_len) });
    *dst_hdr = KrxSwapHeader::new(
        app_id as u16,
        src_state.len() as u16,
        comp_len as u16,
        SWAP_FLAG_COMPRESSED_RLE,
        crc,
    );
    Ok(comp_len as u16)
}

#[inline(never)]
fn verify_and_decompress(
    hdr: &KrxSwapHeader,
    comp: &[u8],
    dst_state: &mut [u8],
) -> Result<usize, ()> {
    let comp_len = hdr.compressed_len as usize;
    if !hdr.is_valid() || comp_len > comp.len() {
        return Err(());
    }
    let slice = unsafe { comp.get_unchecked(..comp_len) };
    if crypto::crc::CrcEngine::crc16_ccitt(slice) != hdr.crc16 {
        return Err(());
    }
    let decomp_len = packbits_decompress(slice, dst_state)?;
    if decomp_len != hdr.original_len as usize {
        return Err(());
    }
    Ok(decomp_len)
}

#[inline(never)]
fn find_slot_mut(app_id: usize, allow_empty: bool) -> Option<&'static mut SwapSlot> {
    let slots = unsafe { &mut *core::ptr::addr_of_mut!(SWAP_SLOTS) };
    let mut empty: Option<usize> = None;
    let mut i = 0;
    while i < MAX_SWAP_SLOTS {
        if slots[i].is_in_use() {
            if slots[i].header.app_id == app_id as u16 {
                return Some(&mut slots[i]);
            }
        } else if allow_empty && empty.is_none() {
            empty = Some(i);
        }
        i += 1;
    }
    empty.map(|idx| &mut slots[idx])
}

/// Zero-allocation Two-Tier Application State Hibernation Manager.
pub struct AppSwapManager;

impl AppSwapManager {
    /// Hibernate application state into Tier-1 RAM (fast RLE slot).
    #[inline(always)]
    pub fn swap_out(app_id: usize, src_state: &[u8]) -> Result<u16, ()> {
        Self::swap_out_tiered(app_id, src_state, None, SwapPolicy::RamOnly)
    }

    /// Two-tier hibernation: tries Tier-1 RAM first (per `policy`), spilling to Tier-2 KernFS
    /// `/sys/swap/aXX.swp` when Tier-1 is full or the compressed payload exceeds 48 B.
    #[inline(never)]
    pub fn swap_out_tiered(
        app_id: usize,
        src_state: &[u8],
        storage: Option<&mut crate::services::SystemStorage>,
        policy: SwapPolicy,
    ) -> Result<u16, ()> {
        if let Some(slot) = find_slot_mut(app_id, true) {
            if let Ok(n) =
                compress_with_header(app_id, src_state, &mut slot.header, &mut slot.payload)
            {
                return Ok(n);
            }
            if slot.header.app_id == app_id as u16 {
                slot.clear();
            }
        }
        if policy == SwapPolicy::RamOnly {
            return Err(());
        }
        let vfs = storage.ok_or(())?;
        let stage = crate::data::arena::try_staging_buf().ok_or(())?;
        let hdr = unsafe { &mut *(stage.as_mut_ptr() as *mut KrxSwapHeader) };
        let pay_bytes = unsafe {
            core::slice::from_raw_parts_mut(
                stage.as_mut_ptr().add(KrxSwapHeader::SIZE),
                stage.len() - KrxSwapHeader::SIZE,
            )
        };
        let comp_len = compress_with_header(app_id, src_state, hdr, pay_bytes)?;
        let total = KrxSwapHeader::SIZE + comp_len as usize;
        let mut pbuf = [0u8; 17];
        vfs.write_file(format_swap_path(app_id, &mut pbuf), unsafe {
            stage.get_unchecked(..total)
        })
        .map_err(|_| ())?;
        Ok(comp_len)
    }

    /// Restore hibernated application state from Tier-1 RAM.
    #[inline(always)]
    pub fn swap_in(app_id: usize, dst_state: &mut [u8]) -> Result<usize, ()> {
        Self::swap_in_tiered(app_id, dst_state, None)
    }

    /// Two-tier restore: checks Tier-1 RAM first, then falls back to Tier-2 `/sys/swap/aXX.swp`.
    #[inline(never)]
    pub fn swap_in_tiered(
        app_id: usize,
        dst_state: &mut [u8],
        storage: Option<&mut crate::services::SystemStorage>,
    ) -> Result<usize, ()> {
        if let Some(slot) = find_slot_mut(app_id, false) {
            let n = verify_and_decompress(&slot.header, &slot.payload, dst_state)?;
            slot.clear();
            return Ok(n);
        }
        let vfs = storage.ok_or(())?;
        let stage = crate::data::arena::try_staging_buf().ok_or(())?;
        let mut pbuf = [0u8; 17];
        let path = format_swap_path(app_id, &mut pbuf);
        let n = vfs.read_file(path, stage).map_err(|_| ())?;
        if n < KrxSwapHeader::SIZE {
            return Err(());
        }
        let hdr = unsafe { &*(stage.as_ptr() as *const KrxSwapHeader) };
        let decomp_len = verify_and_decompress(
            hdr,
            unsafe { stage.get_unchecked(KrxSwapHeader::SIZE..n) },
            dst_state,
        )?;
        let _ = vfs.remove(path);
        Ok(decomp_len)
    }

    /// Check if `app_id` currently occupies a Tier-1 RAM hibernation slot.
    #[inline]
    pub fn has_swap(app_id: usize) -> bool {
        find_slot_mut(app_id, false).is_some()
    }

    /// Release Tier-1 swap slot without decompressing (e.g. on application kill).
    pub fn clear_swap(app_id: usize) -> bool {
        if let Some(slot) = find_slot_mut(app_id, false) {
            slot.clear();
            true
        } else {
            false
        }
    }

    /// Total bytes currently occupied by Tier-1 hibernated swap payloads.
    pub fn total_swap_bytes_used() -> usize {
        let slots = unsafe { &*core::ptr::addr_of!(SWAP_SLOTS) };
        slots
            .iter()
            .filter(|s| s.is_in_use())
            .map(|s| s.header.compressed_len as usize)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier1_ram_swap_roundtrip_and_clear() {
        for i in 0..4 {
            AppSwapManager::clear_swap(i);
        }
        let src = [0x42u8; 32];
        let comp = AppSwapManager::swap_out(1, &src).unwrap();
        assert!(comp < 16);
        assert!(AppSwapManager::has_swap(1));

        let mut dst = [0u8; 32];
        assert_eq!(AppSwapManager::swap_in(1, &mut dst), Ok(32));
        assert_eq!(dst, src);
        assert!(!AppSwapManager::has_swap(1));
    }

    #[test]
    fn tier2_swap_path_and_large_payload_codec_roundtrip() {
        let mut pbuf = [0u8; 17];
        assert_eq!(format_swap_path(0x07, &mut pbuf), "/sys/swap/a07.swp");
        assert_eq!(format_swap_path(0xAB, &mut pbuf), "/sys/swap/aab.swp");

        let mut src = [0u8; 96];
        for (i, b) in src.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        // Payload > 48 B compressed cannot fit in Tier-1 RAM slot (RamOnly rejects)
        assert!(AppSwapManager::swap_out(7, &src).is_err());

        // Tier-2 staging format (16 B KrxSwapHeader + compressed payload) roundtrips cleanly
        let stage = crate::data::arena::try_staging_buf().expect("staging buffer available");
        let (hdr_bytes, pay_bytes) = stage.split_at_mut(KrxSwapHeader::SIZE);
        let hdr = unsafe { &mut *(hdr_bytes.as_mut_ptr() as *mut KrxSwapHeader) };
        let comp_len = compress_with_header(7, &src, hdr, pay_bytes).expect("Tier-2 compress");
        assert!(comp_len as usize > SWAP_PAYLOAD_SIZE);

        let mut dst = [0u8; 96];
        let n = verify_and_decompress(hdr, &pay_bytes[..comp_len as usize], &mut dst)
            .expect("Tier-2 decompress");
        assert_eq!(n, 96);
        assert_eq!(dst, src);
    }
}
