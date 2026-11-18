//! Port of pdftk.java (main, help/version text, prompts, output stream) and of
//! the TK_Session argument parser / is_valid / dump_session_data / create_output.
//! Owner: CLI agent.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2023),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
#![allow(dead_code)]
use crate::keyword::Keyword;
use crate::session::*;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};

// stdout/stderr printing that never panics (e.g. on a closed pipe)
macro_rules! outln {
    () => {{ let _ = writeln!(io::stdout()); }};
    ($($arg:tt)*) => {{ let _ = writeln!(io::stdout(), $($arg)*); }};
}
macro_rules! out {
    ($($arg:tt)*) => {{ let _ = write!(io::stdout(), $($arg)*); }};
}
macro_rules! errln {
    () => {{ let _ = writeln!(io::stderr()); }};
    ($($arg:tt)*) => {{ let _ = writeln!(io::stderr(), $($arg)*); }};
}

const SYNOPSIS: &str =
    include_str!("../resources/synopsis.txt");
const DESCRIPTION: &str =
    include_str!("../resources/description.txt");

// iText's PdfWriter permission bits
pub const ALLOW_PRINTING: i32 = 4 + 2048;
pub const ALLOW_MODIFY_CONTENTS: i32 = 8;
pub const ALLOW_COPY: i32 = 16;
pub const ALLOW_MODIFY_ANNOTATIONS: i32 = 32;
pub const ALLOW_FILL_IN: i32 = 256;
pub const ALLOW_SCREEN_READERS: i32 = 512;
pub const ALLOW_ASSEMBLY: i32 = 1024;
pub const ALLOW_DEGRADED_PRINTING: i32 = 4;

////////////////////////////////////////////////////////////////////////////
// pdftk.java

/// Read one line from stdin, like `new Scanner(System.in).nextLine()`.
/// Java throws NoSuchElementException at end of input, which main() reports
/// as a bug (exit code 2); we panic to get the same outcome.
fn read_stdin_line() -> String {
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(n) if n > 0 => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            line
        }
        _ => panic!("No line found (end of standard input while waiting for a reply)"),
    }
}

/// Port of pdftk.prompt_for_filename().
pub fn prompt_for_filename(message: &str) -> String {
    outln!("{}", message);
    read_stdin_line()
}

/// Port of pdftk.prompt_for_password().
pub fn prompt_for_password(pw_name: &str, pw_subject: &str) -> String {
    outln!("Please enter the {} password to use on {}.", pw_name, pw_subject);
    outln!("   It can be empty, or have a maximum of 32 characters:");
    let mut password = read_stdin_line();
    if 32 < password.chars().count() {
        // too long; trim
        let cut = password.char_indices().nth(32).map(|(i, _)| i).unwrap_or(password.len());
        outln!("The password you entered was over 32 characters long,");
        outln!("   so I am dropping: \"{}\"", &password[cut..]);
        password.truncate(cut);
    }
    password
}

/// Port of pdftk.warn_if_argument_is_password().
pub fn warn_if_argument_is_password(password: &str) {
    if !password.is_empty() && password != "PROMPT" {
        errln!("Warning: Using a password on the command line interface can be insecure.");
        errln!("Use the keyword PROMPT to supply a password via standard input instead.");
    }
}

/// Port of pdftk.confirm_overwrite() / file-exists check used by burst & unpack_files.
pub fn confirm_overwrite(filename: &str) -> bool {
    outln!(
        "Warning: the output file: {} already exists.  Overwrite? (y/n)",
        filename
    );
    let buff = read_stdin_line();
    buff.starts_with('y') || buff.starts_with('Y')
}

/// Port of pdftk.get_output_stream(): "-" is stdout, "PROMPT" prompts; refuses/asks before
/// overwriting when ask_about_warnings_b. Prints its own error and returns None on failure.
pub fn get_output_stream(
    output_filename: &str,
    ask_about_warnings_b: bool,
) -> Option<Box<dyn Write>> {
    let mut output_filename = output_filename.to_string();
    while output_filename.is_empty() || output_filename == "PROMPT" {
        output_filename = prompt_for_filename("Please enter a name for the output:");
    }
    if output_filename == "-" {
        // stdout
        return Some(Box::new(io::BufWriter::new(io::stdout())));
    }
    get_output_stream_file(&output_filename, ask_about_warnings_b, true)
}

/// Port of pdftk.get_output_stream_file(). With `check_overwrite`, an existing file
/// triggers confirm_overwrite(); if declined, a `required` output prompts for a new
/// name, otherwise the file is skipped. Prints its own errors; None on failure/skip.
///
/// Difference from Java: there, answering "y" to the overwrite question still
/// returned null (the file was never opened); here the file is overwritten.
pub fn get_output_stream_file(
    output_filename: &str,
    check_overwrite: bool,
    required: bool,
) -> Option<Box<dyn Write>> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    if check_overwrite {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(true);
    }
    let mut result = opts.open(output_filename);
    if let Err(e) = &result {
        if check_overwrite && e.kind() == io::ErrorKind::AlreadyExists {
            if confirm_overwrite(output_filename) {
                result = std::fs::File::create(output_filename);
            } else if required {
                return get_output_stream("PROMPT", check_overwrite);
            } else {
                errln!("   Skipping: {}", output_filename);
                return None;
            }
        }
    }
    match result {
        Ok(f) => Some(Box::new(io::BufWriter::new(f))),
        Err(_) => {
            errln!("Error: Failed to open output file: ");
            errln!("   {}", output_filename);
            if required {
                errln!("   No output created.");
            } else {
                errln!("   Skipping.");
            }
            None
        }
    }
}

/// Mimics a Java PrintStream in US-ASCII: every non-ASCII character becomes '?'.
struct AsciiWriter<W: Write>(W);

impl<W: Write> Write for AsciiWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_ascii() {
            self.0.write_all(buf)?;
        } else {
            let mut out = Vec::with_capacity(buf.len());
            for &b in buf {
                if b < 0x80 {
                    out.push(b);
                } else if b >= 0xC0 {
                    out.push(b'?'); // lead byte of a multi-byte character
                } // continuation bytes are dropped
            }
            self.0.write_all(&out)?;
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Port of pdftk.get_print_stream(): text output; "" or "-" is stdout. When `utf8_b`
/// is false the stream is US-ASCII (callers XML-escape non-ASCII; anything left over
/// is written as '?', as Java does). Returns None if the file cannot be opened; the
/// caller reports the error (Java throws FileNotFoundException here).
pub fn get_print_stream(output_filename: &str, utf8_b: bool) -> Option<Box<dyn Write>> {
    let inner: Box<dyn Write> = if output_filename.is_empty() || output_filename == "-" {
        Box::new(io::BufWriter::new(io::stdout()))
    } else {
        Box::new(io::BufWriter::new(std::fs::File::create(output_filename).ok()?))
    };
    if utf8_b {
        Some(inner)
    } else {
        Some(Box::new(AsciiWriter(inner)))
    }
}

/// Returns the process exit code. Port of pdftk.main_noexit().
pub fn main_noexit(args: &[String]) -> i32 {
    let mut help_b = false;
    let mut version_b = false;
    let synopsis_b = args.is_empty();
    let mut ret_val = ErrorCode::NoError;

    for argv in args {
        version_b = version_b || argv == "--version" || argv == "-version";
        help_b = help_b || argv == "--help" || argv == "-help" || argv == "-h";
    }

    if help_b {
        describe_full();
    } else if version_b {
        describe_header();
    } else if synopsis_b {
        describe_synopsis();
    } else {
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut session = TkSession::new();
            session.parse(args);

            session.dump_session_data();

            if session.is_valid() {
                // create_output() prints necessary error messages
                let mut ret = session.create_output();
                if ret == ErrorCode::Error {
                    errln!("   No output created.");
                } else if ret == ErrorCode::PartialOutput {
                    errln!("   Not all outputs created.");
                    ret = ErrorCode::Error;
                }
                ret
            } else {
                // error
                errln!("Done.  Input errors, so no output created.");
                ErrorCode::Error
            }
        }));
        ret_val = match run {
            Ok(r) => r,
            Err(_) => {
                // the panic hook has already printed the message
                errln!("Unhandled exception in main() (see above).");
                ErrorCode::Bug
            }
        };
    }
    let _ = io::stdout().flush();
    if ret_val == ErrorCode::Bug {
        describe_bug_report();
    }
    ret_val as i32
}

fn describe_header() {
    outln!(
        "pdftk-rs {} a Handy Tool for Manipulating PDF Documents",
        env!("CARGO_PKG_VERSION")
    );
    outln!(
        "pdftk-rs is a Rust port of pdftk-java {} (the pdftk port to java).",
        PDFTK_VER
    );
    outln!("Copyright (c) 2017-2023 Marc Vinyals - https://gitlab.com/pdftk-java/pdftk");
    outln!("Copyright (c) 2003-2013 Steward and Lee, LLC.");
    outln!("This is free software; see the source code for copying conditions. There is");
    outln!("NO warranty, not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.");
}

fn describe_synopsis() {
    out!("{}", SYNOPSIS);
}

fn describe_full() {
    describe_header();
    outln!();

    describe_synopsis();
    outln!();

    out!("{}", DESCRIPTION);
}

fn describe_bug_report() {
    errln!("There was a problem with pdftk-rs. Please report it,");
    errln!(
        "including the message above, the version of pdftk-rs ({}), and if possible steps to reproduce the error.",
        env!("CARGO_PKG_VERSION")
    );
}

////////////////////////////////////////////////////////////////////////////
// PageRange.java

struct PageRange<'a> {
    beg: i32,
    end: i32,
    m_num_pages: i32,
    m_argv: &'a str,
}

impl<'a> PageRange<'a> {
    fn new(num_pages: i32, argv: &'a str) -> Self {
        PageRange { beg: 0, end: 0, m_num_pages: num_pages, m_argv: argv }
    }

    fn parse_bound(&self, bound: &str) -> i32 {
        if bound == "end" {
            return self.m_num_pages;
        }
        // Java: Integer.parseInt() throws NumberFormatException on overflow,
        // which surfaces as an unhandled exception (exit code 2)
        match bound.parse::<i32>() {
            Ok(n) => n,
            Err(_) => panic!("NumberFormatException: For input string: \"{}\"", bound),
        }
    }

    fn out_of_range_error(&self, where_: &str) {
        errln!("Error: Range {} page number exceeds size of PDF", where_);
        errln!("   here: {}", self.m_argv);
        errln!("   input PDF has: {} pages.", self.m_num_pages);
        errln!("   Exiting.");
    }

    /// Matches `(r?)(end|[0-9]*)` at the start of `s`; returns (reverse, bound, bytes used).
    fn match_page(s: &str) -> (bool, &str, usize) {
        let mut pos = 0;
        let reverse = s.starts_with('r');
        if reverse {
            pos = 1;
        }
        let rest = &s[pos..];
        let len = if rest.starts_with("end") {
            3
        } else {
            rest.bytes().take_while(|b| b.is_ascii_digit()).count()
        };
        (reverse, &rest[..len], pos + len)
    }

    /// Consumes `(r?)(end|[0-9]*)(-(r?)(end|[0-9]*))?` from the front of `unparsed`.
    fn parse(&mut self, unparsed: &mut String) -> bool {
        let (pre_reverse, pre_range, mut used) = Self::match_page(unparsed);
        let pre_range = pre_range.to_string();
        let mut post: Option<(bool, String)> = None;
        if unparsed[used..].starts_with('-') {
            let (post_reverse, post_range, n) = Self::match_page(&unparsed[used + 1..]);
            post = Some((post_reverse, post_range.to_string()));
            used += 1 + n;
        }
        unparsed.drain(..used);
        self.parse_parts(pre_reverse, &pre_range, post)
    }

    fn parse_parts(
        &mut self,
        pre_reverse_b: bool,
        pre_range: &str,
        post: Option<(bool, String)>,
    ) -> bool {
        self.beg = 0; // default value
        if !pre_range.is_empty() {
            self.beg = self.parse_bound(pre_range);

            if self.m_num_pages < self.beg {
                // error: page number out of range
                self.out_of_range_error("start");
                return false;
            }

            if pre_reverse_b {
                // above test ensures good value here
                self.beg = self.m_num_pages - self.beg + 1;
            }
        }

        self.end = self.beg; // default value
        if let Some((post_reverse_b, post_range)) = post {
            if !post_range.is_empty() {
                self.end = self.parse_bound(&post_range);

                if self.m_num_pages < self.end {
                    // error: page number out of range
                    self.out_of_range_error("end");
                    return false;
                }

                if post_reverse_b {
                    // above test ensures good value here
                    self.end = self.m_num_pages - self.end + 1;
                }
            }
        }

        true
    }
}

////////////////////////////////////////////////////////////////////////////
// TK_Session.java: argument parser

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
enum ArgState {
    input_files_e,
    input_pw_e,

    page_seq_e,
    form_data_filename_e,

    attach_file_filename_e,
    attach_file_pagenum_e,
    attach_file_relation_e,

    update_info_filename_e,
    update_xmp_filename_e,

    output_e, // state where we expect output_k, next
    output_filename_e,

    output_args_e, // output args are order-independent; switch here
    output_owner_pw_e,
    output_user_pw_e,
    output_user_perms_e,

    background_filename_e,
    stamp_filename_e,
    replacement_font_filename_e,

    done_e,
}

/// Splits `[<HANDLE>=]<data>`, i.e. the regex `(?:([A-Z]+)=)?(.*)`.
fn split_handle(argv: &str) -> (Option<&str>, &str) {
    let n = argv.bytes().take_while(|b| b.is_ascii_uppercase()).count();
    if n > 0 && argv.as_bytes().get(n) == Some(&b'=') {
        (Some(&argv[..n]), &argv[n + 1..])
    } else {
        (None, argv)
    }
}

/// Port of commons-lang WordUtils.capitalizeFully() for a single word.
fn capitalize_fully(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for c in s.chars() {
        if c.is_whitespace() {
            at_word_start = true;
            out.push(c);
        } else if at_word_start {
            out.extend(c.to_uppercase());
            at_word_start = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

struct Parser<'s> {
    s: &'s mut TkSession,
    arg_state: ArgState,
    argv: String,
    arg_keyword: Keyword,

    // set one and only one to true when p/w used; use to
    // enforce rule that either all p/w use handles or no p/w use handles
    password_using_handles_not_b: bool,
    password_using_handles_b: bool,

    password_input_pdf_index: usize,

    fail_b: bool,
}

impl<'s> Parser<'s> {
    fn parse(&mut self, args: &[String]) {
        // first, look for our "dont_ask" or "do_ask" keywords, since this
        // setting must be known before we begin opening documents, etc.
        for argv in args {
            let kw = Keyword::is_keyword(argv);
            if kw == Keyword::dont_ask_k {
                self.s.m_ask_about_warnings_b = false;
            } else if kw == Keyword::do_ask_k {
                self.s.m_ask_about_warnings_b = true;
            }
        }

        // iterate over cmd line arguments
        for arg in args {
            self.argv = arg.clone();

            if self.fail_b || self.arg_state == ArgState::done_e {
                break;
            }
            self.arg_keyword = Keyword::is_keyword(&self.argv);

            // these keywords can be false hits because of their loose matching requirements;
            // since they are suffixes to page ranges, their appearance here is most likely a
            // false match;
            if self.arg_keyword == Keyword::even_k || self.arg_keyword == Keyword::odd_k {
                self.arg_keyword = Keyword::none_k;
            }
            match self.arg_state {
                ArgState::input_files_e | ArgState::input_pw_e => self.parse_state_input(),
                ArgState::page_seq_e => self.parse_state_page_seq(),
                ArgState::form_data_filename_e => self.parse_state_form_data_filename(),
                ArgState::attach_file_filename_e => self.parse_state_attach_file_filename(),
                ArgState::attach_file_pagenum_e => self.parse_state_attach_file_pagenum(),
                ArgState::attach_file_relation_e => self.parse_state_attach_file_relation(),
                ArgState::update_info_filename_e => self.parse_state_update_info_filename(),
                ArgState::output_e => self.parse_state_output(),
                ArgState::output_filename_e => self.parse_state_output_filename(),
                ArgState::output_args_e => self.parse_state_output_args(),
                ArgState::output_owner_pw_e => self.parse_state_output_owner_pw(),
                ArgState::output_user_pw_e => self.parse_state_output_user_pw(),
                ArgState::output_user_perms_e => self.parse_state_output_user_perms(),
                ArgState::background_filename_e => self.parse_state_background_filename(),
                ArgState::stamp_filename_e => self.parse_state_stamp_filename(),
                ArgState::replacement_font_filename_e => {
                    self.parse_state_replacement_font_filename()
                }
                _ => {
                    // error
                    errln!("Internal Error: Unexpected arg_state.  Exiting.");
                    self.fail_b = true;
                }
            }
        }

        if self.fail_b {
            errln!("Errors encountered.  No output created.");
            self.s.m_valid_b = false;

            self.s.m_input_pdf.clear();

            // preserve other data members for diagnostic dump
        } else {
            self.s.m_valid_b = true;

            if !self.s.m_input_pdf_readers_opened_b {
                self.s.open_input_pdf_readers();
            }
        }
    }

    fn parse_state_input(&mut self) {
        use ArgState::*;
        use Keyword::*;
        // look for keywords that would advance our state,
        // and then handle the specifics of the above cases
        let s = &mut *self.s;
        match self.arg_keyword {
            input_pw_k => {
                // input PDF passwords keyword
                self.arg_state = input_pw_e;
            }
            cat_k => {
                s.m_operation = cat_k;
                self.arg_state = page_seq_e; // collect page sequeces
            }
            shuffle_k => {
                s.m_operation = shuffle_k;
                self.arg_state = page_seq_e; // collect page sequeces
            }
            burst_k => {
                s.m_operation = burst_k;
                self.arg_state = output_args_e; // makes "output <fn>" bit optional
            }
            filter_k => {
                s.m_operation = filter_k;
                self.arg_state = output_e; // look for an output filename
            }
            dump_data_k => {
                s.m_operation = dump_data_k;
                self.arg_state = output_e;
            }
            dump_data_utf8_k => {
                s.m_operation = dump_data_k;
                s.m_output_utf8_b = true;
                self.arg_state = output_e;
            }
            dump_data_fields_k => {
                s.m_operation = dump_data_fields_k;
                self.arg_state = output_e;
            }
            dump_data_fields_utf8_k => {
                s.m_operation = dump_data_fields_k;
                s.m_output_utf8_b = true;
                self.arg_state = output_e;
            }
            dump_data_annots_k => {
                s.m_operation = dump_data_annots_k;
                self.arg_state = output_e;
            }
            generate_fdf_k => {
                s.m_operation = generate_fdf_k;
                s.m_output_utf8_b = true;
                self.arg_state = output_e;
            }
            fill_form_k => {
                s.m_operation = filter_k;
                self.arg_state = form_data_filename_e; // look for an FDF filename
            }
            attach_file_k => {
                s.m_operation = filter_k;
                self.arg_state = attach_file_filename_e;
            }
            attach_file_to_page_k => {
                self.arg_state = attach_file_pagenum_e;
            }
            attach_file_relation_k => {
                self.arg_state = attach_file_relation_e;
            }
            unpack_files_k => {
                s.m_operation = unpack_files_k;
                self.arg_state = output_e;
            }
            update_info_k => {
                s.m_operation = filter_k;
                s.m_update_info_utf8_b = false;
                self.arg_state = update_info_filename_e;
            }
            update_info_utf8_k => {
                s.m_operation = filter_k;
                s.m_update_info_utf8_b = true;
                self.arg_state = update_info_filename_e;
            }
            background_k => {
                s.m_operation = filter_k;
                self.arg_state = background_filename_e;
            }
            multibackground_k => {
                s.m_operation = filter_k;
                s.m_multibackground_b = true;
                self.arg_state = background_filename_e;
            }
            stamp_k => {
                s.m_operation = filter_k;
                self.arg_state = stamp_filename_e;
            }
            multistamp_k => {
                s.m_operation = filter_k;
                s.m_multistamp_b = true;
                self.arg_state = stamp_filename_e;
            }
            rotate_k => {
                s.m_operation = filter_k;
                self.arg_state = page_seq_e; // collect page sequeces
            }
            output_k => {
                // we reached the output section
                self.arg_state = output_filename_e;
            }
            none_k => {
                // here is where the two cases (input_files_e, input_pw_e) diverge
                let argv = self.argv.clone();
                let (handle, data) = split_handle(&argv);
                if self.arg_state == input_files_e {
                    self.parse_state_input_files(handle, data);
                } else if self.arg_state == input_pw_e {
                    self.parse_state_input_pw(handle, data);
                } else {
                    // error
                    errln!("Error: Internal error: unexpected arg_state.  Exiting.");
                    self.fail_b = true;
                }
            }
            _ => {
                // error: unexpected keyword; remark and set fail_b
                errln!("Error: Unexpected command-line data: ");
                errln!("      {}", self.argv);
                if self.arg_state == input_files_e {
                    errln!("   where we were expecting an input PDF filename,");
                    errln!("   operation (e.g. \"cat\") or \"input_pw\".  Exiting.");
                } else {
                    errln!("   where we were expecting an input PDF password");
                    errln!("   or operation (e.g. \"cat\").  Exiting.");
                }
                self.fail_b = true;
            }
        }
    }

    fn parse_state_input_files(&mut self, handle: Option<&str>, data: &str) {
        // input_files_e:
        // expecting input handle=filename pairs, or
        // an input filename w/o a handle
        //
        // treat argv[ii] like an optional input handle and filename
        // like this: [<handle>=]<filename>
        let input_pdf = InputPdf::new(data);

        match handle {
            None => self.s.m_input_pdf.push(input_pdf),
            Some(handle) => {
                // use given handle for filename; test, first
                if let Some(&it) = self.s.m_input_pdf_index.get(handle) {
                    // error: alreay in use
                    errln!("Error: Handle given here: ");
                    errln!("      {}", self.argv);
                    errln!("   is already associated with: ");
                    errln!("      {}", self.s.m_input_pdf[it].m_filename);
                    errln!("   Exiting.");
                    self.fail_b = true;
                } else {
                    // add handle/filename association
                    self.s.m_input_pdf.push(input_pdf);
                    let idx = self.s.m_input_pdf.len() - 1;
                    self.s.m_input_pdf_index.insert(handle.to_string(), idx);
                }
            }
        }
    }

    fn password_handle_rules(&self) {
        errln!();
        errln!("   Handles must be supplied with ~all~ input");
        errln!("   PDF passwords, or with ~no~ input PDF passwords.");
        errln!("   If no handles are supplied, then passwords are applied");
        errln!("   according to input PDF order.");
        errln!();
        errln!("   Handles are given like this: <handle>=<password>, and");
        errln!("   they must be one or more upper-case letters.");
    }

    fn parse_state_input_pw(&mut self, handle: Option<&str>, data: &str) {
        // expecting input handle=password pairs, or
        // an input PDF password w/o a handle
        //
        // treat argv[ii] like an input handle and password
        // like this <handle>=<password>; if no handle is
        // given, assign passwords to input in order;

        // if handles not used for input PDFs, then assume
        // any equals signs found in p/w are part of p/w
        let handle = if self.s.m_input_pdf_index.is_empty() { None } else { handle };

        match handle {
            None => {
                // no equal sign; try using default handles
                if self.password_using_handles_b {
                    // error: expected a handle
                    errln!("Error: Expected a user-supplied handle for this input");
                    errln!("   PDF password: {}", self.argv);
                    self.password_handle_rules();
                    self.fail_b = true;
                } else {
                    self.password_using_handles_not_b = true;

                    if self.password_input_pdf_index < self.s.m_input_pdf.len() {
                        warn_if_argument_is_password(&self.argv);
                        self.s.m_input_pdf[self.password_input_pdf_index].m_password =
                            self.argv.clone();
                        self.password_input_pdf_index += 1;
                    } else {
                        // error
                        errln!("Error: more input passwords than input PDF documents.");
                        errln!("   Exiting.");
                        self.fail_b = true;
                    }
                }
            }
            Some(handle) => {
                // handle given; use for password
                if self.password_using_handles_not_b {
                    // error; remark and set fail_b
                    errln!("Error: Expected ~no~ user-supplied handle for this input");
                    errln!("   PDF password: {}", self.argv);
                    self.password_handle_rules();
                    self.fail_b = true;
                } else {
                    self.password_using_handles_b = true;

                    // look up this handle
                    if let Some(&it) = self.s.m_input_pdf_index.get(handle) {
                        // found
                        if self.s.m_input_pdf[it].m_password.is_empty() {
                            warn_if_argument_is_password(data);
                            self.s.m_input_pdf[it].m_password = data.to_string(); // set
                        } else {
                            // error: password already given
                            errln!("Error: Handle given here: ");
                            errln!("      {}", self.argv);
                            errln!("   is already associated with this password: ");
                            errln!("      {}", self.s.m_input_pdf[it].m_password);
                            errln!("   Exiting.");
                            self.fail_b = true;
                        }
                    } else {
                        // error: no input file matches this handle
                        errln!("Error: Password handle: {}", self.argv);
                        errln!("   is not associated with an input PDF file.");
                        errln!("   Exiting.");
                        self.fail_b = true;
                    }
                }
            }
        }
    }

    fn parse_state_page_seq(&mut self) {
        if self.s.m_page_seq.is_empty() {
            // we just got here; validate input filenames

            if self.s.m_input_pdf.is_empty() {
                // error; remark and set fail_b
                errln!("Error: No input files.  Exiting.");
                self.fail_b = true;
                return;
            }

            // try opening input PDF readers
            if !self.s.open_input_pdf_readers() {
                // failure
                self.fail_b = true;
                return;
            }
        } // end: first pass init. pdf files

        if self.arg_keyword == Keyword::output_k {
            self.arg_state = ArgState::output_filename_e; // advance state
        } else if self.arg_keyword == Keyword::none_k {
            // treat argv[ii] like a page sequence
            let argv = self.argv.clone();

            let mut even_pages_b = false;
            let mut odd_pages_b = false;
            let mut exclude: HashSet<i32> = HashSet::new();

            // leading handle: ([A-Z]*)
            let handle_len = argv.bytes().take_while(|b| b.is_ascii_uppercase()).count();
            let handle = &argv[..handle_len];
            let mut unparsed = argv[handle_len..].to_string();
            let mut range_pdf_index = 0usize; // defaults to first input document
            if !handle.is_empty() {
                // validate handle
                match self.s.m_input_pdf_index.get(handle) {
                    None => {
                        // error
                        errln!("Error: Given handle has no associated file: ");
                        errln!("   {}, used here: {}", handle, argv);
                        errln!("   Exiting.");
                        self.fail_b = true;
                        return;
                    }
                    Some(&it) => range_pdf_index = it,
                }
            }
            let num_pages = self.s.m_input_pdf[range_pdf_index].m_num_pages as i32;

            let mut page_num = PageRange::new(num_pages, &argv);
            if !page_num.parse(&mut unparsed) {
                self.fail_b = true;
                return;
            }

            // DF declare rotate vars
            let mut page_rotate = PageRotate::North;
            let mut page_rotate_absolute = false;

            // trailing keywords (excluding "end" which should have been handled above)
            // possibly more than one keyword, e.g., 3-endevenwest
            while !unparsed.is_empty() {
                // read keyword
                self.arg_keyword = Keyword::consume_keyword(&mut unparsed);
                match self.arg_keyword {
                    Keyword::even_k => even_pages_b = true,
                    Keyword::odd_k => odd_pages_b = true,
                    Keyword::rot_north_k => {
                        page_rotate = PageRotate::North; // rotate 0
                        page_rotate_absolute = true;
                    }
                    Keyword::rot_east_k => {
                        page_rotate = PageRotate::East; // rotate 90
                        page_rotate_absolute = true;
                    }
                    Keyword::rot_south_k => {
                        page_rotate = PageRotate::South; // rotate 180
                        page_rotate_absolute = true;
                    }
                    Keyword::rot_west_k => {
                        page_rotate = PageRotate::West; // rotate 270
                        page_rotate_absolute = true;
                    }
                    Keyword::rot_left_k => {
                        page_rotate = PageRotate::West; // rotate -90
                        page_rotate_absolute = false;
                    }
                    Keyword::rot_right_k => {
                        page_rotate = PageRotate::East; // rotate +90
                        page_rotate_absolute = false;
                    }
                    Keyword::rot_upside_down_k => {
                        page_rotate = PageRotate::South; // rotate +180
                        page_rotate_absolute = false;
                    }
                    Keyword::range_minus_k => {
                        let mut exclude_range = PageRange::new(num_pages, &argv);
                        if exclude_range.parse(&mut unparsed) {
                            for i in exclude_range.beg..=exclude_range.end {
                                exclude.insert(i);
                            }
                        } else {
                            // (as in Java, this does not abort the run)
                            errln!("Error: Unexpected/invalid range in input after '~':");
                            errln!("   {}", argv);
                            errln!("   Exiting.");
                        }
                    }
                    _ => {
                        // error
                        errln!("Error: Unexpected text in page range end, here: ");
                        errln!("   {}", argv);
                        errln!("   Exiting.");
                        errln!("   Acceptable keywords, for example: \"even\" or \"odd\".");
                        errln!("   To rotate pages, use: \"north\" \"south\" \"east\"");
                        errln!("       \"west\" \"left\" \"right\" or \"down\"");
                        self.fail_b = true;
                        return;
                    }
                }
            }

            ////
            // pack this range into our m_page_seq;

            if page_num.beg == 0 && page_num.end == 0 {
                // ref the entire document
                page_num.beg = 1;
                page_num.end = num_pages;

                // test that it's a /full/ pdf
                self.s.m_cat_full_pdfs_b = self.s.m_cat_full_pdfs_b
                    && (!even_pages_b && !odd_pages_b)
                    && exclude.is_empty();
            } else if page_num.beg == 0 || page_num.end == 0 {
                // error
                errln!("Error: Input page numbers include 0 (zero)");
                errln!("   The first PDF page is 1 (one)");
                errln!("   Exiting.");
                self.fail_b = true;
                return;
            } else {
                // the user specified select pages
                self.s.m_cat_full_pdfs_b = false;
            }

            let mut temp_page_seq: Vec<PageRef> = Vec::new();
            let reverse_sequence_b = page_num.end < page_num.beg;
            if reverse_sequence_b {
                // swap
                std::mem::swap(&mut page_num.beg, &mut page_num.end);
            }

            for kk in page_num.beg..=page_num.end {
                if even_pages_b && (kk % 2 != 0) {
                    continue;
                }
                if odd_pages_b && (kk % 2 != 1) {
                    continue;
                }
                if exclude.contains(&kk) {
                    continue;
                }
                if kk > num_pages {
                    // error; break later to get most feedback
                    errln!("Error: Page number: {}", kk);
                    errln!(
                        "   does not exist in file: {}",
                        self.s.m_input_pdf[range_pdf_index].m_filename
                    );
                    self.fail_b = true;
                    continue;
                }

                // look to see if this page of this document
                // has already been referenced; if it has,
                // create a new reader; associate this page
                // with a reader;
                let page = kk as u32;
                let mut associated = false;
                for it in self.s.m_input_pdf[range_pdf_index].m_readers.iter_mut() {
                    if !it.first.contains(&page) {
                        // kk not assoc. w/ this reader
                        it.first.insert(page); // create association
                        associated = true;
                        break;
                    }
                }

                if !associated {
                    // need to create a new reader for kk
                    match self.s.add_reader(range_pdf_index, false) {
                        Some(new_reader) => {
                            self.s.m_input_pdf[range_pdf_index].m_readers[new_reader]
                                .first
                                .insert(page);
                        }
                        None => {
                            errln!("Internal Error: unable to add reader");
                            self.fail_b = true;
                            break;
                        }
                    }
                }

                temp_page_seq.push(PageRef {
                    m_input_pdf_index: range_pdf_index,
                    m_page_num: page,
                    m_page_rot: page_rotate,
                    m_page_abs: page_rotate_absolute,
                }); // DF rotate
            }
            if self.fail_b {
                return;
            }

            if reverse_sequence_b {
                temp_page_seq.reverse();
            }

            self.s.m_page_seq.push(temp_page_seq);
        } else {
            // error
            errln!("Error: expecting page ranges.  Instead, I got:");
            errln!("   {}", self.argv);
            self.fail_b = true;
        }
    }

    fn parse_state_form_data_filename(&mut self) {
        if self.arg_keyword == Keyword::none_k {
            // treat argv[ii] like an FDF file filename

            if self.s.m_form_data_filename.is_empty() {
                self.s.m_form_data_filename = self.argv.clone();
            } else {
                // error
                errln!("Error: Multiple fill_form filenames given: ");
                errln!("   {} and {}", self.s.m_form_data_filename, self.argv);
                errln!("Exiting.");
                self.fail_b = true;
                return;
            }

            // advance state
            self.arg_state = ArgState::output_e; // look for an output filename
        } else {
            // error
            errln!("Error: expecting a form data filename,");
            errln!("   instead I got this keyword: {}", self.argv);
            errln!("Exiting.");
            self.fail_b = true;
        }
    }

    fn parse_state_attach_file_filename(&mut self) {
        // keep packing filenames until we reach an expected keyword
        match self.arg_keyword {
            Keyword::attach_file_to_page_k => {
                self.arg_state = ArgState::attach_file_pagenum_e; // advance state
            }
            Keyword::attach_file_relation_k => {
                self.arg_state = ArgState::attach_file_relation_e; // advance state
            }
            Keyword::output_k => {
                self.arg_state = ArgState::output_filename_e; // advance state
            }
            Keyword::none_k => {
                // pack argv[ii] into our list of attachment filenames
                self.s.m_input_attach_file_filename.push(self.argv.clone());
            }
            _ => {
                // error
                errln!("Error: expecting an attachment filename,");
                errln!("   instead I got this keyword: {}", self.argv);
                errln!("Exiting.");
                self.fail_b = true;
            }
        }
    }

    fn parse_state_attach_file_pagenum(&mut self) {
        if self.argv == "PROMPT" {
            // query the user, later
            self.s.m_input_attach_file_pagenum = -1;
        } else if self.argv == "end" {
            // attach to the final page
            self.s.m_input_attach_file_pagenum = -2;
        } else {
            match self.argv.parse::<i32>() {
                Ok(n) => self.s.m_input_attach_file_pagenum = n,
                Err(_) => {
                    // error
                    errln!("Error: expecting a (1-based) page number.  Instead, I got:");
                    errln!("   {}", self.argv);
                    errln!("Exiting.");
                    self.fail_b = true;
                }
            }
        }

        // advance state
        self.arg_state = ArgState::output_e; // look for an output filename
    }

    fn parse_state_attach_file_relation(&mut self) {
        const STANDARD: [&str; 5] = ["source", "data", "alternative", "supplement", "unspecified"];
        if STANDARD.iter().any(|r| r.eq_ignore_ascii_case(&self.argv)) {
            self.argv = capitalize_fully(&self.argv);
        } else {
            errln!("Warning: non-standard attachment relationship: {}.", self.argv);
        }
        self.s.m_input_attach_file_relation = self.argv.clone();

        // advance state
        self.arg_state = ArgState::output_e; // look for an output filename
    }

    fn parse_state_update_info_filename(&mut self) {
        if self.arg_keyword == Keyword::none_k {
            if self.s.m_update_info_filename.is_empty() {
                self.s.m_update_info_filename = self.argv.clone();
            } else {
                // error
                errln!("Error: Multiple update_info filenames given: ");
                errln!("   {} and {}", self.s.m_update_info_filename, self.argv);
                errln!("Exiting.");
                self.fail_b = true;
                return;
            }
        } else {
            // error
            errln!("Error: expecting an INFO file filename,");
            errln!("   instead I got this keyword: {}", self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // advance state
        self.arg_state = ArgState::output_e; // look for an output filename
    }

    fn parse_state_output(&mut self) {
        if self.s.m_input_pdf.is_empty() {
            // error; remark and set fail_b
            errln!("Error: No input files.  Exiting.");
            self.fail_b = true;
            return;
        }

        if self.arg_keyword == Keyword::output_k {
            self.arg_state = ArgState::output_filename_e; // advance state
        } else {
            // error
            errln!("Error: expecting \"output\" keyword.  Instead, I got:");
            errln!("   {}", self.argv);
            self.fail_b = true;
        }
    }

    fn parse_state_output_filename(&mut self) {
        // we have closed all possible input operations and arguments;
        // see if we should perform any default action based on the input state
        //
        if self.s.m_operation == Keyword::none_k {
            if 1 < self.s.m_input_pdf.len() {
                // no operation given for multiple input PDF, so combine them
                self.s.m_operation = Keyword::cat_k;
            } else {
                self.s.m_operation = Keyword::filter_k;
            }
        }

        // try opening input PDF readers (in case they aren't already)
        if !self.s.open_input_pdf_readers() {
            // failure
            self.fail_b = true;
            return;
        }

        if (self.s.m_operation == Keyword::cat_k || self.s.m_operation == Keyword::shuffle_k)
            && self.s.m_page_seq.is_empty()
        {
            // combining pages, but no sequences given; merge all input PDFs in order
            for ii in 0..self.s.m_input_pdf.len() {
                let input_pdf = &mut self.s.m_input_pdf[ii];

                let mut temp_page_seq: Vec<PageRef> = Vec::new();
                for jj in 1..=input_pdf.m_num_pages {
                    temp_page_seq.push(PageRef {
                        m_input_pdf_index: ii,
                        m_page_num: jj,
                        m_page_rot: PageRotate::North,
                        m_page_abs: false,
                    }); // DF rotate
                    if let Some(last) = input_pdf.m_readers.last_mut() {
                        last.first.insert(jj); // create association
                    }
                }
                self.s.m_page_seq.push(temp_page_seq);
            }
        }

        if self.s.m_output_filename.is_empty() {
            self.s.m_output_filename = self.argv.clone();

            if self.s.m_output_filename != "-" {
                // input and output may both be "-" (stdin and stdout)
                // simple-minded test to see if output matches an input filename
                for it in &self.s.m_input_pdf {
                    if it.m_filename == self.s.m_output_filename {
                        errln!(
                            "Error: The given output filename: {}",
                            self.s.m_output_filename
                        );
                        errln!("   matches an input filename.  Exiting.");
                        self.fail_b = true;
                        break;
                    }
                }
            }
        } else {
            // error
            errln!("Error: Multiple output filenames given: ");
            errln!("   {} and {}", self.s.m_output_filename, self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // advance state
        self.arg_state = ArgState::output_args_e;
    }

    fn parse_state_output_args(&mut self) {
        // output args are order-independent but must follow "output <fn>", if present;
        // we are expecting any of these keywords:
        // owner_pw_k, user_pw_k, user_perms_k ...
        // added output_k case in pdftk 1.10; this permits softer "output <fn>" enforcement
        //
        if !self.handle_some_output_options() {
            errln!("Error: Unexpected data in output section: ");
            errln!("      {}", self.argv);
            errln!("Exiting.");
            self.fail_b = true;
        }
    }

    fn same_passwords_error(&mut self) {
        // identical user and owner password are interpreted by Acrobat
        // (per the spec.) that the doc has no owner password
        errln!("Error: The user and owner passwords are the same.");
        errln!("   PDF Viewers interpret this to mean your PDF has");
        errln!("   no owner password, so they must be different.");
        errln!("   Or, supply no owner password to pdftk if this is");
        errln!("   what you desire.");
        errln!("Exiting.");
        self.fail_b = true;
    }

    fn parse_state_output_owner_pw(&mut self) {
        if self.s.m_output_owner_pw.is_empty() {
            if self.argv == "PROMPT" || self.argv != self.s.m_output_user_pw {
                warn_if_argument_is_password(&self.argv);
                self.s.m_output_owner_pw = self.argv.clone();
            } else {
                self.same_passwords_error();
                return;
            }
        } else {
            // error: we already have an output owner pw
            errln!("Error: Multiple output owner passwords given: ");
            errln!("   {} and {}", self.s.m_output_owner_pw, self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // revert state
        self.arg_state = ArgState::output_args_e;
    }

    fn parse_state_output_user_pw(&mut self) {
        if self.s.m_output_user_pw.is_empty() {
            if self.argv == "PROMPT" || self.s.m_output_owner_pw != self.argv {
                warn_if_argument_is_password(&self.argv);
                self.s.m_output_user_pw = self.argv.clone();
            } else {
                self.same_passwords_error();
                return;
            }
        } else {
            // error: we already have an output user pw
            errln!("Error: Multiple output user passwords given: ");
            errln!("   {} and {}", self.s.m_output_user_pw, self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // revert state
        self.arg_state = ArgState::output_args_e;
    }

    fn parse_state_output_user_perms(&mut self) {
        // we may be given any number of permission arguments,
        // so keep an eye out for other, state-altering keywords
        if self.handle_some_output_options() {
            return;
        }

        let perms = &mut self.s.m_output_user_perms;
        match self.arg_keyword {
            // possible permissions
            Keyword::perm_printing_k => {
                // if both perm_printing_k and perm_degraded_printing_k
                // are given, then perm_printing_k wins;
                *perms |= ALLOW_PRINTING;
            }
            Keyword::perm_modify_contents_k => {
                // Acrobat 5 and 6 don't set both bits, even though
                // they both respect AllowModifyContents --> AllowAssembly;
                // so, no harm in this;
                *perms |= ALLOW_MODIFY_CONTENTS | ALLOW_ASSEMBLY;
            }
            Keyword::perm_copy_contents_k => {
                // Acrobat 5 _does_ allow the user to allow copying contents
                // yet hold back screen reader perms; this is counter-intuitive,
                // and Acrobat 6 does not allow Copy w/o SceenReaders;
                *perms |= ALLOW_COPY | ALLOW_SCREEN_READERS;
            }
            Keyword::perm_modify_annotations_k => {
                *perms |= ALLOW_MODIFY_ANNOTATIONS | ALLOW_FILL_IN;
            }
            Keyword::perm_fillin_k => *perms |= ALLOW_FILL_IN,
            Keyword::perm_screen_readers_k => *perms |= ALLOW_SCREEN_READERS,
            Keyword::perm_assembly_k => *perms |= ALLOW_ASSEMBLY,
            Keyword::perm_degraded_printing_k => *perms |= ALLOW_DEGRADED_PRINTING,
            Keyword::perm_all_k => {
                *perms = ALLOW_PRINTING // top quality printing
                    | ALLOW_MODIFY_CONTENTS
                    | ALLOW_COPY
                    | ALLOW_MODIFY_ANNOTATIONS
                    | ALLOW_FILL_IN
                    | ALLOW_SCREEN_READERS
                    | ALLOW_ASSEMBLY;
            }
            _ => {
                // error: unexpected matter
                errln!("Error: Unexpected data in output section: ");
                errln!("      {}", self.argv);
                errln!("Exiting.");
                self.fail_b = true;
            }
        }
    }

    fn parse_state_background_filename(&mut self) {
        if self.arg_keyword == Keyword::none_k {
            if self.s.m_background_filename.is_empty() {
                self.s.m_background_filename = self.argv.clone();
            } else {
                // error
                errln!("Error: Multiple background filenames given: ");
                errln!("   {} and {}", self.s.m_background_filename, self.argv);
                errln!("Exiting.");
                self.fail_b = true;
                return;
            }
        } else {
            // error
            errln!("Error: expecting a PDF filename for background operation,");
            errln!("   instead I got this keyword: {}", self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // revert state
        // this is more liberal than used with other operations, since we want
        // to preserve backward-compatibility with pdftk 1.00 where "background"
        // was documented as an output option; in pdftk 1.10 we changed it to
        // an operation
        self.arg_state = ArgState::output_args_e;
    }

    fn parse_state_stamp_filename(&mut self) {
        if self.arg_keyword == Keyword::none_k {
            if self.s.m_stamp_filename.is_empty() {
                self.s.m_stamp_filename = self.argv.clone();
            } else {
                // error
                errln!("Error: Multiple stamp filenames given: ");
                errln!("   {} and {}", self.s.m_stamp_filename, self.argv);
                errln!("Exiting.");
                self.fail_b = true;
                return;
            }
        } else {
            // error
            errln!("Error: expecting a PDF filename for stamp operation,");
            errln!("   instead I got this keyword: {}", self.argv);
            errln!("Exiting.");
            self.fail_b = true;
            return;
        }

        // advance state
        self.arg_state = ArgState::output_e; // look for an output filename
    }

    fn parse_state_replacement_font_filename(&mut self) {
        self.s.m_replacement_font = Some(self.argv.clone());
        self.arg_state = ArgState::output_args_e;
    }

    /// Convenience function; returns true iff `self.arg_keyword` was handled
    /// (possibly changing `self.arg_state`).
    fn handle_some_output_options(&mut self) -> bool {
        let s = &mut *self.s;
        match self.arg_keyword {
            Keyword::output_k => {
                // added this case for the burst operation and "output" support;
                // also helps with backward compatibility of the "background" feature
                // change state
                self.arg_state = ArgState::output_filename_e;
            }

            // state-altering keywords
            Keyword::owner_pw_k => self.arg_state = ArgState::output_owner_pw_e,
            Keyword::user_pw_k => self.arg_state = ArgState::output_user_pw_e,
            Keyword::user_perms_k => self.arg_state = ArgState::output_user_perms_e,

            ////
            // no arguments to these keywords, so the state remains unchanged
            Keyword::encrypt_40bit_k => {
                s.m_output_encryption_strength = EncryptionStrength::Rc4_40
            }
            Keyword::encrypt_128bit_k => {
                s.m_output_encryption_strength = EncryptionStrength::Rc4_128
            }
            Keyword::encrypt_aes128_k => {
                s.m_output_encryption_strength = EncryptionStrength::Aes128
            }
            Keyword::filt_uncompress_k => s.m_output_uncompress_b = true,
            Keyword::filt_compress_k => s.m_output_compress_b = true,
            Keyword::flatten_k => s.m_output_flatten_b = true,
            Keyword::need_appearances_k => s.m_output_need_appearances_b = true,
            Keyword::drop_xfa_k => s.m_output_drop_xfa_b = true,
            Keyword::drop_xmp_k => s.m_output_drop_xmp_b = true,
            Keyword::keep_first_id_k => s.m_output_keep_first_id_b = true,
            Keyword::keep_final_id_k => s.m_output_keep_final_id_b = true,
            Keyword::verbose_k => s.m_verbose_reporting_b = true,
            Keyword::dont_ask_k => s.m_ask_about_warnings_b = false,
            Keyword::do_ask_k => s.m_ask_about_warnings_b = true,

            Keyword::background_k => {
                if s.m_operation != Keyword::filter_k {
                    // warning
                    errln!("Warning: the \"background\" output option works only in filter mode.");
                    errln!("  This means it won't work in combination with \"cat\", \"burst\",");
                    errln!("  \"attach_file\", etc.  To run pdftk in filter mode, simply omit");
                    errln!("  the operation, e.g.: pdftk in.pdf output out.pdf background back.pdf");
                    errln!("  Or, use background as an operation; this is the preferred technique:");
                    errln!("    pdftk in.pdf background back.pdf output out.pdf");
                }
                // change state
                self.arg_state = ArgState::background_filename_e;
            }

            Keyword::replacement_font_k => {
                self.arg_state = ArgState::replacement_font_filename_e;
            }

            _ => return false, // not handled here; no change to arg_state
        }

        true
    }
}

////////////////////////////////////////////////////////////////////////////
// TK_Session.java: parse / is_valid / dump_session_data / create_output

impl TkSession {
    /// Port of TK_Session.parse().
    pub fn parse(&mut self, args: &[String]) {
        let mut parser = Parser {
            s: self,
            arg_state: ArgState::input_files_e,
            argv: String::new(),
            arg_keyword: Keyword::none_k,
            password_using_handles_not_b: false,
            password_using_handles_b: false,
            password_input_pdf_index: 0,
            fail_b: false,
        };
        parser.parse(args);
    }

    /// Port of TK_Session.is_valid().
    pub fn is_valid(&self) -> bool {
        use Keyword::*;
        let op = self.m_operation;
        self.m_valid_b
            && (op == dump_data_k
                || op == dump_data_fields_k
                || op == dump_data_annots_k
                || op == generate_fdf_k
                || self.m_authorized_b)
            && !self.m_input_pdf.is_empty()
            && self.m_input_pdf_readers_opened_b
            && matches!(
                op,
                cat_k
                    | shuffle_k
                    | burst_k
                    | filter_k
                    | dump_data_k
                    | dump_data_utf8_k
                    | dump_data_fields_k
                    | dump_data_fields_utf8_k
                    | dump_data_annots_k
                    | generate_fdf_k
                    | unpack_files_k
            )
            // these op.s require a single input PDF file
            && (!(op == burst_k || op == filter_k) || self.m_input_pdf.len() == 1)
            // these op.s do not require an output filename
            && (matches!(
                op,
                burst_k
                    | dump_data_k
                    | dump_data_fields_k
                    | dump_data_annots_k
                    | generate_fdf_k
                    | unpack_files_k
            ) || !self.m_output_filename.is_empty())
    }

    /// Port of TK_Session.dump_session_data() (the `verbose` report).
    pub fn dump_session_data(&self) {
        if !self.m_verbose_reporting_b {
            return;
        }

        if !self.m_input_pdf_readers_opened_b {
            outln!("Input PDF Open Errors");
            return;
        }

        //
        if self.is_valid() {
            outln!("Command Line Data is valid.");
        } else {
            outln!("Command Line Data is NOT valid.");
        }

        // input files
        outln!();
        outln!("Input PDF Filenames & Passwords in Order\n( <filename>[, <password>] ) ");
        if self.m_input_pdf.is_empty() {
            outln!("   No input PDF filenames have been given.");
        } else {
            for it in &self.m_input_pdf {
                out!("   {}", it.m_filename);
                if !it.m_password.is_empty() {
                    out!(", {}", it.m_password);
                }

                if !it.m_authorized_b {
                    out!(", OWNER OR USER PASSWORD REQUIRED, but not given (or incorrect)");
                }

                outln!();
            }
        }

        // operation
        outln!();
        outln!("The operation to be performed: ");
        match self.m_operation {
            Keyword::cat_k => outln!("   cat - Catenate given page ranges into a new PDF."),
            Keyword::shuffle_k => {
                outln!("   shuffle - Interleave given page ranges into a new PDF.")
            }
            Keyword::burst_k => {
                outln!("   burst - Split a single, input PDF into individual pages.")
            }
            Keyword::filter_k => {
                outln!("   filter - Apply 'filters' to a single, input PDF based on output args.");
                outln!("      (When the operation is omitted, this is the default.)");
            }
            Keyword::dump_data_k => {
                outln!("   dump_data - Report statistics on a single, input PDF.")
            }
            Keyword::dump_data_fields_k => {
                outln!("   dump_data_fields - Report form field data on a single, input PDF.")
            }
            Keyword::dump_data_annots_k => {
                outln!("   dump_data_annots - Report annotation data on a single, input PDF.")
            }
            Keyword::generate_fdf_k => {
                outln!("   generate_fdf - Generate a dummy FDF file from a PDF.")
            }
            Keyword::unpack_files_k => {
                outln!("   unpack_files - Copy PDF file attachments into given directory.")
            }
            Keyword::none_k => {
                outln!("   NONE - No operation has been given.  See usage instructions.")
            }
            _ => outln!("   INTERNAL ERROR - An unexpected operation has been given."),
        }

        // output file; may be PDF or text
        outln!();
        outln!("The output file will be named:");
        if self.m_output_filename.is_empty() {
            outln!("   No output filename has been given.");
        } else {
            outln!("   {}", self.m_output_filename);
        }

        // output encryption
        outln!();
        let output_encrypted_b = self.m_output_encryption_strength != EncryptionStrength::None
            || !self.m_output_user_pw.is_empty()
            || !self.m_output_owner_pw.is_empty();

        outln!("Output PDF encryption settings:");
        if output_encrypted_b {
            outln!("   Output PDF will be encrypted.");

            match self.m_output_encryption_strength {
                EncryptionStrength::None => {
                    outln!("   Encryption strength not given. Defaulting to: AES 128 bits.")
                }
                // (sic: the Java says "AES 40 bits" for RC4 40)
                EncryptionStrength::Rc4_40 => {
                    outln!("   Given output encryption strength: AES 40 bits")
                }
                EncryptionStrength::Rc4_128 => {
                    outln!("   Given output encryption strength: RC4 128 bits")
                }
                EncryptionStrength::Aes128 => {
                    outln!("   Given output encryption strength: AES 128 bits")
                }
            }

            outln!();
            {
                let perms = self.m_output_user_perms;
                if self.m_output_user_pw.is_empty() {
                    outln!("   No user password given.");
                } else {
                    outln!("   Given user password: {}", self.m_output_user_pw);
                }
                if self.m_output_owner_pw.is_empty() {
                    outln!("   No owner password given.");
                } else {
                    outln!("   Given owner password: {}", self.m_output_owner_pw);
                }
                //
                // the printing section: Top Quality or Degraded, but not both;
                // AllowPrinting is a superset of both flag settings
                if (perms & ALLOW_PRINTING) == ALLOW_PRINTING {
                    outln!("   ALLOW Top Quality Printing");
                } else if (perms & ALLOW_PRINTING) == ALLOW_DEGRADED_PRINTING {
                    outln!("   ALLOW Degraded Printing (Top-Quality Printing NOT Allowed)");
                } else {
                    outln!("   Printing NOT Allowed");
                }
                if (perms & ALLOW_MODIFY_CONTENTS) == ALLOW_MODIFY_CONTENTS {
                    outln!("   ALLOW Modifying of Contents");
                } else {
                    outln!("   Modifying of Contents NOT Allowed");
                }
                if (perms & ALLOW_COPY) == ALLOW_COPY {
                    outln!("   ALLOW Copying of Contents");
                } else {
                    outln!("   Copying of Contents NOT Allowed");
                }
                if (perms & ALLOW_MODIFY_ANNOTATIONS) == ALLOW_MODIFY_ANNOTATIONS {
                    outln!("   ALLOW Modifying of Annotations");
                } else {
                    outln!("   Modifying of Annotations NOT Allowed");
                }
                if (perms & ALLOW_FILL_IN) == ALLOW_FILL_IN {
                    outln!("   ALLOW Fill-In");
                } else {
                    outln!("   Fill-In NOT Allowed");
                }
                if (perms & ALLOW_SCREEN_READERS) == ALLOW_SCREEN_READERS {
                    outln!("   ALLOW Screen Readers");
                } else {
                    outln!("   Screen Readers NOT Allowed");
                }
                if (perms & ALLOW_ASSEMBLY) == ALLOW_ASSEMBLY {
                    outln!("   ALLOW Assembly");
                } else {
                    outln!("   Assembly NOT Allowed");
                }
            }
        } else {
            outln!("   Output PDF will not be encrypted.");
        }

        // compression filter
        outln!();
        if self.m_operation != Keyword::filter_k
            || output_encrypted_b
            || !(self.m_output_compress_b || self.m_output_uncompress_b)
        {
            outln!("No compression or uncompression being performed on output.");
        } else if self.m_output_compress_b {
            outln!("Compression will be applied to some PDF streams.");
        } else {
            outln!("Some PDF streams will be uncompressed.");
        }
    }

    /// Port of TK_Session.create_output(): the top-level dispatch.
    pub fn create_output(&mut self) -> ErrorCode {
        if !self.is_valid() {
            return ErrorCode::Error;
        }

        if self.m_verbose_reporting_b {
            outln!();
            outln!("Creating Output ...");
        }

        if self.m_output_owner_pw == "PROMPT" {
            self.m_output_owner_pw = prompt_for_password("owner", "the output PDF");
        }
        if self.m_output_user_pw == "PROMPT" {
            self.m_output_user_pw = prompt_for_password("user", "the output PDF");
        }

        if !self.m_output_owner_pw.is_empty() {
            match crate::passwords::utf8_password_to_pdfdoc(&self.m_output_owner_pw, true) {
                Some(pw) => self.m_output_owner_pw_pdfdoc = pw,
                None => {
                    // error
                    errln!("Error: Owner password used to encrypt output PDF includes");
                    errln!("   invalid characters.");
                    return ErrorCode::Error;
                }
            }
        }

        if !self.m_output_user_pw.is_empty() {
            match crate::passwords::utf8_password_to_pdfdoc(&self.m_output_user_pw, true) {
                Some(pw) => self.m_output_user_pw_pdfdoc = pw,
                None => {
                    // error
                    errln!("Error: User password used to encrypt output PDF includes");
                    errln!("   invalid characters.");
                    return ErrorCode::Error;
                }
            }
        }

        match self.m_operation {
            Keyword::burst_k
            | Keyword::dump_data_fields_k
            | Keyword::dump_data_annots_k
            | Keyword::dump_data_k
            | Keyword::generate_fdf_k
            | Keyword::unpack_files_k => {
                // we should have been given only a single, input file
                if 1 < self.m_input_pdf.len() {
                    // error
                    errln!(
                        "Error: Only one input PDF file may be used for the {} operation",
                        self.m_operation
                    );
                    return ErrorCode::Error;
                }
            }
            _ => {}
        }

        match self.m_operation {
            Keyword::cat_k | Keyword::shuffle_k => return crate::cat::create_output_cat(self),
            Keyword::burst_k => return crate::burst::create_output_burst(self),
            Keyword::filter_k => return crate::filter::create_output_filter(self),

            Keyword::dump_data_fields_k | Keyword::dump_data_annots_k | Keyword::dump_data_k => {
                // report on input document
                let utf8_b = self.m_output_utf8_b;
                let Some(mut ofs) = get_print_stream(&self.m_output_filename, utf8_b) else {
                    // error
                    errln!(
                        "Error: unable to open file for output: {}",
                        self.m_output_filename
                    );
                    return ErrorCode::Error;
                };
                let input_reader_p = &self.m_input_pdf[0].m_readers[0].second;
                // like a Java PrintStream, write errors are not reported
                let _ = match self.m_operation {
                    Keyword::dump_data_k => {
                        crate::report::report_on_pdf(&mut *ofs, input_reader_p, utf8_b)
                    }
                    Keyword::dump_data_fields_k => {
                        crate::forms::report_acro_form_fields(&mut *ofs, input_reader_p, utf8_b)
                    }
                    _ => crate::report::report_annots(&mut *ofs, input_reader_p, utf8_b),
                };
                let _ = ofs.flush();
            }

            Keyword::generate_fdf_k => {
                // create a dummy FDF file that would work with the input PDF form
                match get_output_stream(&self.m_output_filename, self.m_ask_about_warnings_b) {
                    Some(mut ofs_p) => {
                        let input_reader_p = &self.m_input_pdf[0].m_readers[0].second;
                        let _ = crate::forms::generate_fdf(&mut *ofs_p, input_reader_p);
                        let _ = ofs_p.flush();
                    }
                    // error: get_output_stream() reports error
                    None => return ErrorCode::Error,
                }
            }

            Keyword::unpack_files_k => {
                // copy PDF file attachments into current directory;
                // as in Java, problems are reported by unpack_files() itself
                // and do not change the exit code
                let _ = crate::attachments::unpack_files(self);
            }

            _ => {
                // error
                errln!("Unexpected pdftk Error in create_output()");
                return ErrorCode::Bug;
            }
        }

        ErrorCode::NoError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> TkSession {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut session = TkSession::new();
        session.parse(&args);
        session
    }

    const BLANK: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files/blank.pdf");
    const REFS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files/refs.pdf");

    // --- CommandParserTest.java ---

    #[test]
    fn test_empty() {
        assert!(!parse(&[]).is_valid());
    }

    #[test]
    fn test_no_input_no_op() {
        assert!(!parse(&["output", "-"]).is_valid());
    }

    #[test]
    fn test_no_input() {
        assert!(!parse(&["cat", "output", "-"]).is_valid());
    }

    #[test]
    fn test_no_output() {
        assert!(!parse(&[BLANK]).is_valid());
    }

    #[test]
    fn test_two_outputs() {
        assert!(!parse(&[BLANK, "cat", "output", "-", "output", "-"]).is_valid());
    }

    #[test]
    fn test_operation_after_output() {
        assert!(!parse(&[BLANK, "output", "-", "cat"]).is_valid());
    }

    #[test]
    fn test_no_operation_filter() {
        let s = parse(&[BLANK, "output", "-"]);
        assert!(s.is_valid());
        assert_eq!(s.m_operation, Keyword::filter_k);
    }

    #[test]
    fn test_no_operation_cat() {
        let s = parse(&[BLANK, BLANK, "output", "-"]);
        assert!(s.is_valid());
        assert_eq!(s.m_operation, Keyword::cat_k);
        assert_eq!(s.m_page_seq.len(), 2);
        assert!(s.m_cat_full_pdfs_b);
    }

    // --- page ranges (need a real PDF) ---

    fn pages(s: &TkSession) -> Vec<Vec<u32>> {
        s.m_page_seq.iter().map(|r| r.iter().map(|p| p.m_page_num).collect()).collect()
    }

    #[test]
    fn page_ranges_on_pdf() {
        let s = parse(&[REFS, "cat", "output", "-"]);
        assert!(s.is_valid());
        let n = s.m_input_pdf[0].m_num_pages;
        assert!(n >= 2, "refs.pdf should have several pages");
        let all: Vec<u32> = (1..=n).collect();
        assert_eq!(pages(&s), vec![all.clone()]);
        assert!(s.m_cat_full_pdfs_b);

        let s = parse(&[REFS, "cat", "1", "end-1", "r1", "1-endeven", "odd", "output", "-"]);
        assert!(s.is_valid());
        let rev: Vec<u32> = all.iter().rev().cloned().collect();
        let even: Vec<u32> = all.iter().cloned().filter(|p| p % 2 == 0).collect();
        let odd: Vec<u32> = all.iter().cloned().filter(|p| p % 2 == 1).collect();
        assert_eq!(pages(&s), vec![vec![1], rev, vec![n], even, odd]);
        assert!(!s.m_cat_full_pdfs_b);
        // pages 1 and n are each used three times, so three readers are needed
        assert_eq!(s.m_input_pdf[0].m_readers.len(), 3);

        let s = parse(&[&format!("A={}", REFS), "cat", "A1east", "Aenddown", "A~1", "output", "-"]);
        assert!(s.is_valid());
        assert_eq!(s.m_page_seq[0][0].m_page_rot, PageRotate::East);
        assert!(s.m_page_seq[0][0].m_page_abs);
        assert_eq!(s.m_page_seq[1][0].m_page_rot, PageRotate::South);
        assert!(!s.m_page_seq[1][0].m_page_abs);
        assert_eq!(pages(&s)[2], all[1..].to_vec());

        assert!(!parse(&[REFS, "cat", "0-1", "output", "-"]).is_valid());
        assert!(!parse(&[REFS, "cat", "1-9999", "output", "-"]).is_valid());
        assert!(!parse(&[REFS, "cat", "B1", "output", "-"]).is_valid());
        assert!(!parse(&[REFS, "cat", "1foo", "output", "-"]).is_valid());
    }

    // --- pure parsing helpers ---

    #[test]
    fn handles() {
        assert_eq!(split_handle("A=foo.pdf"), (Some("A"), "foo.pdf"));
        assert_eq!(split_handle("AB=x=y"), (Some("AB"), "x=y"));
        assert_eq!(split_handle("a=foo.pdf"), (None, "a=foo.pdf"));
        assert_eq!(split_handle("=foo.pdf"), (None, "=foo.pdf"));
        assert_eq!(split_handle("A1=foo.pdf"), (None, "A1=foo.pdf"));
        assert_eq!(split_handle("foo.pdf"), (None, "foo.pdf"));
        assert_eq!(split_handle("A="), (Some("A"), ""));
    }

    fn range(n: i32, s: &str) -> Option<(i32, i32, String)> {
        let mut unparsed = s.to_string();
        let mut r = PageRange::new(n, s);
        if r.parse(&mut unparsed) {
            Some((r.beg, r.end, unparsed))
        } else {
            None
        }
    }

    #[test]
    fn page_range() {
        assert_eq!(range(10, ""), Some((0, 0, "".into())));
        assert_eq!(range(10, "3"), Some((3, 3, "".into())));
        assert_eq!(range(10, "3-5"), Some((3, 5, "".into())));
        assert_eq!(range(10, "end"), Some((10, 10, "".into())));
        assert_eq!(range(10, "end-1"), Some((10, 1, "".into())));
        assert_eq!(range(10, "r1"), Some((10, 10, "".into())));
        assert_eq!(range(10, "r3-r1"), Some((8, 10, "".into())));
        assert_eq!(range(10, "rend"), Some((1, 1, "".into())));
        assert_eq!(range(10, "1-endevenwest"), Some((1, 10, "evenwest".into())));
        assert_eq!(range(10, "3-"), Some((3, 3, "".into())));
        assert_eq!(range(10, "-4"), Some((0, 4, "".into())));
        assert_eq!(range(10, "2-4~3"), Some((2, 4, "~3".into())));
        assert_eq!(range(10, "even"), Some((0, 0, "even".into())));
        // the regex's optional leading 'r' swallows the r of "right"
        assert_eq!(range(10, "right"), Some((0, 0, "ight".into())));
        assert_eq!(range(10, "11"), None);
        assert_eq!(range(10, "1-11"), None);
        assert_eq!(range(10, "0"), Some((0, 0, "".into())));
    }

    #[test]
    fn output_options_without_pdf() {
        // errors that occur before any PDF is opened
        let s = parse(&["burst", "output"]);
        assert!(!s.is_valid());
        let s = parse(&["A=a.pdf", "A=b.pdf"]);
        assert!(!s.is_valid());
        assert!(s.m_input_pdf.is_empty());
        let s = parse(&["a.pdf", "allow"]);
        assert!(!s.is_valid());
        let s = parse(&["a.pdf", "input_pw", "x", "y"]);
        assert!(!s.is_valid());
    }

    #[test]
    fn relation_capitalization() {
        assert_eq!(capitalize_fully("sOURCE"), "Source");
        assert_eq!(capitalize_fully("unspecified"), "Unspecified");
    }

    #[test]
    fn ascii_writer() {
        let mut buf = Vec::new();
        AsciiWriter(&mut buf).write_all("a\u{e9}b\u{4e2d}c\u{1f600}\n".as_bytes()).unwrap();
        assert_eq!(buf, b"a?b?c?\n");
    }

    #[test]
    fn help_and_version_exit_codes() {
        assert_eq!(main_noexit(&["--version".to_string()]), 0);
        assert_eq!(main_noexit(&["x.pdf".to_string(), "-h".to_string()]), 0);
        assert_eq!(main_noexit(&[]), 0);
    }
}
