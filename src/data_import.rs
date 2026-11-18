//! Port of data_import.java + Loadable*.java: parse a dump_data-format info file and apply it
//! (Info dictionary, bookmarks, page labels, page media) to a document. Owner: report agent.
//!
//! The Java UpdateInfo() hands the Info key/value map back to filter.java, which passes it to
//! PdfStamperImp.close(moreInfo). Here `update_info` applies that map to the document's Info
//! dictionary itself, the way the stamper does (new values are UTF-16BE strings with a BOM, an
//! empty value becomes an empty string, which dump_data then no longer reports).
#![allow(dead_code, unused)]
use crate::bookmarks::{self, PdfBookmark};
use crate::pdf;
use crate::report::{self, PdfData, PdfInfo, PdfPageLabel, PdfPageMedia};
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------
// XML entities

/// Port of data_import.XmlStringToJcharArray(): commons-lang3 `StringEscapeUtils.unescapeXml`
/// (the five basic entities plus `&#N;` / `&#xN;` numeric entities; anything else is kept).
pub fn xml_string_to_unicode(s: &str) -> String {
    const BASIC: [(&str, char); 5] =
        [("&quot;", '"'), ("&amp;", '&'), ("&lt;", '<'), ("&gt;", '>'), ("&apos;", '\'')];
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < chars.len() {
        if chars[i] == '&' {
            for (ent, ch) in BASIC.iter() {
                let n = ent.len();
                if i + n <= chars.len() && chars[i..i + n].iter().copied().eq(ent.chars()) {
                    out.push(*ch);
                    i += n;
                    continue 'outer;
                }
            }
            // NumericEntityUnescaper (semicolon required)
            if i + 2 < chars.len() && chars[i + 1] == '#' {
                let mut start = i + 2;
                let mut is_hex = false;
                if chars[start] == 'x' || chars[start] == 'X' {
                    start += 1;
                    is_hex = true;
                }
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_hexdigit() {
                    end += 1;
                }
                if end < chars.len() && chars[end] == ';' {
                    let digits: String = chars[start..end].iter().collect();
                    if let Ok(v) = i32::from_str_radix(&digits, if is_hex { 16 } else { 10 }) {
                        if v >= 0 && v <= 0x10ffff && !digits.starts_with('+') {
                            // (a lone surrogate cannot live in a Rust string)
                            out.push(char::from_u32(v as u32).unwrap_or('\u{fffd}'));
                            i = end + 1;
                            continue 'outer;
                        }
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// LoadableInt / LoadableString / LoadableRectangle

/// Java regex `\s`
fn is_regex_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// Java Character.isWhitespace()
fn is_java_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\x1c'..='\x1f')
        || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
}

/// Port of LoadableInt.BufferInt(): `\s?(\d+).*` -- one or no spaces before data; 0 if no match.
pub fn buffer_int(buff: &str, buff_ii: usize) -> i32 {
    let rest = &buff[buff_ii..];
    let mut it = rest.chars().peekable();
    let mut digits = String::new();
    let rest2 = match rest.chars().next() {
        Some(c) if is_regex_space(c) => &rest[c.len_utf8()..],
        _ => rest,
    };
    for c in rest2.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return 0;
    }
    // (Integer.parseInt throws on overflow, which kills the Java; we read it as 0)
    digits.parse::<i32>().unwrap_or(0)
}

/// Port of LoadableInt.LoadInt(). Uninitialized ints are -1.
pub fn load_int(ii: &mut i32, buff: &str, label: &str) -> bool {
    if buff.starts_with(label) {
        if *ii < 0 {
            *ii = buffer_int(buff, label.len());
        } else {
            eprintln!(
                "pdftk Warning: {} ({}) not empty when reading new {} ({}) -- skipping newer item",
                label,
                ii,
                label,
                buffer_int(buff, label.len())
            );
        }
        return true;
    }
    false
}

/// Port of LoadableString.BufferString(): one or no spaces before data.
pub fn buffer_string(buff: &str, buff_ii: usize) -> String {
    if buff_ii >= buff.len() {
        return String::new();
    }
    let rest = &buff[buff_ii..];
    match rest.chars().next() {
        Some(c) if is_java_whitespace(c) => rest[c.len_utf8()..].to_string(),
        _ => rest.to_string(),
    }
}

/// Port of LoadableString.LoadString().
pub fn load_string(ss: &mut Option<String>, buff: &str, label: &str) -> bool {
    if buff.starts_with(label) {
        match ss {
            None => *ss = Some(buffer_string(buff, label.len())),
            Some(old) => {
                eprintln!(
                    "pdftk Warning: {} ({}) already loaded when reading new {} ({}) -- skipping newer item",
                    label,
                    old,
                    label,
                    buffer_string(buff, label.len())
                );
            }
        }
        return true;
    }
    false
}

/// Port of LoadableString.LoadEnum().
pub fn load_enum(ss: &mut Option<String>, buff: &str, label: &str, is_key: fn(&str) -> bool) -> bool {
    if !load_string(ss, buff, label) {
        return false;
    }
    if ss.as_deref().map_or(false, is_key) {
        return true;
    }
    eprintln!("pdftk Warning: {} invalid value {}", label, ss.as_deref().unwrap_or("null"));
    false
}

/// All the places a `(-?\d*\.?\d+)` match starting at `start` can end, in the order a
/// backtracking regex engine tries them.
fn float_ends(s: &[char], start: usize) -> Vec<usize> {
    let digits_from = |p: usize| s[p.min(s.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut ends = Vec::new();
    let signs: &[bool] = if start < s.len() && s[start] == '-' { &[true, false] } else { &[false] };
    for &neg in signs {
        let p = start + neg as usize;
        for d in (0..=digits_from(p)).rev() {
            let q = p + d;
            let dots: &[bool] = if q < s.len() && s[q] == '.' { &[true, false] } else { &[false] };
            for &dot in dots {
                let r = q + dot as usize;
                for e in (1..=digits_from(r)).rev() {
                    ends.push(r + e);
                }
            }
        }
    }
    ends
}

fn match_floats(s: &[char], pos: usize, spans: &mut Vec<(usize, usize)>) -> bool {
    for end in float_ends(s, pos) {
        spans.push((pos, end));
        if spans.len() == 4 {
            return true;
        }
        if end < s.len() && is_regex_space(s[end]) && match_floats(s, end + 1, spans) {
            return true;
        }
        spans.pop();
    }
    false
}

/// Port of LoadableRectangle.BufferFloats(): `\s?F\sF\sF\sF.*` with F = `(-?\d*\.?\d+)`.
pub fn buffer_floats(buff: &str, buff_ii: usize) -> Option<[f32; 4]> {
    let s: Vec<char> = buff[buff_ii..].chars().collect();
    let mut starts = Vec::new();
    if !s.is_empty() && is_regex_space(s[0]) {
        starts.push(1);
    }
    starts.push(0);
    for start in starts {
        let mut spans = Vec::new();
        if match_floats(&s, start, &mut spans) {
            let mut r = [0.0f32; 4];
            for (k, (a, b)) in spans.iter().enumerate() {
                let text: String = s[*a..*b].iter().collect();
                r[k] = text.parse::<f32>().ok()?;
            }
            return Some(r);
        }
    }
    None
}

/// Port of LoadableRectangle.LoadRectangle().
pub fn load_rectangle(rr: &mut Option<[f32; 4]>, buff: &str, label: &str) -> bool {
    if buff.starts_with(label) {
        match rr {
            None => *rr = buffer_floats(buff, label.len()),
            Some(old) => {
                eprintln!(
                    "pdftk Warning: {} ({}) not empty when reading new {} ({}) -- skipping newer item",
                    label,
                    report::rect_position(old),
                    label,
                    buffer_floats(buff, label.len()).map(|r| report::rect_position(&r)).unwrap_or_default()
                );
            }
        }
        return rr.is_some();
    }
    false
}

// ---------------------------------------------------------------------------------------------
// record loaders

impl PdfInfo {
    pub fn load_key(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_key, buff, Self::KEY_LABEL)
    }
    pub fn load_value(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_value, buff, Self::VALUE_LABEL)
    }
}

/// PdfBookmark while it is being loaded: unlike the shared `PdfBookmark`, the title may still
/// be missing (Java null) and the numbers start out at -1.
#[derive(Debug, Clone)]
struct LoadingBookmark {
    m_title: Option<String>,
    m_level: i32,
    m_page_num: i32,
}

impl LoadingBookmark {
    fn new() -> Self {
        LoadingBookmark { m_title: None, m_level: -1, m_page_num: -1 }
    }
    fn valid(&self) -> bool {
        0 < self.m_level && 0 <= self.m_page_num && self.m_title.is_some()
    }
    fn load_title(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_title, buff, PdfBookmark::TITLE_LABEL)
    }
    fn load_level(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_level, buff, PdfBookmark::LEVEL_LABEL)
    }
    fn load_page_num(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_page_num, buff, PdfBookmark::PAGE_NUMBER_LABEL)
    }
    fn to_bookmark(&self) -> PdfBookmark {
        PdfBookmark {
            m_title: self.m_title.clone().unwrap_or_else(|| "null".to_string()),
            m_level: self.m_level,
            m_page_num: self.m_page_num,
        }
    }
}

impl PdfPageLabel {
    pub fn load_prefix(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_prefix, buff, Self::PREFIX_LABEL)
    }
    pub fn load_num_style(&mut self, buff: &str) -> bool {
        let mut ss = self.m_num_style.clone();
        let success = load_enum(&mut ss, buff, Self::NUM_STYLE_LABEL, PdfPageLabel::is_style);
        if success {
            self.m_num_style = ss;
        }
        success
    }
    pub fn load_new_index(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_new_index, buff, Self::NEW_INDEX_LABEL)
    }
    pub fn load_start(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_start, buff, Self::START_LABEL)
    }
}

impl PdfPageMedia {
    pub fn load_number(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_number, buff, Self::NUMBER_LABEL)
    }
    pub fn load_rotation(&mut self, buff: &str) -> bool {
        let success = load_int(&mut self.m_rotation, buff, Self::ROTATION_LABEL);
        // Faithful to the Java: this normalisation also runs when the label did not match, so
        // an unset rotation (-1) turns into 359 as soon as any later line of the record is
        // examined -- a record without a PageMediaRotation line is therefore "not valid".
        self.m_rotation = (self.m_rotation % 360 + 360) % 360;
        success
    }
    pub fn load_rect(&mut self, buff: &str) -> bool {
        load_rectangle(&mut self.m_rect, buff, Self::RECT_LABEL)
    }
    pub fn load_dimensions(&mut self, buff: &str) -> bool {
        // Ignoring dimensions. Alternative: cross-check with Rect.
        buff.starts_with(Self::DIMENSIONS_LABEL)
    }
    pub fn load_crop(&mut self, buff: &str) -> bool {
        load_rectangle(&mut self.m_crop, buff, Self::CROP_LABEL)
    }
}

impl PdfData {
    pub fn load_num_pages(&mut self, buff: &str) -> bool {
        load_int(&mut self.m_num_pages, buff, Self::NUM_PAGES_LABEL)
    }
    pub fn load_id0(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_id_0, buff, Self::ID_0_LABEL)
    }
    pub fn load_id1(&mut self, buff: &str) -> bool {
        load_string(&mut self.m_id_1, buff, Self::ID_1_LABEL)
    }
}

// ---------------------------------------------------------------------------------------------
// LoadDataFile

/// java.util.Scanner.nextLine() line splitting.
fn scanner_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\r' => {
                lines.push(&text[start..i]);
                if let Some((_, '\n')) = it.peek() {
                    it.next();
                    start = i + 2;
                } else {
                    start = i + 1;
                }
            }
            '\n' | '\u{2028}' | '\u{2029}' | '\u{85}' => {
                lines.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Port of data_import.LoadDataFile(). Bookmark titles, Info values and label prefixes are
/// kept exactly as written in the file (still XML-escaped when the file is).
pub fn load_data_file(input_stream: &mut dyn std::io::Read) -> PdfData {
    let mut pdf_data_p = PdfData::default();

    let mut bytes = Vec::new();
    let _ = input_stream.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let lines = scanner_lines(&text);
    let mut ifs = lines.into_iter();
    let mut next_line = ifs.next();

    let mut buff_prev = String::new();
    let mut buff_prev_len: usize = 0;

    let mut info = PdfInfo::default();
    let mut info_b = false;

    let mut bookmark = LoadingBookmark::new();
    let mut bookmark_b = false;

    let mut pagelabel = PdfPageLabel::default();
    let mut pagelabel_b = false;

    let mut pagemedia = PdfPageMedia::default();
    let mut pagemedia_b = false;

    let mut eof = next_line.is_none();

    while !eof {
        eof = next_line.is_none();
        let buff: &str = if eof { "" } else { next_line.take().unwrap_or("") };
        if !eof {
            next_line = ifs.next();
        }

        if eof
            || buff.starts_with(PdfInfo::BEGIN_MARK)
            || buff.starts_with(PdfBookmark::BEGIN_MARK)
            || buff.starts_with(PdfPageLabel::BEGIN_MARK)
            || buff.starts_with(PdfPageMedia::BEGIN_MARK)
            || !buff_prev.is_empty() && !buff.starts_with(buff_prev.as_str())
        {
            // start of a new record or end of file
            // pack data and reset

            if info_b {
                if info.valid() {
                    pdf_data_p.m_info.push(info.clone());
                } else {
                    eprintln!("pdftk Warning: data info record not valid -- skipped; data:");
                    eprint!("{}", info);
                }
            } else if bookmark_b {
                if bookmark.valid() {
                    pdf_data_p.m_bookmarks.push(bookmark.to_bookmark());
                } else {
                    eprintln!("pdftk Warning: data bookmark record not valid -- skipped; data:");
                    eprint!("{}", bookmark.to_bookmark());
                }
            } else if pagelabel_b {
                if pagelabel.valid() {
                    pdf_data_p.m_pagelabels.push(pagelabel.clone());
                } else {
                    eprintln!("pdftk Warning: page label record not valid -- skipped; data:");
                    eprint!("{}", pagelabel);
                }
            } else if pagemedia_b {
                if pagemedia.valid() {
                    pdf_data_p.m_pagemedia.push(pagemedia.clone());
                } else {
                    eprintln!("pdftk Warning: page media record not valid -- skipped; data:");
                    eprint!("{}", pagemedia);
                }
            }

            // reset
            buff_prev.clear();
            buff_prev_len = 0;
            //
            info = PdfInfo::default();
            info_b = false;
            //
            bookmark = LoadingBookmark::new();
            bookmark_b = false;
            //
            pagelabel = PdfPageLabel::default();
            pagelabel_b = false;
            //
            pagemedia = PdfPageMedia::default();
            pagemedia_b = false;
        }

        // whitespace or comment; skip
        if buff.is_empty() || buff.starts_with('#') {
            continue;
        }
        // info record
        else if buff.starts_with(PdfInfo::PREFIX) {
            buff_prev_len = PdfInfo::PREFIX.len();
            info_b = true;

            if buff.starts_with(PdfInfo::BEGIN_MARK) || info.load_key(buff) || info.load_value(buff) {
                // success
            } else {
                eprintln!("pdftk Warning: unexpected Info case in LoadDataFile(); continuing");
            }
        }
        // bookmark record
        else if buff.starts_with(PdfBookmark::PREFIX) {
            buff_prev_len = PdfBookmark::PREFIX.len();
            bookmark_b = true;

            if buff.starts_with(PdfBookmark::BEGIN_MARK)
                || bookmark.load_title(buff)
                || bookmark.load_level(buff)
                || bookmark.load_page_num(buff)
            {
                // success
            } else {
                eprintln!("pdftk Warning: unexpected Bookmark case in LoadDataFile(); continuing");
            }
        }
        // page label record
        else if buff.starts_with(PdfPageLabel::PREFIX) {
            buff_prev_len = PdfPageLabel::PREFIX.len();
            pagelabel_b = true;

            if buff.starts_with(PdfPageLabel::BEGIN_MARK)
                || pagelabel.load_new_index(buff)
                || pagelabel.load_start(buff)
                || pagelabel.load_prefix(buff)
                || pagelabel.load_num_style(buff)
            {
                // success
            } else {
                eprintln!("pdftk Warning: unexpected PageLabel case in LoadDataFile(); continuing");
            }
        }
        // page media record
        else if buff.starts_with(PdfPageMedia::PREFIX) {
            buff_prev_len = PdfPageMedia::PREFIX.len();
            pagemedia_b = true;

            if buff.starts_with(PdfPageMedia::BEGIN_MARK)
                || pagemedia.load_number(buff)
                || pagemedia.load_rotation(buff)
                || pagemedia.load_rect(buff)
                || pagemedia.load_dimensions(buff)
                || pagemedia.load_crop(buff)
            {
                // success
            } else {
                eprintln!("pdftk Warning: unexpected PageMedia case in LoadDataFile(); continuing");
            }
        }
        // pdf id
        else if buff.starts_with(PdfData::PREFIX) {
            buff_prev_len = 0; // not a record

            if pdf_data_p.load_id0(buff) || pdf_data_p.load_id1(buff) {
                // success
            } else {
                eprintln!("pdftk Warning: unexpected PdfID case in LoadDataFile(); continuing");
            }
        }
        // number of pages
        else if pdf_data_p.load_num_pages(buff) {
            buff_prev_len = 0; // not a record
        } else {
            eprintln!("pdftk Warning: unexpected case 1 in LoadDataFile(); continuing");
        }

        buff_prev = buff[..buff_prev_len].to_string();
    }

    if buff_prev_len != 0 {
        // warning; some incomplete record hasn't been packed
        eprintln!("pdftk Warning in LoadDataFile(): incomplete record;");
    }

    pdf_data_p
}

// ---------------------------------------------------------------------------------------------
// applying

/// iText ByteBuffer.formatDouble(): how a freshly created PdfNumber(float) is written
/// (two decimals up to 32767, five below 1, integers above).
pub fn itext_format_double(value: f64) -> String {
    let mut d = value;
    if d.abs() < 0.000015 {
        return "0".to_string();
    }
    let negative = d < 0.0;
    if negative {
        d = -d;
    }
    let sign = if negative { "-" } else { "" };
    if d < 1.0 {
        d += 0.000005;
        if d >= 1.0 {
            return format!("{}1", sign);
        }
        let v = (d * 100000.0) as i64;
        let mut s = format!("{}0.{:05}", sign, v);
        while s.ends_with('0') {
            s.pop();
        }
        s
    } else if d <= 32767.0 {
        d += 0.005;
        let v = (d * 100.0) as i64;
        let mut s = format!("{}{}", sign, v / 100);
        let frac = v % 100;
        if frac != 0 {
            s.push('.');
            s.push_str(&format!("{:02}", frac));
            if s.ends_with('0') {
                s.pop();
            }
        }
        s
    } else {
        d += 0.5;
        format!("{}{}", sign, d as i64)
    }
}

/// new PdfNumber(float)
fn pdf_number(v: f32) -> Object {
    let s = itext_format_double(v as f64);
    if s.contains('.') {
        Object::Real(s.parse::<f32>().unwrap_or(v))
    } else {
        Object::Integer(s.parse::<i64>().unwrap_or(v as i64))
    }
}

/// Port of data_import.toPdfArray().
fn to_pdf_array(r: &[f32; 4]) -> Object {
    Object::Array(r.iter().map(|v| pdf_number(*v)).collect())
}

/// new PdfString(text, PdfObject.TEXT_UNICODE): UTF-16BE with a byte order mark, always.
fn text_unicode_string(text: &str) -> Object {
    let mut b = vec![0xfe, 0xff];
    for u in text.encode_utf16() {
        b.push((u >> 8) as u8);
        b.push((u & 0xff) as u8);
    }
    Object::string_literal(b)
}

/// Build a dictionary whose entries come in the order iText's HashMap-backed PdfDictionary
/// would write them.
fn itext_dictionary(entries: Vec<(Vec<u8>, Object)>) -> Dictionary {
    let hashes: Vec<i32> = entries.iter().map(|(k, _)| report::itext_name_hash(k)).collect();
    let order = report::java_hashmap_order(&hashes);
    let mut slots: Vec<Option<(Vec<u8>, Object)>> = entries.into_iter().map(Some).collect();
    let mut dict = Dictionary::new();
    for i in order {
        if let Some((k, v)) = slots[i].take() {
            dict.set(k, v);
        }
    }
    dict
}

/// Port of PdfPageLabel.BuildPageLabels() (iText PdfPageLabels.getDictionary()).
pub fn build_page_labels(pagelabels_data: &[PdfPageLabel]) -> Dictionary {
    // PdfPageLabels starts out with "page 1: decimal, starting at 1"
    let mut map: BTreeMap<i32, (Option<&[u8]>, Option<String>, i32)> = BTreeMap::new();
    map.insert(1, (Some(b"D"), None, 1));
    for pagelabel in pagelabels_data {
        let tag = report::NUMBERING_STYLES
            .iter()
            .find(|(r, _)| Some(*r) == pagelabel.m_num_style.as_deref())
            .and_then(|(_, tag)| *tag);
        map.insert(pagelabel.m_new_index, (tag, pagelabel.m_prefix.clone(), pagelabel.m_start));
    }
    let mut array: Vec<Object> = Vec::new();
    for (page, (tag, text, st)) in map {
        let mut entries: Vec<(Vec<u8>, Object)> = Vec::new();
        if let Some(tag) = tag {
            entries.push((b"S".to_vec(), Object::Name(tag.to_vec())));
        }
        if let Some(text) = text {
            entries.push((b"P".to_vec(), text_unicode_string(&text)));
        }
        if st != 1 {
            entries.push((b"St".to_vec(), Object::Integer(st as i64)));
        }
        array.push(Object::Integer((page - 1) as i64));
        array.push(Object::Dictionary(itext_dictionary(entries)));
    }
    let mut dic = Dictionary::new();
    dic.set("Nums", Object::Array(array));
    dic
}

/// The Info entries of `pdf_data` as UpdateInfo() hands them to the stamper: (key, value) in
/// the iteration order of the Java HashMap<String, String>; later records replace earlier ones.
pub fn info_map(pdf_data: &PdfData, utf8_b: bool) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for it in &pdf_data.m_info {
        let key = it.m_key.clone().unwrap_or_default();
        let raw = it.m_value.clone().unwrap_or_default();
        let value = if raw.is_empty() {
            // An empty string signals deletion
            String::new()
        } else if utf8_b {
            raw
        } else {
            xml_string_to_unicode(&raw)
        };
        match entries.iter_mut().find(|(k, _)| *k == key) {
            Some(e) => e.1 = value,
            None => entries.push((key, value)),
        }
    }
    let hashes: Vec<i32> = entries.iter().map(|(k, _)| report::java_string_hash(k)).collect();
    let order = report::java_hashmap_order(&hashes);
    order.into_iter().map(|i| entries[i].clone()).collect()
}

/// Port of the Info part of PdfStamperImp.close(moreInfo): rebuild the Info dictionary from the
/// old one plus `more_info`.
pub fn apply_more_info(doc: &mut Document, more_info: &[(String, String)]) {
    let info_ref = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    let mut entries: Vec<(Vec<u8>, Object)> = Vec::new();
    {
        let old_info = match doc.trailer.get(b"Info").ok().and_then(|o| pdf::resolve(doc, o)) {
            Some(Object::Dictionary(d)) => Some(d),
            _ => None,
        };
        if let Some(old_info) = old_info {
            for key in report::itext_dict_keys(old_info) {
                let raw = match old_info.get(key) {
                    Ok(o) => o,
                    Err(_) => continue,
                };
                let value = match pdf::resolve(doc, raw) {
                    None | Some(Object::Null) => continue, // put(key, null) removes
                    Some(Object::Stream(_)) => raw.clone(),
                    Some(o) => o.clone(),
                };
                entries.push((key.clone(), value));
            }
        }
    }
    for (key, value) in more_info {
        // new PdfName(key): chars are cut down to their low byte
        let name: Vec<u8> = key.encode_utf16().map(|u| (u & 0xff) as u8).collect();
        let obj = text_unicode_string(value);
        match entries.iter_mut().find(|(k, _)| *k == name) {
            Some(e) => e.1 = obj,
            None => entries.push((name, obj)),
        }
    }
    if entries.is_empty() {
        doc.trailer.remove(b"Info");
        return;
    }
    let new_info = Object::Dictionary(itext_dictionary(entries));
    match info_ref {
        Some(id) if matches!(doc.objects.get(&id), Some(Object::Dictionary(_))) => {
            doc.objects.insert(id, new_info);
        }
        _ => {
            let id = doc.add_object(new_info);
            doc.trailer.set("Info", Object::Reference(id));
        }
    }
}

/// Port of data_import.UpdateInfo() for already-parsed data.
pub fn update_info_from_data(doc: &mut Document, pdf_data: &PdfData, utf8_b: bool) -> bool {
    let mut ret_val_b = true;

    let has_root = match doc.trailer.get(b"Root").ok().and_then(|o| pdf::resolve(doc, o)) {
        Some(Object::Dictionary(_)) => true,
        _ => false,
    };
    if has_root {
        // bookmarks
        if !pdf_data.m_bookmarks.is_empty() {
            let list: Vec<PdfBookmark> = pdf_data
                .m_bookmarks
                .iter()
                .map(|b| PdfBookmark {
                    m_title: if utf8_b { b.m_title.clone() } else { xml_string_to_unicode(&b.m_title) },
                    m_level: b.m_level,
                    m_page_num: b.m_page_num,
                })
                .collect();
            bookmarks::build_bookmarks(doc, &list);
        }

        // page labels
        if !pdf_data.m_pagelabels.is_empty() {
            let pagelabels_p = build_page_labels(&pdf_data.m_pagelabels);
            let pagelabels_ref_p = doc.add_object(Object::Dictionary(pagelabels_p));
            if let Ok(root_p) = doc.catalog_mut() {
                root_p.set("PageLabels", Object::Reference(pagelabels_ref_p));
            }
        }
    } else {
        eprintln!("pdftk Error in UpdateInfo(): no Root dictionary found;");
        ret_val_b = false;
    }

    // page media
    if !pdf_data.m_pagemedia.is_empty() {
        let pages = pdf::page_ids(doc);
        for pagemedia in &pdf_data.m_pagemedia {
            let page_p = if pagemedia.m_number >= 1 {
                pages
                    .get((pagemedia.m_number - 1) as usize)
                    .and_then(|id| doc.get_dictionary_mut(*id).ok())
            } else {
                None
            };
            match page_p {
                Some(page_p) => {
                    if pagemedia.m_rotation >= 0 {
                        page_p.set("Rotate", Object::Integer(pagemedia.m_rotation as i64));
                    }
                    if let Some(rect) = &pagemedia.m_rect {
                        page_p.set("MediaBox", to_pdf_array(rect));
                    }
                    if let Some(crop) = &pagemedia.m_crop {
                        page_p.set("CropBox", to_pdf_array(crop));
                    }
                }
                None => {
                    eprintln!("pdftk Error in UpdateInfo(): page {} not found;", pagemedia.m_number);
                    ret_val_b = false;
                }
            }
        }
    }

    // metadata
    if !pdf_data.m_info.is_empty() {
        let more_info = info_map(pdf_data, utf8_b);
        apply_more_info(doc, &more_info);
    }

    ret_val_b
}

/// Port of data_import.UpdateInfo(). Returns false if nothing was applied (caller prints the warning).
pub fn update_info(doc: &mut Document, input: &mut dyn std::io::Read, utf8_b: bool) -> bool {
    let pdf_data = load_data_file(input);
    update_info_from_data(doc, &pdf_data, utf8_b)
}

#[cfg(test)]
mod tests {
    //! Ports of the update_info cases of DataTest.java, plus comparisons with the Java pdftk.
    use super::*;
    use crate::report::tests::{dump, fixture, load, oracle};

    fn apply(doc: &mut Document, data: &str, utf8_b: bool) -> bool {
        update_info(doc, &mut data.as_bytes(), utf8_b)
    }

    fn parse(lines: &[&str]) -> PdfData {
        load_data_file(&mut lines.join("\n").as_bytes())
    }

    /// Save and reopen, as `pdftk in.pdf update_info - output out.pdf` + `pdftk out.pdf dump_data` would.
    fn roundtrip(doc: &mut Document, tag: &str) -> (String, Document) {
        let dir = std::env::temp_dir().join(format!("pdftk-rs-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}.pdf", tag)).to_string_lossy().into_owned();
        doc.save(&path).unwrap();
        let reopened = load(&path);
        (path, reopened)
    }

    // DataTest.idempotent
    #[test]
    fn idempotent() {
        let mut doc = load(&fixture("refs.pdf"));
        let expected = dump(&doc, true);
        assert!(apply(&mut doc, &expected, true));
        let (_, reopened) = roundtrip(&mut doc, "idempotent");
        assert_eq!(expected, dump(&reopened, true));
    }

    // DataTest.update_info_incomplete_record
    #[test]
    fn update_info_incomplete_record() {
        let data = parse(&["InfoBegin", "InfoKey: Title", " ", "InfoBegin", "InfoKey: Author", " "]);
        assert!(data.m_info.is_empty()); // "data info record not valid" twice
    }

    // DataTest.update_page_labels_new
    #[test]
    fn update_page_labels_new() {
        let data = [
            "PageLabelBegin",
            "PageLabelNewIndex: 3",
            "PageLabelStart: 3",
            "PageLabelPrefix: p",
            "PageLabelNumStyle: LowercaseRomanNumerals",
        ]
        .join("\n");
        let mut doc = load(&fixture("refs.pdf"));
        assert!(apply(&mut doc, &data, false));
        let (_, reopened) = roundtrip(&mut doc, "labels_new");
        assert!(dump(&reopened, true).contains(&data));
    }

    // DataTest.update_page_labels_replace
    #[test]
    fn update_page_labels_replace() {
        let data1 =
            ["PageLabelBegin", "PageLabelNewIndex: 3", "PageLabelStart: 3", "PageLabelNumStyle: LowercaseRomanNumerals"]
                .join("\n");
        let data2 =
            ["PageLabelBegin", "PageLabelNewIndex: 4", "PageLabelStart: 4", "PageLabelNumStyle: UppercaseRomanNumerals"]
                .join("\n");
        let mut doc = load(&fixture("refs.pdf"));
        assert!(apply(&mut doc, &data1, false));
        let (_, mut doc2) = roundtrip(&mut doc, "labels_replace1");
        assert!(dump(&doc2, true).contains(&data1));
        assert!(apply(&mut doc2, &data2, false));
        let (_, doc3) = roundtrip(&mut doc2, "labels_replace2");
        let out = dump(&doc3, true);
        assert!(!out.contains(&data1));
        assert!(out.contains(&data2));
    }

    // DataTest.update_page_labels_badindex / badstart / badstyle
    #[test]
    fn update_page_labels_bad() {
        for lines in [
            ["PageLabelBegin", "PageLabelNewIndex: -1", "PageLabelStart: 3", "PageLabelNumStyle: LowercaseRomanNumerals"],
            ["PageLabelBegin", "PageLabelNewIndex: 3", "PageLabelStart: -1", "PageLabelNumStyle: LowercaseRomanNumerals"],
            ["PageLabelBegin", "PageLabelNewIndex: 3", "PageLabelStart: 3", "PageLabelNumStyle: NotAStyle"],
        ] {
            assert!(parse(&lines).m_pagelabels.is_empty(), "{:?}", lines); // "page label record not valid"
        }
    }

    // DataTest.update_page_media_replace
    #[test]
    fn update_page_media_replace() {
        let data = ["PageMediaBegin", "PageMediaNumber: 3", "PageMediaRotation: 90", "PageMediaRect: 1 1 611 791", "PageMediaCropRect: 2 2 610 792"];
        let expect = [data[0], data[1], data[2], data[3], "PageMediaDimensions: 610 790", data[4]].join("\n");
        let mut doc = load(&fixture("refs.pdf"));
        assert!(apply(&mut doc, &data.join("\n"), false));
        let (_, reopened) = roundtrip(&mut doc, "media_replace");
        assert!(dump(&reopened, true).contains(&expect));
    }

    // DataTest.update_page_media_badpage
    #[test]
    fn update_page_media_badpage() {
        let data = ["PageMediaBegin", "PageMediaNumber: 42", "PageMediaRotation: 90", "PageMediaRect: 1 1 611 791", "PageMediaCropRect: 2 2 610 792"];
        let mut doc = load(&fixture("refs.pdf"));
        assert!(!apply(&mut doc, &data.join("\n"), false)); // "page 42 not found"
    }

    // DataTest.update_page_media_badrotation / badrect
    #[test]
    fn update_page_media_bad() {
        let badrot = parse(&["PageMediaBegin", "PageMediaNumber: 3", "PageMediaRotation: 45", "PageMediaRect: 1 1 611 791", "PageMediaCropRect: 2 2 610 792"]);
        assert!(badrot.m_pagemedia.is_empty());
        let badrect = parse(&["PageMediaBegin", "PageMediaNumber: 3", "PageMediaRect: 1 1 611"]);
        assert!(badrect.m_pagemedia.is_empty());
        // a Java quirk we keep: without a rotation line the record is rejected (rotation reads as 359)
        let norot = parse(&["PageMediaBegin", "PageMediaNumber: 3", "PageMediaRect: 1 1 611 791"]);
        assert!(norot.m_pagemedia.is_empty());
    }

    #[test]
    fn loadables() {
        assert_eq!(buffer_int("X: 12abc", 2), 12);
        assert_eq!(buffer_int("X:12", 2), 12);
        assert_eq!(buffer_int("X:  12", 2), 0);
        assert_eq!(buffer_int("X: -1", 2), 0);
        assert_eq!(buffer_string("X: a b ", 2), "a b ");
        assert_eq!(buffer_string("X:", 2), "");
        assert_eq!(buffer_floats("R: -.5 0 100.5 200 junk", 2), Some([-0.5, 0.0, 100.5, 200.0]));
        assert_eq!(buffer_floats("R: 1 2 3 45", 2), Some([1.0, 2.0, 3.0, 45.0]));
        assert_eq!(buffer_floats("R: 1 2 3", 2), None);
        assert_eq!(buffer_floats("R: 1  2 3 4", 2), None);
        assert_eq!(buffer_floats("R: 1.2.3 4 5 6", 2), None);
        assert_eq!(xml_string_to_unicode("caf&#233; &lt;x&gt; &#x1F600; &bogus; &#12 &amp;amp; &apos;&quot;"), "caf\u{e9} <x> \u{1f600} &bogus; &#12 &amp; '\"");
        assert_eq!(itext_format_double(100.126), "100.13");
        assert_eq!(itext_format_double(-0.5), "-0.5");
        assert_eq!(itext_format_double(611.0), "611");
        assert_eq!(itext_format_double(40000.6), "40001");
    }

    /// Apply info files with our importer and with the Java pdftk; `pdftk dump_data` of both
    /// results must agree.
    #[test]
    fn update_info_matches_oracle() {
        let cases: [(&str, bool, &str); 4] = [
            ("refs.pdf", false, "InfoBegin\nInfoKey: Subject\nInfoValue:\nInfoBegin\nInfoKey: New Key\nInfoValue: caf&#233; &lt;x&gt; &#x1F600; &bogus; &#12\nInfoBegin\nInfoKey: Title\nInfoValue: plain\nPageLabelBegin\nPageLabelNewIndex: 2\nPageLabelStart: 3\nPageLabelPrefix: caf&#233;\nPageLabelNumStyle: UppercaseLetters\nPageMediaBegin\nPageMediaNumber: 1\nPageMediaRect: 0 0 100.5 200\nPageMediaBegin\nPageMediaNumber: 2\nPageMediaRotation: 450\nPageMediaRect: -.5 0 100.126 200 junk\nPageMediaCropRect: 1 2 3 4\nBookmarkBegin\nBookmarkTitle: Deep &amp; first\nBookmarkLevel: 3\nBookmarkPageNumber: 2\nBookmarkBegin\nBookmarkTitle: top \u{e9}\u{4e2d}\nBookmarkLevel: 1\nBookmarkPageNumber: 9\nBookmarkBegin\nBookmarkTitle: sub\nBookmarkLevel: 2\nBookmarkPageNumber: 0\nBookmarkBegin\nBookmarkTitle: subsubsub\nBookmarkLevel: 4\nBookmarkPageNumber: 3\nBookmarkBegin\nBookmarkTitle: top2\nBookmarkLevel: 1\nBookmarkPageNumber: 1\n"),
            ("refs.pdf", true, "InfoBegin\nInfoKey: Title\nInfoValue: caf\u{e9} &amp; \u{1f600}\nInfoBegin\nInfoKey: Producer\nInfoValue: me\nInfoBegin\nInfoKey: Producer\nInfoValue: me again\nBookmarkBegin\nBookmarkTitle: \u{4e2d}\u{6587} &lt;\nBookmarkLevel: 1\nBookmarkPageNumber: 8\nPageLabelBegin\nPageLabelNewIndex: 1\nPageLabelStart: 7\nPageLabelNumStyle: NoNumber\nPageLabelBegin\nPageLabelNewIndex: 5\nPageLabelStart: 1\nPageLabelPrefix: \u{e9}-\nPageLabelNumStyle: LowercaseLetters\n"),
            ("blank.pdf", false, "# comment\nInfoBegin\nInfoKey: Creator\nInfoValue:\n\nInfoBegin\nInfoKey: A1\nInfoValue: 1\nInfoBegin\nInfoKey: A2\nInfoValue: 2\nInfoBegin\nInfoKey: A3\nInfoValue: 3\nInfoBegin\nInfoKey: A4\nInfoValue: 4\nInfoBegin\nInfoKey: A5\nInfoValue: 5\nInfoBegin\nInfoKey: A6\nInfoValue: 6\nInfoBegin\nInfoKey: A7\nInfoValue: 7\nNumberOfPages: 5\nPdfID0: abc\nPageMediaBegin\nPageMediaNumber: 1\nPageMediaRotation: 270\n"),
            ("issue123.pdf", false, "BookmarkBegin\nBookmarkTitle: only\nBookmarkLevel: 2\nBookmarkPageNumber: 2\nPageMediaBegin\nPageMediaNumber: 2\nPageMediaRotation: 180\nPageMediaCropRect: 10 10 0.25 0.00001\n"),
        ];
        let dir = std::env::temp_dir().join(format!("pdftk-rs-report-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut compared = 0;
        for (n, (name, utf8_b, data)) in cases.iter().enumerate() {
            let input = fixture(name);
            let info = dir.join(format!("case{}.txt", n)).to_string_lossy().into_owned();
            std::fs::write(&info, data).unwrap();
            let java_out = dir.join(format!("case{}-java.pdf", n)).to_string_lossy().into_owned();
            let op = if *utf8_b { "update_info_utf8" } else { "update_info" };
            if oracle(&[&input, op, &info, "output", &java_out]).is_none() {
                eprintln!("oracle unavailable; skipping case {}", n);
                continue;
            }
            let mut doc = load(&input);
            apply(&mut doc, data, *utf8_b);
            let (rust_out, _) = roundtrip(&mut doc, &format!("case{}-rust", n));
            for dump_op in ["dump_data", "dump_data_utf8"] {
                // (page numbers of issue123.pdf's old outline are not at stake: it is replaced)
                let expected = oracle(&[&java_out, dump_op]).expect("oracle dump of its own output");
                let got = oracle(&[&rust_out, dump_op]).expect("oracle dump of our output");
                assert_eq!(
                    String::from_utf8_lossy(&expected),
                    String::from_utf8_lossy(&got),
                    "case {} ({} {})",
                    n,
                    name,
                    dump_op
                );
                compared += 1;
            }
        }
        eprintln!("update_info_matches_oracle: {} dumps compared", compared);
    }
}
