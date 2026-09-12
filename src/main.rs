//! codestego — keyed steganographic watermarking for source code.

use clap::Parser;
use codestego::cli;

fn main() {
    let parsed = cli::Cli::parse();
    match cli::run(parsed) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}
