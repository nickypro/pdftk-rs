//! Stamp / background / multistamp / multibackground (the mark loop of filter.java plus the
//! iText getImportedPage/getUnderContent/getOverContent behaviour). Owner: filter agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2023),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
use crate::data_import::itext_format_double;
use crate::pdf;
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{HashMap, HashSet};

/// A rectangle as iText's Rectangle: llx, lly, urx, ury.
#[derive(Clone, Copy)]
struct Rect {
    llx: f32,
    lly: f32,
    urx: f32,
    ury: f32,
}

impl Rect {
    fn from(r: [f32; 4]) -> Rect {
        Rect { llx: r[0], lly: r[1], urx: r[2], ury: r[3] }
    }
    /// Rectangle.rotate()
    fn rotate(self) -> Rect {
        Rect { llx: self.lly, lly: self.llx, urx: self.ury, ury: self.urx }
    }
    /// rotate once per 90 degrees of `rotation`, as the Java loops do
    fn rotated_by(self, rotation: i32) -> Rect {
        let mut r = self;
        let mut mm = 0;
        while mm < rotation {
            r = r.rotate();
            mm += 90;
        }
        r
    }
    fn width(&self) -> f32 {
        self.urx - self.llx
    }
    fn height(&self) -> f32 {
        self.ury - self.lly
    }
    fn left(&self) -> f32 {
        self.llx
    }
    fn bottom(&self) -> f32 {
        self.lly
    }
    fn right(&self) -> f32 {
        self.urx
    }
    fn top(&self) -> f32 {
        self.ury
    }
}

/// A PdfNumber made from a float.
fn num(v: f32) -> Object {
    if v.is_finite() && v == v.trunc() && v.abs() < 1.0e9 {
        Object::Integer(v as i64)
    } else {
        Object::Real(v)
    }
}

/// ByteBuffer.append(float)
fn fmt(v: f32) -> String {
    itext_format_double(v as f64)
}

/// PdfReader.getStreamBytes(): the decoded stream data (raw if it cannot be decoded).
pub fn stream_bytes(stream: &Stream) -> Vec<u8> {
    stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone())
}

/// PdfReaderInstance.getFormXObject(): build the Form XObject for a mark page, as a new
/// object of `mark` (so that it can be copied, with all it references, into the target).
fn form_xobject(mark: &mut Document, page_id: ObjectId) -> ObjectId {
    let page = mark.get_dictionary(page_id).cloned().unwrap_or_default();
    let contents = page
        .get(b"Contents")
        .ok()
        .and_then(|o| pdf::resolve(mark, o))
        .cloned();

    let mut stream = match contents {
        // a single content stream is used as it is, filters and all
        Some(Object::Stream(s)) => s,
        // an array of streams is decoded and concatenated (PdfReader.getPageContent)
        Some(Object::Array(items)) => {
            let mut bout = Vec::new();
            let n = items.len();
            for (k, item) in items.iter().enumerate() {
                if let Some(Object::Stream(s)) = pdf::resolve(mark, item) {
                    bout.extend_from_slice(&stream_bytes(s));
                    if k != n - 1 {
                        bout.push(b'\n');
                    }
                }
            }
            let mut s = Stream::new(Dictionary::new(), bout);
            let _ = s.compress();
            s
        }
        _ => Stream::new(Dictionary::new(), Vec::new()),
    };

    match pdf::page_attr(mark, page_id, b"Resources") {
        Some(res @ Object::Dictionary(_)) => stream.dict.set("Resources", res),
        _ => {
            stream.dict.remove(b"Resources");
        }
    }
    stream.dict.set("Type", Object::Name(b"XObject".to_vec()));
    stream.dict.set("Subtype", Object::Name(b"Form".to_vec()));
    // PdfImportedPage: the bounding box is the page size (MediaBox)
    let bbox = pdf::page_media_box(mark, page_id);
    stream
        .dict
        .set("BBox", Object::Array(bbox.iter().map(|v| num(*v)).collect()));
    stream.dict.set(
        "Matrix",
        Object::Array([1, 0, 0, 1, 0, 0].iter().map(|v| Object::Integer(*v)).collect()),
    );
    stream.dict.set("FormType", Object::Integer(1));
    stream.dict.set("Length", Object::Integer(stream.content.len() as i64));
    mark.add_object(Object::Stream(stream))
}

/// report.getAnnots(): the page's annotation dictionaries of /Type /Annot.
fn get_annots(doc: &Document, page_id: ObjectId) -> Vec<Dictionary> {
    let mut ret = Vec::new();
    let annots = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| pdf::resolve(doc, o));
    if let Some(Object::Array(items)) = annots {
        for jj in items {
            if let Some(Object::Dictionary(annot)) = pdf::resolve(doc, jj) {
                let is_annot = annot
                    .get(b"Type")
                    .ok()
                    .and_then(|o| pdf::resolve(doc, o))
                    .is_some_and(|o| matches!(o, Object::Name(n) if n == b"Annot"));
                if is_annot {
                    ret.push(annot.clone());
                }
            }
        }
    }
    ret
}

fn as_f32(doc: &Document, obj: &Object) -> Option<f32> {
    match pdf::resolve(doc, obj)? {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// PdfStamperImp.applyRotation(): undo the page rotation for stamped content.
fn apply_rotation(doc: &Document, page_id: ObjectId, out: &mut Vec<u8>) {
    let rotation = pdf::page_rotation(doc, page_id);
    // getPageSizeWithRotation()
    let page = Rect::from(pdf::page_media_box(doc, page_id)).rotated_by(rotation);
    match rotation {
        90 => out.extend_from_slice(format!("0 1 -1 0 {} 0 cm\n", fmt(page.top())).as_bytes()),
        180 => out.extend_from_slice(
            format!("-1 0 0 -1 {} {} cm\n", fmt(page.right()), fmt(page.top())).as_bytes(),
        ),
        270 => out.extend_from_slice(format!("0 -1 1 0 0 {} cm\n", fmt(page.right())).as_bytes()),
        _ => {}
    }
}

/// PageStamp + StampContent.addTemplate() + alterContents() + alterResources() for one page
/// that receives exactly one template.
fn stamp_page(
    doc: &mut Document,
    page_id: ObjectId,
    xobject: ObjectId,
    trans: &[f32; 6],
    background_b: bool,
    name_ptr: &mut u32,
) {
    // PageResources.setOriginalResources(): names already taken on this page
    let original = pdf::page_attr(doc, page_id, b"Resources");
    let mut resources = match &original {
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    };
    let mut forbidden: HashSet<Vec<u8>> = HashSet::new();
    for (_, value) in resources.iter() {
        if let Some(Object::Dictionary(sub)) = pdf::resolve(doc, value) {
            for (key, _) in sub.iter() {
                forbidden.insert(key.clone());
            }
        }
    }
    // PageResources.translateName()
    let name = loop {
        let candidate = format!("Xi{}", *name_ptr).into_bytes();
        *name_ptr += 1;
        if !forbidden.contains(&candidate) {
            break candidate;
        }
    };

    // PdfContentByte.addTemplate()
    let mut template = Vec::new();
    template.extend_from_slice(b"q ");
    for v in trans {
        template.extend_from_slice(fmt(*v).as_bytes());
        template.push(b' ');
    }
    template.extend_from_slice(b"cm /");
    template.extend_from_slice(&name);
    template.extend_from_slice(b" Do Q\n");

    // alterContents()
    let mut ar: Vec<Object> = {
        let raw = doc
            .get_dictionary(page_id)
            .ok()
            .and_then(|p| p.get(b"Contents").ok())
            .cloned();
        match raw.as_ref().and_then(|o| pdf::resolve(doc, o)) {
            Some(Object::Array(items)) => items.clone(),
            Some(Object::Stream(_)) => vec![raw.clone().unwrap()],
            _ => Vec::new(),
        }
    };
    let mut out = Vec::new();
    if background_b {
        out.extend_from_slice(b"q\n");
        apply_rotation(doc, page_id, &mut out);
        out.extend_from_slice(&template);
        out.extend_from_slice(b"Q\n");
    } else {
        out.extend_from_slice(b"q\n");
    }
    let mut first = Stream::new(Dictionary::new(), out);
    let _ = first.compress();
    let first_id = doc.add_object(Object::Stream(first));
    ar.insert(0, Object::Reference(first_id));
    if !background_b {
        let mut out = Vec::new();
        out.extend_from_slice(b" Q\nq\n");
        apply_rotation(doc, page_id, &mut out);
        out.extend_from_slice(&template);
        out.extend_from_slice(b"Q\n");
        let mut last = Stream::new(Dictionary::new(), out);
        let _ = last.compress();
        let last_id = doc.add_object(Object::Stream(last));
        ar.push(Object::Reference(last_id));
    }

    // PageResources.getResources()
    resources.set(
        "ProcSet",
        Object::Array(
            ["PDF", "Text", "ImageB", "ImageC", "ImageI"]
                .iter()
                .map(|n| Object::Name(n.as_bytes().to_vec()))
                .collect(),
        ),
    );
    // PdfResources.add(): merge into an existing /XObject dictionary (in place, as iText does,
    // when it is an indirect object), or add a new one
    let mut merged = false;
    match resources.get(b"XObject").ok().cloned() {
        Some(Object::Reference(_)) => {
            let mut id = None;
            let mut cur = resources.get(b"XObject").ok().cloned();
            for _ in 0..64 {
                match cur {
                    Some(Object::Reference(r)) => {
                        id = Some(r);
                        cur = doc.objects.get(&r).cloned();
                        if !matches!(cur, Some(Object::Reference(_))) {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if let Some(id) = id {
                if let Some(Object::Dictionary(d)) = doc.objects.get_mut(&id) {
                    d.set(name.clone(), Object::Reference(xobject));
                    merged = true;
                }
            }
        }
        Some(Object::Dictionary(mut d)) => {
            d.set(name.clone(), Object::Reference(xobject));
            resources.set("XObject", Object::Dictionary(d));
            merged = true;
        }
        _ => {}
    }
    if !merged {
        let mut d = Dictionary::new();
        d.set(name.clone(), Object::Reference(xobject));
        resources.set("XObject", Object::Dictionary(d));
    }

    if let Ok(page) = doc.get_dictionary_mut(page_id) {
        page.set("Contents", Object::Array(ar));
        page.set("Resources", Object::Dictionary(resources));
    }
}

/// The annotation part of the mark loop plus PdfStamperImp.addAnnotation().
#[allow(clippy::too_many_arguments)]
fn add_mark_annot(
    doc: &mut Document,
    mark: &mut Document,
    map: &mut HashMap<ObjectId, ObjectId>,
    page_id: ObjectId,
    annot: &Dictionary,
    trans: &[f32; 6],
) {
    // the transformed rectangle
    let rect_f: Option<Vec<f32>> = annot
        .get(b"Rect")
        .ok()
        .and_then(|o| pdf::resolve(mark, o))
        .and_then(|o| o.as_array().ok())
        .filter(|a| a.len() >= 4)
        .and_then(|a| a.iter().take(4).map(|v| as_f32(mark, v)).collect());
    let Some(rect_f) = rect_f else {
        return; // Java fails on an annotation without a /Rect
    };
    let m: Vec<f64> = trans.iter().map(|v| *v as f64).collect();
    let tx = |x: f32, y: f32| -> (f32, f32) {
        let (x, y) = (x as f64, y as f64);
        (
            (m[0] * x + m[2] * y + m[4]) as f32,
            (m[1] * x + m[3] * y + m[5]) as f32,
        )
    };
    let (x0, y0) = tx(rect_f[0], rect_f[1]);
    let (x1, y1) = tx(rect_f[2], rect_f[3]);
    // PdfRectangle left/bottom/right/top
    let (left, bottom, right, top) = (x0, y0, x1, y1);
    let mut rect = [left, bottom, right, top];
    if left != 0.0 || right != 0.0 || top != 0.0 || bottom != 0.0 {
        let rotation = pdf::page_rotation(doc, page_id);
        let page_size = Rect::from(pdf::page_media_box(doc, page_id)).rotated_by(rotation);
        match rotation {
            90 => rect = [page_size.top() - bottom, left, page_size.top() - top, right],
            180 => {
                rect = [
                    page_size.right() - left,
                    page_size.top() - bottom,
                    page_size.right() - right,
                    page_size.top() - top,
                ]
            }
            270 => rect = [bottom, page_size.right() - left, top, page_size.right() - right],
            _ => {}
        }
    }

    // copy the annotation and everything it refers to
    let mut new_annot = annot.clone();
    // a reference back to the mark's page makes no sense in the output
    let had_p = new_annot.remove(b"P").is_some();
    new_annot.set("Rect", Object::Array(rect.iter().map(|v| num(*v)).collect()));
    let tmp_id = mark.add_object(Object::Dictionary(new_annot));
    let new_id = pdf::copy_object_graph(mark, doc, tmp_id, map);
    if had_p {
        if let Ok(d) = doc.get_dictionary_mut(new_id) {
            d.set("P", Object::Reference(page_id));
        }
    }

    // add to the page's /Annots
    let annots_raw = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .cloned();
    match annots_raw {
        Some(Object::Array(mut items)) => {
            items.push(Object::Reference(new_id));
            if let Ok(page) = doc.get_dictionary_mut(page_id) {
                page.set("Annots", Object::Array(items));
            }
        }
        Some(Object::Reference(mut id)) => {
            for _ in 0..64 {
                match doc.objects.get(&id) {
                    Some(Object::Reference(next)) => id = *next,
                    _ => break,
                }
            }
            if let Some(Object::Array(items)) = doc.objects.get_mut(&id) {
                items.push(Object::Reference(new_id));
            } else if let Ok(page) = doc.get_dictionary_mut(page_id) {
                page.set("Annots", Object::Array(vec![Object::Reference(new_id)]));
            }
        }
        _ => {
            if let Ok(page) = doc.get_dictionary_mut(page_id) {
                page.set("Annots", Object::Array(vec![Object::Reference(new_id)]));
            }
        }
    }
}

/// Apply `mark` to every page of `doc`. background_b: under content; else over.
/// multi_b: use all mark pages (the last one repeats once they run out).
/// `mark` is used as scratch space for the objects being imported.
pub fn apply_mark(
    doc: &mut Document,
    mark: &mut Document,
    background_b: bool,
    multi_b: bool,
) -> ErrorCode {
    let mark_pages = pdf::page_ids(mark);
    if mark_pages.is_empty() {
        return ErrorCode::NoError;
    }
    let mut mark_num_pages = 1; // default: use only the first page of mark
    if multi_b {
        // use all pages of mark
        mark_num_pages = mark_pages.len();
    }

    // the mark information; initialized inside loop
    let mut mark_xobject: Option<ObjectId> = None;
    let mut mark_page_size = Rect::from([0.0; 4]);
    let mut mark_page_rotation = 0;
    let mut mark_annots: Vec<Dictionary> = Vec::new();
    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut name_ptr: u32 = 0;

    // iterate over document's pages, adding mark_page as
    // a layer above (stamp) or below (watermark) the page content;
    // scale mark_page and move it so it fits within the document's page;
    let doc_pages = pdf::page_ids(doc);
    for (idx, page_id) in doc_pages.iter().enumerate() {
        let ii = idx + 1; // page refs are 1-based, not 0-based

        // the mark page and its geometry
        if ii <= mark_num_pages {
            let mark_page_id = mark_pages[ii - 1];
            mark_page_rotation = pdf::page_rotation(mark, mark_page_id);
            mark_page_size =
                Rect::from(pdf::page_crop_box(mark, mark_page_id)).rotated_by(mark_page_rotation);

            // create a template from the page of mark
            let form_id = form_xobject(mark, mark_page_id);
            mark_xobject = Some(pdf::copy_object_graph(mark, doc, form_id, &mut map));

            mark_annots = get_annots(mark, mark_page_id);
        }
        let Some(xobject) = mark_xobject else { continue };

        // the target page geometry
        let doc_page_rotation = pdf::page_rotation(doc, *page_id);
        let doc_page_size =
            Rect::from(pdf::page_crop_box(doc, *page_id)).rotated_by(doc_page_rotation);

        let h_scale = doc_page_size.width() / mark_page_size.width();
        let v_scale = doc_page_size.height() / mark_page_size.height();
        let mark_scale = if h_scale < v_scale { h_scale } else { v_scale };

        let h_trans = ((doc_page_size.left() - mark_page_size.left() * mark_scale) as f64
            + (doc_page_size.width() - mark_page_size.width() * mark_scale) as f64 / 2.0)
            as f32;
        let v_trans = ((doc_page_size.bottom() - mark_page_size.bottom() * mark_scale) as f64
            + (doc_page_size.height() - mark_page_size.height() * mark_scale) as f64 / 2.0)
            as f32;

        let trans: Option<[f32; 6]> = match mark_page_rotation {
            0 => Some([mark_scale, 0.0, 0.0, mark_scale, h_trans, v_trans]),
            90 => Some([
                0.0,
                -mark_scale,
                mark_scale,
                0.0,
                h_trans,
                v_trans + mark_page_size.height() * mark_scale,
            ]),
            180 => Some([
                -mark_scale,
                0.0,
                0.0,
                -mark_scale,
                h_trans + mark_page_size.width() * mark_scale,
                v_trans + mark_page_size.height() * mark_scale,
            ]),
            270 => Some([
                0.0,
                mark_scale,
                -mark_scale,
                0.0,
                h_trans + mark_page_size.width() * mark_scale,
                v_trans,
            ]),
            _ => None,
        };
        let Some(trans) = trans else { continue };

        stamp_page(doc, *page_id, xobject, &trans, background_b, &mut name_ptr);

        for annot in &mark_annots {
            add_mark_annot(doc, mark, &mut map, *page_id, annot, &trans);
        }
    }
    ErrorCode::NoError
}
