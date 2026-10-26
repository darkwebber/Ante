//! Parser (spec §5) with the disambiguation rules of §6.
//!
//! Hand-written recursive descent over the token stream from the lexer.
//! Statements and items are classified by looking at their whole logical
//! line first (D1, D2), so most decisions need no backtracking. The two
//! places that do backtrack locally are commas inside brackets (D4) and the
//! `nunchi … lopu` range lookahead (D5).

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::lexer;
use crate::source::{SourceFile, Span};
use crate::token::{FPart, Keyword, Token, TokenKind};

type PResult<T> = Result<T, ()>;

pub struct ParseResult {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(src: &SourceFile, tokens: Vec<Token>) -> ParseResult {
    let mut p = Parser::new(src, tokens);
    let module = p.parse_module();
    p.diags.extend(p.deep.take());
    p.diags.sort_by_key(|d| d.span.start);
    ParseResult { module, diagnostics: p.diags }
}

fn is_type_keyword(kw: Keyword) -> bool {
    use Keyword::*;
    matches!(
        kw,
        Ankam
            | Ankam8
            | Ankam16
            | Ankam32
            | Ankam64
            | Amsam
            | Amsam32
            | Amsam64
            | AtoIto
            | Padham
            | Aksharam
            | Jatha
            | Jodi
            | Undochu
            | Varusa
            | Mukka
            | SareThappu
            | Pette
            | Khali
            | Raadhu
            | Idhi
    )
}

fn role_of(kw: Keyword) -> Option<Role> {
    Some(match kw {
        Keyword::Ni => Role::Ni,
        Keyword::Ki => Role::Ki,
        Keyword::Tho => Role::Tho,
        Keyword::Lo => Role::Lo,
        Keyword::Nunchi => Role::Nunchi,
        Keyword::Kosam => Role::Kosam,
        Keyword::Batti => Role::Batti,
        _ => return None,
    })
}

fn compound_op(kind: &TokenKind) -> Option<BinOp> {
    Some(match kind {
        TokenKind::PlusEq => BinOp::Add,
        TokenKind::MinusEq => BinOp::Sub,
        TokenKind::StarEq => BinOp::Mul,
        TokenKind::SlashEq => BinOp::Div,
        _ => return None,
    })
}

/// Binary operators and their binding power (spec §5.4); higher binds tighter.
fn binop_info(kind: &TokenKind) -> Option<(BinOp, u8)> {
    use TokenKind::*;
    Some(match kind {
        Star => (BinOp::Mul, 10),
        Slash => (BinOp::Div, 10),
        Percent => (BinOp::Rem, 10),
        At => (BinOp::MatMul, 10),
        Plus => (BinOp::Add, 9),
        Minus => (BinOp::Sub, 9),
        Shl => (BinOp::Shl, 8),
        Shr => (BinOp::Shr, 8),
        Amp => (BinOp::BitAnd, 7),
        Caret => (BinOp::BitXor, 6),
        Pipe => (BinOp::BitOr, 5),
        Lt => (BinOp::Lt, 4),
        Le => (BinOp::Le, 4),
        Gt => (BinOp::Gt, 4),
        Ge => (BinOp::Ge, 4),
        EqEq => (BinOp::Eq, 3),
        NotEq => (BinOp::Ne, 3),
        AndAnd => (BinOp::And, 2),
        OrOr => (BinOp::Or, 1),
        _ => return None,
    })
}

/// Human-readable name of a token for error messages.
fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Ident(name) => format!("`{name}`"),
        TokenKind::Keyword(kw) => format!("`{}`", kw.as_str()),
        TokenKind::Int(_) | TokenKind::Float(_) => "number".into(),
        TokenKind::Str(_) | TokenKind::FStr(_) => "string".into(),
        TokenKind::Char(_) => "character".into(),
        TokenKind::Newline => "line end".into(),
        TokenKind::Indent => "extra indentation".into(),
        TokenKind::Dedent => "block end".into(),
        TokenKind::Eof => "file end".into(),
        other => format!("`{}`", other.punct_str().unwrap_or("?")),
    }
}

fn join(a: Span, b: Span) -> Span {
    Span::new(a.start, b.end.max(a.end))
}

fn mk(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span }
}

/// Number of top-level `_` phrases in a call (or a `?`-wrapped call).
fn count_holes(e: &Expr) -> usize {
    match &e.kind {
        ExprKind::Call { phrases, .. } => {
            phrases.iter().filter(|p| matches!(p.kind, PhraseKind::Hole)).count()
        }
        ExprKind::Try(inner) => count_holes(inner),
        _ => 0,
    }
}

fn is_call(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Call { .. } => true,
        ExprKind::Try(inner) => is_call(inner),
        _ => false,
    }
}

fn is_place(e: &Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Path(_)
            | ExprKind::Field { .. }
            | ExprKind::TupleIndex { .. }
            | ExprKind::Index { .. }
            | ExprKind::Unary { op: UnOp::Deref, .. }
    )
}

/// How deeply brackets, unary operators, blocks, types and patterns may
/// nest. The compiler walks these recursively, so without a limit a
/// hostile file could overflow its stack (implementation-defined, §20.6).
const MAX_NESTING: usize = 512;

/// How deep one expression's syntax tree may get, counting operator chains
/// like `a + b + c` (each operator is one level).
const MAX_EXPR_DEPTH: usize = 1000;

struct Parser<'a> {
    src: &'a SourceFile,
    toks: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    loop_depth: usize,
    defer_depth: usize,
    index_depth: usize,
    /// Current recursion depth, against `MAX_NESTING`.
    nesting: usize,
    /// Nesting of `parse_pipe_expr_with`: the outermost checks the depth.
    expr_level: usize,
    /// The E0216 report, kept apart from `diags` so backtracking can't drop it.
    deep: Option<Diagnostic>,
    /// List elements that failed to parse, by (position, commas), with the
    /// errors they reported: retrying them gives the same result, and without
    /// this nested brackets would be retried exponentially often.
    failed_elems: std::collections::HashMap<(usize, bool), Vec<Diagnostic>>,
    /// Where to resume after an item error that already skipped its line.
    resume: Option<usize>,
    /// Set when the nesting limit is hit: everything fails fast until the
    /// next line recovery, so alternatives aren't retried exponentially.
    bail: bool,
}

impl<'a> Parser<'a> {
    fn new(src: &'a SourceFile, toks: Vec<Token>) -> Self {
        let mut p = Parser {
            src,
            toks,
            pos: 0,
            diags: Vec::new(),
            loop_depth: 0,
            defer_depth: 0,
            index_depth: 0,
            nesting: 0,
            expr_level: 0,
            deep: None,
            failed_elems: Default::default(),
            resume: None,
            bail: false,
        };
        for tok in &p.toks {
            if let TokenKind::Keyword(kw) = tok.kind {
                if kw.is_reserved_for_later() {
                    p.diags.push(Diagnostic::error(
                        "E1201",
                        tok.span,
                        format!(
                            "`{}` ee edition lo ledhu, taruvatha editions kosam reserve chesam",
                            kw.as_str()
                        ),
                    ));
                }
            }
        }
        p
    }

    /// Runs `f` one nesting level deeper, or reports E0216 at the limit.
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        if self.bail {
            return Err(());
        }
        if self.nesting >= MAX_NESTING {
            self.too_deep_here();
            self.bail = true;
            return Err(());
        }
        self.nesting += 1;
        let result = f(self);
        self.nesting -= 1;
        result
    }

    fn too_deep_here(&mut self) {
        let span = self.span();
        self.report_deep(span, "code chaala lothuga nest ayyindhi");
    }

    fn report_deep(&mut self, span: Span, what: &str) {
        if self.deep.is_none() {
            self.deep = Some(Diagnostic::error(
                "E0216",
                span,
                format!("{what} 🌀 Konchem chinna mukkalu ga vidagottandi (madhyalo variables pettandi)"),
            ));
        }
    }

    // ── token helpers ──────────────────────────────────────────────────────

    fn peek(&self) -> &TokenKind {
        &self.toks[self.pos].kind
    }

    fn peek_at(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].kind
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.toks[self.pos - 1].span.end
        }
    }

    fn since(&self, start: Span) -> Span {
        Span::new(start.start, self.prev_end().max(start.start))
    }

    fn advance(&mut self) -> Token {
        let tok = self.toks[self.pos].clone();
        if tok.kind != TokenKind::Eof {
            self.pos += 1;
        }
        tok
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn at_kw(&self, kw: Keyword) -> bool {
        matches!(self.peek(), TokenKind::Keyword(k) if *k == kw)
    }

    fn kw_at(&self, n: usize, kw: Keyword) -> bool {
        matches!(self.peek_at(n), TokenKind::Keyword(k) if *k == kw)
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Keyword) -> bool {
        if self.at_kw(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(code, span, message));
    }

    fn unexpected(&mut self, expected: &str) {
        let tok = self.toks[self.pos].clone();
        let found = describe(&tok.kind);
        match tok.kind {
            TokenKind::Semi => {
                self.error("E0206", tok.span, "`;` vaddhu. Prathi statement ki oka line, anthe")
            }
            TokenKind::Keyword(Keyword::Ga) => self.error(
                "E0205",
                tok.span,
                "`ga` ikkada raakoodadhu. Named argument ki `value ni peru ga` ani raayandi",
            ),
            _ => self.error("E0201", tok.span, format!("ikkada {expected} kavali, kaani {found} vachindhi")),
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> PResult<Span> {
        if self.at(kind) {
            Ok(self.advance().span)
        } else {
            self.unexpected(what);
            Err(())
        }
    }

    fn expect_kw(&mut self, kw: Keyword) -> PResult<Span> {
        if self.at_kw(kw) {
            Ok(self.advance().span)
        } else {
            self.unexpected(&format!("`{}`", kw.as_str()));
            Err(())
        }
    }

    fn expect_newline(&mut self) -> PResult<()> {
        if self.at(&TokenKind::Newline) || self.at(&TokenKind::Eof) {
            self.eat(&TokenKind::Newline);
            Ok(())
        } else {
            self.unexpected("line end");
            Err(())
        }
    }

    fn expect_ident(&mut self, what: &str) -> PResult<Ident> {
        let tok = self.toks[self.pos].clone();
        match tok.kind {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Ident { name, span: tok.span })
            }
            TokenKind::Keyword(kw) => {
                self.error(
                    "E0307",
                    tok.span,
                    format!("`{}` keyword, {what} ga vaadalevu. Vere peru pettandi", kw.as_str()),
                );
                Err(())
            }
            _ => {
                self.unexpected(what);
                Err(())
            }
        }
    }

    fn snapshot(&self) -> (usize, usize) {
        (self.pos, self.diags.len())
    }

    fn restore(&mut self, snap: (usize, usize)) {
        self.pos = snap.0;
        self.diags.truncate(snap.1);
    }

    /// Index of the NEWLINE (or EOF) that ends the current logical line.
    fn line_end(&self) -> usize {
        let mut i = self.pos;
        while !matches!(self.toks[i].kind, TokenKind::Newline | TokenKind::Eof) {
            i += 1;
        }
        i
    }

    /// Index of the bracket matching the opener at `open`, within the line.
    fn matching_close(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in open..self.toks.len() {
            match self.toks[i].kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                TokenKind::Newline | TokenKind::Eof => return None,
                _ => {}
            }
        }
        None
    }

    /// Skips the rest of the logical line and, if a block follows, that block.
    fn recover_line(&mut self) {
        self.bail = false;
        self.pos = self.line_end();
        self.eat(&TokenKind::Newline);
        if self.at(&TokenKind::Indent) {
            let mut depth = 0usize;
            loop {
                match self.advance().kind {
                    TokenKind::Indent => depth += 1,
                    TokenKind::Dedent => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    TokenKind::Eof => break,
                    _ => {}
                }
            }
        }
    }

    // ── items (D1) ─────────────────────────────────────────────────────────

    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();
        loop {
            while self.eat(&TokenKind::Newline) {}
            match self.peek() {
                TokenKind::Eof => break,
                TokenKind::Indent | TokenKind::Dedent => {
                    self.advance();
                    continue;
                }
                _ => {}
            }
            let start = self.pos;
            match self.parse_item() {
                Ok(item) => items.push(item),
                Err(()) => {
                    self.pos = start;
                    self.recover_line();
                    if let Some(at) = self.resume.take() {
                        self.pos = self.pos.max(at);
                    }
                }
            }
        }
        Module { items }
    }

    fn parse_annotations(&mut self) -> PResult<Vec<Annotation>> {
        let mut annotations = Vec::new();
        while self.at(&TokenKind::At) {
            let start = self.advance().span;
            let name = self.expect_ident("annotation peru")?;
            let arg = match self.peek().clone() {
                TokenKind::Str(s) => {
                    self.advance();
                    AnnotationArg::Str(s)
                }
                TokenKind::Ident(_) => {
                    let mut names = vec![self.expect_ident("trait peru")?];
                    while self.eat(&TokenKind::Comma) {
                        names.push(self.expect_ident("trait peru")?);
                    }
                    AnnotationArg::Names(names)
                }
                _ => AnnotationArg::None,
            };
            let span = self.since(start);
            self.expect_newline()?;
            annotations.push(Annotation { name, arg, span });
        }
        Ok(annotations)
    }

    fn parse_item(&mut self) -> PResult<Item> {
        let start = self.span();
        let annotations = self.parse_annotations()?;
        let public = self.eat_kw(Keyword::Andhariki);

        let line = &self.toks[self.pos..self.line_end()];
        let first = line.first().map(|t| t.kind.clone());
        let last = line.last().map(|t| t.kind.clone());
        let has =
            |seq: &[TokenKind]| line.windows(seq.len()).any(|w| w.iter().map(|t| &t.kind).eq(seq.iter()));
        let is_methods =
            has(&[TokenKind::Keyword(Keyword::Panulu), TokenKind::Keyword(Keyword::Ante), TokenKind::Colon]);
        let is_impl = has(&[TokenKind::Keyword(Keyword::Gunam), TokenKind::Keyword(Keyword::Undhi)]);
        let second = line.get(1).map(|t| t.kind.clone());

        let kind = match (first, last) {
            (_, Some(TokenKind::Keyword(Keyword::Thechuko))) => {
                if !annotations.is_empty() {
                    self.error("E0208", start, "annotation tharvatha declaration raavali, import kaadhu");
                }
                if public {
                    self.error("E0201", start, "`andhariki` imports ki pettalem");
                }
                ItemKind::Import(self.parse_import()?)
            }
            (Some(TokenKind::Keyword(Keyword::Rakam)), _) => ItemKind::Record(self.parse_record()?),
            (Some(TokenKind::Keyword(Keyword::EdhoOkati)), _) => ItemKind::Enum(self.parse_enum()?),
            (Some(TokenKind::Keyword(Keyword::Bayata)), _) => ItemKind::Extern(self.parse_extern()?),
            _ if is_methods => ItemKind::Methods(self.parse_methods()?),
            _ if is_impl => ItemKind::Impl(self.parse_impl()?),
            (Some(TokenKind::Keyword(Keyword::Gunam)), _) => ItemKind::Trait(self.parse_trait()?),
            (Some(TokenKind::Ident(_)), _) if second == Some(TokenKind::Colon) => self.parse_const()?,
            (Some(TokenKind::Ident(_)), _)
                if matches!(second, Some(TokenKind::LParen) | Some(TokenKind::LBracket)) =>
            {
                ItemKind::Fn(self.parse_fn_decl()?)
            }
            _ => {
                let reserved = line
                    .iter()
                    .any(|t| matches!(t.kind, TokenKind::Keyword(kw) if kw.is_reserved_for_later()));
                if reserved {
                    // Already reported as E1201; one error per line is enough.
                } else if !annotations.is_empty() {
                    self.error("E0208", start, "annotation tharvatha function or type declaration raavali");
                    // Skip the line under the annotation too, so it isn't
                    // reported again as E0209.
                    self.recover_line();
                    self.resume = Some(self.pos);
                } else {
                    let span = self.span();
                    self.error(
                        "E0209",
                        span,
                        "module level lo statements raayakoodadhu. Ee code ni `modhalu()` lopala pettandi",
                    );
                }
                return Err(());
            }
        };
        Ok(Item { annotations, public, kind, span: self.since(start) })
    }

    fn parse_module_path(&mut self) -> PResult<Path> {
        let first = self.expect_ident("module peru")?;
        let mut segments = vec![first];
        while self.at(&TokenKind::Dot) {
            self.advance();
            segments.push(self.expect_ident("module peru")?);
        }
        let span = join(segments[0].span, segments.last().expect("non-empty").span);
        Ok(Path { segments, span })
    }

    fn parse_import(&mut self) -> PResult<Import> {
        let module = self.parse_module_path()?;
        let kind = if self.eat_kw(Keyword::Thechuko) {
            ImportKind::Module
        } else if self.eat_kw(Keyword::Nunchi) {
            let mut names = vec![self.expect_ident("import cheyyalsina peru")?];
            while self.eat(&TokenKind::Comma) {
                names.push(self.expect_ident("import cheyyalsina peru")?);
            }
            self.expect_kw(Keyword::Thechuko)?;
            ImportKind::Items(names)
        } else if self.eat_kw(Keyword::Ni) {
            let alias = self.expect_ident("alias peru")?;
            self.expect_kw(Keyword::Ga)?;
            self.expect_kw(Keyword::Thechuko)?;
            ImportKind::Alias(alias)
        } else {
            self.unexpected("`thechuko`, `nunchi` or `ni`");
            return Err(());
        };
        self.expect_newline()?;
        Ok(Import { module, kind })
    }

    fn parse_generic_params(&mut self) -> PResult<Vec<GenericParam>> {
        let mut params = Vec::new();
        if !self.at(&TokenKind::LBracket) {
            return Ok(params);
        }
        self.advance();
        while !self.at(&TokenKind::RBracket) {
            let name = self.expect_ident("generic parameter")?;
            let mut bounds = Vec::new();
            let mut is_const = false;
            if self.eat(&TokenKind::Colon) {
                if self.eat_kw(Keyword::Ankam) {
                    is_const = true;
                } else {
                    bounds.push(self.parse_type()?);
                    while self.eat(&TokenKind::Plus) {
                        bounds.push(self.parse_type()?);
                    }
                }
            }
            params.push(GenericParam { name, bounds, is_const });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "`]`")?;
        Ok(params)
    }

    fn parse_role(&mut self) -> Option<Role> {
        if let TokenKind::Keyword(kw) = *self.peek() {
            if let Some(role) = role_of(kw) {
                self.advance();
                return Some(role);
            }
        }
        None
    }

    fn parse_param(&mut self) -> PResult<Param> {
        let start = self.span();
        if self.at_kw(Keyword::Naa) {
            let tok = self.advance();
            self.expect(&TokenKind::Colon, "`:`")?;
            let ty = self.parse_type()?;
            return Ok(Param {
                kind: ParamKind::Receiver,
                name: Ident { name: "naa".into(), span: tok.span },
                ty,
                role: None,
                keyword_only: false,
                default: None,
                span: self.since(start),
            });
        }
        let kind = if self.eat_kw(Keyword::Migathavi) {
            ParamKind::Variadic
        } else if self.eat_kw(Keyword::MigathaPerlu) {
            ParamKind::NamedCollector
        } else {
            ParamKind::Normal
        };
        let name = self.expect_ident("parameter peru")?;
        self.expect(&TokenKind::Colon, "`:` and type")?;
        let ty = self.parse_type()?;
        let role = self.parse_role();
        let keyword_only = self.eat_kw(Keyword::PeruThone);
        let default = if self.eat(&TokenKind::Eq) { Some(self.parse_binexpr(1)?) } else { None };
        Ok(Param { kind, name, ty, role, keyword_only, default, span: self.since(start) })
    }

    fn parse_fn_sig(&mut self) -> PResult<FnSig> {
        let start = self.span();
        let name = self.expect_ident("function peru")?;
        let generics = self.parse_generic_params()?;
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            params.push(self.parse_param()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        self.expect(&TokenKind::Arrow, "`->` and return type")?;
        let ret = self.parse_type()?;
        Ok(FnSig { name, generics, params, ret, span: self.since(start) })
    }

    fn parse_fn_decl(&mut self) -> PResult<FnDecl> {
        let sig = self.parse_fn_sig()?;
        self.expect_kw(Keyword::Ante)?;
        self.expect(&TokenKind::Colon, "`:`")?;
        let saved = (self.loop_depth, self.defer_depth);
        self.loop_depth = 0;
        self.defer_depth = 0;
        let body = self.parse_block();
        (self.loop_depth, self.defer_depth) = saved;
        Ok(FnDecl { sig, body: body? })
    }

    fn parse_const(&mut self) -> PResult<ItemKind> {
        let name = self.expect_ident("constant peru")?;
        self.expect(&TokenKind::Colon, "`:`")?;
        let ty = self.parse_type()?;
        self.expect_kw(Keyword::Ante)?;
        let value = self.parse_pipe_expr()?;
        self.expect_newline()?;
        Ok(ItemKind::Const { name, ty, value })
    }

    /// `ante :` NEWLINE INDENT, the start of a declaration body.
    fn open_body(&mut self) -> PResult<()> {
        self.expect_kw(Keyword::Ante)?;
        self.open_suite()
    }

    fn open_suite(&mut self) -> PResult<()> {
        self.expect(&TokenKind::Colon, "`:`")?;
        self.expect(&TokenKind::Newline, "line end")?;
        self.expect(&TokenKind::Indent, "indented block")?;
        Ok(())
    }

    fn close_suite(&mut self) -> PResult<()> {
        while self.eat(&TokenKind::Newline) {}
        if self.at(&TokenKind::Eof) {
            return Ok(());
        }
        self.expect(&TokenKind::Dedent, "block end").map(|_| ())
    }

    fn in_suite(&self) -> bool {
        !matches!(self.peek(), TokenKind::Dedent | TokenKind::Eof)
    }

    fn parse_record(&mut self) -> PResult<RecordDecl> {
        self.expect_kw(Keyword::Rakam)?;
        let name = self.expect_ident("rakam peru")?;
        let generics = self.parse_generic_params()?;
        self.open_body()?;
        let mut fields = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let public = self.eat_kw(Keyword::Andhariki);
            let fname = self.expect_ident("field peru")?;
            self.expect(&TokenKind::Colon, "`:` and type")?;
            let ty = self.parse_type()?;
            self.expect_newline()?;
            fields.push(Field { public, name: fname, ty });
        }
        self.close_suite()?;
        Ok(RecordDecl { name, generics, fields })
    }

    fn parse_enum(&mut self) -> PResult<EnumDecl> {
        self.expect_kw(Keyword::EdhoOkati)?;
        let name = self.expect_ident("edho_okati peru")?;
        let generics = self.parse_generic_params()?;
        self.open_body()?;
        let mut variants = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let vname = self.expect_ident("variant peru")?;
            let mut fields = Vec::new();
            if self.eat(&TokenKind::LParen) {
                while !self.at(&TokenKind::RParen) {
                    fields.push(self.parse_type()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "`)`")?;
            }
            self.expect_newline()?;
            variants.push(Variant { name: vname, fields });
        }
        self.close_suite()?;
        Ok(EnumDecl { name, generics, variants })
    }

    /// Function members of `panulu` and implementation blocks.
    fn parse_member_fns(&mut self, what: &str) -> Vec<Item> {
        let mut items = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let start = self.pos;
            let start_span = self.span();
            let result = (|| -> PResult<Item> {
                let annotations = self.parse_annotations()?;
                let public = self.eat_kw(Keyword::Andhariki);
                if !matches!(self.peek(), TokenKind::Ident(_)) {
                    let span = self.span();
                    self.error("E0201", span, format!("{what} lo functions matrame raayochu"));
                    return Err(());
                }
                let decl = self.parse_fn_decl()?;
                Ok(Item { annotations, public, kind: ItemKind::Fn(decl), span: self.since(start_span) })
            })();
            match result {
                Ok(item) => items.push(item),
                Err(()) => {
                    self.pos = start;
                    self.recover_line();
                }
            }
        }
        items
    }

    fn parse_methods(&mut self) -> PResult<MethodsDecl> {
        let generics = self.parse_generic_params()?;
        let self_ty = self.parse_type()?;
        self.expect_kw(Keyword::Panulu)?;
        self.open_body()?;
        let items = self.parse_member_fns("`panulu` block");
        self.close_suite()?;
        Ok(MethodsDecl { generics, self_ty, items })
    }

    fn parse_impl(&mut self) -> PResult<ImplDecl> {
        let generics = self.parse_generic_params()?;
        let self_ty = self.parse_type()?;
        self.expect_kw(Keyword::Ki)?;
        let trait_ref = self.parse_type()?;
        self.expect_kw(Keyword::Gunam)?;
        self.expect_kw(Keyword::Undhi)?;
        let mut items = Vec::new();
        if self.at(&TokenKind::Colon) {
            self.open_suite()?;
            for item in self.parse_member_fns("`gunam undhi` block") {
                if let ItemKind::Fn(decl) = item.kind {
                    items.push(decl);
                }
            }
            self.close_suite()?;
        } else {
            self.expect_newline()?;
        }
        Ok(ImplDecl { generics, self_ty, trait_ref, items })
    }

    fn parse_trait(&mut self) -> PResult<TraitDecl> {
        self.expect_kw(Keyword::Gunam)?;
        let name = self.expect_ident("gunam peru")?;
        let generics = self.parse_generic_params()?;
        self.open_body()?;
        let mut items = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let start = self.pos;
            let result = (|| -> PResult<TraitItem> {
                let sig = self.parse_fn_sig()?;
                if self.eat_kw(Keyword::Ante) {
                    self.expect(&TokenKind::Colon, "`:`")?;
                    let body = self.parse_block()?;
                    Ok(TraitItem { sig, default_body: Some(body) })
                } else {
                    self.expect_newline()?;
                    Ok(TraitItem { sig, default_body: None })
                }
            })();
            match result {
                Ok(item) => items.push(item),
                Err(()) => {
                    self.pos = start;
                    self.recover_line();
                }
            }
        }
        self.close_suite()?;
        Ok(TraitDecl { name, generics, items })
    }

    fn parse_extern(&mut self) -> PResult<ExternBlock> {
        self.expect_kw(Keyword::Bayata)?;
        let abi = match self.peek().clone() {
            TokenKind::Str(s) => {
                self.advance();
                s
            }
            _ => {
                self.unexpected("ABI string, ex: `\"C\"`");
                return Err(());
            }
        };
        self.open_body()?;
        let mut fns = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            fns.push(self.parse_fn_sig()?);
            self.expect_newline()?;
        }
        self.close_suite()?;
        Ok(ExternBlock { abi, fns })
    }

    // ── statements (D2) ────────────────────────────────────────────────────

    /// Parses `NEWLINE INDENT stmt+ DEDENT`; the caller has consumed the `:`.
    fn parse_block(&mut self) -> PResult<Block> {
        self.nested(|p| p.parse_block_inner())
    }

    fn parse_block_inner(&mut self) -> PResult<Block> {
        let start = self.span();
        if !self.at(&TokenKind::Newline) {
            self.unexpected("line end (block `:` tharvatha next line lo start avvali)");
            return Err(());
        }
        self.advance();
        if !self.at(&TokenKind::Indent) {
            // The lexer already reported the missing indentation.
            return Ok(Block { stmts: Vec::new(), span: start });
        }
        self.advance();
        let mut stmts = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let start = self.pos;
            match self.parse_stmt() {
                Ok(stmt) => stmts.push(stmt),
                Err(()) => {
                    self.pos = start;
                    self.recover_line();
                }
            }
        }
        self.eat(&TokenKind::Dedent);
        Ok(Block { stmts, span: self.since(start) })
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let end = self.line_end();
        let line: Vec<TokenKind> = self.toks[self.pos..end].iter().map(|t| t.kind.clone()).collect();
        let first = line.first().cloned().unwrap_or(TokenKind::Newline);
        let last = line.last().cloned().unwrap_or(TokenKind::Newline);
        let before_last = if line.len() >= 2 { line[line.len() - 2].clone() } else { TokenKind::Newline };

        let kind = match (&first, &last) {
            (TokenKind::Keyword(kw @ (Keyword::Aapey | Keyword::Vadhiley)), _) => {
                let kw = *kw;
                let span = self.advance().span;
                if self.loop_depth == 0 {
                    self.error("E0213", span, format!("`{}` loop lopala matrame vaadochu", kw.as_str()));
                }
                if self.defer_depth > 0 {
                    self.error("E0214", span, "`vellagane` block lo control flow vaadakoodadhu");
                }
                self.expect_newline()?;
                if kw == Keyword::Aapey {
                    StmtKind::Break
                } else {
                    StmtKind::Continue
                }
            }
            (_, TokenKind::Colon) => return self.parse_compound_stmt(start, &first, &before_last),
            (_, TokenKind::Keyword(Keyword::Pettu)) => {
                let place = self.parse_place()?;
                self.expect_kw(Keyword::Lo)?;
                let value = self.parse_pipe_expr()?;
                self.expect_kw(Keyword::Pettu)?;
                self.expect_newline()?;
                StmtKind::Assign { place, value }
            }
            (_, TokenKind::Keyword(Keyword::Icchey)) => {
                if self.defer_depth > 0 {
                    self.error("E0214", start, "`vellagane` block lo `icchey` vaadakoodadhu");
                }
                let value = if self.at_kw(Keyword::Icchey) { None } else { Some(self.parse_pipe_expr()?) };
                self.expect_kw(Keyword::Icchey)?;
                self.expect_newline()?;
                StmtKind::Return(value)
            }
            _ if self.is_binding_line(&line) => self.parse_binding()?,
            _ if line.iter().any(|k| compound_op(k).is_some()) => {
                let place = self.parse_place()?;
                let op = match compound_op(self.peek()) {
                    Some(op) => op,
                    None => {
                        self.unexpected("`+=`, `-=`, `*=` or `/=`");
                        return Err(());
                    }
                };
                self.advance();
                let value = self.parse_pipe_expr()?;
                self.expect_newline()?;
                StmtKind::Compound { place, op, value }
            }
            _ => {
                let expr = self.parse_pipe_expr()?;
                self.expect_newline()?;
                StmtKind::Expr(expr)
            }
        };
        Ok(Stmt { kind, span: self.since(start) })
    }

    fn is_binding_line(&self, line: &[TokenKind]) -> bool {
        match line {
            [TokenKind::Keyword(Keyword::Maare), ..] => true,
            [TokenKind::Ident(_), TokenKind::Keyword(Keyword::Ante) | TokenKind::Colon, ..] => true,
            [TokenKind::LParen, ..] => self
                .matching_close(self.pos)
                .is_some_and(|close| self.kw_at(close - self.pos + 1, Keyword::Ante)),
            _ => false,
        }
    }

    fn parse_binding(&mut self) -> PResult<StmtKind> {
        let mutable = self.eat_kw(Keyword::Maare);
        let target = if self.eat(&TokenKind::LParen) {
            let mut names = vec![self.expect_ident("peru")?];
            while self.eat(&TokenKind::Comma) {
                if self.at(&TokenKind::RParen) {
                    break;
                }
                names.push(self.expect_ident("peru")?);
            }
            self.expect(&TokenKind::RParen, "`)`")?;
            BindTarget::Tuple(names)
        } else {
            BindTarget::Name(self.expect_ident("binding peru")?)
        };
        let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
        self.expect_kw(Keyword::Ante)?;
        let value = self.parse_pipe_expr()?;
        self.expect_newline()?;
        Ok(StmtKind::Binding { mutable, target, ty, value })
    }

    fn parse_place(&mut self) -> PResult<Expr> {
        let start = self.span();
        let mut stars = 0;
        while self.eat(&TokenKind::Star) {
            stars += 1;
        }
        let mut place = self.parse_postfix()?;
        for _ in 0..stars {
            place = mk(ExprKind::Unary { op: UnOp::Deref, operand: Box::new(place) }, self.since(start));
        }
        if !is_place(&place) {
            self.error(
                "E0215",
                place.span,
                "deeniki value assign cheyyalem. Variable, field or index kavali",
            );
            return Err(());
        }
        Ok(place)
    }

    fn parse_compound_stmt(
        &mut self,
        start: Span,
        first: &TokenKind,
        before_last: &TokenKind,
    ) -> PResult<Stmt> {
        let kind = match (first, before_last) {
            (TokenKind::Keyword(Keyword::Ledhante), _) => {
                self.error("E0207", start, "`ledhante` mundhu `aithe` block undali");
                return Err(());
            }
            (TokenKind::Keyword(Keyword::NaaRisk), _) => {
                self.advance();
                self.expect(&TokenKind::Colon, "`:`")?;
                StmtKind::Unsafe(self.parse_block()?)
            }
            (TokenKind::Keyword(Keyword::Vellagane), _) => {
                self.advance();
                self.expect(&TokenKind::Colon, "`:`")?;
                self.defer_depth += 1;
                let body = self.parse_block();
                self.defer_depth -= 1;
                StmtKind::Defer(body?)
            }
            (_, TokenKind::Keyword(Keyword::Aithe)) => self.parse_if()?,
            (_, TokenKind::Keyword(Keyword::Unnanthasepu)) => {
                let cond = self.parse_pipe_expr()?;
                self.expect_kw(Keyword::Unnanthasepu)?;
                self.expect(&TokenKind::Colon, "`:`")?;
                StmtKind::While { cond, body: self.parse_loop_body()? }
            }
            (_, TokenKind::Keyword(Keyword::Ki)) => self.parse_for()?,
            (_, TokenKind::Keyword(Keyword::Batti)) => self.parse_match()?,
            (_, TokenKind::Keyword(Keyword::Ante)) => {
                self.error(
                    "E0211",
                    start,
                    "function lopala inkoka function declare cheyyalem. Module level ki move cheyyandi",
                );
                return Err(());
            }
            _ => {
                self.error(
                    "E0201",
                    start,
                    "ee line `:` tho end ayyindhi, kaani `aithe`, `unnanthasepu`, `okko … ki` or `batti` header kaadhu",
                );
                return Err(());
            }
        };
        Ok(Stmt { kind, span: self.since(start) })
    }

    fn parse_loop_body(&mut self) -> PResult<Block> {
        self.loop_depth += 1;
        let body = self.parse_block();
        self.loop_depth -= 1;
        body
    }

    fn parse_if(&mut self) -> PResult<StmtKind> {
        let mut branches = Vec::new();
        let cond = self.parse_pipe_expr()?;
        self.expect_kw(Keyword::Aithe)?;
        self.expect(&TokenKind::Colon, "`:`")?;
        branches.push((cond, self.parse_block()?));
        let mut else_block = None;
        while self.at_kw(Keyword::Ledhante) {
            self.advance();
            if self.eat(&TokenKind::Colon) {
                else_block = Some(self.parse_block()?);
                break;
            }
            let cond = self.parse_pipe_expr()?;
            self.expect_kw(Keyword::Aithe)?;
            self.expect(&TokenKind::Colon, "`:`")?;
            branches.push((cond, self.parse_block()?));
        }
        Ok(StmtKind::If { branches, else_block })
    }

    fn parse_for(&mut self) -> PResult<StmtKind> {
        let iter = self.parse_pipe_expr()?;
        if self.eat_kw(Keyword::Lo) {
            self.expect_kw(Keyword::Okko)?;
        } else if self.at_kw(Keyword::Okko) {
            if !matches!(iter.kind, ExprKind::Range { .. }) {
                let span = self.span();
                self.error("E0201", span, "range kaakapothe `lo okko` ani raayali, ex: `xs lo okko x ki:`");
                return Err(());
            }
            self.advance();
        } else {
            self.unexpected("`lo okko` or `okko`");
            return Err(());
        }
        let pattern = self.parse_pattern()?;
        self.expect_kw(Keyword::Ki)?;
        self.expect(&TokenKind::Colon, "`:`")?;
        Ok(StmtKind::For { iter, pattern, body: self.parse_loop_body()? })
    }

    fn parse_match(&mut self) -> PResult<StmtKind> {
        let scrutinee = self.parse_pipe_expr()?;
        self.expect_kw(Keyword::Batti)?;
        self.open_suite()?;
        let mut arms = Vec::new();
        while self.in_suite() {
            if self.eat(&TokenKind::Newline) {
                continue;
            }
            let start = self.pos;
            let result = (|| -> PResult<Arm> {
                let pattern = self.parse_pattern()?;
                self.expect_kw(Keyword::Aithe)?;
                self.expect(&TokenKind::Colon, "`:`")?;
                Ok(Arm { pattern, body: self.parse_block()? })
            })();
            match result {
                Ok(arm) => arms.push(arm),
                Err(()) => {
                    self.pos = start;
                    self.recover_line();
                }
            }
        }
        self.close_suite()?;
        Ok(StmtKind::Match { scrutinee, arms })
    }

    // ── expressions ────────────────────────────────────────────────────────

    fn parse_pipe_expr(&mut self) -> PResult<Expr> {
        self.parse_pipe_expr_with(true)
    }

    fn parse_pipe_expr_with(&mut self, commas: bool) -> PResult<Expr> {
        self.expr_level += 1;
        let result = self.parse_pipe_expr_inner(commas);
        self.expr_level -= 1;
        if self.expr_level == 0 {
            if let Ok(e) = &result {
                if e.depth() > MAX_EXPR_DEPTH {
                    let span = e.span;
                    self.report_deep(span, "ee expression chaala lothuga undhi");
                    return Err(());
                }
            }
        }
        result
    }

    fn parse_pipe_expr_inner(&mut self, commas: bool) -> PResult<Expr> {
        let mut expr = self.parse_clause(commas)?;
        if count_holes(&expr) > 0 {
            self.error(
                "E0213",
                expr.span,
                "`_` pipe stage lo matrame vaadochu, ex: `x |> _ ni a ga, 5 tho f()`",
            );
        }
        while self.at(&TokenKind::PipeGt) {
            self.advance();
            let stage = self.parse_clause(commas)?;
            if !is_call(&stage) {
                self.error(
                    "E0212",
                    stage.span,
                    "pipe stage function call ayyi undali, ex: `|> choopinchu()`",
                );
            } else if count_holes(&stage) > 1 {
                self.error("E0213", stage.span, "oka pipe stage lo okka `_` matrame undali");
            }
            let span = join(expr.span, stage.span);
            expr = mk(ExprKind::Pipe { input: Box::new(expr), stage: Box::new(stage) }, span);
        }
        Ok(expr)
    }

    /// Whether the next tokens are `path type_args? (`, i.e. a callee (D3, D6).
    fn at_callee_start(&self) -> bool {
        let mut i = self.pos;
        match &self.toks[i].kind {
            TokenKind::Ident(_) => {}
            TokenKind::Keyword(kw) if *kw == Keyword::Naa || is_type_keyword(*kw) => {}
            _ => return false,
        }
        while self.toks[i + 1].kind == TokenKind::Dot && matches!(self.toks[i + 2].kind, TokenKind::Ident(_))
        {
            i += 2;
        }
        if self.toks[i + 1].kind == TokenKind::LBracket {
            match self.matching_close(i + 1) {
                Some(close) => i = close,
                None => return false,
            }
        }
        self.toks[i + 1].kind == TokenKind::LParen
    }

    /// If the next tokens are a literal or bracketed receiver followed by
    /// `.name(`, as in `"ff".ankam_ga()`, the index of the `(`.
    fn non_path_receiver_call(&self) -> Option<usize> {
        let i = self.pos;
        let last = match &self.toks[i].kind {
            TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::Char(_)
            | TokenKind::FStr(_) => i,
            TokenKind::LParen | TokenKind::LBracket => self.matching_close(i)?,
            _ => return None,
        };
        let dot = matches!(self.toks.get(last + 1)?.kind, TokenKind::Dot);
        let name = matches!(self.toks.get(last + 2)?.kind, TokenKind::Ident(_));
        let paren = matches!(self.toks.get(last + 3)?.kind, TokenKind::LParen);
        (dot && name && paren).then_some(last + 3)
    }

    fn starts_expression(&self) -> bool {
        match self.peek() {
            TokenKind::Ident(_)
            | TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::Char(_)
            | TokenKind::FStr(_)
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Minus
            | TokenKind::Bang
            | TokenKind::Amp => true,
            TokenKind::Keyword(kw) => matches!(
                kw,
                Keyword::Avunu
                    | Keyword::Kaadhu
                    | Keyword::Ledhu
                    | Keyword::Sare
                    | Keyword::Undhi
                    | Keyword::Thappu
                    | Keyword::Naa
            ),
            _ => false,
        }
    }

    /// A clause: either phrases followed by a callee, or a plain expression (D3).
    fn parse_clause(&mut self, commas: bool) -> PResult<Expr> {
        let start = self.span();
        let mut phrases = vec![self.parse_phrase()?];
        while commas && self.at(&TokenKind::Comma) {
            self.advance();
            phrases.push(self.parse_phrase()?);
        }
        let plain = phrases.len() == 1
            && phrases[0].role.is_none()
            && phrases[0].label.is_none()
            && matches!(phrases[0].kind, PhraseKind::Value(_));
        if self.at_callee_start() {
            return self.parse_callee(start, phrases);
        }
        if plain {
            let phrase = phrases.pop().expect("one phrase");
            if let PhraseKind::Value(expr) = phrase.kind {
                return Ok(expr);
            }
        }
        let span = self.span();
        if let Some(end) = self.non_path_receiver_call() {
            // `x ni "ff".f()`: a callee must be a path (D3), not a literal.
            let span = join(span, self.toks[end].span);
            self.error(
                "E0202",
                span,
                "verb (callee) ga path matrame raavali, literal kaadhu. Mundhu variable lo pettandi: `s ante \"ff\"`, tharvatha `… s.method()`",
            );
        } else if self.starts_expression() {
            self.error("E0203", span, "phrases madhyalo `,` kavali, ex: `10 ni, 20 tho kalupu()`");
        } else {
            self.error("E0202", span, "verb ekkada? Arguments tharvatha function call (`kalupu()`) raavali");
        }
        Err(())
    }

    fn parse_callee(&mut self, start: Span, phrases: Vec<Phrase>) -> PResult<Expr> {
        let path = self.parse_path()?;
        let callee = mk(ExprKind::Path(path.clone()), path.span);
        let type_args = if self.at(&TokenKind::LBracket) { self.parse_type_args()? } else { Vec::new() };
        self.expect(&TokenKind::LParen, "`()`")?;
        if !self.at(&TokenKind::RParen) {
            self.error(
                "E0204",
                path.span,
                "`f(x)` laaga call cheyyalem. Arguments verb mundhu raayandi: `x ni f()`",
            );
            return Err(());
        }
        self.advance();
        let mut call = mk(ExprKind::Call { callee: Box::new(callee), type_args, phrases }, self.since(start));
        if self.eat(&TokenKind::Question) {
            call = mk(ExprKind::Try(Box::new(call)), self.since(start));
        }
        Ok(call)
    }

    fn parse_phrase(&mut self) -> PResult<Phrase> {
        let start = self.span();
        let kind = if self.eat(&TokenKind::Ellipsis) {
            PhraseKind::Spread(self.parse_operand()?)
        } else if self.at(&TokenKind::Underscore) && self.index_depth == 0 {
            self.advance();
            PhraseKind::Hole
        } else {
            PhraseKind::Value(self.parse_operand()?)
        };
        let role = self.parse_phrase_role();
        let label = if matches!(self.peek(), TokenKind::Ident(_)) && self.kw_at(1, Keyword::Ga) {
            let label = self.expect_ident("label")?;
            self.advance();
            Some(label)
        } else {
            None
        };
        if self.at_kw(Keyword::Ga) {
            self.unexpected("label");
            return Err(());
        }
        Ok(Phrase { kind, role, label, span: self.since(start) })
    }

    /// A role marker after an operand, except `lo okko` (a loop) and
    /// `batti :` (a match header).
    fn parse_phrase_role(&mut self) -> Option<Role> {
        let TokenKind::Keyword(kw) = *self.peek() else { return None };
        if kw == Keyword::Lo && self.kw_at(1, Keyword::Okko) {
            return None;
        }
        if kw == Keyword::Batti && *self.peek_at(1) == TokenKind::Colon {
            return None;
        }
        let role = role_of(kw)?;
        self.advance();
        Some(role)
    }

    /// An operand: a closure, a range, or a binary expression (no call clause).
    fn parse_operand(&mut self) -> PResult<Expr> {
        if self.at_kw(Keyword::Theesuko) || (self.at(&TokenKind::LParen) && self.closure_ahead()) {
            return self.parse_closure();
        }
        let lo = self.parse_binexpr(1)?;
        if self.at_kw(Keyword::Nunchi) && *self.peek_at(1) != TokenKind::Comma {
            let snap = self.snapshot();
            self.advance();
            if let Ok(hi) = self.parse_binexpr(1) {
                if self.at_kw(Keyword::Lopu) || self.at_kw(Keyword::Varaku) {
                    let inclusive = self.advance().kind == TokenKind::Keyword(Keyword::Varaku);
                    let span = join(lo.span, self.since(lo.span));
                    return Ok(mk(ExprKind::Range { lo: Box::new(lo), hi: Box::new(hi), inclusive }, span));
                }
            }
            self.restore(snap);
        }
        Ok(lo)
    }

    fn closure_ahead(&self) -> bool {
        self.matching_close(self.pos).is_some_and(|close| self.toks[close + 1].kind == TokenKind::FatArrow)
    }

    fn parse_closure(&mut self) -> PResult<Expr> {
        let start = self.span();
        let moves = self.eat_kw(Keyword::Theesuko);
        self.expect(&TokenKind::LParen, "closure parameters `(…)`")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            let name = self.expect_ident("parameter peru")?;
            let ty = if self.eat(&TokenKind::Colon) { Some(self.parse_type()?) } else { None };
            let role = self.parse_role();
            params.push(ClosureParam { name, ty, role });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        self.expect(&TokenKind::FatArrow, "`=>`")?;
        let body = self.parse_pipe_expr()?;
        Ok(mk(ExprKind::Closure { moves, params, body: Box::new(body) }, self.since(start)))
    }

    fn parse_binexpr(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            if compound_op(self.peek()).is_some() {
                let span = self.span();
                self.error(
                    "E0210",
                    span,
                    "`+=` laantivi statement ga matrame vaadochu, expression lopala kaadhu",
                );
                return Err(());
            }
            let Some((op, prec)) = binop_info(self.peek()) else { break };
            if prec < min_prec {
                break;
            }
            self.advance();
            let rhs = self.parse_binexpr(prec + 1)?;
            let span = join(lhs.span, rhs.span);
            lhs = mk(ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span);
            if prec == 3 || prec == 4 {
                if let Some((_, next)) = binop_info(self.peek()) {
                    if next == prec {
                        let span = self.span();
                        self.error(
                            "E0417",
                            span,
                            "comparisons chain cheyyalem. `(a < b) && (b < c)` ani raayandi",
                        );
                        return Err(());
                    }
                }
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        self.nested(|p| p.parse_unary_inner())
    }

    fn parse_unary_inner(&mut self) -> PResult<Expr> {
        let start = self.span();
        let op = match self.peek() {
            TokenKind::Minus => UnOp::Neg,
            TokenKind::Bang => UnOp::Not,
            TokenKind::Tilde => UnOp::BitNot,
            TokenKind::Star => UnOp::Deref,
            TokenKind::Amp if self.kw_at(1, Keyword::Maare) => {
                self.advance();
                UnOp::RefMut
            }
            TokenKind::Amp => UnOp::Ref,
            _ => return self.parse_postfix(),
        };
        self.advance();
        let operand = self.parse_unary()?;
        Ok(mk(ExprKind::Unary { op, operand: Box::new(operand) }, self.since(start)))
    }

    fn generic_call_ahead(&self) -> bool {
        self.matching_close(self.pos).is_some_and(|close| self.toks[close + 1].kind == TokenKind::LParen)
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let start = self.span();
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek().clone() {
                TokenKind::Dot => {
                    self.advance();
                    match self.peek().clone() {
                        TokenKind::Ident(_) => {
                            let name = self.expect_ident("field peru")?;
                            expr = mk(ExprKind::Field { base: Box::new(expr), name }, self.since(start));
                        }
                        TokenKind::Int(lit) if lit.radix == 10 => {
                            self.advance();
                            let index = lit.digits.parse().unwrap_or(u32::MAX);
                            expr =
                                mk(ExprKind::TupleIndex { base: Box::new(expr), index }, self.since(start));
                        }
                        _ => {
                            self.unexpected("field peru or tuple index");
                            return Err(());
                        }
                    }
                }
                TokenKind::LBracket if self.generic_call_ahead() => {
                    // D7: `xs[0]()` reads as a generic call, and `0` isn't a type.
                    if matches!(
                        self.peek_at(1),
                        TokenKind::Int(_)
                            | TokenKind::Float(_)
                            | TokenKind::Str(_)
                            | TokenKind::Char(_)
                            | TokenKind::FStr(_)
                            | TokenKind::Minus
                            | TokenKind::Keyword(Keyword::Avunu | Keyword::Kaadhu)
                    ) {
                        let close = self.matching_close(self.pos).unwrap_or(self.pos);
                        let text = &self.src.text[expr.span.start..self.toks[close].span.end];
                        let span = join(expr.span, self.toks[close + 1].span);
                        self.error(
                            "E0401",
                            span,
                            format!(
                                "`{text}()` generic call laaga chadhuvuthundhi, `[…]` lo type kavali. Index chesi call cheyyalante `({text})()` ani raayandi"
                            ),
                        );
                        return Err(());
                    }
                    let type_args = self.parse_type_args()?;
                    self.expect(&TokenKind::LParen, "`()`")?;
                    if !self.at(&TokenKind::RParen) {
                        let span = self.span();
                        self.error(
                            "E0204",
                            span,
                            "`f[T](x)` laaga call cheyyalem. `x ni f[T]()` ani raayandi",
                        );
                        return Err(());
                    }
                    self.advance();
                    expr = mk(
                        ExprKind::Call { callee: Box::new(expr), type_args, phrases: Vec::new() },
                        self.since(start),
                    );
                }
                TokenKind::LBracket => {
                    self.advance();
                    self.index_depth += 1;
                    let list = self.parse_list_until(&TokenKind::RBracket);
                    self.index_depth -= 1;
                    let (indices, _) = list?;
                    self.expect(&TokenKind::RBracket, "`]`")?;
                    if indices.is_empty() {
                        let span = self.since(start);
                        self.error("E0201", span, "`[]` lopala index kavali");
                        return Err(());
                    }
                    expr = mk(ExprKind::Index { base: Box::new(expr), indices }, self.since(start));
                }
                TokenKind::LParen if *self.peek_at(1) == TokenKind::RParen => {
                    self.advance();
                    self.advance();
                    expr = mk(
                        ExprKind::Call { callee: Box::new(expr), type_args: Vec::new(), phrases: Vec::new() },
                        self.since(start),
                    );
                }
                TokenKind::LParen => {
                    // `name(args)` is only a constructor form (D10).
                    let ExprKind::Path(path) = expr.kind.clone() else {
                        let span = self.span();
                        self.error("E0204", span, "`f(x)` laaga call cheyyalem. `x ni f()` ani raayandi");
                        return Err(());
                    };
                    if path.segments.len() == 1 {
                        let name = &path.segments[0].name;
                        self.error(
                            "E0204",
                            path.span,
                            format!("`{name}(…)` laaga call cheyyalem. Arguments verb mundhu raayandi: `x ni {name}()`"),
                        );
                        return Err(());
                    }
                    let args = self.parse_paren_args()?;
                    expr = mk(ExprKind::Ctor { path, args }, self.since(start));
                }
                TokenKind::Question => {
                    self.advance();
                    expr = mk(ExprKind::Try(Box::new(expr)), self.since(start));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_paren_args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let (args, _) = self.parse_list_until(&TokenKind::RParen)?;
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(args)
    }

    /// One element of a bracketed list. A comma joins phrases only when the
    /// phrase list ends in a callee; otherwise it separates elements (D4).
    fn parse_list_elem(&mut self) -> PResult<Expr> {
        let snap = self.snapshot();
        match self.list_elem_attempt(true) {
            Ok(expr) => Ok(expr),
            Err(()) => {
                self.restore(snap);
                self.list_elem_attempt(false)
            }
        }
    }

    fn list_elem_attempt(&mut self, commas: bool) -> PResult<Expr> {
        let key = (self.pos, commas);
        if let Some(diags) = self.failed_elems.get(&key) {
            self.diags.extend(diags.iter().cloned());
            return Err(());
        }
        let before = self.diags.len();
        let result = self.parse_pipe_expr_with(commas);
        if result.is_err() {
            self.failed_elems.insert(key, self.diags[before..].to_vec());
        }
        result
    }

    /// Comma-separated elements up to (not including) `close`; returns
    /// whether a trailing comma was present.
    fn parse_list_until(&mut self, close: &TokenKind) -> PResult<(Vec<Expr>, bool)> {
        let mut elems = Vec::new();
        let mut trailing = false;
        while !self.at(close) {
            elems.push(self.parse_list_elem()?);
            trailing = false;
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            trailing = true;
        }
        Ok((elems, trailing))
    }

    fn parse_path(&mut self) -> PResult<Path> {
        let tok = self.toks[self.pos].clone();
        let first = match tok.kind {
            TokenKind::Ident(name) => Ident { name, span: tok.span },
            TokenKind::Keyword(kw) if kw == Keyword::Naa || is_type_keyword(kw) => {
                Ident { name: kw.as_str().to_string(), span: tok.span }
            }
            _ => {
                self.unexpected("peru");
                return Err(());
            }
        };
        self.advance();
        let mut segments = vec![first];
        while self.at(&TokenKind::Dot) && matches!(self.peek_at(1), TokenKind::Ident(_)) {
            self.advance();
            segments.push(self.expect_ident("peru")?);
        }
        let span = join(segments[0].span, segments.last().expect("non-empty").span);
        Ok(Path { segments, span })
    }

    fn keyword_path(&self, kw: Keyword, span: Span) -> Path {
        Path { segments: vec![Ident { name: kw.as_str().to_string(), span }], span }
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        self.nested(|p| p.parse_primary_inner())
    }

    fn parse_primary_inner(&mut self) -> PResult<Expr> {
        let tok = self.toks[self.pos].clone();
        let span = tok.span;
        let kind = match tok.kind {
            TokenKind::Int(lit) => {
                self.advance();
                ExprKind::Int(lit)
            }
            TokenKind::Float(text) => {
                self.advance();
                ExprKind::Float(text)
            }
            TokenKind::Str(s) => {
                self.advance();
                ExprKind::Str(s)
            }
            TokenKind::Char(c) => {
                self.advance();
                ExprKind::Char(c)
            }
            TokenKind::FStr(parts) => {
                self.advance();
                ExprKind::FStr(self.parse_fstring_parts(parts))
            }
            TokenKind::Keyword(Keyword::Avunu) => {
                self.advance();
                ExprKind::Bool(true)
            }
            TokenKind::Keyword(Keyword::Kaadhu) => {
                self.advance();
                ExprKind::Bool(false)
            }
            TokenKind::Keyword(Keyword::Ledhu) => {
                self.advance();
                ExprKind::Ledhu
            }
            TokenKind::Keyword(kw @ (Keyword::Sare | Keyword::Undhi | Keyword::Thappu)) => {
                self.advance();
                if self.at(&TokenKind::LParen) && *self.peek_at(1) != TokenKind::RParen {
                    let args = self.parse_paren_args()?;
                    ExprKind::Ctor { path: self.keyword_path(kw, span), args }
                } else if kw == Keyword::Sare {
                    ExprKind::BareSare
                } else {
                    self.error(
                        "E0704",
                        span,
                        format!("`{}` ki oka value kavali: `{}(x)`", kw.as_str(), kw.as_str()),
                    );
                    return Err(());
                }
            }
            TokenKind::Underscore => {
                self.advance();
                if self.index_depth == 0 {
                    self.error(
                        "E0213",
                        span,
                        "`_` pipe stage argument ga or tensor index lo matrame vaadochu",
                    );
                    return Err(());
                }
                ExprKind::Hole
            }
            TokenKind::Ident(_) => return self.parse_path_expr(),
            TokenKind::Keyword(kw) if kw == Keyword::Naa || is_type_keyword(kw) => {
                return self.parse_path_expr()
            }
            TokenKind::LParen => {
                self.advance();
                if self.eat(&TokenKind::RParen) {
                    ExprKind::Unit
                } else {
                    let (mut elems, trailing) = self.parse_list_until(&TokenKind::RParen)?;
                    self.expect(&TokenKind::RParen, "`)`")?;
                    if elems.len() == 1 && !trailing {
                        let mut inner = elems.pop().expect("one element");
                        inner.span = self.since(span);
                        return Ok(inner);
                    }
                    ExprKind::Tuple(elems)
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let (elems, _) = self.parse_list_until(&TokenKind::RBracket)?;
                self.expect(&TokenKind::RBracket, "`]`")?;
                ExprKind::List(elems)
            }
            TokenKind::LBrace => {
                self.advance();
                let mut pairs = Vec::new();
                while !self.at(&TokenKind::RBrace) {
                    let key = self.parse_list_elem()?;
                    self.expect(&TokenKind::Colon, "`:`")?;
                    let value = self.parse_list_elem()?;
                    pairs.push((key, value));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RBrace, "`}`")?;
                ExprKind::Map(pairs)
            }
            _ => {
                self.unexpected("expression");
                return Err(());
            }
        };
        Ok(mk(kind, self.since(span)))
    }

    fn parse_path_expr(&mut self) -> PResult<Expr> {
        let start = self.span();
        let path = self.parse_path()?;
        if !self.at(&TokenKind::LBrace) {
            let span = path.span;
            return Ok(mk(ExprKind::Path(path), span));
        }
        self.advance();
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            let name = self.expect_ident("field peru")?;
            self.expect(&TokenKind::Colon, "`:`")?;
            let value = self.parse_list_elem()?;
            fields.push((name, value));
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "`}`")?;
        Ok(mk(ExprKind::Record { path, fields }, self.since(start)))
    }

    /// Parses each `{…}` of an f-string as its own clause (spec §4.5).
    fn parse_fstring_parts(&mut self, parts: Vec<FPart>) -> Vec<FStrPart> {
        let mut out = Vec::new();
        for part in parts {
            match part {
                FPart::Text(text) => out.push(FStrPart::Text(text)),
                FPart::Expr(span) => {
                    let text = &self.src.text[span.start..span.end];
                    let trimmed = text.trim_start();
                    let offset = span.start + (text.len() - trimmed.len());
                    let sub = SourceFile::new(self.src.name.clone(), trimmed.trim_end());
                    let lexed = lexer::lex(&sub);
                    let shift = |s: Span| Span::new(s.start + offset, s.end + offset);
                    if !lexed.diagnostics.is_empty() {
                        for mut d in lexed.diagnostics {
                            d.span = shift(d.span);
                            self.diags.push(d);
                        }
                        continue;
                    }
                    let tokens: Vec<Token> = lexed
                        .tokens
                        .into_iter()
                        .map(|mut t| {
                            t.span = shift(t.span);
                            if let TokenKind::FStr(inner) = &mut t.kind {
                                for p in inner {
                                    if let FPart::Expr(s) = p {
                                        *s = shift(*s);
                                    }
                                }
                            }
                            t
                        })
                        .collect();
                    let mut sub_parser = Parser::new(self.src, tokens);
                    let parsed = sub_parser.parse_pipe_expr().and_then(|e| {
                        sub_parser.expect_newline()?;
                        if sub_parser.at(&TokenKind::Eof) {
                            Ok(e)
                        } else {
                            sub_parser.unexpected("`}`");
                            Err(())
                        }
                    });
                    self.diags.append(&mut sub_parser.diags);
                    if let Ok(expr) = parsed {
                        out.push(FStrPart::Expr(expr));
                    }
                }
            }
        }
        out
    }

    // ── types ──────────────────────────────────────────────────────────────

    fn parse_type_args(&mut self) -> PResult<Vec<Type>> {
        self.expect(&TokenKind::LBracket, "`[`")?;
        let mut args = Vec::new();
        while !self.at(&TokenKind::RBracket) {
            args.push(self.parse_type()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "`]`")?;
        Ok(args)
    }

    fn parse_type(&mut self) -> PResult<Type> {
        self.nested(|p| p.parse_type_inner())
    }

    fn parse_type_inner(&mut self) -> PResult<Type> {
        let start = self.span();
        let tok = self.toks[self.pos].clone();
        let kind = match tok.kind {
            TokenKind::Amp | TokenKind::Star => {
                self.advance();
                let mutable = self.eat_kw(Keyword::Maare);
                let inner = Box::new(self.parse_type()?);
                if tok.kind == TokenKind::Amp {
                    TypeKind::Ref { mutable, inner }
                } else {
                    TypeKind::Ptr { mutable, inner }
                }
            }
            TokenKind::LBracket => {
                self.advance();
                let elem = Box::new(self.parse_type()?);
                self.expect(&TokenKind::Semi, "`;` and length")?;
                let len = Box::new(self.parse_binexpr(1)?);
                self.expect(&TokenKind::RBracket, "`]`")?;
                TypeKind::Array { elem, len }
            }
            TokenKind::Keyword(Keyword::Aasajeevi) => {
                self.advance();
                let int = self.toks[self.pos].clone();
                let name = match int.kind {
                    TokenKind::Keyword(
                        kw @ (Keyword::Ankam
                        | Keyword::Ankam8
                        | Keyword::Ankam16
                        | Keyword::Ankam32
                        | Keyword::Ankam64),
                    ) => {
                        self.advance();
                        format!("aasajeevi {}", kw.as_str())
                    }
                    _ => {
                        self.unexpected("`ankam` type (`aasajeevi ankam32` laaga)");
                        return Err(());
                    }
                };
                let span = self.since(start);
                TypeKind::Named {
                    path: Path { segments: vec![Ident { name, span }], span },
                    args: Vec::new(),
                }
            }
            TokenKind::Keyword(Keyword::Pani) => {
                self.advance();
                self.expect(&TokenKind::LBracket, "`[`")?;
                self.expect(&TokenKind::LParen, "`(`")?;
                let mut params = Vec::new();
                while !self.at(&TokenKind::RParen) {
                    let ty = self.parse_type()?;
                    let role = self.parse_role();
                    params.push((ty, role));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "`)`")?;
                self.expect(&TokenKind::Arrow, "`->`")?;
                let ret = Box::new(self.parse_type()?);
                self.expect(&TokenKind::RBracket, "`]`")?;
                TypeKind::Fn { params, ret }
            }
            TokenKind::Keyword(kw) if is_type_keyword(kw) => {
                let path = self.keyword_path(kw, tok.span);
                self.advance();
                let args = if self.at(&TokenKind::LBracket) { self.parse_type_args()? } else { Vec::new() };
                TypeKind::Named { path, args }
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path()?;
                let args = if self.at(&TokenKind::LBracket) { self.parse_type_args()? } else { Vec::new() };
                TypeKind::Named { path, args }
            }
            _ => {
                self.unexpected("type");
                return Err(());
            }
        };
        Ok(Type { kind, span: self.since(start) })
    }

    // ── patterns ───────────────────────────────────────────────────────────

    fn parse_pattern(&mut self) -> PResult<Pattern> {
        let start = self.span();
        let first = self.parse_alt_pattern()?;
        if !self.at(&TokenKind::Pipe) {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat(&TokenKind::Pipe) {
            alts.push(self.parse_alt_pattern()?);
        }
        Ok(Pattern { kind: PatternKind::Or(alts), span: self.since(start) })
    }

    /// A literal pattern bound, with an optional leading `-`.
    fn parse_literal_pattern(&mut self) -> PResult<Expr> {
        let start = self.span();
        let negative = self.eat(&TokenKind::Minus);
        let tok = self.toks[self.pos].clone();
        let kind = match tok.kind {
            TokenKind::Int(lit) => ExprKind::Int(lit),
            TokenKind::Float(text) => {
                self.error(
                    "E0705",
                    tok.span,
                    "float values tho match cheyyalem. `aithe` tho compare cheyyandi",
                );
                ExprKind::Float(text)
            }
            TokenKind::Str(s) if !negative => ExprKind::Str(s),
            TokenKind::Char(c) if !negative => ExprKind::Char(c),
            TokenKind::Keyword(Keyword::Avunu) if !negative => ExprKind::Bool(true),
            TokenKind::Keyword(Keyword::Kaadhu) if !negative => ExprKind::Bool(false),
            _ => {
                self.unexpected("literal");
                return Err(());
            }
        };
        self.advance();
        let lit = mk(kind, self.since(start));
        if negative {
            return Ok(mk(ExprKind::Unary { op: UnOp::Neg, operand: Box::new(lit) }, self.since(start)));
        }
        Ok(lit)
    }

    fn literal_value(e: &Expr) -> Option<i128> {
        match &e.kind {
            ExprKind::Int(lit) => i128::from_str_radix(&lit.digits, lit.radix).ok(),
            ExprKind::Char(c) => Some(*c as i128),
            ExprKind::Unary { op: UnOp::Neg, operand } => Self::literal_value(operand).map(|v| -v),
            _ => None,
        }
    }

    fn parse_alt_pattern(&mut self) -> PResult<Pattern> {
        self.nested(|p| p.parse_alt_pattern_inner())
    }

    fn parse_alt_pattern_inner(&mut self) -> PResult<Pattern> {
        let start = self.span();
        let kind = match self.peek().clone() {
            TokenKind::Underscore => {
                self.advance();
                PatternKind::Wild
            }
            TokenKind::Minus
            | TokenKind::Int(_)
            | TokenKind::Float(_)
            | TokenKind::Str(_)
            | TokenKind::Char(_)
            | TokenKind::Keyword(Keyword::Avunu)
            | TokenKind::Keyword(Keyword::Kaadhu) => {
                let lo = self.parse_literal_pattern()?;
                if self.eat_kw(Keyword::Nunchi) {
                    let hi = self.parse_literal_pattern()?;
                    let inclusive = if self.eat_kw(Keyword::Varaku) {
                        true
                    } else {
                        self.expect_kw(Keyword::Lopu)?;
                        false
                    };
                    let valid_kind =
                        matches!((Self::literal_value(&lo), Self::literal_value(&hi)), (Some(_), Some(_)));
                    match (Self::literal_value(&lo), Self::literal_value(&hi)) {
                        (Some(a), Some(b)) if a <= b => {}
                        _ => {
                            let span = self.since(start);
                            let msg = if valid_kind {
                                "range lo first value last value kanna ekkuva undakoodadhu"
                            } else {
                                "range pattern lo numbers or characters matrame vaadochu"
                            };
                            self.error("E0707", span, msg);
                        }
                    }
                    PatternKind::Range { lo, hi, inclusive }
                } else {
                    PatternKind::Literal(lo)
                }
            }
            TokenKind::Keyword(Keyword::Ledhu) => {
                let span = self.advance().span;
                PatternKind::Ctor { path: self.keyword_path(Keyword::Ledhu, span), args: None }
            }
            TokenKind::Keyword(kw @ (Keyword::Undhi | Keyword::Sare | Keyword::Thappu)) => {
                let span = self.advance().span;
                let path = self.keyword_path(kw, span);
                let args = self.parse_pattern_args()?;
                PatternKind::Ctor { path, args: Some(args) }
            }
            TokenKind::Keyword(Keyword::Maare) => {
                self.advance();
                PatternKind::Bind { mutable: true, name: self.expect_ident("binding peru")? }
            }
            TokenKind::LParen => {
                self.advance();
                let mut pats = Vec::new();
                let mut trailing = false;
                while !self.at(&TokenKind::RParen) {
                    pats.push(self.parse_pattern()?);
                    trailing = false;
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                    trailing = true;
                }
                self.expect(&TokenKind::RParen, "`)`")?;
                if pats.len() == 1 && !trailing {
                    let mut inner = pats.pop().expect("one pattern");
                    inner.span = self.since(start);
                    return Ok(inner);
                }
                PatternKind::Tuple(pats)
            }
            TokenKind::Ident(_) => {
                let path = self.parse_path()?;
                if self.at(&TokenKind::LBrace) {
                    self.advance();
                    let mut fields = Vec::new();
                    let mut rest = false;
                    while !self.at(&TokenKind::RBrace) {
                        if self.eat(&TokenKind::DotDot) {
                            rest = true;
                            break;
                        }
                        let name = self.expect_ident("field peru")?;
                        let pat =
                            if self.eat(&TokenKind::Colon) { Some(self.parse_pattern()?) } else { None };
                        fields.push((name, pat));
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RBrace, "`}`")?;
                    PatternKind::Record { path, fields, rest }
                } else if self.at(&TokenKind::LParen) || path.segments.len() > 1 {
                    if path.segments.len() == 1 {
                        let name = path.segments[0].name.clone();
                        self.error(
                            "E0201",
                            path.span,
                            format!("variant ni type peru tho raayali, ex: `Rangu.{name}(…)`"),
                        );
                    }
                    let args =
                        if self.at(&TokenKind::LParen) { Some(self.parse_pattern_args()?) } else { None };
                    PatternKind::Ctor { path, args }
                } else {
                    let name = path.segments.into_iter().next().expect("one segment");
                    PatternKind::Bind { mutable: false, name }
                }
            }
            _ => {
                self.unexpected("pattern");
                return Err(());
            }
        };
        Ok(Pattern { kind, span: self.since(start) })
    }

    fn parse_pattern_args(&mut self) -> PResult<Vec<Pattern>> {
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut args = Vec::new();
        while !self.at(&TokenKind::RParen) {
            args.push(self.parse_pattern()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`)`")?;
        Ok(args)
    }
}
