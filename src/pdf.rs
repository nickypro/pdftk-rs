//! Shared PDF layer over lopdf: opening inputs (port of InputPdf.add_reader) and writing
//! output with pdftk output options (port of TK_Session.prepare_writer + the iText writer
//! behaviours pdftk relies on: compress/uncompress, encryption, /ID handling, Producer/Creator).
//! Owner: core agent. Other modules may only CALL these functions.
#![allow(dead_code, unused)]
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId};

/// Port of InputPdf.add_reader(): open (and decrypt) the PDF, print pdftk-style errors/warnings
/// to stderr, set m_num_pages, push onto m_readers. Returns the new reader index.
pub fn add_reader(_input: &mut InputPdf, _keep_artifacts_b: bool, _ask_about_warnings_b: bool) -> Option<usize> { todo!("core agent") }

/// Write `doc` to `out` applying the session output options (compress/uncompress, encryption,
/// user perms, keep_first_id/keep_final_id, drop_xmp is handled by callers).
/// `source_id` is the /ID array of the relevant input, for keep_*_id.
pub fn write_output(_session: &TkSession, _doc: &mut Document, _out: &mut dyn std::io::Write) -> ErrorCode { todo!("core agent") }

/// 1-based page number -> page object id, in document order.
pub fn page_ids(_doc: &Document) -> Vec<ObjectId> { todo!("core agent") }

/// Resolve a (possibly indirect) object.
pub fn resolve<'a>(_doc: &'a Document, _obj: &'a Object) -> Option<&'a Object> { todo!("core agent") }

/// Look up `key` on a page, walking /Parent for inheritable attributes (MediaBox, CropBox, Rotate, Resources).
pub fn page_attr(_doc: &Document, _page: ObjectId, _key: &[u8]) -> Option<Object> { todo!("core agent") }

/// Page /Rotate normalised to 0/90/180/270 (iText getPageRotation).
pub fn page_rotation(_doc: &Document, _page: ObjectId) -> i32 { todo!("core agent") }

/// [llx, lly, urx, ury], normalised. iText getPageSize (MediaBox) / getCropBox (CropBox, falling back to MediaBox).
pub fn page_media_box(_doc: &Document, _page: ObjectId) -> [f32; 4] { todo!("core agent") }
pub fn page_crop_box(_doc: &Document, _page: ObjectId) -> [f32; 4] { todo!("core agent") }

/// Port of TK_Session.apply_rotation_to_page().
pub fn apply_rotation_to_page(_doc: &mut Document, _page_num: u32, _rotation: i32, _absolute: bool) { todo!("core agent") }

/// Decode a PDF text string (PDFDocEncoding or UTF-16BE with BOM) to a Rust String (iText toUnicodeString).
pub fn text_string_to_unicode(_bytes: &[u8]) -> String { todo!("core agent") }

/// Encode a Rust String as a PDF text string: PDFDocEncoding if possible, else UTF-16BE with BOM (iText PdfString TEXT_UNICODE rule).
pub fn unicode_to_text_string(_s: &str) -> Vec<u8> { todo!("core agent") }

/// Copy the object graph reachable from `root` in `src` into `dst`, returning the new id of `root`.
/// `map` caches src-id -> dst-id so shared objects are copied once. /Parent links of pages are NOT followed
/// (the caller handles page-tree placement).
pub fn copy_object_graph(_src: &Document, _dst: &mut Document, _root: ObjectId, _map: &mut std::collections::HashMap<ObjectId, ObjectId>) -> ObjectId { todo!("core agent") }
