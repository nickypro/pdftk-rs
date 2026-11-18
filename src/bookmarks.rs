//! Port of bookmarks.java (read outlines -> PdfBookmark list; build outlines from a list).
//! Owner: report agent. Also used by cat.rs when merging outlines.
//!
//! Differences from the Java layout, all forced by the shared stub signatures:
//! * `PdfBookmark::m_title` always holds the *decoded* Unicode title. The Java stores the
//!   title already XML-escaped (or not) according to `utf8_b`; here the escaping is applied
//!   by the caller when printing (`report.rs`) and undone by the caller when importing
//!   (`data_import.rs`).
//! * Named / string destinations are not looked up here: as in the Java, that is done once
//!   when the input is opened (`pdf::consolidate_named_destinations`, called by
//!   `pdf::add_reader`). A document loaded by other means must be consolidated first.
#![allow(dead_code, unused)]
use crate::pdf;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct PdfBookmark {
    pub m_title: String,
    pub m_level: i32,
    pub m_page_num: i32, // zero means no destination
}

impl PdfBookmark {
    pub const PREFIX: &'static str = "Bookmark";
    pub const BEGIN_MARK: &'static str = "BookmarkBegin";
    pub const TITLE_LABEL: &'static str = "BookmarkTitle:";
    pub const LEVEL_LABEL: &'static str = "BookmarkLevel:";
    pub const PAGE_NUMBER_LABEL: &'static str = "BookmarkPageNumber:";

    /// Port of PdfBookmark.valid() (the title is never null here).
    pub fn valid(&self) -> bool {
        0 < self.m_level && 0 <= self.m_page_num
    }
}

/// Port of PdfBookmark.toString(); the title is written as stored (no escaping).
impl std::fmt::Display for PdfBookmark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\n{} {}\n{} {}\n{} {}\n",
            Self::BEGIN_MARK,
            Self::TITLE_LABEL,
            self.m_title,
            Self::LEVEL_LABEL,
            self.m_level,
            Self::PAGE_NUMBER_LABEL,
            self.m_page_num
        )
    }
}

// ---------------------------------------------------------------------------------------------
// small object helpers

/// reader.getPdfObject(): resolve, treating PDF null like "absent".
fn res<'a>(doc: &'a Document, obj: Option<&'a Object>) -> Option<&'a Object> {
    let o = pdf::resolve(doc, obj?)?;
    if o.is_null() {
        None
    } else {
        Some(o)
    }
}

fn dget<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    res(doc, dict.get(key).ok())
}

fn as_dict(obj: Option<&Object>) -> Option<&Dictionary> {
    match obj? {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn as_array(obj: Option<&Object>) -> Option<&Vec<Object>> {
    match obj? {
        Object::Array(a) => Some(a),
        _ => None,
    }
}

fn is_number(obj: &Object) -> bool {
    matches!(obj, Object::Integer(_) | Object::Real(_))
}

/// PdfNumber.intValue(): (int) of the double value.
fn int_value(obj: &Object) -> i32 {
    match obj {
        Object::Integer(i) => (*i).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        Object::Real(r) => *r as i32,
        _ => 0,
    }
}

/// Object identity: the id of the indirect object a reference finally lands on.
/// Java compares PdfDictionary references; a direct dictionary is only ever identical to itself.
fn ident(doc: &Document, obj: &Object) -> Option<ObjectId> {
    match obj {
        Object::Reference(_) => doc.dereference(obj).ok().and_then(|(id, _)| id),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// reading

/// Port of bookmarks.GetPageNumber(): take a page (or page tree node) and return its zero-based
/// location in the document by walking up the page tree, counting pages as we go.
/// `dict_id` is the object identity of `dict_p` (None for a direct dictionary).
pub fn get_page_number(
    doc: &Document,
    dict_id: Option<ObjectId>,
    dict_p: &Dictionary,
    cache: &mut HashMap<ObjectId, i32>,
) -> i32 {
    get_page_number_rec(doc, dict_id, dict_p, cache, 0)
}

fn get_page_number_rec(
    doc: &Document,
    dict_id: Option<ObjectId>,
    dict_p: &Dictionary,
    cache: &mut HashMap<ObjectId, i32>,
    depth: usize,
) -> i32 {
    // consult the cache
    if let Some(id) = dict_id {
        if let Some(v) = cache.get(&id) {
            return *v;
        }
    }

    let mut ret_val = 0;

    if dict_p.has(b"Parent") && depth < 256 {
        let parent_raw = dict_p.get(b"Parent").ok();
        let parent_id = parent_raw.and_then(|o| ident(doc, o));
        if let Some(Object::Dictionary(parent_p)) = res(doc, parent_raw) {
            // a parent is a page tree object and will have Kids

            // recurse up the page tree
            let mut sum_pages = get_page_number_rec(doc, parent_id, parent_p, cache, depth + 1);

            if let Some(kids_p) = as_array(dget(doc, parent_p, b"Kids")) {
                // Kids may be Pages or Page Tree Nodes

                // iterate over *dict_p's parent's kids until we run into *dict_p
                for kids_ii in kids_p {
                    let kid_id = ident(doc, kids_ii);
                    if let Some(Object::Dictionary(kid_p)) = res(doc, Some(kids_ii)) {
                        // Translator note: comparing references
                        let same = match (kid_id, dict_id) {
                            (Some(a), Some(b)) => a == b,
                            _ => std::ptr::eq(kid_p, dict_p),
                        };
                        if same {
                            // we have what we were looking for
                            ret_val = sum_pages;
                        }

                        // is kid a page, or is kid a page tree object? add count to sum
                        match dget(doc, kid_p, b"Type") {
                            Some(Object::Name(kid_type)) => {
                                if kid_type == b"Page" {
                                    // store page number in our cache
                                    if let Some(id) = kid_id {
                                        cache.insert(id, sum_pages);
                                    }
                                    sum_pages += 1;
                                } else if kid_type == b"Pages" {
                                    match dget(doc, kid_p, b"Count") {
                                        Some(count_p) if is_number(count_p) => {
                                            sum_pages = sum_pages.wrapping_add(int_value(count_p));
                                        }
                                        _ => {
                                            eprintln!("pdftk Error in GetPageNumber(): invalid count;");
                                        }
                                    }
                                } else {
                                    eprintln!("pdftk Error in GetPageNumber(): unexpected kid type;");
                                }
                            }
                            _ => {
                                eprintln!("pdftk Error in GetPageNumber(): invalid kid_type_p;");
                            }
                        }
                    } else {
                        eprintln!("pdftk Error in GetPageNumber(): invalid kid_p;");
                    }
                }
            } else {
                eprintln!("pdftk Error in GetPageNumber(): invalid kids array;");
            }
        } else {
            eprintln!("pdftk Error in GetPageNumber(): invalid parent;");
        }
    } else {
        // *dict_p has no parent; end recursion
        ret_val = 0;
        if let Some(id) = dict_id {
            cache.insert(id, ret_val);
        }
    }

    ret_val
}

/// Port of bookmarks.ReadOutlines(): walk `outline_p` and its siblings/children, appending to
/// `bookmark_data`. Returns non-zero if a title was missing (as the Java does).
pub fn read_outlines_from(
    bookmark_data: &mut Vec<PdfBookmark>,
    doc: &Document,
    outline_p: &Dictionary,
    level: i32,
) -> i32 {
    let mut seen: HashSet<*const Dictionary> = HashSet::new();
    read_outlines_rec(bookmark_data, doc, outline_p, level, &mut seen)
}

fn read_outlines_rec<'a>(
    bookmark_data: &mut Vec<PdfBookmark>,
    doc: &'a Document,
    outline_p: &'a Dictionary,
    level: i32,
    seen: &mut HashSet<*const Dictionary>,
) -> i32 {
    let mut ret_val = 0;
    let mut cache: HashMap<ObjectId, i32> = HashMap::new();
    let mut outline_opt = Some(outline_p);

    while let Some(outline_p) = outline_opt {
        // the Java loops (or overflows its stack) forever on a cyclic outline tree; we stop
        if !seen.insert(outline_p as *const Dictionary) || level > 512 {
            break;
        }

        // load this with collected data, then add to vector
        let mut bookmark = PdfBookmark::default();

        // the title
        match dget(doc, outline_p, b"Title") {
            Some(Object::String(s, _)) => {
                bookmark.m_title = pdf::text_string_to_unicode(s);
            }
            _ => {
                // error; the Java leaves m_title null, which prints as "null"
                bookmark.m_title = "null".to_string();
                ret_val = 1;
            }
        }

        // the level; 1-based to jive with HTML heading level concept
        bookmark.m_level = level + 1;

        // page number, 1-based;
        // a zero value indicates no page destination or an error
        {
            let mut fail_b = false;

            // the destination object may take be in a couple different places
            // and may take a couple, different forms
            let mut destination_p: Option<&Object> = None;
            if outline_p.has(b"Dest") {
                destination_p = dget(doc, outline_p, b"Dest");
            } else if outline_p.has(b"A") {
                if let Some(Object::Dictionary(action_p)) = dget(doc, outline_p, b"A") {
                    if let Some(Object::Name(s_p)) = dget(doc, action_p, b"S") {
                        if s_p == b"GoTo" {
                            destination_p = dget(doc, action_p, b"D");
                        } else {
                            // immediate action is not a link in this document; not an error
                        }
                    } else {
                        fail_b = true;
                    }
                } else {
                    fail_b = true;
                }
            } else {
                // unexpected
                fail_b = true;
            }

            // destination is an array
            // Named destinations handled by PdfReader::consolidateNamedDestinations
            match destination_p {
                Some(Object::Array(array_list_p)) if !array_list_p.is_empty() => {
                    let first = &array_list_p[0];
                    match res(doc, Some(first)) {
                        Some(Object::Dictionary(page_p)) => {
                            bookmark.m_page_num =
                                get_page_number(doc, ident(doc, first), page_p, &mut cache) + 1;
                        }
                        Some(page_p) if is_number(page_p) => {
                            // page_p should always be a PdfDictionary (cf PDF Reference 1.7
                            // Section 12.3.2.2) but seen PdfNumber in the wild (pdftk-java issue 123)
                            bookmark.m_page_num = int_value(page_p).wrapping_add(1);
                        }
                        _ => {
                            fail_b = true;
                        }
                    }
                }
                _ => {
                    fail_b = true;
                }
            }

            if fail_b {
                // output our 'null page reference' code
                bookmark.m_page_num = 0;
            }
        }

        // add bookmark to collected data
        if 0 < bookmark.m_level {
            bookmark_data.push(bookmark);
        }

        // recurse into any children
        if outline_p.has(b"First") {
            if let Some(Object::Dictionary(child_p)) = dget(doc, outline_p, b"First") {
                ret_val += read_outlines_rec(bookmark_data, doc, child_p, level + 1, seen);
            }
        }

        // iterate over siblings
        outline_opt = None;
        if outline_p.has(b"Next") {
            if let Some(Object::Dictionary(sibling_p)) = dget(doc, outline_p, b"Next") {
                outline_opt = Some(sibling_p);
            }
        }
    }

    ret_val
}

/// Port of bookmarks.ReadOutlines(): flatten the outline tree in document order.
/// Titles are decoded Unicode (not XML-escaped); a missing title reads as "null" (as Java prints).
pub fn read_outlines(doc: &Document) -> Vec<PdfBookmark> {
    let mut bookmark_data = Vec::new();
    if let Ok(catalog_p) = doc.catalog() {
        if let Some(Object::Dictionary(outlines_p)) = dget(doc, catalog_p, b"Outlines") {
            if let Some(Object::Dictionary(top_outline_p)) = dget(doc, outlines_p, b"First") {
                read_outlines_from(&mut bookmark_data, doc, top_outline_p, 0);
            }
        }
    }
    bookmark_data
}

// ---------------------------------------------------------------------------------------------
// writing

/// Port of bookmarks.RemoveBookmarks(): unlink an outline (sub)tree so that its items become
/// unreferenced. (The Java expects the writer to drop the then-unused objects.)
pub fn remove_bookmarks(doc: &mut Document, bookmark_id: ObjectId) {
    let mut stack = vec![bookmark_id];
    let mut seen: HashSet<ObjectId> = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let mut next_ids: Vec<ObjectId> = Vec::new();
        if let Ok(dict) = doc.get_dictionary(id) {
            for key in [&b"First"[..], &b"Next"[..]] {
                if let Ok(o) = dict.get(key) {
                    if let Ok((Some(child_id), Object::Dictionary(_))) = doc.dereference(o) {
                        next_ids.push(child_id);
                    }
                }
            }
        }
        if let Ok(dict) = doc.get_dictionary_mut(id) {
            dict.remove(b"First");
            dict.remove(b"Next");
            dict.remove(b"Parent");
            dict.remove(b"Prev");
            dict.remove(b"Last");
        }
        stack.extend(next_ids);
    }
}

/// Port of bookmarks.BuildBookmarks() (the PdfReader flavour): create outline items for
/// `bookmarks[*pos..]` under `parent_id`, advancing `*pos`. Returns num_bookmarks_total.
/// `page_refs` are the document's pages in order (reader_p.getPageOrigRef()).
pub fn build_bookmarks_under(
    doc: &mut Document,
    bookmarks: &[PdfBookmark],
    pos: &mut usize,
    parent_id: ObjectId,
    parent_level: i32,
    page_refs: &[ObjectId],
) -> i32 {
    let mut num_bookmarks_total = 0;
    if *pos >= bookmarks.len() {
        return 0;
    }

    let mut bookmark_first_ref_p: Option<ObjectId> = None;
    let mut bookmark_prev_ref_p: Option<ObjectId> = None;
    let mut num_bookmarks = 0;

    if parent_level + 1 < bookmarks[*pos].m_level {
        // first child jumping levels: add missing level
        num_bookmarks += 1;
        num_bookmarks_total += 1;
        let mut bookmark_p = Dictionary::new();
        bookmark_p.set("Parent", Object::Reference(parent_id));
        bookmark_p.set("Title", Object::string_literal(Vec::<u8>::new()));
        let bookmark_ref_p = doc.add_object(Object::Dictionary(bookmark_p));
        bookmark_first_ref_p = Some(bookmark_ref_p);
        bookmark_prev_ref_p = Some(bookmark_ref_p);
        // recurse in loop
    }

    while *pos < bookmarks.len() {
        let it_content = &bookmarks[*pos];

        if parent_level + 1 < it_content.m_level {
            // encountered child; recurse
            let prev = match bookmark_prev_ref_p {
                Some(p) => p,
                None => break, // cannot happen: a jumping first child created its parent above
            };
            num_bookmarks_total +=
                build_bookmarks_under(doc, bookmarks, pos, prev, parent_level + 1, page_refs);
            continue;
        } else if it_content.m_level < parent_level + 1 {
            break; // no more children; add children to parent and return
        }

        // create child
        num_bookmarks += 1;
        num_bookmarks_total += 1;
        let mut bookmark_p = Dictionary::new();
        bookmark_p.set("Parent", Object::Reference(parent_id));
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
            if let Some(page_ref_p) = page_refs.get((it_content.m_page_num - 1) as usize) {
                dest_p.push(Object::Reference(*page_ref_p));
            }
            dest_p.push(Object::Name(b"Fit".to_vec()));
            bookmark_p.set("Dest", Object::Array(dest_p));
        }
        let bookmark_ref_p = doc.add_object(Object::Dictionary(bookmark_p));
        if bookmark_first_ref_p.is_none() {
            bookmark_first_ref_p = Some(bookmark_ref_p);
        }
        if let Some(prev) = bookmark_prev_ref_p {
            if let Ok(prev_p) = doc.get_dictionary_mut(prev) {
                prev_p.set("Next", Object::Reference(bookmark_ref_p));
            }
        }
        bookmark_prev_ref_p = Some(bookmark_ref_p);
        *pos += 1;
    }

    if let (Some(first), Some(last)) = (bookmark_first_ref_p, bookmark_prev_ref_p) {
        // pack these children into parent before returning
        if let Ok(parent_p) = doc.get_dictionary_mut(parent_id) {
            parent_p.set("First", Object::Reference(first));
            parent_p.set("Last", Object::Reference(last));
            if parent_level == 0 {
                parent_p.set("Count", Object::Integer(num_bookmarks_total as i64));
            } else {
                parent_p.set("Count", Object::Integer(num_bookmarks as i64));
            }
        }
    }

    num_bookmarks_total
}

/// Port of the bookmark part of data_import.UpdateInfo() + bookmarks.BuildBookmarks(): replace
/// the document outline tree with `bookmarks` (page numbers are 1-based pages of `doc`;
/// titles are plain Unicode). The old tree, if any, is unlinked. Does nothing for an empty list.
pub fn build_bookmarks(doc: &mut Document, bookmarks: &[PdfBookmark]) {
    if bookmarks.is_empty() {
        return;
    }
    let page_refs = pdf::page_ids(doc);

    let mut outlines_p = Dictionary::new();
    outlines_p.set("Type", Object::Name(b"Outlines".to_vec()));
    let outlines_ref_p = doc.add_object(Object::Dictionary(outlines_p));

    let mut pos = 0;
    build_bookmarks_under(doc, bookmarks, &mut pos, outlines_ref_p, 0, &page_refs);

    // erase old bookmarks
    let old_id = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| match doc.dereference(o) {
            Ok((Some(id), Object::Dictionary(_))) => Some(id),
            _ => None,
        });
    if let Some(old_id) = old_id {
        remove_bookmarks(doc, old_id);
    }

    // insert into document
    if let Ok(root_p) = doc.catalog_mut() {
        root_p.set("Outlines", Object::Reference(outlines_ref_p));
    }
}
