//! pdftk-rs: a Rust port of pdftk-java (https://gitlab.com/pdftk-java/pdftk).
//! Port of pdftk.java. Owner: CLI agent.
mod attachments;
mod bookmarks;
mod burst;
mod cat;
mod cli;
mod data_import;
mod filter;
mod forms;
mod keyword;
mod passwords;
mod pdf;
mod report;
mod session;
mod stamp;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(cli::main_noexit(&args));
}
