//! Port of pdftk.java (main, help/version text, prompts, output stream) and of
//! the TK_Session argument parser / is_valid / dump_session_data / create_output.
//! Owner: CLI agent.
#![allow(dead_code, unused)]
use crate::session::*;

/// Returns the process exit code.
pub fn main_noexit(_args: &[String]) -> i32 { todo!("CLI agent") }

/// Port of pdftk.prompt_for_filename().
pub fn prompt_for_filename(_message: &str) -> String { todo!("CLI agent") }

/// Port of pdftk.prompt_for_password().
pub fn prompt_for_password(_pw_name: &str, _pw_subject: &str) -> String { todo!("CLI agent") }

/// Port of pdftk.get_output_stream(): "-" is stdout, "PROMPT" prompts; refuses/asks before
/// overwriting when ask_about_warnings_b. Prints its own error and returns None on failure.
pub fn get_output_stream(_output_filename: &str, _ask_about_warnings_b: bool) -> Option<Box<dyn std::io::Write>> { todo!("CLI agent") }

/// Port of pdftk.get_print_stream(): text output; `utf8_b` false means non-ASCII is XML-entity escaped by the caller.
pub fn get_print_stream(_output_filename: &str, _utf8_b: bool) -> Option<Box<dyn std::io::Write>> { todo!("CLI agent") }

/// Port of pdftk.confirm_overwrite() / file-exists check used by burst & unpack_files.
pub fn confirm_overwrite(_filename: &str) -> bool { todo!("CLI agent") }
