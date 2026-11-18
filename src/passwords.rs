//! Port of passwords.java (UTF-8 <-> PDFDocEncoding password conversion). Owner: CLI agent.
//!
//! Password character mapping by Acrobat (pre-1.7 PDF):
//!  * characters in PDFDocEncoding use PDFDocEncoding
//!  * unicode 0x200 and up: Acrobat/Reader prohibits input
//!  * some chars below 0x200 are mapped to . (period), some to a non-accented
//!    ASCII char, many to Windows CP-1250
//!
//! The extended map should only be used for mapping passwords for decryption.
#![allow(dead_code)]

/// unicode 0x100 - 0x1FF (Latin Extended-A/B) as mapped by Acrobat on Windows.
static UNICODE_LATIN_EXTENDED_WINDOWS_MAP: [u8; 256] = [
    0x41, 0x61, 0xC3, 0xC4, 0xA5, 0xB9, 0xC6, 0xE6, 0x2E, 0x2E, 0x2E, 0x2E, 0xC8, 0xE8, 0xCF, 0xEF, // U+0100
    0xD0, 0xF0, 0x45, 0x65, 0x2E, 0x2E, 0x45, 0x65, 0xCA, 0xEA, 0xCC, 0xEC, 0x2E, 0x2E, 0x47, 0x67, // U+0110
    0x2E, 0x2E, 0x47, 0x67, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x49, 0x69, 0x2E, 0x2E, 0x49, 0x69, // U+0120
    0x49, 0x69, 0x2E, 0x2E, 0x2E, 0x2E, 0x4B, 0x6B, 0x2E, 0xC5, 0xE5, 0x4C, 0x6C, 0xBC, 0xBE, 0x2E, // U+0130
    0x2E, 0xA3, 0xB3, 0xD1, 0xF1, 0x4E, 0x6E, 0xD2, 0xF2, 0x2E, 0x2E, 0x2E, 0x4F, 0x6F, 0x2E, 0x2E, // U+0140
    0xD5, 0xF5, 0x96, 0x9C, 0xC0, 0xE0, 0x52, 0x72, 0xD8, 0xF8, 0x8C, 0x9C, 0x2E, 0x2E, 0xAA, 0xBA, // U+0150
    0x8A, 0x9A, 0xDE, 0xFE, 0x8D, 0x9D, 0x54, 0x74, 0x2E, 0x2E, 0x55, 0x75, 0x2E, 0x2E, 0xD9, 0xF9, // U+0160
    0xDB, 0xFB, 0x55, 0x75, 0x2E, 0x2E, 0x2E, 0x2E, 0x98, 0x8F, 0x9F, 0xAF, 0xBF, 0x99, 0x9E, 0x2E, // U+0170
    0x62, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0xD0, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, // U+0180
    0x2E, 0x83, 0x83, 0x2E, 0x2E, 0x2E, 0x2E, 0x49, 0x2E, 0x2E, 0x6C, 0x2E, 0x2E, 0x2E, 0x2E, 0x4F, // U+0190
    0x4F, 0x6F, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x74, 0x2E, 0x2E, 0x54, 0x55, // U+01A0
    0x75, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, // U+01B0
    0x7C, 0x2E, 0x2E, 0x21, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, // U+01C0
    0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x41, 0x61, // U+01D0
    0x2E, 0x2E, 0x2E, 0x2E, 0x47, 0x67, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x4F, 0x6F, 0x2E, 0x2E, // U+01E0
    0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, 0x2E, // U+01F0
];

/// Port of passwords.utf8_password_to_pdfdoc().
///
/// Older PDF security passwords use PDFDocEncoding. Returns None if the password
/// contains characters that cannot be mapped. When `encrypt_b`, only the most
/// conservative set of characters is allowed (platform independence); when
/// decrypting, as many input characters as possible are accepted.
pub fn utf8_password_to_pdfdoc(pw: &str, encrypt_b: bool) -> Option<Vec<u8>> {
    let mut bb = Vec::with_capacity(pw.len());
    for ch in pw.chars() {
        let mut data = ch as u32;
        // the Java decoder only understands 1-3 byte sequences
        if data > 0xFFFF {
            return None;
        }
        if (0x20..0x7f).contains(&data) || (0xa0..=0xff).contains(&data) {
            // data point good
        } else {
            // convert some unicode points to PDFDocEncoding
            data = match data {
                0x0152 => 0o226, // OE ligature
                0x0153 => 0o234, // oe ligature
                0x0160 => 0o227, // Scaron
                0x017E => 0o236, // zcaron
                0x0178 => 0o230, // Ydieresis
                0x017D => 0o231, // Zcaron
                0x0192 => 0o206, // florin
                0x0161 => 0o235, // scaron
                _ => {
                    if encrypt_b {
                        return None; // data point out of bounds
                    } else if (0x100..=0x1FF).contains(&data) {
                        // accepted by Acrobat/Reader but mapped to single-byte
                        // alternatives in a platform-dependent way
                        UNICODE_LATIN_EXTENDED_WINDOWS_MAP[(data - 0x100) as usize] as u32
                    } else {
                        match data {
                            0x20AC => 0o240, // Euro
                            0x2022 => 0o200, // bullet
                            0x2020 => 0o201, // dagger
                            0x2021 => 0o202, // daggerdbl
                            0x2026 => 0o203, // ellipsis
                            0x02C6 => 0o032, // circumflex
                            0x2014 => 0o204, // emdash
                            0x2013 => 0o205, // endash
                            0x2039 => 0o210, // guilsinglleft
                            0x203A => 0o211, // guilsinglright
                            0x2030 => 0o213, // perthousand
                            0x201E => 0o214, // quotedblbase
                            0x201C => 0o215, // quotedblleft
                            0x201D => 0o216, // quotedblright
                            0x2018 => 0o217, // quoteleft
                            0x2019 => 0o220, // quoteright
                            0x201A => 0o221, // quotesinglebase
                            0x02DC => 0o037, // tilde
                            0x2122 => 0o222, // trademark
                            _ => return None, // still out of bounds
                        }
                    }
                }
            };
        }
        bb.push(data as u8);
    }
    Some(bb)
}

#[cfg(test)]
mod tests {
    use super::utf8_password_to_pdfdoc as conv;

    #[test]
    fn ascii_and_latin1() {
        assert_eq!(conv("", true), Some(vec![]));
        assert_eq!(conv("foo bar~", true), Some(b"foo bar~".to_vec()));
        assert_eq!(conv("caf\u{e9}", true), Some(vec![b'c', b'a', b'f', 0xe9]));
        assert_eq!(conv("\u{a0}\u{ff}", true), Some(vec![0xa0, 0xff]));
    }

    #[test]
    fn rejects_control_and_gap() {
        assert_eq!(conv("a\tb", true), None);
        assert_eq!(conv("a\tb", false), None);
        assert_eq!(conv("\u{7f}", true), None);
        assert_eq!(conv("\u{80}", false), None);
        assert_eq!(conv("\u{9f}", true), None);
    }

    #[test]
    fn pdfdoc_specials_allowed_when_encrypting() {
        assert_eq!(conv("\u{152}\u{153}\u{160}\u{17e}", true), Some(vec![0o226, 0o234, 0o227, 0o236]));
        assert_eq!(conv("\u{178}\u{17d}\u{192}\u{161}", true), Some(vec![0o230, 0o231, 0o206, 0o235]));
    }

    #[test]
    fn extended_only_when_decrypting() {
        assert_eq!(conv("\u{100}", true), None);
        assert_eq!(conv("\u{100}\u{101}\u{102}\u{141}", false), Some(vec![b'A', b'a', 0xC3, 0xA3]));
        assert_eq!(conv("\u{1ff}", false), Some(vec![b'.']));
        assert_eq!(conv("\u{1c0}", false), Some(vec![b'|']));
        assert_eq!(conv("\u{20ac}", true), None);
        assert_eq!(conv("\u{20ac}\u{2022}\u{2122}\u{2dc}\u{2c6}", false), Some(vec![0o240, 0o200, 0o222, 0o037, 0o032]));
        assert_eq!(conv("\u{200}", false), None);
        assert_eq!(conv("\u{4e2d}", false), None);
        assert_eq!(conv("\u{1f600}", false), None);
    }
}
