//! Black-box checks of the command-line parser against the Java pdftk oracle
//! (/usr/bin/pdftk). Oracle comparisons are skipped when it is not installed.
use std::path::Path;
use std::process::{Command, Stdio};

const ORACLE: &str = "/usr/bin/pdftk";
const FILES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files");

fn run(bin: &str, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin)
        .args(args)
        .current_dir(FILES)
        .stdin(Stdio::null())
        .output()
        .expect("failed to run");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn rs(args: &[&str]) -> (i32, String, String) {
    run(env!("CARGO_BIN_EXE_pdftk-rs"), args)
}

/// Runs every command line through both programs; returns how many were compared.
fn compare_all(cases: &[&[&str]]) -> usize {
    if !Path::new(ORACLE).exists() {
        eprintln!("oracle {} not installed; skipping", ORACLE);
        return 0;
    }
    let mut failures = Vec::new();
    for args in cases {
        let java = run(ORACLE, args);
        let rust = rs(args);
        if java != rust {
            failures.push(format!("{:?}\n  java: {:?}\n  rust: {:?}", args, java, rust));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} command lines differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    cases.len()
}

/// Command lines that are rejected before any input PDF is opened.
const NO_PDF_CASES: &[&[&str]] = &[
    &["output", "-"],
    &["cat", "output", "-"],
    &["cat"],
    &["burst"],
    &["burst", "output"],
    &["shuffle", "1-2", "output", "x.pdf"],
    &["dump_data"],
    &["dump_data", "output", "foo.txt"],
    &["allow", "printing"],
    &["owner_pw", "foo"],
    &["verbose"],
    &["dont_ask"],
    &["A=a.pdf", "A=b.pdf"],
    &["A=a.pdf", "B=b.pdf", "A=c.pdf", "cat", "A", "B", "output", "-"],
    &["a.pdf", "allow"],
    &["a.pdf", "encrypt_128bit"],
    &["a.pdf", "flatten"],
    &["a.pdf", "compress", "output", "x.pdf"],
    &["a.pdf", "input_pw", "foo", "bar"],
    &["a.pdf", "input_pw", "foo", "allow"],
    &["a.pdf", "input_pw", "PROMPT", "PROMPT"],
    &["A=a.pdf", "input_pw", "A=foo", "A=bar"],
    &["A=a.pdf", "input_pw", "A=foo", "B=bar"],
    &["A=a.pdf", "input_pw", "A=foo", "bar"],
    &["A=a.pdf", "B=b.pdf", "input_pw", "foo", "B=bar"],
    &["A=a.pdf", "input_pw", "B=bar"],
    &["a.pdf", "input_pw", "A=foo", "x=y", "z"],
    &["fill_form"],
    &["fill_form", "x.fdf", "output", "-"],
    &["attach_files", "a.txt", "to_page", "1", "output", "-"],
    &["stamp", "s.pdf", "output", "-"],
    &["update_info", "i.txt", "output", "o.pdf"],
    &["background"],
    &["rotate", "1east", "output", "-"],
    &["unpack_files", "output", "/tmp"],
    &["generate_fdf", "output", "-"],
    &["dump_data_fields_utf8"],
    &["a.pdf", "owner_pw"],
    &["a.pdf", "user_pw", "x"],
    &["even.pdf", "odd.pdf", "flatten"],
    &["a.pdf", "verbose"],
    &["a.pdf", "PRINTING"],
    &["a.pdf", "allfeatures"],
    &["a.pdf", "uncompress"],
    &["a.pdf", "to_page", "5", "cat"],
    &["a.pdf", "relation", "foo", "cat"],
    &["a.pdf", "do_ask", "need_appearances"],
    &["a.pdf", "drop_xfa"],
    &["a.pdf", "keep_first_id"],
    &["a.pdf", "replacement_font"],
    &["a.pdf", "north"],
    &["a.pdf", "~"],
    &["a.pdf", "DUMPDATA", "FOO"],
];

#[test]
fn errors_before_any_pdf_is_opened_match_oracle() {
    compare_all(NO_PDF_CASES);
}

#[test]
fn errors_before_any_pdf_is_opened() {
    // independent of the oracle
    let (code, out, err) = rs(&["cat", "output", "-"]);
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert_eq!(
        err,
        "Error: No input files.  Exiting.\nErrors encountered.  No output created.\n\
         Done.  Input errors, so no output created.\n"
    );

    let (code, _, err) = rs(&["A=a.pdf", "A=b.pdf"]);
    assert_eq!(code, 1);
    assert!(err.starts_with(
        "Error: Handle given here: \n      A=b.pdf\n   is already associated with: \n      a.pdf\n   Exiting.\n"
    ));

    let (code, _, err) = rs(&["a.pdf", "flatten"]);
    assert_eq!(code, 1);
    assert!(err.starts_with("Error: Unexpected command-line data: \n      flatten\n"));
}

#[test]
fn synopsis_help_version() {
    let (code, out, err) = rs(&[]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.starts_with("SYNOPSIS\n       pdftk <input PDF files | - | PROMPT>\n"));
    assert!(out.trim_end().ends_with("For Complete Help: pdftk --help"));

    for flag in ["--version", "-version"] {
        let (code, out, err) = rs(&[flag]);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.starts_with("pdftk-rs "));
        assert!(out.contains("pdftk-java"));
        assert!(out.contains("Marc Vinyals"));
        assert!(out.contains("Copyright (c) 2003-2013 Steward and Lee, LLC."));
        assert!(!out.contains("SYNOPSIS"));
    }

    for args in [&["--help"][..], &["-help"], &["-h"], &["a.pdf", "cat", "-h"], &["--version", "--help"]] {
        let (code, out, err) = rs(args);
        assert_eq!((code, err.as_str()), (0, ""));
        assert!(out.starts_with("pdftk-rs "));
        assert!(out.contains("\nSYNOPSIS\n"));
        assert!(out.contains("\nDESCRIPTION\n"));
        assert!(out.contains("\nOPTIONS\n"));
    }

    if Path::new(ORACLE).exists() {
        assert_eq!(run(ORACLE, &[]), rs(&[]));
        // --help differs only in the version header
        let java = run(ORACLE, &["--help"]).1;
        let rust = rs(&["--help"]).1;
        let tail = |s: &str| s[s.find("\nSYNOPSIS\n").unwrap()..].to_string();
        assert_eq!(tail(&java), tail(&rust));
    }
}

/// Command lines exercising the parser on real input PDFs (run in test/files).
const PDF_CASES: &[&[&str]] = &[
    &["refs.pdf", "cat", "right", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-r0", "output", "/dev/null"],
    &["refs.pdf", "cat", "1~99", "output", "/dev/null"],
    &["refs.pdf", "cat", "0", "output", "/dev/null"],
    &["refs.pdf", "cat", "0-1", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-0", "output", "/dev/null"],
    &["refs.pdf", "cat", "99", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-99", "output", "/dev/null"],
    &["refs.pdf", "cat", "r99", "output", "/dev/null"],
    &["refs.pdf", "cat", "B1", "output", "/dev/null"],
    &["refs.pdf", "cat", "1foo", "output", "/dev/null"],
    &["refs.pdf", "cat", "1westeven", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-endoddeast", "1", "end-1", "r1", "output", "/dev/null"],
    &["refs.pdf", "cat", "1", "flatten", "output", "/dev/null"],
    &["refs.pdf", "cat", "1", "output", "/dev/null", "output", "/dev/null"],
    &["refs.pdf", "cat", "1", "output", "refs.pdf"],
    &["refs.pdf", "cat"],
    &["refs.pdf"],
    &["refs.pdf", "output", "/dev/null", "cat"],
    &["refs.pdf", "output", "/dev/null", "foo"],
    &["refs.pdf", "output", "/dev/null", "allow", "foo"],
    &["refs.pdf", "output", "/dev/null", "owner_pw", "a", "user_pw", "a"],
    &["refs.pdf", "output", "/dev/null", "owner_pw", "a", "owner_pw", "b"],
    &["refs.pdf", "output", "/dev/null", "user_pw", "a", "user_pw", "b"],
    &["refs.pdf", "output", "/dev/null", "user_pw", "a", "owner_pw", "a"],
    &["refs.pdf", "blank.pdf", "burst"],
    &["refs.pdf", "blank.pdf", "dump_data"],
    &["refs.pdf", "blank.pdf", "output", "/dev/null", "flatten"],
    &["refs.pdf", "blank.pdf", "fill_form", "x.fdf", "output", "/dev/null"],
    &["refs.pdf", "fill_form", "a.fdf", "b.fdf", "output", "/dev/null"],
    &["refs.pdf", "fill_form", "output", "/dev/null"],
    &["refs.pdf", "fill_form", "a.fdf", "foo"],
    &["refs.pdf", "stamp", "output"],
    &["refs.pdf", "stamp", "a.pdf", "b.pdf"],
    &["refs.pdf", "background", "output"],
    &["refs.pdf", "update_info", "output"],
    &["refs.pdf", "update_info", "a", "b"],
    &["refs.pdf", "attach_files", "a.txt", "to_page", "x", "output", "/dev/null"],
    &["refs.pdf", "attach_files", "a.txt", "cat"],
    &["refs.pdf", "attach_files", "a.txt", "to_page", "1", "foo"],
    &["refs.pdf", "dump_data", "output", "/nonexistent/x"],
    &["refs.pdf", "dump_data", "foo"],
    &["refs.pdf", "generate_fdf", "output", "/nonexistent/x"],
    &["nonexist.pdf", "output", "/dev/null"],
    &["nonexist.pdf", "cat", "output", "/dev/null"],
    &["refs.pdf", "nonexist.pdf", "cat", "output", "/dev/null"],
    &["A=refs.pdf", "B=blank.pdf", "cat", "C", "output", "/dev/null"],
    &["A=refs.pdf", "cat", "A1-2", "Aend~1", "output", "/dev/null", "verbose"],
    &["refs.pdf", "cat", "output", "/dev/null", "verbose", "owner_pw", "foo", "allow", "printing", "copycontents", "encrypt_128bit"],
    &["refs.pdf", "output", "/dev/null", "verbose", "compress"],
    &["refs.pdf", "cat", "1", "output", "/dev/null", "background", "x.pdf"],
    &["refs.pdf", "output", "/dev/null", "verbose", "encrypt_40bit", "allow", "degradedprinting", "fillin"],
    &["refs.pdf", "verbose"],
    &["refs.pdf", "burst", "verbose", "foo"],
    &["refs.pdf", "input_pw", "foo", "output", "/dev/null", "verbose"],
    &["refs.pdf", "cat", "1-2~", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-2~5-99", "output", "/dev/null"],
    &["refs.pdf", "cat", "~1", "output", "/dev/null", "verbose"],
    &["refs.pdf", "shuffle", "even", "odd", "output", "/dev/null", "verbose"],
    &["refs.pdf", "rotate", "1east", "output", "/dev/null", "verbose"],
    &["refs.pdf", "rotate", "1east"],
    &["refs.pdf", "cat", "r", "output", "/dev/null"],
    &["refs.pdf", "cat", "-", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-", "output", "/dev/null"],
    &["refs.pdf", "cat", "-1", "output", "/dev/null"],
    &["refs.pdf", "cat", "1-end-2", "output", "/dev/null"],
    &["refs.pdf", "cat", "1 2", "output", "/dev/null"],
    &["refs.pdf", "cat", "1EAST", "output", "/dev/null", "verbose"],
    &["refs.pdf", "cat", "END", "output", "/dev/null"],
    &["refs.pdf", "output", "/dev/null", "replacement_font"],
    &["refs.pdf", "attach_files", "nonexist.txt", "relation", "foo", "output", "/dev/null"],
];

#[test]
fn parsing_with_input_pdfs_matches_oracle() {
    compare_all(PDF_CASES);
}

#[test]
fn unparsable_page_number_is_reported_as_bug() {
    // Java: NumberFormatException -> "Unhandled Java Exception in main()", exit code 2
    let (code, _, _) = rs(&["refs.pdf", "cat", "99999999999", "output", "/dev/null"]);
    assert_eq!(code, 2);
}

#[test]
fn prompt_at_end_of_input_is_reported_as_bug() {
    // Java: Scanner.nextLine() throws NoSuchElementException, exit code 2
    let (code, _, _) = rs(&["refs.pdf", "output", "/dev/null", "user_pw", "PROMPT"]);
    assert_eq!(code, 2);
}
