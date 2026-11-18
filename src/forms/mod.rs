//! AcroForm work: dump_data_fields (report.ReportAcroFormFields), generate_fdf, FDF/XFDF reading,
//! fill_form (iText AcroFields.setFields incl. appearance generation), flatten (PdfStamperImp
//! form flattening). Owner: forms agent.
//!
//! Layout:
//! - `util`       object helpers, java.util.HashMap emulation, iText number formatting
//! - `dump`       report.ReportAcroFormFields (dump_data_fields[_utf8])
//! - `acro`       iText AcroFields: field discovery (fill()), getField()
//! - `fdfwrite`   iText FdfWriter (generate_fdf)
//! - `syntax`     PDF-syntax tokenizer / object parser (PRTokeniser) used for FDF, /DA, ToUnicode
//! - `fdfread`    iText FdfReader
//! - `xfdf`       iText XfdfReader + SimpleXMLParser
//! - `fonts`      BaseFont subset: built-in Type1 (AFM) fonts and DocumentFont
//! - `appearance` TextField / BaseField appearance streams (+ the ColumnText subset they use)
//! - `fill`       AcroFields.setFields / setField / setListSelection
//! - `flatten`    PdfStamperImp.flatFields
#![allow(dead_code)]
use lopdf::Document;

mod acro;
mod appearance;
mod dump;
mod fdfread;
mod fdfwrite;
mod fill;
mod flatten;
mod fonts;
mod syntax;
mod util;
mod xfdf;

#[cfg(test)]
mod tests;

/// Port of report.ReportAcroFormFields(): dump_data_fields[_utf8].
pub fn report_acro_form_fields(out: &mut dyn std::io::Write, doc: &Document, utf8_b: bool) -> std::io::Result<()> {
    dump::report_acro_form_fields(out, doc, utf8_b)
}

/// generate_fdf: write an FDF for the form in `doc` (iText FdfWriter as driven by TK_Session.create_output).
pub fn generate_fdf(out: &mut dyn std::io::Write, doc: &Document) -> std::io::Result<()> {
    let fields = acro::AcroFields::new(doc);
    let mut writer = fdfwrite::FdfWriter::new();
    fields.export_as_fdf(doc, &mut writer);
    writer.write_to(out)
}

/// Parsed form data (the union of what FdfReader and XfdfReader expose to AcroFields.setFields).
///
/// `fields` holds every fully-qualified field name that has a single value (`getFieldValue()`
/// non-null), in the order AcroFields.setFields() visits them (java.util.HashMap order).
/// `rich_values` and `multi_values` carry the rest of what setFields() consumes.
#[derive(Debug, Clone, Default)]
pub struct FormData {
    pub fields: Vec<(String, String)>,
    pub has_rich_text: bool,
    /// name -> rich text value (FDF /RV, XFDF <value-richtext>)
    pub rich_values: Vec<(String, String)>,
    /// name -> list selection (FdfReader.getFieldMultiValue / XfdfReader.getListValues)
    pub multi_values: Vec<(String, Vec<String>)>,
    /// every field name in visiting order (including fields without any value)
    pub names: Vec<String>,
}

/// Try FDF first, then XFDF, as filter.java does. None if neither parses.
pub fn read_form_data(bytes: &[u8]) -> Option<FormData> {
    match fdfread::read_fdf(bytes) {
        Ok(d) => Some(d),
        Err(_) => xfdf::read_xfdf(bytes).ok(),
    }
}

/// Does the document have an AcroForm (iText reader.getAcroForm() != null)?
pub fn has_acro_form(doc: &Document) -> bool {
    acro::reader_get_acro_form(doc)
}

/// Port of AcroFields.setFields() with setGenerateAppearances(true). Returns true if rich text
/// input was found (caller then sets NeedAppearances).
///
/// `replacement_font` (filter.java: AcroFields.setReplacementFont) is NOT supported: it needs
/// TrueType/CFF embedding as a subset Identity-H font. When given, a warning is printed and the
/// fields are filled with the fonts their /DA names, as if the option were absent.
pub fn fill_form(doc: &mut Document, data: &FormData, replacement_font: Option<&str>) -> bool {
    fill::fill_form(doc, data, replacement_font)
}

/// Port of PdfStamperImp form flattening (setFormFlattening(true)): merge widget appearances
/// into page content and remove the fields.
pub fn flatten(doc: &mut Document) {
    flatten::flatten(doc)
}
