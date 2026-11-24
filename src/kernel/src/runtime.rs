//! KRX Executable Runtime Contracts: Binary Image Format, Relocation, Paging, Swap & Faults.
//!
//! Decoupled from process accounting (`crate::proc`) so binary container parsing,
//! demand-paged overlays, Thumb-1 delta relocations, hibernation codecs, and guest
//! fault records live in clean, focused modules.

pub mod fault;
pub mod image;
pub mod paged;
pub mod reloc;
pub mod swap;

#[cfg(test)]
mod tests;

pub use fault::{AppFault, Exception, KRX_FAULT_SENTINEL};
pub use image::{
    ASSET_ICON_8, ASSET_ICON_16, ASSET_ICON_TRAY, AssetDescriptor, BOOT_IDENT, BlockEntry,
    CALIB_IDENT, CFG_KIND_CHOICE, CFG_KIND_RANGE, CFG_KIND_TOGGLE, CapBinding, FLAG_LEAF_PINNED,
    FLAG_LEAF_PREFETCH, FLAG_LEAF_RT, HwClaim, KERNFS_IDENT, KRX_IDENT, KRX_PCB_IDENT,
    KrxActionDecl, KrxAuxSections, KrxClass, KrxConfigHeader, KrxConfigItem, KrxExecType,
    KrxIndexEntry, KrxLaunchError, KrxPreamble, KrxProfile, KrxSectionIterator, KrxSummary,
    LeafContext, LeafContinuation, LeafEntry, SECTION_ACTIONS, SECTION_ASSETS, SECTION_BLOCK_MAP,
    SECTION_CAP_IMPORTS, SECTION_CONFIG, SECTION_EXECUTION, SECTION_IDENTITY, SECTION_LEAF_MAP,
    SECTION_SEAL, SECTION_SERVICE_EXPORTS, SECTION_TYPE_LIFECYCLE, SIG_IDENT, SLAB_IDENT,
    SWAP_IDENT, SectionExecution, SectionHeader, SectionIdentity, SectionType, ServiceExport,
    find_section,
};
pub use paged::{
    BLOCK_FLAG_HINT_NEXT, BLOCK_FLAG_PINNED, BLOCK_FLAG_RT, CacheSlotMeta, FixedWindowCache,
    PagedError, ReturnFrame, ShadowReturnStack, StreamingLeafState, StructuredLeafManager,
};
pub use reloc::{RelocError, apply_relocs};
pub use swap::{KrxSwapHeader, SWAP_FLAG_COMPRESSED_RLE, packbits_compress, packbits_decompress};
