//! ARM EABI Intrinsics for unaligned byte accesses on Cortex-M0+ architectures.

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uwrite8(val: u64, addr: *mut u8) {
    let bytes = val.to_le_bytes();
    for i in 0..8 {
        unsafe {
            addr.add(i).write_volatile(bytes[i]);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uwrite4(val: u32, addr: *mut u8) {
    let bytes = val.to_le_bytes();
    for i in 0..4 {
        unsafe {
            addr.add(i).write_volatile(bytes[i]);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uread4(addr: *const u8) -> u32 {
    let mut bytes = [0u8; 4];
    for i in 0..4 {
        unsafe {
            bytes[i] = addr.add(i).read_volatile();
        }
    }
    u32::from_le_bytes(bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __aeabi_uread8(addr: *const u8) -> u64 {
    let mut bytes = [0u8; 8];
    for i in 0..8 {
        unsafe {
            bytes[i] = addr.add(i).read_volatile();
        }
    }
    u64::from_le_bytes(bytes)
}
