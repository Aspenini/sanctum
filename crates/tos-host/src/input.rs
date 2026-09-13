//! TempleOS keyboard character and Set-1 scan-code conversion.

pub const SCF_SHIFT: i64 = 1 << 9;
pub const SCF_CTRL: i64 = 1 << 10;
pub const SCF_ALT: i64 = 1 << 11;

pub fn scan_flags(shift: bool, ctrl: bool, alt: bool) -> i64 {
    (if shift { SCF_SHIFT } else { 0 })
        | (if ctrl { SCF_CTRL } else { 0 })
        | (if alt { SCF_ALT } else { 0 })
}

fn ascii_scan_code(ch: char) -> Option<(i64, bool)> {
    let normal = [
        ("1234567890-=", 0x02),
        ("qwertyuiop[]", 0x10),
        ("asdfghjkl;'`", 0x1e),
        ("zxcvbnm,./", 0x2c),
    ];
    for (row, first) in normal {
        if let Some(index) = row.find(ch.to_ascii_lowercase()) {
            return Some((first + index as i64, ch.is_ascii_uppercase()));
        }
    }
    let shifted = [
        ("!@#$%^&*()_+", 0x02),
        ("{}", 0x1a),
        (":\"~", 0x27),
        ("|", 0x2b),
        ("<>?", 0x33),
    ];
    for (row, first) in shifted {
        if let Some(index) = row.find(ch) {
            return Some((first + index as i64, true));
        }
    }
    (ch == '\\').then_some((0x2b, false))
}

/// Convert one decoded ASCII character to TempleOS character and scan codes.
pub fn ascii_key_event(ch: char, shift: bool, ctrl: bool, alt: bool) -> Option<(i64, i64)> {
    if !ch.is_ascii() {
        return None;
    }
    let (scan, inferred_shift) = if ch == ' ' {
        (0x39, false)
    } else {
        ascii_scan_code(ch)?
    };
    let shift = shift || inferred_shift;
    let ch = if ctrl && ch.is_ascii_alphabetic() {
        i64::from(ch.to_ascii_lowercase() as u8 - b'a' + 1)
    } else if shift && ch == ' ' {
        0x1f
    } else {
        ch as i64
    };
    Some((ch, scan | scan_flags(shift, ctrl, alt)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_templeos_ascii_scan_codes() {
        assert_eq!(
            ascii_key_event('a', false, false, false),
            Some((b'a' as i64, 0x1e))
        );
        assert_eq!(
            ascii_key_event('A', false, false, false),
            Some((b'A' as i64, 0x1e | SCF_SHIFT))
        );
        assert_eq!(
            ascii_key_event('?', false, false, true),
            Some((b'?' as i64, 0x35 | SCF_SHIFT | SCF_ALT))
        );
        assert_eq!(
            ascii_key_event('c', false, true, false),
            Some((3, 0x2e | SCF_CTRL))
        );
        assert_eq!(
            ascii_key_event(' ', true, false, false),
            Some((0x1f, 0x39 | SCF_SHIFT))
        );
        assert_eq!(ascii_key_event('Ω', false, false, false), None);
    }
}
