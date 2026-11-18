//! End-to-end checks of the filter path (filter.rs, stamp.rs, attachments.rs) against the
//! Java pdftk oracle (/usr/bin/pdftk): ports of StampTest.java and AttachFilesTest.java plus
//! the plain filter operations. Skipped when the oracle or the poppler/qpdf tools are missing.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const ORACLE: &str = "/usr/bin/pdftk";
const OURS: &str = env!("CARGO_BIN_EXE_pdftk-rs");
const FILES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files");

fn tools_available() -> bool {
    let ok = Path::new(ORACLE).exists()
        && ["qpdf", "pdfinfo", "pdftotext", "pdftoppm", "pdfdetach"].iter().all(|t| {
            Command::new(t)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        });
    if !ok {
        eprintln!("oracle or helper tools not installed; skipping");
    }
    ok
}

/// A fresh scratch directory with `j` (oracle) and `r` (ours) subdirectories.
fn scratch(name: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pdftk-rs-filter-{}-{}-{}",
        std::process::id(),
        name,
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("j")).unwrap();
    std::fs::create_dir_all(dir.join("r")).unwrap();
    dir
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn run_in(bin: &str, args: &[String], stdin: &[u8]) -> Run {
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(FILES)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run");
    let mut pipe = child.stdin.take().unwrap();
    let input = stdin.to_vec();
    let feeder = std::thread::spawn(move || {
        let _ = pipe.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    let _ = feeder.join();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn tool(cmd: &str, args: &[&str]) -> (i32, String) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(FILES)
        .stdin(Stdio::null())
        .output()
        .expect("failed to run tool");
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// Run a command line through both programs. "@" in an argument stands for the program's own
/// scratch subdirectory. Returns (oracle, ours) with the scratch paths normalised in stderr.
fn both(dir: &Path, args: &[&str], stdin: &[u8]) -> (Run, Run) {
    let run = |bin: &str, sub: &str| {
        let own = dir.join(sub);
        let own = own.to_str().unwrap();
        let args: Vec<String> = args.iter().map(|a| a.replace('@', own)).collect();
        let mut r = run_in(bin, &args, stdin);
        r.stderr = r.stderr.replace(own, "@");
        r.stdout = String::from_utf8_lossy(&r.stdout).replace(own, "@").into_bytes();
        r
    };
    (run(ORACLE, "j"), run(OURS, "r"))
}

fn page_count(pdf: &Path) -> String {
    tool("pdfinfo", &[pdf.to_str().unwrap()])
        .1
        .lines()
        .find(|l| l.starts_with("Pages:"))
        .unwrap_or("")
        .to_string()
}

/// Rasterise both files at 40 dpi; the largest fraction of differing samples over the pages.
fn raster_diff(a: &Path, b: &Path, dir: &Path) -> Result<f64, String> {
    let render = |pdf: &Path, prefix: &str| -> Vec<Vec<u8>> {
        let root = dir.join(prefix);
        let _ = Command::new("pdftoppm")
            .args(["-r", "40", pdf.to_str().unwrap(), root.to_str().unwrap()])
            .output();
        let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{}-", prefix)) && n.ends_with(".ppm"))
            })
            .collect();
        pages.sort();
        pages
            .iter()
            .map(|p| {
                let data = std::fs::read(p).unwrap();
                let _ = std::fs::remove_file(p);
                data
            })
            .collect()
    };
    let pa = render(a, "rasta");
    let pb = render(b, "rastb");
    if pa.is_empty() || pa.len() != pb.len() {
        return Err(format!("rendered {} vs {} pages", pa.len(), pb.len()));
    }
    let mut worst = 0.0f64;
    for (n, (x, y)) in pa.iter().zip(pb.iter()).enumerate() {
        if x.len() != y.len() {
            return Err(format!("page {} renders at a different size", n + 1));
        }
        let differing = x
            .iter()
            .zip(y.iter())
            .filter(|(p, q)| (**p as i32 - **q as i32).abs() > 32)
            .count();
        worst = worst.max(differing as f64 / x.len() as f64);
    }
    Ok(worst)
}

#[derive(Clone, Copy)]
struct Check {
    /// compare oracle dump_data of both outputs
    dump: bool,
    /// largest tolerated fraction of differing raster samples
    raster: f64,
}

const FULL: Check = Check { dump: true, raster: 0.002 };

/// Run one filter command line ("@/out.pdf" is the output) through both programs and compare
/// everything observable. Returns a description of every mismatch.
fn compare_case(dir: &Path, args: &[&str], stdin: &[u8], check: Check) -> Vec<String> {
    let mut problems = Vec::new();
    for sub in ["j", "r"] {
        let _ = std::fs::remove_file(dir.join(sub).join("out.pdf"));
    }
    let (java, rust) = both(dir, args, stdin);
    if java.code != rust.code {
        problems.push(format!("exit code: java {} rust {}", java.code, rust.code));
    }
    if java.stderr != rust.stderr {
        problems.push(format!("stderr:\n  java: {:?}\n  rust: {:?}", java.stderr, rust.stderr));
    }
    if java.stdout != rust.stdout {
        problems.push("stdout differs".to_string());
    }
    let jout = dir.join("j/out.pdf");
    let rout = dir.join("r/out.pdf");
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if (size(&jout) > 0) != (size(&rout) > 0) {
        problems.push(format!("output presence: java {} bytes, rust {} bytes", size(&jout), size(&rout)));
    }
    if size(&jout) > 0 && size(&rout) > 0 {
        let (j, r) = (jout.to_str().unwrap(), rout.to_str().unwrap());
        let (qcode, qout) = tool("qpdf", &["--check", r]);
        if qcode != 0 {
            problems.push(format!("qpdf --check: {}", qout));
        }
        if page_count(&jout) != page_count(&rout) {
            problems.push(format!("pages: java {:?} rust {:?}", page_count(&jout), page_count(&rout)));
        }
        let text = |p: &str| {
            Command::new("pdftotext").args([p, "-"]).stderr(Stdio::null()).output().map(|o| o.stdout).ok()
        };
        if text(j) != text(r) {
            problems.push("pdftotext differs".to_string());
        }
        if check.dump {
            let dj = tool(ORACLE, &[j, "dump_data"]);
            let dr = tool(ORACLE, &[r, "dump_data"]);
            if dj != dr {
                problems.push(format!("dump_data:\n  java: {}\n  rust: {}", dj.1, dr.1));
            }
        }
        match raster_diff(&jout, &rout, dir) {
            Ok(worst) if worst <= check.raster => {}
            Ok(worst) => problems.push(format!("raster: {:.4} of the samples differ", worst)),
            Err(e) => problems.push(format!("raster: {}", e)),
        }
    }
    problems
}

fn run_cases(name: &str, cases: &[(&[&str], Check)]) {
    if !tools_available() {
        return;
    }
    let dir = scratch(name);
    let mut failures = Vec::new();
    for (args, check) in cases {
        let problems = compare_case(&dir, args, b"", *check);
        if !problems.is_empty() {
            failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    eprintln!("{}: {} cases match the oracle", name, cases.len());
}

////////////////////////////////////////////////////////////////////////////
// stamp / background

const OPERATIONS: [&str; 4] = ["background", "multibackground", "stamp", "multistamp"];

/// (input, mark): different sizes, rotated pages on either side, marks with fewer and with
/// more pages than the input.
const MARK_PAIRS: [(&str, &str); 9] = [
    ("blank.pdf", "duck.pdf"),
    ("duck.pdf", "blank.pdf"),
    ("blank-rotated.pdf", "duck.pdf"),
    ("duck.pdf", "blank-rotated.pdf"),
    ("blank-rotated.pdf", "blank-rotated.pdf"),
    ("refs.pdf", "blank-rotated.pdf"),
    ("refs.pdf", "issue18.pdf"),
    ("issue123.pdf", "duck.pdf"),
    ("issue88.pdf", "form.pdf"),
];

#[test]
fn stamp_matrix() {
    if !tools_available() {
        return;
    }
    let dir = scratch("stamp");
    let mut failures = Vec::new();
    let mut n = 0;
    for op in OPERATIONS {
        for (input, mark) in MARK_PAIRS {
            let args = [input, op, mark, "output", "@/out.pdf"];
            let problems = compare_case(&dir, &args, b"", FULL);
            n += 1;
            if !problems.is_empty() {
                failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), n, failures.join("\n"));
    eprintln!("stamp_matrix: {} cases match the oracle", n);
}

/// Marks whose pages carry annotations. pdftk-java 3.3.3 (the source being ported) copies
/// them onto the stamped pages; the 3.3.2 oracle does not, so rendered annotations may differ
/// slightly and only the page content is compared (with a looser tolerance).
#[test]
fn stamp_marks_with_annotations() {
    let loose = Check { dump: true, raster: 0.02 };
    run_cases(
        "stamp-annots",
        &[
            (&["blank.pdf", "stamp", "annotation.pdf", "output", "@/out.pdf"], loose),
            (&["blank-rotated.pdf", "background", "refs.pdf", "output", "@/out.pdf"], loose),
            (&["issue88.pdf", "multistamp", "refsalt.pdf", "output", "@/out.pdf"], loose),
            (&["refs.pdf", "multibackground", "refs.pdf", "output", "@/out.pdf"], loose),
            (&["cat-refs-refsalt.pdf", "multibackground", "issue123.pdf", "output", "@/out.pdf", "compress"], loose),
        ],
    );
}

/// filter.java's mark_annots loop: the mark's annotations land on the stamped page, their
/// rectangles run through the placement matrix (the identity here: same page size).
#[test]
fn stamp_copies_mark_annotations() {
    if !tools_available() {
        return;
    }
    let dir = scratch("annots");
    let out = dir.join("r/out.pdf");
    let args: Vec<String> = ["blank.pdf", "stamp", "annotation.pdf", "output", out.to_str().unwrap()]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let r = run_in(OURS, &args, b"");
    assert_eq!((r.code, r.stderr.as_str()), (0, ""));
    let expected = tool(ORACLE, &["annotation.pdf", "dump_data_annots"]).1;
    let actual = tool(ORACLE, &[out.to_str().unwrap(), "dump_data_annots"]).1;
    let rects = |s: &str| -> Vec<String> {
        s.lines()
            .filter(|l| l.starts_with("AnnotRect:") || l.starts_with("AnnotSubtype:"))
            .map(|l| l.to_string())
            .collect()
    };
    assert!(!rects(&expected).is_empty());
    assert_eq!(rects(&expected), rects(&actual));
    assert_eq!(tool("qpdf", &["--check", out.to_str().unwrap()]).0, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// StampTest.java: the input or the mark on stdin, output on stdout.
#[test]
fn stamp_stdin() {
    if !tools_available() {
        return;
    }
    let dir = scratch("stdin");
    let blank = std::fs::read(Path::new(FILES).join("blank.pdf")).unwrap();
    let duck = std::fs::read(Path::new(FILES).join("duck.pdf")).unwrap();
    let mut n = 0;
    for op in OPERATIONS {
        let cases: [(&[&str], &[u8]); 2] = [
            (&["-", op, "duck.pdf", "output", "-"], &blank),
            (&["blank.pdf", op, "-", "output", "-"], &duck),
        ];
        for (args, stdin) in cases {
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let expected: Vec<String> = ["blank.pdf", op, "duck.pdf", "output", "-"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let java = run_in(ORACLE, &args, stdin);
            let rust = run_in(OURS, &args, stdin);
            let plain = run_in(OURS, &expected, b"");
            assert_eq!((java.code, &java.stderr), (rust.code, &rust.stderr), "{:?}", args);
            assert_eq!(rust.code, 0);
            let write = |name: &str, data: &[u8]| {
                let p = dir.join(name);
                std::fs::write(&p, data).unwrap();
                p
            };
            let (j, r, p) = (
                write("j.pdf", &java.stdout),
                write("r.pdf", &rust.stdout),
                write("p.pdf", &plain.stdout),
            );
            assert_eq!(tool("qpdf", &["--check", r.to_str().unwrap()]).0, 0, "{:?}", args);
            assert_eq!(raster_diff(&j, &r, &dir), Ok(0.0), "{:?} vs oracle", args);
            assert_eq!(raster_diff(&p, &r, &dir), Ok(0.0), "{:?} vs file input", args);
            n += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("stamp_stdin: {} cases match the oracle", n);
}

////////////////////////////////////////////////////////////////////////////
// plain filter paths

#[test]
fn filter_paths() {
    if !tools_available() {
        return;
    }
    let dir = scratch("paths");
    let info = dir.join("info.txt");
    std::fs::write(
        &info,
        "InfoBegin\nInfoKey: Title\nInfoValue: Hello W\u{f6}rld\nInfoBegin\nInfoKey: Author\nInfoValue: Me\n",
    )
    .unwrap();
    let info = info.to_str().unwrap();
    // encrypted output cannot be dumped without its password, and gets a fresh /ID
    let nodump = Check { dump: false, raster: 0.002 };
    let cases: Vec<(Vec<&str>, Check)> = vec![
        (vec!["blank.pdf", "output", "@/out.pdf"], FULL),
        (vec!["refs.pdf", "output", "@/out.pdf", "uncompress"], FULL),
        (vec!["refs.pdf", "output", "@/out.pdf", "compress"], FULL),
        (vec!["issue88.pdf", "output", "@/out.pdf", "uncompress"], FULL),
        (vec!["refs.pdf", "rotate", "1east", "2-endsouth", "output", "@/out.pdf"], FULL),
        (vec!["blank-rotated.pdf", "rotate", "1left", "output", "@/out.pdf"], FULL),
        (vec!["refs.pdf", "rotate", "1-3west", "endnorth", "output", "@/out.pdf", "uncompress"], FULL),
        (vec!["refs.pdf", "update_info", info, "output", "@/out.pdf"], FULL),
        (vec!["refs.pdf", "update_info_utf8", info, "output", "@/out.pdf"], FULL),
        (vec!["refs.pdf", "update_info", "/nonexistent/info.txt", "output", "@/out.pdf"], FULL),
        (vec!["form.pdf", "output", "@/out.pdf", "drop_xmp"], FULL),
        (vec!["form.pdf", "output", "@/out.pdf", "drop_xfa"], FULL),
        (vec!["form.pdf", "output", "@/out.pdf", "need_appearances"], FULL),
        (vec!["form.pdf", "output", "@/out.pdf", "flatten"], FULL),
        (vec!["form.pdf", "fill_form", "form-filled.fdf", "output", "@/out.pdf"], FULL),
        (vec!["form.pdf", "fill_form", "form-filled.fdf", "output", "@/out.pdf", "flatten"], FULL),
        (vec!["form.pdf", "fill_form", "form-filled.fdf", "output", "@/out.pdf", "need_appearances"], FULL),
        (vec!["form.pdf", "stamp", "duck.pdf", "output", "@/out.pdf", "flatten"], FULL),
        (vec!["form.pdf", "background", "duck.pdf", "output", "@/out.pdf", "flatten"], FULL),
        (vec!["form.pdf", "rotate", "1east", "output", "@/out.pdf", "flatten"], FULL),
        (vec!["form.pdf", "attach_files", "blank.tex", "to_page", "1", "output", "@/out.pdf", "flatten", "drop_xfa", "drop_xmp"], FULL),
        (vec!["form.pdf", "stamp", "duck.pdf", "output", "@/out.pdf", "owner_pw", "x", "allow", "printing"], nodump),
        (vec!["blank.pdf", "output", "@/out.pdf", "encrypt_40bit", "owner_pw", "foo"], nodump),
        // errors
        (vec!["blank.pdf", "refs.pdf", "stamp", "duck.pdf", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "refs.pdf", "attach_files", "blank.tex", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "fill_form", "/nonexistent.fdf", "output", "@/out.pdf"], FULL),
        (vec!["form.pdf", "fill_form", "blank.tex", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "stamp", "/nonexistent.pdf", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "background", "blank.tex", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "multistamp", "/nonexistent.pdf", "output", "@/out.pdf"], FULL),
        (vec!["blank.pdf", "output", "/nonexistent/dir/out.pdf"], FULL),
    ];
    let mut failures = Vec::new();
    for (args, check) in &cases {
        let problems = compare_case(&dir, args, b"", *check);
        if !problems.is_empty() {
            failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    eprintln!("filter_paths: {} cases match the oracle", cases.len());
}

/// Filenames given as PROMPT or "-" (read from stdin).
#[test]
fn filter_prompts_and_stdin() {
    if !tools_available() {
        return;
    }
    let dir = scratch("prompts");
    let info = dir.join("info.txt");
    std::fs::write(&info, "InfoBegin\nInfoKey: Title\nInfoValue: Prompted\n").unwrap();
    let info_line = format!("{}\n", info.to_str().unwrap());
    let fdf = std::fs::read(Path::new(FILES).join("form-filled.fdf")).unwrap();
    let cases: Vec<(Vec<&str>, Vec<u8>)> = vec![
        (vec!["blank.pdf", "update_info", "PROMPT", "output", "@/out.pdf"], info_line.clone().into_bytes()),
        (vec!["blank.pdf", "update_info", "-", "output", "@/out.pdf"], std::fs::read(&info).unwrap()),
        (vec!["blank.pdf", "update_info", "-", "output", "@/out.pdf"], b"garbage\n".to_vec()),
        (vec!["form.pdf", "fill_form", "PROMPT", "output", "@/out.pdf"], b"form-filled.fdf\n".to_vec()),
        (vec!["form.pdf", "fill_form", "-", "output", "@/out.pdf"], fdf),
        (vec!["form.pdf", "fill_form", "-", "output", "@/out.pdf"], b"junk".to_vec()),
        (vec!["blank.pdf", "stamp", "PROMPT", "output", "@/out.pdf"], b"duck.pdf\n".to_vec()),
        (vec!["blank.pdf", "background", "PROMPT", "output", "@/out.pdf"], b"duck.pdf\n".to_vec()),
        (vec!["blank.pdf", "attach_files", "PROMPT", "output", "@/out.pdf"], b"blank.tex\n".to_vec()),
        (vec!["refs.pdf", "attach_files", "blank.tex", "duck.tex", "to_page", "PROMPT", "output", "@/out.pdf"], b"end\n".to_vec()),
        (vec!["refs.pdf", "attach_files", "blank.tex", "to_page", "PROMPT", "output", "@/out.pdf"], b"\n".to_vec()),
        (vec!["refs.pdf", "attach_files", "blank.tex", "to_page", "PROMPT", "output", "@/out.pdf"], b"3xyz\n".to_vec()),
        (vec!["refs.pdf", "attach_files", "blank.tex", "to_page", "PROMPT", "output", "@/out.pdf"], b"xyz\n".to_vec()),
        (vec!["refs.pdf", "attach_files", "blank.tex", "to_page", "PROMPT", "output", "@/out.pdf"], b"99999999999\n".to_vec()),
    ];
    let mut failures = Vec::new();
    for (args, stdin) in &cases {
        let mut problems = compare_case(&dir, args, stdin, FULL);
        let dump = |sub: &str, op: &str| {
            tool(ORACLE, &[dir.join(sub).join("out.pdf").to_str().unwrap(), op]).1.replace(
                dir.join(sub).to_str().unwrap(),
                "@",
            )
        };
        for op in ["dump_data_annots", "dump_data_fields"] {
            if dump("j", op) != dump("r", op) {
                problems.push(format!("{} differs", op));
            }
        }
        if !problems.is_empty() {
            failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from the oracle:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    eprintln!("filter_prompts_and_stdin: {} cases match the oracle", cases.len());
}

/// Operations that end with a warning: the messages match, and so should the exit code.
#[test]
fn filter_warnings_stderr() {
    if !tools_available() {
        return;
    }
    let dir = scratch("warn");
    let cases: [&[&str]; 2] = [
        &["blank.pdf", "fill_form", "form.fdf", "output", "@/out.pdf"],
        &["form.pdf", "fill_form", "form-filled.fdf", "output", "@/out.pdf", "replacement_font", "/nonexistent.ttf"],
    ];
    for args in cases {
        let (java, rust) = both(&dir, args, b"");
        assert_eq!(java.stderr, rust.stderr, "{:?}", args);
        assert!(java.stderr.starts_with("Warning:"), "{:?}", args);
        assert_eq!(java.code, 3);
        assert!(rust.code == 3 || rust.code == 4, "{:?}: {}", args, rust.code);
        assert_eq!(
            tool(ORACLE, &[dir.join("j/out.pdf").to_str().unwrap(), "dump_data_fields"]).1,
            tool(ORACLE, &[dir.join("r/out.pdf").to_str().unwrap(), "dump_data_fields"]).1
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn filter_warnings_exit_code() {
    if !tools_available() {
        return;
    }
    let dir = scratch("warncode");
    let (java, rust) = both(&dir, &["blank.pdf", "fill_form", "form.fdf", "output", "@/out.pdf"], b"");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(java.code, rust.code);
}

////////////////////////////////////////////////////////////////////////////
// attachments

fn list_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect();
    files.sort();
    files
}

fn args_of(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

/// Attach with both programs, then unpack every output with both programs: all four
/// directories must hold exactly the attached files. Returns the number of files.
fn attach_roundtrip(dir: &Path, args: &[&str], expected: &[(&str, Vec<u8>)]) -> Vec<String> {
    let mut problems = compare_case(dir, args, b"", FULL);
    let mut expected: Vec<(String, Vec<u8>)> =
        expected.iter().map(|(n, d)| (n.to_string(), d.clone())).collect();
    expected.sort();
    let (jout, rout) = (dir.join("j/out.pdf"), dir.join("r/out.pdf"));
    for (what, pdf, bin) in [
        ("oracle unpacks ours", &rout, ORACLE),
        ("ours unpacks oracle's", &jout, OURS),
        ("ours unpacks ours", &rout, OURS),
    ] {
        let target = dir.join("unpacked");
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).unwrap();
        let r = run_in(
            bin,
            &args_of(&[pdf.to_str().unwrap(), "unpack_files", "output", target.to_str().unwrap()]),
            b"",
        );
        if r.code != 0 || !r.stderr.is_empty() {
            problems.push(format!("{}: exit {} stderr {:?}", what, r.code, r.stderr));
        }
        let got = list_dir(&target);
        if got != expected {
            problems.push(format!(
                "{}: got {:?}, expected {:?}",
                what,
                got.iter().map(|(n, d)| (n.clone(), d.len())).collect::<Vec<_>>(),
                expected.iter().map(|(n, d)| (n.clone(), d.len())).collect::<Vec<_>>()
            ));
        }
    }
    let list = |p: &Path| tool("pdfdetach", &["-list", p.to_str().unwrap()]).1;
    if list(&jout) != list(&rout) {
        problems.push(format!("pdfdetach -list:\n  java: {}\n  rust: {}", list(&jout), list(&rout)));
    }
    let annots = |p: &Path| tool(ORACLE, &[p.to_str().unwrap(), "dump_data_annots"]).1;
    if annots(&jout) != annots(&rout) {
        problems.push("dump_data_annots differs".to_string());
    }
    problems
}

#[test]
fn attach_and_unpack() {
    if !tools_available() {
        return;
    }
    let dir = scratch("attach");
    let read = |name: &str| std::fs::read(Path::new(FILES).join(name)).unwrap();
    let blank_tex = read("blank.tex");
    let duck_tex = read("duck.tex");
    // binary and non-ASCII material
    let binary: Vec<u8> = (0..20000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let bin_path = dir.join("bin.dat");
    std::fs::write(&bin_path, &binary).unwrap();
    let latin_path = dir.join("\u{fc}b\u{e9}r \u{20ac}.txt");
    std::fs::write(&latin_path, "h\u{e9}llo").unwrap();
    let cjk_path = dir.join("\u{65e5}\u{672c}.tex");
    std::fs::write(&cjk_path, &blank_tex).unwrap();
    let (bin_s, latin_s, cjk_s) = (
        bin_path.to_str().unwrap(),
        latin_path.to_str().unwrap(),
        cjk_path.to_str().unwrap(),
    );
    let all = vec![
        ("blank.tex", blank_tex.clone()),
        ("duck.tex", duck_tex.clone()),
        ("bin.dat", binary.clone()),
        ("\u{fc}b\u{e9}r \u{20ac}.txt", "h\u{e9}llo".as_bytes().to_vec()),
        ("\u{65e5}\u{672c}.tex", blank_tex.clone()),
    ];
    let one = vec![("blank.tex", blank_tex.clone())];
    let two = vec![("blank.tex", blank_tex.clone()), ("duck.tex", duck_tex.clone())];

    let cases: Vec<(Vec<&str>, &Vec<(&str, Vec<u8>)>)> = vec![
        // AttachFilesTest.attach_one_file
        (vec!["blank.pdf", "attach_files", "blank.tex", "output", "@/out.pdf"], &one),
        // AttachFilesTest.attach_to_page
        (vec!["blank.pdf", "attach_files", "blank.tex", "to_page", "1", "output", "@/out.pdf"], &one),
        // AttachFilesTest.attach_relation
        (vec!["blank.pdf", "attach_files", "blank.tex", "relation", "Source", "output", "@/out.pdf"], &one),
        (vec!["blank.pdf", "attach_files", "blank.tex", "relation", "Bogus", "output", "@/out.pdf"], &one),
        (vec!["blank.pdf", "attach_files", "blank.tex", "duck.tex", bin_s, latin_s, cjk_s, "output", "@/out.pdf"], &all),
        (vec!["refs.pdf", "attach_files", "blank.tex", "duck.tex", bin_s, latin_s, cjk_s, "to_page", "end", "output", "@/out.pdf"], &all),
        (vec!["refs.pdf", "attach_files", "blank.tex", "duck.tex", "to_page", "3", "output", "@/out.pdf"], &two),
        (vec!["annotation.pdf", "attach_files", "blank.tex", "duck.tex", "to_page", "1", "output", "@/out.pdf"], &two),
        (vec!["blank-rotated.pdf", "attach_files", "blank.tex", "duck.tex", "to_page", "1", "output", "@/out.pdf", "uncompress"], &two),
        // problems that are reported and skipped
        (vec!["blank.pdf", "attach_files", "/nonexistent", "blank.tex", "output", "@/out.pdf"], &one),
        (vec!["blank.pdf", "attach_files", "/tmp", "blank.tex", "output", "@/out.pdf"], &one),
    ];
    let mut failures = Vec::new();
    for (args, expected) in &cases {
        let problems = attach_roundtrip(&dir, args, expected);
        if !problems.is_empty() {
            failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
        }
    }
    let n = cases.len();

    // nothing gets attached
    let none: Vec<(&str, Vec<u8>)> = Vec::new();
    for args in [
        vec!["blank.pdf", "attach_files", "/nonexistent", "output", "@/out.pdf"],
        vec!["blank.pdf", "attach_files", "blank.tex", "to_page", "7", "output", "@/out.pdf"],
    ] {
        let problems = attach_roundtrip(&dir, &args, &none);
        if !problems.is_empty() {
            failures.push(format!("{:?}\n  {}", args, problems.join("\n  ")));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{} cases differ:\n{}", failures.len(), failures.join("\n"));
    eprintln!("attach_and_unpack: {} cases match the oracle", n + 2);
}

/// The oracle (3.3.2) writes an unreadable file for issue18.pdf in filter mode (even for a
/// plain "issue18.pdf output x.pdf"), so that input is checked on its own.
#[test]
fn filter_issue18_input() {
    if !tools_available() {
        return;
    }
    let dir = scratch("issue18");
    let out = dir.join("r/out.pdf");
    for args in [
        vec!["issue18.pdf", "output", out.to_str().unwrap()],
        vec!["issue18.pdf", "multistamp", "refs.pdf", "output", out.to_str().unwrap()],
        vec!["issue18.pdf", "background", "duck.pdf", "output", out.to_str().unwrap(), "uncompress"],
    ] {
        let r = run_in(OURS, &args_of(&args), b"");
        assert_eq!((r.code, r.stderr.as_str()), (0, ""), "{:?}", args);
        assert_eq!(tool("qpdf", &["--check", out.to_str().unwrap()]).0, 0, "{:?}", args);
        assert_eq!(page_count(&out), page_count(&Path::new(FILES).join("issue18.pdf")));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Attachments in an encrypted output survive, and can be unpacked again.
#[test]
fn attach_encrypted_output() {
    if !tools_available() {
        return;
    }
    let dir = scratch("attach-enc");
    let blank_tex = std::fs::read(Path::new(FILES).join("blank.tex")).unwrap();
    let (java, rust) = both(
        &dir,
        &["blank.pdf", "attach_files", "blank.tex", "to_page", "1", "output", "@/out.pdf", "owner_pw", "foo", "user_pw", "bar"],
        b"",
    );
    assert_eq!((java.code, &java.stderr), (rust.code, &rust.stderr));
    for (maker, unpacker) in [("r", ORACLE), ("j", OURS), ("r", OURS)] {
        let target = dir.join("unpacked");
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).unwrap();
        let pdf = dir.join(maker).join("out.pdf");
        let r = run_in(
            unpacker,
            &args_of(&[pdf.to_str().unwrap(), "input_pw", "foo", "unpack_files", "output", target.to_str().unwrap()]),
            b"",
        );
        assert_eq!(r.code, 0, "{} unpacking {}: {}", unpacker, maker, r.stderr);
        assert_eq!(list_dir(&target), vec![("blank.tex".to_string(), blank_tex.clone())]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// AttachFilesTest.same_file_twice, in every combination of the two programs.
#[test]
fn attach_same_file_twice() {
    if !tools_available() {
        return;
    }
    let dir = scratch("twice");
    let blank_tex = std::fs::read(Path::new(FILES).join("blank.tex")).unwrap();
    let duck_tex = std::fs::read(Path::new(FILES).join("duck.tex")).unwrap();
    for first in [ORACLE, OURS] {
        let once = dir.join("once.pdf");
        let r = run_in(
            first,
            &args_of(&["blank.pdf", "attach_files", "blank.tex", "output", once.to_str().unwrap()]),
            b"",
        );
        assert_eq!((r.code, r.stderr.as_str()), (0, ""));
        let once_s = once.to_str().unwrap().to_string();
        let problems = attach_roundtrip(
            &dir,
            &[once_s.as_str(), "attach_files", "blank.tex", "duck.tex", "output", "@/out.pdf"],
            &[("blank.tex", blank_tex.clone()), ("duck.tex", duck_tex.clone())],
        );
        assert!(problems.is_empty(), "first by {}: {}", first, problems.join("\n"));
        // both name tree entries for blank.tex are there
        let listing = tool("pdfdetach", &["-list", dir.join("r/out.pdf").to_str().unwrap()]).1;
        assert!(listing.starts_with("3 embedded files"), "{}", listing);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// More names than fit one name tree leaf (PdfNameTree.writeTree's /Kids branch).
#[test]
fn attach_many_files() {
    if !tools_available() {
        return;
    }
    let dir = scratch("many");
    let many = dir.join("many");
    std::fs::create_dir_all(&many).unwrap();
    let mut paths = Vec::new();
    let mut expected = Vec::new();
    let names: Vec<String> = (1..=150).map(|i| format!("f{}.txt", i)).collect();
    for name in &names {
        let data = format!("file {}\n", name).into_bytes();
        std::fs::write(many.join(name), &data).unwrap();
        paths.push(many.join(name).to_str().unwrap().to_string());
        expected.push((name.as_str(), data));
    }
    let mut args: Vec<&str> = vec!["blank.pdf", "attach_files"];
    args.extend(paths.iter().map(|s| s.as_str()));
    args.extend(["output", "@/out.pdf"]);
    let problems = attach_roundtrip(&dir, &args, &expected);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A file that cannot be read while attaching to a page: Java dies with an uncaught
/// IOException (exit code 2), leaving an empty output file.
#[test]
fn attach_to_page_missing_file() {
    if !tools_available() {
        return;
    }
    let dir = scratch("missing");
    let (java, rust) = both(
        &dir,
        &["blank.pdf", "attach_files", "/nonexistent", "to_page", "1", "output", "@/out.pdf"],
        b"",
    );
    assert_eq!((java.code, rust.code), (2, 2));
    let head = |s: &str| s.lines().take(2).map(|l| l.to_string()).collect::<Vec<_>>();
    assert_eq!(head(&java.stderr), head(&rust.stderr));
    for sub in ["j", "r"] {
        assert_eq!(std::fs::metadata(dir.join(sub).join("out.pdf")).unwrap().len(), 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// unpack_files: AttachFilesTest.no_attachment, output directory forms, PROMPT, errors.
#[test]
fn unpack_files_cases() {
    if !tools_available() {
        return;
    }
    let dir = scratch("unpack");
    // a PDF with a document level and two page level attachments, made by the oracle
    let step = dir.join("step.pdf");
    let src = dir.join("src.pdf");
    let r = run_in(
        ORACLE,
        &args_of(&["refs.pdf", "attach_files", "blank.tex", "output", step.to_str().unwrap()]),
        b"",
    );
    assert_eq!(r.code, 0);
    let r = run_in(
        ORACLE,
        &args_of(&[
            step.to_str().unwrap(),
            "attach_files",
            "duck.tex",
            "refs.tex",
            "to_page",
            "2",
            "output",
            src.to_str().unwrap(),
        ]),
        b"",
    );
    assert_eq!(r.code, 0);
    let src = src.to_str().unwrap().to_string();
    let missing = dir.join("no/such/dir");
    let missing = missing.to_str().unwrap().to_string();

    let cases: Vec<(Vec<&str>, Vec<u8>)> = vec![
        (vec!["blank.pdf", "unpack_files", "output", "@"], Vec::new()),
        (vec![src.as_str(), "unpack_files", "output", "@"], Vec::new()),
        (vec![src.as_str(), "unpack_files", "output", "@/"], Vec::new()),
        (vec![src.as_str(), "unpack_files", "output", "PROMPT"], b"@\n".to_vec()),
        // existing files are overwritten without asking ...
        (vec![src.as_str(), "unpack_files", "output", "@"], Vec::new()),
        // ... unless do_ask is given: decline the first
        (vec![src.as_str(), "unpack_files", "output", "@", "do_ask"], b"n\n".to_vec()),
        (vec![src.as_str(), "unpack_files", "output", missing.as_str()], Vec::new()),
        (vec![src.as_str(), "blank.pdf", "unpack_files", "output", "@"], Vec::new()),
        (vec![src.as_str(), "unpack_files"], Vec::new()),
    ];
    let mut failures = Vec::new();
    for (i, (args, stdin)) in cases.iter().enumerate() {
        // "@" on stdin is the program's own directory, too
        let run = |bin: &str, sub: &str| {
            let own = dir.join(sub);
            let own = own.to_str().unwrap();
            let args: Vec<String> = args.iter().map(|a| a.replace('@', own)).collect();
            let stdin = String::from_utf8_lossy(stdin).replace('@', own);
            let mut r = run_in(bin, &args, stdin.as_bytes());
            r.stderr = r.stderr.replace(own, "@");
            r.stdout = String::from_utf8_lossy(&r.stdout).replace(own, "@").into_bytes();
            r
        };
        let java = run(ORACLE, "j");
        let rust = run(OURS, "r");
        // with do_ask Java's Scanner fails at the second question once stdin is exhausted
        // (a new Scanner per prompt); compare only up to the first answer there
        let asks = args.contains(&"do_ask");
        if !asks
            && (java.code != rust.code || java.stderr != rust.stderr || java.stdout != rust.stdout)
        {
            failures.push(format!(
                "case {} {:?}:\n  java: {} {:?} {:?}\n  rust: {} {:?} {:?}",
                i,
                args,
                java.code,
                String::from_utf8_lossy(&java.stdout),
                java.stderr,
                rust.code,
                String::from_utf8_lossy(&rust.stdout),
                rust.stderr
            ));
        }
        if asks {
            let first_line = |r: &Run| String::from_utf8_lossy(&r.stdout).lines().next().map(|l| l.to_string());
            if first_line(&java) != first_line(&rust) || first_line(&rust).is_none() {
                failures.push(format!("case {} {:?}: overwrite question differs", i, args));
            }
        }
        let (j, r) = (list_dir(&dir.join("j")), list_dir(&dir.join("r")));
        if j != r {
            failures.push(format!(
                "case {} {:?}: unpacked files differ: java {:?} rust {:?}",
                i,
                args,
                j.iter().map(|(n, d)| (n.clone(), d.len())).collect::<Vec<_>>(),
                r.iter().map(|(n, d)| (n.clone(), d.len())).collect::<Vec<_>>()
            ));
        }
        if i == 1 && r.len() != 3 {
            failures.push(format!("expected 3 unpacked files, got {}", r.len()));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!("unpack_files_cases: {} cases match the oracle", cases.len());
}
