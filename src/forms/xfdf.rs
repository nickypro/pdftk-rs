//! Port of iText XfdfReader and the parts of SimpleXMLParser it exercises.
use super::util::JavaHashMap;
use super::FormData;
use std::collections::HashMap;
use std::sync::OnceLock;

/// SimpleXMLParser's entity table, read from the Java source so the two cannot drift apart.
fn entity_map() -> &'static HashMap<String, char> {
    static MAP: OnceLock<HashMap<String, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let src = include_str!("../../resources/SimpleXMLParser.java");
        let mut m = HashMap::new();
        for line in src.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("entityMap.put(\"") {
                if let Some(q) = rest.find('"') {
                    let name = &rest[..q];
                    if let Some(i) = rest.find("new Character('") {
                        let lit = &rest[i + 15..];
                        let ch = if let Some(hex) = lit.strip_prefix("\\u") {
                            u32::from_str_radix(&hex[..4], 16).ok().and_then(char::from_u32)
                        } else if let Some(esc) = lit.strip_prefix('\\') {
                            esc.chars().next()
                        } else {
                            lit.chars().next()
                        };
                        if let Some(c) = ch {
                            m.insert(name.to_string(), c);
                        }
                    }
                }
            }
        }
        m
    })
}

/// SimpleXMLParser.getEncodingName()
fn encoding_name(b4: &[u8]) -> &'static str {
    let (b0, b1, b2, b3) = (b4[0], b4[1], b4[2], b4[3]);
    if b0 == 0xFE && b1 == 0xFF {
        return "UTF-16BE";
    }
    if b0 == 0xFF && b1 == 0xFE {
        return "UTF-16LE";
    }
    if b0 == 0xEF && b1 == 0xBB && b2 == 0xBF {
        return "UTF-8";
    }
    if (b0 == 0x00 && b1 == 0x00 && b2 == 0x00 && b3 == 0x3C)
        || (b0 == 0x3C && b1 == 0x00 && b2 == 0x00 && b3 == 0x00)
        || (b0 == 0x00 && b1 == 0x00 && b2 == 0x3C && b3 == 0x00)
        || (b0 == 0x00 && b1 == 0x3C && b2 == 0x00 && b3 == 0x00)
    {
        return "ISO-10646-UCS-4";
    }
    if b0 == 0x00 && b1 == 0x3C && b2 == 0x00 && b3 == 0x3F {
        return "UTF-16BE";
    }
    if b0 == 0x3C && b1 == 0x00 && b2 == 0x3F && b3 == 0x00 {
        return "UTF-16LE";
    }
    if b0 == 0x4C && b1 == 0x6F && b2 == 0xA7 && b3 == 0x94 {
        return "CP037";
    }
    "UTF-8"
}

/// SimpleXMLParser.getDeclaredEncoding()
fn declared_encoding(decl: &str) -> Option<String> {
    let idx = decl.find("encoding")?;
    let idx1 = decl[idx..].find('"').map(|i| (i + idx) as i64).unwrap_or(-1);
    let idx2 = decl[idx..].find('\'').map(|i| (i + idx) as i64).unwrap_or(-1);
    if idx1 == idx2 {
        return None;
    }
    if (idx1 < 0 && idx2 > 0) || (idx2 > 0 && idx2 < idx1) {
        let s = (idx2 + 1) as usize;
        let e = decl[s..].find('\'')? + s;
        return Some(decl[s..e].to_string());
    }
    if (idx2 < 0 && idx1 > 0) || (idx1 > 0 && idx1 < idx2) {
        let s = (idx1 + 1) as usize;
        let e = decl[s..].find('"')? + s;
        return Some(decl[s..e].to_string());
    }
    None
}

/// Decode the document body. Only the charsets below are ported; anything else is an error
/// (the Java accepts every charset the JVM knows).
fn decode(bytes: &[u8], encoding: &str) -> Result<Vec<char>, String> {
    let up = encoding.to_ascii_uppercase();
    let utf16 = |be: bool, b: &[u8]| -> Vec<char> {
        let units: Vec<u16> = b
            .chunks(2)
            .map(|c| {
                if c.len() < 2 {
                    0xfffd
                } else if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units).chars().collect()
    };
    match up.as_str() {
        "UTF-8" | "UTF8" => Ok(String::from_utf8_lossy(bytes).chars().collect()),
        "ISO-8859-1" | "ISO8859_1" | "LATIN1" | "ISO_8859-1" | "L1" | "IBM819" | "CP819" | "CSISOLATIN1"
        | "ISO-IR-100" | "ISO_8859-1:1987" => Ok(bytes.iter().map(|b| *b as char).collect()),
        "US-ASCII" | "ASCII" | "ISO646-US" | "ISO_646.IRV:1991" | "ANSI_X3.4-1968" | "ANSI_X3.4-1986"
        | "IBM367" | "CP367" | "CSASCII" | "ISO-IR-6" | "US" | "ISO_646.IRV:1983" => {
            Ok(bytes.iter().map(|b| if *b < 128 { *b as char } else { '\u{fffd}' }).collect())
        }
        "WINDOWS-1252" | "CP1252" => Ok(bytes
            .iter()
            .map(|b| {
                const HI: [u16; 32] = [
                    0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
                    0x0152, 0xFFFD, 0x017D, 0xFFFD, 0xFFFD, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
                    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178,
                ];
                if (0x80..0xa0).contains(b) {
                    char::from_u32(HI[(*b - 0x80) as usize] as u32).unwrap()
                } else {
                    *b as char
                }
            })
            .collect()),
        "UTF-16BE" | "UNICODEBIGUNMARKED" => Ok(utf16(true, bytes)),
        "UTF-16LE" | "UNICODELITTLEUNMARKED" => Ok(utf16(false, bytes)),
        "UTF-16" => {
            if bytes.starts_with(&[0xff, 0xfe]) {
                Ok(utf16(false, &bytes[2..]))
            } else if bytes.starts_with(&[0xfe, 0xff]) {
                Ok(utf16(true, &bytes[2..]))
            } else {
                Ok(utf16(true, bytes))
            }
        }
        _ => Err(format!("unsupported XML encoding {}", encoding)),
    }
}

fn is_ws(c: char) -> bool {
    // Character.isWhitespace() for the characters that matter in markup
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1f}')
        || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Text,
    Entity,
    OpenTag,
    CloseTag,
    StartTag,
    AttributeLvalue,
    AttributeEqual,
    AttributeRvalue,
    Quote,
    InTag,
    SingleTag,
    Comment,
    Done,
    Doctype,
    Pre,
    Cdata,
}

/// XfdfReader (the SimpleXMLDocHandler)
#[derive(Default)]
struct Handler {
    found_root: bool,
    field_names: Vec<String>,
    field_values: Vec<String>,
    fields: Option<JavaHashMap<String>>,
    list_fields: JavaHashMap<Vec<String>>,
    fields_rich_text: JavaHashMap<String>,
}

impl Handler {
    fn start_element(&mut self, tag: &str, attrs: &HashMap<String, String>) -> Result<(), String> {
        if !self.found_root {
            if tag != "xfdf" {
                return Err("Root element is not Bookmark.".into());
            }
            self.found_root = true;
        }
        match tag {
            "xfdf" | "f" => {}
            "fields" => {
                self.fields = Some(JavaHashMap::new());
                self.list_fields = JavaHashMap::new();
                self.fields_rich_text = JavaHashMap::new();
            }
            "field" => {
                // a missing name attribute is a null on the Java stack and prints as "null"
                self.field_names.push(attrs.get("name").cloned().unwrap_or_else(|| "null".into()));
            }
            "value" | "value-richtext" => self.field_values.push(String::new()),
            _ => {}
        }
        Ok(())
    }

    fn end_element(&mut self, tag: &str) {
        if tag == "value" || tag == "value-richtext" {
            let mut f_name = String::new();
            for n in &self.field_names {
                f_name.push('.');
                f_name.push_str(n);
            }
            if f_name.starts_with('.') {
                f_name.remove(0);
            }
            let f_val = self.field_values.pop().unwrap_or_default();
            if self.fields.is_none() {
                return; // (NullPointerException in the Java)
            }
            if tag == "value" {
                let old = self.fields.as_mut().unwrap().put(&f_name, f_val.clone());
                if let Some(old) = old {
                    let mut l = self.list_fields.get(&f_name).cloned().unwrap_or_else(|| vec![old]);
                    l.push(f_val);
                    self.list_fields.put(&f_name, l);
                }
            } else {
                self.fields_rich_text.put(&f_name, f_val);
            }
        } else if tag == "field" {
            self.field_names.pop();
        }
    }

    fn text(&mut self, s: &str) {
        if self.field_names.is_empty() || self.field_values.is_empty() {
            return;
        }
        self.field_values.last_mut().unwrap().push_str(s);
    }
}

/// SimpleXMLParser.parse(doc, null, reader, false)
fn parse(h: &mut Handler, input: &[char]) -> Result<(), String> {
    fn pop_mode(st: &mut Vec<Mode>) -> Mode {
        st.pop().unwrap_or(Mode::Pre)
    }
    let mut st: Vec<Mode> = Vec::new();
    let mut depth = 0i32;
    let mut mode = Mode::Pre;
    let mut quotec = '"';
    let mut sb = String::new();
    let mut etag = String::new();
    let mut tag_name: Option<String> = None;
    let mut lvalue = String::new();
    let mut attrs: HashMap<String, String> = HashMap::new();
    let (mut line, mut col) = (1, 0);
    let mut eol = false;
    let mut push_back: Option<char> = None;
    let mut i = 0usize;
    loop {
        let mut c = if let Some(p) = push_back.take() {
            p
        } else if i < input.len() {
            i += 1;
            input[i - 1]
        } else {
            break;
        };
        // We need to map \r, \r\n, and \n to \n
        if c == '\n' && eol {
            eol = false;
            continue;
        } else if eol {
            eol = false;
        } else if c == '\n' {
            line += 1;
            col = 0;
        } else if c == '\r' {
            eol = true;
            c = '\n';
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        let exc = |s: &str| -> String { format!("{} near line {}, column {}", s, line, col) };

        match mode {
            Mode::Done => return Ok(()),
            Mode::Text => {
                if c == '<' {
                    st.push(mode);
                    mode = Mode::StartTag;
                    if !sb.is_empty() {
                        h.text(&sb);
                        sb.clear();
                    }
                } else if c == '&' {
                    st.push(mode);
                    mode = Mode::Entity;
                    etag.clear();
                } else {
                    sb.push(c);
                }
            }
            Mode::CloseTag => {
                if c == '>' {
                    mode = pop_mode(&mut st);
                    let name = std::mem::take(&mut sb);
                    depth -= 1;
                    if depth == 0 {
                        mode = Mode::Done;
                    }
                    h.end_element(&name);
                } else if !is_ws(c) {
                    sb.push(c);
                }
            }
            Mode::Cdata => {
                if c == '>' && sb.ends_with("]]") {
                    sb.truncate(sb.len() - 2);
                    h.text(&sb);
                    sb.clear();
                    mode = pop_mode(&mut st);
                } else {
                    sb.push(c);
                }
            }
            Mode::Comment => {
                if c == '>' && sb.ends_with("--") {
                    sb.clear();
                    mode = pop_mode(&mut st);
                } else {
                    sb.push(c);
                }
            }
            Mode::Pre => {
                if c == '<' {
                    st.push(Mode::Text);
                    mode = Mode::StartTag;
                }
            }
            Mode::Doctype => {
                if c == '>' {
                    mode = pop_mode(&mut st);
                    if mode == Mode::Text {
                        mode = Mode::Pre;
                    }
                }
            }
            Mode::StartTag => {
                mode = pop_mode(&mut st);
                if c == '/' {
                    st.push(mode);
                    mode = Mode::CloseTag;
                } else if c == '?' {
                    mode = Mode::Doctype;
                } else {
                    st.push(mode);
                    mode = Mode::OpenTag;
                    tag_name = None;
                    attrs = HashMap::new();
                    sb.push(c);
                }
            }
            Mode::Entity => {
                if c == ';' {
                    mode = pop_mode(&mut st);
                    let cent = std::mem::take(&mut etag);
                    let num = if let Some(hex) = cent.strip_prefix("#x") {
                        Some(i32::from_str_radix(hex, 16).ok())
                    } else if let Some(dec) = cent.strip_prefix('#') {
                        Some(dec.parse::<i32>().ok())
                    } else {
                        None
                    };
                    match num {
                        Some(Some(v)) => {
                            // (char) cast: keep the low 16 bits
                            sb.push(char::from_u32((v as u32) & 0xffff).unwrap_or('\u{fffd}'));
                        }
                        Some(None) => {
                            sb.push('&');
                            sb.push_str(&cent);
                            sb.push(';');
                        }
                        None => match entity_map().get(&cent) {
                            Some(ce) => sb.push(*ce),
                            None => {
                                sb.push('&');
                                sb.push_str(&cent);
                                sb.push(';');
                            }
                        },
                    }
                } else if (c != '#' && !c.is_ascii_digit() && !c.is_ascii_lowercase() && !c.is_ascii_uppercase())
                    || etag.chars().count() >= 7
                {
                    mode = pop_mode(&mut st);
                    push_back = Some(c);
                    sb.push('&');
                    sb.push_str(&etag);
                    etag.clear();
                } else {
                    etag.push(c);
                }
            }
            Mode::SingleTag => {
                let name = tag_name.take().unwrap_or_else(|| sb.clone());
                if c != '>' {
                    return Err(exc(&format!("Expected > for tag: <{}/>", name)));
                }
                h.start_element(&name, &attrs)?;
                h.end_element(&name);
                if depth == 0 {
                    return Ok(());
                }
                sb.clear();
                attrs = HashMap::new();
                mode = pop_mode(&mut st);
            }
            Mode::OpenTag => {
                if c == '>' {
                    let name = tag_name.take().unwrap_or_else(|| sb.clone());
                    sb.clear();
                    depth += 1;
                    h.start_element(&name, &attrs)?;
                    attrs = HashMap::new();
                    mode = pop_mode(&mut st);
                } else if c == '/' {
                    mode = Mode::SingleTag;
                } else if c == '-' && sb == "!-" {
                    mode = Mode::Comment;
                    sb.clear();
                } else if c == '[' && sb == "![CDATA" {
                    mode = Mode::Cdata;
                    sb.clear();
                } else if c == 'E' && sb == "!DOCTYP" {
                    sb.clear();
                    mode = Mode::Doctype;
                } else if is_ws(c) {
                    tag_name = Some(std::mem::take(&mut sb));
                    mode = Mode::InTag;
                } else {
                    sb.push(c);
                }
            }
            Mode::Quote => {
                if c == quotec {
                    attrs.insert(lvalue.clone(), std::mem::take(&mut sb));
                    mode = Mode::InTag;
                } else if " \r\n\t".contains(c) {
                    sb.push(' ');
                } else if c == '&' {
                    st.push(mode);
                    mode = Mode::Entity;
                    etag.clear();
                } else {
                    sb.push(c);
                }
            }
            Mode::AttributeRvalue => {
                if c == '"' || c == '\'' {
                    quotec = c;
                    mode = Mode::Quote;
                } else if is_ws(c) {
                } else {
                    return Err(exc("Error in attribute processing"));
                }
            }
            Mode::AttributeLvalue => {
                if is_ws(c) {
                    lvalue = std::mem::take(&mut sb);
                    mode = Mode::AttributeEqual;
                } else if c == '=' {
                    lvalue = std::mem::take(&mut sb);
                    mode = Mode::AttributeRvalue;
                } else {
                    sb.push(c);
                }
            }
            Mode::AttributeEqual => {
                if c == '=' {
                    mode = Mode::AttributeRvalue;
                } else if is_ws(c) {
                } else {
                    return Err(exc("Error in attribute processing."));
                }
            }
            Mode::InTag => {
                if c == '>' {
                    mode = pop_mode(&mut st);
                    let name = tag_name.take().unwrap_or_default();
                    h.start_element(&name, &attrs)?;
                    depth += 1;
                    attrs = HashMap::new();
                } else if c == '/' {
                    mode = Mode::SingleTag;
                } else if is_ws(c) {
                } else {
                    mode = Mode::AttributeLvalue;
                    sb.push(c);
                }
            }
        }
    }
    if mode == Mode::Done {
        Ok(())
    } else {
        Err(format!("missing end tag near line {}, column {}", line, col))
    }
}

pub fn read_xfdf(bytes: &[u8]) -> Result<FormData, String> {
    // SimpleXMLParser.parse(doc, InputStream): the first four bytes are consumed by the encoding
    // sniffer and, for UTF-8, so is everything up to the first '>' (assumed to be the XML
    // declaration). An XFDF file without a declaration therefore loses its root tag, as in Java.
    if bytes.len() < 4 {
        return Err("Insufficient length.".into());
    }
    let mut encoding = encoding_name(&bytes[..4]).to_string();
    let mut rest = &bytes[4..];
    if encoding == "UTF-8" {
        let end = rest.iter().position(|c| *c == b'>');
        let decl: String = rest[..end.unwrap_or(rest.len())].iter().map(|b| *b as char).collect();
        rest = match end {
            Some(e) => &rest[e + 1..],
            None => &[],
        };
        if let Some(d) = declared_encoding(&decl) {
            encoding = d;
        }
    } else if encoding == "CP037" {
        return Err("EBCDIC XML is not supported".into());
    }
    let chars = decode(rest, &encoding)?;
    let mut h = Handler::default();
    parse(&mut h, &chars)?;

    let mut data = FormData::default();
    let fields = match &h.fields {
        Some(f) => f,
        None => return Ok(data), // no <fields> element (NullPointerException in the Java)
    };
    for (name, value) in fields.iter() {
        data.names.push(name.to_string());
        data.fields.push((name.to_string(), value.clone()));
        if let Some(rv) = h.fields_rich_text.get(name) {
            data.rich_values.push((name.to_string(), rv.clone()));
        }
        if let Some(l) = h.list_fields.get(name) {
            data.multi_values.push((name.to_string(), l.clone()));
        }
    }
    data.has_rich_text = !data.rich_values.is_empty();
    Ok(data)
}
