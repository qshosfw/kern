//! PBKDF-like Key Derivation Function (KDF) using Ascon Permutation Sponge as PRF.
//!
//! Stretches MCU UID, user passcode, and unique salt into a 256-bit cryptographic key.

use crypto::ascon::AsconPermutation;
use crypto::buffer::SecretBuf;

#[derive(Clone, Copy)]
pub struct ProgressHook {
    pub ctx: *mut (),
    pub callback: fn(*mut (), u8),
}

impl ProgressHook {
    #[inline(always)]
    pub const fn new(ctx: *mut (), callback: fn(*mut (), u8)) -> Self {
        Self { ctx, callback }
    }

    #[inline(always)]
    pub fn report(&self, pct: u8) {
        (self.callback)(self.ctx, pct);
    }
}

/// Key Derivation Function (PBKDF) using Ascon sponge permutation with in-word PRF stretching,
/// watchdog feeding, and optional progress callback.
#[inline(never)]
pub fn derive_master_key(
    mcu_uid: &[u8; 12],
    user_passcode: &[u8],
    salt: &[u8; 16],
    iterations: u32,
    out_key: &mut SecretBuf<32>,
    progress: Option<ProgressHook>,
) {
    let mut state = AsconPermutation::new();

    // Domain separation IV for KERN Ascon-KDF: 'KERN' | 'KDF1'
    state.x[0] = 0x4B45_524E_4B44_4631;

    #[cfg(target_endian = "little")]
    unsafe {
        let p = (&raw mut state.x[1]).cast::<u8>();
        core::ptr::copy_nonoverlapping(salt.as_ptr(), p, 16);
        core::ptr::copy_nonoverlapping(mcu_uid.as_ptr(), p.add(16), 12);
        core::ptr::copy_nonoverlapping(iterations.to_le_bytes().as_ptr(), p.add(28), 4);
    }
    #[cfg(not(target_endian = "little"))]
    {
        for i in 0..2 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&salt[i * 8..(i + 1) * 8]);
            state.x[1 + i] = u64::from_le_bytes(b);
        }
        let mut uid_b = [0u8; 8];
        uid_b.copy_from_slice(&mcu_uid[0..8]);
        state.x[3] = u64::from_le_bytes(uid_b);
        let uid_high = u32::from_le_bytes([mcu_uid[8], mcu_uid[9], mcu_uid[10], mcu_uid[11]]);
        state.x[4] = (uid_high as u64) | ((iterations as u64) << 32);
    }

    // Initial permutation p^12
    state.permute(12);

    // Absorb user passcode in 8-byte blocks
    let mut off = 0;
    while off + 8 <= user_passcode.len() {
        let mut b = [0u8; 8];
        b.copy_from_slice(&user_passcode[off..off + 8]);
        state.x[0] ^= u64::from_le_bytes(b);
        state.permute(6);
        off += 8;
    }
    let rem = &user_passcode[off..];
    let mut pad_block = [0u8; 8];
    pad_block[..rem.len()].copy_from_slice(rem);
    pad_block[rem.len()] = 0x80;
    state.x[0] ^= u64::from_le_bytes(pad_block);
    state.permute(12);

    let iterations_count = iterations.max(100);
    let update_interval = ((iterations_count * 205) >> 10).max(64);
    let mut next_update_step = update_interval;
    let mut current_pct: u8 = 0;

    let initial_clk = py32_hal::rcc::sysclk_hz();
    #[cfg(feature = "usb")]
    let usb_pinned = drivers::usb::is_active_usb_connected();
    #[cfg(not(feature = "usb"))]
    let usb_pinned = false;
    let boosted = !usb_pinned && initial_clk < 72_000_000;
    if boosted {
        py32_hal::rcc::set_sysclk_72mhz();
    }

    // Layer 2: Fast Ascon permutation PRF key stretching loop
    for step in 0..iterations_count {
        if (step & 63) == 0 {
            bsp::Board::wdt_feed();
        }
        if step >= next_update_step {
            next_update_step = next_update_step.saturating_add(update_interval);
            if let Some(ref hook) = progress {
                current_pct = current_pct.saturating_add(20).min(100);
                hook.report(current_pct);
            }
        }

        // Mix step counter into state word 2 and permute
        state.x[2] ^= (step.rotate_left(step & 31)) as u64;
        state.permute(6);
    }

    if boosted {
        py32_hal::rcc::set_sysclk_index(crate::master_config_ref().system.cpu_clock);
    }

    if let Some(ref hook) = progress {
        hook.report(100);
    }

    // Final squeeze (p^12) into 256-bit output key
    state.permute(12);
    #[cfg(target_endian = "little")]
    unsafe {
        core::ptr::copy_nonoverlapping(
            state.x.as_ptr().cast::<u8>(),
            out_key.as_mut_slice().as_mut_ptr(),
            32,
        );
    }
    #[cfg(not(target_endian = "little"))]
    {
        let out_slice = out_key.as_mut_slice();
        for i in 0..4 {
            out_slice[i * 8..(i + 1) * 8].copy_from_slice(&state.x[i].to_le_bytes());
        }
    }
}
