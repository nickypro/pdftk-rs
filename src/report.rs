//! Port of report.java: dump_data, dump_data_utf8, dump_data_annots (+ PdfInfo/PdfBookmark/
//! PdfPageLabel/PdfPageMedia/PdfData records). dump_data_fields lives in forms.rs. Owner: report agent.
#![allow(dead_code, unused)]
use crate::session::*;
use lopdf::Document;

/// Port of report.ReportOnPdf(). `utf8_b` false => non-ASCII emitted as XML numeric entities.
pub fn report_on_pdf(_out: &mut dyn std::io::Write, _doc: &Document, _utf8_b: bool) -> std::io::Result<()> { todo!("report agent") }

/// Port of report.ReportAnnots().
pub fn report_annots(_out: &mut dyn std::io::Write, _doc: &Document, _utf8_b: bool) -> std::io::Result<()> { todo!("report agent") }

/// Port of report.OutputXmlString / OutputUtf8String: write `s`, escaping per utf8_b. Shared with forms.rs.
pub fn output_string(_out: &mut dyn std::io::Write, _s: &str, _utf8_b: bool) -> std::io::Result<()> { todo!("report agent") }

/// Port of report.getAnnots(): resolved annotation dictionaries (object ids) of a page.
pub fn get_annots(_doc: &Document, _page: lopdf::ObjectId) -> Vec<lopdf::ObjectId> { todo!("report agent") }
