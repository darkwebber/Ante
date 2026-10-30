//! Positions. The compiler counts bytes; LSP counts lines and UTF-16 code
//! units within a line, so a Telugu letter (3 bytes) is 1 unit and an emoji
//! (4 bytes) is 2.

use super::json::{obj, Json};
use crate::source::Span;

/// Where each line of a text starts. LF, CRLF and lone CR end a line.
pub struct Lines<'t> {
    text: &'t str,
    starts: Vec<usize>,
}

impl<'t> Lines<'t> {
    pub fn new(text: &'t str) -> Self {
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
        Lines { text, starts }
    }

    /// The byte range of line `line` without its terminator.
    fn line_range(&self, line: usize) -> (usize, usize) {
        let start = self.starts[line];
        let end = self.starts.get(line + 1).copied().unwrap_or(self.text.len());
        let body = self.text[start..end].trim_end_matches(['\n', '\r']);
        (start, start + body.len())
    }

    /// `(line, utf16 column)` of a byte offset; offsets inside a character
    /// count from its start.
    pub fn position(&self, offset: usize) -> (usize, usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.starts.partition_point(|&s| s <= offset) - 1;
        let (start, end) = self.line_range(line);
        let col = self.text[start..offset.min(end)].encode_utf16().count();
        (line, col)
    }

    /// The byte offset of `(line, utf16 column)`, clamped to the text and
    /// to the line's end. A column inside a surrogate pair rounds down.
    pub fn offset(&self, line: usize, col: usize) -> usize {
        let Some(_) = self.starts.get(line) else { return self.text.len() };
        let (start, end) = self.line_range(line);
        let mut units = 0;
        for (i, c) in self.text[start..end].char_indices() {
            let next = units + c.len_utf16();
            if next > col {
                return start + i;
            }
            units = next;
        }
        end
    }

    pub fn range(&self, span: Span) -> Json {
        range_json(self.position(span.start), self.position(span.end.max(span.start)))
    }

    /// The range covering the whole text.
    pub fn whole(&self) -> Json {
        range_json((0, 0), self.position(self.text.len()))
    }
}

pub fn range_json(start: (usize, usize), end: (usize, usize)) -> Json {
    obj([("start", pos_json(start)), ("end", pos_json(end))])
}

fn pos_json((line, col): (usize, usize)) -> Json {
    obj([("line", line.into()), ("character", col.into())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_round_trip() {
        let text = "a ante \"తెలుగు 🚀\"\r\nb\rc\n";
        let lines = Lines::new(text);
        let rocket = text.find('🚀').unwrap();
        // `a ante "` is 8 units, `తెలుగు ` is 7 more.
        assert_eq!(lines.position(rocket), (0, 15));
        assert_eq!(lines.offset(0, 15), rocket);
        assert_eq!(lines.offset(0, 16), rocket, "inside the surrogate pair");
        assert_eq!(lines.position(rocket + 4), (0, 17));
        assert_eq!(lines.position(text.find('b').unwrap()), (1, 0));
        assert_eq!(lines.position(text.find('c').unwrap()), (2, 0));
        assert_eq!(lines.offset(0, 999), text.find('\r').unwrap());
        assert_eq!(lines.offset(9, 0), text.len());
        assert_eq!(lines.position(rocket + 1), (0, 15), "inside a character");
    }
}
