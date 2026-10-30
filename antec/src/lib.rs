//! `antec`, the stage-0 compiler for "Ante", written in Rust.
//! Milestones: M1 lexer and layout (spec Part II), M2 parser (Part III),
//! M3 names and modules (§7–8), M4 types (§9–14).

pub mod ast;
pub mod codegen;
pub mod derive;
pub mod diag;
pub mod fmt;
pub mod interp;
pub mod lexer;
pub mod lint;
pub mod mir;
pub mod package;
pub mod parser;
pub mod pretty;
pub mod resolve;
pub mod source;
pub mod stdlib;
pub mod token;
pub mod typeck;
pub mod types;

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

/// The stack `antec` runs on. Parsing and checking recurse over a program's
/// nesting, which the parser bounds (E0216), so this is always enough; it's
/// only reserved, not used, until needed.
pub const COMPILER_STACK: usize = 256 << 20;

/// Runs `f` on a thread with `COMPILER_STACK` bytes of stack. Embedders
/// (tests, editors) should compile through this too, since the default
/// thread stack is small.
pub fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(COMPILER_STACK)
            .spawn_scoped(s, f)
            .expect("antec thread start avvaledhu")
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e))
    })
}

/// Lexes and parses a file. Parsing is skipped when lexing reports errors,
/// so one mistake doesn't cascade into many.
pub fn parse_source(src: &SourceFile) -> (Option<ast::Module>, Vec<Diagnostic>) {
    let lexed = lexer::lex(src);
    if lexed.diagnostics.iter().any(Diagnostic::is_error) {
        return (None, lexed.diagnostics);
    }
    let parsed = parser::parse(src, lexed.tokens);
    let mut diags = lexed.diagnostics;
    diags.extend(parsed.diagnostics);
    (Some(parsed.module), diags)
}

/// Runs every compiler stage implemented so far on a lone file and returns
/// its diagnostics. Use `package::check_path` for packages.
pub fn check_source(src: &SourceFile) -> Vec<Diagnostic> {
    package::check_single(src)
}
