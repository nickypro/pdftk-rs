//! Port of attachments.java (attach_files, unpack_files). Owner: filter agent.
#![allow(dead_code, unused)]
use crate::session::*;
use lopdf::Document;

/// Port of attachments.attach_files(): attach session.m_input_attach_file_filename to `doc`
/// (document level, or to page m_input_attach_file_pagenum; -1 means "end"/PROMPT semantics as in Java).
pub fn attach_files(_session: &mut TkSession, _doc: &mut Document) -> ErrorCode { todo!("filter agent") }

/// Port of attachments.unpack_files(): operation unpack_files_k.
pub fn unpack_files(_session: &mut TkSession) -> ErrorCode { todo!("filter agent") }
