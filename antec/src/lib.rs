//! `antec`, the stage-0 compiler for Ante, written in Rust.
//! Milestone M1: source handling, lexer and layout (spec Part II).

pub mod diag;
pub mod lexer;
pub mod source;
pub mod token;

use std::path::Path;

use diag::Diagnostic;
use source::{SourceFile, Span};

/// Builds a source file from raw bytes. Invalid UTF-8 is E0101; the file then
/// holds only the valid prefix so the error position can still be reported.
pub fn source_from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> (SourceFile, Vec<Diagnostic>) {
    match String::from_utf8(bytes) {
        Ok(text) => (SourceFile::new(name, text), Vec::new()),
        Err(err) => {
            let valid = err.utf8_error().valid_up_to();
            let mut bytes = err.into_bytes();
            bytes.truncate(valid);
            let text = String::from_utf8(bytes).expect("prefix is valid UTF-8");
            let diag = Diagnostic::error(
                "E0101",
                Span::point(valid),
                "ee file valid UTF-8 kaadhu. Editor lo UTF-8 ga save cheyyandi",
            );
            (SourceFile::new(name, text), vec![diag])
        }
    }
}

pub fn load_source(path: &Path) -> std::io::Result<(SourceFile, Vec<Diagnostic>)> {
    let bytes = std::fs::read(path)?;
    Ok(source_from_bytes(path.display().to_string(), bytes))
}

/// Runs every compiler stage implemented so far and returns its diagnostics.
pub fn check_source(src: &SourceFile) -> Vec<Diagnostic> {
    lexer::lex(src).diagnostics
}
