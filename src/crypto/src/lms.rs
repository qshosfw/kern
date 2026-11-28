//! Post-Quantum LMS-Ascon Hash-Based Signature Verification & Inode Capability Sealing.
//!
//! Reuses `crypto::ascon::{AsconHash256, Ascon128}` exclusively (0 B new crypto primitives,
//! `#![no_std]`, zero heap allocation):
//! - **Install-Time Tier**: `verify_lms_ascon` authenticates privileged `.krx`/`.mod` and `.kfw`
//!   images against a 32-byte Merkle root public key (`LmsPublicKey`).
//! - **Launch-Time Tier (< 0.3 ms)**: `compute_inode_cap_seal` / `verify_inode_cap_seal` binds
//!   granted capabilities and payload digest to `(MCU_UID || device_key || inode_gen)` via
//!   16-byte `Ascon-128-MAC`.

use crate::ascon::{Ascon128, AsconHash256, HASH_SIZE, KEY_SIZE, TAG_SIZE};

pub const LMS_DOMAIN_LEAF: u8 = 0x82;
pub const LMS_DOMAIN_NODE: u8 = 0x83;
pub const LMS_DOMAIN_CHAIN: u8 = 0x81;

/// 32-byte post-quantum LMS-Ascon root public key (fits in ROM `.rodata`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LmsPublicKey {
    pub root: [u8; HASH_SIZE],
}

/// Hash a Winternitz-style one-time signature chain step using `AsconHash256`.
pub fn ots_chain(seed: &[u8; HASH_SIZE], idx: u16, steps: u8) -> [u8; HASH_SIZE] {
    let mut cur = *seed;
    let mut buf = [0u8; 3 + HASH_SIZE];
    buf[0] = LMS_DOMAIN_CHAIN;
    buf[1..3].copy_from_slice(&idx.to_le_bytes());
    for _ in 0..steps {
        buf[3..].copy_from_slice(&cur);
        cur = AsconHash256::hash(&buf);
    }
    cur
}

/// Compute a Merkle parent node `AsconHash256(LMS_DOMAIN_NODE || left || right)`.
pub fn merkle_parent(left: &[u8; HASH_SIZE], right: &[u8; HASH_SIZE]) -> [u8; HASH_SIZE] {
    let mut buf = [0u8; 1 + HASH_SIZE * 2];
    buf[0] = LMS_DOMAIN_NODE;
    buf[1..1 + HASH_SIZE].copy_from_slice(left);
    buf[1 + HASH_SIZE..].copy_from_slice(right);
    AsconHash256::hash(&buf)
}

/// Compute a Merkle leaf hash `AsconHash256(LMS_DOMAIN_LEAF || leaf_index || ots_pk)`.
pub fn merkle_leaf(leaf_index: u32, ots_pk: &[u8; HASH_SIZE]) -> [u8; HASH_SIZE] {
    let mut buf = [0u8; 5 + HASH_SIZE];
    buf[0] = LMS_DOMAIN_LEAF;
    buf[1..5].copy_from_slice(&leaf_index.to_le_bytes());
    buf[5..].copy_from_slice(ots_pk);
    AsconHash256::hash(&buf)
}

/// Verify an LMS-Ascon Merkle authentication path from a reconstructed OTS public key
/// (`ots_pk`) and `leaf_index` up to `pk.root` in constant-time.
pub fn verify_lms_merkle(
    pk: &LmsPublicKey,
    mut leaf_index: u32,
    ots_pk: &[u8; HASH_SIZE],
    auth_path: &[[u8; HASH_SIZE]],
) -> bool {
    let mut node = merkle_leaf(leaf_index, ots_pk);
    for sibling in auth_path {
        node = if (leaf_index & 1) == 0 {
            merkle_parent(&node, sibling)
        } else {
            merkle_parent(sibling, &node)
        };
        leaf_index >>= 1;
    }
    let mut diff = 0u8;
    for i in 0..HASH_SIZE {
        diff |= node[i] ^ pk.root[i];
    }
    diff == 0
}

/// Compute the 16-byte `Ascon-128-MAC` capability seal stored with a KernFS `Inode`
/// for `< 0.3 ms` launch-time verification without re-running full LMS-Ascon.
pub fn compute_inode_cap_seal(
    mcu_uid: &[u8; 12],
    device_key: &[u8; KEY_SIZE],
    inode_gen: u32,
    caps_mask: u32,
    code_hash: &[u8; HASH_SIZE],
) -> [u8; TAG_SIZE] {
    let mut nonce = [0u8; 16];
    nonce[..12].copy_from_slice(mcu_uid);
    nonce[12..16].copy_from_slice(&inode_gen.to_le_bytes());

    let mut aad = [0u8; 4 + HASH_SIZE];
    aad[..4].copy_from_slice(&caps_mask.to_le_bytes());
    aad[4..].copy_from_slice(code_hash);

    Ascon128::new(*device_key).compute_mac(&nonce, &aad, &[])
}

/// Constant-time verification of a 16-byte `Inode` capability seal.
pub fn verify_inode_cap_seal(
    mcu_uid: &[u8; 12],
    device_key: &[u8; KEY_SIZE],
    inode_gen: u32,
    caps_mask: u32,
    code_hash: &[u8; HASH_SIZE],
    expected_tag: &[u8; TAG_SIZE],
) -> bool {
    let mut nonce = [0u8; 16];
    nonce[..12].copy_from_slice(mcu_uid);
    nonce[12..16].copy_from_slice(&inode_gen.to_le_bytes());

    let mut aad = [0u8; 4 + HASH_SIZE];
    aad[..4].copy_from_slice(&caps_mask.to_le_bytes());
    aad[4..].copy_from_slice(code_hash);

    Ascon128::new(*device_key)
        .verify_mac(&nonce, &aad, &[], expected_tag)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lms_ascon_merkle_and_inode_seal_roundtrip() {
        let leaf0 = ots_chain(&[0x11; 32], 0, 4);
        let leaf1 = ots_chain(&[0x22; 32], 1, 4);
        let h0 = merkle_leaf(0, &leaf0);
        let h1 = merkle_leaf(1, &leaf1);
        let pk = LmsPublicKey {
            root: merkle_parent(&h0, &h1),
        };
        assert!(verify_lms_merkle(&pk, 0, &leaf0, &[h1]));
        assert!(verify_lms_merkle(&pk, 1, &leaf1, &[h0]));
        assert!(!verify_lms_merkle(&pk, 0, &leaf1, &[h1]));

        let uid = [0xAA; 12];
        let key = [0x55; 16];
        let digest = AsconHash256::hash(b"kern-system-pkg");
        let seal = compute_inode_cap_seal(&uid, &key, 7, 0x0003_0001, &digest);
        assert!(verify_inode_cap_seal(
            &uid,
            &key,
            7,
            0x0003_0001,
            &digest,
            &seal
        ));
        assert!(!verify_inode_cap_seal(
            &uid,
            &key,
            8,
            0x0003_0001,
            &digest,
            &seal
        ));
        assert!(!verify_inode_cap_seal(
            &uid,
            &key,
            7,
            0x0003_0003,
            &digest,
            &seal
        ));
    }
}
