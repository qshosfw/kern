//! Embedded Firmware Binary Metadata & Build Descriptors.
//!
//! Provides compile-time build stamps, Git revision metadata,
//! and a compact binary firmware header (`.rodata.fw_meta`) readable by
//! external flashers and updater utilities.

#[repr(C)]
pub struct FirmwareMeta {
    pub ident: [u8; 4], // "KERN"
    pub version_major: u8,
    pub version_minor: u8,
    pub version_patch: u8,
    pub target_id: u8,     // 1 = UV-K1, 5 = UV-K5
    pub build_epoch: u32,  // Unix timestamp (32-bit UTC seconds)
    pub git_hash: [u8; 8], // 7-char short hash + null
    pub flags: u32,        // Active feature flag bitmask
}

const _: () = {
    assert!(core::mem::size_of::<FirmwareMeta>() == 24);
    assert!(core::mem::align_of::<FirmwareMeta>() == 4);
};

#[used]
#[unsafe(link_section = ".rodata.fw_meta")]
pub static FW_METADATA: FirmwareMeta = FirmwareMeta {
    ident: *b"KERN",
    version_major: 0,
    version_minor: 1,
    version_patch: 0,
    target_id: 1,
    build_epoch: compile_epoch(),
    git_hash: compile_git_hash(),
    flags: 0,
};

const fn compile_epoch() -> u32 {
    let s = env!("KERN_BUILD_EPOCH");
    let bytes = s.as_bytes();
    let mut val = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        val = val * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    val
}

const fn compile_git_hash() -> [u8; 8] {
    let s = env!("KERN_GIT_HASH");
    let bytes = s.as_bytes();
    let mut out = [0u8; 8];
    let mut i = 0;
    while i < bytes.len() && i < 7 {
        out[i] = bytes[i];
        i += 1;
    }
    out
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_HASH: &str = env!("KERN_GIT_HASH");
pub const GIT_REF: &str = env!("KERN_GIT_REF");
pub const BUILD_DATE: &str = env!("KERN_BUILD_DATE");
pub const BUILD_TIME: &str = env!("KERN_BUILD_TIME");
pub const BUILD_STAMP: &str = env!("KERN_BUILD_STAMP");
pub const ABOUT_TEXT: &str = env!("KERN_ABOUT_TEXT");
