//! The slice of iText's BaseFont machinery that form-field appearances need:
//! - the 14 built-in Type1 fonts with WinAnsi encoding (metrics from the bundled AFM files),
//! - DocumentFont: a font that already lives in the PDF (simple Type1/TrueType fonts with
//!   /Encoding + /Differences + /Widths, and Identity-H Type0 fonts with a /ToUnicode CMap).
//!
//! Not ported: CJK "mirror" fonts (DocumentFont.cjkMirror, the CJK stdFieldFontNames), embedding
//! of TrueType/CFF fonts (AcroFields extensionFonts / replacementFont / substitutionFonts).
use super::syntax::{Lexer, Token};
use super::util::*;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;
use std::sync::OnceLock;

macro_rules! afm {
    ($name:literal) => {
        ($name, include_str!(concat!("../../resources/afm/", $name, ".afm")))
    };
}

static AFM_FILES: [(&str, &str); 14] = [
    afm!("Courier"),
    afm!("Courier-Bold"),
    afm!("Courier-BoldOblique"),
    afm!("Courier-Oblique"),
    afm!("Helvetica"),
    afm!("Helvetica-Bold"),
    afm!("Helvetica-BoldOblique"),
    afm!("Helvetica-Oblique"),
    afm!("Symbol"),
    afm!("Times-Roman"),
    afm!("Times-Bold"),
    afm!("Times-BoldItalic"),
    afm!("Times-Italic"),
    afm!("ZapfDingbats"),
];

/// PdfEncodings.winansiByteToChar[128..160]
const WINANSI_HI: [u16; 32] = [
    8364, 65533, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 65533, 381, 65533, 65533, 8216, 8217,
    8220, 8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 65533, 382, 376,
];

/// Java's "MacRoman" charset, bytes 128..256.
const MACROMAN_HI: [u16; 128] = [
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1, 0x00E0, 0x00E2, 0x00E4, 0x00E3, 0x00E5, 0x00E7,
    0x00E9, 0x00E8, 0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3, 0x00F2, 0x00F4, 0x00F6, 0x00F5,
    0x00FA, 0x00F9, 0x00FB, 0x00FC, 0x2020, 0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF, 0x00AE, 0x00A9,
    0x2122, 0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8, 0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202, 0x2211,
    0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8, 0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248,
    0x2206, 0x00AB, 0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153, 0x2013, 0x2014, 0x201C, 0x201D,
    0x2018, 0x2019, 0x00F7, 0x25CA, 0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02, 0x2021, 0x00B7,
    0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1, 0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF, 0x00CC, 0x00D3, 0x00D4,
    0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC, 0x00AF, 0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD,
    0x02DB, 0x02C7,
];

/// DocumentFont.stdEnc
const STD_ENC: [u16; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32, 33, 34, 35, 36,
    37, 38, 8217, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
    65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93,
    94, 95, 8216, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117,
    118, 119, 120, 121, 122, 123, 124, 125, 126, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 161, 162, 163, 8260, 165, 402, 167, 164, 39, 8220, 171, 8249, 8250, 64257, 64258,
    0, 8211, 8224, 8225, 183, 0, 182, 8226, 8218, 8222, 8221, 187, 8230, 8240, 0, 191, 0, 96, 180, 710, 732, 175, 728,
    729, 168, 0, 730, 184, 0, 733, 731, 711, 8212, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 198, 0, 170, 0, 0,
    0, 0, 321, 216, 338, 186, 0, 0, 0, 0, 0, 230, 0, 0, 0, 305, 0, 0, 322, 248, 339, 223, 0, 0, 0, 0,
];

fn winansi_byte_to_char(k: usize) -> u16 {
    if (128..160).contains(&k) {
        WINANSI_HI[k - 128]
    } else {
        k as u16
    }
}

/// PdfEncodings.winansi (char -> byte for the 128..160 block)
fn winansi_get(c: u32) -> u8 {
    for k in 128..161usize {
        let ch = winansi_byte_to_char(k);
        if ch != 65533 && ch as u32 == c {
            return k as u8;
        }
    }
    0
}

/// iText GlyphList, parsed out of the Java source (two parallel arrays).
struct GlyphList {
    unicode2name: HashMap<u32, String>,
    name2unicode: HashMap<String, Vec<u32>>,
}

fn glyph_list() -> &'static GlyphList {
    static GL: OnceLock<GlyphList> = OnceLock::new();
    GL.get_or_init(|| {
        let src = include_str!("../../resources/GlyphList.java");
        let mut unicode: Vec<u32> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut section = 0;
        for line in src.lines() {
            let t = line.trim();
            if t.contains("unicode[] =") {
                section = 1;
            } else if t.contains("names[] =") {
                section = 2;
            } else if t.starts_with("};") {
                section = 0;
            } else if section == 1 {
                if let Some(hex) = t.trim_end_matches(',').strip_prefix("0x") {
                    if let Ok(v) = u32::from_str_radix(hex, 16) {
                        unicode.push(v);
                    }
                }
            } else if section == 2 {
                if let Some(rest) = t.strip_prefix('"') {
                    if let Some(end) = rest.find('"') {
                        names.push(rest[..end].to_string());
                    }
                }
            }
        }
        let mut gl = GlyphList { unicode2name: HashMap::new(), name2unicode: HashMap::new() };
        for (u, n) in unicode.iter().zip(names.iter()) {
            gl.unicode2name.insert(*u, n.clone());
            gl.name2unicode.entry(n.clone()).or_default().push(*u);
        }
        gl
    })
}

fn unicode_to_name(c: u32) -> Option<&'static str> {
    glyph_list().unicode2name.get(&c).map(|s| s.as_str())
}

fn name_to_unicode(name: &str) -> Option<&'static Vec<u32>> {
    glyph_list().name2unicode.get(name)
}

/// A parsed AFM file (Type1Font.process()).
struct Afm {
    llx: i32,
    lly: i32,
    urx: i32,
    ury: i32,
    cap_height: i32,
    ascender: i32,
    descender: i32,
    italic_angle: f32,
    font_specific: bool,
    by_code: HashMap<i32, i32>,
    by_name: HashMap<String, i32>,
}

fn parse_afm(src: &str) -> Afm {
    let mut afm = Afm {
        llx: -50,
        lly: -200,
        urx: 1000,
        ury: 900,
        cap_height: 700,
        ascender: 800,
        descender: -200,
        italic_angle: 0.0,
        font_specific: true,
        by_code: HashMap::new(),
        by_name: HashMap::new(),
    };
    let mut lines = src.lines();
    let num = |t: Option<&str>| -> i32 { t.and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0) as i32 };
    for line in lines.by_ref() {
        let mut tok = line.split(|c: char| " ,\n\r\t\x0c".contains(c)).filter(|s| !s.is_empty());
        let ident = match tok.next() {
            Some(i) => i,
            None => continue,
        };
        match ident {
            "ItalicAngle" => afm.italic_angle = tok.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            "FontBBox" => {
                afm.llx = num(tok.next());
                afm.lly = num(tok.next());
                afm.urx = num(tok.next());
                afm.ury = num(tok.next());
            }
            "EncodingScheme" => {
                let scheme = line.trim_start()["EncodingScheme".len()..].trim();
                afm.font_specific = !(scheme == "AdobeStandardEncoding" || scheme == "StandardEncoding");
            }
            "CapHeight" => afm.cap_height = num(tok.next()),
            "Ascender" => afm.ascender = num(tok.next()),
            "Descender" => afm.descender = num(tok.next()),
            "StartCharMetrics" => break,
            _ => {}
        }
    }
    for line in lines {
        let ident = match line.split_whitespace().next() {
            Some(i) => i,
            None => continue,
        };
        if ident == "EndCharMetrics" {
            break;
        }
        let mut c = -1;
        let mut wx = 250;
        let mut n = String::new();
        for part in line.split(';') {
            let mut t = part.split_whitespace();
            match t.next() {
                Some("C") => c = t.next().and_then(|s| s.parse().ok()).unwrap_or(-1),
                Some("WX") => wx = num(t.next()),
                Some("N") => n = t.next().unwrap_or("").to_string(),
                _ => {}
            }
        }
        if c >= 0 {
            afm.by_code.insert(c, wx);
        }
        afm.by_name.insert(n, wx);
    }
    if !afm.by_name.contains_key("nonbreakingspace") {
        if let Some(w) = afm.by_name.get("space").copied() {
            afm.by_name.insert("nonbreakingspace".into(), w);
        }
    }
    afm
}

fn afm(name: &str) -> Option<&'static Afm> {
    static AFMS: OnceLock<HashMap<&'static str, Afm>> = OnceLock::new();
    AFMS.get_or_init(|| AFM_FILES.iter().map(|(n, src)| (*n, parse_afm(src))).collect()).get(name)
}

impl Afm {
    /// Type1Font.getRawWidth()
    fn raw_width(&self, c: i32, name: Option<&str>) -> i32 {
        match name {
            None => self.by_code.get(&c).copied().unwrap_or(0),
            Some(".notdef") => 0,
            Some(n) => self.by_name.get(n).copied().unwrap_or(0),
        }
    }
}

pub fn is_builtin_font(name: &str) -> bool {
    AFM_FILES.iter().any(|(n, _)| *n == name)
}

/// AcroFields.stdFieldFontNames: /DA resource name -> built-in font (the CJK entries are not ported).
pub fn std_field_font(da_name: &str) -> Option<&'static str> {
    Some(match da_name {
        "CoBO" => "Courier-BoldOblique",
        "CoBo" => "Courier-Bold",
        "CoOb" => "Courier-Oblique",
        "Cour" => "Courier",
        "HeBO" => "Helvetica-BoldOblique",
        "HeBo" => "Helvetica-Bold",
        "HeOb" => "Helvetica-Oblique",
        "Helv" => "Helvetica",
        "Symb" => "Symbol",
        "TiBI" => "Times-BoldItalic",
        "TiBo" => "Times-Bold",
        "TiIt" => "Times-Italic",
        "TiRo" => "Times-Roman",
        "ZaDb" => "ZapfDingbats",
        _ => return None,
    })
}

/// PdfAppearance.stdFieldFontNames: PostScript name -> resource name used in appearance streams.
pub fn std_resource_name(ps_name: &str) -> Option<&'static str> {
    Some(match ps_name {
        "Courier-BoldOblique" => "CoBO",
        "Courier-Bold" => "CoBo",
        "Courier-Oblique" => "CoOb",
        "Courier" => "Cour",
        "Helvetica-BoldOblique" => "HeBO",
        "Helvetica-Bold" => "HeBo",
        "Helvetica-Oblique" => "HeOb",
        "Helvetica" => "Helv",
        "Symbol" => "Symb",
        "Times-BoldItalic" => "TiBI",
        "Times-Bold" => "TiBo",
        "Times-Italic" => "TiIt",
        "Times-Roman" => "TiRo",
        "ZapfDingbats" => "ZaDb",
        "HYSMyeongJo-Medium" | "HYSMyeongJoStd-Medium" => "HySm",
        "HYGoThic-Medium" => "HyGo",
        "HeiseiKakuGo-W5" => "KaGo",
        "HeiseiMin-W3" | "KozMinPro-Regular" => "KaMi",
        "MHei-Medium" => "MHei",
        "MSung-Light" | "MSungStd-Light" => "MSun",
        "STSong-Light" | "STSongStd-Light" => "STSo",
        _ => return None,
    })
}

#[derive(Debug, Clone)]
enum Kind {
    /// built-in Type1 font, WinAnsi ("fastWinansi")
    BuiltinWinAnsi,
    /// built-in Symbol / ZapfDingbats (font-specific encoding)
    BuiltinSpecific,
    /// DocumentFont, simple
    DocSimple { uni2byte: HashMap<u32, u8> },
    /// DocumentFont, Identity-H with ToUnicode: unicode -> (cid, width)
    DocType0 { metrics: HashMap<u32, (u32, i32)> },
}

#[derive(Debug, Clone)]
pub struct Font {
    kind: Kind,
    /// BaseFont.getPostscriptFontName()
    pub ps_name: String,
    /// the font object in the document (DocumentFont), None for a built-in font
    pub doc_ref: Option<ObjectId>,
    widths: Vec<i32>,
    ascender: f32,
    cap_height: f32,
    descender: f32,
    llx: f32,
    lly: f32,
    urx: f32,
    ury: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Desc {
    Ascent,
    Descent,
    BboxLlx,
    BboxLly,
    BboxUrx,
    BboxUry,
}

impl Font {
    /// BaseFont.createFont(name, WINANSI, false) for one of the 14 built-in fonts.
    pub fn builtin(name: &str) -> Option<Font> {
        let afm = afm(name)?;
        let mut widths = vec![0i32; 256];
        if afm.font_specific {
            for (k, w) in widths.iter_mut().enumerate() {
                *w = afm.raw_width(k as i32, None);
            }
        } else {
            for (k, w) in widths.iter_mut().enumerate() {
                let c = winansi_byte_to_char(k) as u32;
                let name = unicode_to_name(c).unwrap_or(".notdef");
                *w = afm.raw_width(c as i32, Some(name));
            }
        }
        Some(Font {
            kind: if afm.font_specific { Kind::BuiltinSpecific } else { Kind::BuiltinWinAnsi },
            ps_name: name.to_string(),
            doc_ref: None,
            widths,
            ascender: afm.ascender as f32,
            cap_height: afm.cap_height as f32,
            descender: afm.descender as f32,
            llx: afm.llx as f32,
            lly: afm.lly as f32,
            urx: afm.urx as f32,
            ury: afm.ury as f32,
        })
    }

    /// new DocumentFont(ref)
    pub fn document(doc: &Document, id: ObjectId) -> Font {
        let mut f = Font {
            kind: Kind::DocSimple { uni2byte: HashMap::new() },
            ps_name: String::new(),
            doc_ref: Some(id),
            widths: vec![0i32; 256],
            ascender: 800.0,
            cap_height: 700.0,
            descender: -200.0,
            llx: -50.0,
            lly: -200.0,
            urx: 100.0,
            ury: 900.0,
        };
        let font = match dict_of(doc, id) {
            Some(d) => d,
            None => return f,
        };
        f.ps_name = match dget(doc, font, b"BaseFont") {
            Some(Object::Name(n)) => name_to_string(n),
            Some(Object::String(s, _)) => text(s),
            _ => String::new(),
        };
        let subtype = get_as_name(doc, font, b"Subtype");
        if subtype == Some(b"Type1") || subtype == Some(b"TrueType") {
            f.do_type1_tt(doc, font);
        } else {
            let enc = get_as_name(doc, font, b"Encoding").map(name_to_string).unwrap_or_default();
            if subtype == Some(b"Type0") && enc == "Identity-H" {
                f.process_type0(doc, font);
            }
            // (CJK mirror fonts are not ported: such a font encodes nothing)
        }
        f
    }

    fn do_type1_tt(&mut self, doc: &Document, font: &Dictionary) {
        let mut uni2byte: HashMap<u32, u8> = HashMap::new();
        let mut diffmap: Vec<(u32, u8)> = Vec::new();
        let fill_encoding = |m: &mut HashMap<u32, u8>, enc: Option<&[u8]>| match enc {
            Some(b"WinAnsiEncoding") => {
                for k in 0..256usize {
                    m.insert(winansi_byte_to_char(k) as u32, k as u8);
                }
            }
            Some(b"MacRomanEncoding") => {
                for k in 0..256usize {
                    let c = if k < 128 { k as u16 } else { MACROMAN_HI[k - 128] };
                    m.insert(c as u32, k as u8);
                }
            }
            _ => {
                for k in 0..256usize {
                    m.insert(STD_ENC[k] as u32, k as u8);
                }
            }
        };
        match dget(doc, font, b"Encoding") {
            None => fill_encoding(&mut uni2byte, None),
            Some(Object::Name(n)) => fill_encoding(&mut uni2byte, Some(n)),
            Some(Object::Dictionary(enc_dic)) => {
                fill_encoding(&mut uni2byte, get_as_name(doc, enc_dic, b"BaseEncoding"));
                if let Some(diffs) = get_as_array(doc, enc_dic, b"Differences") {
                    let mut current: i32 = 0;
                    for d in diffs {
                        match res(doc, Some(d)) {
                            Some(o) if is_number(o) => current = int_value(o),
                            Some(Object::Name(n)) => {
                                if let Some(c) = name_to_unicode(&name_to_string(n)) {
                                    if !c.is_empty() {
                                        uni2byte.insert(c[0], current as u8);
                                        diffmap.push((c[0], current as u8));
                                    }
                                }
                                current += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => fill_encoding(&mut uni2byte, None),
        }
        if is_builtin_font(&self.ps_name) {
            let bf = afm(&self.ps_name).unwrap();
            let mut keys: Vec<u32> = uni2byte.keys().copied().collect();
            keys.sort();
            for e in keys {
                let n = uni2byte[&e] as usize;
                self.widths[n] = bf.raw_width(n as i32, unicode_to_name(e));
            }
            diffmap.sort();
            let last: HashMap<u32, u8> = diffmap.iter().copied().collect();
            let mut dkeys: Vec<u32> = last.keys().copied().collect();
            dkeys.sort();
            for e in dkeys {
                let n = last[&e] as usize;
                self.widths[n] = bf.raw_width(n as i32, unicode_to_name(e));
            }
            self.ascender = bf.ascender as f32;
            self.cap_height = bf.cap_height as f32;
            self.descender = bf.descender as f32;
            self.llx = bf.llx as f32;
            self.lly = bf.lly as f32;
            self.urx = bf.urx as f32;
            self.ury = bf.ury as f32;
        }
        let first = get_as_int(doc, font, b"FirstChar");
        let last = get_as_int(doc, font, b"LastChar");
        if let (Some(f), Some(_), Some(new_widths)) = (first, last, get_as_array(doc, font, b"Widths")) {
            for (k, w) in new_widths.iter().enumerate() {
                let i = f as i64 + k as i64;
                if (0..256).contains(&i) {
                    self.widths[i as usize] = res(doc, Some(w)).map(int_value).unwrap_or(0);
                }
            }
        }
        self.fill_font_desc(doc, get_as_dict(doc, font, b"FontDescriptor"));
        self.kind = Kind::DocSimple { uni2byte };
    }

    fn fill_font_desc(&mut self, doc: &Document, desc: Option<&Dictionary>) {
        let d = match desc {
            Some(d) => d,
            None => return,
        };
        if let Some(v) = get_as_float(doc, d, b"Ascent") {
            self.ascender = v;
        }
        if let Some(v) = get_as_float(doc, d, b"CapHeight") {
            self.cap_height = v;
        }
        if let Some(v) = get_as_float(doc, d, b"Descent") {
            self.descender = v;
        }
        if let Some(bbox) = get_as_array(doc, d, b"FontBBox") {
            if bbox.len() >= 4 {
                let g = |i: usize| res(doc, Some(&bbox[i])).map(float_value).unwrap_or(0.0);
                self.llx = g(0);
                self.lly = g(1);
                self.urx = g(2);
                self.ury = g(3);
                if self.llx > self.urx {
                    std::mem::swap(&mut self.llx, &mut self.urx);
                }
                if self.lly > self.ury {
                    std::mem::swap(&mut self.lly, &mut self.ury);
                }
            }
        }
    }

    fn process_type0(&mut self, doc: &Document, font: &Dictionary) {
        let mut metrics: HashMap<u32, (u32, i32)> = HashMap::new();
        let cidft = get_as_array(doc, font, b"DescendantFonts")
            .and_then(|a| a.first())
            .and_then(|o| match res(doc, Some(o)) {
                Some(Object::Dictionary(d)) => Some(d),
                _ => None,
            });
        if let Some(cidft) = cidft {
            let dw = get_as_int(doc, cidft, b"DW").unwrap_or(1000);
            // readWidths()
            let mut widths: HashMap<u32, i32> = HashMap::new();
            if let Some(ws) = get_as_array(doc, cidft, b"W") {
                let mut k = 0;
                while k + 1 < ws.len() {
                    let mut c1 = res(doc, Some(&ws[k])).map(int_value).unwrap_or(0);
                    k += 1;
                    match res(doc, Some(&ws[k])) {
                        Some(Object::Array(a2)) => {
                            for w in a2 {
                                widths.insert(c1 as u32, res(doc, Some(w)).map(int_value).unwrap_or(0));
                                c1 += 1;
                            }
                        }
                        Some(o) => {
                            let c2 = int_value(o);
                            k += 1;
                            let w = ws.get(k).and_then(|x| res(doc, Some(x))).map(int_value).unwrap_or(0);
                            if c2 - c1 < 70000 {
                                while c1 <= c2 {
                                    widths.insert(c1 as u32, w);
                                    c1 += 1;
                                }
                            }
                        }
                        None => {}
                    }
                    k += 1;
                }
            }
            self.fill_font_desc(doc, get_as_dict(doc, cidft, b"FontDescriptor"));
            if let Some(Object::Stream(s)) = dget(doc, font, b"ToUnicode") {
                let bytes = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
                fill_metrics(&bytes, &widths, dw, &mut metrics);
            }
        }
        self.kind = Kind::DocType0 { metrics };
    }

    /// BaseFont.getFontDescriptor(key, fontSize)
    pub fn descriptor(&self, key: Desc, font_size: f32) -> f32 {
        let v = match key {
            Desc::Ascent => self.ascender,
            Desc::Descent => self.descender,
            Desc::BboxLlx => self.llx,
            Desc::BboxLly => self.lly,
            Desc::BboxUrx => self.urx,
            Desc::BboxUry => self.ury,
        };
        v * font_size / 1000.0
    }

    /// BaseFont.convertToBytes(String); `units` are UTF-16 code units like a Java String.
    pub fn convert_to_bytes(&self, units: &[u16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(units.len());
        match &self.kind {
            Kind::BuiltinWinAnsi | Kind::BuiltinSpecific => {
                for &u in units {
                    let c = u as u32;
                    let b = if c < 128 || (c > 160 && c <= 255) { c as u8 } else { winansi_get(c) };
                    if b != 0 {
                        out.push(b);
                    }
                }
            }
            Kind::DocSimple { uni2byte } => {
                for &u in units {
                    if let Some(b) = uni2byte.get(&(u as u32)) {
                        out.push(*b);
                    }
                }
            }
            Kind::DocType0 { metrics } => {
                for &u in units {
                    if let Some((g, _)) = metrics.get(&(u as u32)) {
                        out.push((g / 256) as u8);
                        out.push(*g as u8);
                    }
                }
            }
        }
        out
    }

    /// BaseFont.getWidth(int char1)
    pub fn width(&self, u: u16) -> i32 {
        let c = u as u32;
        match &self.kind {
            Kind::BuiltinWinAnsi => {
                if c < 128 || (160..=255).contains(&c) {
                    self.widths[c as usize]
                } else {
                    self.widths[winansi_get(c) as usize]
                }
            }
            Kind::BuiltinSpecific | Kind::DocSimple { .. } => {
                self.convert_to_bytes(&[u]).iter().map(|b| self.widths[*b as usize]).sum()
            }
            Kind::DocType0 { metrics } => metrics.get(&c).map(|m| m.1).unwrap_or(0),
        }
    }

    /// BaseFont.getWidth(String)
    pub fn width_str(&self, units: &[u16]) -> i32 {
        match &self.kind {
            Kind::BuiltinWinAnsi | Kind::DocType0 { .. } => units.iter().map(|u| self.width(*u)).sum(),
            _ => self.convert_to_bytes(units).iter().map(|b| self.widths[*b as usize]).sum(),
        }
    }

    /// BaseFont.getWidthPoint(int char1, float fontSize)
    pub fn width_point(&self, u: u16, size: f32) -> f32 {
        self.width(u) as f32 * 0.001f32 * size
    }

    /// The font dictionary iText's writer.addSimple() emits for a built-in font.
    pub fn builtin_font_dict(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Font".to_vec()));
        d.set("Subtype", Object::Name(b"Type1".to_vec()));
        d.set("BaseFont", Object::Name(self.ps_name.as_bytes().to_vec()));
        if matches!(self.kind, Kind::BuiltinWinAnsi) {
            d.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        }
        d
    }
}

/// DocumentFont.fillMetrics(): read a ToUnicode CMap.
fn fill_metrics(touni: &[u8], widths: &HashMap<u32, i32>, dw: i32, metrics: &mut HashMap<u32, (u32, i32)>) {
    // DocumentFont.decodeString(): hex strings are UTF-16BE, literal ones text strings
    fn dec(t: &Token) -> Option<Vec<u16>> {
        match t {
            Token::Str(b, true) => Some(b.chunks(2).map(|c| if c.len() == 2 { u16::from_be_bytes([c[0], c[1]]) } else { (c[0] as u16) << 8 }).collect()),
            Token::Str(b, false) => Some(text(b).encode_utf16().collect()),
            _ => None,
        }
    }
    let mut put = |cid: u32, uni: u32| {
        let w = widths.get(&cid).copied().unwrap_or(dw);
        metrics.insert(uni, (cid, w));
    };
    let mut lx = Lexer::new(touni);
    let mut last: Option<Token> = None;
    while let Ok(Some(tok)) = lx.next_valid_token() {
        let cmd = match &tok {
            Token::Other(s) => s.clone(),
            _ => {
                last = Some(tok);
                continue;
            }
        };
        let n = match &last {
            Some(Token::Number(s)) => s.parse::<f32>().unwrap_or(0.0) as i32,
            _ => 0,
        };
        if cmd == "beginbfchar" {
            for _ in 0..n {
                let (a, b) = match (lx.next_valid_token(), lx.next_valid_token()) {
                    (Ok(Some(a)), Ok(Some(b))) => (a, b),
                    _ => return,
                };
                if let (Some(cid), Some(uni)) = (dec(&a), dec(&b)) {
                    if uni.len() == 1 && !cid.is_empty() {
                        put(cid[0] as u32, uni[0] as u32);
                    }
                }
            }
        } else if cmd == "beginbfrange" {
            for _ in 0..n {
                let (a, b, c) = match (lx.next_valid_token(), lx.next_valid_token(), lx.next_valid_token()) {
                    (Ok(Some(a)), Ok(Some(b)), Ok(Some(c))) => (a, b, c),
                    _ => return,
                };
                let (cid1, cid2) = match (dec(&a), dec(&b)) {
                    (Some(x), Some(y)) if !x.is_empty() && !y.is_empty() => (x[0] as u32, y[0] as u32),
                    _ => continue,
                };
                if let Token::StartArray = c {
                    let mut cid = cid1;
                    loop {
                        match lx.next_valid_token() {
                            Ok(Some(Token::EndArray)) | Ok(None) | Err(_) => break,
                            Ok(Some(t)) => {
                                if let Some(uni) = dec(&t) {
                                    if uni.len() == 1 {
                                        put(cid, uni[0] as u32);
                                    }
                                }
                                cid += 1;
                            }
                        }
                    }
                } else if let Some(uni) = dec(&c) {
                    if uni.len() == 1 {
                        let mut unic = uni[0] as u32;
                        let mut cid = cid1;
                        while cid <= cid2 {
                            put(cid, unic);
                            cid += 1;
                            unic += 1;
                        }
                    }
                }
            }
        }
    }
}
