//! Port of passwords.java (UTF-8 <-> PDFDocEncoding password conversion). Owner: CLI agent.
#![allow(dead_code, unused)]

/// Returns None if the password contains characters not representable in PDFDocEncoding
/// (or, when encrypt_b, exceeds the 32-byte limit as in the Java).
pub fn utf8_password_to_pdfdoc(_pw: &str, _encrypt_b: bool) -> Option<Vec<u8>> { todo!("CLI agent") }
