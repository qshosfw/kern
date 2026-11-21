//! AGC Table Configurations and Dynamic Attenuation.

/// Default 5-entry AGC gain threshold tables for BK4829.
pub const AGC_DEFAULT_TABLE: [u16; 5] = [0x0000, 0x1020, 0x2040, 0x3060, 0x4080];
