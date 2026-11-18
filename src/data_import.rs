//! Port of data_import.java + Loadable*.java: parse a dump_data-format info file and apply it
//! (Info dictionary, bookmarks, page labels, page media) to a document. Owner: report agent.
#![allow(dead_code, unused)]
use lopdf::Document;

/// Port of data_import.UpdateInfo(). Returns false if nothing was applied (caller prints the warning).
pub fn update_info(_doc: &mut Document, _input: &mut dyn std::io::Read, _utf8_b: bool) -> bool { todo!("report agent") }
