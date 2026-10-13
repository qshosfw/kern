//! PBKDF-like Key Derivation Function (KDF) using ChaCha20 Permutation as PRF.
//!
//! Stretches MCU UID, user passcode, and unique salt into a 256-bit cryptographic key.

use crypto::buffer::SecretBuffer;
use crypto::chacha20poly1305::ChaCha20Poly1305;

#[inline(always)]
fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

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

/// Key Derivation Function (PBKDF) with in-word PRF stretching, watchdog feeding, and optional progress callback
pub fn derive_master_key(
    mcu_uid: &[u8; 12],
    user_passcode: &[u8],
    salt: &[u8; 12],
    iterations: u32,
    out_key: &mut SecretBuffer,
    wdt_feed_fn: Option<fn()>,
    progress: Option<ProgressHook>,
) {
    let mut current_key = SecretBuffer::new();
    let current_key_slice = current_key.as_mut_slice();

    // Layer 1: Hardware 96-bit UID + User Passcode seed expansion
    for i in 0..12 {
        current_key_slice[i] = mcu_uid[i];
    }
    for (i, &b) in user_passcode.iter().enumerate().take(20) {
        current_key_slice[i % 32] ^= b;
        current_key_slice[(12 + i) % 32] ^= b.rotate_left(3);
    }

    let iterations_count = iterations.max(100);

    let mut key_words = [0u32; 8];
    for i in 0..8 {
        key_words[i] = read_u32_le(current_key_slice, i * 4);
    }

    let mut nonce_words = [0u32; 3];
    for i in 0..3 {
        nonce_words[i] = read_u32_le(salt, i * 4);
    }

    const CHACHA_CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

    let update_interval = ((iterations_count * 205) >> 10).max(64);
    let mut next_update_step = update_interval;
    let mut current_pct: u8 = 0;

    let mut working = [0u32; 16];

    let initial_clk = py32_hal::rcc::sysclk_hz();
    if initial_clk < 72_000_000 {
        py32_hal::rcc::set_sysclk_72mhz();
    }

    // Layer 2: Fast In-Word ChaCha20 PRF key stretching iterations
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

        working[0] = CHACHA_CONSTANTS[0];
        working[1] = CHACHA_CONSTANTS[1];
        working[2] = CHACHA_CONSTANTS[2];
        working[3] = CHACHA_CONSTANTS[3];
        working[4] = key_words[0];
        working[5] = key_words[1];
        working[6] = key_words[2];
        working[7] = key_words[3];
        working[8] = key_words[4];
        working[9] = key_words[5];
        working[10] = key_words[6];
        working[11] = key_words[7];
        working[12] = step;
        working[13] = nonce_words[0];
        working[14] = nonce_words[1];
        working[15] = nonce_words[2];

        ChaCha20Poly1305::rounds(&mut working);

        for i in 0..4 {
            let kw = working[i].wrapping_add(CHACHA_CONSTANTS[i]);
            key_words[i] = kw;
            key_words[4 + i] = working[4 + i].wrapping_add(kw);
        }
    }

    let keep_72 = unsafe {
        let cfg = &*core::ptr::addr_of!(crate::MASTER_CONFIG);
        cfg.battery.cpu_clock == 1
    };
    if initial_clk < 72_000_000 && !keep_72 {
        py32_hal::rcc::set_sysclk_48mhz();
    }

    if let Some(ref hook) = progress {
        hook.report(100);
    }

    let out_slice = out_key.as_mut_slice();
    for i in 0..8 {
        out_slice[i * 4..(i + 1) * 4].copy_from_slice(&key_words[i].to_le_bytes());
    }
}
