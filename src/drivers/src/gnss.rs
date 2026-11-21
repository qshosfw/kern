//! Fixed-Point NMEA 0183 GNSS Parser & `ATGM336H-5N31` Configuration Commands.
//!
//! Zero-float `#![no_std]` driver consuming raw serial bytes from any transport
//! (muxed `USART1` via `BL1551B` on UV-K10 Pro, external serial jack, or future companion bridges).

/// Fixed-point GNSS position, velocity, and UTC solution (zero floating point).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(C)]
pub struct GnssFix {
    /// Latitude in degrees × 10^7 (signed: negative = South).
    pub lat_e7: i32,
    /// Longitude in degrees × 10^7 (signed: negative = West).
    pub lon_e7: i32,
    /// Altitude in decimeters (0.1 m) above mean sea level.
    pub alt_dm: i32,
    /// UTC time encoded as `HH * 10000 + MM * 100 + SS`.
    pub utc_hhmmss: u32,
    /// UTC date encoded as `DD * 10000 + MM * 100 + YY`.
    pub date_ddmmyy: u32,
    /// Ground speed in cm/s (converted from knots when parsed from RMC).
    pub speed_cms: u16,
    /// Course over ground in 0.01 degrees (`0..35999`).
    pub course_cdeg: u16,
    /// Satellites in use (`0..=99`).
    pub sats: u8,
    /// NMEA fix quality (`0` = invalid, `1` = GPS/SPS, `2` = DGPS).
    pub fix_quality: u8,
    /// True once RMC `A` (valid) or GGA `fix_quality > 0` has been decoded.
    pub valid: bool,
}

/// Verified CASIC `$PCAS` commands for the `ATGM336H-5N31` GNSS module (`9600` default baud).
pub mod casic {
    /// Switch baud rate to 115200 bps (`$PCAS01,5*19\r\n`).
    pub const BAUD_115200: &[u8] = b"$PCAS01,5*19\r\n";
    /// Set 1 Hz fix rate (`$PCAS02,1000*2E\r\n`).
    pub const RATE_1HZ: &[u8] = b"$PCAS02,1000*2E\r\n";
    /// Set 5 Hz fix rate (`$PCAS02,200*1D\r\n`).
    pub const RATE_5HZ: &[u8] = b"$PCAS02,200*1D\r\n";
    /// Set 10 Hz max fix rate (`$PCAS02,100*1E\r\n`).
    pub const RATE_10HZ: &[u8] = b"$PCAS02,100*1E\r\n";
    /// Minimal GGA+RMC-only output to keep UART bandwidth low (`$PCAS03,1,0,0,0,1,0,0,0*03\r\n`).
    pub const SENTENCES_GGA_RMC: &[u8] = b"$PCAS03,1,0,0,0,1,0,0,0*03\r\n";
    /// OEM K10 Pro sentence mask observed at `0x08019174` (`$PCAS03,1,1,1,0,1,1,1,1*03\r\n`).
    pub const SENTENCES_OEM_K10PRO: &[u8] = b"$PCAS03,1,1,1,0,1,1,1,1*03\r\n";
}

/// Incremental `#![no_std]` fixed-point NMEA 0183 parser (`$G*RMC` & `$G*GGA`).
pub struct NmeaParser {
    line: [u8; 96],
    len: usize,
    in_sentence: bool,
    pub fix: GnssFix,
}

impl NmeaParser {
    pub const fn new() -> Self {
        Self {
            line: [0u8; 96],
            len: 0,
            in_sentence: false,
            fix: GnssFix {
                lat_e7: 0,
                lon_e7: 0,
                alt_dm: 0,
                utc_hhmmss: 0,
                date_ddmmyy: 0,
                speed_cms: 0,
                course_cdeg: 0,
                sats: 0,
                fix_quality: 0,
                valid: false,
            },
        }
    }

    /// Feed one NMEA byte; returns `true` whenever a valid sentence updates `self.fix`.
    pub fn push_byte(&mut self, b: u8) -> bool {
        if b == b'$' {
            self.len = 0;
            self.in_sentence = true;
            return false;
        }
        if !self.in_sentence {
            return false;
        }
        if b == b'\r' || b == b'\n' {
            self.in_sentence = false;
            return self.parse_sentence();
        }
        if self.len < self.line.len() {
            self.line[self.len] = b;
            self.len += 1;
        } else {
            self.in_sentence = false;
        }
        false
    }

    fn parse_sentence(&mut self) -> bool {
        let s = &self.line[..self.len];
        let Some(star) = s.iter().rposition(|&c| c == b'*') else {
            return false;
        };
        if star + 3 > s.len() || !verify_nmea_xor(&s[..star], &s[star + 1..star + 3]) {
            return false;
        }
        let body = &s[..star];
        if body.len() < 6 {
            return false;
        }
        let kind = &body[2..5];
        if kind == b"RMC" {
            Self::parse_rmc(body, &mut self.fix)
        } else if kind == b"GGA" {
            Self::parse_gga(body, &mut self.fix)
        } else {
            false
        }
    }

    fn parse_rmc(body: &[u8], fix: &mut GnssFix) -> bool {
        let mut fields = body.split(|&c| c == b',');
        let _hdr = fields.next();
        let time_s = fields.next().unwrap_or(b"");
        let status = fields.next().unwrap_or(b"");
        let lat_s = fields.next().unwrap_or(b"");
        let ns = fields.next().unwrap_or(b"");
        let lon_s = fields.next().unwrap_or(b"");
        let ew = fields.next().unwrap_or(b"");
        let spd_s = fields.next().unwrap_or(b"");
        let cog_s = fields.next().unwrap_or(b"");
        let date_s = fields.next().unwrap_or(b"");

        if let Some(t) = parse_u32_prefix(time_s, 6) {
            fix.utc_hhmmss = t;
        }
        if let Some(d) = parse_u32_prefix(date_s, 6) {
            fix.date_ddmmyy = d;
        }
        if status != b"A" {
            fix.valid = false;
            return false;
        }
        if let (Some(lat), Some(lon)) = (parse_nmea_coord(lat_s, ns), parse_nmea_coord(lon_s, ew)) {
            fix.lat_e7 = lat;
            fix.lon_e7 = lon;
            fix.valid = true;
            if let Some(knots_x100) = parse_scaled_u32(spd_s, 2) {
                // 1 knot = 51.4444 cm/s
                fix.speed_cms =
                    (knots_x100.saturating_mul(5144) / 10_000).min(u16::MAX as u32) as u16;
            }
            if let Some(cog) = parse_scaled_u32(cog_s, 2) {
                fix.course_cdeg = cog.min(35_999) as u16;
            }
            return true;
        }
        false
    }

    fn parse_gga(body: &[u8], fix: &mut GnssFix) -> bool {
        let mut fields = body.split(|&c| c == b',');
        let _hdr = fields.next();
        let time_s = fields.next().unwrap_or(b"");
        let lat_s = fields.next().unwrap_or(b"");
        let ns = fields.next().unwrap_or(b"");
        let lon_s = fields.next().unwrap_or(b"");
        let ew = fields.next().unwrap_or(b"");
        let qual_s = fields.next().unwrap_or(b"");
        let sats_s = fields.next().unwrap_or(b"");
        let _hdop = fields.next();
        let alt_s = fields.next().unwrap_or(b"");

        if let Some(t) = parse_u32_prefix(time_s, 6) {
            fix.utc_hhmmss = t;
        }
        let qual = qual_s
            .first()
            .copied()
            .map(|c| c.saturating_sub(b'0'))
            .unwrap_or(0);
        fix.fix_quality = qual;
        fix.sats = parse_uint(sats_s).unwrap_or(0).min(99) as u8;
        if qual == 0 {
            fix.valid = false;
            return false;
        }
        if let (Some(lat), Some(lon)) = (parse_nmea_coord(lat_s, ns), parse_nmea_coord(lon_s, ew)) {
            fix.lat_e7 = lat;
            fix.lon_e7 = lon;
            fix.valid = true;
        }
        if let Some(alt) = parse_scaled_i32(alt_s, 1) {
            fix.alt_dm = alt;
        }
        fix.valid
    }
}

impl Default for NmeaParser {
    fn default() -> Self {
        Self::new()
    }
}

fn verify_nmea_xor(body: &[u8], hex2: &[u8]) -> bool {
    let mut sum = 0u8;
    for &b in body {
        sum ^= b;
    }
    let hi = hex_nibble(hex2[0]);
    let lo = hex_nibble(hex2[1]);
    matches!((hi, lo), (Some(h), Some(l)) if ((h << 4) | l) == sum)
}

#[inline(always)]
const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn parse_uint(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v = 0u32;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(v)
}

fn parse_u32_prefix(s: &[u8], digits: usize) -> Option<u32> {
    if s.len() < digits {
        return None;
    }
    parse_uint(&s[..digits])
}

fn parse_scaled_u32(s: &[u8], scale: usize) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut whole = 0u32;
    let mut frac = 0u32;
    let mut frac_digits = 0usize;
    let mut seen_dot = false;
    for &b in s {
        if b == b'.' {
            if seen_dot {
                return None;
            }
            seen_dot = true;
            continue;
        }
        if !b.is_ascii_digit() {
            return None;
        }
        let d = (b - b'0') as u32;
        if !seen_dot {
            whole = whole.checked_mul(10)?.checked_add(d)?;
        } else if frac_digits < scale {
            frac = frac * 10 + d;
            frac_digits += 1;
        }
    }
    while frac_digits < scale {
        frac *= 10;
        frac_digits += 1;
    }
    let mut mult = 1u32;
    for _ in 0..scale {
        mult *= 10;
    }
    whole.checked_mul(mult)?.checked_add(frac)
}

fn parse_scaled_i32(s: &[u8], scale: usize) -> Option<i32> {
    if let Some(rest) = s.strip_prefix(b"-") {
        let u = parse_scaled_u32(rest, scale)?;
        i32::try_from(u).ok()?.checked_neg()
    } else {
        let u = parse_scaled_u32(s, scale)?;
        i32::try_from(u).ok()
    }
}

/// Parse NMEA `ddmm.mmmm` or `dddmm.mmmm` + hemisphere (`N`/`S`/`E`/`W`) into signed `deg * 1e7`.
fn parse_nmea_coord(coord: &[u8], hemi: &[u8]) -> Option<i32> {
    let dot = coord.iter().position(|&c| c == b'.')?;
    if dot < 4 {
        return None;
    }
    let (sign, max_deg) = match hemi {
        b"N" => (1i64, 90i64),
        b"S" => (-1i64, 90i64),
        b"E" => (1i64, 180i64),
        b"W" => (-1i64, 180i64),
        _ => return None,
    };
    let deg_len = dot - 2;
    let deg = parse_uint(&coord[..deg_len])? as i64;
    let min_int = parse_uint(&coord[deg_len..dot])? as i64;
    if min_int >= 60 || deg > max_deg {
        return None;
    }
    let frac_bytes = &coord[dot + 1..];
    let mut frac_e5 = 0i64;
    let mut count = 0usize;
    for &b in frac_bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        if count < 5 {
            frac_e5 = frac_e5 * 10 + (b - b'0') as i64;
            count += 1;
        }
    }
    while count < 5 {
        frac_e5 *= 10;
        count += 1;
    }
    let min_e5 = min_int * 100_000 + frac_e5;
    let deg_e7 = deg * 10_000_000 + (min_e5 * 100) / 60;
    if deg_e7 > max_deg * 10_000_000 {
        return None;
    }
    Some((deg_e7 * sign) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nmea_rmc_and_gga_fixed_point_parsing() {
        let mut parser = NmeaParser::new();
        let rmc = b"$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A\r\n";
        for &b in rmc {
            parser.push_byte(b);
        }
        assert!(parser.fix.valid);
        assert_eq!(parser.fix.utc_hhmmss, 123519);
        assert_eq!(parser.fix.date_ddmmyy, 230394);
        assert_eq!(parser.fix.lat_e7, 481_173_000);
        assert_eq!(parser.fix.lon_e7, 115_166_666);
        assert_eq!(parser.fix.course_cdeg, 8440);
        assert_eq!(parser.fix.speed_cms, 1152);

        let gga = b"$GNGGA,123520,3746.494,N,12225.164,W,1,09,0.9,545.4,M,46.9,M,,*48\r\n";
        let mut updated = false;
        for &b in gga {
            updated |= parser.push_byte(b);
        }
        assert!(updated);
        assert_eq!(parser.fix.fix_quality, 1);
        assert_eq!(parser.fix.sats, 9);
        assert_eq!(parser.fix.alt_dm, 5454);
        assert!(parser.fix.lon_e7 < 0);
    }

    #[test]
    fn rejects_invalid_checksum_and_void_fix() {
        let mut parser = NmeaParser::new();
        let bad_csum = b"$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*00\r\n";
        let mut updated = false;
        for &b in bad_csum {
            updated |= parser.push_byte(b);
        }
        assert!(!updated);
        assert!(!parser.fix.valid);
    }
}
