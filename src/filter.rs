//! Port of filter.java (single-input pipeline: fill_form, update_info, rotate, stamp/background,
//! attach_files, flatten, compress/uncompress, encrypt...). Owner: filter agent.
#![allow(dead_code, unused)]
use crate::session::*;

pub fn create_output_filter(_session: &mut TkSession) -> ErrorCode { todo!("filter agent") }
