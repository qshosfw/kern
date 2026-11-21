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
pub fn derive_master_key(
    mcu_uid: &[u8; 12],
    user_passcode: &[u8],
    salt: &[u8; 16],
    iterations: u32,
    out_key: &mut SecretBuf<32>,
    wdt_feed_fn: Option<fn()>,
    progress: Option<ProgressHook>,
) {
    let mut state = AsconPermutation::new();

    // Domain separation IV for KERN Ascon-KDF: 'KERN' | 'KDF1'
    state.x[0] = 0x4B45_524E_4B44_4631;

    // Word 1 & 2: 16-byte salt
    state.x[1] = u64::from_le_bytes([
        salt[0], salt[1], salt[2], salt[3], salt[4], salt[5], salt[6], salt[7],
    ]);
    state.x[2] = u64::from_le_bytes([
        salt[8], salt[9], salt[10], salt[11], salt[12], salt[13], salt[14], salt[15],
    ]);

    // Word 3 & 4: 12-byte MCU UID + 4-byte iterations count
    let uid_low = u64::from_le_bytes([
        mcu_uid[0], mcu_uid[1], mcu_uid[2], mcu_uid[3], mcu_uid[4], mcu_uid[5], mcu_uid[6],
        mcu_uid[7],
    ]);
    let uid_high = u32::from_le_bytes([mcu_uid[8], mcu_uid[9], mcu_uid[10], mcu_uid[11]]);
    state.x[3] = uid_low;
    state.x[4] = (uid_high as u64) | ((iterations as u64) << 32);

    // Initial permutation p^12
    state.permute(12);

    // Absorb user passcode in 8-byte blocks
    let full_blocks = user_passcode.len() / 8;
    for i in 0..full_blocks {
        let chunk = &user_passcode[i * 8..(i + 1) * 8];
        let p_word = u64::from_le_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]);
        state.x[0] ^= p_word;
        state.permute(6);
    }
    let rem = &user_passcode[full_blocks * 8..];
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
    if initial_clk < 72_000_000 {
        py32_hal::rcc::set_sysclk_72mhz();
    }

    // Layer 2: Fast Ascon permutation PRF key stretching loop
    for step in 0..iterations_count {
        if (step & 63) == 0 {
            if let Some(feed) = wdt_feed_fn {
                feed();
            }
        }
        if step >= next_update_step {
            next_update_step = next_update_step.saturating_add(update_interval);
            if let Some(ref hook) = progress {
                current_pct = current_pct.saturating_add(20).min(100);
                hook.report(current_pct);
            }
        }

        // Mix step counter into state word 2 and permute
        state.x[2] ^= (step as u64).rotate_left((step & 31) as u32);
        state.permute(6);
    }

    let keep_72 = crate::master_config_ref().system.cpu_clock == 4;
    if initial_clk < 72_000_000 && !keep_72 {
        py32_hal::rcc::set_sysclk_index(crate::master_config_ref().system.cpu_clock);
    }

    if let Some(ref hook) = progress {
        hook.report(100);
    }

    // Final squeeze (p^12) into 256-bit output key
    state.permute(12);
    let out_slice = out_key.as_mut_slice();
    out_slice[0..8].copy_from_slice(&state.x[0].to_le_bytes());
    out_slice[8..16].copy_from_slice(&state.x[1].to_le_bytes());
    out_slice[16..24].copy_from_slice(&state.x[2].to_le_bytes());
    out_slice[24..32].copy_from_slice(&state.x[3].to_le_bytes());
}
