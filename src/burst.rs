//! Port of burst.java. Owner: core agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2020),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
#![allow(dead_code)]
use crate::pdf;
use crate::session::*;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashMap;
use std::io::Write;

/// burst input into pages
pub fn create_output_burst(session: &mut TkSession) -> ErrorCode {
    let mut ret_val = ErrorCode::NoError;

    // grab the first reader, since there's only one
    if session.m_input_pdf.is_empty() || session.m_input_pdf[0].m_readers.is_empty() {
        return ErrorCode::Error;
    }
    let input_num_pages = session.m_input_pdf[0].m_num_pages;
    let m_output_utf8_b = session.m_output_utf8_b;

    let mut m_output_filename = session.m_output_filename.clone();
    if m_output_filename == "PROMPT" {
        m_output_filename = crate::cli::prompt_for_filename(
            "Please enter a filename pattern for the PDF pages (e.g. pg_%04d.pdf):",
        );
    }
    let checked = (|| -> Result<(), FormatError> {
        let s1 = java_format(&m_output_filename, 1)?;
        let s2 = java_format(&m_output_filename, 2)?;
        if s1 == s2 {
            m_output_filename.push_str("pg_%04d.pdf");
            java_format(&m_output_filename, 1)?;
        }
        Ok(())
    })();
    if checked.is_err() {
        eprintln!("Error: Invalid output pattern:");
        eprintln!("   {}", m_output_filename);
        return ErrorCode::Error;
    }

    let session: &TkSession = session;
    let input_reader_p = &session.m_input_pdf[0].m_readers[0].second;
    let pages = pdf::page_ids(input_reader_p);

    for ii in 0..input_num_pages as usize {
        // the filename
        let output_filename_p = match java_format(&m_output_filename, ii as i32 + 1) {
            Ok(s) => s,
            Err(_) => return ErrorCode::Error,
        };
        let mut ofs_p = match crate::cli::get_output_stream_file(&output_filename_p, false, false) {
            Some(ofs) => ofs,
            None => {
                ret_val = ErrorCode::PartialOutput;
                continue;
            }
        };
        let page_id = match pages.get(ii) {
            Some(id) => *id,
            None => {
                ret_val = ErrorCode::PartialOutput;
                continue;
            }
        };

        let mut output_doc = single_page_document(input_reader_p, page_id);
        let written = pdf::write_output(session, &mut output_doc, &mut *ofs_p);
        if written != ErrorCode::NoError || ofs_p.flush().is_err() {
            return ErrorCode::Error;
        }
    }

    ////
    // dump document data

    let mut doc_data_fn = "doc_data.txt".to_string();
    if let Some(loc) = m_output_filename.rfind(std::path::MAIN_SEPARATOR) {
        doc_data_fn = format!(
            "{}{}{}",
            &m_output_filename[..loc],
            std::path::MAIN_SEPARATOR,
            doc_data_fn
        );
    }
    let reported = match crate::cli::get_print_stream(&doc_data_fn, m_output_utf8_b) {
        Some(mut ofs) => crate::report::report_on_pdf(&mut *ofs, input_reader_p, m_output_utf8_b)
            .and_then(|_| ofs.flush())
            .is_ok(),
        None => false,
    };
    if !reported {
        // error
        eprintln!("Error: unable to open file for output: doc_data.txt");
        ret_val = ErrorCode::PartialOutput;
    }
    ret_val
}

/// What burst.java builds with PdfCopy for one page: the page and everything it uses, the
/// form fields found on it (AcroForm /Fields and /DR), the input's Info dictionary and its
/// XMP metadata.
pub fn single_page_document(src: &Document, page_id: ObjectId) -> Document {
    let mut dst = Document::with_version("1.4"); // PdfWriter's default
    let mut map: HashMap<ObjectId, ObjectId> = HashMap::new();

    // setFromReader(): the input's page tree root stands for ours
    let pages_id = dst.new_object_id();
    dst.objects.insert(pages_id, Object::Dictionary(Dictionary::new()));
    if let Some(src_pages) = src
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .and_then(|o| o.as_reference().ok())
    {
        map.insert(src_pages, pages_id);
    }

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));

    // copy the Info dictionary metadata
    match src.trailer.get(b"Info") {
        Ok(Object::Reference(info_id))
            if matches!(src.objects.get(info_id), Some(Object::Dictionary(_))) =>
        {
            let id = pdf::copy_object_graph(src, &mut dst, *info_id, &mut map);
            dst.trailer.set("Info", Object::Reference(id));
        }
        Ok(Object::Dictionary(info)) => {
            let mut copy = Dictionary::new();
            for (key, value) in info.iter() {
                copy.set(key.clone(), copy_value(src, &mut dst, value, &mut map));
            }
            let id = dst.add_object(Object::Dictionary(copy));
            dst.trailer.set("Info", Object::Reference(id));
        }
        _ => {}
    }
    // reader.getMetadata() / writer.setXmpMetadata()
    if let Some(Object::Stream(xmp)) = src
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Metadata").ok())
        .and_then(|o| pdf::resolve(src, o))
    {
        let data = if xmp.dict.has(b"Filter") {
            xmp.decompressed_content().ok()
        } else {
            Some(xmp.content.clone())
        };
        if let Some(data) = data {
            let mut dict = Dictionary::new();
            dict.set("Type", Object::Name(b"Metadata".to_vec()));
            dict.set("Subtype", Object::Name(b"XML".to_vec()));
            let id = dst.add_object(Object::Stream(Stream::new(dict, data)));
            catalog.set("Metadata", Object::Reference(id));
        }
    }

    // the page
    let new_page = pdf::copy_object_graph(src, &mut dst, page_id, &mut map);
    if let Ok(page) = dst.get_dictionary_mut(new_page) {
        page.set("Parent", Object::Reference(pages_id));
    }

    // copy page form field ref.s into document AcroForm:
    // dig down to annot, and then dig up to topmost Parent
    let mut fields: Vec<Object> = Vec::new();
    let annots = src
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").ok())
        .and_then(|o| pdf::resolve(src, o))
        .and_then(|o| o.as_array().ok());
    for annot_ref in annots.into_iter().flatten() {
        // an annotation may be direct or indirect; ours must be indirect
        let mut annot_id = match annot_ref {
            Object::Reference(id) => *id,
            _ => continue,
        };
        let mut annot = match src.objects.get(&annot_id) {
            Some(Object::Dictionary(d)) => d,
            _ => continue,
        };
        let is_widget = matches!(
            annot.get(b"Subtype").ok().and_then(|o| pdf::resolve(src, o)),
            Some(Object::Name(n)) if n == b"Widget"
        );
        if !is_widget {
            continue;
        }
        // we have a form field; dig upwards, parent-wise
        for _ in 0..1000 {
            // PdfCopy.getParent(): the parent must really be another form field
            let parent_id = match annot.get(b"Parent") {
                Ok(Object::Reference(id)) => *id,
                _ => break,
            };
            match src.objects.get(&parent_id) {
                Some(Object::Dictionary(parent)) if parent.has(b"Kids") => {
                    annot_id = parent_id;
                    annot = parent;
                }
                _ => break,
            }
        }
        if let Some(new_id) = map.get(&annot_id) {
            let field = Object::Reference(*new_id);
            if !fields.contains(&field) {
                fields.push(field);
            }
        }
    }
    if !fields.is_empty() {
        let mut acro_form = Dictionary::new();
        acro_form.set("Fields", Object::Array(fields));
        // merge the reader AcroForm/DR dictionary with the target
        let dr = src
            .catalog()
            .ok()
            .and_then(|c| c.get(b"AcroForm").ok())
            .and_then(|o| pdf::resolve(src, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"DR").ok())
            .and_then(|o| pdf::resolve(src, o));
        if let Some(dr @ Object::Dictionary(_)) = dr {
            let dr_copy = copy_value(src, &mut dst, dr, &mut map);
            acro_form.set("DR", dr_copy);
        }
        let id = dst.add_object(Object::Dictionary(acro_form));
        catalog.set("AcroForm", Object::Reference(id));
    }

    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(vec![Object::Reference(new_page)]));
    pages.set("Count", Object::Integer(1));
    dst.objects.insert(pages_id, Object::Dictionary(pages));

    let catalog_id = dst.add_object(Object::Dictionary(catalog));
    dst.trailer.set("Root", Object::Reference(catalog_id));
    dst
}

/// Copy a direct object from `src`, bringing along whatever it refers to (PdfCopy.copyObject).
fn copy_value(
    src: &Document,
    dst: &mut Document,
    value: &Object,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Object {
    match value {
        Object::Reference(id) => Object::Reference(pdf::copy_object_graph(src, dst, *id, map)),
        Object::Array(items) => {
            Object::Array(items.iter().map(|o| copy_value(src, dst, o, map)).collect())
        }
        Object::Dictionary(dict) => {
            let mut copy = Dictionary::new();
            for (key, item) in dict.iter() {
                copy.set(key.clone(), copy_value(src, dst, item, map));
            }
            Object::Dictionary(copy)
        }
        other => other.clone(),
    }
}

////////////////////////////////////////////////////////////////////////////
// String.format(pattern, int)

/// Stands for java.util.IllegalFormatException.
#[derive(Debug, PartialEq)]
pub struct FormatError;

/// `String.format(pattern, arg)` for a single Integer argument, as burst uses it to turn the
/// output pattern into file names. Follows java.util.Formatter: `%[index$][flags][width]
/// [.precision]conversion`, and fails where Java throws an IllegalFormatException.
pub fn java_format(pattern: &str, arg: i32) -> Result<String, FormatError> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // the ordinary index: how many arguments plain specifiers have consumed
    let mut ordinary = 0usize;
    let mut last: Option<usize> = None;

    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        // argument index
        let mut index: Option<usize> = None;
        let mut relative = false;
        {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i && j < chars.len() && chars[j] == '$' {
                let n: String = chars[i..j].iter().collect();
                index = Some(n.parse::<usize>().map_err(|_| FormatError)?);
                i = j + 1;
            }
        }
        // flags
        let mut flags = String::new();
        while i < chars.len() && "-#+ 0,(<".contains(chars[i]) {
            if chars[i] == '<' {
                relative = true;
            } else {
                if flags.contains(chars[i]) {
                    return Err(FormatError); // DuplicateFormatFlagsException
                }
                flags.push(chars[i]);
            }
            i += 1;
        }
        // width
        let mut width: Option<usize> = None;
        {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i {
                let n: String = chars[i..j].iter().collect();
                width = Some(n.parse::<usize>().map_err(|_| FormatError)?);
                i = j;
            }
        }
        // precision
        let mut precision: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j == i + 1 {
                return Err(FormatError); // UnknownFormatConversionException
            }
            let n: String = chars[i + 1..j].iter().collect();
            precision = Some(n.parse::<usize>().map_err(|_| FormatError)?);
            i = j;
        }
        // date/time prefix: an Integer is not a valid date argument
        if i < chars.len() && (chars[i] == 't' || chars[i] == 'T') {
            return Err(FormatError);
        }
        let conv = *chars.get(i).ok_or(FormatError)?;
        i += 1;
        if !conv.is_ascii_alphabetic() && conv != '%' {
            return Err(FormatError);
        }

        let left = flags.contains('-');
        let zero = flags.contains('0');
        if (left || zero) && width.is_none() {
            return Err(FormatError); // MissingFormatWidthException
        }
        if left && zero || flags.contains('+') && flags.contains(' ') {
            return Err(FormatError); // IllegalFormatFlagsException
        }
        let pad = |body: String| -> String {
            let len = body.chars().count();
            match width {
                Some(w) if w > len => {
                    if left {
                        format!("{}{}", body, " ".repeat(w - len))
                    } else {
                        format!("{}{}", " ".repeat(w - len), body)
                    }
                }
                _ => body,
            }
        };

        // conversions that take no argument
        match conv {
            '%' => {
                if flags.chars().any(|c| c != '-') || precision.is_some() {
                    return Err(FormatError);
                }
                out.push_str(&pad("%".to_string()));
                continue;
            }
            'n' => {
                if !flags.is_empty() || width.is_some() || precision.is_some() {
                    return Err(FormatError);
                }
                out.push('\n');
                continue;
            }
            _ => {}
        }

        // which argument?
        let arg_index = if relative {
            last.ok_or(FormatError)? // MissingFormatArgumentException
        } else if let Some(n) = index {
            if n == 0 {
                return Err(FormatError);
            }
            n - 1
        } else {
            ordinary += 1;
            ordinary - 1
        };
        if arg_index != 0 {
            return Err(FormatError); // MissingFormatArgumentException
        }
        last = Some(arg_index);

        match conv {
            'd' => {
                if precision.is_some() || flags.contains('#') {
                    return Err(FormatError);
                }
                let negative = arg < 0;
                let mut digits = (arg as i64).abs().to_string();
                if flags.contains(',') {
                    let mut grouped = String::new();
                    for (k, c) in digits.chars().enumerate() {
                        if k > 0 && (digits.len() - k) % 3 == 0 {
                            grouped.push(',');
                        }
                        grouped.push(c);
                    }
                    digits = grouped;
                }
                let (prefix, suffix) = if negative {
                    if flags.contains('(') {
                        ("(", ")")
                    } else {
                        ("-", "")
                    }
                } else if flags.contains('+') {
                    ("+", "")
                } else if flags.contains(' ') {
                    (" ", "")
                } else {
                    ("", "")
                };
                if zero {
                    let used = prefix.len() + suffix.len() + digits.chars().count();
                    let w = width.unwrap_or(0);
                    if w > used {
                        digits = format!("{}{}", "0".repeat(w - used), digits);
                    }
                }
                out.push_str(&pad(format!("{}{}{}", prefix, digits, suffix)));
            }
            'o' | 'x' | 'X' => {
                if precision.is_some() || flags.chars().any(|c| !"-#0".contains(c)) {
                    return Err(FormatError);
                }
                let mut body = if conv == 'o' {
                    format!("{:o}", arg as u32)
                } else {
                    format!("{:x}", arg as u32)
                };
                let prefix = if flags.contains('#') {
                    if conv == 'o' {
                        "0"
                    } else {
                        "0x"
                    }
                } else {
                    ""
                };
                if zero {
                    let used = prefix.len() + body.len();
                    let w = width.unwrap_or(0);
                    if w > used {
                        body = format!("{}{}", "0".repeat(w - used), body);
                    }
                }
                let mut s = format!("{}{}", prefix, body);
                if conv == 'X' {
                    s = s.to_uppercase();
                }
                out.push_str(&pad(s));
            }
            's' | 'S' | 'b' | 'B' | 'h' | 'H' => {
                if flags.chars().any(|c| c != '-') {
                    return Err(FormatError);
                }
                let mut s = match conv {
                    's' | 'S' => arg.to_string(),
                    'b' | 'B' => "true".to_string(),
                    _ => format!("{:x}", arg as u32),
                };
                if let Some(p) = precision {
                    s = s.chars().take(p).collect();
                }
                if conv.is_ascii_uppercase() {
                    s = s.to_uppercase();
                }
                out.push_str(&pad(s));
            }
            'c' | 'C' => {
                if flags.chars().any(|c| c != '-') || precision.is_some() {
                    return Err(FormatError);
                }
                let c = u32::try_from(arg)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or(FormatError)?; // IllegalFormatCodePointException
                let mut s = c.to_string();
                if conv == 'C' {
                    s = s.to_uppercase();
                }
                out.push_str(&pad(s));
            }
            // e, f, g, a: IllegalFormatConversionException for an Integer;
            // anything else: UnknownFormatConversionException
            _ => return Err(FormatError),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyword::Keyword;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/files")
            .join(name)
    }

    fn open(path: &Path) -> Document {
        let bytes = std::fs::read(path).unwrap();
        let mut doc = pdf::open_pdf_bytes(&bytes, b"", "").unwrap().doc;
        // what add_reader() does when !keep_artifacts_b
        pdf::consolidate_named_destinations(&mut doc);
        pdf::remove_unused_objects(&mut doc);
        doc
    }

    fn field_count(doc: &Document) -> usize {
        doc.catalog()
            .ok()
            .and_then(|c| c.get(b"AcroForm").ok())
            .and_then(|o| pdf::resolve(doc, o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"Fields").ok())
            .and_then(|o| pdf::resolve(doc, o))
            .and_then(|o| o.as_array().ok())
            .map(|a| a.len())
            .unwrap_or(0)
    }

    // BurstTest.burst_issue18 / burst_issue90, and more: every page document must be valid,
    // hold exactly the page Java's burst produces, the same form fields and the input's Info.
    #[test]
    fn single_page_documents_match_java_burst() {
        let dir = std::env::temp_dir().join(format!("pdftk-rs-core-{}-burst", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = TkSession::new();
        session.m_operation = Keyword::burst_k;
        let mut total = 0;
        for name in ["issue18.pdf", "issue90.pdf", "refs.pdf", "form.pdf", "annotation.pdf", "blank.pdf"] {
            let src = open(&fixture(name));
            let java_pattern = dir.join(format!("java-{}-%04d.pdf", name));
            let out = Command::new("/usr/bin/pdftk")
                .current_dir(&dir)
                .args([fixture(name).to_str().unwrap(), "burst", "output", java_pattern.to_str().unwrap()])
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

            let src_info = src.trailer.get(b"Info").ok().and_then(|o| pdf::resolve(&src, o)).cloned();
            for (i, page_id) in pdf::page_ids(&src).into_iter().enumerate() {
                let mut doc = single_page_document(&src, page_id);
                let mut bytes = Vec::new();
                assert_eq!(pdf::write_output(&session, &mut doc, &mut bytes), ErrorCode::NoError);
                assert!(bytes.starts_with(b"%PDF-1.4"));
                let path = dir.join(format!("ours-{}-{:04}.pdf", name, i + 1));
                std::fs::write(&path, &bytes).unwrap();
                let check = Command::new("qpdf").args(["--check", path.to_str().unwrap()]).output().unwrap();
                let report = format!(
                    "{}{}",
                    String::from_utf8_lossy(&check.stdout),
                    String::from_utf8_lossy(&check.stderr)
                );
                assert!(check.status.success() && !report.contains("WARNING"), "{} page {}: {}", name, i + 1, report);

                let ours = pdf::open_pdf_bytes(&bytes, b"", "").unwrap().doc;
                let java = open(&dir.join(format!("java-{}-{:04}.pdf", name, i + 1)));
                let ours_pages = pdf::page_ids(&ours);
                let java_pages = pdf::page_ids(&java);
                assert_eq!((ours_pages.len(), java_pages.len()), (1, 1), "{} page {}", name, i + 1);
                assert_eq!(
                    ours.get_page_content(ours_pages[0]),
                    java.get_page_content(java_pages[0]),
                    "{} page {}",
                    name,
                    i + 1
                );
                assert_eq!(
                    pdf::page_media_box(&ours, ours_pages[0]),
                    pdf::page_media_box(&java, java_pages[0])
                );
                let annots = |d: &Document, p: ObjectId| {
                    pdf::page_attr(d, p, b"Annots").and_then(|a| a.as_array().ok().map(|a| a.len())).unwrap_or(0)
                };
                assert_eq!(annots(&ours, ours_pages[0]), annots(&java, java_pages[0]), "{} page {}", name, i + 1);
                assert_eq!(field_count(&ours), field_count(&java), "{} page {}", name, i + 1);
                assert_eq!(
                    ours.catalog().unwrap().has(b"Metadata"),
                    java.catalog().unwrap().has(b"Metadata"),
                    "{} page {}",
                    name,
                    i + 1
                );
                // no bookmarks in a burst page
                assert!(!ours.catalog().unwrap().has(b"Outlines"));

                // the input's Info entries win; pdftk only fills in what is missing
                let info = ours.trailer.get(b"Info").ok().and_then(|o| pdf::resolve(&ours, o)).unwrap().as_dict().unwrap();
                if let Some(Object::Dictionary(src_info)) = &src_info {
                    for (key, value) in src_info.iter() {
                        if matches!(value, Object::Reference(_)) {
                            // renumbered in the copy (issue18.pdf even points at its page tree)
                            assert!(info.has(key));
                            continue;
                        }
                        assert_eq!(info.get(key).ok(), Some(value), "{} page {}", name, i + 1);
                    }
                }
                for key in [&b"Creator"[..], b"Producer", b"CreationDate", b"ModDate"] {
                    assert!(info.has(key), "{} page {}", name, i + 1);
                }
                // the size is in the same ballpark as Java's (nothing dragged along)
                let java_len = std::fs::metadata(dir.join(format!("java-{}-{:04}.pdf", name, i + 1))).unwrap().len();
                assert!(
                    (bytes.len() as u64) < java_len * 3 / 2 + 2048,
                    "{} page {}: {} vs {}",
                    name,
                    i + 1,
                    bytes.len(),
                    java_len
                );
                total += 1;
            }
        }
        assert_eq!(total, 14);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn java_format_matches_java() {
        assert_eq!(java_format("pg_%04d.pdf", 7).unwrap(), "pg_0007.pdf");
        assert_eq!(java_format("page%d.pdf", 12).unwrap(), "page12.pdf");
        assert_eq!(java_format("%3d|%-3d|", 5).unwrap_err(), FormatError); // two arguments
        assert_eq!(java_format("%1$3d|%1$-3d|%<d", 5).unwrap(), "  5|5  |5");
        assert_eq!(java_format("100%%_%x_%X_%o", 255).unwrap_err(), FormatError);
        assert_eq!(java_format("100%%_%1$x_%1$X_%1$o_%1$#x", 255).unwrap(), "100%_ff_FF_377_0xff");
        assert_eq!(java_format("%,d", 1234567).unwrap(), "1,234,567");
        assert_eq!(java_format("%+d %1$(d", -5).unwrap(), "-5 (5)");
        assert_eq!(java_format("%s", 3).unwrap(), "3");
        assert_eq!(java_format("plain.pdf", 3).unwrap(), "plain.pdf");
        for bad in ["%", "%q", "%-d", "%0d", "%.2d", "%f", "%2$d", "%-05d", "%tY", "%#d", "abc%"] {
            assert_eq!(java_format(bad, 1), Err(FormatError), "{}", bad);
        }
    }
}
