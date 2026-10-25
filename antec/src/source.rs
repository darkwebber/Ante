//! Source files and byte spans.

/// A half-open byte range `start..end` into a source file's text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// A zero-width span, used for synthetic tokens such as NEWLINE or INDENT.
    pub fn point(at: usize) -> Self {
        Span { start: at, end: at }
    }
}

pub struct SourceFile {
    pub name: String,
    pub text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = compute_line_starts(&text);
        SourceFile { name: name.into(), text, line_starts }
    }

    /// 1-based line and column of a byte offset. Columns count Unicode scalar
    /// values from the start of the line (spec §23.4).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let start = self.line_starts[line];
        let col = match self.text.get(start..offset) {
            Some(prefix) => prefix.chars().count(),
            None => offset - start,
        };
        (line + 1, col + 1)
    }

    /// Text of a 1-based line, without its line terminator.
    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1];
        let end = self.line_starts.get(line).copied().unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }
}

/// LF, CRLF and lone CR all end a line (spec §4.1.1).
fn compute_line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => starts.push(i + 1),
            b'\r' => {
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                starts.push(i + 1);
            }
            _ => {}
        }
        i += 1;
    }
    starts
}
