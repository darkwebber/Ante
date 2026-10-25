//! Diagnostics. Conformance tests compare only the code and the primary span;
//! the message text is free to be colloquial (spec §23.4, Appendix B).

use crate::source::{SourceFile, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Diagnostic { code, severity: Severity::Error, span, message: message.into() }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Diagnostic { code, severity: Severity::Warning, span, message: message.into() }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn render(&self, src: &SourceFile) -> String {
        let (line, col) = src.line_col(self.span.start);
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let (end_line, end_col) = src.line_col(self.span.end);
        let width = if end_line == line && end_col > col { end_col - col } else { 1 };
        let gutter = line.to_string();
        let pad = " ".repeat(gutter.len());
        format!(
            "{severity}[{code}]: {message}\n{pad}--> {file}:{line}:{col}\n{pad} |\n{gutter} | {text}\n{pad} | {indent}{marks}\n",
            code = self.code,
            message = self.message,
            file = src.name,
            text = src.line_text(line),
            indent = " ".repeat(col - 1),
            marks = "^".repeat(width),
        )
    }
}
