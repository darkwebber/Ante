//! A small JSON value with a parser and a writer: just what the language
//! server's messages need, so `antec` keeps no dependencies.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Keys in the order written.
    Obj(Vec<(String, Json)>),
}

pub static NULL: Json = Json::Null;

impl Json {
    /// The member `key` of an object; `Null` when missing.
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { bytes: text.as_bytes(), pos: 0, depth: 0 };
        let value = p.value()?;
        p.ws();
        if p.pos != p.bytes.len() {
            return Err(format!("extra text at byte {}", p.pos));
        }
        Ok(value)
    }

    pub fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else if n.is_finite() {
                    let _ = write!(out, "{n}");
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_str(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Json::Obj(members) => {
                out.push('{');
                for (i, (k, v)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        self.write(&mut s);
        f.write_str(&s)
    }
}

/// An object from `(key, value)` pairs.
pub fn obj<const N: usize>(members: [(&str, Json); N]) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}

impl From<usize> for Json {
    fn from(n: usize) -> Json {
        Json::Num(n as f64)
    }
}

impl From<u32> for Json {
    fn from(n: u32) -> Json {
        Json::Num(n as f64)
    }
}

impl From<i64> for Json {
    fn from(n: i64) -> Json {
        Json::Num(n as f64)
    }
}

impl From<Vec<Json>> for Json {
    fn from(items: Vec<Json>) -> Json {
        Json::Arr(items)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Nesting deeper than this is rejected rather than risking the stack.
const MAX_DEPTH: usize = 512;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.bytes.get(self.pos).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.pos))
    }

    fn eat(&mut self, word: &str) -> bool {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let Some(&b) = self.bytes.get(self.pos) else { return self.err("unexpected end") };
        match b {
            b'n' if self.eat("null") => Ok(Json::Null),
            b't' if self.eat("true") => Ok(Json::Bool(true)),
            b'f' if self.eat("false") => Ok(Json::Bool(false)),
            b'"' => self.string().map(Json::Str),
            b'[' | b'{' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return self.err("nesting too deep");
                }
                let v = if b == b'[' { self.array() } else { self.object() };
                self.depth -= 1;
                v
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => self.err("unexpected character"),
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.eat("]") {
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            if self.eat(",") {
                continue;
            }
            if self.eat("]") {
                return Ok(Json::Arr(items));
            }
            return self.err("expected `,` or `]`");
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.pos += 1;
        let mut members = Vec::new();
        self.ws();
        if self.eat("}") {
            return Ok(Json::Obj(members));
        }
        loop {
            self.ws();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return self.err("expected a key");
            }
            let key = self.string()?;
            self.ws();
            if !self.eat(":") {
                return self.err("expected `:`");
            }
            members.push((key, self.value()?));
            self.ws();
            if self.eat(",") {
                continue;
            }
            if self.eat("}") {
                return Ok(Json::Obj(members));
            }
            return self.err("expected `,` or `}`");
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
        match text.parse::<f64>() {
            Ok(n) => Ok(Json::Num(n)),
            Err(_) => self.err("bad number"),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.bytes.get(self.pos..self.pos + 4).and_then(|d| std::str::from_utf8(d).ok());
        match digits.and_then(|d| u32::from_str_radix(d, 16).ok()) {
            Some(n) => {
                self.pos += 4;
                Ok(n)
            }
            None => self.err("bad `\\u` escape"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let start = self.pos;
            while self.bytes.get(self.pos).is_some_and(|&b| b != b'"' && b != b'\\') {
                self.pos += 1;
            }
            match std::str::from_utf8(&self.bytes[start..self.pos]) {
                Ok(s) => out.push_str(s),
                Err(_) => return self.err("invalid UTF-8"),
            }
            match self.bytes.get(self.pos) {
                None => return self.err("unterminated string"),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(_) => {
                    self.pos += 1;
                    let Some(&e) = self.bytes.get(self.pos) else { return self.err("unterminated string") };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) && self.eat("\\u") {
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                } else {
                                    0xFFFD
                                }
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => return self.err("bad escape"),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let text = r#"{"a":[1,2.5,-3],"b":"x\"y\\z\n","c":null,"d":true,"e":{}}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.to_string(), text);
    }

    #[test]
    fn escapes_and_surrogates() {
        let v = Json::parse(r#""అం 🚀 తెలుగు""#).unwrap();
        assert_eq!(v.str(), Some("అం 🚀 తెలుగు"));
        assert!(Json::parse("[1,").is_err());
        assert!(Json::parse(&"[".repeat(10_000)).is_err());
    }
}
