//! Lightweight zero-allocation path validation and root-directory classification.

pub const ROOT_SYS: u8 = 1 << 0;
pub const ROOT_USER: u8 = 1 << 1;
pub const ROOT_PREF: u8 = 1 << 2;
pub const ROOT_APPS: u8 = 1 << 3;

const ROOTS: [(&[u8], u8); 4] = [
    (b"/sys/", ROOT_SYS),
    (b"/user/", ROOT_USER),
    (b"/pref/", ROOT_PREF),
    (b"/apps/", ROOT_APPS),
];

/// Validates that `b` is non-empty, starts with `'/'`, and contains no `".."` or `"//"`.
#[inline(never)]
pub fn is_clean_path(b: &[u8]) -> bool {
    if b.is_empty() || b[0] != b'/' {
        return false;
    }
    let mut i = 1;
    while i < b.len() {
        let prev = b[i - 1];
        let cur = b[i];
        if (prev == b'.' && cur == b'.') || (prev == b'/' && cur == b'/') {
            return false;
        }
        i += 1;
    }
    true
}

/// Returns the root directory bit (`ROOT_*`) matching `b`, or `0` if unrecognized.
#[inline(never)]
pub fn root_bit(b: &[u8]) -> u8 {
    for &(prefix, bit) in &ROOTS {
        if b.starts_with(prefix) {
            return bit;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_path_checks() {
        assert!(is_clean_path(b"/user/test.bin"));
        assert!(!is_clean_path(b""));
        assert!(!is_clean_path(b"user/test.bin"));
        assert!(!is_clean_path(b"/user/../sys/calib.dat"));
        assert!(!is_clean_path(b"/user//test.bin"));
    }

    #[test]
    fn root_bit_checks() {
        assert_eq!(root_bit(b"/sys/calib.dat"), ROOT_SYS);
        assert_eq!(root_bit(b"/user/ch.ch"), ROOT_USER);
        assert_eq!(root_bit(b"/pref/ui.cfg"), ROOT_PREF);
        assert_eq!(root_bit(b"/apps/demo.krx"), ROOT_APPS);
        assert_eq!(root_bit(b"/tmp/foo"), 0);
    }
}
