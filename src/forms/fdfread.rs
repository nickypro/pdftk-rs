//! Port of iText FdfReader (with PdfReader.rebuildXref()/readDocObj() for the FDF container).
use super::syntax::{is_whitespace, Lexer, Token};
use super::util::{name_to_string, text, JavaHashMap};
use super::FormData;
use lopdf::{Dictionary, Object};
use std::collections::HashMap;

struct Fdf {
    objects: HashMap<u32, Object>,
}

impl Fdf {
    /// PdfReader.getPdfObject()
    fn res<'a>(&'a self, obj: Option<&'a Object>) -> Option<&'a Object> {
        let mut cur = obj?;
        for _ in 0..64 {
            match cur {
                Object::Reference((n, _)) => cur = self.objects.get(n)?,
                Object::Null => return None,
                _ => return Some(cur),
            }
        }
        None
    }
    fn get<'a>(&'a self, d: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
        self.res(d.get(key).ok())
    }
    fn get_dict<'a>(&'a self, d: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
        match self.get(d, key) {
            Some(Object::Dictionary(x)) => Some(x),
            _ => None,
        }
    }
}

/// PRTokeniser.readLineSegment() with a 64-byte buffer: returns the line (without the " X"
/// sentinel) or None at end of input.
fn read_line_segment(data: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    let mut c: Option<u8>;
    loop {
        c = data.get(*pos).copied();
        if c.is_some() {
            *pos += 1;
        }
        match c {
            Some(ch) if is_whitespace(ch) => continue,
            _ => break,
        }
    }
    loop {
        match c {
            None | Some(b'\n') => break,
            Some(b'\r') => {
                if data.get(*pos) == Some(&b'\n') {
                    *pos += 1;
                }
                break;
            }
            Some(ch) => {
                if line.len() < 64 {
                    line.push(ch);
                }
            }
        }
        c = data.get(*pos).copied();
        if c.is_some() {
            *pos += 1;
        }
    }
    if c.is_none() && line.is_empty() {
        return None;
    }
    Some(line)
}

/// PRTokeniser.checkObjectStart()
fn check_object_start(line: &[u8]) -> Option<(u32, i64)> {
    let mut lx = Lexer::new(line);
    let num = match lx.next_token() {
        Ok(Some(Token::Number(s))) => s.parse::<i64>().ok()?,
        _ => return None,
    };
    let gen = match lx.next_token() {
        Ok(Some(Token::Number(s))) => s.parse::<i64>().ok()?,
        _ => return None,
    };
    match lx.next_token() {
        Ok(Some(t)) if t.string_value() == "obj" => {}
        _ => return None,
    }
    if !(0..=10_000_000).contains(&num) {
        return None;
    }
    Some((num as u32, gen))
}

fn parse_container(bytes: &[u8]) -> Result<(Fdf, Dictionary), String> {
    // checkFdfHeader()
    let head = &bytes[..bytes.len().min(1024)];
    let idx = head.windows(8).position(|w| w == b"%FDF-1.2").ok_or("FDF header signature not found.")?;
    let data = &bytes[idx..];

    // rebuildXref()
    let mut xr: HashMap<u32, (usize, i64)> = HashMap::new();
    let mut trailer: Option<Dictionary> = None;
    let mut pos = 0usize;
    loop {
        let start = pos;
        let line = match read_line_segment(data, &mut pos) {
            Some(l) => l,
            None => break,
        };
        if line.first() == Some(&b't') {
            if !line.starts_with(b"trailer") {
                continue;
            }
            let mut lx = Lexer::new(data);
            lx.pos = start;
            let _ = lx.next_token();
            pos = lx.pos;
            if let Ok(Object::Dictionary(dic)) = lx.read_object() {
                if dic.has(b"Root") {
                    trailer = Some(dic);
                    pos = lx.pos;
                }
            }
        } else if line.first().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            if let Some((num, gen)) = check_object_start(&line) {
                let replace = match xr.get(&num) {
                    None => true,
                    Some((_, g)) => gen >= *g,
                };
                if replace {
                    xr.insert(num, (start, gen));
                }
            }
        }
    }
    let trailer = trailer.ok_or("trailer not found.")?;

    // readDocObj()
    let mut objects = HashMap::new();
    for (num, (start, _)) in xr.iter() {
        if *num == 0 || *start == 0 {
            continue;
        }
        let mut lx = Lexer::new(data);
        lx.pos = *start;
        for _ in 0..3 {
            let _ = lx.next_valid_token();
        }
        if let Ok(obj) = lx.read_object() {
            objects.insert(*num, obj);
        }
    }
    Ok((Fdf { objects }, trailer))
}

/// FdfReader.kidNode()
fn kid_node(fdf: &Fdf, mut merged: Dictionary, name: String, fields: &mut JavaHashMap<Dictionary>, depth: usize) {
    let kids: Option<Vec<Object>> = match fdf.get(&merged, b"Kids") {
        Some(Object::Array(a)) if !a.is_empty() => Some(a.clone()),
        _ => None,
    };
    match kids {
        None => {
            let name: String = name.chars().skip(1).collect();
            fields.put(&name, merged);
        }
        Some(kids) => {
            if depth > 200 {
                return;
            }
            merged.remove(b"Kids");
            for kid in &kids {
                let new_dic = match fdf.res(Some(kid)) {
                    Some(Object::Dictionary(d)) => d,
                    _ => continue, // (the Java dies with a NullPointerException here)
                };
                let mut dic = merged.clone();
                let mut new_name = name.clone();
                if let Some(Object::String(t, _)) = fdf.get(new_dic, b"T") {
                    new_name.push('.');
                    new_name.push_str(&text(t));
                }
                for (k, v) in new_dic.iter() {
                    dic.set(k.clone(), v.clone());
                }
                dic.remove(b"T");
                kid_node(fdf, dic, new_name, fields, depth + 1);
            }
        }
    }
}

/// FdfReader.decodeString()
fn decode_string(encoding: Option<&[u8]>, b: &[u8]) -> String {
    let enc = match encoding {
        None => return text(b),
        Some(e) => e,
    };
    if b.len() >= 2 && b[0] == 254 && b[1] == 255 {
        return text(b);
    }
    match enc {
        b"utf_8" => String::from_utf8_lossy(b).into_owned(),
        b"utf_16" => {
            // Java "UTF-16": honour a BOM, default big-endian
            let (le, body) = if b.len() >= 2 && b[0] == 0xff && b[1] == 0xfe { (true, &b[2..]) } else { (false, b) };
            let units: Vec<u16> = body
                .chunks(2)
                .map(|c| {
                    if c.len() < 2 {
                        0xfffd
                    } else if le {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    }
                })
                .collect();
            String::from_utf16_lossy(&units)
        }
        // Shift-JIS, UHC, GBK and BigFive need legacy CJK decoders that are not ported:
        // such strings are read like unmarked ones (PDFDocEncoding).
        _ => text(b),
    }
}

pub fn read_fdf(bytes: &[u8]) -> Result<FormData, String> {
    let (fdf, trailer) = parse_container(bytes)?;
    // readFields()
    let catalog = fdf.get_dict(&trailer, b"Root");
    let fdf_dict = catalog.and_then(|c| fdf.get_dict(c, b"FDF")).ok_or("Invalid FDF catalog.")?;
    let mut fields: JavaHashMap<Dictionary> = JavaHashMap::new();
    let mut encoding: Option<Vec<u8>> = None;
    if let Some(fld @ Object::Array(_)) = fdf.get(fdf_dict, b"Fields") {
        if let Some(Object::Name(n)) = fdf.get(fdf_dict, b"Encoding") {
            encoding = Some(n.clone());
        }
        let mut merged = Dictionary::new();
        merged.set("Kids", fld.clone());
        kid_node(&fdf, merged, String::new(), &mut fields, 0);
    }
    let enc = encoding.as_deref();

    let mut data = FormData::default();
    for (name, field) in fields.iter() {
        data.names.push(name.to_string());
        // getFieldValue()
        let v = fdf.get(field, b"V");
        let value = match v {
            Some(Object::Name(n)) => Some(name_to_string(n)),
            Some(Object::String(s, _)) => Some(decode_string(enc, s)),
            _ => None,
        };
        // getFieldRichValue()
        match fdf.get(field, b"RV") {
            Some(Object::Name(n)) => data.rich_values.push((name.to_string(), name_to_string(n))),
            Some(Object::String(s, _)) => data.rich_values.push((name.to_string(), text(s))),
            _ => {}
        }
        // getFieldMultiValue()
        match v {
            Some(Object::Array(vv)) => {
                let values: Vec<String> = vv
                    .iter()
                    .filter_map(|vi| match vi {
                        Object::String(s, _) => Some(decode_string(enc, s)),
                        _ => None,
                    })
                    .collect();
                data.multi_values.push((name.to_string(), values));
            }
            _ => {
                if let Some(single) = &value {
                    data.multi_values.push((name.to_string(), vec![single.clone()]));
                }
            }
        }
        if let Some(value) = value {
            data.fields.push((name.to_string(), value));
        }
    }
    data.has_rich_text = !data.rich_values.is_empty();
    Ok(data)
}
