//! Port of cat.java (cat and shuffle), plus the parts of iText's PdfCopy that cat.java relies
//! on (page copying, form-field name-collision handling, AcroForm merging). Owner: cat agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2020),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
use crate::bookmarks::{self, PdfBookmark};
use crate::keyword::Keyword;
use crate::pdf;
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
use std::collections::{HashMap, HashSet};

/// PdfCopy.TopFormFieldData
#[derive(Default)]
struct TopFormFieldData {
    /// map new top parent field names to refs (in the source reader)
    new_names_refs: HashMap<String, ObjectId>,
    /// map new top parent field names to the kids array object (in the source reader)
    new_names_kids: HashMap<String, ObjectId>,
    /// ~all~ names, new and not-new
    all_names: HashSet<String>,
}

/// Per-reader state: PdfCopy.indirects, TopFormFieldData and the page list.
#[derive(Default)]
struct ReaderState {
    /// source id -> output id
    map: HashMap<ObjectId, ObjectId>,
    form: TopFormFieldData,
    pages: Vec<ObjectId>,
}

/// The state PdfCopy (and its PdfDocument) keeps while pages are added.
struct Copier {
    dst: Document,
    /// PdfCopy.topPageParent
    top_page_parent: ObjectId,
    /// PdfCopy.pageNumbersToRefs
    page_refs: Vec<ObjectId>,
    /// keyed by (input index, reader index)
    readers: HashMap<(usize, usize), ReaderState>,
    /// all full field names; track to prevent collision
    full_form_field_names: HashSet<String>,
    /// across all readers; track to prevent collision
    top_form_field_names: HashSet<String>,
    /// PdfAcroForm.documentFields
    document_fields: Vec<ObjectId>,
    /// the /DR of the output AcroForm
    acro_dr: Option<Dictionary>,
}

/// PdfReader.getPdfObject() on a dictionary entry: resolved, with null read as missing.
fn dget<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    match pdf::resolve(doc, dict.get(key).ok()?) {
        Some(Object::Null) | None => None,
        Some(o) => Some(o),
    }
}

fn is_page_dict(obj: Option<&Object>) -> bool {
    matches!(obj, Some(Object::Dictionary(d)) if matches!(d.get(b"Type"), Ok(Object::Name(n)) if n == b"Page"))
}

/// The version digit of a "1.x" name, as cat.GetPdfVersionChar(): '4' unless recognised.
fn version_char_of(bytes: Option<&[u8]>) -> u8 {
    match bytes {
        Some([b'1', b'.', c @ b'0'..=b'7']) => *c,
        _ => b'4', // default
    }
}

/// `new PdfName(obj.toString())` for a non-name object, as far as it matters here.
fn object_as_name_bytes(obj: &Object) -> Option<Vec<u8>> {
    match obj {
        Object::Name(n) => Some(n.clone()),
        Object::String(s, _) => Some(pdf::text_string_to_unicode(s).into_bytes()),
        Object::Integer(i) => Some(i.to_string().into_bytes()),
        Object::Real(r) => Some(r.to_string().into_bytes()),
        Object::Boolean(b) => Some(b.to_string().into_bytes()),
        _ => None,
    }
}

/// Port of cat.GetPdfVersionChar()
fn get_pdf_version_char(version_p: Option<&Object>) -> u8 {
    match version_p {
        None => b'4',
        Some(o) => version_char_of(object_as_name_bytes(o).as_deref()),
    }
}

/// PdfCopy.copyObject() on a direct object of `src`: everything it refers to is copied into
/// `dst` (once per reader, via `map`), except pages, whose references are only translated.
fn copy_direct(
    src: &Document,
    dst: &mut Document,
    obj: &Object,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Object {
    fn walk(
        src: &Document,
        dst: &mut Document,
        obj: &mut Object,
        map: &mut HashMap<ObjectId, ObjectId>,
    ) {
        match obj {
            Object::Reference(id) => {
                let target = *id;
                *id = if is_page_dict(src.objects.get(&target)) {
                    match map.get(&target) {
                        Some(new_id) => *new_id,
                        None => {
                            let new_id = dst.new_object_id();
                            map.insert(target, new_id);
                            new_id
                        }
                    }
                } else {
                    pdf::copy_object_graph(src, dst, target, map)
                };
            }
            Object::Array(items) => items.iter_mut().for_each(|o| walk(src, dst, o, map)),
            Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| walk(src, dst, o, map)),
            Object::Stream(stream) => stream
                .dict
                .iter_mut()
                .for_each(|(_, o)| walk(src, dst, o, map)),
            _ => {}
        }
    }
    let mut out = obj.clone();
    walk(src, dst, &mut out, map);
    out
}

/// Port of PdfCopy.getParent(): a form field parent. There is an additional check to make
/// sure the parent is really another form field, since some inputs in the wild have pointers
/// to wrong parents.
fn get_parent(src: &Document, annot: ObjectId) -> Option<ObjectId> {
    let dict = match src.objects.get(&annot) {
        Some(Object::Dictionary(d)) => d,
        _ => return None,
    };
    let parent_ref = match dict.get(b"Parent") {
        Ok(Object::Reference(id)) => *id,
        _ => return None,
    };
    match pdf::resolve(src, dict.get(b"Parent").ok()?) {
        Some(Object::Dictionary(parent)) if parent.has(b"Kids") => Some(parent_ref),
        _ => None,
    }
}

/// The partial name (/T) of a field: (PdfString.toString(), PdfString.isUnicode()).
fn field_t(src: &Document, annot: ObjectId) -> Option<(String, bool)> {
    let dict = match src.objects.get(&annot) {
        Some(Object::Dictionary(d)) => d,
        _ => return None,
    };
    match dget(src, dict, b"T") {
        Some(Object::String(bytes, _)) => Some((
            pdf::text_string_to_unicode(bytes),
            bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff,
        )),
        _ => None,
    }
}

/// The indirect widget annotations of a page, in /Annots order.
fn page_widgets(src: &Document, page_id: ObjectId) -> Vec<ObjectId> {
    let mut widgets = Vec::new();
    let page = match src.objects.get(&page_id) {
        Some(Object::Dictionary(d)) => d,
        _ => return widgets,
    };
    if let Some(Object::Array(annots)) = dget(src, page, b"Annots") {
        for annot_ref_obj in annots {
            // an annotation may be direct or indirect; ours must be indirect
            if let Object::Reference(annot_ref) = annot_ref_obj {
                if let Some(Object::Dictionary(annot)) = src.objects.get(annot_ref) {
                    if matches!(dget(src, annot, b"Subtype"), Some(Object::Name(n)) if n == b"Widget")
                    {
                        widgets.push(*annot_ref);
                    }
                }
            }
        }
    }
    widgets
}

/// dig upwards, parent-wise, to the top-most form field dictionary
fn top_field(src: &Document, annot: ObjectId) -> ObjectId {
    let mut annot_ref = annot;
    let mut seen = HashSet::new();
    seen.insert(annot_ref);
    while let Some(parent_ref) = get_parent(src, annot_ref) {
        if !seen.insert(parent_ref) {
            break; // (the Java would loop forever on a cyclic /Parent chain)
        }
        annot_ref = parent_ref;
    }
    annot_ref
}

impl Copier {
    fn new() -> Self {
        let mut dst = Document::with_version("1.4");
        // PdfCopy.open(): topPageParent
        let top_page_parent = dst.new_object_id();
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        dst.objects.insert(top_page_parent, Object::Dictionary(pages));
        Copier {
            dst,
            top_page_parent,
            page_refs: Vec::new(),
            readers: HashMap::new(),
            full_form_field_names: HashSet::new(),
            top_form_field_names: HashSet::new(),
            document_fields: Vec::new(),
            acro_dr: None,
        }
    }

    /// PdfCopy.setFromReader()
    fn reader_state(&mut self, key: (usize, usize), src: &Document) -> &mut ReaderState {
        let top_page_parent = self.top_page_parent;
        self.readers.entry(key).or_insert_with(|| {
            let mut rs = ReaderState::default();
            if let Some(Object::Reference(pages)) =
                src.catalog().ok().and_then(|c| c.get(b"Pages").ok())
            {
                rs.map.insert(*pages, top_page_parent);
            }
            rs.pages = pdf::page_ids(src);
            rs
        })
    }

    /// The first half of PdfCopy.addPage(): if duplicate form field names are encountered,
    /// make names unique by inserting a new top parent "field". This new parent is inserted
    /// into the *input* document; our changes to it are then copied into the output.
    fn uniquify_field_names(&mut self, key: (usize, usize), src: &mut Document, page_id: ObjectId) {
        for widget in page_widgets(src, page_id) {
            // we have a form field

            // get its full name
            let mut full_name = String::new(); // construct a full name from partial names using '.', e.g.: foo.bar.
            let mut top_name = String::new();
            let mut is_unicode_b = false; // if names are unicode, they must all be unicode
            if let Some((tt, unicode)) = field_t(src, widget) {
                top_name = tt;
                is_unicode_b = is_unicode_b || unicode;
            }

            // dig upwards, parent-wise; replace annot as we go with the
            // top-most form field dictionary
            let mut annot_ref = widget;
            let mut seen = HashSet::new();
            seen.insert(annot_ref);
            while let Some(parent_ref) = get_parent(src, annot_ref) {
                if !seen.insert(parent_ref) {
                    break;
                }
                annot_ref = parent_ref;
                if let Some((tt, unicode)) = field_t(src, annot_ref) {
                    if !top_name.is_empty() {
                        full_name.push_str(&top_name);
                        full_name.push('.');
                    }
                    top_name = tt;
                    is_unicode_b = is_unicode_b || unicode;
                }
            }

            // once we have seen a top-level field parent, we wave
            // it through and assume that it harbors no illegal field duplicates;
            // this is good, because sometimes authors want a single field
            // represented by more than one annotation on the page; this logic
            // respects that programming
            let full_key = format!("{}{}.", full_name, top_name);
            let reader_data = &mut self.readers.get_mut(&key).unwrap().form;
            if reader_data.all_names.contains(&top_name) {
                // a parent we have seen or created in this reader
                self.full_form_field_names.insert(full_key); // tally
            } else if self.full_form_field_names.contains(&full_key) {
                // insert new, top-most parent

                // name for new parent
                let mut new_parent_name_ii = 1;
                let mut new_parent_name = new_parent_name_ii.to_string();
                while self
                    .full_form_field_names
                    .contains(&format!("{}{}.", full_key, new_parent_name))
                    || self.top_form_field_names.contains(&new_parent_name)
                        && !reader_data.new_names_kids.contains_key(&new_parent_name)
                {
                    new_parent_name_ii += 1;
                    new_parent_name = new_parent_name_ii.to_string();
                }

                let (new_parent_ref, new_parent_kids) =
                    if reader_data.new_names_kids.contains_key(&new_parent_name) {
                        // a new parent we already created
                        (
                            reader_data.new_names_refs[&new_parent_name],
                            reader_data.new_names_kids[&new_parent_name],
                        )
                    } else {
                        // create a new parent using this name
                        let name_bytes: Vec<u8> = if is_unicode_b {
                            // if names are unicode, they must all be unicode
                            let mut b = vec![0xfe, 0xff];
                            for unit in new_parent_name.encode_utf16() {
                                b.extend_from_slice(&unit.to_be_bytes());
                            }
                            b
                        } else {
                            new_parent_name.clone().into_bytes()
                        };
                        let kids_ref = src.add_object(Object::Array(Vec::new()));
                        let mut new_parent = Dictionary::new();
                        new_parent.set("T", Object::String(name_bytes, StringFormat::Literal));
                        new_parent.set("Kids", Object::Reference(kids_ref));
                        let parent_ref = src.add_object(Object::Dictionary(new_parent));

                        // tally new parent
                        reader_data
                            .new_names_refs
                            .insert(new_parent_name.clone(), parent_ref);
                        reader_data
                            .new_names_kids
                            .insert(new_parent_name.clone(), kids_ref);
                        reader_data.all_names.insert(new_parent_name.clone());
                        self.top_form_field_names.insert(new_parent_name.clone());
                        (parent_ref, kids_ref)
                    };

                // wire annot and new parent together
                if let Some(Object::Dictionary(annot)) = src.objects.get_mut(&annot_ref) {
                    annot.set("Parent", Object::Reference(new_parent_ref));
                }
                // the new parent must point at the field, too
                if let Some(Object::Array(kids)) = src.objects.get_mut(&new_parent_kids) {
                    kids.push(Object::Reference(annot_ref));
                }

                // tally full field name
                self.full_form_field_names
                    .insert(format!("{}{}.", full_key, new_parent_name));
            } else {
                // tally parent
                reader_data.all_names.insert(top_name.clone());
                self.top_form_field_names.insert(top_name);

                // tally full field name
                self.full_form_field_names.insert(full_key);
            }
        }
    }

    /// Port of PdfCopy.addPage() (with getImportedPage()).
    fn add_page(&mut self, key: (usize, usize), src: &mut Document, page_num: u32) -> Result<(), ()> {
        let rs = self.reader_state(key, src);
        let page_id = match (page_num as usize).checked_sub(1).and_then(|i| rs.pages.get(i)) {
            Some(id) => *id,
            None => return Err(()),
        };
        // if we already have an iref for the page (we got here by another link), use it
        let page_ref = match rs.map.get(&page_id) {
            Some(id) => *id,
            None => {
                let id = self.dst.new_object_id();
                self.readers.get_mut(&key).unwrap().map.insert(page_id, id);
                id
            }
        };

        if !self.dst.objects.contains_key(&page_ref) {
            self.uniquify_field_names(key, src, page_id);

            // copy the page; this will copy our work, above, into the target document
            let rs = self.readers.get_mut(&key).unwrap();
            pdf::copy_object_graph(src, &mut self.dst, page_id, &mut rs.map);

            // copy page form field ref.s into document AcroForm:
            // dig down to annot, and then dig up to topmost Parent
            for widget in page_widgets(src, page_id) {
                let annot_ref = top_field(src, widget);
                if let Some(new_ref) = rs.map.get(&annot_ref) {
                    if !self.document_fields.contains(new_ref) {
                        self.document_fields.push(*new_ref);
                    }
                }
            }

            // merge the reader AcroForm/DR dictionary with the target
            let dr = src
                .catalog()
                .ok()
                .and_then(|catalog| dget(src, catalog, b"AcroForm"))
                .and_then(|o| o.as_dict().ok())
                .and_then(|acro_form| dget(src, acro_form, b"DR"))
                .and_then(|o| o.as_dict().ok());
            if let Some(dr) = dr {
                match &mut self.acro_dr {
                    None => {
                        let copy =
                            copy_direct(src, &mut self.dst, &Object::Dictionary(dr.clone()), &mut rs.map);
                        if let Object::Dictionary(copy) = copy {
                            self.acro_dr = Some(copy);
                        }
                    }
                    Some(dr_target) => {
                        for (dr_key, dr_val) in dr.iter() {
                            if !dr_target.has(dr_key) {
                                // copy key/value to dr_target
                                let val = copy_direct(src, &mut self.dst, dr_val, &mut rs.map);
                                dr_target.set(dr_key.clone(), val);
                            }
                        }
                    }
                }
            }

            if let Some(Object::Dictionary(new_page)) = self.dst.objects.get_mut(&page_ref) {
                new_page.set("Parent", Object::Reference(self.top_page_parent));
            }
        }
        self.page_refs.push(page_ref);
        Ok(())
    }
}

/// Port of cat.create_output_page()
fn create_output_page(
    session: &mut TkSession,
    copier: &mut Copier,
    page_ref: &PageRef,
    output_page_count: u32,
) -> ErrorCode {
    let mut ret_val = ErrorCode::NoError;

    // get the reader associated with this page ref.
    if page_ref.m_input_pdf_index < session.m_input_pdf.len() {
        let verbose = session.m_verbose_reporting_b;
        let uncompress = session.m_output_uncompress_b;
        let compress = session.m_output_compress_b;
        let page_pdf = &mut session.m_input_pdf[page_ref.m_input_pdf_index];

        if verbose {
            // DF rotate (the Java prints the PageRotate enum constant)
            let rot = match page_ref.m_page_rot {
                PageRotate::North => "NORTH",
                PageRotate::East => "EAST",
                PageRotate::South => "SOUTH",
                PageRotate::West => "WEST",
            };
            print!("   Adding page {} X{}X ", page_ref.m_page_num, rot);
            println!(" from {}", page_pdf.m_filename);
        }

        // take the first, associated reader and then disassociate
        let mut input_reader: Option<usize> = None;
        for (ii, mt) in page_pdf.m_readers.iter_mut().enumerate() {
            if mt.first.contains(&page_ref.m_page_num) {
                // assoc. found
                input_reader = Some(ii);
                mt.first.remove(&page_ref.m_page_num); // remove this assoc.
                break;
            }
        }

        if let Some(reader_index) = input_reader {
            let input_reader_p = &mut page_pdf.m_readers[reader_index].second;

            if uncompress {
                pdf::add_mark_to_page(input_reader_p, page_ref.m_page_num, output_page_count + 1);
            } else if compress {
                pdf::remove_mark_from_page(input_reader_p, page_ref.m_page_num);
            }

            // DF rotate
            pdf::apply_rotation_to_page(
                input_reader_p,
                page_ref.m_page_num,
                page_ref.m_page_rot as i32,
                page_ref.m_page_abs,
            );

            let key = (page_ref.m_input_pdf_index, reader_index);
            if copier.add_page(key, input_reader_p, page_ref.m_page_num).is_err() {
                // error
                eprint!("Internal Error: getImportedPage() failed for: ");
                eprintln!("{} in file: {}", page_ref.m_page_num, page_pdf.m_filename);
                ret_val = ErrorCode::Bug;
            }
        } else {
            // error
            eprint!("Internal Error: no reader found for page: ");
            eprintln!("{} in file: {}", page_ref.m_page_num, page_pdf.m_filename);
            ret_val = ErrorCode::Bug;
        }
    } else {
        // error
        eprintln!("Internal Error: Unable to find handle in m_input_pdf.");
        ret_val = ErrorCode::Bug;
    }

    ret_val
}

/// The outline items built so far. iText serialises a dictionary when it is added to the
/// body, and adding it again replaces the earlier copy; so a change made after the last
/// add_to_body() of an item never reaches the output. `items` holds the live dictionaries.
struct OutlineWriter<'a> {
    dst: &'a mut Document,
    items: HashMap<ObjectId, Dictionary>,
    page_refs: &'a [ObjectId],
    num_bookmarks_total: i32,
}

impl OutlineWriter<'_> {
    fn new_item(&mut self, dict: Dictionary) -> ObjectId {
        let id = self.dst.new_object_id();
        self.items.insert(id, dict);
        id
    }

    fn put(&mut self, id: ObjectId, key: &str, value: Object) {
        if let Some(dict) = self.items.get_mut(&id) {
            dict.set(key, value);
        }
    }

    fn add_to_body(&mut self, id: ObjectId) {
        if let Some(dict) = self.items.get(&id) {
            self.dst.objects.insert(id, Object::Dictionary(dict.clone()));
        }
    }

    /// Port of bookmarks.BuildBookmarks() (the PdfWriter flavour, utf8_b == true).
    /// Returns the final child, so that the caller can call this serially; when using
    /// `after_child`, the caller must add_to_body() the final child itself.
    fn build_bookmarks(
        &mut self,
        bookmarks: &[PdfBookmark],
        pos: &mut usize,
        parent_ref_p: ObjectId,
        after_child_ref_p: Option<ObjectId>,
        parent_level: i32,
        page_num_offset: i32,
    ) -> Option<ObjectId> {
        let mut bookmark_prev_ref_p = after_child_ref_p;
        let mut bookmark_first_ref_p: Option<ObjectId> = None;
        let mut num_bookmarks = 0;

        if *pos >= bookmarks.len() {
            return bookmark_prev_ref_p;
        }

        if parent_level + 1 < bookmarks[*pos].m_level {
            // first child jumping levels: add missing level
            num_bookmarks += 1;
            self.num_bookmarks_total += 1;
            let mut bookmark_p = Dictionary::new();
            bookmark_p.set("Parent", Object::Reference(parent_ref_p));
            bookmark_p.set("Title", Object::string_literal(Vec::<u8>::new()));
            let bookmark_ref_p = self.new_item(bookmark_p);
            bookmark_first_ref_p = Some(bookmark_ref_p);
            bookmark_prev_ref_p = Some(bookmark_ref_p);
            // recurse in loop
        }

        while *pos < bookmarks.len() {
            let it_content = &bookmarks[*pos];

            if parent_level + 1 < it_content.m_level {
                // encountered child; recurse
                let prev = match bookmark_prev_ref_p {
                    Some(prev) => prev,
                    None => break, // cannot happen: see "jumping levels" above
                };
                self.build_bookmarks(bookmarks, pos, prev, None, parent_level + 1, page_num_offset);
                continue;
            } else if it_content.m_level < parent_level + 1 {
                break; // no more children; add children to parent and return
            }

            // create child
            num_bookmarks += 1;
            self.num_bookmarks_total += 1;
            let mut bookmark_p = Dictionary::new();
            bookmark_p.set("Parent", Object::Reference(parent_ref_p));
            if let Some(prev) = bookmark_prev_ref_p {
                bookmark_p.set("Prev", Object::Reference(prev));
            }
            bookmark_p.set(
                "Title",
                Object::string_literal(pdf::unicode_to_text_string(&it_content.m_title)),
            );
            if 0 < it_content.m_page_num {
                // destination
                let mut dest_p: Vec<Object> = Vec::new();
                let page = (it_content.m_page_num + page_num_offset) as usize;
                if let Some(page_ref_p) = page.checked_sub(1).and_then(|i| self.page_refs.get(i)) {
                    dest_p.push(Object::Reference(*page_ref_p));
                }
                dest_p.push(Object::Name(b"Fit".to_vec()));
                bookmark_p.set("Dest", Object::Array(dest_p));
            }
            let bookmark_ref_p = self.new_item(bookmark_p);
            if bookmark_first_ref_p.is_none() {
                bookmark_first_ref_p = Some(bookmark_ref_p);
            }

            if let Some(prev) = bookmark_prev_ref_p {
                self.put(prev, "Next", Object::Reference(bookmark_ref_p));
                // finished with prev; add to body
                self.add_to_body(prev);
            }

            bookmark_prev_ref_p = Some(bookmark_ref_p);
            *pos += 1;
        }

        // finished with prev; add to body (unless we're appending)
        if let (Some(prev), None) = (bookmark_prev_ref_p, after_child_ref_p) {
            self.add_to_body(prev);
        }

        if let (Some(first), Some(last)) = (bookmark_first_ref_p, bookmark_prev_ref_p) {
            // pack these children into parent before returning
            let has_first = self
                .items
                .get(&parent_ref_p)
                .is_some_and(|parent_p| parent_p.has(b"First"));
            if !has_first {
                // in case we're appending
                self.put(parent_ref_p, "First", Object::Reference(first));
            }
            self.put(parent_ref_p, "Last", Object::Reference(last));
            let count = if parent_level == 0 {
                // only for top-level "outlines" dict
                self.num_bookmarks_total
            } else {
                num_bookmarks
            };
            self.put(parent_ref_p, "Count", Object::Integer(count as i64));
        }

        // pass back to calling function so it can call build_bookmarks serially
        bookmark_prev_ref_p
    }
}

/// The bookmarks of one (full) input PDF: None if it has none or ReadOutlines failed.
fn read_reader_outlines(reader_p: &Document) -> Option<Vec<PdfBookmark>> {
    let catalog_p = reader_p.catalog().ok()?;
    let outlines_p = dget(reader_p, catalog_p, b"Outlines")?.as_dict().ok()?;
    let top_outline_p = dget(reader_p, outlines_p, b"First")?.as_dict().ok()?;
    let mut bookmark_data = Vec::new();
    let rr = bookmarks::read_outlines_from(&mut bookmark_data, reader_p, top_outline_p, 0);
    if rr == 0 && !bookmark_data.is_empty() {
        Some(bookmark_data)
    } else {
        None
    }
}

/// The PDF extensions found in the inputs' catalogs: developer -> (base version, level).
fn collect_extensions(
    reader_p: &Document,
    catalog_p: &Dictionary,
    order: &mut Vec<Vec<u8>>,
    ext_base_versions: &mut HashMap<Vec<u8>, Option<Vec<u8>>>,
    ext_levels: &mut HashMap<Vec<u8>, i64>,
) {
    let extensions_p = match dget(reader_p, catalog_p, b"Extensions") {
        Some(Object::Dictionary(d)) => d,
        _ => return,
    };
    // iterate over developers
    for (developer_p, _) in extensions_p.iter() {
        let dev_exts_p = match dget(reader_p, extensions_p, developer_p) {
            Some(Object::Dictionary(d)) => d,
            _ => continue,
        };
        if !(dev_exts_p.has(b"BaseVersion") && dev_exts_p.has(b"ExtensionLevel")) {
            continue;
        }
        // use the greater base version or the greater extension level
        let base_version_po = dev_exts_p
            .get(b"BaseVersion")
            .ok()
            .and_then(|o| pdf::resolve(reader_p, o));
        let base_version_p: Option<Vec<u8>> = match base_version_po {
            // (pdftk-java 3.3.3 assigns the still-null variable to itself here, which drops
            // /BaseVersion from the output; we do what 3.3.2 does and was clearly meant)
            Some(Object::Name(n)) => Some(n.clone()),
            Some(o) => match object_as_name_bytes(o) {
                Some(n) => Some(n),
                None => continue,
            },
            None => continue,
        };
        let ext_level_i = match dev_exts_p
            .get(b"ExtensionLevel")
            .ok()
            .and_then(|o| pdf::resolve(reader_p, o))
        {
            Some(Object::Integer(i)) => *i,
            Some(Object::Real(r)) => *r as i64,
            _ => continue,
        };

        let po_char = get_pdf_version_char(base_version_po);
        let stored_char = ext_base_versions
            .get(developer_p)
            .map(|v| version_char_of(v.as_deref()));
        match stored_char {
            Some(stored) if stored >= po_char => {
                if stored == po_char && ext_levels.get(developer_p).copied().unwrap_or(0) < ext_level_i {
                    // greater extension level for current base version
                    ext_levels.insert(developer_p.clone(), ext_level_i);
                }
            }
            _ => {
                // new developer or greater base version
                if stored_char.is_none() {
                    order.push(developer_p.clone());
                }
                ext_base_versions.insert(developer_p.clone(), base_version_p);
                ext_levels.insert(developer_p.clone(), ext_level_i);
            }
        }
    }
}

/// Port of cat.create_output_cat(): catenate pages or shuffle pages
pub fn create_output_cat(session: &mut TkSession) -> ErrorCode {
    let mut ofs_p =
        match crate::cli::get_output_stream(&session.m_output_filename, session.m_ask_about_warnings_b) {
            Some(ofs_p) => ofs_p,
            None => return ErrorCode::Error, // file open error
        };

    let mut copier = Copier::new();

    // update to suit any features that we add, e.g. encryption;
    let mut max_version_cc: u8 = pdf::output_encryption_pdf_version(session)
        .as_bytes()
        .get(2)
        .copied()
        .unwrap_or(b'2');

    // copy file ID?
    if session.m_output_keep_first_id_b || session.m_output_keep_final_id_b {
        let input = if session.m_output_keep_first_id_b {
            session.m_input_pdf.first()
        } else {
            session.m_input_pdf.last()
        };
        if let Some(input_reader_p) = input.and_then(|i| i.m_readers.first()).map(|r| &r.second) {
            if let Some(file_id_p @ Object::Array(_)) = dget(input_reader_p, &input_reader_p.trailer, b"ID")
            {
                copier.dst.trailer.set("ID", file_id_p.clone());
            }
        }
    }

    // set output PDF version to the max PDF ver of all the input PDFs;
    // also find the maximum extension levels, if present
    let mut ext_order: Vec<Vec<u8>> = Vec::new();
    let mut ext_base_versions: HashMap<Vec<u8>, Option<Vec<u8>>> = HashMap::new();
    let mut ext_levels: HashMap<Vec<u8>, i64> = HashMap::new();
    for it in &session.m_input_pdf {
        let reader_p = match it.m_readers.first() {
            Some(r) => &r.second,
            None => continue,
        };

        // version in header (PRTokeniser.checkPdfHeader(): the character after "%PDF-1.")
        if let Some(version_cc) = reader_p.version.as_bytes().get(2).copied() {
            if max_version_cc < version_cc {
                max_version_cc = version_cc;
            }
        }

        // version override in catalog; used only if greater than header version, per PDF spec
        if let Ok(catalog_p) = reader_p.catalog() {
            if catalog_p.has(b"Version") {
                let version_p = catalog_p
                    .get(b"Version")
                    .ok()
                    .and_then(|o| pdf::resolve(reader_p, o));
                let version_cc = get_pdf_version_char(version_p);
                if max_version_cc < version_cc {
                    max_version_cc = version_cc;
                }
            }

            // PDF extensions
            collect_extensions(
                reader_p,
                catalog_p,
                &mut ext_order,
                &mut ext_base_versions,
                &mut ext_levels,
            );
        }
    }
    // set the pdf version
    copier.dst.version = format!("1.{}", max_version_cc as char);

    // set any pdf version extensions we might have found
    let mut extensions_ref_p: Option<ObjectId> = None;
    if !ext_base_versions.is_empty() {
        let mut extensions_dict_p = Dictionary::new();
        for developer_p in &ext_order {
            let mut ext_dict_p = Dictionary::new();
            if let Some(Some(base_version_p)) = ext_base_versions.get(developer_p) {
                ext_dict_p.set("BaseVersion", Object::Name(base_version_p.clone()));
            }
            ext_dict_p.set(
                "ExtensionLevel",
                Object::Integer(ext_levels.get(developer_p).copied().unwrap_or(0)),
            );
            extensions_dict_p.set(developer_p.clone(), Object::Dictionary(ext_dict_p));
        }
        extensions_ref_p = Some(copier.dst.add_object(Object::Dictionary(extensions_dict_p)));
    }

    let page_seq = session.m_page_seq.clone();
    let mut outlines_ref: Option<ObjectId> = None;

    if session.m_operation == Keyword::shuffle_k {
        let max_seq_length = page_seq.iter().map(|jt| jt.len()).max().unwrap_or(0);

        let mut output_page_count = 0;
        // iterate over ranges
        for ii in 0..max_seq_length {
            // iterate over ranges
            for jt in &page_seq {
                if ii < jt.len() {
                    let ret_val = create_output_page(session, &mut copier, &jt[ii], output_page_count);
                    if ret_val != ErrorCode::NoError {
                        return ret_val;
                    }
                    output_page_count += 1;
                }
            }
        }
    } else {
        // cat_k
        let mut output_page_count = 0;
        // iterate over page ranges
        for jt in &page_seq {
            // iterate over pages in page range
            for it in jt {
                let ret_val = create_output_page(session, &mut copier, it, output_page_count);
                if ret_val != ErrorCode::NoError {
                    return ret_val;
                }
                output_page_count += 1;
            }
        }

        // first impl added a bookmark for each input PDF and then
        // added any of that PDFs bookmarks under that; now it
        // appends input PDF bookmarks, which is more attractive
        if session.m_cat_full_pdfs_b {
            // add bookmark info
            let mut outlines_p = Dictionary::new();
            outlines_p.set("Type", Object::Name(b"Outlines".to_vec()));
            let mut writer = OutlineWriter {
                dst: &mut copier.dst,
                items: HashMap::new(),
                page_refs: &copier.page_refs,
                num_bookmarks_total: 0,
            };
            let output_outlines_ref_p = writer.new_item(outlines_p);
            let mut after_child_ref_p: Option<ObjectId> = None;

            let mut page_count: i64 = 1;
            // iterate over page ranges; each full PDF has one page seq in m_page_seq;
            // using m_page_seq instead of m_input_pdf, so the doc order is right
            for jt in &page_seq {
                let input = match jt.first().and_then(|p| session.m_input_pdf.get(p.m_input_pdf_index)) {
                    Some(input) => input,
                    None => continue,
                };
                let reader_page_count = input.m_num_pages as i64;

                // pdf bookmarks -> children
                if let Some(bookmark_data) = input
                    .m_readers
                    .first()
                    .and_then(|r| read_reader_outlines(&r.second))
                {
                    let mut pos = 0;
                    after_child_ref_p = writer.build_bookmarks(
                        &bookmark_data,
                        &mut pos,
                        output_outlines_ref_p,
                        after_child_ref_p,
                        0,
                        (page_count - 1) as i32, // page offset is 0-based
                    );
                }

                page_count += reader_page_count;
            }

            if writer.num_bookmarks_total != 0 {
                // we encountered bookmarks

                // necessary for serial appending to outlines
                if let Some(after_child) = after_child_ref_p {
                    writer.add_to_body(after_child);
                }
                writer.add_to_body(output_outlines_ref_p);
                outlines_ref = Some(output_outlines_ref_p);
            }
        }
    }

    // output_doc_p.close(): the page tree, the AcroForm and the catalog
    let Copier {
        mut dst,
        top_page_parent,
        page_refs,
        document_fields,
        acro_dr,
        ..
    } = copier;

    if let Some(Object::Dictionary(pages)) = dst.objects.get_mut(&top_page_parent) {
        pages.set("Count", Object::Integer(page_refs.len() as i64));
        pages.set(
            "Kids",
            Object::Array(page_refs.iter().map(|id| Object::Reference(*id)).collect()),
        );
    }

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(top_page_parent));
    if !document_fields.is_empty() {
        // PdfAcroForm.isValid()
        let mut acro_form = Dictionary::new();
        if let Some(dr) = acro_dr {
            acro_form.set("DR", Object::Dictionary(dr));
        }
        acro_form.set(
            "Fields",
            Object::Array(document_fields.iter().map(|id| Object::Reference(*id)).collect()),
        );
        let acro_form_ref = dst.add_object(Object::Dictionary(acro_form));
        catalog.set("AcroForm", Object::Reference(acro_form_ref));
    }
    if let Some(outlines) = outlines_ref {
        catalog.set("Outlines", Object::Reference(outlines));
    }
    if let Some(extensions) = extensions_ref_p {
        catalog.set("Extensions", Object::Reference(extensions));
    }
    let catalog_ref = dst.add_object(Object::Dictionary(catalog));
    dst.trailer.set("Root", Object::Reference(catalog_ref));

    // iText's PdfPages.writePageTree throws here; Java reports it as an unhandled
    // exception and exits with the BUG code.
    if pdf::page_ids(&dst).is_empty() {
        eprintln!("Error: The document has no pages.");
        return ErrorCode::Bug;
    }

    pdf::write_output(session, &mut dst, &mut ofs_p)
}
