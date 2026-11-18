//! Port of the read side of iText AcroFields: field discovery (fill()), getField(), exportAsFdf().
use super::fdfwrite::FdfWriter;
use super::util::*;
use crate::pdf;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// The AcroForm dictionary of the catalog (reader.getAcroForm() != null test).
pub fn acro_form_dict(doc: &Document) -> Option<&Dictionary> {
    let catalog = doc.catalog().ok()?;
    get_as_dict(doc, catalog, b"AcroForm")
}

/// PdfReader.getAcroForm() != null. iText builds a PRAcroForm by walking /Fields and gives up
/// (returns null) on any exception, so this is stricter than "the catalog has /AcroForm":
/// /Fields must be an array, every indirect entry must lead to a dictionary whose /T, if
/// present, is a direct string, and so on down the /Kids.
pub fn reader_get_acro_form(doc: &Document) -> bool {
    fn iterate(doc: &Document, list: &[Object], depth: usize) -> bool {
        if depth > 200 {
            return false; // cyclic /Kids (a StackOverflowError in the Java)
        }
        for refobj in list {
            if !matches!(refobj, Object::Reference(_)) {
                continue;
            }
            let dict = match res(doc, Some(refobj)) {
                Some(Object::Dictionary(d)) => d,
                Some(Object::Stream(s)) => &s.dict,
                _ => return false, // NullPointerException / ClassCastException
            };
            match dict.get(b"T") {
                Ok(Object::String(..)) | Err(_) => {}
                Ok(_) => return false, // ClassCastException
            }
            if let Some(kids) = get_as_array(doc, dict, b"Kids") {
                if !iterate(doc, kids, depth + 1) {
                    return false;
                }
            }
        }
        true
    }
    let catalog = match doc.catalog() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let form = match catalog.get(b"AcroForm") {
        Ok(f) => f,
        Err(_) => return false,
    };
    let root = match res(doc, Some(form)) {
        None => return true, // readAcroForm(null): an empty form
        Some(Object::Dictionary(d)) => d,
        Some(Object::Stream(s)) => &s.dict,
        Some(_) => return false,
    };
    match res(doc, root.get(b"Fields").ok()) {
        Some(Object::Array(a)) => iterate(doc, a, 0),
        _ => false,
    }
}

/// Where a dictionary that iText holds by reference lives in our document.
#[derive(Debug, Clone)]
pub enum Loc {
    /// an indirect object
    Id(ObjectId),
    /// a direct dictionary (copied; cannot be written back)
    Direct(Dictionary),
}

impl Loc {
    pub fn dict<'a>(&'a self, doc: &'a Document) -> Option<&'a Dictionary> {
        match self {
            Loc::Id(id) => dict_of(doc, *id),
            Loc::Direct(d) => Some(d),
        }
    }
    pub fn id(&self) -> Option<ObjectId> {
        match self {
            Loc::Id(id) => Some(*id),
            Loc::Direct(_) => None,
        }
    }
}

/// AcroFields.Item
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub values: Vec<Loc>,
    pub widgets: Vec<Loc>,
    pub widget_refs: Vec<Option<ObjectId>>,
    pub merged: Vec<Dictionary>,
    pub page: Vec<i32>,
    pub tab_order: Vec<i32>,
}

impl Item {
    pub fn size(&self) -> usize {
        self.values.len()
    }
}

pub struct AcroFields {
    pub fields: JavaHashMap<Item>,
    /// (value dictionary, /V object) pairs that fill() writes into the document as a side effect:
    /// a named field without /V inherits the /V of a nameless descendant on the path to the widget.
    pub pending_v: Vec<(ObjectId, Object)>,
}

fn loc_of(doc: &Document, obj: &Object) -> Option<Loc> {
    match obj {
        Object::Reference(_) => {
            let id = final_id(doc, obj)?;
            dict_of(doc, id).map(|_| Loc::Id(id))
        }
        Object::Dictionary(d) => Some(Loc::Direct(d.clone())),
        _ => None,
    }
}

/// PdfDictionary.mergeDifferent(): add the keys of `other` that `dic` lacks.
fn merge_different(dic: &mut Dictionary, other: &Dictionary) {
    for (k, v) in other.iter() {
        if !dic.has(k) {
            dic.set(k.clone(), v.clone());
        }
    }
}

impl AcroFields {
    /// AcroFields.fill()
    pub fn new(doc: &Document) -> AcroFields {
        let mut af = AcroFields { fields: JavaHashMap::new(), pending_v: Vec::new() };
        let top = match acro_form_dict(doc) {
            Some(t) => t,
            None => return af,
        };
        let arrfds = match get_as_array(doc, top, b"Fields") {
            Some(a) if !a.is_empty() => a,
            _ => return af,
        };
        let pages = pdf::page_ids(doc);
        for (k0, page_id) in pages.iter().enumerate() {
            let k = k0 as i32 + 1;
            let page = match dict_of(doc, *page_id) {
                Some(p) => p,
                None => continue,
            };
            let annots = match get_as_array(doc, page, b"Annots") {
                Some(a) => a,
                None => continue,
            };
            for (j, annot_obj) in annots.iter().enumerate() {
                let widget_loc = match loc_of(doc, annot_obj) {
                    Some(l) => l,
                    None => continue,
                };
                let widget = widget_loc.dict(doc).unwrap();
                if get_as_name(doc, widget, b"Subtype") != Some(b"Widget") {
                    continue;
                }
                let mut dic = widget.clone();
                let mut name = String::new();
                let mut value: Option<Loc> = None;
                let mut last_v: Option<Object> = None;
                let mut cur: Option<Loc> = Some(widget_loc.clone());
                let mut depth = 0;
                while let Some(loc) = cur {
                    let annot = loc.dict(doc).unwrap();
                    // a /V that an earlier pass of this loop put() into this dictionary
                    let patched: Dictionary;
                    let annot = match loc.id().and_then(|id| af.pending_v.iter().find(|(i, _)| *i == id)) {
                        Some((_, v)) if !annot.has(b"V") => {
                            let mut d = annot.clone();
                            d.set("V", v.clone());
                            patched = d;
                            &patched
                        }
                        _ => annot,
                    };
                    merge_different(&mut dic, annot);
                    let t = get_as_string(doc, annot, b"T");
                    if let Some(t) = t {
                        name = format!("{}.{}", text(t), name);
                    }
                    if last_v.is_none() && annot.has(b"V") {
                        last_v = res(doc, annot.get(b"V").ok()).cloned();
                    }
                    if value.is_none() && t.is_some() {
                        if !annot.has(b"V") {
                            if let (Some(v), Some(id)) = (&last_v, loc.id()) {
                                af.pending_v.push((id, v.clone()));
                            }
                        }
                        value = Some(loc.clone());
                    }
                    depth += 1;
                    cur = if depth > 100 {
                        None // cyclic /Parent chain: the Java never terminates
                    } else {
                        annot.get(b"Parent").ok().and_then(|p| match res(doc, Some(p)) {
                            Some(Object::Dictionary(_)) => loc_of(doc, p),
                            _ => None,
                        })
                    };
                }
                if !name.is_empty() {
                    name.pop();
                }
                merge_different(&mut dic, top);
                if !af.fields.contains_key(&name) {
                    af.fields.put(&name, Item::default());
                }
                let item = af.fields.get_mut(&name).unwrap();
                item.values.push(value.unwrap_or_else(|| widget_loc.clone()));
                item.widgets.push(widget_loc.clone());
                item.widget_refs.push(match annot_obj {
                    Object::Reference(id) => Some(*id),
                    _ => None,
                });
                item.merged.push(dic);
                item.page.push(k);
                item.tab_order.push(j as i32);
            }
        }
        // some tools produce invisible signatures without an entry in the page annotation array
        // look for a single level annotation
        let sig_flags = get_as_int(doc, top, b"SigFlags");
        if sig_flags.map(|f| f & 1) != Some(1) {
            return af;
        }
        for fobj in arrfds {
            let loc = match loc_of(doc, fobj) {
                Some(l) => l,
                None => continue,
            };
            let annot = loc.dict(doc).unwrap();
            if get_as_name(doc, annot, b"Subtype") != Some(b"Widget") {
                continue;
            }
            if dget(doc, annot, b"Kids").is_some() {
                continue;
            }
            let name = match get_as_string(doc, annot, b"T") {
                Some(t) => text(t),
                None => continue,
            };
            if af.fields.contains_key(&name) {
                continue;
            }
            // iText registers a *copy* of the dictionary here (dic.putAll(annot)), so whatever
            // setField() later writes to this item never reaches the document.
            let loc = Loc::Direct(annot.clone());
            let mut item = Item::default();
            item.values.push(loc.clone());
            item.widgets.push(loc.clone());
            item.widget_refs.push(match fobj {
                Object::Reference(id) => Some(*id),
                _ => None,
            });
            item.merged.push(annot.clone());
            item.page.push(-1);
            item.tab_order.push(-1);
            af.fields.put(&name, item);
        }
        af
    }

    /// AcroFields.getField(): (value, lastWasString). None if the field does not exist.
    pub fn get_field(&self, doc: &Document, name: &str) -> Option<(String, bool)> {
        let item = self.fields.get(name)?;
        let merged = &item.merged[0];
        let v = res(doc, merged.get(b"V").ok());
        if let Some(Object::Stream(s)) = v {
            let bytes = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            return Some((String::from_utf8_lossy(&bytes).into_owned(), true));
        }
        if get_as_name(doc, merged, b"FT") == Some(b"Btn") {
            let flags = get_as_int(doc, merged, b"Ff").unwrap_or(0);
            if flags & FF_PUSHBUTTON != 0 {
                return Some((String::new(), false));
            }
            let mut value = match v {
                Some(Object::Name(n)) => name_to_string(n),
                Some(Object::String(s, _)) => text(s),
                _ => String::new(),
            };
            let mut last_was_string = false;
            let opts = item.values[0].dict(doc).and_then(|d| get_as_array(doc, d, b"Opt"));
            if let Some(opts) = opts {
                if let Some(idx) = super::dump::java_parse_int(&value) {
                    if idx >= 0 {
                        if let Some(Object::String(s, _)) = opts.get(idx as usize).and_then(|o| res(doc, Some(o))) {
                            value = text(s);
                            last_was_string = true;
                        }
                    }
                }
            }
            return Some((value, last_was_string));
        }
        Some(match v {
            Some(Object::String(s, _)) => (text(s), true),
            Some(Object::Name(n)) => (name_to_string(n), false),
            _ => (String::new(), true),
        })
    }

    /// AcroFields.exportAsFdf()
    pub fn export_as_fdf(&self, doc: &Document, writer: &mut FdfWriter) {
        for name in self.fields.keys() {
            if let Some((value, last_was_string)) = self.get_field(doc, &name) {
                if last_was_string {
                    writer.set_field_as_string(&name, &value);
                } else {
                    writer.set_field_as_name(&name, &value);
                }
            }
        }
    }
}

pub const FF_READ_ONLY: i32 = 1;
pub const FF_RADIO: i32 = 32768;
pub const FF_PUSHBUTTON: i32 = 65536;
pub const FF_MULTILINE: i32 = 4096;
pub const FF_PASSWORD: i32 = 8192;
pub const FF_COMBO: i32 = 131072;
pub const FF_EDIT: i32 = 262144;
pub const FF_MULTISELECT: i32 = 2097152;
pub const FF_COMB: i32 = 16777216;
pub const FLAGS_HIDDEN: i32 = 2;
pub const FLAGS_PRINT: i32 = 4;
pub const FLAGS_NOVIEW: i32 = 32;
