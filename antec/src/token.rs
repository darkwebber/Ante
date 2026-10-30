//! Tokens (spec §4.4–4.6).

use std::fmt;

use crate::source::Span;

macro_rules! keywords {
    ($($name:ident = $text:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Keyword {
            $($name,)*
        }

        impl Keyword {
            pub const ALL: &'static [Keyword] = &[$(Keyword::$name,)*];

            pub fn from_word(word: &str) -> Option<Keyword> {
                match word {
                    $($text => Some(Keyword::$name),)*
                    _ => None,
                }
            }

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Keyword::$name => $text,)*
                }
            }
        }
    };
}

keywords! {
    // Declarations
    Ante = "ante",
    Rakam = "rakam",
    EdhoOkati = "edho_okati",
    Gunam = "gunam",
    Panulu = "panulu",
    Undhi = "undhi",
    Andhariki = "andhariki",
    Bayata = "bayata",
    Thechuko = "thechuko",
    // Bindings and statements
    Maare = "maare",
    Naa = "naa",
    Pettu = "pettu",
    Icchey = "icchey",
    Aapey = "aapey",
    Vadhiley = "vadhiley",
    Vellagane = "vellagane",
    NaaRisk = "naa_risk",
    Theesuko = "theesuko",
    // Control
    Aithe = "aithe",
    Ledhante = "ledhante",
    Unnanthasepu = "unnanthasepu",
    Okko = "okko",
    Batti = "batti",
    // Case markers
    Ni = "ni",
    Ki = "ki",
    Tho = "tho",
    Lo = "lo",
    Nunchi = "nunchi",
    Kosam = "kosam",
    Ga = "ga",
    // Ranges
    Lopu = "lopu",
    Varaku = "varaku",
    // Parameters
    Migathavi = "migathavi",
    MigathaPerlu = "migatha_perlu",
    PeruThone = "peru_thone",
    // Literals and constructors
    Avunu = "avunu",
    Kaadhu = "kaadhu",
    Ledhu = "ledhu",
    Sare = "sare",
    Thappu = "thappu",
    // Types
    Ankam = "ankam",
    Ankam8 = "ankam8",
    Ankam16 = "ankam16",
    Ankam32 = "ankam32",
    Ankam64 = "ankam64",
    Aasajeevi = "aasajeevi",
    Amsam = "amsam",
    Amsam32 = "amsam32",
    Amsam64 = "amsam64",
    AtoIto = "ato_ito",
    Padham = "padham",
    Aksharam = "aksharam",
    Jatha = "jatha",
    Jodi = "jodi",
    Undochu = "undochu",
    Varusa = "varusa",
    Mukka = "mukka",
    SareThappu = "sare_thappu",
    Pani = "pani",
    Pette = "pette",
    Khali = "khali",
    Raadhu = "raadhu",
    Idhi = "idhi",
    Panchukunna = "panchukunna",
    Edhaina = "edhaina",
    // Reserved for later editions
    Kitiki = "kitiki",
    Gurinchi = "gurinchi",
    Async = "async",
    Await = "await",
    Yield = "yield",
}

impl Keyword {
    /// Case markers that can tag an argument phrase (spec §5.1 `role`).
    pub fn is_role(self) -> bool {
        matches!(
            self,
            Keyword::Ni
                | Keyword::Ki
                | Keyword::Tho
                | Keyword::Lo
                | Keyword::Nunchi
                | Keyword::Kosam
                | Keyword::Batti
        )
    }

    /// Words of this edition that `antec` doesn't implement yet (E1202):
    /// `edhaina` (Annex P).
    pub fn is_planned(self) -> bool {
        self.planned_milestone().is_some()
    }

    /// The roadmap milestone (spec Appendix E) that implements a planned word.
    pub fn planned_milestone(self) -> Option<&'static str> {
        match self {
            Keyword::Edhaina => Some("M22"),
            _ => None,
        }
    }

    /// Words reserved for later editions; using them is E1201.
    pub fn is_reserved_for_later(self) -> bool {
        matches!(self, Keyword::Kitiki | Keyword::Gurinchi | Keyword::Async | Keyword::Await | Keyword::Yield)
    }
}

/// An integer literal with separators removed; the value is checked against
/// its type during type checking (spec §10.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntLit {
    pub radix: u32,
    pub digits: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FPart {
    Text(String),
    /// Source span of an interpolated clause; the parser lexes it separately.
    Expr(Span),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    Keyword(Keyword),
    Int(IntLit),
    /// Float literal text with separators removed, ready for exact parsing.
    Float(String),
    Str(String),
    Char(char),
    FStr(Vec<FPart>),
    /// `sql"…"`: lexed like an f-string; its interpolations are query
    /// parameters (Services, §29.3.1).
    SqlStr(Vec<FPart>),
    Underscore,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    At,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Bang,
    Shl,
    Shr,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    Eq,
    Arrow,
    FatArrow,
    PipeGt,
    Question,
    Ellipsis,
    DotDot,
    Dot,
    Comma,
    Colon,
    Semi,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    Newline,
    Indent,
    Dedent,
    Eof,
}

impl TokenKind {
    pub fn punct_str(&self) -> Option<&'static str> {
        use TokenKind::*;
        Some(match self {
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            At => "@",
            Amp => "&",
            Pipe => "|",
            Caret => "^",
            Tilde => "~",
            Bang => "!",
            Shl => "<<",
            Shr => ">>",
            EqEq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            AndAnd => "&&",
            OrOr => "||",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            Eq => "=",
            Arrow => "->",
            FatArrow => "=>",
            PipeGt => "|>",
            Question => "?",
            Ellipsis => "...",
            DotDot => "..",
            Dot => ".",
            Comma => ",",
            Colon => ":",
            Semi => ";",
            LParen => "(",
            RParen => ")",
            LBracket => "[",
            RBracket => "]",
            LBrace => "{",
            RBrace => "}",
            Underscore => "_",
            _ => return None,
        })
    }
}

/// Quotes text for display, escaping only what Ante's own literals would
/// need escaped, so Telugu (including vowel signs) prints as written.
pub fn quote(text: &str, quote: char) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn write_quoted(f: &mut fmt::Formatter<'_>, text: &str, q: char) -> fmt::Result {
    f.write_str(&quote(text, q))
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(p) = self.punct_str() {
            return f.write_str(p);
        }
        match self {
            TokenKind::Ident(name) => write!(f, "id:{name}"),
            TokenKind::Keyword(kw) => write!(f, "kw:{}", kw.as_str()),
            TokenKind::Int(lit) if lit.radix == 10 => write!(f, "int:{}", lit.digits),
            TokenKind::Int(lit) => write!(f, "int:{}:{}", lit.radix, lit.digits),
            TokenKind::Float(text) => write!(f, "float:{text}"),
            TokenKind::Str(value) => {
                f.write_str("str:")?;
                write_quoted(f, value, '"')
            }
            TokenKind::Char(c) => {
                f.write_str("char:")?;
                write_quoted(f, c.encode_utf8(&mut [0; 4]), '\'')
            }
            TokenKind::FStr(parts) | TokenKind::SqlStr(parts) => {
                f.write_str(if matches!(self, TokenKind::FStr(_)) { "fstr[" } else { "sqlstr[" })?;
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    match part {
                        FPart::Text(text) => {
                            f.write_str("text:")?;
                            write_quoted(f, text, '"')?
                        }
                        FPart::Expr(span) => write!(f, "expr@{}..{}", span.start, span.end)?,
                    }
                }
                f.write_str("]")
            }
            TokenKind::Newline => f.write_str("NEWLINE"),
            TokenKind::Indent => f.write_str("INDENT"),
            TokenKind::Dedent => f.write_str("DEDENT"),
            TokenKind::Eof => f.write_str("EOF"),
            _ => unreachable!("punctuation handled above"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
