//! Port of attachments.java (attach_files, unpack_files). Owner: filter agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2023),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
use crate::cli;
use crate::pdf;
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::collections::BTreeMap;
use std::io::Write;

fn drop_path(ss: &str) -> &str {
    match ss.rfind('/') {
        Some(loc) if loc != ss.len() - 1 => &ss[loc + 1..],
        _ => ss,
    }
}

fn normalize_pathname(output_pathname: &str) -> String {
    let mut output_pathname = output_pathname.to_string();
    if output_pathname == "PROMPT" {
        output_pathname = cli::prompt_for_filename(
            "Please enter the directory where you want attachments unpacked:",
        );
    }
    // (Java: lastIndexOf() == length() - 1, which also holds for the empty string)
    if output_pathname.is_empty() || output_pathname.ends_with('/') {
        output_pathname
    } else {
        // add delim to end
        output_pathname + "/"
    }
}

fn name(n: &str) -> Object {
    Object::Name(n.as_bytes().to_vec())
}

fn is_null(o: &Object) -> bool {
    matches!(o, Object::Null)
}

/// PdfReader.getPdfObject(): the resolved object, None for null / missing.
fn get_pdf_object<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
    pdf::resolve(doc, obj?).filter(|o| !is_null(o))
}

/// The id of the indirect object `obj` finally refers to, if it is a reference.
fn final_ref(doc: &Document, obj: &Object) -> Option<ObjectId> {
    let mut id = match obj {
        Object::Reference(id) => *id,
        _ => return None,
    };
    for _ in 0..64 {
        match doc.objects.get(&id) {
            Some(Object::Reference(next)) => id = *next,
            _ => break,
        }
    }
    Some(id)
}

fn catalog_id(doc: &Document) -> Option<ObjectId> {
    let id = final_ref(doc, doc.trailer.get(b"Root").ok()?)?;
    match doc.objects.get(&id) {
        Some(Object::Dictionary(_)) => Some(id),
        _ => None,
    }
}

////////////////////////////////////////////////////////////////////////////
// strings

/// PdfString(value, TEXT_UNICODE): UTF-16BE with a byte order mark, written in hex.
fn unicode_string(units: &[u16]) -> Object {
    let mut bytes = Vec::with_capacity(units.len() * 2 + 2);
    bytes.extend_from_slice(&[0xfe, 0xff]);
    for u in units {
        bytes.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Hexadecimal)
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// iText's pdfEncodingByteToChar for bytes 128..=160.
const PDFDOC_HIGH: [u16; 33] = [
    0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203a, 0x2212,
    0x2030, 0x201e, 0x201c, 0x201d, 0x2018, 0x2019, 0x201a, 0x2122, 0xfb01, 0xfb02, 0x0141,
    0x0152, 0x0160, 0x0178, 0x017d, 0x0131, 0x0142, 0x0153, 0x0161, 0x017e, 0xfffd, 0x20ac,
];

/// PdfString(value, TEXT_PDFDOCENCODING): characters outside PDFDocEncoding are dropped.
fn pdfdoc_string(s: &str) -> Object {
    let mut bytes = Vec::new();
    for c in s.encode_utf16() {
        if c < 128 || (c > 160 && c <= 255) {
            bytes.push(c as u8);
        } else if c != 0xfffd {
            if let Some(i) = PDFDOC_HIGH.iter().position(|h| *h == c) {
                bytes.push(128 + i as u8);
            }
        }
    }
    Object::string_literal(bytes)
}

/// PdfEncodings.convertToString(bytes, TEXT_UNICODE): Java's "UnicodeBig" decoder
/// (big endian unless a byte order mark says otherwise), as UTF-16 code units.
fn unicode_big_to_units(bytes: &[u8]) -> Vec<u16> {
    let (little, body) = match bytes {
        [0xfe, 0xff, rest @ ..] => (false, rest),
        [0xff, 0xfe, rest @ ..] => (true, rest),
        _ => (false, bytes),
    };
    let mut units = Vec::with_capacity(body.len() / 2 + 1);
    let mut chunks = body.chunks_exact(2);
    for c in &mut chunks {
        units.push(if little {
            u16::from_le_bytes([c[0], c[1]])
        } else {
            u16::from_be_bytes([c[0], c[1]])
        });
    }
    if !chunks.remainder().is_empty() {
        units.push(0xfffd); // malformed input
    }
    units
}

/// PdfDate for a time stamp (UTC).
fn pdf_date(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

////////////////////////////////////////////////////////////////////////////
// PdfFileSpecification / PdfNameTree

/// Port of PdfFileSpecification.fileEmbedded(). Err carries the IOException message.
fn file_embedded(doc: &mut Document, file_path: &str, file_display: &str) -> Result<Dictionary, String> {
    let data = match std::fs::read(file_path) {
        Ok(data) => data,
        Err(e) => {
            return Err(if std::path::Path::new(file_path).is_dir() {
                format!("java.io.FileNotFoundException: {} (Is a directory)", file_path)
            } else if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::PermissionDenied
            {
                format!("java.io.IOException: {} not found as file or resource.", file_path)
            } else {
                format!("java.io.IOException: {}", e)
            });
        }
    };
    let mod_secs = std::fs::metadata(file_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut fs = Dictionary::new();
    fs.set("Type", name("Filespec"));
    fs.set("F", pdfdoc_string(file_display));
    fs.set("UF", unicode_string(&utf16(file_display))); // introduced in PDF 1.7

    let mut params = Dictionary::new();
    params.set("Size", Object::Integer(data.len() as i64));
    params.set("ModDate", Object::string_literal(pdf_date(mod_secs).into_bytes()));
    let params_id = doc.add_object(Object::Dictionary(params));

    let mut dict = Dictionary::new();
    dict.set("Type", name("EmbeddedFile"));
    dict.set("Subtype", name("application/octet-stream"));
    dict.set("Params", Object::Reference(params_id));
    let mut stream = Stream::new(dict, data);
    let _ = stream.compress();
    let stream_id = doc.add_object(Object::Stream(stream));

    let mut f = Dictionary::new();
    f.set("F", Object::Reference(stream_id));
    fs.set("EF", Object::Dictionary(f));
    Ok(fs)
}

const LEAF_SIZE: usize = 64;

/// Port of PdfNameTree.writeTree(). Keys are Java strings (UTF-16 code units), whose
/// natural order is the BTreeMap's.
fn write_tree(doc: &mut Document, items: &BTreeMap<Vec<u16>, Object>) -> Option<Dictionary> {
    if items.is_empty() {
        return None;
    }
    let names: Vec<&Vec<u16>> = items.keys().collect();
    if names.len() <= LEAF_SIZE {
        let mut dic = Dictionary::new();
        let mut ar = Vec::new();
        for n in &names {
            ar.push(unicode_string(n));
            ar.push(items[*n].clone());
        }
        dic.set("Names", Object::Array(ar));
        return Some(dic);
    }
    let mut skip = LEAF_SIZE;
    let mut kids: Vec<Object> = Vec::new();
    for k in 0..names.len().div_ceil(LEAF_SIZE) {
        let offset = k * LEAF_SIZE;
        let end = (offset + LEAF_SIZE).min(names.len());
        let mut dic = Dictionary::new();
        dic.set(
            "Limits",
            Object::Array(vec![unicode_string(names[offset]), unicode_string(names[end - 1])]),
        );
        let mut arr = Vec::new();
        for n in &names[offset..end] {
            arr.push(unicode_string(n));
            arr.push(items[*n].clone());
        }
        dic.set("Names", Object::Array(arr));
        kids.push(Object::Reference(doc.add_object(Object::Dictionary(dic))));
    }
    let mut top = kids.len();
    loop {
        if top <= LEAF_SIZE {
            let mut dic = Dictionary::new();
            dic.set("Kids", Object::Array(kids[..top].to_vec()));
            return Some(dic);
        }
        skip *= LEAF_SIZE;
        let tt = names.len().div_ceil(skip);
        for k in 0..tt {
            let offset = k * LEAF_SIZE;
            let end = (offset + LEAF_SIZE).min(top);
            let mut dic = Dictionary::new();
            dic.set(
                "Limits",
                Object::Array(vec![
                    unicode_string(names[k * skip]),
                    unicode_string(names[((k + 1) * skip).min(names.len()) - 1]),
                ]),
            );
            dic.set("Kids", Object::Array(kids[offset..end].to_vec()));
            kids[k] = Object::Reference(doc.add_object(Object::Dictionary(dic)));
        }
        top = tt;
    }
}

/// Port of PdfNameTree.readTree(): (key, value) in tree order; later duplicates replace
/// earlier ones in place, as HashMap.put() does.
fn read_tree(doc: &Document, dic: &Dictionary) -> Vec<(Vec<u16>, Object)> {
    fn iterate(doc: &Document, dic: &Dictionary, items: &mut Vec<(Vec<u16>, Object)>, depth: u32) {
        if depth > 64 {
            return;
        }
        if let Some(Object::Array(nn)) = get_pdf_object(doc, dic.get(b"Names").ok()) {
            let mut k = 0;
            while k + 1 < nn.len() {
                if let Some(Object::String(bytes, _)) = get_pdf_object(doc, Some(&nn[k])) {
                    let key = unicode_big_to_units(bytes);
                    let value = nn[k + 1].clone();
                    match items.iter_mut().find(|(n, _)| *n == key) {
                        Some(entry) => entry.1 = value,
                        None => items.push((key, value)),
                    }
                }
                k += 2;
            }
        } else if let Some(Object::Array(nn)) = get_pdf_object(doc, dic.get(b"Kids").ok()) {
            for kid in nn {
                if let Some(Object::Dictionary(kid)) = get_pdf_object(doc, Some(kid)) {
                    iterate(doc, kid, items, depth + 1);
                }
            }
        }
    }
    let mut items = Vec::new();
    iterate(doc, dic, &mut items, 0);
    items
}

/// The order java.util.HashMap<String, _>.values() iterates in.
fn java_hash_map_order<V>(items: Vec<(Vec<u16>, V)>) -> Vec<V> {
    let mut n: usize = 16;
    while items.len() as f64 > 0.75 * n as f64 {
        n *= 2;
    }
    let mut indexed: Vec<(usize, V)> = items
        .into_iter()
        .map(|(key, value)| {
            let mut h: i32 = 0;
            for u in &key {
                h = h.wrapping_mul(31).wrapping_add(*u as i32);
            }
            let h = h as u32;
            let spread = h ^ (h >> 16);
            ((spread as usize) & (n - 1), value)
        })
        .collect();
    indexed.sort_by_key(|(bucket, _)| *bucket); // stable: insertion order within a bucket
    indexed.into_iter().map(|(_, v)| v).collect()
}

////////////////////////////////////////////////////////////////////////////
// attach_files

/// What Java prints when an exception escapes create_output_filter().
fn unhandled_exception(message: &str) -> ErrorCode {
    eprintln!("Unhandled Java Exception in create_output():");
    eprintln!("{}", message);
    ErrorCode::Bug
}

fn number(doc: &Document, obj: &Object) -> Option<f32> {
    match pdf::resolve(doc, obj)? {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn num(v: f32) -> Object {
    if v.is_finite() && v == v.trunc() && v.abs() < 1.0e9 {
        Object::Integer(v as i64)
    } else {
        Object::Real(v)
    }
}

/// Port of attachments.attach_files(): attach session.m_input_attach_file_filename to `doc`
/// (document level, or to page m_input_attach_file_pagenum; -1: prompt, -2: "end").
///
/// Returns ErrorCode::Bug (after printing what Java prints for the uncaught IOException)
/// when a file cannot be read while attaching to a page; the caller must then stop without
/// writing output. Every other problem is reported and skipped, as in Java.
pub fn attach_files(session: &mut TkSession, doc: &mut Document) -> ErrorCode {
    if session.m_input_attach_file_filename.is_empty() {
        return ErrorCode::NoError;
    }
    let pages = pdf::page_ids(doc);
    let num_pages = pages.len() as i32;

    if session.m_input_attach_file_pagenum == -1 {
        // our signal to prompt the user for a pagenum
        println!("Please enter the page number you want to attach these files to.");
        println!("   The first page is 1.  The final page is \"end\".");
        let buff =
            cli::prompt_for_filename("   To attach files at the document level, just press Enter.");
        if buff.is_empty() {
            // attach to document
            session.m_input_attach_file_pagenum = 0;
        }
        if buff == "end" {
            // the final page
            session.m_input_attach_file_pagenum = num_pages;
        } else {
            let digits: String = buff.chars().take_while(|c| c.is_ascii_digit()).collect();
            session.m_input_attach_file_pagenum = digits.parse::<i32>().unwrap_or(0);
        }
    } else if session.m_input_attach_file_pagenum == -2 {
        // the final page ("end")
        session.m_input_attach_file_pagenum = num_pages;
    }
    let pagenum = session.m_input_attach_file_pagenum;
    let filenames = session.m_input_attach_file_filename.clone();

    if pagenum != 0 {
        // attach to a page using annotations
        let trans = 27.0f32;
        let margin = 18.0f32;

        if 0 < pagenum && pagenum <= num_pages {
            let page_id = pages[pagenum as usize - 1];
            let crop = pdf::page_crop_box(doc, page_id); // llx lly urx ury
            let (crop_left, crop_bottom, crop_right, crop_top) = (crop[0], crop[1], crop[2], crop[3]);
            let mut corner_top = crop_top - margin;
            let mut corner_left = crop_left + margin;

            let annots_raw = doc
                .get_dictionary(page_id)
                .ok()
                .and_then(|p| p.get(b"Annots").ok())
                .cloned();
            let annots_po = get_pdf_object(doc, annots_raw.as_ref()).cloned();
            let annots_new_b = annots_po.is_none();
            let mut annots_p: Vec<Object> = match annots_po {
                None => Vec::new(), // create Annots array
                Some(Object::Array(items)) => items,
                Some(_) => return ErrorCode::NoError,
            };
            // grab corner_top and corner_left from the bottom right of the newest annot
            for ii in &annots_p {
                if let Some(Object::Dictionary(annot)) = get_pdf_object(doc, Some(ii)) {
                    if let Some(Object::Array(bbox)) = get_pdf_object(doc, annot.get(b"Rect").ok()) {
                        if bbox.len() == 4 {
                            if let (Some(t), Some(l)) = (number(doc, &bbox[1]), number(doc, &bbox[2])) {
                                corner_top = t;
                                corner_left = l;
                            }
                        }
                    }
                }
            }
            for vit in &filenames {
                let mut vit = vit.clone();
                if vit == "PROMPT" {
                    vit = cli::prompt_for_filename("Please enter a filename for attachment:");
                }
                let filename = drop_path(&vit).to_string();

                // wrap our location over page bounds, if needed
                if crop_right < corner_left + trans {
                    corner_left = crop_left + margin;
                }
                if corner_top - trans < crop_bottom {
                    corner_top = crop_top - margin;
                }

                let fs = match file_embedded(doc, &vit, &filename) {
                    Ok(fs) => fs,
                    Err(message) => return unhandled_exception(&message),
                };
                let fs_id = doc.add_object(Object::Dictionary(fs));

                let mut annot = Dictionary::new();
                annot.set(
                    "Rect",
                    Object::Array(vec![
                        num(corner_left),
                        num(corner_top - trans),
                        num(corner_left + trans),
                        num(corner_top),
                    ]),
                );
                annot.set("Subtype", name("FileAttachment"));
                annot.set("Contents", unicode_string(&utf16(&filename)));
                annot.set("FS", Object::Reference(fs_id));
                let ref_p = doc.add_object(Object::Dictionary(annot));
                annots_p.push(Object::Reference(ref_p));

                // advance the location of our annotation
                corner_left += trans;
                corner_top -= trans;
            }
            if annots_new_b {
                // add new Annots array to page dict
                let ref_p = doc.add_object(Object::Array(annots_p));
                if let Ok(page) = doc.get_dictionary_mut(page_id) {
                    page.set("Annots", Object::Reference(ref_p));
                }
            } else {
                match annots_raw.as_ref().and_then(|o| final_ref(doc, o)) {
                    Some(id) => {
                        doc.objects.insert(id, Object::Array(annots_p));
                    }
                    None => {
                        if let Ok(page) = doc.get_dictionary_mut(page_id) {
                            page.set("Annots", Object::Array(annots_p));
                        }
                    }
                }
            }
        } else {
            // error
            eprintln!("Error: page number {} is not present in the input PDF.", pagenum);
        }
        return ErrorCode::NoError;
    }

    // attach to document using the EmbeddedFiles name tree
    let Some(catalog_id) = catalog_id(doc) else {
        // error
        eprintln!("Internal Error: couldn't read input PDF Root dictionary.");
        eprintln!("   File attachment failed; no new files attached to output.");
        return ErrorCode::NoError;
    };
    let catalog = doc.get_dictionary(catalog_id).cloned().unwrap_or_default();

    // the Names dict
    let names_raw = catalog.get(b"Names").ok().cloned();
    let names_po = get_pdf_object(doc, names_raw.as_ref()).cloned();
    let names_new_b = names_po.is_none();
    let af_raw = catalog.get(b"AF").ok().cloned();
    let af_po = get_pdf_object(doc, af_raw.as_ref()).cloned();
    let af_new_b = af_po.is_none();
    let (mut names_p, mut af_p) = match (
        names_po.unwrap_or_else(|| Object::Dictionary(Dictionary::new())),
        af_po.unwrap_or_else(|| Object::Array(Vec::new())),
    ) {
        (Object::Dictionary(n), Object::Array(a)) => (n, a),
        _ => {
            // error
            eprintln!("Internal Error: couldn't read or create PDF Names dictionary.");
            return ErrorCode::NoError;
        }
    };

    // the EmbeddedFiles name tree (ref. 1.5, sec. 3.8.5), which is a dict at top
    let emb_raw = names_p.get(b"EmbeddedFiles").ok().cloned();
    let emb_files_tree_p = get_pdf_object(doc, emb_raw.as_ref()).cloned();
    let emb_files_tree_new_b = emb_files_tree_p.is_none();
    let mut emb_files_map_p: BTreeMap<Vec<u16>, Object> = BTreeMap::new();
    if let Some(Object::Dictionary(tree)) = &emb_files_tree_p {
        // read current name tree of attachments into a map
        for (key, value) in read_tree(doc, tree) {
            emb_files_map_p.insert(key, value);
        }
    }

    ////
    // add matter to name tree

    for vit in &filenames {
        let mut vit = vit.clone();
        if vit == "PROMPT" {
            vit = cli::prompt_for_filename("Please enter a filename for attachment:");
        }
        let filename = drop_path(&vit).to_string();

        // create the file spec. from file
        let mut filespec_p = match file_embedded(doc, &vit, &filename) {
            Ok(fs) => fs,
            Err(_) => {
                // file open error
                eprintln!("Error: Failed to open attachment file: ");
                eprintln!("   {}", vit);
                eprintln!("   Skipping this file.");
                continue;
            }
        };
        filespec_p.set(
            "AFRelationship",
            Object::Name(session.m_input_attach_file_relation.as_bytes().to_vec()),
        );

        // add file spec. to PDF via indirect ref.
        let ref_p = doc.add_object(Object::Dictionary(filespec_p));

        // contruct a name, if necessary, to prevent possible key collision on the name tree
        let mut key_p = utf16(&vit);
        let mut counter = 1;
        while emb_files_map_p.contains_key(&key_p) {
            // append a unique suffix
            key_p = utf16(&format!("{}-{}", vit, counter));
            counter += 1;
        }

        // add file spec. to map
        emb_files_map_p.insert(key_p, Object::Reference(ref_p));
        af_p.push(Object::Reference(ref_p));
    }

    if !emb_files_map_p.is_empty() {
        // create a name tree from map
        let emb_files_tree_new_p = write_tree(doc, &emb_files_map_p);

        match (emb_files_tree_new_b, emb_files_tree_p, emb_files_tree_new_p) {
            (true, _, Some(new_tree)) => {
                // adding new material
                let ref_p = doc.add_object(Object::Dictionary(new_tree));
                names_p.set("EmbeddedFiles", Object::Reference(ref_p));
            }
            (false, Some(Object::Dictionary(mut old_tree)), Some(new_tree)) => {
                // supplementing old material (PdfDictionary.merge)
                for (key, value) in new_tree.iter() {
                    old_tree.set(key.clone(), value.clone());
                }
                match emb_raw.as_ref().and_then(|o| final_ref(doc, o)) {
                    Some(id) => {
                        doc.objects.insert(id, Object::Dictionary(old_tree));
                    }
                    None => names_p.set("EmbeddedFiles", Object::Dictionary(old_tree)),
                }
            }
            _ => {
                // error
                eprintln!("Internal Error: no valid EmbeddedFiles tree to add to PDF.");
            }
        }

        // put the Names dict. and the AF array (back) in place
        let names_target = if names_new_b {
            Object::Reference(doc.add_object(Object::Dictionary(names_p)))
        } else {
            match names_raw.as_ref().and_then(|o| final_ref(doc, o)) {
                Some(id) => {
                    doc.objects.insert(id, Object::Dictionary(names_p));
                    names_raw.clone().unwrap()
                }
                None => Object::Dictionary(names_p),
            }
        };
        let af_target = if af_new_b {
            Object::Array(af_p)
        } else {
            match af_raw.as_ref().and_then(|o| final_ref(doc, o)) {
                Some(id) => {
                    doc.objects.insert(id, Object::Array(af_p));
                    af_raw.clone().unwrap()
                }
                None => Object::Array(af_p),
            }
        };
        if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
            catalog.set("Names", names_target);
            catalog.set("AF", af_target);
        }
    }
    ErrorCode::NoError
}

////////////////////////////////////////////////////////////////////////////
// unpack_files

fn unpack_file(
    doc: &Document,
    filespec_p: &Dictionary,
    output_pathname: &str,
    ask_about_warnings_b: bool,
) {
    let type_ok = matches!(
        get_pdf_object(doc, filespec_p.get(b"Type").ok()),
        Some(Object::Name(n)) if n == b"Filespec" || n == b"F"
    );
    if !type_ok {
        return;
    }
    let Some(Object::Dictionary(ef_p)) = get_pdf_object(doc, filespec_p.get(b"EF").ok()) else {
        return;
    };

    // UF introduced in PDF 1.7
    let fn_p = get_pdf_object(doc, filespec_p.get(b"UF").ok())
        .or_else(|| get_pdf_object(doc, filespec_p.get(b"F").ok())); // try the F key
    let Some(Object::String(fn_bytes, _)) = fn_p else {
        return;
    };
    let fn_str = pdf::text_string_to_unicode(fn_bytes);
    let mut filename = drop_path(&fn_str).to_string();

    // did the user supply a path?
    if !output_pathname.is_empty() {
        // prepend it; output_pathname has been normalized, already
        filename = format!("{}{}", output_pathname, filename);
    }

    // assuming that F key is used to store the data, and not DOS, Mac, or Unix
    if let Some(Object::Stream(f_p)) = get_pdf_object(doc, ef_p.get(b"F").ok()) {
        let result: std::io::Result<()> = (|| {
            let bytes_p = match f_p.decompressed_content() {
                Ok(bytes) => bytes,
                Err(e) => {
                    if f_p.dict.has(b"Filter") {
                        return Err(std::io::Error::other(e.to_string()));
                    }
                    f_p.content.clone()
                }
            };
            if let Some(mut os) = cli::get_output_stream_file(&filename, ask_about_warnings_b, false)
            {
                os.write_all(&bytes_p)?;
                os.flush()?;
            }
            Ok(())
        })();
        if result.is_err() {
            eprintln!("Error: failed to write attachment file: ");
            eprintln!("   {}", filename);
            eprintln!("   Skipping.");
        }
    }
}

/// Port of attachments.unpack_files(): operation unpack_files_k.
pub fn unpack_files(session: &mut TkSession) -> ErrorCode {
    // output pathname; PROMPT if necessary
    let output_pathname = normalize_pathname(&session.m_output_filename);
    let ask_about_warnings_b = session.m_ask_about_warnings_b;
    let Some(doc) = session
        .m_input_pdf
        .first()
        .and_then(|input| input.m_readers.first())
        .map(|reader| &reader.second)
    else {
        return ErrorCode::NoError;
    };

    // unpack document attachments
    if let Some(catalog_id) = catalog_id(doc) {
        let catalog = doc.get_dictionary(catalog_id).ok();
        // the Names dict
        if let Some(Object::Dictionary(names_p)) =
            get_pdf_object(doc, catalog.and_then(|c| c.get(b"Names").ok()))
        {
            // the EmbeddedFiles name tree (ref. 1.5, sec. 3.8.5), which is a dict at top
            if let Some(Object::Dictionary(tree)) =
                get_pdf_object(doc, names_p.get(b"EmbeddedFiles").ok())
            {
                // read current name tree of attachments into a map
                let emb_files_map_p = read_tree(doc, tree);
                for value_p in java_hash_map_order(emb_files_map_p) {
                    if let Some(Object::Dictionary(filespec_p)) = get_pdf_object(doc, Some(&value_p))
                    {
                        unpack_file(doc, filespec_p, &output_pathname, ask_about_warnings_b);
                    }
                }
            }
        }
    }

    // unpack page attachments
    for page_id in pdf::page_ids(doc) {
        let Ok(page_p) = doc.get_dictionary(page_id) else { continue };
        if let Some(Object::Array(annots)) = get_pdf_object(doc, page_p.get(b"Annots").ok()) {
            for jj in annots {
                if let Some(Object::Dictionary(annot_p)) = get_pdf_object(doc, Some(jj)) {
                    let is_attachment = matches!(
                        get_pdf_object(doc, annot_p.get(b"Subtype").ok()),
                        Some(Object::Name(n)) if n == b"FileAttachment"
                    );
                    if is_attachment {
                        if let Some(Object::Dictionary(filespec_p)) =
                            get_pdf_object(doc, annot_p.get(b"FS").ok())
                        {
                            unpack_file(doc, filespec_p, &output_pathname, ask_about_warnings_b);
                        }
                    }
                }
            }
        }
    }
    ErrorCode::NoError
}
