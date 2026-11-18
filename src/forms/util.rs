//! Small shared helpers for the forms code.
use crate::pdf;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// PdfReader.getPdfObject(): follow references. None if `obj` is None or the reference dangles.
/// A direct PDF null stays `Some(Object::Null)` like iText's PdfNull.
pub fn res<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
    pdf::resolve(doc, obj?)
}

/// dict.get(key) resolved; PDF null counts as absent (PdfDictionary.put(key, PdfNull) removes).
pub fn dget<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    match res(doc, dict.get(key).ok()) {
        Some(Object::Null) => None,
        o => o,
    }
}

/// PdfDictionary.getAsDict(): streams do not count.
pub fn get_as_dict<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    match dget(doc, dict, key) {
        Some(Object::Dictionary(d)) => Some(d),
        _ => None,
    }
}

pub fn get_as_array<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Vec<Object>> {
    match dget(doc, dict, key) {
        Some(Object::Array(a)) => Some(a),
        _ => None,
    }
}

pub fn get_as_name<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    match dget(doc, dict, key) {
        Some(Object::Name(n)) => Some(n.as_slice()),
        _ => None,
    }
}

pub fn get_as_string<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    match dget(doc, dict, key) {
        Some(Object::String(s, _)) => Some(s.as_slice()),
        _ => None,
    }
}

pub fn is_number(obj: &Object) -> bool {
    matches!(obj, Object::Integer(_) | Object::Real(_))
}

/// PdfNumber.intValue()
pub fn int_value(obj: &Object) -> i32 {
    match obj {
        Object::Integer(i) => *i as i32,
        Object::Real(r) => *r as i32,
        _ => 0,
    }
}

/// PdfNumber.floatValue()
pub fn float_value(obj: &Object) -> f32 {
    match obj {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r as f32,
        _ => 0.0,
    }
}

pub fn get_as_int(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<i32> {
    dget(doc, dict, key).filter(|o| is_number(o)).map(int_value)
}

pub fn get_as_float(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<f32> {
    dget(doc, dict, key).filter(|o| is_number(o)).map(float_value)
}

/// A decoded PDF name as a Java String (PdfName.decodeName): one char per byte.
pub fn name_to_string(name: &[u8]) -> String {
    name.iter().map(|b| *b as char).collect()
}

/// `new PdfName(String)`: iText keeps only the low byte of every UTF-16 unit.
pub fn string_to_name(s: &str) -> Vec<u8> {
    s.encode_utf16().map(|u| (u & 0xff) as u8).collect()
}

/// PdfString.toUnicodeString() of a string object's bytes.
pub fn text(bytes: &[u8]) -> String {
    pdf::text_string_to_unicode(bytes)
}

/// PdfReader.getNormalizedRectangle(): [llx, lly, urx, ury].
pub fn normalized_rect(doc: &Document, obj: Option<&Object>) -> Option<[f32; 4]> {
    let arr = match res(doc, obj) {
        Some(Object::Array(a)) if a.len() >= 4 => a,
        _ => return None,
    };
    let mut v = [0f32; 4];
    for i in 0..4 {
        v[i] = res(doc, Some(&arr[i])).map(float_value).unwrap_or(0.0);
    }
    Some([v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
}

pub fn dict_of(doc: &Document, id: ObjectId) -> Option<&Dictionary> {
    match doc.objects.get(&id) {
        Some(Object::Dictionary(d)) => Some(d),
        Some(Object::Stream(s)) => Some(&s.dict),
        _ => None,
    }
}

pub fn dict_of_mut(doc: &mut Document, id: ObjectId) -> Option<&mut Dictionary> {
    match doc.objects.get_mut(&id) {
        Some(Object::Dictionary(d)) => Some(d),
        Some(Object::Stream(s)) => Some(&mut s.dict),
        _ => None,
    }
}

/// Follow a chain of references to the id of the final (non-reference) object.
pub fn final_id(doc: &Document, obj: &Object) -> Option<ObjectId> {
    let mut cur = obj;
    let mut last = None;
    for _ in 0..64 {
        match cur {
            Object::Reference(id) => {
                last = Some(*id);
                cur = doc.objects.get(id)?;
            }
            _ => return last,
        }
    }
    None
}

/// java.lang.String.hashCode()
pub fn java_string_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// Emulation of `java.util.HashMap<String, V>` iteration order (OpenJDK 8+): buckets indexed by
/// the spread hash, insertion order inside a bucket, table doubling at load factor 0.75 and when
/// a bin grows past 8 entries in a table smaller than 64. (Bins that Java would turn into trees
/// are kept as lists; a tree bin can iterate in a slightly different order.)
#[derive(Debug, Clone)]
pub struct JavaHashMap<V> {
    buckets: Vec<Vec<(String, i32, V)>>,
    size: usize,
}

impl<V> JavaHashMap<V> {
    pub fn new() -> Self {
        JavaHashMap { buckets: Vec::new(), size: 0 }
    }
    fn index(hash: i32, cap: usize) -> usize {
        let h = hash as u32;
        ((h ^ (h >> 16)) as usize) & (cap - 1)
    }
    fn resize(&mut self) {
        let new_cap = if self.buckets.is_empty() { 16 } else { self.buckets.len() * 2 };
        let old = std::mem::take(&mut self.buckets);
        let mut nb: Vec<Vec<(String, i32, V)>> = (0..new_cap).map(|_| Vec::new()).collect();
        for b in old {
            for e in b {
                let i = Self::index(e.1, new_cap);
                nb[i].push(e);
            }
        }
        self.buckets = nb;
    }
    /// Returns the previous value, like Map.put().
    pub fn put(&mut self, key: &str, value: V) -> Option<V> {
        if self.buckets.is_empty() {
            self.resize();
        }
        let hash = java_string_hash(key);
        let i = Self::index(hash, self.buckets.len());
        if let Some(e) = self.buckets[i].iter_mut().find(|e| e.0 == key) {
            return Some(std::mem::replace(&mut e.2, value));
        }
        let had = self.buckets[i].len();
        self.buckets[i].push((key.to_string(), hash, value));
        if had >= 8 && self.buckets.len() < 64 {
            self.resize(); // treeifyBin() on a small table resizes instead
        }
        self.size += 1;
        if self.size > self.buckets.len() * 3 / 4 {
            self.resize();
        }
        None
    }
    pub fn get(&self, key: &str) -> Option<&V> {
        if self.buckets.is_empty() {
            return None;
        }
        let i = Self::index(java_string_hash(key), self.buckets.len());
        self.buckets[i].iter().find(|e| e.0 == key).map(|e| &e.2)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        if self.buckets.is_empty() {
            return None;
        }
        let i = Self::index(java_string_hash(key), self.buckets.len());
        self.buckets[i].iter_mut().find(|e| e.0 == key).map(|e| &mut e.2)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.buckets.iter().flat_map(|b| b.iter().map(|e| (e.0.as_str(), &e.2)))
    }
    pub fn keys(&self) -> Vec<String> {
        self.iter().map(|(k, _)| k.to_string()).collect()
    }
}

/// iText ByteBuffer.formatDouble() as used when appending floats to content streams.
pub fn format_double(d: f64) -> String {
    let mut d = d;
    if d.abs() < 0.000015 {
        return "0".into();
    }
    let mut res = String::new();
    let negative = d < 0.0;
    if negative {
        d = -d;
    }
    if d < 1.0 {
        d += 0.000005;
        if d >= 1.0 {
            return if negative { "-1".into() } else { "1".into() };
        }
        let v = (d * 100000.0) as i32;
        if negative {
            res.push('-');
        }
        res.push_str("0.");
        res.push((b'0' + (v / 10000) as u8) as char);
        if v % 10000 != 0 {
            res.push((b'0' + ((v / 1000) % 10) as u8) as char);
            if v % 1000 != 0 {
                res.push((b'0' + ((v / 100) % 10) as u8) as char);
                if v % 100 != 0 {
                    res.push((b'0' + ((v / 10) % 10) as u8) as char);
                    if v % 10 != 0 {
                        res.push((b'0' + (v % 10) as u8) as char);
                    }
                }
            }
        }
        res
    } else if d <= 32767.0 {
        d += 0.005;
        let v = (d * 100.0) as i32;
        if negative {
            res.push('-');
        }
        res.push_str(&(v / 100).to_string());
        if v % 100 != 0 {
            res.push('.');
            res.push((b'0' + ((v / 10) % 10) as u8) as char);
            if v % 10 != 0 {
                res.push((b'0' + (v % 10) as u8) as char);
            }
        }
        res
    } else {
        if negative {
            res.push('-');
        }
        d += 0.5;
        res.push_str(&(d as i64).to_string());
        res
    }
}

/// ByteBuffer.append(float)
pub fn fmt(v: f32) -> String {
    format_double(v as f64)
}

/// PdfContentByte.escapeString()
pub fn escape_string(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + 2);
    out.push(b'(');
    for &c in b {
        match c {
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push(b')');
    out
}

impl<V> Default for JavaHashMap<V> {
    fn default() -> Self {
        Self::new()
    }
}
