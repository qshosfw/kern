//! ORDS-01 Power-of-Two Record & Snapshot Formats (`ZoneRecord`, `BandPlanRecord`,
//! `ScanHitRecord`, `FmStationRecord`, `VfoFileHeader`, and `VfoSnapshotFile`).
//!
//! All structures strictly adhere to `AGENTS.md §1.3 & §2`:
//! - Exact power-of-two record sizes ($2^4 = 16\text{ B}$, $2^5 = 32\text{ B}$, $2^6 = 64\text{ B}$)
//!   so records never straddle 4096-byte LittleFS flash erase sectors and seeking is a
//!   single bit-shift (`offset = 16 + (index << N)`).
//! - Naturally aligned `#[repr(C)]` layouts for zero-copy deserialization on Cortex-M0+.

use super::channel_record::ChannelRecord;
use super::file_header::FILE_ID_VFO;

// ── 1. ZoneRecord (32 Bytes, 2^5, `/user/zones.zn`, Header `ZONE`) ──────────

/// 32-byte naturally-aligned Zone / Bank / Scanlist record (`/user/zones.zn`).
///
/// Supports up to 256 zones. Membership for zones `0..16` is indexed in $O(1)$ via
/// `ChannelRecord::zone_mask`, while `pinned_channels` stores up to 5 priority/home
/// channels per zone for instant 1-key recall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ZoneRecord {
    /// 0-based logical zone index (`0..255`).
    pub zone_id: u8,
    /// Number of channels assigned to this zone.
    pub channel_count: u8,
    /// Zone configuration and scan flags (`FLAG_*`).
    pub flags: u16,
    /// Default / home memory channel index when switching into this zone.
    pub home_channel: u16,
    /// Designated priority watch channel index (`0xFFFF` = none).
    pub priority_channel: u16,
    /// Default `ContactRecord` group or topic index bound to this zone (`0` = none).
    pub default_contact: u16,
    /// Up to 5 pinned quick-access channel indices in `/user/channels.ch` (`0xFFFF` = empty).
    pub pinned_channels: [u16; 5],
    /// 12-byte human-readable zone name (e.g. `"LOCAL RPT"`, `"AIRBAND"`, `"EMERGENCY"`).
    pub name: [u8; 12],
}

const _: () = {
    assert!(core::mem::size_of::<ZoneRecord>() == 32);
    assert!(core::mem::align_of::<ZoneRecord>() == 2);
};

impl ZoneRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    pub const FLAG_OCCUPIED: u16 = 1 << 0;
    pub const FLAG_SCAN_INCLUDED: u16 = 1 << 1;
    pub const FLAG_FAVORITE: u16 = 1 << 2;
    pub const FLAG_TX_INHIBIT: u16 = 1 << 3;
    pub const FLAG_LOCKED: u16 = 1 << 4;
    pub const FLAG_DUAL_WATCH: u16 = 1 << 5;

    pub const fn empty() -> Self {
        Self {
            zone_id: 0,
            channel_count: 0,
            flags: 0,
            home_channel: 0,
            priority_channel: 0xFFFF,
            default_contact: 0,
            pinned_channels: [0xFFFF; 5],
            name: [0; 12],
        }
    }

    pub fn new(zone_id: u8, name: &str, home_channel: u16) -> Self {
        let mut rec = Self::empty();
        rec.zone_id = zone_id;
        rec.flags = Self::FLAG_OCCUPIED | Self::FLAG_SCAN_INCLUDED;
        rec.home_channel = home_channel;
        rec.set_name(name);
        rec
    }

    #[inline(always)]
    pub const fn is_occupied(&self) -> bool {
        (self.flags & Self::FLAG_OCCUPIED) != 0
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        utils::format::ascii_str(&self.name[..len]).unwrap_or("")
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 12];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(self.name.len());
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

// ── 2. BandPlanRecord (32 Bytes, 2^5, `/user/bandplans.bnd`, Header `BNDP`) ─

/// 32-byte naturally-aligned Bandplan & Band-Stacking Register record (`/user/bandplans.bnd`).
///
/// Unifies regulatory band boundaries, automatic modulation/step/repeater-offset rules
/// (IJV / F4HWN / EGZUMER parity), hardware PA bias limits, and a persistent `last_freq_hz`
/// **Band-Stacking Register** so cycling bands recalls the last tuned frequency in each band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct BandPlanRecord {
    /// Lower band boundary in Hertz (inclusive).
    pub start_freq_hz: u32,
    /// Upper band boundary in Hertz (inclusive).
    pub end_freq_hz: u32,
    /// Band-stacking register: most recently tuned VFO frequency in this band (Hz).
    pub last_freq_hz: u32,
    /// Standard repeater offset in kHz (e.g. `-600` on 2m, `-7600` / `+5000` on 70cm, `0` simplex).
    pub repeater_offset_khz: i16,
    /// Maximum allowed TX power in milliwatts (`0` = RX-only band).
    pub max_power_mw: u16,
    /// Default tuning raster step in 10 Hz units (e.g. `1250` = 12.5 kHz, `833` = 8.33 kHz Airband).
    pub default_step_x10: u16,
    /// Band policy bitflags (`FLAG_*`).
    pub flags: u16,
    /// Default `ModulationMode` discriminant (`0 = FM`, `1 = AM`, `3 = USB`, `5 = CW`, etc.).
    pub default_modulation: u8,
    /// Default `Bandwidth` discriminant (`0 = Wide25k`, `1 = Narrow12k5`, `2 = Narrow6k25`).
    pub default_bandwidth: u8,
    /// Calibration table band index (`0..7`) in `/sys/calib.dat`.
    pub calib_band_index: u8,
    /// Hardware PA bias ceiling for thermal and harmonic protection (`0..255`).
    pub max_pa_bias: u8,
    /// 8-byte band label (e.g. `"2M HAM"`, `"70CM"`, `"AIRBAND"`, `"PMR446"`, `"MARINE"`).
    pub name: [u8; 8],
}

const _: () = {
    assert!(core::mem::size_of::<BandPlanRecord>() == 32);
    assert!(core::mem::align_of::<BandPlanRecord>() == 4);
};

impl BandPlanRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    pub const FLAG_OCCUPIED: u16 = 1 << 0;
    pub const FLAG_TX_ALLOWED: u16 = 1 << 1;
    pub const FLAG_AUTO_MODULATION: u16 = 1 << 2;
    pub const FLAG_AUTO_STEP: u16 = 1 << 3;
    pub const FLAG_AUTO_OFFSET: u16 = 1 << 4;
    pub const FLAG_UHF_RF_PATH: u16 = 1 << 5;
    pub const FLAG_AMATEUR_BAND: u16 = 1 << 6;
    pub const FLAG_LICENSE_FREE: u16 = 1 << 7;
    pub const FLAG_AIRBAND: u16 = 1 << 8;
    pub const FLAG_MARINE: u16 = 1 << 9;

    pub const fn empty() -> Self {
        Self {
            start_freq_hz: 0,
            end_freq_hz: 0,
            last_freq_hz: 0,
            repeater_offset_khz: 0,
            max_power_mw: 0,
            default_step_x10: 1250,
            flags: 0,
            default_modulation: 0,
            default_bandwidth: 1,
            calib_band_index: 0,
            max_pa_bias: 0xA0,
            name: [0; 8],
        }
    }

    pub fn new(
        start_freq_hz: u32,
        end_freq_hz: u32,
        default_step_x10: u16,
        default_modulation: u8,
        tx_allowed: bool,
        name: &str,
    ) -> Self {
        let mut rec = Self::empty();
        rec.start_freq_hz = start_freq_hz;
        rec.end_freq_hz = end_freq_hz;
        rec.last_freq_hz = start_freq_hz;
        rec.default_step_x10 = default_step_x10;
        rec.default_modulation = default_modulation;
        rec.max_power_mw = if tx_allowed { 5000 } else { 0 };
        rec.flags = Self::FLAG_OCCUPIED
            | Self::FLAG_AUTO_MODULATION
            | Self::FLAG_AUTO_STEP
            | if tx_allowed { Self::FLAG_TX_ALLOWED } else { 0 }
            | if start_freq_hz >= 280_000_000 {
                Self::FLAG_UHF_RF_PATH
            } else {
                0
            };
        rec.set_name(name);
        rec
    }

    #[inline(always)]
    pub const fn contains_freq(&self, freq_hz: u32) -> bool {
        (self.flags & Self::FLAG_OCCUPIED) != 0
            && freq_hz >= self.start_freq_hz
            && freq_hz <= self.end_freq_hz
    }

    #[inline(always)]
    pub const fn is_tx_allowed(&self, freq_hz: u32) -> bool {
        self.contains_freq(freq_hz)
            && (self.flags & Self::FLAG_TX_ALLOWED) != 0
            && self.max_power_mw > 0
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        utils::format::ascii_str(&self.name[..len]).unwrap_or("")
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 8];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(self.name.len());
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

// ── 3. ScanHitRecord (32 Bytes, 2^5, `/user/scanner.hit`, Header `HITS`) ────

/// 32-byte naturally-aligned Scanner & Spectrum Activity Hit record (`/user/scanner.hit`).
///
/// Logs discovered carriers, close-call captures, decoded CTCSS/DCS subaudible tones,
/// peak RSSI, cumulative active duration, nuisance-skip lockout flags, and matched contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ScanHitRecord {
    /// Captured carrier frequency in Hertz.
    pub freq_hz: u32,
    /// Timestamp (uptime or RTC epoch seconds) of most recent activity.
    pub last_seen_sec: u32,
    /// Decoded subaudible CTCSS/DCS word (using `ChannelRecord::encode_subaudible` format).
    pub subaudible: u16,
    /// Number of distinct squelch openings recorded on this frequency.
    pub hit_count: u16,
    /// Cumulative active carrier duration in seconds.
    pub active_duration_sec: u16,
    /// Hit status and filtering flags (`FLAG_*`).
    pub flags: u16,
    /// Peak observed RSSI in dBm (`-128..=0`).
    pub peak_rssi_dbm: i8,
    /// Observed background noise indicator (`0..127`).
    pub noise_floor: u8,
    /// Active `ModulationMode` when captured.
    pub modulation: u8,
    /// Active `Bandwidth` when captured.
    pub bandwidth: u8,
    /// Matched contact index in `/user/contacts.ct` if digital/tone ID was decoded (`0` = none).
    pub matched_contact: u16,
    /// 10-byte auto-band tag or user annotation.
    pub label: [u8; 10],
}

const _: () = {
    assert!(core::mem::size_of::<ScanHitRecord>() == 32);
    assert!(core::mem::align_of::<ScanHitRecord>() == 4);
};

impl ScanHitRecord {
    pub const SIZE: usize = 32;
    pub const STRIDE_SHIFT: u32 = 5; // 1 << 5 == 32 bytes

    pub const FLAG_OCCUPIED: u16 = 1 << 0;
    /// Nuisance-delete / skip list: scanner ignores this frequency during sweeps.
    pub const FLAG_LOCKED_OUT: u16 = 1 << 1;
    pub const FLAG_BOOKMARKED: u16 = 1 << 2;
    pub const FLAG_CLOSE_CALL: u16 = 1 << 3;
    pub const FLAG_FSK_DATA: u16 = 1 << 4;
    pub const FLAG_SAVED_TO_CH: u16 = 1 << 5;

    pub const fn empty() -> Self {
        Self {
            freq_hz: 0,
            last_seen_sec: 0,
            subaudible: 0,
            hit_count: 0,
            active_duration_sec: 0,
            flags: 0,
            peak_rssi_dbm: -128,
            noise_floor: 0,
            modulation: 0,
            bandwidth: 1,
            matched_contact: 0,
            label: [0; 10],
        }
    }

    pub fn new_hit(freq_hz: u32, rssi_dbm: i8, subaudible: u16, timestamp_sec: u32) -> Self {
        let mut rec = Self::empty();
        rec.freq_hz = freq_hz;
        rec.last_seen_sec = timestamp_sec;
        rec.subaudible = subaudible;
        rec.hit_count = 1;
        rec.flags = Self::FLAG_OCCUPIED;
        rec.peak_rssi_dbm = rssi_dbm;
        rec
    }

    /// Update an existing hit record with a new carrier opening observation.
    pub fn record_observation(&mut self, rssi_dbm: i8, subaudible: u16, timestamp_sec: u32) {
        self.hit_count = self.hit_count.saturating_add(1);
        self.last_seen_sec = timestamp_sec;
        if rssi_dbm > self.peak_rssi_dbm {
            self.peak_rssi_dbm = rssi_dbm;
        }
        if subaudible != 0 {
            self.subaudible = subaudible;
        }
    }

    #[inline(always)]
    pub const fn is_locked_out(&self) -> bool {
        (self.flags & Self::FLAG_LOCKED_OUT) != 0
    }

    pub fn label_as_str(&self) -> &str {
        let len = self
            .label
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.label.len());
        utils::format::ascii_str(&self.label[..len]).unwrap_or("")
    }

    pub fn set_label(&mut self, s: &str) {
        self.label = [0; 10];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(self.label.len());
        self.label[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

// ── 4. FmStationRecord (16 Bytes, 2^4, `/user/fm_radio.fm`, Header `FMBC`) ──

/// 16-byte naturally-aligned Broadcast FM Preset & RDS Station record (`/user/fm_radio.fm`).
///
/// Aligned to 16 bytes ($2^4$) so exactly 256 records fit in a 4096-byte flash sector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FmStationRecord {
    /// Broadcast frequency in kilohertz (e.g. `87_500..=108_000` or `64_000..=108_000`).
    pub freq_khz: u32,
    /// RDS Program Identification (PI) 16-bit code (`0` = none).
    pub rds_pi: u16,
    /// Station flags (`FLAG_OCCUPIED`, `FLAG_FAVORITE`, `FLAG_FORCE_MONO`, `FLAG_RDS_VALID`).
    pub flags: u8,
    /// Last observed RSSI in dBm.
    pub last_rssi_dbm: i8,
    /// 8-byte RDS PS (Program Service) or user station name.
    pub name: [u8; 8],
}

const _: () = {
    assert!(core::mem::size_of::<FmStationRecord>() == 16);
    assert!(core::mem::align_of::<FmStationRecord>() == 4);
};

impl FmStationRecord {
    pub const SIZE: usize = 16;
    pub const STRIDE_SHIFT: u32 = 4; // 1 << 4 == 16 bytes

    pub const FLAG_OCCUPIED: u8 = 1 << 0;
    pub const FLAG_FAVORITE: u8 = 1 << 1;
    pub const FLAG_FORCE_MONO: u8 = 1 << 2;
    pub const FLAG_RDS_VALID: u8 = 1 << 3;

    pub const fn empty() -> Self {
        Self {
            freq_khz: 0,
            rds_pi: 0,
            flags: 0,
            last_rssi_dbm: -128,
            name: [0; 8],
        }
    }

    pub fn new(freq_khz: u32, name: &str) -> Self {
        let mut rec = Self::empty();
        rec.freq_khz = freq_khz;
        rec.flags = Self::FLAG_OCCUPIED;
        rec.set_name(name);
        rec
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        utils::format::ascii_str(&self.name[..len]).unwrap_or("")
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 8];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(self.name.len());
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 16] {
        unsafe { &*(self as *const Self as *const [u8; 16]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

// ── 5. VfoSnapshotFile (160 Bytes: 32B Header + 2x 64B ChannelRecord) ───────

/// VFO operational tuning mode stored in `VfoFileHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VfoOperatingMode {
    /// Free frequency synthesis VFO mode.
    FrequencyVfo = 0,
    /// Global memory channel index mode (`/user/channels.ch`).
    MemoryChannel = 1,
    /// Zone-filtered memory channel mode (`/user/zones.zn` + `/user/channels.ch`).
    ZoneChannel = 2,
    /// Active range or spectrum sweep mode.
    RangeScan = 3,
}

/// 32-byte naturally-aligned header for `/user/vfo_a.vfo` and `/user/vfo_b.vfo` (ORDS-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct VfoFileHeader {
    /// Format identifier (`FILE_ID_VFO = "VFO "`).
    pub ident: u32,
    /// Structure revision (`0x0100`).
    pub version: u16,
    /// VFO slot index (`0 = VfoA`, `1 = VfoB`).
    pub vfo_slot: u8,
    /// Operating mode (`VfoOperatingMode`).
    pub operating_mode: u8,
    /// Active memory channel index in `/user/channels.ch` (`0xFFFF` if pure frequency VFO).
    pub memory_index: u16,
    /// Active zone filter (`0..254`, `0xFF` = all zones).
    pub active_zone: u8,
    /// Active channel bank (`0 = primary`, `1 = shadow/split`).
    pub active_bank: u8,
    /// Vernier sub-Hz offset in deci-Hertz (0.1 Hz units, e.g. `+62 = +6.2 Hz`).
    pub fine_offset_dhz: i16,
    /// Receiver Incremental Tuning (RIT / Clarifier) offset in Hz for SSB/CW.
    pub rit_offset_hz: i16,
    /// VFO range scan lower frequency bound in Hz.
    pub scan_lower_hz: u32,
    /// VFO range scan upper frequency bound in Hz.
    pub scan_upper_hz: u32,
    /// Last active bandplan index in `/user/bandplans.bnd`.
    pub bandplan_index: u8,
    /// VFO state bitflags (`FLAG_DUAL_WATCH`, `FLAG_MONITOR`, `FLAG_FREQ_LOCK`, `FLAG_SPLIT_TX`).
    pub flags: u8,
    /// Reserved alignment padding.
    pub reserved: u16,
    /// CRC-32 checksum over bytes `[0..28)` of the header + `[32..160)` of the two channel records.
    pub crc32: u32,
}

const _: () = {
    assert!(core::mem::size_of::<VfoFileHeader>() == 32);
    assert!(core::mem::align_of::<VfoFileHeader>() == 4);
};

/// 160-byte composite VFO persistence payload (`/user/vfo_a.vfo`, `/user/vfo_b.vfo`).
///
/// Combines a 32-byte `VfoFileHeader` with two 64-byte `ChannelRecord` slots:
/// - `primary`: The active tuning configuration (or loaded memory channel copy).
/// - `secondary`: The shadow free-tuning VFO (preserved when entering Memory mode so switching
///   `VFO -> MR -> VFO` never loses the user's frequency) or the Split-TX channel profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct VfoSnapshotFile {
    pub header: VfoFileHeader,
    pub primary: ChannelRecord,
    pub secondary: ChannelRecord,
}

const _: () = {
    assert!(core::mem::size_of::<VfoSnapshotFile>() == 160);
    assert!(core::mem::align_of::<VfoSnapshotFile>() == 4);
};

impl VfoSnapshotFile {
    pub const SIZE: usize = 160;

    pub const fn new(vfo_slot: u8, default_freq_hz: u32) -> Self {
        let mut primary = ChannelRecord::empty();
        primary.rx_frequency_hz = default_freq_hz;
        Self {
            header: VfoFileHeader {
                ident: FILE_ID_VFO,
                version: 0x0100,
                vfo_slot,
                operating_mode: VfoOperatingMode::FrequencyVfo as u8,
                memory_index: 0xFFFF,
                active_zone: 0xFF,
                active_bank: 0,
                fine_offset_dhz: 0,
                rit_offset_hz: 0,
                scan_lower_hz: 144_000_000,
                scan_upper_hz: 146_000_000,
                bandplan_index: 0,
                flags: 0,
                reserved: 0,
                crc32: 0,
            },
            primary,
            secondary: primary,
        }
    }

    /// Compute CRC-32 over the 160-byte snapshot (with `header.crc32` field treated as 0).
    pub fn compute_crc32(&self) -> u32 {
        let bytes = self.as_bytes();
        // Hash header bytes [0..28) and channel records [32..160)
        let mut crc = crypto::crc::CrcEngine::crc32(&bytes[0..28]);
        crc ^= crypto::crc::CrcEngine::crc32(&bytes[32..160]).rotate_left(1);
        crc
    }

    pub fn update_crc(&mut self) {
        self.primary.record_crc = self.primary.compute_crc();
        self.secondary.record_crc = self.secondary.compute_crc();
        self.header.crc32 = self.compute_crc32();
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        self.header.ident == FILE_ID_VFO && self.header.crc32 == self.compute_crc32()
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 160] {
        unsafe { &*(self as *const Self as *const [u8; 160]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 160] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 160]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 160]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ords01_all_records_geometry_and_roundtrip() {
        // 1. ZoneRecord (32B)
        let mut zn = ZoneRecord::new(2, "AIR & RPT", 10);
        zn.priority_channel = 15;
        zn.pinned_channels[0] = 10;
        let zn_dec = ZoneRecord::from_bytes(zn.as_bytes());
        assert_eq!(zn_dec, zn);
        assert_eq!(zn_dec.name_as_str(), "AIR & RPT");

        // 2. BandPlanRecord (32B) with Band-Stacking
        let mut bnd = BandPlanRecord::new(118_000_000, 136_991_660, 833, 1, false, "AIRBAND");
        bnd.last_freq_hz = 121_500_000;
        assert!(bnd.contains_freq(121_500_000));
        assert!(!bnd.is_tx_allowed(121_500_000));
        let bnd_dec = BandPlanRecord::from_bytes(bnd.as_bytes());
        assert_eq!(bnd_dec, bnd);
        assert_eq!(bnd_dec.last_freq_hz, 121_500_000);

        // 3. ScanHitRecord (32B)
        let mut hit = ScanHitRecord::new_hit(433_500_000, -72, 885, 1000);
        hit.record_observation(-64, 885, 1025);
        hit.set_label("70CM CALL");
        assert_eq!(hit.hit_count, 2);
        assert_eq!(hit.peak_rssi_dbm, -64);
        let hit_dec = ScanHitRecord::from_bytes(hit.as_bytes());
        assert_eq!(hit_dec, hit);

        // 4. FmStationRecord (16B)
        let mut fm = FmStationRecord::new(102_500, "RTL 1025");
        fm.rds_pi = 0x5218;
        let fm_dec = FmStationRecord::from_bytes(fm.as_bytes());
        assert_eq!(fm_dec, fm);
        assert_eq!(fm_dec.name_as_str(), "RTL 1025");

        // 5. VfoSnapshotFile (160B)
        let mut vfo = VfoSnapshotFile::new(0, 145_500_000);
        vfo.header.fine_offset_dhz = 62;
        vfo.secondary.rx_frequency_hz = 433_500_000;
        vfo.update_crc();
        assert!(vfo.is_valid());
        let vfo_dec = VfoSnapshotFile::from_bytes(vfo.as_bytes());
        assert_eq!(vfo_dec, vfo);
        assert!(vfo_dec.is_valid());
    }
}
