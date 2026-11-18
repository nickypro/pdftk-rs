//! AcroForm work: dump_data_fields (report.ReportAcroFormFields), generate_fdf, FDF/XFDF reading,
//! fill_form (iText AcroFields.setFields incl. appearance generation), flatten (PdfStamperImp
//! form flattening). Owner: forms agent. Split into submodules under src/forms/ if it grows.
#![allow(dead_code, unused)]
use crate::session::*;
use lopdf::Document;

/// Port of report.ReportAcroFormFields(): dump_data_fields[_utf8].
pub fn report_acro_form_fields(_out: &mut dyn std::io::Write, _doc: &Document, _utf8_b: bool) -> std::io::Result<()> { todo!("forms agent") }

/// generate_fdf: write an FDF for the form in `doc` (iText FdfWriter as driven by TK_Session.create_output).
pub fn generate_fdf(_out: &mut dyn std::io::Write, _doc: &Document) -> std::io::Result<()> { todo!("forms agent") }

/// Parsed form data: fully-qualified field name -> value (plus rich text flag).
pub struct FormData { pub fields: Vec<(String, String)>, pub has_rich_text: bool }

/// Try FDF first, then XFDF, as filter.java does. None if neither parses.
pub fn read_form_data(_bytes: &[u8]) -> Option<FormData> { todo!("forms agent") }

/// Does the document have an AcroForm (iText reader.getAcroForm() != null)?
pub fn has_acro_form(_doc: &Document) -> bool { todo!("forms agent") }

/// Port of AcroFields.setFields() with setGenerateAppearances(true). Returns true if rich text
/// input was found (caller then sets NeedAppearances). `replacement_font`: see filter.java.
pub fn fill_form(_doc: &mut Document, _data: &FormData, _replacement_font: Option<&str>) -> bool { todo!("forms agent") }

/// Port of PdfStamperImp form flattening (setFormFlattening(true)): merge widget appearances
/// into page content and remove the fields.
pub fn flatten(_doc: &mut Document) { todo!("forms agent") }
