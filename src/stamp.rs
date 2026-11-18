//! Stamp / background / multistamp / multibackground (the mark loop of filter.java plus the
//! iText getImportedPage/getUnderContent/getOverContent behaviour). Owner: filter agent.
#![allow(dead_code, unused)]
use crate::session::*;
use lopdf::Document;

/// Apply `mark` to every page of `doc`. background_b: under content; else over. multi_b: use all mark pages.
pub fn apply_mark(_doc: &mut Document, _mark: &Document, _background_b: bool, _multi_b: bool) -> ErrorCode { todo!("filter agent") }
