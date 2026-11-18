//! End-to-end tests of the core PDF layer and burst through the pdftk-rs binary, against the
//! Java pdftk (/usr/bin/pdftk) and qpdf as oracles. Ports BurstTest.java, CryptoTest.java
//! and ReaderTest.java.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const OURS: &str = env!("CARGO_BIN_EXE_pdftk-rs");
const JAVA: &str = "/usr/bin/pdftk";

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/files")
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdftk-rs-e2e-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn run_in(tool: &str, dir: &Path, args: &[&str]) -> Run {
    let out = Command::new(tool)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{}: {}", tool, e));
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn qpdf_check(path: &Path, password: Option<&str>) {
    let mut cmd = Command::new("qpdf");
    cmd.arg("--check");
    if let Some(pw) = password {
        cmd.arg(format!("--password={}", pw));
    }
    let out = cmd.arg(path).output().unwrap();
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && !report.contains("WARNING"),
        "qpdf --check {}:\n{}",
        path.display(),
        report
    );
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The text of each page, as pdftotext sees it.
fn page_text(path: &Path, password: Option<&str>) -> String {
    let mut cmd = Command::new("pdftotext");
    if let Some(pw) = password {
        cmd.args(["-upw", pw]);
    }
    let out = cmd.arg(path).arg("-").output().unwrap();
    assert!(out.status.success(), "pdftotext {}", path.display());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn svg(pdf: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut child = Command::new("pdftocairo")
        .args(["-svg", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(pdf).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success() && !out.stdout.is_empty());
    out.stdout
}

/// Compare two SVG renderings, allowing numbers to differ in the fifth significant digit or
/// so: lopdf holds PDF reals as f32, so a /MediaBox of 595.303937 is written as 595.30396.
fn assert_same_rendering(ours: &[u8], java: &[u8], what: &str) {
    fn tokens(svg: &[u8]) -> Vec<(String, Option<f64>)> {
        let text = String::from_utf8_lossy(svg);
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut numeric = false;
        for c in text.chars() {
            let is_num = c.is_ascii_digit() || c == '.';
            if is_num != numeric && !cur.is_empty() {
                let value = if numeric { cur.parse::<f64>().ok() } else { None };
                out.push((std::mem::take(&mut cur), value));
            }
            numeric = is_num;
            cur.push(c);
        }
        let value = if numeric { cur.parse::<f64>().ok() } else { None };
        out.push((cur, value));
        out
    }
    let a = tokens(ours);
    let b = tokens(java);
    assert_eq!(a.len(), b.len(), "{}", what);
    for (x, y) in a.iter().zip(b.iter()) {
        match (x.1, y.1) {
            (Some(p), Some(q)) => assert!((p - q).abs() <= 0.002, "{}: {} vs {}", what, x.0, y.0),
            _ => assert_eq!(x.0, y.0, "{}", what),
        }
    }
}

/// Run the same command with both tools, each in its own empty directory, and require the
/// same exit code, the same stderr and stdout, and the same set of files.
fn same_as_java(name: &str, args: &[&str]) -> (PathBuf, PathBuf) {
    let ours_dir = tmp_dir(&format!("{}-ours", name));
    let java_dir = tmp_dir(&format!("{}-java", name));
    let ours = run_in(OURS, &ours_dir, args);
    let java = run_in(JAVA, &java_dir, args);
    assert_eq!(ours.code, java.code, "{:?}\nours: {}\njava: {}", args, ours.stderr, java.stderr);
    assert_eq!(ours.stderr, java.stderr, "{:?}", args);
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&java.stdout),
        "{:?}",
        args
    );
    assert_eq!(listing(&ours_dir), listing(&java_dir), "{:?}", args);
    (ours_dir, java_dir)
}

// BurstTest.burst_issue18, burst_issue90 (and a few more inputs and patterns)
#[test]
fn burst_matches_java() {
    let mut pages = 0;
    for (i, (file, extra)) in [
        ("issue18.pdf", vec!["output", "page%04d.pdf"]),
        ("issue90.pdf", vec!["output", "page%04d.pdf"]),
        ("refs.pdf", vec![]), // default pattern pg_%04d.pdf
        ("refs.pdf", vec!["output", "fixed"]), // no conversion: pattern is appended
        ("form.pdf", vec!["output", "f%d.pdf"]),
        ("duck.pdf", vec!["output", "d-%3d.pdf", "compress"]),
        ("blank.pdf", vec!["output", "u_%1$x.pdf", "uncompress"]),
        ("issue88.pdf", vec!["output", "i%d.pdf"]), // damaged, encrypted input
    ]
    .into_iter()
    .enumerate()
    {
        let input = fixture(file);
        let mut args = vec![input.as_str(), "burst"];
        args.extend(extra);
        let (ours_dir, java_dir) = same_as_java(&format!("burst{}", i), &args);
        // doc_data.txt is the same report
        assert_eq!(
            std::fs::read_to_string(ours_dir.join("doc_data.txt")).unwrap(),
            std::fs::read_to_string(java_dir.join("doc_data.txt")).unwrap(),
            "{:?}",
            args
        );
        for name in listing(&ours_dir) {
            if !name.ends_with(".pdf") && name != "fixedpg_0001.pdf" {
                continue;
            }
            let ours = ours_dir.join(&name);
            qpdf_check(&ours, None);
            assert_eq!(page_text(&ours, None), page_text(&java_dir.join(&name), None), "{} {}", file, name);
            assert_same_rendering(
                &svg(&std::fs::read(&ours).unwrap()),
                &svg(&std::fs::read(java_dir.join(&name)).unwrap()),
                &format!("{} {}", file, name),
            );
            pages += 1;
        }
        let _ = std::fs::remove_dir_all(ours_dir);
        let _ = std::fs::remove_dir_all(java_dir);
    }
    assert!(pages >= 30, "{}", pages);
}

#[test]
fn burst_with_encryption_matches_java() {
    let input = fixture("form.pdf");
    let (ours_dir, java_dir) = same_as_java(
        "burst-enc",
        &[&input, "burst", "output", "p%d.pdf", "user_pw", "foo", "owner_pw", "bar", "encrypt_128bit", "allow", "printing"],
    );
    let mut n = 0;
    for name in listing(&ours_dir) {
        if !name.ends_with(".pdf") {
            continue;
        }
        let show = |dir: &Path| {
            let out = Command::new("qpdf")
                .args(["--show-encryption", "--password=foo"])
                .arg(dir.join(&name))
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        assert_eq!(show(&ours_dir), show(&java_dir), "{}", name);
        qpdf_check(&ours_dir.join(&name), Some("foo"));
        assert_eq!(
            page_text(&ours_dir.join(&name), Some("foo")),
            page_text(&java_dir.join(&name), Some("foo"))
        );
        n += 1;
    }
    assert!(n >= 1);
    let _ = std::fs::remove_dir_all(ours_dir);
    let _ = std::fs::remove_dir_all(java_dir);
}

#[test]
fn burst_errors_match_java() {
    let refs = fixture("refs.pdf");
    let blank = fixture("blank.pdf");
    for (i, args) in [
        vec![refs.as_str(), "burst", "output", "x%q.pdf"],
        vec![refs.as_str(), "burst", "output", "%d%d.pdf"],
        vec![blank.as_str(), "burst", "output", "%-d.pdf"],
        vec![refs.as_str(), "burst", "output", "nodir/p%d.pdf"],
    ]
    .into_iter()
    .enumerate()
    {
        let (a, b) = same_as_java(&format!("burst-err{}", i), &args);
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(b);
    }
}

// ReaderTest.mergerequest21, mergerequest21_2, and the other ways opening an input fails
#[test]
fn reader_errors_match_java() {
    let dir = tmp_dir("reader-inputs");
    let p = |name: &str| dir.join(name).to_str().unwrap().to_string();
    std::fs::write(p("junk.pdf"), b"hello\n").unwrap();
    std::fs::write(p("empty.pdf"), b"").unwrap();
    std::fs::write(p("hdr.pdf"), b"%PDF-1.4\nfoo").unwrap();
    let refs = std::fs::read(fixture("refs.pdf")).unwrap();
    std::fs::write(p("trunc.pdf"), &refs[..3000]).unwrap();

    let cve = fixture("CVE-2007-0103_AcrobatReader");
    let cve_mutated = fixture("CVE-2007-0103_AcrobatReader_mutated");
    let mut n = 0;
    for input in [cve, cve_mutated, p("junk.pdf"), p("empty.pdf"), p("hdr.pdf"), p("trunc.pdf"), p("nonexistent.pdf")] {
        for op in [vec!["output", "/dev/null"], vec!["burst"], vec!["cat", "output", "out.pdf"]] {
            let mut args = vec![input.as_str()];
            args.extend(op);
            let ours = run_in(OURS, &dir, &args);
            let java = run_in(JAVA, &dir, &args);
            assert_eq!(java.code, 1, "{:?}", args);
            assert_eq!(ours.code, java.code, "{:?}: {}", args, ours.stderr);
            assert_eq!(ours.stderr, java.stderr, "{:?}", args);
            n += 1;
        }
    }
    let ours = run_in(OURS, &dir, &[&fixture("CVE-2007-0103_AcrobatReader"), "output", "/dev/null"]);
    assert!(ours.stderr.contains("Invalid reference on Kids"));
    assert_eq!(n, 21);
    let _ = std::fs::remove_dir_all(dir);
}

// CryptoTest.set_password
#[test]
fn crypto_set_password() {
    let dir = tmp_dir("crypto-set");
    let r = run_in(
        OURS,
        &dir,
        &[&fixture("blank.pdf"), "output", "/dev/null", "owner_pw", "\"'**", "user_pw", "**\"'"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let _ = std::fs::remove_dir_all(dir);
}

// CryptoTest.idempotent_aes / idempotent_rc4 (plus 40 bit, and Java reading what we wrote)
#[test]
fn crypto_idempotent() {
    let dir = tmp_dir("crypto-idem");
    let blank = fixture("blank.pdf");
    let expected = svg(&std::fs::read(&blank).unwrap());
    for strength in ["encrypt_aes128", "encrypt_128bit", "encrypt_40bit"] {
        let encrypted = dir.join(format!("{}.pdf", strength));
        let e = encrypted.to_str().unwrap();
        let r = run_in(OURS, &dir, &[&blank, "output", e, strength, "user_pw", "correcthorsebatterystaple"]);
        assert_eq!(r.code, 0, "{}", r.stderr);
        qpdf_check(&encrypted, Some("correcthorsebatterystaple"));
        for tool in [OURS, JAVA] {
            let r = run_in(tool, &dir, &[e, "input_pw", "correcthorsebatterystaple", "output", "-"]);
            assert_eq!(r.code, 0, "{} {}: {}", tool, strength, r.stderr);
            assert_eq!(svg(&r.stdout), expected, "{} {}", tool, strength);
        }
        // and the other way round: we read what Java encrypts
        let java_encrypted = dir.join(format!("java-{}.pdf", strength));
        let je = java_encrypted.to_str().unwrap();
        let r = run_in(JAVA, &dir, &[&blank, "output", je, strength, "user_pw", "correcthorsebatterystaple"]);
        assert_eq!(r.code, 0, "{}", r.stderr);
        let r = run_in(OURS, &dir, &[je, "input_pw", "correcthorsebatterystaple", "output", "-"]);
        assert_eq!(r.code, 0, "{}: {}", strength, r.stderr);
        assert_eq!(svg(&r.stdout), expected, "{}", strength);
    }
    let _ = std::fs::remove_dir_all(dir);
}

// CryptoTest.no_password_fails / wrong_password_fails
#[test]
fn crypto_password_failures() {
    let dir = tmp_dir("crypto-fail");
    let encrypted = dir.join("encrypted.pdf");
    let e = encrypted.to_str().unwrap();
    let r = run_in(OURS, &dir, &[&fixture("blank.pdf"), "output", e, "user_pw", "correcthorsebatterystaple"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    // no strength keyword: AES-128, as in Java
    let show = Command::new("qpdf")
        .args(["--show-encryption", "--password=correcthorsebatterystaple", e])
        .output()
        .unwrap();
    let show = String::from_utf8_lossy(&show.stdout).into_owned();
    assert!(show.contains("R = 4") && show.contains("stream encryption method: AESv2"), "{}", show);

    for (args, message) in [
        (vec![e, "output", "-"], "OWNER OR USER PASSWORD REQUIRED, but not given"),
        (vec![e, "input_pw", "Tr0ub4dor&3", "output", "-"], "OWNER OR USER PASSWORD REQUIRED, but incorrect"),
    ] {
        let ours = run_in(OURS, &dir, &args);
        let java = run_in(JAVA, &dir, &args);
        assert_eq!(ours.code, 1);
        assert!(ours.stderr.contains(message), "{}", ours.stderr);
        assert_eq!(ours.stderr, java.stderr);
        assert_eq!(ours.stdout, java.stdout);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Output options on the filter path: the encryption parameters are exactly Java's, and the
/// owner-password warning appears under the same conditions.
#[test]
fn output_options_match_java() {
    let dir = tmp_dir("output-opts");
    let refs = fixture("refs.pdf");
    let mut n = 0;
    for (i, opts) in [
        vec!["owner_pw", "foo", "user_pw", "bar", "encrypt_128bit", "allow", "printing"],
        vec!["owner_pw", "foo", "encrypt_40bit", "allow", "CopyContents", "FillIn"],
        vec!["user_pw", "bar", "allow", "AllFeatures"],
        vec!["owner_pw", "foo", "user_pw", "bar", "encrypt_aes128"],
        vec!["encrypt_128bit"],
    ]
    .into_iter()
    .enumerate()
    {
        let ours_path = dir.join(format!("ours{}.pdf", i));
        let java_path = dir.join(format!("java{}.pdf", i));
        let user = if opts.contains(&"user_pw") { "bar" } else { "" };
        for (tool, path) in [(OURS, &ours_path), (JAVA, &java_path)] {
            let mut args = vec![refs.as_str(), "output", path.to_str().unwrap()];
            args.extend(opts.iter());
            let r = run_in(tool, &dir, &args);
            assert_eq!(r.code, 0, "{} {:?}: {}", tool, opts, r.stderr);
        }
        qpdf_check(&ours_path, Some(user));
        let show = |path: &Path| {
            let out = Command::new("qpdf")
                .arg("--show-encryption")
                .arg(format!("--password={}", user))
                .arg(path)
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        assert_eq!(show(&ours_path), show(&java_path), "{:?}", opts);
        assert_eq!(page_text(&ours_path, Some(user)), page_text(&java_path, Some(user)));

        // each tool reads the other's file the same way, with the user and the owner password
        for pw in ["bar", "foo", ""] {
            let mut results = Vec::new();
            for (tool, path) in [(OURS, &java_path), (JAVA, &java_path), (OURS, &ours_path), (JAVA, &ours_path)] {
                let p = path.to_str().unwrap();
                let mut args = vec![p];
                if !pw.is_empty() {
                    args.extend(["input_pw", pw]);
                }
                args.extend(["dump_data"]);
                let r = run_in(tool, &dir, &args);
                let stderr = r.stderr.replace(p, "FILE");
                let pages = String::from_utf8_lossy(&r.stdout).contains("NumberOfPages: 8");
                results.push((r.code, stderr, pages));
                n += 1;
            }
            assert!(results.iter().all(|r| r == &results[0]), "{:?} pw={:?}: {:?}", opts, pw, results);
        }
    }
    assert_eq!(n, 60);

    // compress / uncompress
    for mode in ["compress", "uncompress"] {
        for file in ["refs.pdf", "duck.pdf", "form.pdf", "issue19.pdf"] {
            let ours_path = dir.join(format!("ours-{}-{}", mode, file));
            let java_path = dir.join(format!("java-{}-{}", mode, file));
            for (tool, path) in [(OURS, &ours_path), (JAVA, &java_path)] {
                let r = run_in(tool, &dir, &[&fixture(file), "output", path.to_str().unwrap(), mode]);
                assert_eq!(r.code, 0, "{} {} {}: {}", tool, mode, file, r.stderr);
            }
            qpdf_check(&ours_path, None);
            assert_eq!(page_text(&ours_path, None), page_text(&java_path, None), "{} {}", mode, file);
            let count = |path: &Path, needle: &[u8]| {
                let bytes = std::fs::read(path).unwrap();
                bytes.windows(needle.len()).filter(|w| *w == needle).count()
            };
            // the page markers, and the same streams left undecoded
            assert_eq!(count(&ours_path, b"/pdftk_PageNum"), count(&java_path, b"/pdftk_PageNum"), "{} {}", mode, file);
            if mode == "uncompress" {
                assert_eq!(count(&ours_path, b"/FlateDecode"), count(&java_path, b"/FlateDecode"), "{}", file);
                assert_eq!(count(&ours_path, b"/DCTDecode"), count(&java_path, b"/DCTDecode"), "{}", file);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}
