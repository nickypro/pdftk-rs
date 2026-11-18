//! Port of report.java: dump_data, dump_data_utf8, dump_data_annots (+ PdfInfo/PdfBookmark/
//! PdfPageLabel/PdfPageMedia/PdfData records). dump_data_fields lives in forms.rs. Owner: report agent.
#![allow(dead_code, unused)]
use crate::bookmarks::{self, PdfBookmark};
use crate::pdf;
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::io::Write;

// ---------------------------------------------------------------------------------------------
// record types (PdfInfo.java, PdfPageLabel.java, PdfPageMedia.java, PdfData.java)
// The load*() methods used by the data importer live in data_import.rs.

fn or_null(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("null")
}

/// Port of PdfInfo.java. `None` is Java's null.
#[derive(Debug, Clone, Default)]
pub struct PdfInfo {
    pub m_key: Option<String>,
    pub m_value: Option<String>,
}

impl PdfInfo {
    pub const PREFIX: &'static str = "Info";
    pub const BEGIN_MARK: &'static str = "InfoBegin";
    pub const KEY_LABEL: &'static str = "InfoKey:";
    pub const VALUE_LABEL: &'static str = "InfoValue:";

    pub fn valid(&self) -> bool {
        self.m_key.is_some() && self.m_value.is_some()
    }
}

impl std::fmt::Display for PdfInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\n{} {}\n{} {}\n",
            Self::BEGIN_MARK,
            Self::KEY_LABEL,
            or_null(&self.m_key),
            Self::VALUE_LABEL,
            or_null(&self.m_value)
        )
    }
}

/// Port of PdfPageLabel.NumberingStyle: (representation, /S name).
pub const NUMBERING_STYLES: [(&str, Option<&[u8]>); 7] = [
    ("DecimalArabicNumerals", Some(b"D")),
    ("UppercaseRomanNumerals", Some(b"R")),
    ("LowercaseRomanNumerals", Some(b"r")),
    ("UppercaseLetters", Some(b"A")),
    ("LowercaseLetters", Some(b"a")),
    ("NoNumber", None),
    ("[PDFTK ERROR]", None),
];

/// Port of PdfPageLabel.java.
#[derive(Debug, Clone)]
pub struct PdfPageLabel {
    pub m_new_index: i32,
    pub m_start: i32,
    pub m_prefix: Option<String>,
    pub m_num_style: Option<String>,
}

impl Default for PdfPageLabel {
    fn default() -> Self {
        PdfPageLabel { m_new_index: -1, m_start: -1, m_prefix: None, m_num_style: None }
    }
}

impl PdfPageLabel {
    pub const PREFIX: &'static str = "PageLabel";
    pub const BEGIN_MARK: &'static str = "PageLabelBegin";
    pub const NEW_INDEX_LABEL: &'static str = "PageLabelNewIndex:";
    pub const START_LABEL: &'static str = "PageLabelStart:";
    pub const PREFIX_LABEL: &'static str = "PageLabelPrefix:";
    pub const NUM_STYLE_LABEL: &'static str = "PageLabelNumStyle:";

    /// NumberingStyle.fromString.containsKey()
    pub fn is_style(s: &str) -> bool {
        NUMBERING_STYLES.iter().any(|(r, _)| *r == s)
    }

    pub fn valid(&self) -> bool {
        self.m_new_index > 0
            && self.m_start > 0
            && self.m_num_style.as_deref().map_or(false, Self::is_style)
    }
}

impl std::fmt::Display for PdfPageLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", Self::BEGIN_MARK)?;
        writeln!(f, "{} {}", Self::NEW_INDEX_LABEL, self.m_new_index)?;
        writeln!(f, "{} {}", Self::START_LABEL, self.m_start)?;
        if let Some(p) = &self.m_prefix {
            writeln!(f, "{} {}", Self::PREFIX_LABEL, p)?;
        }
        writeln!(f, "{} {}", Self::NUM_STYLE_LABEL, or_null(&self.m_num_style))
    }
}

/// Port of PdfPageMedia.java. Rectangles are [left, bottom, right, top].
#[derive(Debug, Clone)]
pub struct PdfPageMedia {
    pub m_number: i32,
    pub m_rotation: i32,
    pub m_rect: Option<[f32; 4]>,
    pub m_crop: Option<[f32; 4]>,
}

impl Default for PdfPageMedia {
    fn default() -> Self {
        PdfPageMedia { m_number: -1, m_rotation: -1, m_rect: None, m_crop: None }
    }
}

impl PdfPageMedia {
    pub const PREFIX: &'static str = "PageMedia";
    pub const BEGIN_MARK: &'static str = "PageMediaBegin";
    pub const NUMBER_LABEL: &'static str = "PageMediaNumber:";
    pub const ROTATION_LABEL: &'static str = "PageMediaRotation:";
    pub const RECT_LABEL: &'static str = "PageMediaRect:";
    pub const DIMENSIONS_LABEL: &'static str = "PageMediaDimensions:";
    pub const CROP_LABEL: &'static str = "PageMediaCropRect:";

    pub fn valid(&self) -> bool {
        if self.m_rotation >= 0 && self.m_rotation % 90 != 0 {
            return false;
        }
        self.m_number > 0 && (self.m_rotation >= 0 || self.m_rect.is_some() || self.m_crop.is_some())
    }
}

impl std::fmt::Display for PdfPageMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", Self::BEGIN_MARK)?;
        writeln!(f, "{} {}", Self::NUMBER_LABEL, self.m_number)?;
        if self.m_rotation != -1 {
            writeln!(f, "{} {}", Self::ROTATION_LABEL, self.m_rotation)?;
        }
        if let Some(rect) = &self.m_rect {
            writeln!(f, "{} {}", Self::RECT_LABEL, rect_position(rect))?;
            writeln!(f, "{} {}", Self::DIMENSIONS_LABEL, rect_dimensions(rect))?;
            if let Some(crop) = &self.m_crop {
                if crop != rect {
                    writeln!(f, "{} {}", Self::CROP_LABEL, rect_position(crop))?;
                }
            }
        }
        Ok(())
    }
}

/// Port of PdfData.java.
#[derive(Debug, Clone)]
pub struct PdfData {
    pub m_info: Vec<PdfInfo>,
    pub m_bookmarks: Vec<PdfBookmark>,
    pub m_pagelabels: Vec<PdfPageLabel>,
    pub m_pagemedia: Vec<PdfPageMedia>,
    pub m_num_pages: i32,
    pub m_id_0: Option<String>,
    pub m_id_1: Option<String>,
}

impl Default for PdfData {
    fn default() -> Self {
        PdfData {
            m_info: Vec::new(),
            m_bookmarks: Vec::new(),
            m_pagelabels: Vec::new(),
            m_pagemedia: Vec::new(),
            m_num_pages: -1,
            m_id_0: None,
            m_id_1: None,
        }
    }
}

impl PdfData {
    pub const PREFIX: &'static str = "PdfID";
    pub const ID_0_LABEL: &'static str = "PdfID0:";
    pub const ID_1_LABEL: &'static str = "PdfID1:";
    pub const NUM_PAGES_LABEL: &'static str = "NumberOfPages:";
}

impl std::fmt::Display for PdfData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for vit in &self.m_info {
            write!(f, "{}", vit)?;
        }
        writeln!(f, "{} {}", Self::ID_0_LABEL, or_null(&self.m_id_0))?;
        writeln!(f, "{} {}", Self::ID_1_LABEL, or_null(&self.m_id_1))?;
        writeln!(f, "{} {}", Self::NUM_PAGES_LABEL, self.m_num_pages)?;
        for vit in &self.m_bookmarks {
            write!(f, "{}", vit)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// number formatting

/// Java `NumberFormat.getInstance(Locale.ROOT)` without grouping, applied to a float:
/// at most three fraction digits, HALF_EVEN, no trailing zeros (LoadableRectangle.c_format).
pub fn c_format(v: f32) -> String {
    let d = v as f64;
    if d.is_nan() {
        return "NaN".to_string();
    }
    if d.is_infinite() {
        return if d > 0.0 { "\u{221e}".to_string() } else { "-\u{221e}".to_string() };
    }
    let mut s = format!("{:.3}", d);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// LoadableRectangle.position()
pub fn rect_position(r: &[f32; 4]) -> String {
    format!("{} {} {} {}", c_format(r[0]), c_format(r[1]), c_format(r[2]), c_format(r[3]))
}

/// LoadableRectangle.dimensions()
pub fn rect_dimensions(r: &[f32; 4]) -> String {
    format!("{} {}", c_format(r[2] - r[0]), c_format(r[3] - r[1]))
}

/// 5^n, or None on overflow.
fn pow5_u128(n: i32) -> Option<u128> {
    let mut v: u128 = 1;
    for _ in 0..n {
        v = v.checked_mul(5)?;
    }
    Some(v)
}

/// FloatingDecimal.N_5_BITS: bits needed for 5^n (table of 27), else the 3n estimate.
fn n_5_bits(n: i32) -> i32 {
    if (0..27).contains(&n) {
        if n == 0 {
            0
        } else {
            128 - pow5_u128(n).unwrap_or(1).leading_zeros() as i32
        }
    } else {
        n * 3
    }
}

/// FloatingDecimal.BinaryToASCIIBuffer.roundup()
fn fd_roundup(digits: &mut Vec<u8>, dec_exponent: &mut i32) {
    let mut i = digits.len() - 1;
    let mut q = digits[i];
    if q == b'9' {
        while q == b'9' && i > 0 {
            digits[i] = b'0';
            i -= 1;
            q = digits[i];
        }
        if q == b'9' {
            // carryout! High-order 1, rest 0s, larger exp.
            *dec_exponent += 1;
            digits[0] = b'1';
            return;
        }
    }
    digits[i] = q + 1;
}

/// The digit generation of `sun.misc.FloatingDecimal` as shipped up to JDK 18 (the oracle
/// runs on JDK 17), for a positive finite float: (digits, decExponent) with the value being
/// 0.DIGITS x 10^decExponent. This is not always the shortest round-trip representation
/// (ties go to the even digit, and some values get an extra digit), which is why it is
/// ported rather than approximated. None when the intermediate values need more than 128
/// bits (magnitudes far outside anything a page coordinate can be).
fn java_float_digits(f: f32) -> Option<(Vec<u8>, i32)> {
    const EXP_SHIFT: i32 = 52;
    let f_bits = f.to_bits();
    let mut fract_bits32: u32 = f_bits & 0x007f_ffff;
    let mut bin_exp: i32 = ((f_bits & 0x7f80_0000) >> 23) as i32;
    let n_significant_bits: i32;
    if bin_exp == 0 {
        if fract_bits32 == 0 {
            return None;
        }
        let leading_zeros = fract_bits32.leading_zeros() as i32;
        let shift = leading_zeros - 8;
        fract_bits32 <<= shift;
        bin_exp = 1 - shift;
        n_significant_bits = 32 - leading_zeros;
    } else {
        fract_bits32 |= 0x0080_0000;
        n_significant_bits = 24;
    }
    bin_exp -= 127;
    let mut fract_bits: u64 = (fract_bits32 as u64) << 29;

    // dtoa()
    let tail_zeros = fract_bits.trailing_zeros() as i32;
    let n_fract_bits = EXP_SHIFT + 1 - tail_zeros;
    let n_tiny_bits = (n_fract_bits - bin_exp - 1).max(0);
    if (-21..=62).contains(&bin_exp) && n_tiny_bits == 0 && n_tiny_bits < 27 && n_fract_bits + n_5_bits(n_tiny_bits) < 64 {
        // the easy case: an integer that fits a long
        let insignificant = if bin_exp > n_significant_bits {
            let p2 = bin_exp - n_significant_bits - 1;
            if p2 > 1 && p2 < 64 {
                ((p2 as f64) * std::f64::consts::LOG10_2).floor() as i32
            } else {
                0
            }
        } else {
            0
        };
        let mut lvalue: u64 =
            if bin_exp >= EXP_SHIFT { fract_bits << (bin_exp - EXP_SHIFT) } else { fract_bits >> (EXP_SHIFT - bin_exp) };
        // developLongDigits()
        let mut dec_exponent = 0;
        if insignificant != 0 {
            let pow10 = 10u64.pow(insignificant as u32);
            let residue = lvalue % pow10;
            lvalue /= pow10;
            dec_exponent += insignificant;
            if residue >= (pow10 >> 1) {
                lvalue += 1;
            }
        }
        let mut digits: Vec<u8> = Vec::new();
        let mut c = (lvalue % 10) as u8;
        lvalue /= 10;
        while c == 0 && lvalue != 0 {
            dec_exponent += 1;
            c = (lvalue % 10) as u8;
            lvalue /= 10;
        }
        while lvalue != 0 {
            digits.push(c + b'0');
            dec_exponent += 1;
            c = (lvalue % 10) as u8;
            lvalue /= 10;
        }
        digits.push(c + b'0');
        digits.reverse();
        return Some((digits, dec_exponent + 1));
    }

    // the hard case: estimateDecExp()
    let mut dec_exp: i32 = {
        let d2 = f64::from_bits(0x3ff0_0000_0000_0000 | (fract_bits & 0x000f_ffff_ffff_ffff));
        let d = (d2 - 1.5) * 0.289529654 + 0.176091259 + (bin_exp as f64) * 0.301029995663981;
        d.floor() as i32
    };

    let b5 = (-dec_exp).max(0);
    let mut b2 = b5 + n_tiny_bits + bin_exp;
    let s5 = dec_exp.max(0);
    let mut s2 = s5 + n_tiny_bits;
    let m5 = b5;
    let mut m2 = b2 - n_significant_bits;

    fract_bits >>= tail_zeros;
    b2 -= n_fract_bits - 1;
    let common2factor = b2.min(s2);
    b2 -= common2factor;
    s2 -= common2factor;
    m2 -= common2factor;

    // exact powers of two: the next smaller number is only half as far away
    if n_fract_bits == 1 {
        m2 -= 1;
    }
    if m2 < 0 {
        b2 -= m2;
        s2 -= m2;
        m2 = 0;
    }

    let mut digits: Vec<u8> = Vec::new();
    let low: bool;
    let high: bool;
    let low_digit_difference: i64;

    let b_bits = n_fract_bits + b2 + n_5_bits(b5);
    let ten_s_bits = s2 + 1 + n_5_bits(s5 + 1);
    // the E-form always shows at least one digit after the point
    let force_more = dec_exp < -3 || dec_exp >= 8;

    macro_rules! small_path {
        ($t:ty) => {{
            let p5 = |n: i32| -> $t { pow5_u128(n).unwrap_or(0) as $t };
            let mut b: $t = (fract_bits as $t).wrapping_mul(p5(b5)).wrapping_shl(b2 as u32);
            let s: $t = p5(s5).wrapping_shl(s2 as u32);
            let mut m: $t = p5(m5).wrapping_shl(m2 as u32);
            let tens: $t = s.wrapping_mul(10);
            if s == 0 {
                return None;
            }
            let mut q = b / s;
            b = (b % s).wrapping_mul(10);
            m = m.wrapping_mul(10);
            let mut lo = b < m;
            let mut hi = b.wrapping_add(m) > tens;
            if q == 0 && !hi {
                // oops. Usually ignore leading zero.
                dec_exp -= 1;
            } else {
                digits.push(b'0' + q as u8);
            }
            if force_more {
                hi = false;
                lo = false;
            }
            while !lo && !hi {
                q = b / s;
                b = (b % s).wrapping_mul(10);
                m = m.wrapping_mul(10);
                if m > 0 {
                    lo = b < m;
                    hi = b.wrapping_add(m) > tens;
                } else {
                    // hack -- m might overflow!
                    lo = true;
                    hi = true;
                }
                digits.push(b'0' + q as u8);
            }
            (lo, hi, (b.wrapping_shl(1).wrapping_sub(tens)) as i64)
        }};
    }

    if b_bits < 64 && ten_s_bits < 64 {
        let (lo, hi, ldd) = if b_bits < 32 && ten_s_bits < 32 { small_path!(i32) } else { small_path!(i64) };
        low = lo;
        high = hi;
        low_digit_difference = ldd;
    } else {
        // FDBigInteger arithmetic, done here in 128 bits
        let shl = |v: u128, n: i32| -> Option<u128> {
            if n < 0 || n >= 128 || (n > 0 && v >> (128 - n) != 0) {
                None
            } else {
                Some(v << n)
            }
        };
        let s_val = shl(pow5_u128(s5)?, s2)?;
        let mut b_val = shl((fract_bits as u128).checked_mul(pow5_u128(b5)?)?, b2)?;
        let mut m_val = shl(pow5_u128(m5 + 1)?, m2 + 1)?;
        let ten_s_val = shl(pow5_u128(s5 + 1)?, s2 + 1)?;
        // head room for the multiplications by ten below
        ten_s_val.checked_mul(1000)?;

        let mut q = b_val / s_val;
        b_val = (b_val % s_val).checked_mul(10)?;
        let mut lo = b_val < m_val;
        let mut hi = ten_s_val <= b_val.checked_add(m_val)?;
        if q == 0 && !hi {
            dec_exp -= 1;
        } else {
            digits.push(b'0' + q as u8);
        }
        if force_more {
            hi = false;
            lo = false;
        }
        while !lo && !hi {
            q = b_val / s_val;
            b_val = (b_val % s_val).checked_mul(10)?;
            m_val = m_val.checked_mul(10)?;
            lo = b_val < m_val;
            hi = ten_s_val <= b_val.checked_add(m_val)?;
            digits.push(b'0' + q as u8);
        }
        low_digit_difference = if hi && lo {
            match (b_val.checked_mul(2)?).cmp(&ten_s_val) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        } else {
            0
        };
        low = lo;
        high = hi;
    }

    if digits.is_empty() {
        return None;
    }
    let mut dec_exponent = dec_exp + 1;
    // Last digit gen'd; now Round-up, if necessary.
    if high {
        if low {
            if low_digit_difference == 0 {
                // it's a tie! choose based on which digits we like.
                if digits[digits.len() - 1] & 1 != 0 {
                    fd_roundup(&mut digits, &mut dec_exponent);
                }
            } else if low_digit_difference > 0 {
                fd_roundup(&mut digits, &mut dec_exponent);
            }
        } else {
            fd_roundup(&mut digits, &mut dec_exponent);
        }
    }
    Some((digits, dec_exponent))
}

/// Java `Float.toString(float)` as JDK 17 computes it: plain notation for 1e-3 <= |v| < 1e7
/// (always with a fraction digit), computerized scientific notation otherwise.
pub fn java_float_to_string(v: f32) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0".to_string() } else { "0.0".to_string() };
    }
    let (digits, dec_exponent) = match java_float_digits(v.abs()) {
        Some(r) => r,
        None => {
            // shortest round-trip digits
            let sci = format!("{:e}", v.abs());
            let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
            let exp: i32 = exp.parse().unwrap_or(0);
            (mant.bytes().filter(|c| c.is_ascii_digit()).collect(), exp + 1)
        }
    };
    let digits = String::from_utf8_lossy(&digits).into_owned();
    let n_digits = digits.len() as i32;

    // BinaryToASCIIBuffer.getChars()
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    if dec_exponent > 0 && dec_exponent < 8 {
        // print digits.digits.
        let char_length = n_digits.min(dec_exponent);
        out.push_str(&digits[..char_length as usize]);
        if char_length < dec_exponent {
            out.extend(std::iter::repeat('0').take((dec_exponent - char_length) as usize));
            out.push_str(".0");
        } else {
            out.push('.');
            if char_length < n_digits {
                out.push_str(&digits[char_length as usize..]);
            } else {
                out.push('0');
            }
        }
    } else if dec_exponent <= 0 && dec_exponent > -3 {
        out.push_str("0.");
        out.extend(std::iter::repeat('0').take((-dec_exponent) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if n_digits > 1 {
            out.push_str(&digits[1..]);
        } else {
            out.push('0');
        }
        out.push('E');
        out.push_str(&(dec_exponent - 1).to_string());
    }
    out
}

// ---------------------------------------------------------------------------------------------
// string output

/// Port of report.OutputXmlString(): commons-lang3 `ESCAPE_XML10` plus numeric entities for
/// everything from U+007F up.
pub fn output_xml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let cp = c as u32;
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => {
                if cp <= 0x08 || cp == 0x0b || cp == 0x0c || (0x0e..=0x1f).contains(&cp) || cp == 0xfffe || cp == 0xffff
                {
                    // not representable in XML 1.0: dropped
                } else if cp >= 0x7f {
                    out.push_str("&#");
                    out.push_str(&cp.to_string());
                    out.push(';');
                } else {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Port of report.OutputUtf8String().
pub fn output_utf8_string(s: &str) -> String {
    s.to_string()
}

/// Escape `s` for output per `utf8_b`.
pub fn output_string_to_string(s: &str, utf8_b: bool) -> String {
    if utf8_b {
        output_utf8_string(s)
    } else {
        output_xml_string(s)
    }
}

/// Port of report.OutputXmlString / OutputUtf8String: write `s`, escaping per utf8_b. Shared with forms.rs.
pub fn output_string(out: &mut dyn std::io::Write, s: &str, utf8_b: bool) -> std::io::Result<()> {
    if utf8_b {
        out.write_all(s.as_bytes())
    } else {
        out.write_all(output_xml_string(s).as_bytes())
    }
}

/// Port of report.OutputPdfString(): "" unless `obj` is a string.
pub fn output_pdf_string(obj: Option<&Object>, utf8_b: bool) -> String {
    match obj {
        Some(Object::String(bytes, _)) => {
            output_string_to_string(&pdf::text_string_to_unicode(bytes), utf8_b)
        }
        _ => String::new(),
    }
}

/// A (decoded) PDF name as iText's decodeName() yields it: one char per byte.
fn name_to_string(name: &[u8]) -> String {
    name.iter().map(|b| *b as char).collect()
}

/// Port of report.OutputPdfName(): "" unless `obj` is a name.
pub fn output_pdf_name(obj: Option<&Object>, utf8_b: bool) -> String {
    match obj {
        Some(Object::Name(name)) => output_string_to_string(&name_to_string(name), utf8_b),
        _ => String::new(),
    }
}

/// Port of report.OutputPdfStringOrName(): None unless `obj` is a string or a name.
pub fn output_pdf_string_or_name(obj: Option<&Object>, utf8_b: bool) -> Option<String> {
    match obj {
        Some(Object::String(..)) => Some(output_pdf_string(obj, utf8_b)),
        Some(Object::Name(..)) => Some(output_pdf_name(obj, utf8_b)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// object helpers

/// reader.getPdfObject(): resolve, treating PDF null like "absent".
fn res<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
    let o = pdf::resolve(doc, obj?)?;
    if o.is_null() {
        None
    } else {
        Some(o)
    }
}

fn dget<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    res(doc, dict.get(key).ok())
}

fn is_number(obj: &Object) -> bool {
    matches!(obj, Object::Integer(_) | Object::Real(_))
}

/// PdfNumber.intValue()
fn int_value(obj: &Object) -> i32 {
    match obj {
        Object::Integer(i) => (*i).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        Object::Real(r) => *r as i32,
        _ => 0,
    }
}

/// PdfNumber.floatValue()
fn float_value(obj: &Object) -> f32 {
    match obj {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => 0.0,
    }
}

/// PdfName.encodeName(): the bytes iText keeps for a name (leading slash, #xx escapes).
pub fn itext_name_bytes(name: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(name.len() + 1);
    buf.push(b'/');
    for &c in name {
        match c {
            b' ' | b'%' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'#' => {
                buf.extend_from_slice(format!("#{:x}", c).as_bytes());
            }
            32..=126 => buf.push(c),
            _ => buf.extend_from_slice(format!("#{:02x}", c).as_bytes()),
        }
    }
    buf
}

/// PdfName.hashCode()
pub fn itext_name_hash(name: &[u8]) -> i32 {
    let mut h: i32 = 0;
    for b in itext_name_bytes(name) {
        h = h.wrapping_mul(31).wrapping_add(b as i32);
    }
    h
}

/// java.lang.String.hashCode()
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// Iteration order of a default-constructed `java.util.HashMap` filled by inserting the given
/// hash codes in order (distinct keys, no removals): returns the indices in iteration order.
/// iText keeps dictionaries in such a map, so this is the order pdftk reports Info keys in.
pub fn java_hashmap_order(hashes: &[i32]) -> Vec<usize> {
    let mut cap: usize = 16;
    while hashes.len() as f64 > cap as f64 * 0.75 {
        cap *= 2;
    }
    let mut idx: Vec<usize> = (0..hashes.len()).collect();
    idx.sort_by_key(|&i| {
        let h = hashes[i] as u32;
        ((h ^ (h >> 16)) as usize) & (cap - 1)
    }); // stable: insertion order within a bucket
    idx
}

/// The keys of `dict` in the order iText's PdfDictionary.getKeys() iterates them.
pub fn itext_dict_keys(dict: &Dictionary) -> Vec<&Vec<u8>> {
    let keys: Vec<&Vec<u8>> = dict.iter().map(|(k, _)| k).collect();
    let hashes: Vec<i32> = keys.iter().map(|k| itext_name_hash(k)).collect();
    java_hashmap_order(&hashes).into_iter().map(|i| keys[i]).collect()
}

// ---------------------------------------------------------------------------------------------
// annotations

/// Port of report.ReportAction().
fn report_action(
    ofs: &mut dyn Write,
    doc: &Document,
    action_p: &Dictionary,
    utf8_b: bool,
    prefix: &str,
    depth: usize,
) -> std::io::Result<()> {
    if depth > 100 {
        return Ok(());
    }
    if action_p.has(b"S") {
        // URI action
        if let Some(Object::Name(s_p)) = dget(doc, action_p, b"S") {
            if s_p == b"URI" {
                writeln!(ofs, "{}ActionSubtype: URI", prefix)?;

                // report URI
                if action_p.has(b"URI") {
                    let uri_p = dget(doc, action_p, b"URI");
                    if let Some(Object::String(..)) = uri_p {
                        writeln!(ofs, "{}ActionURI: {}", prefix, output_pdf_string(uri_p, utf8_b))?;
                    }
                }

                // report IsMap
                if action_p.has(b"IsMap") {
                    if let Some(Object::Boolean(true)) = dget(doc, action_p, b"IsMap") {
                        writeln!(ofs, "{}ActionIsMap: true", prefix)?;
                    } else {
                        writeln!(ofs, "{}ActionIsMap: false", prefix)?;
                    }
                }
            }
        }
    }

    // subsequent actions? can be a single action or an array
    if action_p.has(b"Next") {
        match dget(doc, action_p, b"Next") {
            Some(Object::Dictionary(next_p)) => {
                report_action(ofs, doc, next_p, utf8_b, prefix, depth + 1)?;
            }
            Some(Object::Array(actions_p)) => {
                for ii in actions_p {
                    if let Some(Object::Dictionary(next_action_p)) = res(doc, Some(ii)) {
                        report_action(ofs, doc, next_action_p, utf8_b, prefix, depth + 1)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

const LLX: usize = 0;
const LLY: usize = 1;
const URX: usize = 2;
const URY: usize = 3;

/// Port of report.ReportAnnot(): things common to all annots.
fn report_annot(
    ofs: &mut dyn Write,
    doc: &Document,
    page_id: ObjectId,
    annot_p: &Dictionary,
    utf8_b: bool,
) -> std::io::Result<()> {
    // subtype
    let subtype_p = dget(doc, annot_p, b"Subtype");
    if let Some(Object::Name(_)) = subtype_p {
        writeln!(ofs, "AnnotSubtype: {}", output_pdf_name(subtype_p, utf8_b))?;
    }

    // get raw rect from annot
    let mut rect = [0.0f32; 4];
    if let Some(Object::Array(rect_al_p)) = dget(doc, annot_p, b"Rect") {
        if rect_al_p.len() == 4 {
            for ii in 0..4 {
                rect[ii] = match res(doc, Some(&rect_al_p[ii])) {
                    Some(coord_p) if is_number(coord_p) => float_value(coord_p),
                    _ => -1.0, // error value
                };
            }
        }
    }

    // transform rect according to page crop box
    // grab width and height for later xform
    let page_crop_p = pdf::page_crop_box(doc, page_id);
    rect[0] -= page_crop_p[0];
    rect[1] -= page_crop_p[1];
    rect[2] -= page_crop_p[0];
    rect[3] -= page_crop_p[1];
    let page_crop_width = page_crop_p[2] - page_crop_p[0];
    let page_crop_height = page_crop_p[3] - page_crop_p[1];

    // create new rect based on page rotation
    let page_rot = pdf::page_rotation(doc, page_id) % 360;
    let rot_rect: [f32; 4] = match page_rot {
        90 => [rect[LLY], page_crop_width - rect[URX], rect[URY], page_crop_width - rect[LLX]],
        180 => [
            page_crop_width - rect[URX],
            page_crop_height - rect[URY],
            page_crop_width - rect[LLX],
            page_crop_height - rect[LLY],
        ],
        270 => [page_crop_height - rect[URY], rect[LLX], page_crop_height - rect[LLY], rect[URX]],
        _ => rect, // 0 deg
    };

    // output rotated rect
    writeln!(
        ofs,
        "AnnotRect: {} {} {} {}",
        java_float_to_string(rot_rect[0]),
        java_float_to_string(rot_rect[1]),
        java_float_to_string(rot_rect[2]),
        java_float_to_string(rot_rect[3])
    )
}

/// The entries of a page's /Annots that getAnnots() accepts: (object id if indirect, dictionary).
pub fn get_annot_dicts<'a>(
    doc: &'a Document,
    page: ObjectId,
) -> Vec<(Option<ObjectId>, &'a Dictionary)> {
    let mut ret = Vec::new();
    let page_p = match doc.get_dictionary(page) {
        Ok(p) => p,
        Err(_) => return ret,
    };
    let annots_al_p = match dget(doc, page_p, b"Annots") {
        Some(Object::Array(a)) => a,
        _ => return ret,
    };
    for jj in annots_al_p {
        let annot_p = match res(doc, Some(jj)) {
            Some(Object::Dictionary(d)) => d,
            _ => continue,
        };
        match dget(doc, annot_p, b"Type") {
            Some(Object::Name(n)) if n == b"Annot" => {}
            _ => continue,
        }
        let id = match jj {
            Object::Reference(_) => doc.dereference(jj).ok().and_then(|(id, _)| id),
            _ => None,
        };
        ret.push((id, annot_p));
    }
    ret
}

/// Port of report.getAnnots(): resolved annotation dictionaries (object ids) of a page.
/// Only annotations with `/Type /Annot` qualify (as in the Java). Annotation dictionaries
/// written directly inside the /Annots array have no object id and are not returned here;
/// use `get_annot_dicts` to see those too.
pub fn get_annots(doc: &Document, page: lopdf::ObjectId) -> Vec<lopdf::ObjectId> {
    get_annot_dicts(doc, page).into_iter().filter_map(|(id, _)| id).collect()
}

/// Port of report.ReportAnnots().
pub fn report_annots(ofs: &mut dyn std::io::Write, doc: &Document, utf8_b: bool) -> std::io::Result<()> {
    let pages = pdf::page_ids(doc);

    // document page count
    writeln!(ofs, "NumberOfPages: {}", pages.len())?;

    // document base url
    if let Ok(catalog) = doc.catalog() {
        if let Some(Object::Dictionary(uri_p)) = dget(doc, catalog, b"URI") {
            let base_p = dget(doc, uri_p, b"Base");
            if let Some(Object::String(..)) = base_p {
                writeln!(ofs, "PdfUriBase: {}", output_pdf_string(base_p, utf8_b))?;
            }
        }
    }

    // iterate over pages
    for (idx, page_id) in pages.iter().enumerate() {
        let ii = idx + 1;

        // iterate over annotations
        for (_, annot_p) in get_annot_dicts(doc, *page_id) {
            let subtype_p = dget(doc, annot_p, b"Subtype");

            writeln!(ofs, "---")?; // delim
            report_annot(ofs, doc, *page_id, annot_p, utf8_b)?; // base annot items
            writeln!(ofs, "AnnotPageNumber: {}", ii)?;

            // link annotation
            if let Some(Object::Name(n)) = subtype_p {
                if n == b"Link" && annot_p.has(b"A") {
                    // link-specific items: action
                    if let Some(Object::Dictionary(action_p)) = dget(doc, annot_p, b"A") {
                        report_action(ofs, doc, action_p, utf8_b, "Annot", 0)?;
                    }
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// document data

/// Port of report.ReportOutlines().
fn report_outlines(
    ofs: &mut dyn Write,
    doc: &Document,
    outline_p: &Dictionary,
    utf8_b: bool,
) -> std::io::Result<()> {
    let mut bookmark_data = Vec::new();
    bookmarks::read_outlines_from(&mut bookmark_data, doc, outline_p, 0);
    for it in &bookmark_data {
        let escaped = PdfBookmark {
            m_title: output_string_to_string(&it.m_title, utf8_b),
            m_level: it.m_level,
            m_page_num: it.m_page_num,
        };
        write!(ofs, "{}", escaped)?;
    }
    Ok(())
}

/// Port of report.ReportInfo().
fn report_info(ofs: &mut dyn Write, doc: &Document, info_p: &Dictionary, utf8_b: bool) -> std::io::Result<()> {
    // iterate over Info keys
    for key_p in itext_dict_keys(info_p) {
        let value_p = dget(doc, info_p, key_p);

        // don't output empty keys or values
        if let Some(Object::String(bytes, _)) = value_p {
            if !key_p.is_empty() && !pdf::text_string_to_unicode(bytes).is_empty() {
                writeln!(ofs, "{}", PdfInfo::BEGIN_MARK)?;
                writeln!(
                    ofs,
                    "{} {}",
                    PdfInfo::KEY_LABEL,
                    output_string_to_string(&name_to_string(key_p), utf8_b)
                )?;
                writeln!(ofs, "{} {}", PdfInfo::VALUE_LABEL, output_pdf_string(value_p, utf8_b))?;
            }
        }
    }
    Ok(())
}

/// Port of report.ReportPageLabels(): if the number tree node has Nums, report them;
/// else if it has Kids, recurse. Outputs 1-based page numbers, as for bookmarks.
fn report_page_labels<'a>(
    ofs: &mut dyn Write,
    doc: &'a Document,
    numtree_node_p: &'a Dictionary,
    utf8_b: bool,
    mut dangling_p: Option<&mut Option<&'a Object>>,
    depth: usize,
) -> std::io::Result<()> {
    if let Some(Object::Array(nums_p)) = dget(doc, numtree_node_p, b"Nums") {
        // report page numbers

        // This array should be even, but seen odd in the wild.
        // https://gitlab.com/pdftk-java/pdftk/-/issues/144
        // If the array is odd-length, we add the last element to the
        // beginning of the next array.
        let mut labels_p: Vec<&'a Object> = Vec::with_capacity(nums_p.len() + 1);
        if let Some(d) = dangling_p.as_mut() {
            if let Some(o) = d.take() {
                labels_p.push(o);
            }
        }
        labels_p.extend(nums_p.iter());

        let mut labels_ii = labels_p.into_iter();
        while let Some(index_raw) = labels_ii.next() {
            // label index
            let index_p = res(doc, Some(index_raw));
            let label_raw = match labels_ii.next() {
                Some(l) => l,
                None => {
                    // (the Java dies with a NullPointerException here when there is no
                    // enclosing Kids node to carry the dangling entry over to)
                    if let Some(d) = dangling_p.as_mut() {
                        **d = index_p;
                    }
                    return Ok(());
                }
            };

            // label dictionary
            let label_po = res(doc, Some(label_raw));

            match (index_p, label_po) {
                (Some(index_p), Some(Object::Dictionary(label_p))) if is_number(index_p) => {
                    let mut pagelabel = PdfPageLabel::default();

                    // PageLabelNewIndex
                    pagelabel.m_new_index = int_value(index_p).wrapping_add(1);

                    // PageLabelStart
                    pagelabel.m_start = match dget(doc, label_p, b"St") {
                        Some(start_p) if is_number(start_p) => int_value(start_p),
                        _ => 1, // the default
                    };

                    // PageLabelPrefix
                    let prefix_p = dget(doc, label_p, b"P");
                    if let Some(Object::String(..)) = prefix_p {
                        pagelabel.m_prefix = Some(output_pdf_string(prefix_p, utf8_b));
                    }

                    // PageLabelNumStyle
                    let style = match dget(doc, label_p, b"S") {
                        Some(Object::Name(style_p)) => NUMBERING_STYLES
                            .iter()
                            .find(|(_, tag)| tag.map_or(false, |t| t == style_p.as_slice()))
                            .map(|(r, _)| *r)
                            .unwrap_or("[PDFTK ERROR]"),
                        _ => "NoNumber", // default
                    };
                    pagelabel.m_num_style = Some(style.to_string());

                    write!(ofs, "{}", pagelabel)?;
                }
                _ => {
                    // error
                    writeln!(ofs, "[PDFTK ERROR: INVALID label_p IN ReportPageLabelNode]")?;
                }
            }
        }
    } else {
        // try recursing
        if let Some(Object::Array(kids_ar_p)) = dget(doc, numtree_node_p, b"Kids") {
            let mut dangling: Option<&'a Object> = None;
            for kids_ii in kids_ar_p {
                match res(doc, Some(kids_ii)) {
                    Some(Object::Dictionary(kid_p)) if depth < 100 => {
                        // recurse
                        report_page_labels(ofs, doc, kid_p, utf8_b, Some(&mut dangling), depth + 1)?;
                    }
                    _ => {
                        // error
                        writeln!(ofs, "[PDFTK ERROR: INVALID kid_p]")?;
                    }
                }
            }
        } else {
            // error; a number tree must have one or the other
            writeln!(ofs, "[PDFTK ERROR: INVALID PAGE LABEL NUMBER TREE]")?;
        }
    }
    Ok(())
}

/// The page-media record of a page as ReportPageMedia() collects it.
pub fn page_media(doc: &Document, page_id: ObjectId, number: i32) -> PdfPageMedia {
    let has_box = |key: &[u8]| -> bool {
        match pdf::page_attr(doc, page_id, key) {
            Some(o) => matches!(res(doc, Some(&o)), Some(Object::Array(_))),
            None => false,
        }
    };
    PdfPageMedia {
        m_number: number,
        m_rotation: pdf::page_rotation(doc, page_id),
        m_rect: if has_box(b"MediaBox") { Some(pdf::page_media_box(doc, page_id)) } else { None },
        m_crop: if has_box(b"CropBox") { Some(pdf::page_crop_box(doc, page_id)) } else { None },
    }
}

/// Port of report.ReportPageMedia(): page metrics, rotation, stamptkData.
fn report_page_media(ofs: &mut dyn Write, doc: &Document, pages: &[ObjectId], utf8_b: bool) -> std::io::Result<()> {
    for (idx, page_id) in pages.iter().enumerate() {
        let pagemedia = page_media(doc, *page_id, (idx + 1) as i32);
        write!(ofs, "{}", pagemedia)?;

        if let Ok(page_p) = doc.get_dictionary(*page_id) {
            let stamptk_data_p = dget(doc, page_p, b"stamptkData");
            if let Some(Object::String(..)) = stamptk_data_p {
                writeln!(ofs, "PageMediaStamptkData: {}", output_pdf_string(stamptk_data_p, utf8_b))?;
            }
        }
    }
    Ok(())
}

/// Port of report.ReportOnPdf(). `utf8_b` false => non-ASCII emitted as XML numeric entities.
pub fn report_on_pdf(ofs: &mut dyn std::io::Write, doc: &Document, utf8_b: bool) -> std::io::Result<()> {
    {
        // trailer data
        let trailer_p = &doc.trailer;

        // metadata
        match dget(doc, trailer_p, b"Info") {
            Some(Object::Dictionary(info_p)) => report_info(ofs, doc, info_p, utf8_b)?,
            _ => eprintln!("Warning: no info dictionary found"),
        }

        // pdf ID; optional
        if let Some(Object::Array(id_al_p)) = dget(doc, trailer_p, b"ID") {
            for (ii, id_raw) in id_al_p.iter().enumerate() {
                write!(ofs, "PdfID{}: ", ii)?;
                match res(doc, Some(id_raw)) {
                    Some(Object::String(bb, _)) => {
                        for bb_ss in bb {
                            write!(ofs, "{:02x}", bb_ss)?;
                        }
                    }
                    _ => eprintln!("pdftk Error in ReportOnPdf(): invalid pdf id array string;"),
                }
                writeln!(ofs)?;
            }
        }
    }

    let pages = pdf::page_ids(doc);

    match doc.catalog() {
        Ok(catalog_p) => {
            // number of pages
            writeln!(ofs, "NumberOfPages: {}", pages.len())?;

            // outlines; optional
            if let Some(Object::Dictionary(outlines_p)) = dget(doc, catalog_p, b"Outlines") {
                if let Some(Object::Dictionary(top_outline_p)) = dget(doc, outlines_p, b"First") {
                    report_outlines(ofs, doc, top_outline_p, utf8_b)?;
                }
            }
        }
        Err(_) => eprintln!("pdftk Error in ReportOnPdf(): couldn't find catalog;"),
    }

    report_page_media(ofs, doc, &pages, utf8_b)?;

    // page labels (a/k/a logical page numbers)
    match doc.catalog() {
        Ok(catalog_p) => {
            if let Some(Object::Dictionary(pagelabels_p)) = dget(doc, catalog_p, b"PageLabels") {
                report_page_labels(ofs, doc, pagelabels_p, utf8_b, None, 0)?;
            }
        }
        Err(_) => eprintln!("pdftk Error in ReportOnPdf(): couldn't find catalog (2);"),
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    //! Ports of DataTest.java (dump side) and AnnotationsTest.java, plus byte-for-byte
    //! comparisons with the Java pdftk when it is installed.
    use super::*;
    use std::path::Path;
    use std::process::Command;

    pub(crate) const ORACLE: &str = "/usr/bin/pdftk";
    pub(crate) const FILES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files");

    /// Open a PDF the way pdf::add_reader leaves it for the reports.
    pub(crate) fn load(path: &str) -> Document {
        let mut doc = Document::load(path).unwrap_or_else(|e| panic!("load {}: {}", path, e));
        pdf::consolidate_named_destinations(&mut doc);
        doc
    }

    pub(crate) fn fixture(name: &str) -> String {
        format!("{}/{}", FILES, name)
    }

    pub(crate) fn dump(doc: &Document, utf8_b: bool) -> String {
        let mut out = Vec::new();
        report_on_pdf(&mut out, doc, utf8_b).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn annots(doc: &Document) -> String {
        let mut out = Vec::new();
        report_annots(&mut out, doc, false).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn slurp(name: &str) -> String {
        std::fs::read_to_string(fixture(name)).unwrap()
    }

    /// stdout of the Java pdftk, or None if it is missing or failed.
    pub(crate) fn oracle(args: &[&str]) -> Option<Vec<u8>> {
        if !Path::new(ORACLE).exists() {
            return None;
        }
        let out = Command::new(ORACLE).args(args).output().ok()?;
        if out.status.success() {
            Some(out.stdout)
        } else {
            None
        }
    }

    /// The installed oracle is pdftk-java 3.3.2, the source we port is 3.3.3, which stopped
    /// printing page-media numbers with a thousands separator ("1,800" -> "1800"; the old
    /// form could not be read back by update_info). Bring oracle output to the 3.3.3 form.
    pub(crate) fn ungroup(oracle_out: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(oracle_out);
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            if line.starts_with("PageMediaRect:")
                || line.starts_with("PageMediaDimensions:")
                || line.starts_with("PageMediaCropRect:")
            {
                out.push_str(&line.replace(',', ""));
            } else {
                out.push_str(line);
            }
        }
        out.into_bytes()
    }

    pub(crate) fn fixture_pdfs() -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(FILES)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().to_string_lossy().into_owned())
            .filter(|p| p.ends_with(".pdf"))
            .collect();
        v.sort();
        v
    }

    // DataTest.dump_data
    #[test]
    fn dump_data() {
        assert_eq!(slurp("blank.data"), dump(&load(&fixture("blank.pdf")), true));
    }

    // DataTest.dump_data_xml
    #[test]
    fn dump_data_xml() {
        assert_eq!(slurp("blank.xmlesc.data"), dump(&load(&fixture("blank.pdf")), false));
    }

    // DataTest.dump_data_invalid_outlines
    #[test]
    fn dump_data_invalid_outlines() {
        assert_eq!(slurp("issue123.data"), dump(&load(&fixture("issue123.pdf")), true));
    }

    // AnnotationsTest.dump_links
    #[test]
    fn dump_links() {
        assert_eq!(slurp("refs_annots.txt"), annots(&load(&fixture("refs.pdf"))));
    }

    // AnnotationsTest.dump_text
    #[test]
    fn dump_text() {
        assert_eq!(slurp("annotation_annots.txt"), annots(&load(&fixture("annotation.pdf"))));
    }

    /// Every fixture PDF, three reports each, byte for byte against the Java pdftk.
    #[test]
    fn fixtures_match_oracle() {
        let mut compared = 0;
        let mut failures = Vec::new();
        for path in fixture_pdfs() {
            let doc = match Document::load(&path) {
                Ok(mut d) => {
                    pdf::consolidate_named_destinations(&mut d);
                    d
                }
                Err(_) => continue,
            };
            for op in ["dump_data", "dump_data_utf8", "dump_data_annots"] {
                // 3.3.3 reads the numeric page destinations of issue123.pdf, 3.3.2 reports
                // page 0 for them; that file is checked against issue123.data instead
                if path.ends_with("issue123.pdf") && op != "dump_data_annots" {
                    continue;
                }
                let expected = match oracle(&[&path, op]) {
                    Some(e) => ungroup(&e),
                    None => continue,
                };
                let got = match op {
                    "dump_data" => dump(&doc, false),
                    "dump_data_utf8" => dump(&doc, true),
                    _ => annots(&doc),
                };
                compared += 1;
                if got.as_bytes() != expected.as_slice() {
                    failures.push(format!("{} {}", op, path));
                }
            }
        }
        eprintln!("fixtures_match_oracle: {} reports compared", compared);
        assert!(failures.is_empty(), "differ from the oracle: {:#?}", failures);
    }

    #[test]
    fn xml_escaping() {
        assert_eq!(
            output_xml_string("a<b>&\"'\u{1}\t\u{7f}\u{e9}\u{2022}\u{1f600}\u{fffe}z"),
            "a&lt;b&gt;&amp;&quot;&apos;\t&#127;&#233;&#8226;&#128512;z"
        );
        let mut out = Vec::new();
        output_string(&mut out, "caf\u{e9} <", true).unwrap();
        assert_eq!(out, "caf\u{e9} <".as_bytes());
    }

    #[test]
    fn number_formats() {
        assert_eq!(c_format(400.12345), "400.123");
        assert_eq!(c_format(595.276), "595.276");
        assert_eq!(c_format(612.0), "612");
        assert_eq!(c_format(-0.5), "-0.5");
        assert_eq!(c_format(0.0005), "0.001"); // 0.0005f is a hair above 0.0005
        assert_eq!(c_format(0.0004), "0");
        assert_eq!(c_format(1840.78), "1840.78"); // no grouping (3.3.2 printed 1,840.78)
        assert_eq!(c_format(0.0015), "0.002");
        assert_eq!(java_float_to_string(232.454), "232.454");
        assert_eq!(java_float_to_string(411.94), "411.94");
        assert_eq!(java_float_to_string(0.0), "0.0");
        assert_eq!(java_float_to_string(100.0), "100.0");
        assert_eq!(java_float_to_string(-1.0), "-1.0");
        assert_eq!(java_float_to_string(1.0e7), "1.0E7");
        assert_eq!(java_float_to_string(1234567.0), "1234567.0");
        assert_eq!(java_float_to_string(0.001), "0.001");
        assert_eq!(java_float_to_string(0.00012), "1.2E-4");
        // JDK 17 digit generation: a tie goes to the even digit, not to the shortest-nearest
        assert_eq!(java_float_to_string(277.953125), "277.95312");
        assert_eq!(java_float_to_string(289.390625), "289.39062");
        assert_eq!(java_float_to_string(3.0e10), "3.0000001E10");
        assert_eq!(java_float_to_string(8.8553139e14), "8.8553139E14"); // not the shortest (8.855314E14)
        assert_eq!(java_float_to_string(123456789.0), "1.23456792E8");
        assert_eq!(java_float_to_string(33554432.0), "3.3554432E7");
        assert_eq!(java_float_to_string(1.0e-10), "1.0E-10");
        assert_eq!(java_float_to_string(f32::MAX), "3.4028235E38");
        assert_eq!(java_float_to_string(0.1), "0.1");
        assert_eq!(java_float_to_string(-712.1865), "-712.1865");
    }

    #[test]
    fn info_key_order_is_java_hashmap_order() {
        // the order pdftk reports the Info keys of blank.pdf in (file order differs)
        let mut d = Dictionary::new();
        for k in ["Author", "Title", "Subject", "Creator", "Producer", "Keywords", "CreationDate", "ModDate", "Trapped", "PTEX.Fullbanner"] {
            d.set(k, Object::Null);
        }
        let shown = ["Creator", "ModDate", "CreationDate", "Producer", "PTEX.Fullbanner"];
        let keys: Vec<String> = itext_dict_keys(&d)
            .into_iter()
            .map(|k| String::from_utf8_lossy(k).into_owned())
            .filter(|k| shown.contains(&k.as_str()))
            .collect();
        assert_eq!(keys, shown);
        assert_eq!(java_string_hash("InfoKey"), "InfoKey".chars().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)));
    }
}
