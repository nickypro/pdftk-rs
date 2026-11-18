//! Tests for the forms code: ports of FormTest.java / DataFieldsTest.java against the upstream
//! fixtures, plus comparisons against the installed Java pdftk (skipped when it is absent) on
//! the upstream fixtures and on the synthetic forms in tests/forms/ (see the gen_*.py there).
//!
//! The installed oracle is pdftk-java 3.3.2 while the port follows the 3.3.3 sources, which
//! differ in three places exercised here: DocumentFont drops characters the font cannot encode
//! (3.3.2 writes a NUL byte each), DocumentFont understands Identity-H fonts with a /ToUnicode
//! map (3.3.2 does not), and FdfReader honours /Encoding /utf_8 (3.3.2 does not). Those cases
//! are checked against expectations derived from the Java source instead of the oracle.
use super::util::*;
use super::*;
use crate::pdf;
use lopdf::{Dictionary, Document, Object};
use std::path::{Path, PathBuf};
use std::process::Command;

const ORACLE: &str = "/usr/bin/pdftk";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn upstream(name: &str) -> PathBuf {
    root().join("tests/files").join(name)
}
fn synth(name: &str) -> PathBuf {
    root().join("tests/forms").join(name)
}
fn load(path: &Path) -> Document {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
    match pdf::open_pdf_bytes(&bytes, b"", "") {
        Ok(o) => o.doc,
        Err(_) => panic!("cannot open {}", path.display()),
    }
}
fn have(tool: &str) -> bool {
    Path::new(tool).exists() || Command::new(tool).arg("--version").output().is_ok()
}
fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdftk-rs-forms-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn oracle(args: &[&str]) -> (Vec<u8>, i32) {
    let out = Command::new(ORACLE).args(args).output().expect("run pdftk");
    (out.stdout, out.status.code().unwrap_or(-1))
}
fn dump(doc: &Document, utf8: bool) -> Vec<u8> {
    let mut out = Vec::new();
    report_acro_form_fields(&mut out, doc, utf8).unwrap();
    out
}
fn fdf(doc: &Document) -> Vec<u8> {
    let mut out = Vec::new();
    generate_fdf(&mut out, doc).unwrap();
    out
}

// ---------------------------------------------------------------------------------------------
// FormTest.java / DataFieldsTest.java

#[test]
fn dump_data_fields() {
    assert_eq!(dump(&load(&upstream("form.pdf")), false), std::fs::read(upstream("form.data")).unwrap());
}

#[test]
fn generate_fdf_form() {
    assert_eq!(fdf(&load(&upstream("form.pdf"))), std::fs::read(upstream("form.fdf")).unwrap());
}

#[test]
fn generate_fdf_issue88() {
    assert_eq!(fdf(&load(&upstream("issue88.pdf"))), std::fs::read(upstream("issue88.fdf")).unwrap());
}

#[test]
fn dump_data_fields_utf8_options() {
    assert_eq!(
        dump(&load(&upstream("form-pdfdocencoding.pdf")), true),
        std::fs::read(upstream("form-pdfdocencoding.data")).unwrap()
    );
}

#[test]
fn generate_fdf_utf8_options() {
    assert_eq!(
        fdf(&load(&upstream("form-pdfdocencoding.pdf"))),
        std::fs::read(upstream("form-pdfdocencoding.fdf")).unwrap()
    );
}

#[test]
fn ignore_field_with_successor_names() {
    assert_eq!(dump(&load(&upstream("issue19.pdf")), false), std::fs::read(upstream("issue19.data")).unwrap());
}

#[test]
fn escape_unicode() {
    assert_eq!(dump(&load(&upstream("issue21.pdf")), false), std::fs::read(upstream("issue21.data")).unwrap());
}

/// Every PDF we have: dump_data_fields, dump_data_fields_utf8 and generate_fdf byte for byte.
#[test]
fn text_outputs_match_oracle() {
    if !have(ORACLE) {
        eprintln!("skipping: no {}", ORACLE);
        return;
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in [root().join("tests/files"), root().join("tests/forms")] {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().map(|x| x == "pdf").unwrap_or(false) {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut compared = 0;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let doc = match pdf::open_pdf_bytes(&bytes, b"", "") {
            Ok(o) => o.doc,
            Err(_) => continue, // encrypted / deliberately broken fixtures
        };
        let fs = f.to_str().unwrap();
        for (op, utf8) in [("dump_data_fields", false), ("dump_data_fields_utf8", true)] {
            let (exp, rc) = oracle(&[fs, op]);
            if rc != 0 {
                continue;
            }
            assert_eq!(dump(&doc, utf8), exp, "{} {}", op, fs);
            compared += 1;
        }
        let (exp, rc) = oracle(&[fs, "generate_fdf", "output", "-"]);
        if rc == 0 {
            assert_eq!(fdf(&doc), exp, "generate_fdf {}", fs);
            compared += 1;
        }
    }
    assert!(compared >= 30, "only {} comparisons ran", compared);
}

// ---------------------------------------------------------------------------------------------
// form data readers

fn value<'a>(d: &'a FormData, name: &str) -> Option<&'a str> {
    d.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[test]
fn read_fdf_fixture() {
    let d = read_form_data(&std::fs::read(upstream("form-filled.fdf")).unwrap()).unwrap();
    assert_eq!(d.fields.len(), 3);
    assert_eq!(value(&d, "CheckBox1"), Some("On"));
    assert_eq!(value(&d, "Radio1"), Some("2"));
    assert_eq!(value(&d, "TextBox1"), Some("It was the best of times, it was the worst of times"));
    assert!(!d.has_rich_text);
    // a generated FDF reads back
    let d = read_form_data(&std::fs::read(upstream("issue88.fdf")).unwrap()).unwrap();
    assert_eq!(d.names.len(), 155); // one per "/V" line of the file
}

#[test]
fn read_fdf_hierarchy_rich_text_and_arrays() {
    let d = read_form_data(&std::fs::read(synth("edge1.fdf")).unwrap()).unwrap();
    assert_eq!(value(&d, "parent.child1"), Some("kid one"));
    assert_eq!(value(&d, "parent.child2"), Some("NameVal"));
    assert_eq!(value(&d, "p2.text"), Some("dotted"));
    assert_eq!(value(&d, "text_multi"), Some("a\n\nb\n"));
    assert_eq!(value(&d, "text_times"), Some("\u{20ac} euro \u{153}"));
    assert_eq!(value(&d, "list_multi"), None);
    assert!(d.has_rich_text);
    assert_eq!(d.rich_values, vec![("parent.child1".to_string(), "<p>rich</p>".to_string())]);
    let multi = d.multi_values.iter().find(|(k, _)| k == "list_multi").unwrap();
    assert_eq!(multi.1, vec!["1", "2", "3"]);
    // a single value doubles as a one-element selection
    let single = d.multi_values.iter().find(|(k, _)| k == "list").unwrap();
    assert_eq!(single.1, vec!["zzz"]);
}

#[test]
fn read_fdf_encoding_and_indirect_values() {
    let d = read_form_data(&std::fs::read(synth("u8.fdf")).unwrap()).unwrap();
    assert_eq!(value(&d, "text_left"), Some("utf8 caf\u{e9} \u{20ac}"));
    assert_eq!(value(&d, "text_center"), Some("indirect value"));
}

#[test]
fn read_fdf_rejects_what_itext_rejects() {
    assert!(super::fdfread::read_fdf(b"garbage").is_err());
    assert!(super::fdfread::read_fdf(b"%FDF-1.4\n1 0 obj\n<< /FDF << /Fields [] >> >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n").is_err());
    assert!(super::fdfread::read_fdf(b"%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [] >> >>\nendobj\n").is_err());
    assert!(super::fdfread::read_fdf(b"%FDF-1.2\n1 0 obj\n<< /NotFDF 1 >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n").is_err());
    assert!(read_form_data(b"garbage").is_none());
}

#[test]
fn read_xfdf() {
    let d = read_form_data(&std::fs::read(synth("x1.xfdf")).unwrap()).unwrap();
    assert_eq!(value(&d, "text_left"), Some("Xfdf & entities <> \u{e9} \u{e9} \u{20ac} &bogus; caf\u{e9}"));
    assert_eq!(value(&d, "text_center"), Some("cdata <b> & stuff"));
    assert_eq!(value(&d, "parent.child1"), Some("nested one"));
    assert_eq!(value(&d, "parent.child2"), Some("nested two"));
    assert_eq!(value(&d, "text_multi"), Some("line1\nline2\n"));
    assert_eq!(value(&d, "list_multi"), Some("4"));
    assert_eq!(d.multi_values, vec![("list_multi".to_string(), vec!["1".to_string(), "4".to_string()])]);
    assert!(d.has_rich_text);
    assert_eq!(d.rich_values[0].0, "parent.child2");
    // declared ISO-8859-1
    let d = read_form_data(&std::fs::read(synth("x3.xfdf")).unwrap()).unwrap();
    assert_eq!(value(&d, "text_left"), Some("latin \u{e9}\u{e8}"));
    // SimpleXMLParser swallows everything up to the first '>' as the XML declaration, so a file
    // without one loses its root tag and is rejected ("Root element is not ...")
    assert!(read_form_data(b"<xfdf><fields><field name=\"a\"><value>v</value></field></fields></xfdf>").is_none());
    assert!(read_form_data(b"<?xml version=\"1.0\"?><other><fields/></other>").is_none());
}

// ---------------------------------------------------------------------------------------------
// fill_form / flatten against the oracle

fn fmt_obj(doc: &Document, o: &Object, depth: usize) -> String {
    if depth > 6 {
        return "...".into();
    }
    match o {
        Object::Reference(_) => match pdf::resolve(doc, o) {
            Some(Object::Stream(_)) => "STREAM".into(),
            Some(r) => fmt_obj(doc, r, depth + 1),
            None => "null".into(),
        },
        Object::Integer(i) => format!("{}", i),
        Object::Real(r) => {
            let v = (*r as f64 * 100.0).round() / 100.0;
            if v == v.trunc() {
                format!("{}", v as i64)
            } else {
                format!("{}", v)
            }
        }
        Object::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
        Object::String(s, _) => format!("({})", pdf::text_string_to_unicode(s).escape_debug()),
        Object::Array(a) => format!("[{}]", a.iter().map(|x| fmt_obj(doc, x, depth + 1)).collect::<Vec<_>>().join(" ")),
        Object::Dictionary(d) => {
            let mut keys: Vec<&Vec<u8>> = d.iter().map(|(k, _)| k).collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("/{} {}", String::from_utf8_lossy(k), fmt_obj(doc, d.get(k).unwrap(), depth + 1)))
                .collect();
            format!("<<{}>>", parts.join(" "))
        }
        Object::Boolean(b) => format!("{}", b),
        Object::Null => "null".into(),
        Object::Stream(_) => "STREAM".into(),
    }
}

fn stream_desc(doc: &Document, o: &Object) -> String {
    match pdf::resolve(doc, o) {
        Some(Object::Stream(s)) => {
            let data = s.decompressed_content().unwrap_or_else(|_| s.content.clone());
            let mut fonts = String::new();
            if let Some(Object::Dictionary(r)) = s.dict.get(b"Resources").ok().and_then(|r| pdf::resolve(doc, r)) {
                if let Some(Object::Dictionary(f)) = r.get(b"Font").ok().and_then(|f| pdf::resolve(doc, f)) {
                    let mut names: Vec<String> = f
                        .iter()
                        .map(|(k, v)| {
                            let base = match pdf::resolve(doc, v) {
                                Some(Object::Dictionary(fd)) => fd.get(b"BaseFont").map(|b| fmt_obj(doc, b, 0)).unwrap_or_default(),
                                _ => String::new(),
                            };
                            format!("{}={}", String::from_utf8_lossy(k), base)
                        })
                        .collect();
                    names.sort();
                    fonts = names.join(",");
                }
            }
            let g = |k: &[u8]| s.dict.get(k).map(|v| fmt_obj(doc, v, 0)).unwrap_or_else(|_| "-".into());
            format!(
                "BBox={} Matrix={} Subtype={} Fonts={{{}}} {:?}",
                g(b"BBox"),
                g(b"Matrix"),
                g(b"Subtype"),
                fonts,
                String::from_utf8_lossy(&data)
            )
        }
        _ => "NOSTREAM".into(),
    }
}

/// A normalised description of everything fill_form/flatten touch: the AcroForm, every field
/// with its values and normal appearances, and every page's annotations, content and XObjects.
fn form_dump(doc: &Document) -> String {
    let mut lines: Vec<String> = Vec::new();
    fn walk(doc: &Document, o: &Object, prefix: &str, depth: usize, lines: &mut Vec<String>) {
        let f: &Dictionary = match pdf::resolve(doc, o) {
            Some(Object::Dictionary(d)) if depth < 30 => d,
            _ => return,
        };
        let mut name = prefix.to_string();
        if let Ok(Object::String(t, _)) = f.get(b"T") {
            if !name.is_empty() {
                name.push('.');
            }
            name.push_str(&pdf::text_string_to_unicode(t));
        }
        let mut info = String::new();
        for k in ["FT", "V", "DV", "AS", "I", "TI", "Ff", "RV", "F", "Rect", "Q", "MaxLen"] {
            if let Ok(v) = f.get(k.as_bytes()) {
                info.push_str(&format!(" /{} {}", k, fmt_obj(doc, v, 0)));
            }
        }
        lines.push(format!("FIELD {}{}", name, info));
        if let Some(Object::Dictionary(ap)) = f.get(b"AP").ok().and_then(|a| pdf::resolve(doc, a)) {
            if let Ok(n) = ap.get(b"N") {
                match pdf::resolve(doc, n) {
                    Some(Object::Stream(_)) => lines.push(format!("  AP/N {}", stream_desc(doc, n))),
                    Some(Object::Dictionary(states)) => {
                        let mut keys: Vec<&Vec<u8>> = states.iter().map(|(k, _)| k).collect();
                        keys.sort();
                        for k in keys {
                            lines.push(format!(
                                "  AP/N/{} {}",
                                String::from_utf8_lossy(k),
                                stream_desc(doc, states.get(k).unwrap())
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(Object::Array(kids)) = f.get(b"Kids").ok().and_then(|k| pdf::resolve(doc, k)) {
            for k in kids {
                walk(doc, k, &name, depth + 1, lines);
            }
        }
    }
    match super::acro::acro_form_dict(doc) {
        None => lines.push("NO ACROFORM".into()),
        Some(acro) => {
            let mut keys: Vec<String> =
                acro.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).filter(|k| k != "Fields").collect();
            keys.sort();
            lines.push(format!(
                "ACROFORM {:?} NeedAppearances={}",
                keys,
                acro.get(b"NeedAppearances").map(|v| fmt_obj(doc, v, 0)).unwrap_or_default()
            ));
            if let Some(Object::Array(fields)) = acro.get(b"Fields").ok().and_then(|f| pdf::resolve(doc, f)) {
                for f in fields {
                    walk(doc, f, "", 0, &mut lines);
                }
            }
        }
    }
    for (i, page_id) in pdf::page_ids(doc).iter().enumerate() {
        let page = doc.get_dictionary(*page_id).unwrap();
        let mut subtypes = Vec::new();
        if let Some(Object::Array(annots)) = page.get(b"Annots").ok().and_then(|a| pdf::resolve(doc, a)) {
            for a in annots {
                if let Some(Object::Dictionary(d)) = pdf::resolve(doc, a) {
                    subtypes.push(d.get(b"Subtype").map(|s| fmt_obj(doc, s, 0)).unwrap_or_default());
                }
            }
        }
        lines.push(format!("PAGE {} annots={:?}", i + 1, subtypes));
        let mut content = Vec::new();
        let mut add = |o: &Object| {
            if let Some(Object::Stream(s)) = pdf::resolve(doc, o) {
                content.extend_from_slice(&s.decompressed_content().unwrap_or_else(|_| s.content.clone()));
                content.push(b'\n');
            }
        };
        match page.get(b"Contents").ok().and_then(|c| pdf::resolve(doc, c)) {
            Some(Object::Array(a)) => a.iter().for_each(&mut add),
            Some(Object::Stream(_)) => add(page.get(b"Contents").unwrap()),
            _ => {}
        }
        lines.push(format!("  CONTENT {:?}", String::from_utf8_lossy(&content)));
        if let Some(Object::Dictionary(r)) = pdf::page_attr(doc, *page_id, b"Resources") {
            if let Some(Object::Dictionary(xo)) = r.get(b"XObject").ok().and_then(|x| pdf::resolve(doc, x)) {
                let mut keys: Vec<&Vec<u8>> = xo.iter().map(|(k, _)| k).collect();
                keys.sort();
                for k in keys {
                    lines.push(format!("  XO {} {}", String::from_utf8_lossy(k), stream_desc(doc, xo.get(k).unwrap())));
                }
            }
        }
    }
    lines.join("\n")
}

struct Filled {
    ours: PathBuf,
    java: PathBuf,
    rich: bool,
}

/// Fill (and optionally flatten) `pdf` with `data` using our functions and using the oracle.
fn fill_both(tag: &str, pdf_path: &Path, data: Option<&Path>, flatten_b: bool) -> Filled {
    let dir = tmp(tag);
    let mut doc = load(pdf_path);
    let mut rich = false;
    if let Some(data) = data {
        let fd = read_form_data(&std::fs::read(data).unwrap()).expect("form data parses");
        assert!(has_acro_form(&doc));
        rich = fill_form(&mut doc, &fd, None);
        if rich {
            // what filter.java does when setFields() reports rich text
            super::fill::acro_form_mut(&mut doc).unwrap().set("NeedAppearances", Object::Boolean(true));
        }
    }
    if flatten_b {
        flatten(&mut doc);
    }
    let ours = dir.join("ours.pdf");
    doc.save(&ours).unwrap();
    let java = dir.join("java.pdf");
    let mut args: Vec<&str> = vec![pdf_path.to_str().unwrap()];
    if let Some(data) = data {
        args.push("fill_form");
        args.push(data.to_str().unwrap());
    }
    args.push("output");
    args.push(java.to_str().unwrap());
    if flatten_b {
        args.push("flatten");
    }
    let (_, rc) = oracle(&args);
    assert_eq!(rc, 0, "oracle failed: {:?}", args);
    Filled { ours, java, rich }
}

/// Our result and the oracle's must agree on fields, appearances, page content and XObjects
/// (`known` lists lines that legitimately differ: (oracle line fragment, our line fragment)),
/// on `pdftk dump_data_fields`, and on the text poppler extracts; and qpdf must accept ours.
fn assert_same(f: &Filled, known: &[(&str, &str)]) -> usize {
    let ours = form_dump(&load(&f.ours));
    let java = form_dump(&load(&f.java));
    let (ol, jl): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), java.lines().collect());
    assert_eq!(ol.len(), jl.len(), "line count\n--- ours\n{}\n--- java\n{}", ours, java);
    let mut exceptions = 0;
    for (o, j) in ol.iter().zip(jl.iter()) {
        if o != j {
            let ok = known.iter().any(|(jf, of)| j.contains(jf) && o.contains(of));
            assert!(ok, "difference\n  ours: {}\n  java: {}", o, j);
            exceptions += 1;
        }
    }
    let (a, _) = oracle(&[f.ours.to_str().unwrap(), "dump_data_fields_utf8"]);
    let (b, _) = oracle(&[f.java.to_str().unwrap(), "dump_data_fields_utf8"]);
    if exceptions == 0 {
        assert_eq!(String::from_utf8_lossy(&a), String::from_utf8_lossy(&b), "dump_data_fields of the outputs");
    }
    if have("qpdf") {
        let out = Command::new("qpdf").arg("--check").arg(&f.ours).output().unwrap();
        assert!(out.status.success(), "qpdf --check: {}", String::from_utf8_lossy(&out.stderr));
    }
    if have("pdftotext") && exceptions == 0 {
        let t = |p: &Path| Command::new("pdftotext").arg("-layout").arg(p).arg("-").output().unwrap().stdout;
        assert_eq!(String::from_utf8_lossy(&t(&f.ours)), String::from_utf8_lossy(&t(&f.java)), "pdftotext -layout");
    }
    ol.len()
}

macro_rules! oracle_or_skip {
    () => {
        if !have(ORACLE) {
            eprintln!("skipping: no {}", ORACLE);
            return;
        }
    };
}

/// FormTest.fill_from_fdf
#[test]
fn fill_from_fdf() {
    oracle_or_skip!();
    let f = fill_both("fill_from_fdf", &upstream("form.pdf"), Some(&upstream("form-filled.fdf")), false);
    assert!(!f.rich);
    assert_same(&f, &[]);
    let f = fill_both("fill_from_fdf_flat", &upstream("form.pdf"), Some(&upstream("form-filled.fdf")), true);
    assert_same(&f, &[]);
}

/// FormTest.fill_from_fdf_utf8_options
#[test]
fn fill_from_fdf_utf8_options() {
    oracle_or_skip!();
    let pdf_path = upstream("form-pdfdocencoding.pdf");
    let data = upstream("form-pdfdocencoding-filled.fdf");
    assert_same(&fill_both("fill_utf8", &pdf_path, Some(&data), false), &[]);
    assert_same(&fill_both("fill_utf8_flat", &pdf_path, Some(&data), true), &[]);
}

#[test]
fn fill_issue88_roundtrip() {
    oracle_or_skip!();
    let n = assert_same(&fill_both("i88", &upstream("issue88.pdf"), Some(&upstream("issue88.fdf")), false), &[]);
    assert!(n > 300, "{} lines", n);
    assert_same(&fill_both("i88_flat", &upstream("issue88.pdf"), Some(&upstream("issue88.fdf")), true), &[]);
}

#[test]
fn flatten_without_filling() {
    oracle_or_skip!();
    for name in ["form.pdf", "issue19.pdf", "issue21.pdf", "issue88.pdf", "form-pdfdocencoding.pdf"] {
        assert_same(&fill_both(&format!("flat_{}", name), &upstream(name), None, true), &[]);
    }
    assert_same(&fill_both("flat_kitchen", &synth("kitchen.pdf"), None, true), &[]);
    // shared indirect /Resources and /XObject dictionaries, rotated pages, direct annotations
    assert_same(&fill_both("flat_k4", &synth("k4.pdf"), None, true), &[]);
}

/// Every text-field layout variant: alignment, auto size, comb, password, multiline, borders,
/// /MK colours and rotation, choice fields, radio groups, hierarchical names, two widgets.
#[test]
fn fill_kitchen_sink() {
    oracle_or_skip!();
    // the one field whose value Helvetica cannot fully encode (see the module comment)
    let known = [("(Unicode \\0\\0 caf", "(Unicode  caf")];
    let f = fill_both("kitchen", &synth("kitchen.pdf"), Some(&synth("kitchen.fdf")), false);
    let n = assert_same(&f, &known);
    assert!(n > 100, "{} lines", n);
    assert_same(&fill_both("kitchen_flat", &synth("kitchen.pdf"), Some(&synth("kitchen.fdf")), true), &known);
}

/// Empty and blank values, blank lines, CR/LF variants, unbreakable words, over-long comb text,
/// values outside the option list, rich text, /Kids hierarchies in the FDF.
#[test]
fn fill_edge_cases() {
    oracle_or_skip!();
    let f = fill_both("edge", &synth("kitchen.pdf"), Some(&synth("edge1.fdf")), false);
    assert!(f.rich);
    assert_same(&f, &[]);
    assert_same(&fill_both("edge_flat", &synth("kitchen.pdf"), Some(&synth("edge1.fdf")), true), &[]);
}

#[test]
fn fill_from_xfdf() {
    oracle_or_skip!();
    let f = fill_both("xfdf", &synth("kitchen.pdf"), Some(&synth("x1.xfdf")), false);
    assert!(f.rich);
    assert_same(&f, &[]);
    assert_same(&fill_both("xfdf_latin1", &synth("kitchen.pdf"), Some(&synth("x3.xfdf")), true), &[]);
}

/// Direct widget dictionaries, an indirect and a shared /AP dictionary, a radio group whose /V
/// sits on a kid, a check box with /Opt, fields that are not on any page, rotated pages.
#[test]
fn fill_structural_cases() {
    oracle_or_skip!();
    assert_same(&fill_both("k3", &synth("k3.pdf"), Some(&synth("k3.fdf")), false), &[]);
    assert_same(&fill_both("k4", &synth("k4.pdf"), Some(&synth("k3.fdf")), false), &[]);
    // (3.3.2 crashes flattening k3.pdf: a field on "page -1"; 3.3.3 skips it, and so do we)
    let mut doc = load(&synth("k3.pdf"));
    let fd = read_form_data(&std::fs::read(synth("k3.fdf")).unwrap()).unwrap();
    fill_form(&mut doc, &fd, None);
    flatten(&mut doc);
    let d = form_dump(&doc);
    assert!(d.contains("PAGE 1 annots=[\"/Link\"]"), "{}", d);
    assert!(d.contains("-1 0 0 -1 400 300 cm"), "{}", d);
    assert!(d.contains("0 -1 1 0 0 520 cm"), "{}", d);
    assert!(!d.contains("FIELD"), "{}", d);
}

/// Fonts taken from the document: MacRoman TrueType with /Widths, a Type1 font without
/// /Encoding, /Differences, Symbol, a direct font dictionary; all as in 3.3.2 except for the
/// dropped-character difference and the Identity-H fonts.
#[test]
fn fill_document_fonts() {
    oracle_or_skip!();
    let known = [
        ("/ABCDEF+MyCID", "/ABCDEF+MyCID"),
        ("(Mac cafe\\0", "(Mac cafe"),
        ("(Std `quote' \\0 ", "(Std `quote'  "),
    ];
    let f = fill_both("fonts", &synth("kitchen2.pdf"), Some(&synth("k2.fdf")), false);
    assert_same(&f, &known);
    // Identity-H: CIDs from the /ToUnicode map, widths from /W and /DW, metrics from the
    // descendant's /FontDescriptor
    let d = form_dump(&load(&f.ours));
    let cid = d.lines().skip_while(|l| !l.starts_with("FIELD f_cid ")).nth(1).unwrap();
    assert!(
        cid.contains("1 0 0 1 2 5.16 Tm /ABCDEF+MyCID 11 Tf 0 g (\\0$\\0%\\0&\\0\\u{3}\\0D\\0E\\0F\\0\\u{3}\\0'\\0\\u{10}\\0\\u{11}\\0;)Tj"),
        "{}",
        cid
    );
    let auto = d.lines().skip_while(|l| !l.starts_with("FIELD f_cid_auto ")).nth(1).unwrap();
    assert!(auto.contains("/ABCDEF+MyCID 12 Tf"), "{}", auto);
    assert_eq!(auto.matches("Tj").count(), 2, "{}", auto);
}

#[test]
fn fill_utf8_fdf() {
    let mut doc = load(&synth("kitchen.pdf"));
    let fd = read_form_data(&std::fs::read(synth("u8.fdf")).unwrap()).unwrap();
    assert!(!fill_form(&mut doc, &fd, None));
    let d = form_dump(&doc);
    assert!(d.contains("FIELD text_left /FT /Tx /V (utf8 caf\u{e9} \u{20ac})"), "{}", d);
    assert!(d.contains("(indirect value)Tj"), "{}", d);
}

/// A choice field without /Opt: 3.3.2 dies in setListSelection(), 3.3.3 returns false.
#[test]
fn fill_editable_combo_without_options() {
    let mut doc = load(&synth("kitchen.pdf"));
    let fd = FormData {
        fields: vec![("combo_edit".into(), "free text".into())],
        multi_values: vec![("combo_edit".into(), vec!["free text".into()])],
        names: vec!["combo_edit".into()],
        ..Default::default()
    };
    fill_form(&mut doc, &fd, None);
    let d = form_dump(&doc);
    let i = d.lines().position(|l| l.starts_with("FIELD combo_edit ")).unwrap();
    let lines: Vec<&str> = d.lines().collect();
    assert!(lines[i].contains("/V (free text)"), "{}", lines[i]);
    assert!(lines[i + 1].contains("/Helv 10 Tf 0 g (free text)Tj"), "{}", lines[i + 1]);
}

#[test]
fn has_acro_form_reports_forms() {
    assert!(has_acro_form(&load(&upstream("form.pdf"))));
    assert!(!has_acro_form(&load(&upstream("blank.pdf"))));
    let mut doc = load(&upstream("blank.pdf"));
    assert!(!fill_form(&mut doc, &FormData::default(), None));
    flatten(&mut doc); // a no-op that must not disturb the file
    assert_eq!(pdf::page_ids(&doc).len(), 1);
}

// ---------------------------------------------------------------------------------------------
// small pieces

#[test]
fn itext_number_formatting() {
    for (v, s) in [
        (0.0, "0"),
        (0.00001, "0"),
        (1.0, "1"),
        (0.5, "0.5"),
        (0.18039, "0.18039"),
        (0.999999, "1"),
        (-0.25, "-0.25"),
        (5.686, "5.69"),
        (475.052, "475.05"),
        (-20000.0, "-20000"),
        (40000.4, "40000"),
        (12.0, "12"),
        (10.1, "10.1"),
    ] {
        assert_eq!(format_double(v), s, "{}", v);
    }
}

#[test]
fn java_hash_map_iteration_order() {
    // order verified against the oracle through generate_fdf (form.fdf)
    let mut m: JavaHashMap<i32> = JavaHashMap::new();
    for (i, k) in ["TextBox1", "CheckBox1", "Radio1"].iter().enumerate() {
        m.put(k, i as i32);
    }
    assert_eq!(m.keys(), vec!["CheckBox1", "Radio1", "TextBox1"]);
    assert_eq!(m.put("Radio1", 9), Some(2));
    assert_eq!(m.len(), 3);
    assert_eq!(java_string_hash("abc"), 96354);
    assert_eq!(java_string_hash("polygenelubricants"), i32::MIN);
    // growth keeps every key reachable
    for i in 0..500 {
        m.put(&format!("k{}", i), i);
    }
    assert_eq!(m.len(), 503);
    assert_eq!(m.get("k499"), Some(&499));
    assert_eq!(m.iter().count(), 503);
}

#[test]
fn default_appearance_parsing() {
    use super::appearance::{split_da_elements, Color};
    let (f, s, c, l) = split_da_elements("0.18039 0.20392 0.21176 rg /F2 10 Tf");
    assert_eq!(f.as_deref(), Some("F2"));
    assert_eq!(s, Some(10.0));
    assert_eq!(c, Some(Color::Rgb(46, 52, 54)));
    assert_eq!(l, None);
    let (f, s, c, l) = split_da_elements("/Helv 0 Tf 0 g 14 TL % comment");
    assert_eq!((f.as_deref(), s, c, l), (Some("Helv"), Some(0.0), None, Some(14.0)));
    let (_, _, c, _) = split_da_elements("0 0 0 1 k");
    assert_eq!(c, Some(Color::Cmyk(0.0, 0.0, 0.0, 1.0)));
}
