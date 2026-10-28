//! Lexer and layout (spec §4).
//!
//! Lexing runs in two passes. `Scanner` turns each physical line into tokens
//! and records its indentation. `layout` then joins physical lines into
//! logical lines (§4.2.3) and emits NEWLINE, INDENT and DEDENT (§4.2.4).

use crate::diag::Diagnostic;
use crate::source::{SourceFile, Span};
use crate::token::{FPart, IntLit, Keyword, Token, TokenKind};

pub struct LexResult {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
    /// Every `# …` comment, for `antec fmt`.
    pub comments: Vec<Span>,
}

pub fn lex(src: &SourceFile) -> LexResult {
    let mut scanner = Scanner::new(&src.text);
    let lines = scanner.scan_lines();
    let mut diagnostics = scanner.diags;
    let tokens = layout(lines, src.text.len(), &mut diagnostics);
    diagnostics.sort_by_key(|d| d.span.start);
    LexResult { tokens, diagnostics, comments: scanner.comments }
}

/// A physical line that contains at least one token.
struct PhysLine {
    indent: usize,
    tokens: Vec<Token>,
}

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    diags: Vec<Diagnostic>,
    /// After a `.` token, digits lex as an integer so `t.0.1` is two indices.
    prev_dot: bool,
    comments: Vec<Span>,
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn digit_group_ok(group: &str) -> bool {
    !group.starts_with('_') && !group.ends_with('_') && !group.contains("__")
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        let pos = if text.starts_with('\u{FEFF}') { '\u{FEFF}'.len_utf8() } else { 0 };
        Scanner {
            text,
            bytes: text.as_bytes(),
            pos,
            diags: Vec::new(),
            prev_dot: false,
            comments: Vec::new(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn next_char(&mut self) -> char {
        let c = self.text[self.pos..].chars().next().expect("not at end of input");
        self.pos += c.len_utf8();
        c
    }

    fn error(&mut self, code: &'static str, start: usize, end: usize, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(code, Span::new(start, end), message));
    }

    fn at_line_end(&self) -> bool {
        matches!(self.peek(), None | Some(b'\n') | Some(b'\r'))
    }

    fn scan_lines(&mut self) -> Vec<PhysLine> {
        let mut lines = Vec::new();
        while self.pos < self.bytes.len() {
            let mut indent = 0;
            let mut tab_at = None;
            while let Some(b @ (b' ' | b'\t')) = self.peek() {
                if b == b'\t' && tab_at.is_none() {
                    tab_at = Some(self.pos);
                }
                indent += 1;
                self.pos += 1;
            }
            let mut tokens = Vec::new();
            loop {
                match self.peek() {
                    None => break,
                    Some(b'\n') => {
                        self.pos += 1;
                        break;
                    }
                    Some(b'\r') => {
                        self.pos += 1;
                        if self.peek() == Some(b'\n') {
                            self.pos += 1;
                        }
                        break;
                    }
                    Some(b' ' | b'\t') => self.pos += 1,
                    Some(b'#') => {
                        let start = self.pos;
                        while !self.at_line_end() {
                            self.pos += 1;
                        }
                        self.comments.push(Span::new(start, self.pos));
                    }
                    Some(_) => {
                        if let Some(tok) = self.scan_token() {
                            self.prev_dot = tok.kind == TokenKind::Dot;
                            tokens.push(tok);
                        }
                    }
                }
            }
            if !tokens.is_empty() {
                if let Some(at) = tab_at {
                    self.error("E0102", at, at + 1, "indentation lo tab vaddhu boss, spaces vaadandi 🙏");
                }
                lines.push(PhysLine { indent, tokens });
            }
        }
        lines
    }

    fn scan_token(&mut self) -> Option<Token> {
        let start = self.pos;
        let b = self.peek()?;
        let kind = if b.is_ascii_alphabetic() || b == b'_' {
            self.scan_word()
        } else if b.is_ascii_digit() {
            self.scan_number()
        } else if b == b'"' {
            TokenKind::Str(self.scan_string())
        } else if b == b'\'' {
            self.scan_char()
        } else if let Some(kind) = self.scan_punct() {
            kind
        } else {
            self.invalid_char();
            return None;
        };
        Some(Token { kind, span: Span::new(start, self.pos) })
    }

    fn scan_word(&mut self) -> TokenKind {
        let text = self.text;
        let start = self.pos;
        while matches!(self.peek(), Some(b) if is_word_byte(b)) {
            self.pos += 1;
        }
        let word = &text[start..self.pos];
        if word == "f" && self.peek() == Some(b'"') {
            return TokenKind::FStr(self.scan_fstring(1));
        }
        if word == "sql" && self.peek() == Some(b'"') {
            return TokenKind::SqlStr(self.scan_fstring(3));
        }
        if word == "_" {
            return TokenKind::Underscore;
        }
        match Keyword::from_word(word) {
            Some(kw) => TokenKind::Keyword(kw),
            None => TokenKind::Ident(word.to_string()),
        }
    }

    fn eat_decimal_digits(&mut self) {
        while matches!(self.peek(), Some(b) if b.is_ascii_digit() || b == b'_') {
            self.pos += 1;
        }
    }

    fn scan_number(&mut self) -> TokenKind {
        let text = self.text;
        let start = self.pos;

        let radix = match (self.peek(), self.peek_at(1)) {
            (Some(b'0'), Some(b'b')) => 2,
            (Some(b'0'), Some(b'o')) => 8,
            (Some(b'0'), Some(b'x')) => 16,
            _ => 10,
        };
        if radix != 10 {
            self.pos += 2;
            let digits_start = self.pos;
            while matches!(self.peek(), Some(b) if is_word_byte(b)) {
                self.pos += 1;
            }
            let raw = &text[digits_start..self.pos];
            let ok =
                !raw.is_empty() && digit_group_ok(raw) && raw.chars().all(|c| c == '_' || c.is_digit(radix));
            if !ok {
                self.error(
                    "E0106",
                    start,
                    self.pos,
                    format!("`{}` base-{radix} number laaga ledhu", &text[start..self.pos]),
                );
            }
            return TokenKind::Int(IntLit { radix, digits: raw.replace('_', "") });
        }

        self.eat_decimal_digits();
        let mut is_float = false;
        if !self.prev_dot
            && self.peek() == Some(b'.')
            && matches!(self.peek_at(1), Some(b) if b.is_ascii_digit())
        {
            self.pos += 1;
            self.eat_decimal_digits();
            is_float = true;
        }
        if !self.prev_dot && matches!(self.peek(), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.peek_at(1), Some(b'+' | b'-')));
            if matches!(self.peek_at(1 + sign), Some(b) if b.is_ascii_digit()) {
                self.pos += 1 + sign;
                self.eat_decimal_digits();
                is_float = true;
            }
        }

        let mut ok = true;
        if matches!(self.peek(), Some(b) if is_word_byte(b)) {
            while matches!(self.peek(), Some(b) if is_word_byte(b)) {
                self.pos += 1;
            }
            ok = false;
        }
        let raw = &text[start..self.pos];
        if ok {
            ok = raw.split(['.', 'e', 'E', '+', '-']).filter(|group| !group.is_empty()).all(digit_group_ok);
        }
        if !ok {
            self.error(
                "E0106",
                start,
                self.pos,
                format!("`{raw}` number laaga ledhu. `_` digits madhyalo matrame, okkatokkate pettandi"),
            );
        }
        let clean = raw.replace('_', "");
        if is_float {
            TokenKind::Float(clean)
        } else {
            TokenKind::Int(IntLit { radix: 10, digits: clean })
        }
    }

    /// Scans an escape starting at a backslash. Returns `None` after reporting
    /// an error, or when the line ends right after the backslash (the caller
    /// then reports the unterminated literal).
    fn scan_escape(&mut self) -> Option<char> {
        let start = self.pos;
        self.pos += 1;
        if self.at_line_end() {
            return None;
        }
        let c = self.next_char();
        match c {
            '\\' => Some('\\'),
            '"' => Some('"'),
            '\'' => Some('\''),
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            '0' => Some('\0'),
            'u' => self.scan_unicode_escape(start),
            other => {
                self.error(
                    "E0105",
                    start,
                    self.pos,
                    format!(
                        "`\\{other}` ane escape ledhu. Ivi vaadochu: \\n \\t \\r \\0 \\\\ \\\" \\' \\u{{…}}"
                    ),
                );
                None
            }
        }
    }

    fn scan_unicode_escape(&mut self, start: usize) -> Option<char> {
        let text = self.text;
        if self.peek() != Some(b'{') {
            self.error(
                "E0105",
                start,
                self.pos,
                "`\\u` tharvatha `{hex digits}` raavali, ex: `\\u{C24}` = త",
            );
            return None;
        }
        self.pos += 1;
        let hex_start = self.pos;
        while matches!(self.peek(), Some(b) if b.is_ascii_hexdigit()) {
            self.pos += 1;
        }
        let hex = &text[hex_start..self.pos];
        if self.peek() != Some(b'}') || hex.is_empty() || hex.len() > 6 {
            self.error("E0105", start, self.pos, "`\\u{…}` lo 1 to 6 hex digits undali");
            return None;
        }
        self.pos += 1;
        let value = u32::from_str_radix(hex, 16).expect("validated hex digits");
        match char::from_u32(value) {
            Some(c) => Some(c),
            None => {
                self.error(
                    "E0108",
                    start,
                    self.pos,
                    format!("`\\u{{{hex}}}` valid Unicode character kaadhu"),
                );
                None
            }
        }
    }

    fn scan_string(&mut self) -> String {
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => {
                    self.error(
                        "E0104",
                        start,
                        self.pos,
                        "ee string ki closing `\"` ledhu. Line lo ne close cheyyandi",
                    );
                    break;
                }
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    if let Some(c) = self.scan_escape() {
                        value.push(c);
                    }
                }
                Some(_) => value.push(self.next_char()),
            }
        }
        value
    }

    fn scan_char(&mut self) -> TokenKind {
        let start = self.pos;
        self.pos += 1;
        let mut chars = Vec::new();
        let mut closed = false;
        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => {
                    self.error("E0104", start, self.pos, "ee character ki closing `'` ledhu");
                    break;
                }
                Some(b'\'') => {
                    self.pos += 1;
                    closed = true;
                    break;
                }
                Some(b'\\') => chars.push(self.scan_escape().unwrap_or('\u{FFFD}')),
                Some(_) => chars.push(self.next_char()),
            }
        }
        if closed && chars.len() != 1 {
            self.error(
                "E0109",
                start,
                self.pos,
                "`'…'` lo exactly oka character undali. Text kosam \"…\" vaadandi",
            );
        }
        TokenKind::Char(chars.first().copied().unwrap_or('\u{FFFD}'))
    }

    /// Scans an f-string; `self.pos` is at the `"` after the `f`.
    /// An f-string (or `sql"…"`) whose `prefix`-byte prefix was just read.
    fn scan_fstring(&mut self, prefix: usize) -> Vec<FPart> {
        let text_src = self.text;
        let start = self.pos - prefix;
        self.pos += 1;
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n' | b'\r') => {
                    self.error("E0104", start, self.pos, "ee f-string ki closing `\"` ledhu");
                    break;
                }
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    if let Some(c) = self.scan_escape() {
                        text.push(c);
                    }
                }
                Some(b'{') if self.peek_at(1) == Some(b'{') => {
                    text.push('{');
                    self.pos += 2;
                }
                Some(b'}') if self.peek_at(1) == Some(b'}') => {
                    text.push('}');
                    self.pos += 2;
                }
                Some(b'}') => {
                    self.error(
                        "E0110",
                        self.pos,
                        self.pos + 1,
                        "f-string lo okka `}` kaavalante `}}` ani raayandi",
                    );
                    self.pos += 1;
                }
                Some(b'{') => {
                    if !text.is_empty() {
                        parts.push(FPart::Text(std::mem::take(&mut text)));
                    }
                    let open = self.pos;
                    self.pos += 1;
                    let expr_start = self.pos;
                    let mut depth = 1;
                    let mut closed = false;
                    while let Some(b) = self.peek() {
                        match b {
                            b'\n' | b'\r' | b'"' => break,
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    closed = true;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        self.pos += 1;
                    }
                    if !closed {
                        // A `"` inside the braces: a string literal, which an
                        // interpolation can't hold (§4.5).
                        let msg = if self.peek() == Some(b'"') {
                            "f-string `{…}` lopala string literal (`\"…\"`) raayakoodadhu. Mundhu daanini oka variable lo pettandi, `{peru}` ga vaadandi"
                        } else {
                            "ee `{` open chesaru, kaani `}` tho close cheyyaledhu"
                        };
                        self.error("E0110", open, self.pos, msg);
                        continue;
                    }
                    if text_src[expr_start..self.pos].trim().is_empty() {
                        self.error(
                            "E0110",
                            open,
                            self.pos + 1,
                            "`{}` lopala emi ledhu, edhaina expression pettandi",
                        );
                    } else {
                        parts.push(FPart::Expr(Span::new(expr_start, self.pos)));
                    }
                    self.pos += 1;
                }
                Some(_) => text.push(self.next_char()),
            }
        }
        if !text.is_empty() {
            parts.push(FPart::Text(text));
        }
        parts
    }

    fn scan_punct(&mut self) -> Option<TokenKind> {
        use TokenKind::*;
        let (kind, len) = match (self.peek()?, self.peek_at(1), self.peek_at(2)) {
            (b'.', Some(b'.'), Some(b'.')) => (Ellipsis, 3),
            (b'.', Some(b'.'), _) => (DotDot, 2),
            (b'<', Some(b'<'), _) => (Shl, 2),
            (b'>', Some(b'>'), _) => (Shr, 2),
            (b'=', Some(b'='), _) => (EqEq, 2),
            (b'!', Some(b'='), _) => (NotEq, 2),
            (b'<', Some(b'='), _) => (Le, 2),
            (b'>', Some(b'='), _) => (Ge, 2),
            (b'&', Some(b'&'), _) => (AndAnd, 2),
            (b'|', Some(b'|'), _) => (OrOr, 2),
            (b'|', Some(b'>'), _) => (PipeGt, 2),
            (b'+', Some(b'='), _) => (PlusEq, 2),
            (b'-', Some(b'='), _) => (MinusEq, 2),
            (b'*', Some(b'='), _) => (StarEq, 2),
            (b'/', Some(b'='), _) => (SlashEq, 2),
            (b'-', Some(b'>'), _) => (Arrow, 2),
            (b'=', Some(b'>'), _) => (FatArrow, 2),
            (b'+', _, _) => (Plus, 1),
            (b'-', _, _) => (Minus, 1),
            (b'*', _, _) => (Star, 1),
            (b'/', _, _) => (Slash, 1),
            (b'%', _, _) => (Percent, 1),
            (b'@', _, _) => (At, 1),
            (b'&', _, _) => (Amp, 1),
            (b'|', _, _) => (Pipe, 1),
            (b'^', _, _) => (Caret, 1),
            (b'~', _, _) => (Tilde, 1),
            (b'!', _, _) => (Bang, 1),
            (b'<', _, _) => (Lt, 1),
            (b'>', _, _) => (Gt, 1),
            (b'=', _, _) => (Eq, 1),
            (b'?', _, _) => (Question, 1),
            (b'.', _, _) => (Dot, 1),
            (b',', _, _) => (Comma, 1),
            (b':', _, _) => (Colon, 1),
            (b';', _, _) => (Semi, 1),
            (b'(', _, _) => (LParen, 1),
            (b')', _, _) => (RParen, 1),
            (b'[', _, _) => (LBracket, 1),
            (b']', _, _) => (RBracket, 1),
            (b'{', _, _) => (LBrace, 1),
            (b'}', _, _) => (RBrace, 1),
            _ => return None,
        };
        self.pos += len;
        Some(kind)
    }

    fn invalid_char(&mut self) {
        let start = self.pos;
        let c = self.next_char();
        if c == '\\' {
            self.error(
                "E0201",
                start,
                self.pos,
                "backslash tho line continue avvadhu. Brackets lopala, `,` or `|>` tharvatha, \
                 or next line ekkuva indent chesthe, lines automatic ga join avthayi",
            );
        } else if !c.is_ascii() {
            while matches!(self.peek(), Some(b) if !b.is_ascii()) {
                self.next_char();
            }
            let text = &self.text[start..self.pos];
            self.error(
                "E0101",
                start,
                self.pos,
                format!("`{text}` strings lo or comments lo matrame raayochu. Code lo ippatiki English letters matrame 🙂"),
            );
        } else {
            self.error("E0101", start, self.pos, format!("`{c}` ki ikkada pani ledhu"));
        }
    }
}

fn closes(open: &TokenKind, close: &TokenKind) -> bool {
    matches!(
        (open, close),
        (TokenKind::LParen, TokenKind::RParen)
            | (TokenKind::LBracket, TokenKind::RBracket)
            | (TokenKind::LBrace, TokenKind::RBrace)
    )
}

fn dedent_to(
    indent: usize,
    at: Span,
    indents: &mut Vec<usize>,
    out: &mut Vec<Token>,
    diags: &mut Vec<Diagnostic>,
) {
    while indent < *indents.last().expect("indent stack is never empty") {
        indents.pop();
        out.push(Token { kind: TokenKind::Dedent, span: at });
    }
    if indent != *indents.last().expect("indent stack is never empty") {
        diags.push(Diagnostic::error(
            "E0103",
            at,
            "ee line indentation e block tho match avvatledhu. Spaces lekka choodandi 🔍",
        ));
    }
}

fn layout(lines: Vec<PhysLine>, eof: usize, diags: &mut Vec<Diagnostic>) -> Vec<Token> {
    let mut out = Vec::new();
    let mut indents = vec![0usize];
    let mut brackets: Vec<Token> = Vec::new();
    let mut pending_suite: Option<Span> = None;
    let mut lines = lines.into_iter().peekable();

    while let Some(first) = lines.next() {
        let indent = first.indent;
        let at = Span::point(first.tokens[0].span.start);
        let top = *indents.last().expect("indent stack is never empty");
        match pending_suite.take() {
            Some(_) if indent > top => {
                indents.push(indent);
                out.push(Token { kind: TokenKind::Indent, span: at });
            }
            Some(colon) => {
                diags.push(Diagnostic::error(
                    "E0201",
                    colon,
                    "ee `:` tharvatha indented block raavali, kaani raaledhu",
                ));
                dedent_to(indent, at, &mut indents, &mut out, diags);
            }
            None if indent > top => {
                diags.push(Diagnostic::error(
                    "E0201",
                    at,
                    "ikkada indent enduku? Ee line lo extra spaces unnayi",
                ));
            }
            None => dedent_to(indent, at, &mut indents, &mut out, diags),
        }

        // Join physical lines into one logical line (§4.2.3).
        let logical_indent = indent;
        let mut line = first;
        let last = loop {
            let last = line.tokens.last().expect("lines have tokens").clone();
            for tok in line.tokens {
                match tok.kind {
                    TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => brackets.push(tok.clone()),
                    TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => match brackets.pop() {
                        Some(open) if closes(&open.kind, &tok.kind) => {}
                        Some(open) => diags.push(Diagnostic::error(
                            "E0201",
                            tok.span,
                            format!(
                                "`{}` tho `{}` close avvadhu. Brackets pair ayyela choodandi",
                                tok.kind, open.kind
                            ),
                        )),
                        None => diags.push(Diagnostic::error(
                            "E0201",
                            tok.span,
                            "ee closing bracket ki opening ledhu",
                        )),
                    },
                    _ => {}
                }
                out.push(tok);
            }
            let joins = match lines.peek() {
                None => false,
                Some(next) => {
                    !brackets.is_empty()
                        || matches!(last.kind, TokenKind::Comma | TokenKind::PipeGt)
                        || next.tokens[0].kind == TokenKind::PipeGt
                        || (next.indent > logical_indent && last.kind != TokenKind::Colon)
                }
            };
            if !joins {
                break last;
            }
            line = lines.next().expect("peeked line exists");
        };
        out.push(Token { kind: TokenKind::Newline, span: Span::point(last.span.end) });
        if last.kind == TokenKind::Colon && brackets.is_empty() {
            pending_suite = Some(last.span);
        }
    }

    for open in &brackets {
        diags.push(Diagnostic::error("E0201", open.span, "ee bracket open chesaru, close cheyyaledhu"));
    }
    if let Some(colon) = pending_suite {
        diags.push(Diagnostic::error(
            "E0201",
            colon,
            "ee `:` tharvatha indented block raavali, kaani raaledhu",
        ));
    }
    let end = Span::point(eof);
    for _ in 1..indents.len() {
        out.push(Token { kind: TokenKind::Dedent, span: end });
    }
    out.push(Token { kind: TokenKind::Eof, span: end });
    out
}
