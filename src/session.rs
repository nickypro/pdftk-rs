//! Port of TK_Session.java (state) plus the small value types
//! (PageRef, PageRotate, InputPdf, ErrorCode).
//!
//! SHARED CONTRACT: the struct fields here are read by every module.
//! Add fields if you need them, but do not rename or remove existing ones.
#![allow(dead_code)]
use crate::keyword::Keyword;
use std::collections::{HashMap, HashSet};

/// Port of ErrorCode.java. The numeric value is the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    NoError = 0,
    Error = 1,
    Bug = 2,
    PartialOutput = 3,
    Warning = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRotate {
    North = 0,
    East = 90,
    South = 180,
    West = 270,
}

#[derive(Debug, Clone)]
pub struct PageRef {
    pub m_input_pdf_index: usize,
    /// 1-based
    pub m_page_num: u32,
    pub m_page_rot: PageRotate,
    /// absolute (true) / relative (false) rotation
    pub m_page_abs: bool,
}

/// One opened copy of an input PDF. pdftk opens an extra reader whenever the
/// same page of an input is output twice; `first` tallies pages already claimed.
pub struct PagesReader {
    pub first: HashSet<u32>,
    pub second: lopdf::Document,
    /// true if the file was encrypted (already decrypted in `second`)
    pub encrypted: bool,
    pub owner_password_used: bool,
}

pub struct InputPdf {
    pub m_filename: String,
    pub m_password: String,
    pub m_authorized_b: bool,
    /// copy of stdin if the input is "-"
    pub m_buffer: Option<Vec<u8>>,
    pub m_role: String,
    pub m_role_determined: String,
    pub m_readers: Vec<PagesReader>,
    pub m_num_pages: u32,
}

impl InputPdf {
    pub fn new(filename: &str) -> Self {
        InputPdf {
            m_filename: filename.to_string(),
            m_password: String::new(),
            m_authorized_b: true,
            m_buffer: None,
            m_role: "input".into(),
            m_role_determined: "an input".into(),
            m_readers: Vec::new(),
            m_num_pages: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionStrength {
    None,
    Rc4_40,
    Rc4_128,
    Aes128,
}

pub struct TkSession {
    pub m_valid_b: bool,
    pub m_authorized_b: bool,
    pub m_input_pdf_readers_opened_b: bool,
    pub m_verbose_reporting_b: bool,
    pub m_ask_about_warnings_b: bool,

    pub m_input_pdf: Vec<InputPdf>,
    /// handle ("A", "B", ...) -> index into m_input_pdf
    pub m_input_pdf_index: HashMap<String, usize>,

    pub m_input_attach_file_filename: Vec<String>,
    pub m_input_attach_file_pagenum: i32,
    pub m_input_attach_file_relation: String,

    pub m_update_info_filename: String,
    pub m_update_info_utf8_b: bool,
    pub m_update_xmp_filename: String,

    pub m_operation: Keyword,
    pub m_page_seq: Vec<Vec<PageRef>>,

    pub m_form_data_filename: String,
    pub m_background_filename: String,
    pub m_stamp_filename: String,
    pub m_replacement_font: Option<String>,
    pub m_output_filename: String,
    pub m_output_utf8_b: bool,
    pub m_output_owner_pw: String,
    pub m_output_user_pw: String,
    pub m_output_user_perms: i32,
    pub m_multistamp_b: bool,
    pub m_multibackground_b: bool,
    pub m_output_uncompress_b: bool,
    pub m_output_compress_b: bool,
    pub m_output_flatten_b: bool,
    pub m_output_need_appearances_b: bool,
    pub m_output_drop_xfa_b: bool,
    pub m_output_drop_xmp_b: bool,
    pub m_output_keep_first_id_b: bool,
    pub m_output_keep_final_id_b: bool,
    pub m_cat_full_pdfs_b: bool,
    pub m_output_encryption_strength: EncryptionStrength,
    pub m_output_owner_pw_pdfdoc: Vec<u8>,
    pub m_output_user_pw_pdfdoc: Vec<u8>,
}

pub const PDFTK_VER: &str = "3.3.3";
pub const ASK_ABOUT_WARNINGS: bool = false;

impl TkSession {
    pub fn creator() -> String {
        format!("pdftk-rs {} (port of pdftk-java {})", env!("CARGO_PKG_VERSION"), PDFTK_VER)
    }

    pub fn new() -> Self {
        TkSession {
            m_valid_b: false,
            m_authorized_b: true,
            m_input_pdf_readers_opened_b: false,
            m_verbose_reporting_b: false,
            m_ask_about_warnings_b: ASK_ABOUT_WARNINGS,
            m_input_pdf: Vec::new(),
            m_input_pdf_index: HashMap::new(),
            m_input_attach_file_filename: Vec::new(),
            m_input_attach_file_pagenum: 0,
            m_input_attach_file_relation: "Unspecified".into(),
            m_update_info_filename: String::new(),
            m_update_info_utf8_b: false,
            m_update_xmp_filename: String::new(),
            m_operation: Keyword::none_k,
            m_page_seq: Vec::new(),
            m_form_data_filename: String::new(),
            m_background_filename: String::new(),
            m_stamp_filename: String::new(),
            m_replacement_font: None,
            m_output_filename: String::new(),
            m_output_utf8_b: false,
            m_output_owner_pw: String::new(),
            m_output_user_pw: String::new(),
            m_output_user_perms: 0,
            m_multistamp_b: false,
            m_multibackground_b: false,
            m_output_uncompress_b: false,
            m_output_compress_b: false,
            m_output_flatten_b: false,
            m_output_need_appearances_b: false,
            m_output_drop_xfa_b: false,
            m_output_drop_xmp_b: false,
            m_output_keep_first_id_b: false,
            m_output_keep_final_id_b: false,
            m_cat_full_pdfs_b: true,
            m_output_encryption_strength: EncryptionStrength::None,
            m_output_owner_pw_pdfdoc: Vec::new(),
            m_output_user_pw_pdfdoc: Vec::new(),
        }
    }

    /// Port of TK_Session.add_reader(): open one more reader for input `idx`.
    /// Returns the index into m_input_pdf[idx].m_readers, or None on failure.
    pub fn add_reader(&mut self, idx: usize, keep_artifacts_b: bool) -> Option<usize> {
        let ask = self.m_ask_about_warnings_b;
        let r = crate::pdf::add_reader(&mut self.m_input_pdf[idx], keep_artifacts_b, ask);
        self.m_authorized_b = self.m_authorized_b && self.m_input_pdf[idx].m_authorized_b;
        r
    }

    /// Port of TK_Session.open_input_pdf_readers().
    pub fn open_input_pdf_readers(&mut self) -> bool {
        let mut ok = true;
        if !self.m_input_pdf_readers_opened_b {
            if self.m_operation == Keyword::filter_k && self.m_input_pdf.len() == 1 {
                ok = self.add_reader(0, true).is_some();
            } else {
                for i in 0..self.m_input_pdf.len() {
                    ok = self.add_reader(i, false).is_some() && ok;
                }
            }
            self.m_input_pdf_readers_opened_b = ok;
        }
        ok
    }
}
