//! Unit tests for src/pdf.rs (included from there with #[path]); they drive the PDF layer
//! directly on the upstream fixtures, with /usr/bin/pdftk (the Java tool) and qpdf as oracles.
//! Ports the relevant cases of CryptoTest.java and ReaderTest.java.
use super::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/files")
        .join(name)
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdftk-rs-core-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// Run a tool, returning (exit status ok, stdout, stderr).
fn run(tool: &str, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(tool).args(args).output().expect(tool);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn java_pdftk(args: &[&str]) -> (bool, String, String) {
    run("/usr/bin/pdftk", args)
}

fn qpdf_check(path: &Path, password: Option<&str>) {
    let mut args = vec!["--check".to_string()];
    if let Some(pw) = password {
        args.push(format!("--password={}", pw));
    }
    args.push(path.to_str().unwrap().to_string());
    let args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let (ok, out, err) = run("qpdf", &args);
    assert!(ok, "qpdf --check failed for {}:\n{}\n{}", path.display(), out, err);
    assert!(
        !out.contains("WARNING") && !err.contains("WARNING"),
        "qpdf --check warned about {}:\n{}\n{}",
        path.display(),
        out,
        err
    );
}

fn open(path: &Path, password: &str) -> Result<OpenedPdf, OpenError> {
    let bytes = std::fs::read(path).unwrap();
    let pdfdoc = unicode_to_text_string(password);
    open_pdf_bytes(&bytes, &pdfdoc, password)
}

fn session(op: Keyword) -> TkSession {
    let mut s = TkSession::new();
    s.m_operation = op;
    s
}

fn write(session: &TkSession, doc: &mut Document, path: &Path) {
    let mut out = Vec::new();
    assert_eq!(write_output(session, doc, &mut out), ErrorCode::NoError);
    std::fs::write(path, out).unwrap();
}

/// The decoded content of every page, in order.
fn page_contents(doc: &Document) -> Vec<Vec<u8>> {
    page_ids(doc)
        .into_iter()
        .map(|id| doc.get_page_content(id))
        .collect()
}

const PDF_FIXTURES: [&str; 14] = [
    "annotation.pdf",
    "blank-rotated.pdf",
    "blank.pdf",
    "cat-refs-refsalt.pdf",
    "duck.pdf",
    "form-pdfdocencoding.pdf",
    "form.pdf",
    "issue123.pdf",
    "issue18.pdf",
    "issue19.pdf",
    "issue21.pdf",
    "issue88.pdf",
    "issue90.pdf",
    "refs.pdf",
];

#[test]
fn text_string_codecs() {
    assert_eq!(text_string_to_unicode(b"plain"), "plain");
    assert_eq!(text_string_to_unicode(b"\xfe\xff\x00M\x00a\x20\xac"), "Ma\u{20ac}");
    // surrogate pair and a dangling byte
    assert_eq!(text_string_to_unicode(b"\xfe\xff\xd8\x3d\xde\x00\x00"), "\u{1f600}\u{fffd}");
    // PDFDocEncoding: iText's table
    assert_eq!(text_string_to_unicode(b"\x80\x84\xa0\xe9\x9f"), "\u{2022}\u{2014}\u{20ac}\u{e9}\u{fffd}");
    assert_eq!(unicode_to_text_string("caf\u{e9} \u{20ac}\u{2022}"), b"caf\xe9 \xa0\x80");
    assert_eq!(unicode_to_text_string("\u{3b1}"), b"\xfe\xff\x03\xb1");
    assert_eq!(unicode_to_text_string("\u{1f600}"), b"\xfe\xff\xd8\x3d\xde\x00");
    // U+00A0 and U+FFFD are not PDFDocEncoding characters for iText
    assert_eq!(unicode_to_text_string("\u{a0}"), b"\xfe\xff\x00\xa0");
    assert!(!is_pdfdoc_encoding("\u{fffd}"));
    // every byte but the undefined 0x9f survives a round trip
    for b in 0..=255u8 {
        if b == 0x9f {
            continue;
        }
        let s = text_string_to_unicode(&[b'x', b]);
        assert_eq!(unicode_to_text_string(&s), vec![b'x', b], "byte {:#x}", b);
    }
}

#[test]
fn pdf_date_shape() {
    let d = pdf_date_now();
    assert_eq!(d.len(), 17, "{}", d);
    assert!(d.starts_with("D:20") && d.ends_with('Z'), "{}", d);
}

#[test]
fn fixtures_open_with_the_page_count_java_reports() {
    for name in PDF_FIXTURES {
        let path = fixture(name);
        let opened = open(&path, "").unwrap_or_else(|e| panic!("{}: {:?}", name, e));
        // issue88.pdf is damaged *and* encrypted (empty user password, unknown owner password)
        assert_eq!(opened.encrypted, name == "issue88.pdf", "{}", name);
        assert!(!opened.owner_password_used, "{}", name);
        let pages = page_ids(&opened.doc);
        let (ok, out, _) = java_pdftk(&[path.to_str().unwrap(), "dump_data"]);
        assert!(ok);
        let expected: usize = out
            .lines()
            .find_map(|l| l.strip_prefix("NumberOfPages: "))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(pages.len(), expected, "{}", name);
        // readPages() pushed the inherited attributes into each page
        for id in &pages {
            let page = opened.doc.get_dictionary(*id).unwrap();
            assert!(page.has(b"MediaBox"), "{}", name);
            assert!(page.has_type(b"Page"), "{}", name);
        }
    }
}

#[test]
fn page_helpers() {
    let opened = open(&fixture("blank-rotated.pdf"), "").unwrap();
    let mut doc = opened.doc;
    let page = page_ids(&doc)[0];
    let (_, out, _) = java_pdftk(&[fixture("blank-rotated.pdf").to_str().unwrap(), "dump_data"]);
    let rot: i32 = out
        .lines()
        .find_map(|l| l.strip_prefix("PageMediaRotation: "))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(page_rotation(&doc, page), rot);
    let dims: Vec<f32> = out
        .lines()
        .find_map(|l| l.strip_prefix("PageMediaRect: "))
        .unwrap()
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    let media = page_media_box(&doc, page);
    for i in 0..4 {
        assert!((media[i] - dims[i]).abs() < 0.01, "{:?} vs {:?}", media, dims);
    }
    assert_eq!(page_crop_box(&doc, page), media);

    // apply_rotation_to_page: relative, absolute, back to north (key removed)
    apply_rotation_to_page(&mut doc, 1, 90, false);
    assert_eq!(page_rotation(&doc, page), (rot + 90) % 360);
    apply_rotation_to_page(&mut doc, 1, 180, true);
    assert_eq!(page_rotation(&doc, page), 180);
    apply_rotation_to_page(&mut doc, 1, 180, false);
    assert_eq!(page_rotation(&doc, page), 0);
    assert!(!doc.get_dictionary(page).unwrap().has(b"Rotate"));
    apply_rotation_to_page(&mut doc, 1, 270, false);
    apply_rotation_to_page(&mut doc, 1, 180, false);
    assert_eq!(page_rotation(&doc, page), 90);

    // inherited attributes are found through /Parent on documents that were not normalised
    let raw = Document::load(fixture("refs.pdf")).unwrap();
    for id in page_ids(&raw) {
        assert!(page_attr(&raw, id, b"MediaBox").is_some());
        assert!(matches!(page_attr(&raw, id, b"Resources"), Some(Object::Dictionary(_))));
        assert_eq!(page_attr(&raw, id, b"NoSuchKey"), None);
    }

    // page markers
    add_marks_to_pages(&mut doc);
    assert_eq!(
        doc.get_dictionary(page).unwrap().get(PAGE_MARKER).unwrap().as_i64().unwrap(),
        1
    );
    remove_marks_from_pages(&mut doc);
    assert!(!doc.get_dictionary(page).unwrap().has(PAGE_MARKER));
}

// ReaderTest.mergerequest21 / mergerequest21_2
#[test]
fn reader_invalid_reference_on_kids() {
    for name in ["CVE-2007-0103_AcrobatReader", "CVE-2007-0103_AcrobatReader_mutated"] {
        match open(&fixture(name), "") {
            Err(OpenError::Invalid(message)) => {
                assert!(message.contains("Invalid reference on Kids"), "{}: {}", name, message)
            }
            Err(e) => panic!("{}: {:?}", name, e),
            Ok(_) => panic!("{} opened", name),
        }
    }
}

#[test]
fn reader_error_messages() {
    let no_header = open_pdf_bytes(b"hello\n", b"", "");
    assert!(matches!(no_header, Err(OpenError::Invalid(m)) if m == "PDF header signature not found."));
    let empty = open_pdf_bytes(b"", b"", "");
    assert!(matches!(empty, Err(OpenError::Invalid(m)) if m == "PDF header signature not found."));
    let header_only = open_pdf_bytes(b"%PDF-1.4\nfoo", b"", "");
    assert!(
        matches!(&header_only, Err(OpenError::Invalid(m)) if m.starts_with("Rebuild failed: trailer not found.")),
        "{:?}",
        header_only.err()
    );
    assert!(matches!(read_input_file("/nonexistent/x.pdf"), Err(OpenError::NotFound)));
    assert!(matches!(
        read_input_file(std::env::temp_dir().to_str().unwrap()),
        Err(OpenError::Unexpected(_))
    ));
}

/// Damaged files that Java pdftk still opens (it rebuilds the cross-reference table).
#[test]
fn reader_tolerates_damaged_xref() {
    let dir = tmp_dir("damaged");
    for name in ["blank.pdf", "refs.pdf", "issue18.pdf", "form.pdf"] {
        let good = std::fs::read(fixture(name)).unwrap();
        let expected = page_contents(&open(&fixture(name), "").unwrap().doc);

        // 1. a startxref that points nowhere useful
        let pos = good.windows(9).rposition(|w| w == b"startxref").unwrap();
        let mut bad_offset = good[..pos].to_vec();
        bad_offset.extend_from_slice(b"startxref\n7\n%%EOF\n");
        // 2. junk in front of the header, which shifts every offset
        let mut shifted = b"junk junk junk\n".to_vec();
        shifted.extend_from_slice(&good);

        for (label, bytes) in [("bad-offset", bad_offset), ("shifted", shifted)] {
            let path = dir.join(format!("{}-{}", label, name));
            std::fs::write(&path, &bytes).unwrap();
            let (java_ok, out, _) = java_pdftk(&[path.to_str().unwrap(), "dump_data"]);
            let ours = open_pdf_bytes(&bytes, b"", "");
            if java_ok {
                let java_pages: usize = out
                    .lines()
                    .find_map(|l| l.strip_prefix("NumberOfPages: "))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                let ours = ours.unwrap_or_else(|e| panic!("{} {}: {:?}", label, name, e));
                assert_eq!(page_ids(&ours.doc).len(), java_pages, "{} {}", label, name);
                assert_eq!(page_contents(&ours.doc), expected, "{} {}", label, name);
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

// CryptoTest.no_password_fails / wrong_password_fails, for every strength Java can write,
// plus owner-vs-user detection.
#[test]
fn reads_what_java_encrypts() {
    let dir = tmp_dir("java-enc");
    let plain = fixture("refs.pdf");
    let expected = page_contents(&open(&plain, "").unwrap().doc);
    assert_eq!(expected.len(), 8);
    let mut cases = 0;
    for strength in ["encrypt_40bit", "encrypt_128bit", "encrypt_aes128", ""] {
        for (user, owner) in [("correcthorsebatterystaple", ""), ("", "0wner"), ("us3r", "0wner")] {
            let path = dir.join(format!("{}-{}-{}.pdf", strength, user, owner));
            let mut args = vec![plain.to_str().unwrap(), "output", path.to_str().unwrap()];
            if !strength.is_empty() {
                args.push(strength);
            }
            if !user.is_empty() {
                args.extend(["user_pw", user]);
            }
            if !owner.is_empty() {
                args.extend(["owner_pw", owner]);
            }
            let (ok, _, err) = java_pdftk(&args);
            assert!(ok, "{}", err);
            let label = format!("{} user={:?} owner={:?}", strength, user, owner);

            // the user password
            match open(&path, user) {
                Ok(o) => {
                    assert!(o.encrypted, "{}", label);
                    // with only a user password, iText uses it as the owner password too
                    assert_eq!(o.owner_password_used, owner.is_empty(), "{}", label);
                    assert_eq!(page_contents(&o.doc), expected, "{}", label);
                }
                Err(e) => panic!("{}: {:?}", label, e),
            }
            // the owner password
            if !owner.is_empty() {
                let o = open(&path, owner).unwrap_or_else(|e| panic!("{}: {:?}", label, e));
                assert!(o.encrypted && o.owner_password_used, "{}", label);
                assert_eq!(page_contents(&o.doc), expected, "{}", label);
            }
            // a wrong password fails, even if the user password is empty (as in Java)
            assert!(
                matches!(open(&path, "Tr0ub4dor&3"), Err(OpenError::BadPassword)),
                "{}",
                label
            );
            // no password
            if !user.is_empty() {
                assert!(matches!(open(&path, ""), Err(OpenError::BadPassword)), "{}", label);
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 12);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reads_non_ascii_passwords() {
    let dir = tmp_dir("pw-latin");
    let path = dir.join("enc.pdf");
    let pw = "p\u{e4}ssw\u{f6}rd";
    let (ok, _, err) = java_pdftk(&[
        fixture("blank.pdf").to_str().unwrap(),
        "output",
        path.to_str().unwrap(),
        "user_pw",
        pw,
        "encrypt_128bit",
    ]);
    assert!(ok, "{}", err);
    let o = open(&path, pw).unwrap();
    assert!(o.encrypted);
    assert_eq!(page_ids(&o.doc).len(), 1);
    assert!(matches!(open(&path, "password"), Err(OpenError::BadPassword)));
    let _ = std::fs::remove_dir_all(dir);
}

/// AES-256 (R6) input, which Java pdftk cannot read at all ("unknown.encryption.type.r").
#[test]
fn reads_aes256() {
    if !have("qpdf") {
        return;
    }
    let dir = tmp_dir("aes256");
    let path = dir.join("aes256.pdf");
    let plain = fixture("refs.pdf");
    let (ok, _, err) = run(
        "qpdf",
        &["--encrypt", "us\u{e9}r", "owner", "256", "--", plain.to_str().unwrap(), path.to_str().unwrap()],
    );
    assert!(ok, "{}", err);
    let expected = page_contents(&open(&plain, "").unwrap().doc);
    let user = open(&path, "us\u{e9}r").unwrap();
    assert!(user.encrypted && !user.owner_password_used);
    assert_eq!(page_contents(&user.doc), expected);
    let owner = open(&path, "owner").unwrap();
    assert!(owner.encrypted && owner.owner_password_used);
    assert_eq!(page_contents(&owner.doc), expected);
    assert!(matches!(open(&path, "nope"), Err(OpenError::BadPassword)));
    assert!(matches!(open(&path, ""), Err(OpenError::BadPassword)));
    let _ = std::fs::remove_dir_all(dir);
}

// CryptoTest.set_password / idempotent_aes / idempotent_rc4: what we encrypt, qpdf and
// Java pdftk must be able to open, and we must read back unchanged.
#[test]
fn writes_what_java_and_qpdf_decrypt() {
    let dir = tmp_dir("our-enc");
    let plain = fixture("refs.pdf");
    let expected = page_contents(&open(&plain, "").unwrap().doc);
    let perms = 4 + 2048; // "allow printing"
    let mut cases = 0;
    for (strength, r) in [
        (EncryptionStrength::Rc4_40, "R = 2"),
        (EncryptionStrength::Rc4_128, "R = 3"),
        (EncryptionStrength::Aes128, "R = 4"),
        (EncryptionStrength::None, "R = 4"), // a password but no keyword: AES-128
    ] {
        for (user, owner) in [("correcthorsebatterystaple", ""), ("", "\"'**"), ("**\"'", "f\u{f6}o")] {
            let mut s = session(Keyword::filter_k);
            s.m_output_encryption_strength = strength;
            s.m_output_user_pw = user.to_string();
            s.m_output_owner_pw = owner.to_string();
            s.m_output_user_pw_pdfdoc = unicode_to_text_string(user);
            s.m_output_owner_pw_pdfdoc = unicode_to_text_string(owner);
            s.m_output_user_perms = perms;
            let label = format!("{:?} user={:?} owner={:?}", strength, user, owner);

            let mut doc = open(&plain, "").unwrap().doc;
            let original_id = doc.trailer.get(b"ID").unwrap().clone();
            let path = dir.join(format!("enc-{}.pdf", cases));
            write(&s, &mut doc, &path);
            let p = path.to_str().unwrap();

            // qpdf: structure, encryption parameters, permissions
            qpdf_check(&path, Some(user));
            let (ok, show, err) = run("qpdf", &["--show-encryption", &format!("--password={}", user), p]);
            assert!(ok, "{}: {}", label, err);
            assert!(show.contains(r), "{}: {}", label, show);
            if r == "R = 4" {
                assert!(show.contains("stream encryption method: AESv2"), "{}: {}", label, show);
                assert!(show.contains("string encryption method: AESv2"), "{}: {}", label, show);
            }
            assert!(show.contains("print low resolution: allowed"), "{}: {}", label, show);
            assert!(show.contains("modify anything: not allowed"), "{}: {}", label, show);
            assert!(show.contains("extract for any purpose: not allowed"), "{}: {}", label, show);
            // the same /P that iText computes
            let p_expected = if strength == EncryptionStrength::Rc4_40 { "P = -60" } else { "P = -1852" };
            assert!(show.contains(p_expected), "{}: {}", label, show);

            // qpdf decrypts to the same page content
            let dec = dir.join(format!("dec-{}.pdf", cases));
            let (ok, _, err) = run(
                "qpdf",
                &["--decrypt", &format!("--password={}", user), p, dec.to_str().unwrap()],
            );
            assert!(ok, "{}: {}", label, err);
            assert_eq!(page_contents(&open(&dec, "").unwrap().doc), expected, "{}", label);

            // Java pdftk opens it with either password, and not without / with a wrong one
            for pw in [user, owner] {
                if pw.is_empty() && !user.is_empty() {
                    continue;
                }
                let mut args = vec![p];
                if !pw.is_empty() {
                    args.extend(["input_pw", pw]);
                }
                args.push("dump_data");
                let (ok, out, err) = java_pdftk(&args);
                assert!(ok, "{} pw={:?}: {}", label, pw, err);
                assert!(out.contains("NumberOfPages: 8"), "{}", label);
            }
            let (ok, _, err) = java_pdftk(&[p, "input_pw", "Tr0ub4dor&3", "dump_data"]);
            assert!(!ok && err.contains("OWNER OR USER PASSWORD REQUIRED, but incorrect"), "{}: {}", label, err);
            if !user.is_empty() {
                let (ok, _, err) = java_pdftk(&[p, "dump_data"]);
                assert!(!ok && err.contains("OWNER OR USER PASSWORD REQUIRED, but not given"), "{}: {}", label, err);
            }

            // and we read our own output back (idempotent_aes / idempotent_rc4)
            let back = open(&path, user).unwrap_or_else(|e| panic!("{}: {:?}", label, e));
            assert!(back.encrypted, "{}", label);
            assert_eq!(page_contents(&back.doc), expected, "{}", label);
            if !owner.is_empty() {
                let back = open(&path, owner).unwrap();
                assert!(back.owner_password_used, "{}", label);
            }
            // encrypting always renews the file ID
            assert_ne!(back.doc.trailer.get(b"ID").unwrap(), &original_id, "{}", label);
            cases += 1;
        }
    }
    assert_eq!(cases, 12);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn plain_compress_and_uncompress_output() {
    let dir = tmp_dir("filter");
    let mut written = 0;
    for name in PDF_FIXTURES {
        let expected = page_contents(&open(&fixture(name), "").unwrap().doc);
        for mode in ["plain", "compress", "uncompress"] {
            let mut s = session(Keyword::filter_k);
            s.m_output_compress_b = mode == "compress";
            s.m_output_uncompress_b = mode == "uncompress";
            let mut doc = open(&fixture(name), "").unwrap().doc;
            if mode == "uncompress" {
                add_marks_to_pages(&mut doc);
            }
            let id_before = doc.trailer.get(b"ID").ok().cloned();
            let info_before = doc.trailer.get(b"Info").ok().and_then(|o| resolve(&doc, o)).cloned();
            let version_before = doc.version.clone();
            let path = dir.join(format!("{}-{}", mode, name));
            write(&s, &mut doc, &path);
            qpdf_check(&path, None);
            written += 1;

            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.starts_with(format!("%PDF-{}", version_before).as_bytes()), "{} {}", mode, name);
            let back = open(&path, "").unwrap().doc;
            assert_eq!(page_contents(&back), expected, "{} {}", mode, name);
            // "stamper" mode: the file ID and the Info dictionary are the input's
            assert_eq!(back.trailer.get(b"ID").ok().cloned(), id_before, "{} {}", mode, name);
            let info_after = back.trailer.get(b"Info").ok().and_then(|o| resolve(&back, o)).cloned();
            assert_eq!(info_after, info_before, "{} {}", mode, name);

            let raw = Document::load(&path).unwrap();
            for id in page_ids(&raw) {
                for content in raw.get_page_contents(id) {
                    let stream = raw.get_object(content).unwrap().as_stream().unwrap();
                    match mode {
                        "uncompress" => assert!(!stream.dict.has(b"Filter"), "{} {}", mode, name),
                        "compress" => {
                            // small streams are left alone when deflating would not shrink them
                            if stream.content.len() > 200 {
                                assert!(stream.dict.has(b"Filter"), "{} {}", mode, name)
                            }
                        }
                        _ => {}
                    }
                }
                let page = raw.get_dictionary(id).unwrap();
                assert_eq!(page.has(PAGE_MARKER), mode == "uncompress", "{} {}", mode, name);
            }
            // Java pdftk agrees it is the same document
            let (ok, out, err) = java_pdftk(&[path.to_str().unwrap(), "dump_data"]);
            assert!(ok, "{} {}: {}", mode, name, err);
            assert!(out.contains(&format!("NumberOfPages: {}", expected.len())), "{} {}", mode, name);

            // compress undoes uncompress, markers included
            if mode == "uncompress" {
                let mut s = session(Keyword::filter_k);
                s.m_output_compress_b = true;
                let mut doc = open(&path, "").unwrap().doc;
                let again = dir.join(format!("recompress-{}", name));
                write(&s, &mut doc, &again);
                qpdf_check(&again, None);
                written += 1;
                let raw = Document::load(&again).unwrap();
                for id in page_ids(&raw) {
                    assert!(!raw.get_dictionary(id).unwrap().has(PAGE_MARKER), "{}", name);
                }
                assert_eq!(page_contents(&raw), expected, "recompress {}", name);
            }
        }
    }
    assert_eq!(written, PDF_FIXTURES.len() * 4);
    let _ = std::fs::remove_dir_all(dir);
}

/// uncompress output is meant for a text editor: one dictionary entry per line, readable
/// content streams.
#[test]
fn uncompress_is_text_editable() {
    let mut s = session(Keyword::filter_k);
    s.m_output_uncompress_b = true;
    let mut doc = open(&fixture("blank.pdf"), "").unwrap().doc;
    add_marks_to_pages(&mut doc);
    let mut out = Vec::new();
    assert_eq!(write_output(&s, &mut doc, &mut out), ErrorCode::NoError);
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/pdftk_PageNum 1\n"), "{}", &text[..600.min(text.len())]);
    assert!(text.contains("BT\n/F8 9.9626 Tf 303.133 139.255 Td [(1)]TJ\nET"));
    assert!(text.lines().any(|l| l.trim() == "/Type /Page"));
}

#[test]
fn copy_mode_info_and_id() {
    let dir = tmp_dir("copy");
    let src = open(&fixture("refs.pdf"), "").unwrap().doc;
    let src_id = src.trailer.get(b"ID").unwrap().clone();

    let build = || -> Document {
        // a two-page document assembled with copy_object_graph, the way cat does
        let mut dst = Document::with_version("1.5");
        let pages_id = dst.new_object_id();
        let mut map = HashMap::new();
        let ids = page_ids(&src);
        let mut kids = Vec::new();
        for id in [ids[2], ids[0]] {
            let new = copy_object_graph(&src, &mut dst, id, &mut map);
            dst.get_dictionary_mut(new).unwrap().set("Parent", Object::Reference(pages_id));
            kids.push(Object::Reference(new));
        }
        // copying a page twice yields the same object
        assert_eq!(copy_object_graph(&src, &mut dst, ids[0], &mut map), kids[1].as_reference().unwrap());
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Count", Object::Integer(2));
        pages.set("Kids", Object::Array(kids));
        dst.objects.insert(pages_id, Object::Dictionary(pages));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));
        let catalog_id = dst.add_object(Object::Dictionary(catalog));
        dst.trailer.set("Root", Object::Reference(catalog_id));
        dst
    };

    // default: new ID, pdftk Creator/Producer and dates
    let mut doc = build();
    doc.trailer.set("ID", src_id.clone());
    let path = dir.join("cat.pdf");
    write(&session(Keyword::cat_k), &mut doc, &path);
    qpdf_check(&path, None);
    let back = open(&path, "").unwrap().doc;
    let expected = page_contents(&src);
    assert_eq!(page_contents(&back), vec![expected[2].clone(), expected[0].clone()]);
    let id = back.trailer.get(b"ID").unwrap().as_array().unwrap();
    assert_eq!(id.len(), 2);
    assert_ne!(back.trailer.get(b"ID").unwrap(), &src_id);
    assert_ne!(id[0], id[1]);
    let info = back.trailer.get(b"Info").ok().and_then(|o| resolve(&back, o)).unwrap().as_dict().unwrap();
    assert_eq!(info.get(b"Creator").unwrap().as_str().unwrap(), TkSession::creator().as_bytes());
    assert!(info.has(b"Producer") && info.has(b"CreationDate") && info.has(b"ModDate"));
    let (ok, out, err) = java_pdftk(&[path.to_str().unwrap(), "dump_data"]);
    assert!(ok, "{}", err);
    assert!(out.contains("NumberOfPages: 2"));

    // keep_first_id / keep_final_id: the ID the caller put into the trailer survives
    let mut s = session(Keyword::cat_k);
    s.m_output_keep_first_id_b = true;
    let mut doc = build();
    doc.trailer.set("ID", src_id.clone());
    let path = dir.join("cat-keep.pdf");
    write(&s, &mut doc, &path);
    qpdf_check(&path, None);
    assert_eq!(open(&path, "").unwrap().doc.trailer.get(b"ID").unwrap(), &src_id);

    // ... but not when encrypting (iText's encryption supplies its own ID); and cat raises
    // the header version to what the encryption needs
    s.m_output_user_pw = "x".into();
    s.m_output_user_pw_pdfdoc = b"x".to_vec();
    let mut doc = build();
    doc.trailer.set("ID", src_id.clone());
    let path = dir.join("cat-keep-enc.pdf");
    write(&s, &mut doc, &path);
    qpdf_check(&path, Some("x"));
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF-1.6"));
    assert_ne!(open(&path, "x").unwrap().doc.trailer.get(b"ID").unwrap(), &src_id);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn named_destinations_are_consolidated() {
    // refs.pdf (pdfTeX + hyperref) uses named destinations for links and bookmarks
    let bytes = std::fs::read(fixture("refs.pdf")).unwrap();
    let mut doc = open_pdf_bytes(&bytes, b"", "").unwrap().doc;
    let names = named_destinations(&doc);
    assert!(names.len() > 50, "{}", names.len());
    let count_named = |doc: &Document| -> (usize, usize) {
        let mut named = 0;
        let mut explicit = 0;
        for obj in doc.objects.values() {
            if let Object::Dictionary(d) = obj {
                let targets = [
                    d.get(b"Dest").ok(),
                    d.get(b"A").ok().and_then(|a| resolve(doc, a)).and_then(|a| a.as_dict().ok()).and_then(|a| a.get(b"D").ok()),
                ];
                for t in targets.into_iter().flatten() {
                    match resolve(doc, t) {
                        Some(Object::String(..)) | Some(Object::Name(_)) => named += 1,
                        Some(Object::Array(_)) => explicit += 1,
                        _ => {}
                    }
                }
            }
        }
        (named, explicit)
    };
    let (named_before, explicit_before) = count_named(&doc);
    assert!(named_before > 50 && explicit_before == 0, "{} {}", named_before, explicit_before);
    consolidate_named_destinations(&mut doc);
    remove_unused_objects(&mut doc);
    let (named_after, explicit_after) = count_named(&doc);
    assert_eq!(named_after, 0);
    assert_eq!(explicit_after, named_before);
}
