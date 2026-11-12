//! KRX Native Thumb-1 Delta Relocation Engine v2 (KRX-ABI 2.5.0 Section 6.2).
//!
//! Provides normative in-place delta relocation decoding with 16-bit escape sequences.
//! Designed strictly for zero allocation and Cortex-M0+ execution safety.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocError {
    TruncatedStream,
    BadRelocOffset,
    RelocCountMismatch,
    UnalignedTarget,
}

/// Apply delta relocation stream v2 (KRX Listing 6.1).
///
/// # Arguments
/// * `data_base` - Pointer to the start of the relocatable data/code region.
/// * `reloc_region_size` - Size in bytes of the relocatable region.
/// * `stream` - Byte slice containing the escape-capable delta relocation stream.
/// * `load_delta` - Address displacement (`load_base - link_base`).
/// * `expect_count` - Total number of expected relocations from Execution manifest.
///
/// # Safety
/// Caller must ensure `data_base` points to writable memory of at least `reloc_region_size` bytes.
pub unsafe fn apply_relocs(
    data_base: *mut u8,
    reloc_region_size: usize,
    stream: &[u8],
    load_delta: u32,
    expect_count: u16,
) -> Result<(), RelocError> {
    if load_delta == 0 && expect_count == 0 {
        return Ok(());
    }

    let mut offset: usize = 0;
    let mut count: u16 = 0;
    let mut i = 0usize;

    loop {
        let d = *stream.get(i).ok_or(RelocError::TruncatedStream)?;
        i += 1;
        if d == 0 {
            break;
        }

        let delta: usize = if d == 0xFF {
            let lo = *stream.get(i).ok_or(RelocError::TruncatedStream)? as usize;
            let hi = *stream.get(i + 1).ok_or(RelocError::TruncatedStream)? as usize;
            i += 2;
            (lo | (hi << 8)).max(1)
        } else {
            d as usize
        };

        offset = if count == 0 { delta } else { offset + delta };
        if (offset & 3) != 0 || offset + 4 > reloc_region_size {
            return Err(RelocError::BadRelocOffset);
        }

        unsafe {
            let ptr = data_base.add(offset) as *mut u32;
            let current = *ptr;
            *ptr = current.wrapping_add(load_delta);
        }

        count += 1;
    }

    if count != expect_count {
        return Err(RelocError::RelocCountMismatch);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relocs_empty() {
        let mut buf = [0u8; 16];
        unsafe {
            assert_eq!(apply_relocs(buf.as_mut_ptr(), 16, &[0], 0, 0), Ok(()));
            assert_eq!(apply_relocs(buf.as_mut_ptr(), 16, &[], 0, 0), Ok(()));
        }
    }

    #[test]
    fn test_relocs_single() {
        let mut buf = [0u8; 32];
        // At offset 4, put initial pointer 0x2000_0000
        let initial_ptr = 0x2000_0000u32;
        buf[4..8].copy_from_slice(&initial_ptr.to_le_bytes());

        // Stream: delta=4, then end-of-stream=0
        let stream = [4u8, 0u8];
        let load_delta = 0x1000u32;

        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, load_delta, 1);
            assert_eq!(res, Ok(()));
        }

        let updated_ptr = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        assert_eq!(updated_ptr, 0x2000_1000);
    }

    #[test]
    fn test_relocs_multiple_positive_deltas() {
        let mut buf = [0u8; 64];
        let p1 = 0x1000_0000u32;
        let p2 = 0x1000_0040u32;
        buf[4..8].copy_from_slice(&p1.to_le_bytes());
        buf[12..16].copy_from_slice(&p2.to_le_bytes());

        // First reloc offset 4, second offset 12 (delta = 12 - 4 = 8)
        let stream = [4u8, 8u8, 0u8];
        let load_delta = 0x0000_0200u32;

        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, load_delta, 2);
            assert_eq!(res, Ok(()));
        }

        let updated_p1 = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let updated_p2 = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        assert_eq!(updated_p1, 0x1000_0200);
        assert_eq!(updated_p2, 0x1000_0240);
    }

    #[test]
    fn test_relocs_16bit_escape() {
        let mut buf = [0u8; 512];
        let target_offset = 300usize; // 300 % 4 == 0
        let p = 0x0800_1000u32;
        buf[target_offset..target_offset + 4].copy_from_slice(&p.to_le_bytes());

        // Escape: 0xFF, followed by 300 in little-endian (0x2C, 0x01), then 0
        let stream = [0xFFu8, 0x2Cu8, 0x01u8, 0u8];
        let load_delta = 0x0001_0000u32;

        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, load_delta, 1);
            assert_eq!(res, Ok(()));
        }

        let updated_p =
            u32::from_le_bytes(buf[target_offset..target_offset + 4].try_into().unwrap());
        assert_eq!(updated_p, 0x0801_1000);
    }

    #[test]
    fn test_relocs_unaligned_rejected() {
        let mut buf = [0u8; 32];
        // Delta 3 is unaligned (3 & 3 != 0)
        let stream = [3u8, 0u8];
        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, 0x100, 1);
            assert_eq!(res, Err(RelocError::BadRelocOffset));
        }
    }

    #[test]
    fn test_relocs_out_of_bounds_rejected() {
        let mut buf = [0u8; 16];
        // Target offset 16 -> offset + 4 > 16
        let stream = [16u8, 0u8];
        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, 0x100, 1);
            assert_eq!(res, Err(RelocError::BadRelocOffset));
        }
    }

    #[test]
    fn test_relocs_truncated_stream() {
        let mut buf = [0u8; 32];
        // Missing trailing 0
        let stream = [4u8];
        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, 0x100, 1);
            assert_eq!(res, Err(RelocError::TruncatedStream));
        }

        // Incomplete escape sequence
        let stream2 = [0xFFu8, 0x20u8];
        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream2, 0x100, 1);
            assert_eq!(res, Err(RelocError::TruncatedStream));
        }
    }

    #[test]
    fn test_relocs_count_mismatch() {
        let mut buf = [0u8; 32];
        let stream = [4u8, 0u8]; // 1 relocation
        unsafe {
            let res = apply_relocs(buf.as_mut_ptr(), buf.len(), &stream, 0x100, 2);
            assert_eq!(res, Err(RelocError::RelocCountMismatch));
        }
    }
}
