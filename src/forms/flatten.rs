//! Port of PdfStamperImp.flatFields() (setFormFlattening(true)) together with the parts of
//! alterContents()/alterResources() that put the stamped "over" content on the pages.
use super::acro::*;
use super::fill::{acro_form_mut, MERGED_AS};
use super::util::*;
use crate::{pdf, report};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{BTreeMap, HashMap, HashSet};

/// PdfReader.killIndirect(): free the indirect object `obj` refers to and return what it was.
fn kill_indirect(doc: &mut Document, obj: &Object) -> Option<Object> {
    match obj {
        Object::Reference(id) => {
            let o = doc.objects.remove(id)?;
            match o {
                Object::Reference(_) => kill_indirect(doc, &o),
                o => Some(o),
            }
        }
        Object::Null => None,
        o => Some(o.clone()),
    }
}

/// PdfStamperImp.sweepKids()
fn sweep_kids(doc: &mut Document, obj: &Object, depth: usize) {
    if depth > 500 {
        return;
    }
    let dic = match kill_indirect(doc, obj) {
        Some(Object::Dictionary(d)) => d,
        _ => return,
    };
    let kids = match dic.get(b"Kids").ok().and_then(|k| kill_indirect(doc, k)) {
        Some(Object::Array(a)) => a,
        _ => return,
    };
    for k in &kids {
        sweep_kids(doc, k, depth + 1);
    }
}

struct PageStamp {
    over: Vec<u8>,
    xobjects: Vec<(Vec<u8>, ObjectId)>,
    forbidden: HashSet<Vec<u8>>,
    /// PageResources.usedNames: template -> the /XiN name it already has on this page
    used_names: HashMap<Origin, Vec<u8>>,
}

/// Identifies the PdfIndirectReference *instance* an appearance was taken from. iText names
/// templates per reference instance (PdfWriter.formXObjects), so widgets that share one /AP
/// dictionary object share one XObject name on a page, while widgets whose own /AP dictionaries
/// merely point at the same stream get a name each.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Origin {
    /// the object holding the dictionary the reference sits in (or the widget, for direct ones)
    holder: Option<ObjectId>,
    /// distinguishes widgets that are not indirect objects
    serial: usize,
    key: Vec<u8>,
}

fn stream_obj(content: Vec<u8>) -> Object {
    let mut s = Stream::new(Dictionary::new(), content);
    let _ = s.compress();
    Object::Stream(s)
}

pub fn flatten(doc: &mut Document) {
    let af = AcroFields::new(doc);
    let merged_as = MERGED_AS.with(|m| std::mem::take(&mut *m.borrow_mut()));
    let pages = pdf::page_ids(doc);
    // Flatten fields in hierarchical order (parent first): Arrays.sort() on Java strings
    let mut fieldnames = af.fields.keys();
    fieldnames.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));

    let mut stamps: BTreeMap<usize, PageStamp> = BTreeMap::new();
    let mut name_ptr = 0usize;
    let mut serial = 0usize;
    for name in &fieldnames {
        let item = af.fields.get(name).unwrap();
        for k in 0..item.merged.len() {
            let merged = &item.merged[k];
            let flags = match res(doc, merged.get(b"F").ok()) {
                Some(o) if is_number(o) => int_value(o),
                _ => 0,
            };
            let page = item.page[k];
            // Page might be out of bounds
            if page < 1 || page as usize > pages.len() {
                continue;
            }
            let app_dic = match res(doc, merged.get(b"AP").ok()) {
                Some(Object::Dictionary(d)) => d,
                _ => continue,
            };
            if (flags & FLAGS_PRINT) == 0 || (flags & FLAGS_HIDDEN) != 0 {
                continue;
            }
            let obj = app_dic.get(b"N").ok();
            let mut app: Option<ObjectId> = None;
            serial += 1;
            // where the /AP dictionary lives: its own object, or inside the widget
            let ap_holder = match merged.get(b"AP") {
                Ok(o @ Object::Reference(_)) => final_id(doc, o),
                _ => None,
            };
            let mut origin = match ap_holder {
                Some(id) => Origin { holder: Some(id), serial: 0, key: b"N".to_vec() },
                None => match item.widgets[k].id() {
                    Some(wid) => Origin { holder: Some(wid), serial: 0, key: b"N".to_vec() },
                    None => Origin { holder: None, serial, key: b"N".to_vec() },
                },
            };
            match res(doc, obj) {
                Some(Object::Stream(_)) => app = obj.and_then(|o| final_id(doc, o)),
                Some(Object::Dictionary(states)) => {
                    if let Some(o @ Object::Reference(_)) = obj {
                        // the state dictionary is an object of its own
                        origin = Origin { holder: final_id(doc, o), serial: 0, key: Vec::new() };
                    }
                    let mut as_name: Option<Vec<u8>> = match res(doc, merged.get(b"AS").ok()) {
                        Some(Object::Name(n)) => Some(n.clone()),
                        _ => None,
                    };
                    // the merged /AS that fill_form() would have left in AcroFields (see fill.rs)
                    if let Some(wid) = item.widgets[k].id() {
                        if let Some(over) = merged_as.get(&wid) {
                            let v_matches = matches!(res(doc, merged.get(b"V").ok()), Some(Object::Name(v)) if v == over);
                            if v_matches {
                                as_name = Some(over.clone());
                            }
                        }
                    }
                    if let Some(as_name) = as_name {
                        if let Ok(iref @ Object::Reference(_)) = states.get(&as_name) {
                            if let Some(Object::Stream(_)) = res(doc, Some(iref)) {
                                app = final_id(doc, iref);
                                origin.key.push(b'/');
                                origin.key.extend_from_slice(&as_name);
                            }
                        }
                    }
                }
                _ => {}
            }
            let app = match app {
                Some(a) => a,
                None => continue,
            };
            let rect = normalized_rect(doc, merged.get(b"Rect").ok()).unwrap_or([0.0; 4]);
            let page_idx = (page - 1) as usize;
            if !stamps.contains_key(&page_idx) {
                // PageResources.setOriginalResources(): names already used by the page
                let mut forbidden = HashSet::new();
                if let Some(Object::Dictionary(resources)) = pdf::page_attr(doc, pages[page_idx], b"Resources") {
                    for (_, sub) in resources.iter() {
                        if let Some(Object::Dictionary(d)) = res(doc, Some(sub)) {
                            for (key, _) in d.iter() {
                                forbidden.insert(key.clone());
                            }
                        }
                    }
                }
                stamps.insert(
                    page_idx,
                    PageStamp { over: Vec::new(), xobjects: Vec::new(), forbidden, used_names: HashMap::new() },
                );
            }
            let ps = stamps.get_mut(&page_idx).unwrap();
            // PageResources.translateName()
            let xname = match ps.used_names.get(&origin) {
                Some(n) => n.clone(),
                None => {
                    let n = loop {
                        let n = format!("Xi{}", name_ptr).into_bytes();
                        name_ptr += 1;
                        if !ps.forbidden.contains(&n) {
                            break n;
                        }
                    };
                    ps.used_names.insert(origin, n.clone());
                    ps.xobjects.push((n.clone(), app));
                    n
                }
            };
            // cb.setLiteral("Q "); cb.addTemplate(app, box.left(), box.bottom()); cb.setLiteral("q ");
            ps.over.extend_from_slice(b"Q ");
            ps.over.extend_from_slice(format!("q 1 0 0 1 {} {} cm ", fmt(rect[0]), fmt(rect[1])).as_bytes());
            ps.over.extend_from_slice(&report::itext_name_bytes(&xname));
            ps.over.extend_from_slice(b" Do Q\n");
            ps.over.extend_from_slice(b"q ");
        }
    }
    // the appearance streams become form XObjects
    let used: Vec<ObjectId> = stamps.values().flat_map(|ps| ps.xobjects.iter().map(|x| x.1)).collect();
    for id in used {
        if let Some(Object::Stream(s)) = doc.objects.get_mut(&id) {
            s.dict.set("Subtype", Object::Name(b"Form".to_vec()));
        }
    }

    // remove every widget annotation
    for page_id in &pages {
        let raw = match dict_of(doc, *page_id).and_then(|p| p.get(b"Annots").ok()) {
            Some(r) => r.clone(),
            None => continue,
        };
        let annots = match res(doc, Some(&raw)) {
            Some(Object::Array(a)) => a.clone(),
            _ => continue,
        };
        let kept: Vec<Object> = annots
            .into_iter()
            .filter(|a| match res(doc, Some(a)) {
                Some(Object::Dictionary(d)) => !matches!(d.get(b"Subtype"), Ok(Object::Name(n)) if n == b"Widget"),
                _ => false,
            })
            .collect();
        if kept.is_empty() {
            kill_indirect(doc, &raw);
            if let Some(p) = dict_of_mut(doc, *page_id) {
                p.remove(b"Annots");
            }
        } else {
            match final_id(doc, &raw) {
                Some(id) => {
                    doc.objects.insert(id, Object::Array(kept));
                }
                None => {
                    if let Some(p) = dict_of_mut(doc, *page_id) {
                        p.set("Annots", Object::Array(kept));
                    }
                }
            }
        }
    }

    // eliminateAcroformObjects()
    let i_fields = acro_form_dict(doc).and_then(|a| a.get(b"Fields").ok()).cloned();
    if let Some(i_fields) = i_fields {
        let mut kids = Dictionary::new();
        kids.set("Kids", i_fields.clone());
        sweep_kids(doc, &Object::Dictionary(kids), 0);
        kill_indirect(doc, &i_fields);
        if let Some(acro) = acro_form_mut(doc) {
            acro.set("Fields", Object::Array(Vec::new()));
        }
    }

    // alterContents()
    let stamped: Vec<usize> = stamps.keys().copied().collect();
    for (page_idx, ps) in &stamps {
        let page_id = pages[*page_idx];
        // applyRotation()
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(b" Q\nq\n");
        let mb = pdf::page_media_box(doc, page_id);
        match pdf::page_rotation(doc, page_id) {
            90 => out.extend_from_slice(format!("0 1 -1 0 {} 0 cm\n", fmt(mb[2])).as_bytes()),
            180 => out.extend_from_slice(format!("-1 0 0 -1 {} {} cm\n", fmt(mb[2]), fmt(mb[3])).as_bytes()),
            270 => out.extend_from_slice(format!("0 -1 1 0 0 {} cm\n", fmt(mb[3])).as_bytes()),
            _ => {}
        }
        out.extend_from_slice(&ps.over);
        out.extend_from_slice(b"Q\n");
        let first = doc.add_object(stream_obj(b"q\n".to_vec()));
        let last = doc.add_object(stream_obj(out));

        let raw = dict_of(doc, page_id).and_then(|p| p.get(b"Contents").ok()).cloned();
        let resolved = raw.as_ref().and_then(|r| res(doc, Some(r))).cloned();
        match (raw, resolved) {
            (Some(raw @ Object::Reference(_)), Some(Object::Array(mut ar))) => {
                ar.insert(0, Object::Reference(first));
                ar.push(Object::Reference(last));
                if let Some(id) = final_id(doc, &raw) {
                    doc.objects.insert(id, Object::Array(ar));
                }
            }
            (_, Some(Object::Array(mut ar))) => {
                ar.insert(0, Object::Reference(first));
                ar.push(Object::Reference(last));
                if let Some(p) = dict_of_mut(doc, page_id) {
                    p.set("Contents", Object::Array(ar));
                }
            }
            (Some(raw), Some(Object::Stream(_))) => {
                if let Some(p) = dict_of_mut(doc, page_id) {
                    p.set("Contents", Object::Array(vec![Object::Reference(first), raw, Object::Reference(last)]));
                }
            }
            _ => {
                if let Some(p) = dict_of_mut(doc, page_id) {
                    p.set("Contents", Object::Array(vec![Object::Reference(first), Object::Reference(last)]));
                }
            }
        }
    }

    // alterResources() / PageResources.getResources().
    //
    // iText gives each stamped page a new /Resources dictionary that is a shallow copy of the
    // original, and PdfResources.add() then putAll()s the new XObject names into the *original*
    // /XObject dictionary instance. Pages that share that instance (an indirect /XObject or
    // /Resources object, or resources inherited from a /Pages node) therefore all end up listing
    // each other's new names. `XoHome` identifies the instance.
    #[derive(Clone, PartialEq, Eq, Hash)]
    enum XoHome {
        /// the /XObject dictionary is an object of its own
        Object(ObjectId),
        /// direct /XObject inside a /Resources dictionary that is an object of its own
        InResourcesObject(ObjectId),
        /// direct /XObject inside a direct /Resources of this page or /Pages node
        InNode(ObjectId),
        /// the page has no /XObject dictionary yet: nothing is shared
        Private(usize),
    }
    struct PageRes {
        page_idx: usize,
        resources: Dictionary,
        home: XoHome,
    }
    let mut page_res: Vec<PageRes> = Vec::new();
    let mut additions: HashMap<XoHome, Vec<(Vec<u8>, ObjectId)>> = HashMap::new();
    for page_idx in &stamped {
        // find the node the page takes its /Resources from
        let mut node = pages[*page_idx];
        let mut found: Option<(ObjectId, Object)> = None;
        for _ in 0..200 {
            let d = match dict_of(doc, node) {
                Some(d) => d,
                None => break,
            };
            if let Ok(r) = d.get(b"Resources") {
                if matches!(res(doc, Some(r)), Some(Object::Dictionary(_))) {
                    found = Some((node, r.clone()));
                    break;
                }
            }
            match d.get(b"Parent").ok().and_then(|p| final_id(doc, p)) {
                Some(p) => node = p,
                None => break,
            }
        }
        let (resources, home) = match &found {
            None => (Dictionary::new(), XoHome::Private(*page_idx)),
            Some((node, raw)) => {
                let resources = match res(doc, Some(raw)) {
                    Some(Object::Dictionary(d)) => d.clone(),
                    _ => Dictionary::new(),
                };
                let home = match resources.get(b"XObject") {
                    Ok(x @ Object::Reference(_)) if matches!(res(doc, Some(x)), Some(Object::Dictionary(_))) => {
                        XoHome::Object(final_id(doc, x).unwrap())
                    }
                    Ok(Object::Dictionary(_)) => match final_id(doc, raw) {
                        Some(id) => XoHome::InResourcesObject(id),
                        None => XoHome::InNode(*node),
                    },
                    _ => XoHome::Private(*page_idx),
                };
                (resources, home)
            }
        };
        additions.entry(home.clone()).or_default().extend(stamps[page_idx].xobjects.iter().cloned());
        page_res.push(PageRes { page_idx: *page_idx, resources, home });
    }
    // the shared instances grow
    for (home, adds) in &additions {
        let target: Option<&mut Dictionary> = match home {
            XoHome::Object(id) => dict_of_mut(doc, *id),
            XoHome::InResourcesObject(id) => match dict_of_mut(doc, *id).and_then(|r| r.get_mut(b"XObject").ok()) {
                Some(Object::Dictionary(d)) => Some(d),
                _ => None,
            },
            XoHome::InNode(id) => match dict_of_mut(doc, *id).and_then(|n| n.get_mut(b"Resources").ok()) {
                Some(Object::Dictionary(r)) => match r.get_mut(b"XObject") {
                    Ok(Object::Dictionary(d)) => Some(d),
                    _ => None,
                },
                _ => None,
            },
            XoHome::Private(_) => None,
        };
        if let Some(d) = target {
            for (n, id) in adds {
                d.set(n.clone(), Object::Reference(*id));
            }
        }
    }
    for pr in page_res {
        let mut resources = pr.resources;
        resources.set(
            "ProcSet",
            Object::Array(
                [&b"PDF"[..], b"Text", b"ImageB", b"ImageC", b"ImageI"].iter().map(|n| Object::Name(n.to_vec())).collect(),
            ),
        );
        let adds = &additions[&pr.home];
        match &pr.home {
            XoHome::Object(_) => {} // the page keeps pointing at the (updated) object
            XoHome::InResourcesObject(_) | XoHome::InNode(_) => {
                if let Ok(Object::Dictionary(d)) = resources.get_mut(b"XObject") {
                    for (n, id) in adds {
                        d.set(n.clone(), Object::Reference(*id));
                    }
                }
            }
            XoHome::Private(_) => {
                let mut xo = Dictionary::new();
                for (n, id) in adds {
                    xo.set(n.clone(), Object::Reference(*id));
                }
                resources.set("XObject", Object::Dictionary(xo));
            }
        }
        if let Some(p) = dict_of_mut(doc, pages[pr.page_idx]) {
            p.set("Resources", Object::Dictionary(resources));
        }
    }
}
