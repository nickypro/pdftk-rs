//! Shared PDF layer over lopdf: opening inputs (port of InputPdf.add_reader) and writing
//! output with pdftk output options (port of TK_Session.prepare_writer + the iText writer
//! behaviours pdftk relies on: compress/uncompress, encryption, /ID handling, Producer/Creator).
//! Owner: core agent. Other modules may only CALL these functions.
//!
//! This is a derivative of pdftk-java (Copyright (c) Marc Vinyals 2017-2023),
//! itself a port of PDFtk (Copyright (c) 2003-2013 Steward and Lee, LLC),
//! licensed under the GNU GPL version 2 or later.
#![allow(dead_code)]
use crate::keyword::Keyword;
use crate::session::*;
use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
use lopdf::encryption::PasswordAlgorithm;
use lopdf::xref::XrefType;
use lopdf::{
    Dictionary, Document, EncryptionState, EncryptionVersion, LoadOptions, Object, ObjectId,
    Permissions, SaveOptions, Stream, StringFormat,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;

/// The page marker pdftk adds to every page on `uncompress` (filter.java g_page_marker).
pub const PAGE_MARKER: &[u8] = b"pdftk_PageNum";

/// iText's default page size (PageSize.LETTER), used when a page has no /MediaBox.
const LETTER: [f32; 4] = [0.0, 0.0, 612.0, 792.0];

/// The attributes a page inherits from its ancestors (iText pageInhCandidates).
const PAGE_INH_CANDIDATES: [&[u8]; 4] = [b"MediaBox", b"Rotate", b"Resources", b"CropBox"];

////////////////////////////////////////////////////////////////////////////
// opening input PDFs

/// Why opening a PDF failed; mirrors the exception classes InputPdf.add_reader tells apart.
#[derive(Debug)]
pub enum OpenError {
    /// IOException("Bad password")
    BadPassword,
    /// "... not found as file or resource"
    NotFound,
    /// InvalidPdfException(message)
    Invalid(String),
    /// any other exception
    Unexpected(String),
}

enum Attempt {
    Opened(usize),
    /// Java "return null" without the closing report
    ReturnNull,
    Failed,
}

/// Port of InputPdf.add_reader(): open (and decrypt) the PDF, print pdftk-style errors/warnings
/// to stderr, set m_num_pages, push onto m_readers. Returns the new reader index.
pub fn add_reader(
    input: &mut InputPdf,
    keep_artifacts_b: bool,
    ask_about_warnings_b: bool,
) -> Option<usize> {
    let mut ask_about_warnings_b = ask_about_warnings_b;
    loop {
        // No point on asking for a password after we read all of stdin.
        if input.m_filename == "-" {
            ask_about_warnings_b = false;
        }

        let attempt = add_reader_attempt(input, keep_artifacts_b);
        let (open_success_b, pr) = match attempt {
            Attempt::ReturnNull => return None,
            Attempt::Opened(idx) => (true, Some(idx)),
            Attempt::Failed => (false, None),
        };

        if !input.m_authorized_b && ask_about_warnings_b {
            // prompt for a new password
            if input.m_password.is_empty() {
                eprintln!("The password for the {} PDF:", input.m_role);
                eprintln!("   {}", input.m_filename);
                eprintln!("   is missing.  This PDF is encrypted, and you must supply the");
                eprintln!("   owner or the user password to open it. To quit, enter a blank");
                eprintln!("   password at the next prompt.");
            } else {
                eprintln!("The password you supplied for the {} PDF:", input.m_role);
                eprintln!("   {}", input.m_filename);
                eprintln!("   did not work.  This PDF is encrypted, and you must supply the");
                eprintln!("   owner or the user password to open it. To quit, enter a blank");
                eprintln!("   password at the next prompt.");
            }

            input.m_password = crate::cli::prompt_for_password(
                "open",
                &format!("the {} PDF:\n   {}", input.m_role, input.m_filename),
            );
            if !input.m_password.is_empty() {
                // reset flags try again
                input.m_authorized_b = true;
                continue; // <--- recurse, return
            }
        }

        // report
        if !open_success_b {
            // file open error
            eprintln!("Error: Failed to open {} PDF file: ", input.m_role);
            eprintln!("   {}", input.m_filename);
            if !input.m_authorized_b {
                if input.m_password.is_empty() {
                    eprintln!("   OWNER OR USER PASSWORD REQUIRED, but not given");
                } else {
                    eprintln!("   OWNER OR USER PASSWORD REQUIRED, but incorrect");
                }
            }
        }

        return if open_success_b { pr } else { None };
    }
}

/// The body of the Java try block.
fn add_reader_attempt(input: &mut InputPdf, keep_artifacts_b: bool) -> Attempt {
    if input.m_filename == "PROMPT" {
        input.m_filename = crate::cli::prompt_for_filename(&format!(
            "Please enter a filename for {} PDF:",
            input.m_role_determined
        ));
    }

    let mut password: Vec<u8> = Vec::new();
    if !input.m_password.is_empty() {
        if input.m_password == "PROMPT" {
            input.m_password = crate::cli::prompt_for_password(
                "open",
                &format!("the {} PDF:\n   {}", input.m_role, input.m_filename),
            );
        }
        // allow user to enter greatest selection of chars
        match crate::passwords::utf8_password_to_pdfdoc(&input.m_password, false) {
            Some(pw) => password = pw,
            None => {
                // bad password
                eprintln!("Error: Password used to decrypt {} PDF:", input.m_role);
                eprintln!("   {}", input.m_filename);
                eprintln!("   includes invalid characters.");
                return Attempt::ReturnNull; // <--- return
            }
        }
    }

    let result = (|| -> Result<OpenedPdf, OpenError> {
        let opened = if input.m_filename == "-" {
            if input.m_buffer.is_none() {
                let mut buffer = Vec::new();
                std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut buffer)
                    .map_err(|e| OpenError::Unexpected(format!("java.io.IOException: {}", e)))?;
                input.m_buffer = Some(buffer);
            }
            open_pdf_bytes(
                input.m_buffer.as_deref().unwrap_or(&[]),
                &password,
                &input.m_password,
            )?
        } else {
            let buffer = read_input_file(&input.m_filename)?;
            open_pdf_bytes(&buffer, &password, &input.m_password)?
        };
        Ok(opened)
    })()
    .and_then(|mut opened| {
        if !keep_artifacts_b {
            // generally useful operations
            consolidate_named_destinations(&mut opened.doc);
            remove_unused_objects(&mut opened.doc);
        }
        Ok(opened)
    });

    match result {
        Ok(opened) => {
            input.m_num_pages = page_ids(&opened.doc).len() as u32;

            // keep tally of which pages have been laid claim to in this reader;
            // when creating the final PDF, this tally will be decremented
            input.m_readers.push(PagesReader {
                first: HashSet::new(),
                second: opened.doc,
                encrypted: opened.encrypted,
                owner_password_used: opened.owner_password_used,
            });

            input.m_authorized_b = true; // instead of: ( !reader->encrypted || reader->ownerPasswordUsed );

            if opened.encrypted && !opened.owner_password_used {
                eprintln!("WARNING: The creator of the {} PDF:", input.m_role);
                eprintln!("   {}", input.m_filename);
                eprintln!(
                    "   has set an owner password (which is not required to handle this PDF)."
                );
                eprintln!("   You did not supply this password. Please respect any copyright.");
            }
            Attempt::Opened(input.m_readers.len() - 1)
        }
        Err(OpenError::BadPassword) => {
            input.m_authorized_b = false;
            Attempt::Failed
        }
        Err(OpenError::NotFound) => {
            eprintln!("Error: Unable to find file.");
            Attempt::Failed
        }
        Err(OpenError::Invalid(message)) => {
            eprintln!("Error: Invalid PDF: {}", message);
            Attempt::Failed
        }
        Err(OpenError::Unexpected(message)) => {
            // unexpected error
            eprintln!("Error: Unexpected Exception in open_reader()");
            eprintln!("{}", message); // debug (Java prints a stack trace here)
            Attempt::Failed
        }
    }
}

fn read_input_file(filename: &str) -> Result<Vec<u8>, OpenError> {
    use std::io::ErrorKind;
    match std::fs::read(filename) {
        Ok(buffer) => Ok(buffer),
        // iText: "<name> not found as file or resource."
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) => {
            Err(OpenError::NotFound)
        }
        Err(e) => {
            let reason = if std::path::Path::new(filename).is_dir() {
                "Is a directory".to_string()
            } else {
                e.to_string()
            };
            Err(OpenError::Unexpected(format!(
                "java.io.FileNotFoundException: {} ({})",
                filename, reason
            )))
        }
    }
}

/// A successfully opened input: the decrypted, page-normalised document.
pub struct OpenedPdf {
    pub doc: Document,
    /// PdfReader.encrypted
    pub encrypted: bool,
    /// PdfReader.ownerPasswordUsed
    pub owner_password_used: bool,
}

/// Open a PDF held in memory, the way `new PdfReader(bytes, password)` does: parse (lopdf
/// repairs damaged cross-reference data by scanning), decrypt, read the page tree (pushing
/// inherited attributes down into every page, as iText does) and drop unused objects.
///
/// `password_pdfdoc` is the PDFDocEncoded password (empty for none); `password_utf8` is the
/// password as typed, used for AES-256 (R5/R6) documents, whose passwords are UTF-8.
pub fn open_pdf_bytes(
    buffer: &[u8],
    password_pdfdoc: &[u8],
    password_utf8: &str,
) -> Result<OpenedPdf, OpenError> {
    // PRTokeniser.checkPdfHeader() looks for the signature in the first 1024 bytes
    let head = &buffer[..buffer.len().min(1024)];
    if find_bytes(head, b"%PDF-").is_none() {
        return Err(OpenError::Invalid(
            "PDF header signature not found.".to_string(),
        ));
    }

    let load = |password: Option<String>| -> Result<Document, OpenError> {
        let options = LoadOptions {
            password,
            ..LoadOptions::default()
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Document::load_mem_with_options(buffer, options)
        })) {
            Ok(Ok(doc)) => Ok(doc),
            Ok(Err(e)) => Err(map_load_error(e)),
            Err(_) => Err(OpenError::Unexpected(
                "java.lang.RuntimeException: PDF parser failure".to_string(),
            )),
        }
    };

    let mut doc = load(None)?;
    let mut encrypted = false;
    let mut owner_password_used = false;

    let needs_password = doc.trailer.has(b"Encrypt");
    if needs_password || doc.encryption_state.is_some() {
        encrypted = true;

        // The document against which passwords are checked: the still-encrypted one, or (when
        // lopdf already opened it with the empty user password) a stand-in carrying a copy of
        // its encryption dictionary and file ID.
        let stand_in;
        let auth_doc: &Document = if needs_password {
            if doc.get_encrypted().is_err() {
                return Err(OpenError::Invalid("unknown.encryption".to_string()));
            }
            &doc
        } else {
            let state = doc.encryption_state.as_ref().unwrap();
            let dict = state
                .encode()
                .map_err(|_| OpenError::Invalid("unknown.encryption".to_string()))?;
            let mut d = Document::with_version("1.4");
            let id = d.add_object(Object::Dictionary(dict));
            d.trailer.set("Encrypt", Object::Reference(id));
            if let Ok(file_id) = doc.trailer.get(b"ID") {
                d.trailer.set("ID", file_id.clone());
            }
            stand_in = d;
            &stand_in
        };

        let revision = auth_doc
            .get_encrypted()
            .ok()
            .and_then(|d| d.get(b"R").ok())
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0);
        let algorithm = PasswordAlgorithm::try_from(auth_doc)
            .map_err(|_| OpenError::Invalid("unknown.encryption.type.r".to_string()))?;

        // R5/R6 (AES-256) passwords are UTF-8 (SASLprep'd); older revisions use PDFDocEncoding.
        // (iText 4.2 rejects R5/R6 outright with "unknown.encryption.type.r"; we read them.)
        let password: Vec<u8> = if revision >= 5 {
            algorithm
                .sanitize_password(password_utf8)
                .unwrap_or_else(|_| password_utf8.as_bytes().to_vec())
        } else {
            password_pdfdoc.to_vec()
        };

        // check by owner password, first, to ensure greatest authorization
        if auth_doc.authenticate_raw_owner_password(&password).is_ok() {
            owner_password_used = true;
        } else if auth_doc.authenticate_raw_user_password(&password).is_err() {
            // check by user password
            return Err(OpenError::BadPassword);
        }

        if needs_password {
            let password_string = if revision >= 5 {
                password_utf8.to_string()
            } else {
                lopdf_password_string(&algorithm, &password).ok_or(OpenError::BadPassword)?
            };
            doc = load(Some(password_string))?;
            if doc.trailer.has(b"Encrypt") {
                return Err(OpenError::BadPassword);
            }
        }
    }

    finish_reader(&mut doc)?;
    Ok(OpenedPdf {
        doc,
        encrypted,
        owner_password_used,
    })
}

/// lopdf only takes passwords as `&str`, which it converts to PDFDocEncoding itself. Build the
/// string that converts back to exactly `bytes`; None if some byte has no such character.
fn lopdf_password_string(algorithm: &PasswordAlgorithm, bytes: &[u8]) -> Option<String> {
    const LOW: [u16; 8] = [
        0x02d8, 0x02c7, 0x02c6, 0x02d9, 0x02dd, 0x02db, 0x02da, 0x02dc,
    ];
    let mut s = String::with_capacity(bytes.len());
    for &b in bytes {
        let c = match b {
            0x18..=0x1f => LOW[(b - 0x18) as usize] as u32,
            0x80..=0xa0 => PDFDOC_HIGH[(b - 0x80) as usize] as u32,
            _ => b as u32,
        };
        s.push(char::from_u32(c)?);
    }
    match algorithm.sanitize_password(&s) {
        Ok(converted) if converted == bytes => Some(s),
        _ => None,
    }
}

fn map_load_error(e: lopdf::Error) -> OpenError {
    use lopdf::Error as E;
    match e {
        E::InvalidPassword => OpenError::BadPassword,
        E::Parse(lopdf::ParseError::InvalidFileHeader) => {
            OpenError::Invalid("PDF header signature not found.".to_string())
        }
        E::Xref(_)
        | E::Parse(lopdf::ParseError::InvalidXref)
        | E::Parse(lopdf::ParseError::InvalidTrailer)
        | E::Parse(lopdf::ParseError::EndOfInput)
        | E::InvalidOffset(_)
        | E::MissingXrefEntry => OpenError::Invalid(
            "Rebuild failed: trailer not found.; Original message: PDF startxref not found."
                .to_string(),
        ),
        E::UnsupportedSecurityHandler(_) => OpenError::Invalid("unknown.encryption".to_string()),
        E::Decryption(_) => OpenError::Invalid("unknown.encryption.type.r".to_string()),
        E::IO(e) => OpenError::Unexpected(format!("java.io.IOException: {}", e)),
        other => OpenError::Invalid(other.to_string()),
    }
}

/// What PdfReader.readPdf() does once the objects are loaded: readPages() and
/// removeUnusedObjects().
fn finish_reader(doc: &mut Document) -> Result<(), OpenError> {
    // entries of a cross-reference stream dictionary that are not trailer entries
    for key in [
        &b"Prev"[..],
        b"XRefStm",
        b"W",
        b"Index",
        b"Filter",
        b"DecodeParms",
        b"Length",
        b"Type",
    ] {
        doc.trailer.remove(key);
    }

    // readPages()
    let catalog_id = match doc.trailer.get(b"Root") {
        Ok(Object::Reference(id)) if matches!(doc.objects.get(id), Some(Object::Dictionary(_))) => {
            *id
        }
        Ok(Object::Dictionary(dict)) => {
            // a direct catalog; make it indirect so that everything else can rely on it
            let dict = dict.clone();
            let id = doc.add_object(Object::Dictionary(dict));
            doc.trailer.set("Root", Object::Reference(id));
            id
        }
        _ => {
            // The catalog did not survive parsing (iText's tokeniser is more forgiving than
            // lopdf's). If the page tree itself is intact, stand in a catalog for it.
            let root = doc.objects.iter().find_map(|(id, o)| match o {
                Object::Dictionary(d)
                    if !d.has(b"Parent")
                        && matches!(d.get(b"Kids").ok().and_then(|k| resolve(doc, k)), Some(Object::Array(_)))
                        && (d.has_type(b"Pages") || !d.has(b"Type")) =>
                {
                    Some(*id)
                }
                _ => None,
            });
            match root {
                Some(root) => {
                    let mut catalog = Dictionary::new();
                    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
                    catalog.set("Pages", Object::Reference(root));
                    let id = doc.add_object(Object::Dictionary(catalog));
                    doc.trailer.set("Root", Object::Reference(id));
                    id
                }
                None => return Err(OpenError::Invalid("catalog not found".to_string())),
            }
        }
    };

    let pages_obj = doc
        .get_dictionary(catalog_id)
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .cloned();
    let root_pages = match pages_obj {
        Some(Object::Reference(id)) if matches!(doc.objects.get(&id), Some(Object::Dictionary(_))) => {
            id
        }
        Some(Object::Dictionary(dict)) => {
            let id = doc.add_object(Object::Dictionary(dict));
            if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
                catalog.set("Pages", Object::Reference(id));
            }
            id
        }
        _ => {
            eprintln!("Warning: failed to read page tree. Corrupt input.");
            // rebuildPages(): collect the orphan pages
            let kids: Vec<Object> = doc
                .objects
                .iter()
                .filter(|(_, o)| matches!(o, Object::Dictionary(d) if d.has_type(b"Page")))
                .map(|(id, _)| Object::Reference(*id))
                .collect();
            let mut root = Dictionary::new();
            root.set("Type", Object::Name(b"Pages".to_vec()));
            root.set("Kids", Object::Array(kids));
            let id = doc.add_object(Object::Dictionary(root));
            if let Ok(catalog) = doc.get_dictionary_mut(catalog_id) {
                catalog.set("Pages", Object::Reference(id));
            }
            id
        }
    };

    // PageRefs.readPages()
    let walk = walk_page_tree(doc, root_pages, true).map_err(OpenError::Invalid)?;
    for (id, cut) in &walk.truncate {
        // a Kids array is cut at its first direct (non-reference) entry
        let kids_ref = match doc.get_dictionary(*id).ok().and_then(|d| d.get(b"Kids").ok()) {
            Some(Object::Reference(r)) => Some(*r),
            _ => None,
        };
        let kids = match kids_ref {
            Some(r) => doc.objects.get_mut(&r).and_then(|o| o.as_array_mut().ok()),
            None => doc
                .get_dictionary_mut(*id)
                .ok()
                .and_then(|d| d.get_mut(b"Kids").ok())
                .and_then(|o| o.as_array_mut().ok()),
        };
        if let Some(kids) = kids {
            kids.truncate(*cut);
        }
    }
    for id in &walk.branches {
        if let Ok(node) = doc.get_dictionary_mut(*id) {
            node.set("Type", Object::Name(b"Pages".to_vec()));
        }
    }
    for (id, inherited) in &walk.leaves {
        if let Ok(page) = doc.get_dictionary_mut(*id) {
            page.set("Type", Object::Name(b"Page".to_vec()));
            for (key, value) in inherited.iter() {
                if !page.has(key) {
                    page.set(key.clone(), value.clone());
                }
            }
            if !page.has(b"MediaBox") {
                page.set(
                    "MediaBox",
                    Object::Array(LETTER.iter().map(|v| Object::Real(*v)).collect()),
                );
            }
        }
    }
    if let Ok(root) = doc.get_dictionary_mut(root_pages) {
        root.set("Count", Object::Integer(walk.leaves.len() as i64));
    }

    remove_unused_objects(doc);
    Ok(())
}

struct PageWalk {
    /// page id and the attributes it inherits, in document order
    leaves: Vec<(ObjectId, Dictionary)>,
    branches: Vec<ObjectId>,
    /// (node, length) for Kids arrays that hold a direct object
    truncate: Vec<(ObjectId, usize)>,
}

const MAX_PAGE_TREE_DEPTH: usize = 512;
const MAX_PAGES: usize = 4_000_000;

/// Port of PdfReader.PageRefs.iteratePages(), without the side effects.
///
/// With `inherit` false the inherited attributes are not gathered (cheaper: page_ids()).
fn walk_page_tree(doc: &Document, root: ObjectId, inherit: bool) -> Result<PageWalk, String> {
    fn iterate(
        doc: &Document,
        id: ObjectId,
        inherited: &Dictionary,
        inherit: bool,
        stack: &mut Vec<ObjectId>,
        out: &mut PageWalk,
    ) -> Result<(), String> {
        // pageobj might be a PdfNull in the wild.
        let page = match doc.objects.get(&id) {
            Some(Object::Dictionary(d)) => d,
            _ => return Ok(()),
        };
        let kids = page
            .get(b"Kids")
            .ok()
            .and_then(|o| resolve(doc, o))
            .and_then(|o| o.as_array().ok());
        match kids {
            // reference to a leaf
            None => {
                if out.leaves.len() >= MAX_PAGES {
                    return Err(format!("Invalid reference on Kids: {}", id.0));
                }
                out.leaves.push((id, inherited.clone()));
            }
            // reference to a branch
            Some(kids) => {
                if stack.contains(&id) || stack.len() >= MAX_PAGE_TREE_DEPTH {
                    return Err(format!("Invalid reference on Kids: {}", id.0));
                }
                out.branches.push(id);
                // pushPageAttributes()
                let mut dic = inherited.clone();
                if inherit {
                    for key in PAGE_INH_CANDIDATES {
                        if let Ok(value) = page.get(key) {
                            dic.set(key, value.clone());
                        }
                    }
                }
                stack.push(id);
                for (k, kid) in kids.iter().enumerate() {
                    let kid_id = match kid {
                        Object::Reference(r) => *r,
                        _ => {
                            out.truncate.push((id, k));
                            break;
                        }
                    };
                    if kid_id.0 == id.0 {
                        return Err(format!("Invalid reference on Kids: {}", kid_id.0));
                    }
                    iterate(doc, kid_id, &dic, inherit, stack, out)?;
                }
                stack.pop();
            }
        }
        Ok(())
    }

    let mut out = PageWalk {
        leaves: Vec::new(),
        branches: Vec::new(),
        truncate: Vec::new(),
    };
    iterate(doc, root, &Dictionary::new(), inherit, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// Port of PdfReader.removeUnusedObjects(): drop every object the trailer does not reach.
pub fn remove_unused_objects(doc: &mut Document) {
    doc.prune_objects();
}

////////////////////////////////////////////////////////////////////////////
// named destinations

type DestNames = HashMap<(bool, Vec<u8>), Object>;

/// PdfReader.getNameArray()
fn get_name_array(doc: &Document, obj: &Object) -> Option<Object> {
    match resolve(doc, obj)? {
        arr @ Object::Array(_) => Some(arr.clone()),
        Object::Dictionary(d) => match resolve(doc, d.get(b"D").ok()?)? {
            arr @ Object::Array(_) => Some(arr.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Port of PdfReader.getNamedDestination(true): keys are (is_name, bytes).
pub fn named_destinations(doc: &Document) -> HashMap<(bool, Vec<u8>), Object> {
    let mut names: DestNames = HashMap::new();
    let catalog = match doc.catalog() {
        Ok(c) => c,
        Err(_) => return names,
    };
    // getNamedDestinationFromNames()
    if let Some(Object::Dictionary(dests)) = catalog.get(b"Dests").ok().and_then(|o| resolve(doc, o))
    {
        for (key, value) in dests.iter() {
            if let Some(arr) = get_name_array(doc, value) {
                names.insert((true, key.clone()), arr);
            }
        }
    }
    // getNamedDestinationFromStrings()
    let tree = catalog
        .get(b"Names")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Dests").ok())
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok());
    if let Some(tree) = tree {
        fn iterate_items(
            doc: &Document,
            dic: &Dictionary,
            names: &mut DestNames,
            seen: &mut HashSet<ObjectId>,
            depth: usize,
        ) {
            if depth > 200 {
                return;
            }
            if let Some(Object::Array(nn)) = dic.get(b"Names").ok().and_then(|o| resolve(doc, o)) {
                for pair in nn.chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    if let Some(Object::String(s, _)) = resolve(doc, &pair[0]) {
                        if let Some(arr) = get_name_array(doc, &pair[1]) {
                            names.insert((false, s.clone()), arr);
                        }
                    }
                }
            } else if let Some(Object::Array(kids)) =
                dic.get(b"Kids").ok().and_then(|o| resolve(doc, o))
            {
                for kid in kids {
                    if let Object::Reference(id) = kid {
                        if !seen.insert(*id) {
                            continue;
                        }
                    }
                    if let Some(Object::Dictionary(kid)) = resolve(doc, kid) {
                        iterate_items(doc, kid, names, seen, depth + 1);
                    }
                }
            }
        }
        iterate_items(doc, tree, &mut names, &mut HashSet::new(), 0);
    }
    names
}

fn lookup_dest(doc: &Document, names: &DestNames, obj: &Object) -> Option<Object> {
    match resolve(doc, obj)? {
        Object::Name(n) => names.get(&(true, n.clone())).cloned(),
        Object::String(s, _) => names.get(&(false, s.clone())).cloned(),
        _ => None,
    }
}

/// The GoTo half of replaceNamedDestination(), on the action dictionary.
fn replace_dest_in_action(doc: &Document, action: &mut Dictionary, names: &DestNames) {
    let is_goto = matches!(
        action.get(b"S").ok().and_then(|o| resolve(doc, o)),
        Some(Object::Name(n)) if n == b"GoTo"
    );
    if !is_goto {
        return;
    }
    if let Some(dest) = action.get(b"D").ok().and_then(|o| lookup_dest(doc, names, o)) {
        action.set("D", dest);
    }
}

/// Port of PdfReader.replaceNamedDestination() on an annotation or outline dictionary.
/// Returns the id of an indirect action dictionary that still has to be processed.
fn replace_dest_in_dict(
    doc: &Document,
    dict: &mut Dictionary,
    names: &DestNames,
) -> Option<ObjectId> {
    let has_dest = matches!(
        dict.get(b"Dest").ok().and_then(|o| resolve(doc, o)),
        Some(o) if !matches!(o, Object::Null)
    );
    if has_dest {
        if let Some(dest) = dict.get(b"Dest").ok().and_then(|o| lookup_dest(doc, names, o)) {
            dict.set("Dest", dest);
        }
        return None;
    }
    match dict.get_mut(b"A") {
        Ok(Object::Reference(id)) => Some(*id),
        Ok(Object::Dictionary(action)) => {
            replace_dest_in_action(doc, action, names);
            None
        }
        _ => None,
    }
}

/// Run `f` on the indirect object `id` while the rest of the document stays readable.
fn with_object_taken<R>(
    doc: &mut Document,
    id: ObjectId,
    f: impl FnOnce(&Document, &mut Object) -> R,
) -> Option<R> {
    let mut obj = doc.objects.remove(&id)?;
    let r = f(doc, &mut obj);
    doc.objects.insert(id, obj);
    Some(r)
}

fn replace_dest_in_object(doc: &mut Document, id: ObjectId, names: &DestNames) {
    let action = with_object_taken(doc, id, |doc, obj| match obj {
        Object::Dictionary(d) => replace_dest_in_dict(doc, d, names),
        _ => None,
    })
    .flatten();
    if let Some(action_id) = action {
        with_object_taken(doc, action_id, |doc, obj| {
            if let Object::Dictionary(d) = obj {
                replace_dest_in_action(doc, d, names);
            }
        });
    }
}

/// Port of PdfReader.consolidateNamedDestinations(): replaces all the local named links
/// (annotations and outline items) with the actual destinations.
pub fn consolidate_named_destinations(doc: &mut Document) {
    let names = named_destinations(doc);
    if names.is_empty() {
        return;
    }

    for page_id in page_ids(doc) {
        // the Annots array is either held by the page or is an object of its own
        let holder = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok()) {
            Some(Object::Reference(id)) => *id,
            Some(Object::Array(_)) => page_id,
            _ => continue,
        };
        let pending = with_object_taken(doc, holder, |doc, obj| {
            let annots = match obj {
                Object::Array(a) => Some(a),
                Object::Dictionary(d) => d.get_mut(b"Annots").ok().and_then(|o| o.as_array_mut().ok()),
                _ => None,
            };
            let mut pending = Vec::new();
            for annot in annots.into_iter().flatten() {
                match annot {
                    Object::Reference(id) => pending.push(*id),
                    Object::Dictionary(d) => {
                        if let Some(action) = replace_dest_in_dict(doc, d, &names) {
                            pending.push(action);
                        }
                    }
                    _ => {}
                }
            }
            pending
        })
        .unwrap_or_default();
        for id in pending {
            // (an indirect action reached from a direct annotation is handled here as well:
            // replace_dest_in_object looks at /Dest and /A, and an action has neither a
            // /Dest nor, normally, an /A, so treat it as an action when it has /S)
            let is_action = matches!(
                doc.objects.get(&id),
                Some(Object::Dictionary(d)) if d.has(b"S") && !d.has(b"Subtype")
            );
            if is_action {
                with_object_taken(doc, id, |doc, obj| {
                    if let Object::Dictionary(d) = obj {
                        replace_dest_in_action(doc, d, &names);
                    }
                });
            } else {
                replace_dest_in_object(doc, id, &names);
            }
        }
    }

    // iterateBookmarks()
    let first = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"First").ok())
        .and_then(|o| o.as_reference().ok());
    let mut todo: Vec<ObjectId> = first.into_iter().collect();
    let mut seen: HashSet<ObjectId> = HashSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        replace_dest_in_object(doc, id, &names);
        if let Ok(outline) = doc.get_dictionary(id) {
            for key in [&b"Next"[..], b"First"] {
                if let Ok(Object::Reference(r)) = outline.get(key) {
                    todo.push(*r);
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////
// page helpers

/// 1-based page number -> page object id, in document order.
pub fn page_ids(doc: &Document) -> Vec<ObjectId> {
    let root = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Pages").ok())
        .and_then(|o| o.as_reference().ok());
    match root {
        Some(root) => walk_page_tree(doc, root, false)
            .map(|w| w.leaves.into_iter().map(|(id, _)| id).collect())
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Resolve a (possibly indirect) object. None if a reference leads nowhere.
pub fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    let mut obj = obj;
    for _ in 0..64 {
        match obj {
            Object::Reference(id) => obj = doc.objects.get(id)?,
            _ => return Some(obj),
        }
    }
    None
}

/// Like [`page_attr`], but returns the entry as stored (possibly an indirect reference).
pub fn page_attr_raw(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(id) {
            return None;
        }
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(value) = dict.get(key) {
            // an entry that resolves to nothing counts as missing
            if matches!(resolve(doc, value), Some(o) if !matches!(o, Object::Null)) {
                return Some(value.clone());
            }
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
}

/// Look up `key` on a page, walking /Parent for inheritable attributes (MediaBox, CropBox,
/// Rotate, Resources). The value is returned resolved (never an indirect reference).
pub fn page_attr(doc: &Document, page: ObjectId, key: &[u8]) -> Option<Object> {
    let raw = page_attr_raw(doc, page, key)?;
    resolve(doc, &raw).cloned()
}

fn object_as_f32(doc: &Document, obj: &Object) -> Option<f32> {
    match resolve(doc, obj)? {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Page /Rotate normalised to 0..360 (iText getPageRotation).
pub fn page_rotation(doc: &Document, page: ObjectId) -> i32 {
    let n = match page_attr(doc, page, b"Rotate") {
        Some(Object::Integer(i)) => i as i32,
        Some(Object::Real(r)) => r as i32,
        _ => return 0,
    };
    let n = n % 360;
    if n < 0 {
        n + 360
    } else {
        n
    }
}

/// PdfReader.getNormalizedRectangle()
fn normalized_rectangle(doc: &Document, obj: &Object) -> Option<[f32; 4]> {
    let arr = resolve(doc, obj)?.as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let llx = object_as_f32(doc, &arr[0])?;
    let lly = object_as_f32(doc, &arr[1])?;
    let urx = object_as_f32(doc, &arr[2])?;
    let ury = object_as_f32(doc, &arr[3])?;
    Some([llx.min(urx), lly.min(ury), llx.max(urx), lly.max(ury)])
}

/// [llx, lly, urx, ury], normalised. iText getPageSize (MediaBox; LETTER if the page has none).
pub fn page_media_box(doc: &Document, page: ObjectId) -> [f32; 4] {
    page_attr(doc, page, b"MediaBox")
        .and_then(|o| normalized_rectangle(doc, &o))
        .unwrap_or(LETTER)
}

/// iText getCropBox (CropBox, falling back to MediaBox).
pub fn page_crop_box(doc: &Document, page: ObjectId) -> [f32; 4] {
    page_attr(doc, page, b"CropBox")
        .and_then(|o| normalized_rectangle(doc, &o))
        .unwrap_or_else(|| page_media_box(doc, page))
}

/// Port of TK_Session.apply_rotation_to_page().
pub fn apply_rotation_to_page(doc: &mut Document, page_num: u32, rotation: i32, absolute: bool) {
    // DF rotate
    let pages = page_ids(doc);
    let page_id = match (page_num as usize).checked_sub(1).and_then(|i| pages.get(i)) {
        Some(id) => *id,
        None => return,
    };
    let mut rotation = rotation;
    if !absolute {
        rotation += page_rotation(doc, page_id);
    }
    rotation %= 360;
    // does an ancestor still supply a rotation? (not the case for add_reader() documents,
    // whose pages carry their inherited attributes themselves)
    let inherited = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Parent").ok())
        .and_then(|o| o.as_reference().ok())
        .map(|parent| page_rotation(doc, parent) != 0)
        .unwrap_or(false);
    if let Ok(page) = doc.get_dictionary_mut(page_id) {
        page.remove(b"Rotate");
        if rotation != PageRotate::North as i32 || inherited {
            // default rotation
            page.set("Rotate", Object::Integer(rotation as i64));
        }
    }
}

/// Port of filter.add_mark_to_page(): `page_index` is the 1-based page, `page_num` the mark.
pub fn add_mark_to_page(doc: &mut Document, page_index: u32, page_num: u32) {
    let pages = page_ids(doc);
    if let Some(id) = (page_index as usize).checked_sub(1).and_then(|i| pages.get(i)) {
        if let Ok(page) = doc.get_dictionary_mut(*id) {
            page.set(PAGE_MARKER, Object::Integer(page_num as i64));
        }
    }
}

/// Port of filter.add_marks_to_pages().
pub fn add_marks_to_pages(doc: &mut Document) {
    for (i, id) in page_ids(doc).into_iter().enumerate() {
        if let Ok(page) = doc.get_dictionary_mut(id) {
            page.set(PAGE_MARKER, Object::Integer(i as i64 + 1));
        }
    }
}

/// Port of filter.remove_mark_from_page().
pub fn remove_mark_from_page(doc: &mut Document, page_num: u32) {
    let pages = page_ids(doc);
    if let Some(id) = (page_num as usize).checked_sub(1).and_then(|i| pages.get(i)) {
        if let Ok(page) = doc.get_dictionary_mut(*id) {
            page.remove(PAGE_MARKER);
        }
    }
}

/// Port of filter.remove_marks_from_pages().
pub fn remove_marks_from_pages(doc: &mut Document) {
    for id in page_ids(doc) {
        if let Ok(page) = doc.get_dictionary_mut(id) {
            page.remove(PAGE_MARKER);
        }
    }
}

////////////////////////////////////////////////////////////////////////////
// text strings

/// iText's pdfEncodingByteToChar for bytes 128..=160; every other byte maps to itself.
const PDFDOC_HIGH: [u16; 33] = [
    0x2022, 0x2020, 0x2021, 0x2026, 0x2014, 0x2013, 0x0192, 0x2044, 0x2039, 0x203a, 0x2212,
    0x2030, 0x201e, 0x201c, 0x201d, 0x2018, 0x2019, 0x201a, 0x2122, 0xfb01, 0xfb02, 0x0141,
    0x0152, 0x0160, 0x0178, 0x017d, 0x0131, 0x0142, 0x0153, 0x0161, 0x017e, 0xfffd, 0x20ac,
];

/// Decode a PDF text string (PDFDocEncoding or UTF-16BE with BOM) to a Rust String (iText toUnicodeString).
pub fn text_string_to_unicode(bytes: &[u8]) -> String {
    // PdfString.isUnicode()
    if bytes.len() >= 2 && bytes[0] == 0xfe && bytes[1] == 0xff {
        // Java's "UTF-16" decoder: big endian after the byte order mark
        let body = &bytes[2..];
        let units = body.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]]));
        let mut s: String = char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect();
        if body.len() % 2 == 1 {
            s.push('\u{fffd}');
        }
        s
    } else {
        bytes
            .iter()
            .map(|&b| match b {
                128..=160 => char::from_u32(PDFDOC_HIGH[(b - 128) as usize] as u32).unwrap_or('\u{fffd}'),
                _ => b as char,
            })
            .collect()
    }
}

/// The PDFDocEncoding byte for a character, per iText (PdfEncodings.pdfEncoding).
fn pdfdoc_byte(c: char) -> Option<u8> {
    let n = c as u32;
    if n < 128 || (n > 160 && n <= 255) {
        return Some(n as u8);
    }
    if n == 0xfffd {
        return None;
    }
    PDFDOC_HIGH
        .iter()
        .position(|&u| u as u32 == n)
        .map(|i| (i + 128) as u8)
}

/// PdfEncodings.isPdfDocEncoding()
pub fn is_pdfdoc_encoding(s: &str) -> bool {
    s.chars().all(|c| pdfdoc_byte(c).is_some())
}

/// Encode a Rust String as a PDF text string: PDFDocEncoding if possible, else UTF-16BE with BOM (iText PdfString TEXT_UNICODE rule).
pub fn unicode_to_text_string(s: &str) -> Vec<u8> {
    if let Some(bytes) = s.chars().map(pdfdoc_byte).collect::<Option<Vec<u8>>>() {
        return bytes;
    }
    let mut bytes = vec![0xfe, 0xff];
    for unit in s.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

////////////////////////////////////////////////////////////////////////////
// copying between documents

fn is_page_dict(obj: &Object) -> bool {
    matches!(obj, Object::Dictionary(d) if matches!(d.get(b"Type"), Ok(Object::Name(n)) if n == b"Page"))
}

fn for_each_reference(obj: &mut Object, f: &mut dyn FnMut(&mut ObjectId)) {
    match obj {
        Object::Reference(id) => f(id),
        Object::Array(items) => items.iter_mut().for_each(|o| for_each_reference(o, f)),
        Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| for_each_reference(o, f)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| for_each_reference(o, f)),
        _ => {}
    }
}

/// Copy the object graph reachable from `root` in `src` into `dst`, returning the new id of `root`.
/// `map` caches src-id -> dst-id so shared objects are copied once. /Parent links of pages are NOT followed
/// (the caller handles page-tree placement).
///
/// As in iText's PdfCopy: a copied page loses its /Parent (and /B); and a reference that leads
/// to some *other* page (a link destination, an annotation's /P, ...) is translated but that
/// page is not copied - it gets its id in `map`, and is filled in when the caller later copies
/// that page as a `root` of its own (otherwise the reference dangles, which PDF allows).
pub fn copy_object_graph(
    src: &Document,
    dst: &mut Document,
    root: ObjectId,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> ObjectId {
    let root_new = match map.get(&root) {
        Some(id) => *id,
        None => {
            let id = dst.new_object_id();
            map.insert(root, id);
            id
        }
    };
    let mut stack = vec![root];
    while let Some(src_id) = stack.pop() {
        let dst_id = map[&src_id];
        if dst.objects.contains_key(&dst_id) {
            continue; // we've already copied this
        }
        let mut obj = match src.objects.get(&src_id) {
            Some(obj) => obj.clone(),
            None => continue, // a broken reference stays one
        };
        if is_page_dict(&obj) {
            if let Object::Dictionary(page) = &mut obj {
                page.remove(b"Parent");
                page.remove(b"B");
            }
        }
        for_each_reference(&mut obj, &mut |id: &mut ObjectId| {
            let target = *id;
            let new_id = match map.get(&target) {
                Some(new_id) => *new_id,
                None => {
                    let new_id = dst.new_object_id();
                    map.insert(target, new_id);
                    new_id
                }
            };
            if !dst.objects.contains_key(&new_id)
                && !src.objects.get(&target).is_some_and(is_page_dict)
            {
                stack.push(target);
            }
            *id = new_id;
        });
        dst.objects.insert(dst_id, obj);
    }
    root_new
}

////////////////////////////////////////////////////////////////////////////
// writing output

/// The strength the output is encrypted with, after the prepare_writer() default: AES-128
/// when a password is given without a strength keyword.
pub fn output_encryption_strength(session: &TkSession) -> EncryptionStrength {
    if session.m_output_encryption_strength != EncryptionStrength::None
        || !session.m_output_owner_pw.is_empty()
        || !session.m_output_user_pw.is_empty()
    {
        if session.m_output_encryption_strength == EncryptionStrength::None {
            EncryptionStrength::Aes128
        } else {
            session.m_output_encryption_strength
        }
    } else {
        EncryptionStrength::None
    }
}

/// The PDF version prepare_writer() returns (encryption_strength.pdf_version).
pub fn output_encryption_pdf_version(session: &TkSession) -> &'static str {
    match output_encryption_strength(session) {
        EncryptionStrength::None => "1.2",
        EncryptionStrength::Rc4_40 => "1.3",
        EncryptionStrength::Rc4_128 => "1.4",
        EncryptionStrength::Aes128 => "1.6",
    }
}

/// 16 unpredictable bytes (stands in for PdfEncryption.createDocumentId()).
fn random_id() -> Vec<u8> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = Vec::with_capacity(16);
    for round in 0..2u64 {
        // RandomState is seeded from the operating system's entropy source
        let mut h = RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_u32(std::process::id());
        h.write_u64(SEQ.fetch_add(1, Ordering::Relaxed));
        h.write_u64(round);
        out.extend_from_slice(&h.finish().to_be_bytes());
    }
    out
}

/// PdfEncryption.createInfoId(): two fresh identifiers.
fn new_file_id() -> Object {
    Object::Array(vec![
        Object::String(random_id(), StringFormat::Hexadecimal),
        Object::String(random_id(), StringFormat::Hexadecimal),
    ])
}

/// The current time as a PDF date (PdfDate), in UTC.
pub fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "D:{:04}{:02}{:02}{:02}{:02}{:02}Z",
        y,
        m,
        d,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn filter_names(doc: &Document, stream: &Stream) -> Option<Vec<Vec<u8>>> {
    match stream.dict.get(b"Filter").ok().map(|o| resolve(doc, o)) {
        None => Some(Vec::new()),
        Some(Some(Object::Null)) => Some(Vec::new()),
        Some(Some(Object::Name(n))) => Some(vec![n.clone()]),
        Some(Some(Object::Array(items))) => items
            .iter()
            .map(|o| match resolve(doc, o) {
                Some(Object::Name(n)) => Some(n.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn canonical_filter(name: &[u8]) -> Option<&'static [u8]> {
    match name {
        b"FlateDecode" | b"Fl" => Some(b"FlateDecode"),
        b"ASCIIHexDecode" | b"AHx" => Some(b"ASCIIHexDecode"),
        b"ASCII85Decode" | b"A85" => Some(b"ASCII85Decode"),
        b"LZWDecode" => Some(b"LZWDecode"),
        _ => None,
    }
}

/// Port of PRStream.filtersAreKnown(): the canonical filter chain if pdftk will re-filter
/// this stream, None if the stream is to be passed through untouched.
fn known_filters(doc: &Document, stream: &Stream) -> Option<Vec<&'static [u8]>> {
    // workaround for occasionally buggy decoding of streams that utilize a predictor
    if stream.dict.has(b"DecodeParms") {
        return None;
    }
    filter_names(doc, stream)?
        .iter()
        .map(|n| canonical_filter(n))
        .collect()
}

/// What PRStream.toPdf() does to every stream when filterStreams / compressStreams is set.
fn refilter_streams(doc: &mut Document, compress: bool) {
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, Object::Stream(_)))
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        let filters = match doc.objects.get(&id) {
            Some(Object::Stream(stream)) => match known_filters(doc, stream) {
                Some(filters) => filters,
                None => continue,
            },
            _ => continue,
        };
        let stream = match doc.objects.get_mut(&id) {
            Some(Object::Stream(stream)) => stream,
            _ => continue,
        };
        if compress && filters.len() == 1 && filters[0] == b"FlateDecode" {
            // already what we would produce
            stream.dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
            continue;
        }
        if !filters.is_empty() {
            // spell the filters out directly (no abbreviations, no indirect objects)
            stream.dict.set(
                "Filter",
                Object::Array(filters.iter().map(|f| Object::Name(f.to_vec())).collect()),
            );
            match stream.decompressed_content() {
                Ok(data) => stream.set_plain_content(data),
                Err(_) => {
                    eprintln!("Warning: failed to read stream data. Corrupt input.");
                    continue;
                }
            }
        }
        if compress {
            let _ = stream.compress();
        }
    }
}

fn encrypt_document(
    doc: &mut Document,
    strength: EncryptionStrength,
    user_pw: &[u8],
    owner_pw: &[u8],
    user_perms: i32,
) -> Result<(), String> {
    // PdfEncryption.setupAllKeys():
    // if no owner password is given, use the user password in its place
    let owner_pw = if owner_pw.is_empty() { user_pw } else { owner_pw };
    // PDF 1.3 ref. (40 bit enc.) says to set unused, upper bits to 1
    let mut permissions = user_perms as u32;
    permissions |= if strength == EncryptionStrength::Rc4_40 {
        0xffffffc0
    } else {
        0xfffff0c0
    };
    permissions &= 0xfffffffc;
    let permissions = Permissions::from_bits_retain(permissions as u64);

    // lopdf wants the passwords as text, and turns them back into PDFDocEncoding
    let algorithm = {
        let mut dict = Dictionary::new();
        dict.set("Filter", Object::Name(b"Standard".to_vec()));
        dict.set("V", Object::Integer(2));
        dict.set("R", Object::Integer(3));
        dict.set("Length", Object::Integer(128));
        dict.set("O", Object::string_literal(vec![0u8; 32]));
        dict.set("U", Object::string_literal(vec![0u8; 32]));
        dict.set("P", Object::Integer(-4));
        let mut d = Document::with_version("1.4");
        let id = d.add_object(Object::Dictionary(dict));
        d.trailer.set("Encrypt", Object::Reference(id));
        PasswordAlgorithm::try_from(&d).map_err(|e| e.to_string())?
    };
    let user = lopdf_password_string(&algorithm, user_pw)
        .ok_or("the user password cannot be represented")?;
    let owner = lopdf_password_string(&algorithm, owner_pw)
        .ok_or("the owner password cannot be represented")?;

    let state = {
        let version = match strength {
            EncryptionStrength::Rc4_40 => EncryptionVersion::V1 {
                document: doc,
                owner_password: &owner,
                user_password: &user,
                permissions,
            },
            EncryptionStrength::Rc4_128 => EncryptionVersion::V2 {
                document: doc,
                owner_password: &owner,
                user_password: &user,
                key_length: 128,
                permissions,
            },
            _ => {
                let mut crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>> = BTreeMap::new();
                crypt_filters.insert(b"StdCF".to_vec(), Arc::new(Aes128CryptFilter));
                EncryptionVersion::V4 {
                    document: doc,
                    encrypt_metadata: true,
                    crypt_filters,
                    stream_filter: b"StdCF".to_vec(),
                    string_filter: b"StdCF".to_vec(),
                    owner_password: &owner,
                    user_password: &user,
                    permissions,
                }
            }
        };
        EncryptionState::try_from(version).map_err(|e| e.to_string())?
    };
    doc.encrypt(&state).map_err(|e| e.to_string())
}

/// Write `doc` to `out` applying the session output options (compress/uncompress, encryption,
/// user perms, keep_first_id/keep_final_id; drop_xmp is handled by callers).
///
/// What the caller prepares, and what happens here:
/// * `doc.version` is written as the header version. For cat/shuffle it is raised to the
///   version the encryption strength needs (prepare_writer()'s return value); set it to the
///   maximum of the input versions beforehand.
/// * /ID: with encryption a fresh /ID is always generated (as iText does). Otherwise, in
///   "stamper" mode (every operation but cat/shuffle/burst) the /ID in `doc.trailer` is kept;
///   in "copy" mode (cat/shuffle/burst) it is kept only under keep_first_id/keep_final_id -
///   so put the chosen input's /ID array into `doc.trailer` - and is replaced otherwise.
/// * Info: in copy mode Creator, Producer, CreationDate and ModDate are added to the Info
///   dictionary (created if need be) unless already present; in stamper mode Info is left alone.
/// * Only /Root, /Info and /ID survive in the trailer; unreachable objects are not written.
///
/// `doc` is left in its written (possibly encrypted) state: do not reuse it afterwards.
pub fn write_output(session: &TkSession, doc: &mut Document, out: &mut dyn Write) -> ErrorCode {
    let copy_mode = matches!(
        session.m_operation,
        Keyword::cat_k | Keyword::shuffle_k | Keyword::burst_k
    );
    let strength = output_encryption_strength(session);
    let encrypt = strength != EncryptionStrength::None;

    // the trailer: nothing but the catalog, the info dictionary and the file ID
    let mut trailer = Dictionary::new();
    for key in [&b"Root"[..], b"Info", b"ID"] {
        if let Ok(value) = doc.trailer.get(key) {
            trailer.set(key, value.clone());
        }
    }
    doc.trailer = trailer;

    // Info
    if copy_mode {
        let info_id = match doc.trailer.get(b"Info") {
            Ok(Object::Reference(id)) if matches!(doc.objects.get(id), Some(Object::Dictionary(_))) => {
                *id
            }
            other => {
                let dict = match other {
                    Ok(Object::Dictionary(d)) => d.clone(),
                    _ => Dictionary::new(),
                };
                let id = doc.add_object(Object::Dictionary(dict));
                doc.trailer.set("Info", Object::Reference(id));
                id
            }
        };
        let date = pdf_date_now();
        if let Ok(info) = doc.get_dictionary_mut(info_id) {
            let defaults: [(&[u8], String); 4] = [
                (b"Creator", TkSession::creator()),
                (b"Producer", TkSession::creator()),
                (b"CreationDate", date.clone()),
                (b"ModDate", date),
            ];
            for (key, value) in defaults {
                if !info.has(key) {
                    info.set(key, Object::string_literal(unicode_to_text_string(&value)));
                }
            }
        }
    }

    // when compressing a PDF, we remove the page marker
    if session.m_output_compress_b && !session.m_output_uncompress_b {
        remove_marks_from_pages(doc);
    }

    // only what the catalog and the info dictionary reach is written
    doc.objects.retain(|_, o| {
        !matches!(o, Object::Stream(s) if s.dict.has_type(b"ObjStm") || s.dict.has_type(b"XRef"))
    });
    doc.prune_objects();

    // A reference to an object that is not there (PdfCopy leaves these behind for pages that
    // were linked to but not copied) is the same as null; write it as such, so that the
    // cross-reference table has no holes past its end.
    {
        let existing: HashSet<ObjectId> = doc.objects.keys().copied().collect();
        let nullify = |obj: &mut Object| nullify_dangling(obj, &existing);
        for obj in doc.objects.values_mut() {
            nullify(obj);
        }
        doc.max_id = existing.iter().map(|id| id.0).max().unwrap_or(0);
    }

    // un/compress output streams?
    if session.m_output_uncompress_b {
        refilter_streams(doc, false);
    } else if session.m_output_compress_b {
        refilter_streams(doc, true);
    }
    for obj in doc.objects.values_mut() {
        if let Object::Stream(stream) = obj {
            // the data is in memory; never rely on an indirect or a damaged /Length
            stream.dict.set("Length", Object::Integer(stream.content.len() as i64));
        }
    }

    // file ID
    let keep_id = !encrypt
        && matches!(doc.trailer.get(b"ID"), Ok(Object::Array(a)) if !a.is_empty())
        && (!copy_mode || session.m_output_keep_first_id_b || session.m_output_keep_final_id_b);
    if !keep_id {
        if copy_mode || encrypt {
            doc.trailer.set("ID", new_file_id());
        } else {
            doc.trailer.remove(b"ID");
        }
    }

    // PDF version
    if matches!(session.m_operation, Keyword::cat_k | Keyword::shuffle_k) {
        let min_version = output_encryption_pdf_version(session);
        if doc.version.as_str() < min_version {
            doc.version = min_version.to_string();
        }
    }

    // encrypt output?
    if encrypt {
        if let Err(message) = encrypt_document(
            doc,
            strength,
            &session.m_output_user_pw_pdfdoc,
            &session.m_output_owner_pw_pdfdoc,
            session.m_output_user_perms,
        ) {
            eprintln!("Error: Failed to encrypt the output PDF: {}", message);
            return ErrorCode::Error;
        }
    }

    doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
    let options = SaveOptions {
        // "uncompress" output is meant to be read and edited in a text editor
        pretty: session.m_output_uncompress_b,
        ..SaveOptions::default()
    };
    let mut target: &mut dyn Write = out;
    let result = doc
        .save_with_options(&mut target, options)
        .and_then(|_| target.flush());
    match result {
        Ok(()) => ErrorCode::NoError,
        Err(e) => {
            eprintln!("Error: Failed to write the output PDF: {}", e);
            ErrorCode::Error
        }
    }
}

fn nullify_dangling(obj: &mut Object, existing: &HashSet<ObjectId>) {
    match obj {
        Object::Reference(id) => {
            if !existing.contains(id) {
                *obj = Object::Null;
            }
        }
        Object::Array(items) => items.iter_mut().for_each(|o| nullify_dangling(o, existing)),
        Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| nullify_dangling(o, existing)),
        Object::Stream(stream) => stream
            .dict
            .iter_mut()
            .for_each(|(_, o)| nullify_dangling(o, existing)),
        _ => {}
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
#[path = "../tests/core/pdf_unit.rs"]
mod core_tests;
