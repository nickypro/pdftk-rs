//! Port of the write side of iText AcroFields: setFields(), setField(), setListSelection(),
//! decodeGenericDictionary() and getAppearance(), run with setGenerateAppearances(true).
use super::acro::*;
use super::appearance::*;
use super::fonts::{std_field_font, Font};
use super::util::*;
use super::FormData;
use crate::pdf;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// AcroFields keeps a `merged` dictionary per widget and setField() gives a check box's merged
    /// /AS the new value even when the widget has no such appearance state (the widget itself gets
    /// /Off). PdfStamperImp.flatFields() later reads the merged /AS. Our flatten() rebuilds the
    /// field list from the document, so fill_form() leaves those overrides here:
    /// widget id -> (value written to /V, merged /AS).
    pub(super) static MERGED_AS: RefCell<HashMap<ObjectId, Vec<u8>>> = RefCell::new(HashMap::new());
}

/// Fonts created so far (AcroFields.extensionFonts / the writer's font table).
struct FontCache {
    doc_fonts: HashMap<ObjectId, Font>,
    builtin: HashMap<String, Font>,
    builtin_objs: HashMap<String, ObjectId>,
}

impl FontCache {
    fn document(&mut self, doc: &Document, id: ObjectId) -> Font {
        self.doc_fonts.entry(id).or_insert_with(|| Font::document(doc, id)).clone()
    }
    fn builtin(&mut self, name: &str) -> Option<Font> {
        if !self.builtin.contains_key(name) {
            self.builtin.insert(name.to_string(), Font::builtin(name)?);
        }
        self.builtin.get(name).cloned()
    }
    /// writer.addSimple(bf): one font object per built-in font
    fn font_ref(&mut self, doc: &mut Document, font: &Font) -> ObjectId {
        if let Some(id) = font.doc_ref {
            return id;
        }
        if let Some(id) = self.builtin_objs.get(&font.ps_name) {
            return *id;
        }
        let id = doc.add_object(Object::Dictionary(font.builtin_font_dict()));
        self.builtin_objs.insert(font.ps_name.clone(), id);
        id
    }
}

/// `new PdfNumber(float)`: the value as iText writes it (ByteBuffer.formatDouble()).
fn num(v: f32) -> Object {
    let s = fmt(v);
    match s.parse::<i64>() {
        Ok(i) => Object::Integer(i),
        Err(_) => Object::Real(s.parse::<f32>().unwrap_or(v)),
    }
}

/// `new PdfString(value, PdfObject.TEXT_UNICODE)`
fn unicode_string(s: &str) -> Object {
    let mut b = vec![0xfe, 0xff];
    for u in s.encode_utf16() {
        b.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(b, StringFormat::Literal)
}

/// AcroFields.decodeGenericDictionary()
fn decode_generic_dictionary(doc: &Document, merged: &Dictionary, tx: &mut TextField, cache: &mut FontCache) {
    // the text size and color
    if let Some(da) = default_appearance(doc, merged) {
        let (da_font, da_size, da_color, da_leading) = split_da_elements(&da);
        if let Some(s) = da_size {
            tx.font_size = s;
        }
        if let Some(c) = da_color {
            tx.text_color = Some(c);
        }
        if let Some(l) = da_leading {
            tx.text_leading = l;
        }
        if let Some(fname) = da_font {
            let font_dict = get_as_dict(doc, merged, b"DR").and_then(|dr| get_as_dict(doc, dr, b"Font"));
            if let Some(font_dict) = font_dict {
                match font_dict.get(&string_to_name(&fname)).ok() {
                    Some(po @ Object::Reference(_)) => {
                        // (no extension font: see fonts.rs)
                        if let Some(id) = final_id(doc, po) {
                            if dict_of(doc, id).is_some() {
                                tx.font = Some(cache.document(doc, id));
                            }
                        }
                    }
                    _ => {
                        if let Some(std) = std_field_font(&fname) {
                            if let Some(bf) = cache.builtin(std) {
                                tx.font = Some(bf);
                            }
                        }
                    }
                }
            }
        }
    }
    // rotation, border and background color
    if let Some(mk) = get_as_dict(doc, merged, b"MK") {
        let border = mk_color(doc, get_as_array(doc, mk, b"BC"));
        tx.border_color = border;
        if border.is_some() {
            tx.border_width = 1.0;
        }
        tx.background_color = mk_color(doc, get_as_array(doc, mk, b"BG"));
        if let Some(r) = get_as_int(doc, mk, b"R") {
            // BaseField.setRotation() (it rejects anything but multiples of 90)
            if r % 90 == 0 {
                let mut r = r % 360;
                if r < 0 {
                    r += 360;
                }
                tx.rotation = r;
            }
        }
    }
    // multiline
    let flags = get_as_int(doc, merged, b"Ff").unwrap_or(0);
    tx.options = flags;
    if (flags & FF_COMB) != 0 {
        tx.max_character_length = get_as_int(doc, merged, b"MaxLen").unwrap_or(0);
    }
    // alignment
    if let Some(q) = get_as_int(doc, merged, b"Q") {
        if q == 1 {
            tx.alignment = ALIGN_CENTER;
        } else if q == 2 {
            tx.alignment = ALIGN_RIGHT;
        }
    }
    // border styles
    if let Some(bs) = get_as_dict(doc, merged, b"BS") {
        if let Some(w) = get_as_float(doc, bs, b"W") {
            tx.border_width = w;
        }
        match get_as_name(doc, bs, b"S") {
            Some(b"D") => tx.border_style = STYLE_DASHED,
            Some(b"B") => tx.border_style = STYLE_BEVELED,
            Some(b"I") => tx.border_style = STYLE_INSET,
            Some(b"U") => tx.border_style = STYLE_UNDERLINE,
            _ => {}
        }
    } else if let Some(bd) = get_as_array(doc, merged, b"Border") {
        if bd.len() >= 3 {
            tx.border_width = res(doc, Some(&bd[2])).map(float_value).unwrap_or(0.0);
        }
        if bd.len() >= 4 {
            tx.border_style = STYLE_DASHED;
        }
    }
}

/// AcroFields.getAppearance(merged, values, fieldName): (appearance, topFirst)
fn get_appearance(doc: &Document, merged: &Dictionary, values: &[String], cache: &mut FontCache) -> Option<(Appearance, i32)> {
    let mut text: Option<String> = values.first().cloned();
    let mut tx = TextField::new();
    decode_generic_dictionary(doc, merged, &mut tx, cache);
    // rect
    let rect = normalized_rect(doc, merged.get(b"Rect").ok()).unwrap_or([0.0; 4]);
    let (mut w, mut h) = (rect[2] - rect[0], rect[3] - rect[1]);
    if tx.rotation == 90 || tx.rotation == 270 {
        std::mem::swap(&mut w, &mut h);
    }
    tx.box_w = w;
    tx.box_h = h;
    let field_type = get_as_name(doc, merged, b"FT");
    if field_type == Some(b"Tx") {
        tx.text = text;
        return Some((tx.get_appearance(), 0));
    }
    if field_type != Some(b"Ch") {
        // "An appearance was requested without a variable text field."
        return None;
    }
    let opt = get_as_array(doc, merged, b"Opt");
    let flags = get_as_int(doc, merged, b"Ff").unwrap_or(0);
    if (flags & FF_COMBO) != 0 && opt.is_none() {
        tx.text = text;
        return Some((tx.get_appearance(), 0));
    }
    if let Some(opt) = opt {
        let mut choices: Vec<String> = Vec::with_capacity(opt.len());
        let mut choices_exp: Vec<String> = Vec::with_capacity(opt.len());
        let s = |o: Option<&Object>| -> String {
            match res(doc, o) {
                Some(Object::String(b, _)) => super::util::text(b),
                _ => String::new(),
            }
        };
        for o in opt {
            match res(doc, Some(o)) {
                Some(Object::String(b, _)) => {
                    choices.push(super::util::text(b));
                    choices_exp.push(super::util::text(b));
                }
                Some(Object::Array(a)) => {
                    choices_exp.push(s(a.first()));
                    choices.push(s(a.get(1)));
                }
                _ => {
                    choices.push(String::new());
                    choices_exp.push(String::new());
                }
            }
        }
        if (flags & FF_COMBO) != 0 {
            if let Some(t) = &text {
                if let Some(k) = choices_exp.iter().position(|e| e == t) {
                    text = Some(choices[k].clone());
                }
            }
            tx.text = text;
            return Some((tx.get_appearance(), 0));
        }
        let mut indexes = Vec::new();
        for (k, e) in choices_exp.iter().enumerate() {
            if values.iter().any(|v| v == e) {
                indexes.push(k as i32);
            }
        }
        tx.choices = choices;
        tx.choice_exports = choices_exp;
        tx.set_choice_selections(indexes);
    }
    let app = tx.get_list_appearance();
    let top_first = tx.top_first;
    Some((app, top_first))
}

/// Turn a PdfAppearance into a form XObject in the document (what closing the stamper does).
fn add_appearance(doc: &mut Document, app: Appearance, cache: &mut FontCache) -> ObjectId {
    let mut resources = Dictionary::new();
    if !app.fonts.is_empty() {
        let mut fonts = Dictionary::new();
        for (name, font) in &app.fonts {
            let id = cache.font_ref(doc, font);
            fonts.set(name.clone(), Object::Reference(id));
        }
        resources.set("Font", Object::Dictionary(fonts));
    }
    resources.set(
        "ProcSet",
        Object::Array(
            [&b"PDF"[..], b"Text", b"ImageB", b"ImageC", b"ImageI"].iter().map(|n| Object::Name(n.to_vec())).collect(),
        ),
    );
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Form".to_vec()));
    dict.set("BBox", Object::Array(vec![num(0.0), num(0.0), num(app.width), num(app.height)]));
    dict.set("Matrix", Object::Array(app.matrix.iter().map(|v| num(*v)).collect()));
    dict.set("FormType", Object::Integer(1));
    dict.set("Resources", Object::Dictionary(resources));
    let mut stream = Stream::new(dict, app.content);
    let _ = stream.compress();
    doc.add_object(Object::Stream(stream))
}

/// The AcroForm dictionary, mutable, wherever it lives.
pub(super) fn acro_form_mut(doc: &mut Document) -> Option<&mut Dictionary> {
    let root = doc.trailer.get(b"Root").ok().and_then(|r| final_id(doc, r))?;
    let acro_ref = match dict_of(doc, root)?.get(b"AcroForm").ok()? {
        o @ Object::Reference(_) => Some(final_id(doc, o)?),
        Object::Dictionary(_) => None,
        _ => return None,
    };
    match acro_ref {
        Some(id) => dict_of_mut(doc, id),
        None => match dict_of_mut(doc, root)?.get_mut(b"AcroForm").ok()? {
            Object::Dictionary(d) => Some(d),
            _ => None,
        },
    }
}

/// iText always holds annotations by reference; give direct widget dictionaries in /Annots an
/// object of their own so they can be written to.
fn make_annots_indirect(doc: &mut Document) {
    for page_id in pdf::page_ids(doc) {
        let (holder, direct): (Option<ObjectId>, Vec<usize>) = {
            let page = match dict_of(doc, page_id) {
                Some(p) => p,
                None => continue,
            };
            let raw = match page.get(b"Annots") {
                Ok(r) => r,
                Err(_) => continue,
            };
            let holder = match raw {
                Object::Reference(_) => match final_id(doc, raw) {
                    Some(id) => Some(id),
                    None => continue,
                },
                _ => None,
            };
            let arr = match res(doc, Some(raw)) {
                Some(Object::Array(a)) => a,
                _ => continue,
            };
            let direct: Vec<usize> =
                arr.iter().enumerate().filter(|(_, o)| matches!(o, Object::Dictionary(_))).map(|(i, _)| i).collect();
            (holder, direct)
        };
        for i in direct {
            let take = |arr: &mut Vec<Object>| std::mem::replace(&mut arr[i], Object::Null);
            let d = match holder {
                Some(h) => match doc.objects.get_mut(&h) {
                    Some(Object::Array(a)) => take(a),
                    _ => continue,
                },
                None => match dict_of_mut(doc, page_id).and_then(|p| p.get_mut(b"Annots").ok()) {
                    Some(Object::Array(a)) => take(a),
                    _ => continue,
                },
            };
            let id = doc.add_object(d);
            let put = |arr: &mut Vec<Object>| arr[i] = Object::Reference(id);
            match holder {
                Some(h) => {
                    if let Some(Object::Array(a)) = doc.objects.get_mut(&h) {
                        put(a)
                    }
                }
                None => {
                    if let Some(Object::Array(a)) = dict_of_mut(doc, page_id).and_then(|p| p.get_mut(b"Annots").ok()) {
                        put(a)
                    }
                }
            }
        }
    }
}

struct Filler<'a> {
    doc: &'a mut Document,
    af: AcroFields,
    cache: FontCache,
}

impl<'a> Filler<'a> {
    fn put(&mut self, loc: &Loc, key: &[u8], value: Object) {
        if let Some(d) = loc.id().and_then(|id| dict_of_mut(self.doc, id)) {
            d.set(key.to_vec(), value);
        }
    }
    fn remove(&mut self, loc: &Loc, key: &[u8]) {
        if let Some(d) = loc.id().and_then(|id| dict_of_mut(self.doc, id)) {
            d.remove(key);
        }
    }

    /// AcroFields.isInAP()
    fn is_in_ap(&self, widget: &Loc, check: &[u8]) -> bool {
        let w = match widget.dict(self.doc) {
            Some(w) => w,
            None => return false,
        };
        get_as_dict(self.doc, w, b"AP")
            .and_then(|ap| get_as_dict(self.doc, ap, b"N"))
            .map(|n| n.has(check))
            .unwrap_or(false)
    }

    /// widget.getAsDict(AP).put(N, ref), creating /AP when the widget has none
    fn set_normal_appearance(&mut self, widget: &Loc, merged: &mut Dictionary, app_ref: ObjectId) {
        let wid = match widget.id() {
            Some(id) => id,
            None => return,
        };
        enum Where {
            Indirect(ObjectId),
            Direct,
            Missing,
        }
        let place = match dict_of(self.doc, wid).and_then(|w| w.get(b"AP").ok()) {
            Some(o @ Object::Reference(_)) => match res(self.doc, Some(o)) {
                Some(Object::Dictionary(_)) => Where::Indirect(final_id(self.doc, o).unwrap()),
                _ => Where::Missing,
            },
            Some(Object::Dictionary(_)) => Where::Direct,
            _ => Where::Missing,
        };
        match place {
            Where::Indirect(id) => {
                if let Some(d) = dict_of_mut(self.doc, id) {
                    d.set("N", Object::Reference(app_ref));
                }
            }
            Where::Direct => {
                if let Some(Object::Dictionary(d)) = dict_of_mut(self.doc, wid).and_then(|w| w.get_mut(b"AP").ok()) {
                    d.set("N", Object::Reference(app_ref));
                }
                if let Ok(Object::Dictionary(d)) = merged.get_mut(b"AP") {
                    d.set("N", Object::Reference(app_ref));
                }
            }
            Where::Missing => {
                let mut ap = Dictionary::new();
                ap.set("N", Object::Reference(app_ref));
                if let Some(w) = dict_of_mut(self.doc, wid) {
                    w.set("AP", Object::Dictionary(ap.clone()));
                }
                merged.set("AP", Object::Dictionary(ap));
            }
        }
    }

    /// AcroFields.setField(name, value, display, rich_value)
    fn set_field(&mut self, name: &str, value: &str, display: &str, rich_value: Option<&str>) -> bool {
        let mut item = match self.af.fields.get(name) {
            Some(i) => i.clone(),
            None => return false,
        };
        let ftype: Option<Vec<u8>> = get_as_name(self.doc, &item.merged[0], b"FT").map(|n| n.to_vec());
        let mut value: String = value.to_string();
        if ftype.as_deref() == Some(b"Tx") {
            let len = get_as_int(self.doc, &item.merged[0], b"MaxLen").unwrap_or(0);
            if len > 0 {
                let units: Vec<u16> = value.encode_utf16().collect();
                let n = (len as usize).min(units.len());
                value = String::from_utf16_lossy(&units[..n]);
            }
        }
        let result = match ftype.as_deref() {
            Some(b"Tx") | Some(b"Ch") => {
                let is_choice = ftype.as_deref() == Some(b"Ch");
                let v = unicode_string(&value);
                let rv = rich_value.map(unicode_string);
                for idx in 0..item.size() {
                    let value_loc = item.values[idx].clone();
                    let widget_loc = item.widgets[idx].clone();
                    self.put(&value_loc, b"V", v.clone());
                    self.remove(&value_loc, b"I");
                    item.merged[idx].remove(b"I");
                    item.merged[idx].set("V", v.clone());
                    if let Some(rv) = &rv {
                        self.put(&value_loc, b"RV", rv.clone());
                        item.merged[idx].set("RV", rv.clone());
                    }
                    if widget_loc.id().is_none() {
                        continue; // a detached copy (see AcroFields::new): nothing reaches the file
                    }
                    let app = get_appearance(self.doc, &item.merged[idx], &[display.to_string()], &mut self.cache);
                    if let Some((app, top_first)) = app {
                        if is_choice {
                            self.put(&widget_loc, b"TI", Object::Integer(top_first as i64));
                            item.merged[idx].set("TI", Object::Integer(top_first as i64));
                        }
                        let app_ref = add_appearance(self.doc, app, &mut self.cache);
                        let mut merged = std::mem::take(&mut item.merged[idx]);
                        self.set_normal_appearance(&widget_loc, &mut merged, app_ref);
                        item.merged[idx] = merged;
                    }
                }
                true
            }
            Some(b"Btn") => {
                let flags = get_as_int(self.doc, &item.merged[0], b"Ff").unwrap_or(0);
                if (flags & FF_PUSHBUTTON) != 0 {
                    return false;
                }
                let mut lopt: Vec<Option<String>> = Vec::new();
                if let Some(opts) = item.values[0].dict(self.doc).and_then(|d| get_as_array(self.doc, d, b"Opt")) {
                    for o in opts {
                        lopt.push(match res(self.doc, Some(o)) {
                            Some(Object::String(s, _)) => Some(text(s)),
                            _ => None,
                        });
                    }
                }
                let vt: Vec<u8> = match lopt.iter().position(|o| o.as_deref() == Some(value.as_str())) {
                    Some(vidx) => vidx.to_string().into_bytes(),
                    None => {
                        if value.encode_utf16().count() > 127 {
                            eprintln!("Warning: a form field has a value longer than 127 characters.");
                            eprintln!("The output will not be standard-compliant.");
                        }
                        string_to_name(&value)
                    }
                };
                let is_check_box = (flags & FF_RADIO) == 0;
                for idx in 0..item.size() {
                    let value_loc = item.values[idx].clone();
                    let widget_loc = item.widgets[idx].clone();
                    self.put(&value_loc, b"V", Object::Name(vt.clone()));
                    item.merged[idx].set("V", Object::Name(vt.clone()));
                    let in_ap = self.is_in_ap(&widget_loc, &vt);
                    let merged_as = if is_check_box || in_ap { vt.clone() } else { b"Off".to_vec() };
                    let widget_as = if in_ap { vt.clone() } else { b"Off".to_vec() };
                    if let Some(wid) = widget_loc.id() {
                        MERGED_AS.with(|m| {
                            let mut m = m.borrow_mut();
                            if merged_as != widget_as {
                                m.insert(wid, merged_as.clone());
                            } else {
                                m.remove(&wid);
                            }
                        });
                    }
                    item.merged[idx].set("AS", Object::Name(merged_as));
                    self.put(&widget_loc, b"AS", Object::Name(widget_as));
                }
                true
            }
            _ => false,
        };
        self.af.fields.put(name, item);
        result
    }

    /// AcroFields.getListOption(fieldName, 0)
    fn list_option_export(&self, item: &Item) -> Option<Vec<String>> {
        let ar = get_as_array(self.doc, &item.merged[0], b"Opt")?;
        Some(
            ar.iter()
                .map(|o| {
                    let mut obj = res(self.doc, Some(o));
                    if let Some(Object::Array(a)) = obj {
                        obj = match a.first() {
                            Some(first) => res(self.doc, Some(first)),
                            None => return String::new(),
                        };
                    }
                    match obj {
                        Some(Object::String(s, _)) => text(s),
                        Some(Object::Name(n)) => format!("/{}", name_to_string(n)),
                        Some(Object::Integer(i)) => i.to_string(),
                        Some(Object::Real(r)) => fmt(*r),
                        Some(Object::Boolean(b)) => b.to_string(),
                        Some(Object::Null) => "null".to_string(),
                        _ => String::new(),
                    }
                })
                .collect(),
        )
    }

    /// AcroFields.setListSelection()
    fn set_list_selection(&mut self, name: &str, value: &[String]) -> bool {
        let mut item = match self.af.fields.get(name) {
            Some(i) => i.clone(),
            None => return false,
        };
        if get_as_name(self.doc, &item.merged[0], b"FT") != Some(b"Ch") {
            return false;
        }
        let options = match self.list_option_export(&item) {
            Some(o) => o,
            None => return false,
        };
        let mut array = Vec::new();
        for v in value {
            if let Some(j) = options.iter().position(|o| o == v) {
                array.push(Object::Integer(j as i64));
            }
        }
        let vals: Vec<Object> =
            value.iter().map(|v| Object::String(pdf::unicode_to_text_string(v), StringFormat::Literal)).collect();
        for idx in 0..item.size() {
            item.merged[idx].set("I", Object::Array(array.clone()));
            item.merged[idx].set("V", Object::Array(vals.clone()));
            let loc = item.values[idx].clone();
            self.put(&loc, b"I", Object::Array(array.clone()));
            self.put(&loc, b"V", Object::Array(vals.clone()));
        }
        if item.widgets.iter().all(|w| w.id().is_none()) {
            return true; // only detached copies
        }
        if let Some((app, _)) = get_appearance(self.doc, &item.merged[0], value, &mut self.cache) {
            let app_ref = add_appearance(self.doc, app, &mut self.cache);
            let mut ap = Dictionary::new();
            ap.set("N", Object::Reference(app_ref));
            for idx in 0..item.size() {
                item.merged[idx].set("AP", Object::Dictionary(ap.clone()));
                let loc = item.widgets[idx].clone();
                self.put(&loc, b"AP", Object::Dictionary(ap.clone()));
            }
        }
        self.af.fields.put(name, item);
        true
    }
}

pub fn fill_form(doc: &mut Document, data: &FormData, replacement_font: Option<&str>) -> bool {
    MERGED_AS.with(|m| m.borrow_mut().clear());
    if acro_form_dict(doc).is_none() {
        return false;
    }
    if let Some(f) = replacement_font {
        eprintln!("Warning: replacement_font is not supported by this port; ignoring \"{}\".", f);
    }
    // setGenerateAppearances(true)
    if let Some(top) = acro_form_mut(doc) {
        top.remove(b"NeedAppearances");
    }
    make_annots_indirect(doc);
    let af = AcroFields::new(doc);
    // side effect of AcroFields.fill()
    for (id, v) in &af.pending_v {
        if let Some(d) = dict_of_mut(doc, *id) {
            if !d.has(b"V") {
                d.set("V", v.clone());
            }
        }
    }
    let mut filler = Filler {
        doc,
        af,
        cache: FontCache { doc_fonts: HashMap::new(), builtin: HashMap::new(), builtin_objs: HashMap::new() },
    };
    let values: HashMap<&str, &str> = data.fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let rich: HashMap<&str, &str> = data.rich_values.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let multi: HashMap<&str, &Vec<String>> = data.multi_values.iter().map(|(k, v)| (k.as_str(), v)).collect();
    // FormData built by hand may leave `names` empty
    let fallback: Vec<String>;
    let names: &Vec<String> = if data.names.is_empty() {
        let mut n: Vec<String> = data.fields.iter().map(|(k, _)| k.clone()).collect();
        for (k, _) in &data.multi_values {
            if !n.contains(k) {
                n.push(k.clone());
            }
        }
        fallback = n;
        &fallback
    } else {
        &data.names
    };
    let mut ret_val_b = false;
    for f in names {
        let rv = rich.get(f.as_str()).copied();
        if rv.is_some() {
            ret_val_b = true;
        }
        if let Some(v) = values.get(f.as_str()) {
            filler.set_field(f, v, v, rv);
        }
        if let Some(l) = multi.get(f.as_str()) {
            filler.set_list_selection(f, l);
        }
    }
    ret_val_b || (data.has_rich_text && data.names.is_empty())
}
