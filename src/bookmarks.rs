//! Port of bookmarks.java (read outlines -> PdfBookmark list; build outlines from a list).
//! Owner: report agent. Also used by cat.rs when merging outlines.
#![allow(dead_code, unused)]
use lopdf::Document;

#[derive(Debug, Clone, Default)]
pub struct PdfBookmark { pub m_title: String, pub m_level: i32, pub m_page_num: i32 }

/// Port of bookmarks.ReadOutlines(): flatten the outline tree in document order.
pub fn read_outlines(_doc: &Document) -> Vec<PdfBookmark> { todo!("report agent") }

/// Port of bookmarks.BuildBookmarks(): replace the document outline tree with `bookmarks`
/// (page numbers are 1-based pages of `doc`).
pub fn build_bookmarks(_doc: &mut Document, _bookmarks: &[PdfBookmark]) { todo!("report agent") }
