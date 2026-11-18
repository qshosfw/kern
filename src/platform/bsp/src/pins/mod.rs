//! Compile-time board pinout selection.
//!
//! Marketing variants sharing an identical PCB/bootloader contract (such as UV-K1 and UV-K5v3)
//! share a single board profile so firmware images remain unified without needless build splits.

#[cfg(not(feature = "target_k10pro"))]
mod k1_family;
#[cfg(not(feature = "target_k10pro"))]
pub use k1_family::*;

#[cfg(feature = "target_k10pro")]
mod k10_pro;
#[cfg(feature = "target_k10pro")]
pub use k10_pro::*;

/// Compact compile-time hardware capability summary exposed to upper layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardSpec {
    /// Internal OEM/USB reference model code (`"K1"` for K1/K5v3 family, `"K8"` for K10 Pro).
    pub internal_model_id: &'static str,
    /// User-facing USB / product name (`"Quansheng UV-K1"` vs `"Quansheng UV-K10 Pro"`).
    pub usb_product_default: &'static str,
    pub has_flashlight: bool,
    pub has_rgb_backlight: bool,
    pub has_coproc: bool,
    pub has_gnss: bool,
    pub has_extra_key_row: bool,
    pub flash_base: u32,
    pub factory_cert_addr: usize,
}

/// Resolved hardware profile for the active target.
pub const BOARD_SPEC: BoardSpec = BoardSpec {
    internal_model_id: if cfg!(feature = "target_k10pro") {
        "K8"
    } else {
        "K1"
    },
    usb_product_default: if cfg!(feature = "target_k10pro") {
        "Quansheng UV-K10 Pro"
    } else {
        "Quansheng UV-K1"
    },
    has_flashlight: HAS_FLASHLIGHT,
    has_rgb_backlight: HAS_RGB_BACKLIGHT,
    has_coproc: HAS_COPROC,
    has_gnss: HAS_GNSS,
    has_extra_key_row: HAS_EXTRA_KEY_ROW,
    flash_base: py32_hal::flash::FIRMWARE_MIN_ADDR,
    factory_cert_addr: py32_hal::uid::FACTORY_CERT_ADDR,
};
