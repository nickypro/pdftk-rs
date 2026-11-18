//! Port of filter.java (single-input pipeline: fill_form, update_info, rotate, stamp/background,
//! attach_files, flatten, compress/uncompress, encrypt...). Owner: filter agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2023),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
use crate::cli;
use crate::forms;
use crate::pdf;
use crate::session::*;
use lopdf::{Document, Object, ObjectId};
use std::io::Read;

/// The catalog's /AcroForm dictionary: its object id if indirect, None if it is a direct
/// entry of the catalog (or absent).
fn acro_form_id(doc: &Document) -> Option<ObjectId> {
    let mut obj = doc.catalog().ok()?.get(b"AcroForm").ok()?;
    let mut id = None;
    for _ in 0..64 {
        match obj {
            Object::Reference(r) => {
                id = Some(*r);
                obj = doc.objects.get(r)?;
            }
            Object::Dictionary(_) => return id,
            _ => return None,
        }
    }
    None
}

/// Run `f` on the AcroForm dictionary, wherever it is stored.
fn with_acro_form(doc: &mut Document, f: impl FnOnce(&mut lopdf::Dictionary)) {
    match acro_form_id(doc) {
        Some(id) => {
            if let Ok(acro_form) = doc.get_dictionary_mut(id) {
                f(acro_form);
            }
        }
        None => {
            if let Ok(catalog) = doc.catalog_mut() {
                if let Ok(Object::Dictionary(acro_form)) = catalog.get_mut(b"AcroForm") {
                    f(acro_form);
                }
            }
        }
    }
}

/// apply operations to given PDF file
pub fn create_output_filter(session: &mut TkSession) -> ErrorCode {
    let mut ret_val = ErrorCode::NoError;

    // we should have been given only a single, input file
    if 1 < session.m_input_pdf.len() {
        // error
        eprintln!("Error: Only one input PDF file may be given for this");
        eprintln!("   operation.  Maybe you meant to use the \"cat\" operator?");
        return ErrorCode::Error;
    }

    // try opening the FDF file before we get too involved;
    // if input is stdin ("-"), don't pass it to both the FDF and XFDF readers
    let mut form_data: Option<forms::FormData> = None;
    if session.m_form_data_filename == "PROMPT" {
        // handle case where user enters '-' or (empty) at the prompt
        session.m_form_data_filename =
            cli::prompt_for_filename("Please enter a filename for the form data:");
    }
    if !session.m_form_data_filename.is_empty() {
        // we have form data to process
        if session.m_form_data_filename == "-" {
            // form data on stdin
            let mut buffer = Vec::new();
            let _ = std::io::stdin().lock().read_to_end(&mut buffer);
            // first try fdf, then xfdf
            match forms::read_form_data(&buffer) {
                Some(data) => form_data = Some(data),
                None => {
                    eprintln!("Error: Failed read form data on stdin.");
                    return ErrorCode::Error;
                }
            }
        } else {
            // form data file; first try fdf, then xfdf
            match std::fs::read(&session.m_form_data_filename)
                .ok()
                .and_then(|buffer| forms::read_form_data(&buffer))
            {
                Some(data) => form_data = Some(data),
                None => {
                    // file open error
                    eprintln!("Error: Failed to open form data file: ");
                    eprintln!("   {}", session.m_form_data_filename);
                    return ErrorCode::Error;
                }
            }
        }
    }

    // try opening the PDF background or stamp before we get too involved
    let mut mark_p: Option<Document> = None;
    let mut background_b = true; // set false for stamp

    // background
    if !session.m_background_filename.is_empty() {
        let mut input = InputPdf::new(&session.m_background_filename);
        input.m_role = "background".into();
        input.m_role_determined = "the background".into();
        match pdf::add_reader(&mut input, false, false) {
            Some(idx) => mark_p = Some(input.m_readers.swap_remove(idx).second),
            None => return ErrorCode::Error,
        }
    }

    // stamp
    if mark_p.is_none() && !session.m_stamp_filename.is_empty() {
        background_b = false;
        let mut input = InputPdf::new(&session.m_stamp_filename);
        input.m_role = "stamp".into();
        input.m_role_determined = "the stamp".into();
        match pdf::add_reader(&mut input, false, false) {
            Some(idx) => mark_p = Some(input.m_readers.swap_remove(idx).second),
            None => return ErrorCode::Error,
        }
    }

    //
    let Some(mut ofs_p) =
        cli::get_output_stream(&session.m_output_filename, session.m_ask_about_warnings_b)
    else {
        return ErrorCode::Error; // file open error
    };

    // the reader is consumed by writing, so take it out of the session
    let Some(reader) = session
        .m_input_pdf
        .first_mut()
        .and_then(|input| input.m_readers.first_mut())
    else {
        eprintln!("Internal Error: no input PDF reader in create_output_filter().");
        return ErrorCode::Bug;
    };
    let mut doc = std::mem::replace(&mut reader.second, Document::new());
    let input_reader_p = &mut doc;

    // drop the xfa?
    if session.m_output_drop_xfa_b {
        with_acro_form(input_reader_p, |acro_form| {
            acro_form.remove(b"XFA");
        });
    }

    // drop the xmp?
    if session.m_output_drop_xmp_b {
        if let Ok(catalog) = input_reader_p.catalog_mut() {
            catalog.remove(b"Metadata");
        }
    }

    // update the info?
    if session.m_update_info_filename == "PROMPT" {
        session.m_update_info_filename =
            cli::prompt_for_filename("Please enter an Info file filename:");
    }
    if !session.m_update_info_filename.is_empty() {
        if session.m_update_info_filename == "-" {
            let mut stdin = std::io::stdin().lock();
            if !crate::data_import::update_info(
                input_reader_p,
                &mut stdin,
                session.m_update_info_utf8_b,
            ) {
                eprintln!("Warning: no Info added to output PDF.");
                ret_val = ErrorCode::Warning;
            }
        } else {
            match std::fs::File::open(&session.m_update_info_filename) {
                Ok(ifs) => {
                    let mut ifs = std::io::BufReader::new(ifs);
                    if !crate::data_import::update_info(
                        input_reader_p,
                        &mut ifs,
                        session.m_update_info_utf8_b,
                    ) {
                        eprintln!("Warning: no Info added to output PDF.");
                        ret_val = ErrorCode::Warning;
                    }
                }
                Err(_) => {
                    // error
                    eprintln!(
                        "Error: unable to open FDF file for input: {}",
                        session.m_update_info_filename
                    );
                    return ErrorCode::Error;
                }
            }
        }
    }

    // rotate pages?
    for jt in &session.m_page_seq {
        for kt in jt {
            pdf::apply_rotation_to_page(
                input_reader_p,
                kt.m_page_num,
                kt.m_page_rot as i32,
                kt.m_page_abs,
            );
        }
    }

    if session.m_output_uncompress_b {
        pdf::add_marks_to_pages(input_reader_p);
    } else {
        pdf::remove_marks_from_pages(input_reader_p);
    }

    // fill form fields?
    let mut m_output_need_appearances_b = session.m_output_need_appearances_b;
    if let Some(data) = &form_data {
        if forms::has_acro_form(input_reader_p) {
            // we really have a form to fill
            let mut replacement_font = session.m_replacement_font.clone();
            if let Some(font) = &replacement_font {
                // (Java also accepts the name of a font installed on the system)
                if !std::path::Path::new(font).is_file() {
                    eprintln!("Warning: could not find replacement font.");
                    ret_val = ErrorCode::Warning;
                    replacement_font = None;
                }
            }
            if forms::fill_form(input_reader_p, data, replacement_font.as_deref()) {
                // Rich Text input found

                // set the PDF so that Acrobat will create appearances;
                // this might appear contradictory to our setGenerateAppearances( true ) call,
                // above; setting this, here, allows us to keep the generated appearances,
                // in case the PDF is opened somewhere besides Acrobat; yet, Acrobat/Reader
                // will create the Rich Text appearance if it has a chance
                m_output_need_appearances_b = true;
            }
        } else {
            // warning
            eprintln!("Warning: input PDF is not an acroform, so its fields were not filled.");
            ret_val = ErrorCode::Warning;
        }
    }

    // cue viewer to render form field appearances?
    if m_output_need_appearances_b {
        with_acro_form(input_reader_p, |acro_form| {
            acro_form.set("NeedAppearances", Object::Boolean(true));
        });
    }

    // add background/watermark?
    if let Some(mark) = mark_p.as_mut() {
        let multi_b = session.m_multistamp_b || session.m_multibackground_b;
        crate::stamp::apply_mark(input_reader_p, mark, background_b, multi_b);
    }

    // attach file to document?
    if !session.m_input_attach_file_filename.is_empty() {
        let attach_ret = crate::attachments::attach_files(session, input_reader_p);
        if attach_ret != ErrorCode::NoError {
            // an exception escaped attach_files(): no output is written
            return attach_ret;
        }
    }

    // flatten form fields? (iText does this as the stamper closes, i.e. after the
    // mark has been added to the page contents)
    if session.m_output_flatten_b {
        forms::flatten(input_reader_p);
    }

    // performed in add_reader(), but this eliminates objects after e.g. drop_xfa,
    // drop_xmp
    pdf::remove_unused_objects(input_reader_p);

    // done; write output
    let write_ret = pdf::write_output(session, input_reader_p, &mut *ofs_p);
    if write_ret != ErrorCode::NoError {
        return write_ret;
    }
    ret_val
}
