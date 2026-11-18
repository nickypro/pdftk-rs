//! cat / shuffle against the Java pdftk oracle (/usr/bin/pdftk): ports of CatTest.java and
//! MultipleTest.java plus wider coverage. Each case runs both programs and compares exit
//! code, stderr, `qpdf --check`, page count, per-page text and `pdftk dump_data` (bookmarks,
//! page media, rotation, labels). Skipped when the oracle is not installed.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const ORACLE: &str = "/usr/bin/pdftk";
const FILES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files");
const BIG: &str = "/data/private/2510.21203.pdf";

fn have_oracle() -> bool {
    let ok = Path::new(ORACLE).exists();
    if !ok {
        eprintln!("oracle {} not installed; skipping", ORACLE);
    }
    ok
}

struct Tmp(PathBuf);
impl Tmp {
    fn new(name: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("pdftk-rs-cat-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Out {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn run_with(bin: &str, args: &[&str], stdin: Option<&str>) -> Out {
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(FILES);
    match stdin {
        Some(file) => cmd.stdin(std::fs::File::open(Path::new(FILES).join(file)).unwrap()),
        None => cmd.stdin(Stdio::null()),
    };
    let out = cmd.output().expect("failed to run");
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn rs_bin() -> &'static str {
    env!("CARGO_BIN_EXE_pdftk-rs")
}

fn tool(bin: &str, args: &[&str]) -> (i32, String) {
    let out = Command::new(bin).args(args).stdin(Stdio::null()).output().expect("tool");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// dump_data without the Info / ID entries (which legitimately differ: dates, producer).
fn dump_data(pdf: &str, pw: Option<&str>) -> String {
    let mut args = vec![pdf];
    if let Some(pw) = pw {
        args.extend(["input_pw", pw]);
    }
    args.push("dump_data_utf8");
    let (code, text) = tool(ORACLE, &args);
    assert_eq!(code, 0, "dump_data failed on {}", pdf);
    text.lines()
        .filter(|l| !l.starts_with("Info") && !l.starts_with("PdfID"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pages_text(pdf: &str, pw: Option<&str>) -> Vec<String> {
    let mut args = vec![];
    if let Some(pw) = pw {
        args.extend(["-upw", pw]);
    }
    args.extend([pdf, "-"]);
    let (code, text) = tool("pdftotext", &args);
    assert_eq!(code, 0, "pdftotext failed on {}", pdf);
    text.split('\u{c}').map(|s| s.to_string()).collect()
}

fn qpdf_check(pdf: &str, pw: Option<&str>) {
    let pw_arg;
    let mut args = vec!["--check"];
    if let Some(pw) = pw {
        pw_arg = format!("--password={}", pw);
        args.push(&pw_arg);
    }
    args.push(pdf);
    let out = Command::new("qpdf").args(&args).output().expect("qpdf");
    assert!(
        out.status.success(),
        "qpdf --check {}: {}{}",
        pdf,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Compare two PDFs semantically.
fn assert_same_pdf(what: &str, java: &str, rust: &str, pw: Option<&str>) {
    qpdf_check(rust, pw);
    let dj = dump_data(java, pw);
    let dr = dump_data(rust, pw);
    assert_eq!(dj, dr, "{}: dump_data differs", what);
    let tj = pages_text(java, pw);
    let tr = pages_text(rust, pw);
    assert_eq!(tj.len(), tr.len(), "{}: page count differs", what);
    for (i, (a, b)) in tj.iter().zip(tr.iter()).enumerate() {
        assert_eq!(a, b, "{}: text of page {} differs", what, i + 1);
    }
    let sj = std::fs::metadata(java).unwrap().len();
    let sr = std::fs::metadata(rust).unwrap().len();
    assert!(
        sr <= sj + sj / 4 + 2048,
        "{}: output is {} bytes, the oracle's {} bytes",
        what,
        sr,
        sj
    );
}

/// Run `args… output <file> extra…` through both programs and compare everything.
/// Returns the path of our output (inside `tmp`).
fn compare_full(tmp: &Tmp, name: &str, args: &[&str], extra: &[&str], stdin: Option<&str>, pw: Option<&str>) -> String {
    let jout = tmp.path(&format!("{}-java.pdf", name));
    let rout = tmp.path(&format!("{}-rust.pdf", name));
    let mut ja: Vec<&str> = args.to_vec();
    ja.extend(["output", &jout]);
    ja.extend(extra);
    let mut ra: Vec<&str> = args.to_vec();
    ra.extend(["output", &rout]);
    ra.extend(extra);
    let j = run_with(ORACLE, &ja, stdin);
    let r = run_with(rs_bin(), &ra, stdin);
    assert_eq!(j.code, r.code, "{:?}: exit code (java stderr {:?}, rust stderr {:?})", args, j.stderr, r.stderr);
    assert_eq!(j.stderr, r.stderr, "{:?}: stderr", args);
    // (verbose output names the output file)
    assert_eq!(
        String::from_utf8_lossy(&j.stdout).replace(&jout, "OUT"),
        String::from_utf8_lossy(&r.stdout).replace(&rout, "OUT"),
        "{:?}: stdout",
        args
    );
    if j.code == 0 {
        assert_same_pdf(&format!("{:?} {:?}", args, extra), &jout, &rout, pw);
    }
    rout
}

fn compare(tmp: &Tmp, name: &str, args: &[&str]) -> String {
    compare_full(tmp, name, args, &[], None, None)
}

/// Two of our own command lines must give equivalent PDFs (the assertPdfEqualsAsSVG tests).
fn assert_equivalent(tmp: &Tmp, name: &str, a: &[&str], b: &[&str]) {
    let ra = compare(tmp, &format!("{}-a", name), a);
    let rb = compare(tmp, &format!("{}-b", name), b);
    assert_eq!(dump_data(&ra, None), dump_data(&rb, None));
    assert_eq!(pages_text(&ra, None), pages_text(&rb, None));
}

fn field_names(pdf: &str) -> Vec<String> {
    let (code, text) = tool(ORACLE, &[pdf, "dump_data_fields_utf8"]);
    assert_eq!(code, 0);
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("FieldName: "))
        .map(|s| s.to_string())
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------------------------
// CatTest.java

#[test]
fn cat() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("cat");
    let ours = compare(&tmp, "cat", &["refs.pdf", "refsalt.pdf", "cat"]);
    let expected = format!("{}/cat-refs-refsalt.pdf", FILES);
    assert_eq!(pages_text(&expected, None), pages_text(&ours, None));
}

#[test]
fn cat_rotate_page_no_op() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("rot-noop");
    assert_equivalent(&tmp, "p", &["blank.pdf", "cat"], &["blank.pdf", "cat", "1north"]);
}

#[test]
fn cat_rotate_range_no_op() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("rot-range-noop");
    assert_equivalent(&tmp, "p", &["blank.pdf", "cat"], &["blank.pdf", "cat", "1-1north"]);
}

#[test]
fn cat_rotate_page() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("rot-page");
    let ours = compare(&tmp, "p", &["blank.pdf", "cat", "1east"]);
    assert!(dump_data(&ours, None).contains("PageMediaRotation: 90"));
}

#[test]
fn cat_rotate_range() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("rot-range");
    assert_equivalent(&tmp, "p", &["blank.pdf", "cat", "1east"], &["blank.pdf", "cat", "1-1east"]);
}

#[test]
fn cat_exclude_range() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("exclude");
    assert_equivalent(&tmp, "p", &["refs.pdf", "cat", "1-3", "6-8"], &["refs.pdf", "cat", "~4-5"]);
}

#[test]
fn cat_include_exclude_range() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("incexc");
    assert_equivalent(
        &tmp,
        "p",
        &["refs.pdf", "cat", "2-3", "6-7"],
        &["refs.pdf", "cat", "2-end~4-5~end"],
    );
}

#[test]
fn cat_even() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("even");
    assert_equivalent(&tmp, "p", &["refs.pdf", "cat", "2", "4", "6"], &["refs.pdf", "cat", "2-7even"]);
}

#[test]
fn cat_odd() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("odd");
    assert_equivalent(&tmp, "p", &["refs.pdf", "cat", "3", "5", "7"], &["refs.pdf", "cat", "2-7odd"]);
}

#[test]
fn cat_handles() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("handles");
    compare(&tmp, "a", &["refs.pdf", "refsalt.pdf", "cat"]);
    compare(&tmp, "b", &["A=refs.pdf", "B=refsalt.pdf", "cat", "B", "A"]);
}

#[test]
fn duplicate_stdin() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("dupstdin");
    let ours = compare_full(&tmp, "p", &["A=-", "cat", "A", "A", "A"], &[], Some("blank.pdf"), None);
    assert_eq!(pages_text(&ours, None).len(), pages_text(&format!("{}/blank.pdf", FILES), None).len() * 3 - 2);
}

// ---------------------------------------------------------------------------------------
// MultipleTest.java

#[test]
fn cat_renames_clashing_forms() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("clash");
    let original = field_names(&format!("{}/form.pdf", FILES));
    let ours = compare(&tmp, "dup", &["A=form.pdf", "cat", "A", "A"]);
    let mut unique = field_names(&ours);
    unique.dedup();
    assert_eq!(2 * original.len(), unique.len());
    // and exactly the names the oracle produces
    assert_eq!(field_names(&tmp.path("dup-java.pdf")), field_names(&ours));
    // the full field dump agrees as well
    let (_, fj) = tool(ORACLE, &[&tmp.path("dup-java.pdf"), "dump_data_fields_utf8"]);
    let (_, fr) = tool(ORACLE, &[&ours, "dump_data_fields_utf8"]);
    assert_eq!(fj, fr);
}

#[test]
fn can_fill_cat_form() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("fillcat");
    let ours = compare(&tmp, "dup", &["A=form.pdf", "cat", "A", "A"]);
    let fdf = tmp.path("dup.fdf");
    let filled = tmp.path("filled.pdf");
    // (with the oracle, so that this does not depend on the other operations being ported)
    assert_eq!(tool(ORACLE, &[&ours, "generate_fdf", "output", &fdf]).0, 0);
    assert_eq!(tool(ORACLE, &[&ours, "fill_form", &fdf, "output", &filled]).0, 0);
    qpdf_check(&filled, None);
}

// ---------------------------------------------------------------------------------------
// wider coverage

/// issue123.pdf has outline destinations whose page is a number, not a reference. The 3.3.3
/// source we port reads those (test/files/issue123.data says page 1); the installed 3.3.2
/// oracle does not (page 0). So only the rest is compared with the oracle.
#[test]
fn cat_issue123_numeric_destinations() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("issue123");
    let jout = tmp.path("j.pdf");
    let rout = tmp.path("r.pdf");
    let j = run_with(ORACLE, &["issue123.pdf", "cat", "output", &jout], None);
    let r = run_with(rs_bin(), &["issue123.pdf", "cat", "output", &rout], None);
    assert_eq!((j.code, &j.stderr), (r.code, &r.stderr));
    qpdf_check(&rout, None);
    assert_eq!(pages_text(&jout, None), pages_text(&rout, None));
    let strip = |d: String| -> String {
        d.lines().filter(|l| !l.starts_with("BookmarkPageNumber")).collect::<Vec<_>>().join("\n")
    };
    let ours = dump_data(&rout, None);
    assert_eq!(strip(dump_data(&jout, None)), strip(ours.clone()));
    let expected = std::fs::read_to_string(format!("{}/issue123.data", FILES)).unwrap();
    let bookmarks = |d: &str| -> Vec<String> {
        d.lines().filter(|l| l.starts_with("Bookmark")).map(|s| s.to_string()).collect()
    };
    assert_eq!(bookmarks(&expected), bookmarks(&ours));
}

#[test]
fn single_file_ranges() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("ranges");
    let cases: &[&[&str]] = &[
        &["refs.pdf", "cat", "1-3", "5", "end-1", "r2", "2-endeven", "1-endeast"],
        &["refs.pdf", "cat", "end-1"],
        &["refs.pdf", "cat", "r3-r1", "1-2south", "3west", "4-5left", "6right", "7down"],
        &["refs.pdf", "cat", "1-endodd", "2-endevenwest"],
        &["blank-rotated.pdf", "cat", "1east", "1north", "1left", "1"],
        &["refs.pdf", "cat", "3", "3", "3"],
        &["refs.pdf", "cat", "1-end", "1-end"],
        &["refs.pdf", "cat", "1-end", "end-1", "2-3", "3"],
        &["duck.pdf", "cat"],
        &["annotation.pdf", "cat"],
        &["issue18.pdf", "cat"],
        &["issue19.pdf", "cat"],
        &["issue21.pdf", "cat"],
        &["issue88.pdf", "cat"],
        &["issue90.pdf", "cat"],
        &["form-pdfdocencoding.pdf", "cat"],
        &["refs.pdf", "cat", "9"],
        &["refs.pdf", "cat", "0"],
        &["refs.pdf", "cat", "B1"],
    ];
    for (i, args) in cases.iter().enumerate() {
        compare(&tmp, &format!("c{}", i), args);
    }
}

#[test]
fn multiple_inputs_with_handles() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("multi");
    let cases: &[&[&str]] = &[
        &["A=refs.pdf", "B=refsalt.pdf", "cat", "A1", "B", "A2-end"],
        &["A=refs.pdf", "B=refsalt.pdf", "cat", "A1", "B1", "A1", "B1east", "A1south"],
        &["A=refs.pdf", "B=blank.pdf", "C=duck.pdf", "cat", "C", "B", "A", "C", "B"],
        &["A=refs.pdf", "B=refs.pdf", "cat", "A", "B"],
        &["A=form.pdf", "B=form-pdfdocencoding.pdf", "cat", "A", "B", "A"],
        &["A=form.pdf", "cat", "A", "A", "A", "A"],
        &["form.pdf", "form.pdf", "cat"],
        &["annotation.pdf", "refs.pdf", "annotation.pdf", "cat"],
    ];
    for (i, args) in cases.iter().enumerate() {
        compare(&tmp, &format!("c{}", i), args);
    }
}

#[test]
fn shuffle() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("shuffle");
    let cases: &[&[&str]] = &[
        &["A=refs.pdf", "B=refsalt.pdf", "shuffle", "A", "B"],
        &["A=refs.pdf", "B=refsalt.pdf", "shuffle", "A1-2", "B"],
        &["A=refs.pdf", "shuffle", "A1-4", "Aend-5", "A1-2east"],
        &["refs.pdf", "refsalt.pdf", "shuffle"],
        &["refs.pdf", "shuffle", "1-endeven", "1-endodd"],
        &["A=form.pdf", "B=refs.pdf", "shuffle", "A", "B1-3", "A"],
    ];
    for (i, args) in cases.iter().enumerate() {
        compare(&tmp, &format!("c{}", i), args);
    }
}

#[test]
fn bookmarks() {
    if !have_oracle() || !Path::new(BIG).exists() {
        return;
    }
    let tmp = Tmp::new("bookmarks");
    let a = format!("A={}", BIG);
    let cases: &[&[&str]] = &[
        &[BIG, "cat"],
        &[&a, "cat", "A", "A"],
        &[&a, "B=refs.pdf", "cat", "B", "A", "B", "A"],
        &[&a, "cat", "A1-end"],
        &[&a, "cat", "A3-5", "Aend", "A1east"],
        &[&a, "B=refs.pdf", "shuffle", "A", "B"],
        &[BIG, "refs.pdf", "cat"],
    ];
    for (i, args) in cases.iter().enumerate() {
        let ours = compare(&tmp, &format!("c{}", i), args);
        if i == 1 {
            assert_eq!(dump_data(&ours, None).matches("BookmarkBegin").count(), 20);
        }
    }
}

#[test]
fn output_options() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("options");
    let cases: &[(&[&str], &[&str], Option<&str>)] = &[
        (&["refs.pdf", "cat", "1-3"], &["uncompress"], None),
        (&["refs.pdf", "cat", "1-3"], &["compress"], None),
        (&["refs.pdf", "cat", "2-4"], &["owner_pw", "foo"], None),
        (&["refs.pdf", "cat", "2-4"], &["user_pw", "bar"], Some("bar")),
        (&["refs.pdf", "cat", "2-4"], &["owner_pw", "foo", "user_pw", "bar", "encrypt_40bit"], Some("bar")),
        (&["refs.pdf", "cat", "2-4"], &["owner_pw", "foo", "user_pw", "bar", "encrypt_128bit", "allow", "printing"], Some("bar")),
        (&["refs.pdf", "cat", "2-4"], &["owner_pw", "foo", "encrypt_aes128", "allow", "AllFeatures"], None),
        (&["refs.pdf", "refsalt.pdf", "cat"], &["keep_first_id"], None),
        (&["refs.pdf", "refsalt.pdf", "cat"], &["keep_final_id"], None),
        (&["refs.pdf", "cat", "1-2"], &["verbose"], None),
        (&["A=refs.pdf", "B=refsalt.pdf", "shuffle", "A1-2east", "B1"], &["verbose"], None),
        (&["form.pdf", "cat"], &["flatten"], None),
        (&["form.pdf", "cat"], &["need_appearances"], None),
    ];
    for (i, (args, extra, pw)) in cases.iter().enumerate() {
        compare_full(&tmp, &format!("c{}", i), args, extra, None, *pw);
    }
}

#[test]
fn keep_id() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("keepid");
    let ids = |pdf: &str| -> Vec<String> {
        let (_, text) = tool(ORACLE, &[pdf, "dump_data"]);
        text.lines().filter(|l| l.starts_with("PdfID")).map(|s| s.to_string()).collect()
    };
    for (flag, source) in [("keep_first_id", "refs.pdf"), ("keep_final_id", "refsalt.pdf")] {
        let out = tmp.path(&format!("{}.pdf", flag));
        let r = run_with(rs_bin(), &["refs.pdf", "refsalt.pdf", "cat", "output", &out, flag], None);
        assert_eq!(r.code, 0);
        let jout = tmp.path(&format!("{}-java.pdf", flag));
        run_with(ORACLE, &["refs.pdf", "refsalt.pdf", "cat", "output", &jout, flag], None);
        assert_eq!(ids(&jout), ids(&out));
        assert_eq!(ids(&format!("{}/{}", FILES, source)), ids(&out));
    }
}

#[test]
fn output_to_stdout() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("stdout");
    let cases: &[&[&str]] = &[
        &["refs.pdf", "refsalt.pdf", "cat", "output", "-"],
        &["A=refs.pdf", "shuffle", "A1-3", "A4-end", "output", "-"],
    ];
    for (i, args) in cases.iter().enumerate() {
        let j = run_with(ORACLE, args, None);
        let r = run_with(rs_bin(), args, None);
        assert_eq!(j.code, r.code);
        assert_eq!(j.stderr, r.stderr);
        let jout = tmp.path(&format!("{}-java.pdf", i));
        let rout = tmp.path(&format!("{}-rust.pdf", i));
        std::fs::write(&jout, &j.stdout).unwrap();
        std::fs::write(&rout, &r.stdout).unwrap();
        assert_same_pdf(&format!("{:?}", args), &jout, &rout, None);
    }
}

#[test]
fn pdf_version_and_structure() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("version");
    let header = |pdf: &str| -> String {
        let bytes = std::fs::read(pdf).unwrap();
        String::from_utf8_lossy(&bytes[..8]).into_owned()
    };
    let cases: &[(&[&str], &[&str])] = &[
        (&["blank.pdf", "cat"], &[]),
        (&["refs.pdf", "duck.pdf", "cat"], &[]),
        (&["blank.pdf", "cat"], &["owner_pw", "x"]),
        (&["blank.pdf", "cat"], &["owner_pw", "x", "encrypt_40bit"]),
        (&["blank.pdf", "cat"], &["owner_pw", "x", "encrypt_128bit"]),
        (&[BIG, "cat", "1"], &[]),
    ];
    for (i, (args, extra)) in cases.iter().enumerate() {
        if args[0] == BIG && !Path::new(BIG).exists() {
            continue;
        }
        let ours = compare_full(&tmp, &format!("c{}", i), args, extra, None, None);
        assert_eq!(header(&tmp.path(&format!("c{}-java.pdf", i))), header(&ours), "{:?}", args);
    }
}

const BM1: &str = "BookmarkBegin\nBookmarkTitle: Top one\nBookmarkLevel: 1\nBookmarkPageNumber: 1\n\
BookmarkBegin\nBookmarkTitle: Child \u{dc}n\u{ef}c\u{f6}d\u{e9} \u{2603}\nBookmarkLevel: 2\nBookmarkPageNumber: 2\n\
BookmarkBegin\nBookmarkTitle: Grandchild\nBookmarkLevel: 3\nBookmarkPageNumber: 3\n\
BookmarkBegin\nBookmarkTitle: Top two no page\nBookmarkLevel: 1\nBookmarkPageNumber: 0\n\
BookmarkBegin\nBookmarkTitle: deep last\nBookmarkLevel: 2\nBookmarkPageNumber: 8\n\
PageLabelBegin\nPageLabelNewIndex: 1\nPageLabelStart: 1\nPageLabelNumStyle: LowercaseRomanNumerals\n";
const BM2: &str = "BookmarkBegin\nBookmarkTitle: Jumped\nBookmarkLevel: 3\nBookmarkPageNumber: 2\n\
BookmarkBegin\nBookmarkTitle: Back\nBookmarkLevel: 1\nBookmarkPageNumber: 4\n\
BookmarkBegin\nBookmarkTitle: Jump again\nBookmarkLevel: 3\nBookmarkPageNumber: 5\n";

/// Nested / level-jumping / non-ASCII bookmarks (made with the oracle's update_info) are merged
/// with page offsets; page labels are not carried; encrypted inputs work.
#[test]
fn merged_bookmarks_and_encrypted_inputs() {
    if !have_oracle() {
        return;
    }
    let tmp = Tmp::new("bm");
    let (t1, t2) = (tmp.path("bm1.txt"), tmp.path("bm2.txt"));
    std::fs::write(&t1, BM1).unwrap();
    std::fs::write(&t2, BM2).unwrap();
    let (bm1, bm2, enc) = (tmp.path("bm1.pdf"), tmp.path("bm2.pdf"), tmp.path("enc.pdf"));
    let refs = format!("{}/refs.pdf", FILES);
    let refsalt = format!("{}/refsalt.pdf", FILES);
    assert_eq!(tool(ORACLE, &[&refs, "update_info_utf8", &t1, "output", &bm1]).0, 0);
    assert_eq!(tool(ORACLE, &[&refsalt, "update_info_utf8", &t2, "output", &bm2]).0, 0);
    assert_eq!(tool(ORACLE, &[&bm1, "output", &enc, "owner_pw", "own", "user_pw", "usr"]).0, 0);
    let (a, b, e) = (format!("A={}", bm1), format!("B={}", bm2), format!("A={}", enc));
    let cases: &[&[&str]] = &[
        &[&bm1, &bm2, "cat"],
        &[&a, &b, "cat", "B", "A", "B"],
        &[&a, &b, "refs.pdf", "cat", "B", "refs.pdf", "A"],
        &[&bm1, "cat", "1-end"],
        &[&bm1, "cat", "2-end"],
        &[&a, &b, "shuffle", "A", "B"],
        &[&enc, "cat"],
        &[&enc, "input_pw", "usr", "cat"],
        &[&enc, "input_pw", "own", "cat"],
        &[&e, "B=refs.pdf", "input_pw", "A=own", "cat", "A", "B1"],
    ];
    for (i, args) in cases.iter().enumerate() {
        let ours = compare(&tmp, &format!("c{}", i), args);
        if i == 1 {
            let d = dump_data(&ours, None);
            assert_eq!(d.matches("BookmarkBegin").count(), 5 + 6 + 6);
            assert!(d.contains("BookmarkTitle: Child \u{dc}n\u{ef}c\u{f6}d\u{e9} \u{2603}"));
        }
    }
}
