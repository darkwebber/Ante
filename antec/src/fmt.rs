//! `antec fmt`: canonical formatting (spec §7.4, D4).
//!
//! The file is parsed and printed back from its syntax tree: four-space
//! indentation, single spaces around binary operators and after commas, the
//! parentheses that D4 and precedence need (and a few for reading, such as
//! `(a + b) icchey`), at most one blank line in a row, and one blank line
//! between top-level items. Literals keep their exact spelling. Comments
//! stay where they were: on their own line before the code that follows
//! them, at the end of the line they ended, or at the end of their block.
//!
//! The result is checked before it's used: it must parse to the same tree
//! (and so evaluate in the same order) and formatting it again must not
//! change it. If either check fails, nothing is written.

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::source::{SourceFile, Span};
use crate::token::quote;

const INDENT: &str = "    ";

/// Signatures longer than this put one parameter on each line.
const MAX_WIDTH: usize = 100;

/// Why a file couldn't be formatted.
pub enum FmtError {
    /// The file has errors; fix them first.
    Invalid(Vec<Diagnostic>),
    /// The formatter produced something different; this is an `antec` bug.
    Bug(String),
}

/// Formats a whole file.
pub fn format_source(src: &SourceFile) -> Result<String, FmtError> {
    let (module, comments) = parse(src)?;
    let out = print(&src.text, &module, &comments);
    // The output must mean exactly the same program …
    let again = SourceFile::new(src.name.clone(), out.clone());
    let (module2, comments2) = parse(&again).map_err(|e| match e {
        FmtError::Invalid(d) => FmtError::Bug(format!(
            "formatted code parse avvaledhu: {}",
            d.first().map(|d| d.message.as_str()).unwrap_or("?")
        )),
        bug => bug,
    })?;
    if crate::pretty::module(&module) != crate::pretty::module(&module2) {
        return Err(FmtError::Bug("formatted code ki syntax tree maarindhi".into()));
    }
    if comments.len() != comments2.len() {
        return Err(FmtError::Bug("comments konni poyayi".into()));
    }
    // … and be a fixed point.
    if print(&out, &module2, &comments2) != out {
        return Err(FmtError::Bug("rendosaari format chesthe maarindhi".into()));
    }
    Ok(out)
}

fn parse(src: &SourceFile) -> Result<(Module, Vec<Span>), FmtError> {
    let lexed = crate::lexer::lex(src);
    if lexed.diagnostics.iter().any(Diagnostic::is_error) {
        return Err(FmtError::Invalid(lexed.diagnostics));
    }
    let comments = lexed.comments;
    let parsed = crate::parser::parse(src, lexed.tokens);
    if parsed.diagnostics.iter().any(Diagnostic::is_error) {
        return Err(FmtError::Invalid(parsed.diagnostics));
    }
    Ok((parsed.module, comments))
}

fn print(text: &str, module: &Module, comments: &[Span]) -> String {
    let mut p = Printer::new(text, comments);
    p.module(module);
    p.out
}

struct Comment {
    span: Span,
    line: usize,
    col: usize,
    own_line: bool,
}

struct Printer<'a> {
    text: &'a str,
    line_starts: Vec<usize>,
    comments: Vec<Comment>,
    next: usize,
    out: String,
    /// Source position of the last thing printed (for blank lines).
    last: Option<usize>,
    /// `Some(true)`: the next line gets a blank line before it regardless.
    force_blank: Option<bool>,
    /// The next line is the first in a block: no blank line before it.
    no_blank: bool,
}

impl<'a> Printer<'a> {
    fn new(text: &'a str, comments: &[Span]) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let mut p = Printer {
            text,
            line_starts,
            comments: Vec::new(),
            next: 0,
            out: String::new(),
            last: None,
            force_blank: None,
            no_blank: false,
        };
        p.comments = comments
            .iter()
            .map(|&span| {
                let line = p.line_of(span.start);
                let col = span.start - p.line_starts[line];
                let own_line = text[p.line_starts[line]..span.start].trim().is_empty();
                Comment { span, line, col, own_line }
            })
            .collect();
        p
    }

    fn line_of(&self, pos: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    fn src(&self, span: Span) -> &'a str {
        &self.text[span.start..span.end]
    }

    /// A literal's spelling. A parenthesized literal's span covers its
    /// parentheses (`(0.0).x`); the printer adds them back where needed.
    fn literal(&self, span: Span) -> String {
        let mut t = self.src(span).trim();
        while let Some(inner) = t.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            t = inner.trim();
        }
        t.to_string()
    }

    /// Is there an empty source line between `a` and `b`?
    fn gap(&self, a: usize, b: usize) -> bool {
        if a >= b {
            return false;
        }
        let (la, lb) = (self.line_of(a), self.line_of(b));
        (la + 1..lb).any(|l| {
            let end = self.line_starts.get(l + 1).copied().unwrap_or(self.text.len());
            self.text[self.line_starts[l]..end].trim().is_empty()
        })
    }

    fn write_line(&mut self, depth: usize, text: &str, src_pos: usize) {
        let gap = self.last.is_some_and(|l| self.gap(l, src_pos));
        let blank = match self.force_blank.take() {
            Some(forced) => forced || gap,
            None => !self.no_blank && gap,
        };
        self.no_blank = false;
        if blank && !self.out.is_empty() {
            self.out.push('\n');
        }
        for _ in 0..depth {
            self.out.push_str(INDENT);
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn comment_text(&self, i: usize) -> &'a str {
        self.src(self.comments[i].span).trim_end()
    }

    /// Prints the comments that come before source line `line` on their own lines.
    fn flush_before_line(&mut self, depth: usize, line: usize) {
        while self.next < self.comments.len() && self.comments[self.next].line < line {
            let i = self.next;
            self.next += 1;
            let text = self.comment_text(i);
            let start = self.comments[i].span.start;
            self.write_line(depth, text, start);
            self.last = Some(self.comments[i].span.end);
        }
    }

    /// Prints one line of code standing for source `start..end`, with the
    /// comments before it and the one at the end of its last source line.
    fn code(&mut self, depth: usize, text: &str, start: usize, end: usize) {
        let end_line = self.line_of(end);
        self.flush_before_line(depth, end_line);
        self.write_line(depth, text, start);
        self.last = Some(end);
        if self.next < self.comments.len() {
            let c = &self.comments[self.next];
            if c.line == end_line && !c.own_line {
                let text = self.comment_text(self.next);
                self.last = Some(c.span.end);
                self.next += 1;
                self.out.pop();
                self.out.push_str("  ");
                self.out.push_str(text);
                self.out.push('\n');
            }
        }
    }

    /// Comments after a block's last statement, indented at least as far as
    /// the block, stay at its end.
    /// `after` is where the block's code ends.
    fn block_end(&mut self, depth: usize, block_col: usize, after: usize) {
        let limit = self.next_code(after);
        while self.next < self.comments.len() {
            let c = &self.comments[self.next];
            if !c.own_line || c.col < block_col || c.span.start >= limit {
                break;
            }
            let (start, end) = (c.span.start, c.span.end);
            let text = self.comment_text(self.next);
            self.next += 1;
            self.write_line(depth, text, start);
            self.last = Some(end);
        }
    }

    // ── items ──────────────────────────────────────────────────────────────

    fn module(&mut self, m: &Module) {
        let mut prev_import: Option<bool> = None;
        for item in &m.items {
            let is_import = matches!(item.kind, ItemKind::Import(_));
            if let Some(prev) = prev_import {
                self.force_blank = Some(!(prev && is_import));
            }
            prev_import = Some(is_import);
            self.item(item, 0);
        }
        let rest = self.line_starts.len() + 1;
        self.flush_before_line(0, rest);
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
    }

    fn annotations(&mut self, annotations: &[Annotation], depth: usize) {
        for a in annotations {
            let arg = match &a.arg {
                AnnotationArg::None => String::new(),
                // The string as written, so its escapes don't change.
                AnnotationArg::Str(_) => format!(" {}", self.text[a.name.span.end..a.span.end].trim()),
                AnnotationArg::Names(ns) => format!(" {}", idents(ns)),
            };
            self.code(depth, &format!("@{}{arg}", a.name.name), a.span.start, a.span.end);
        }
    }

    fn item(&mut self, item: &Item, depth: usize) {
        self.annotations(&item.annotations, depth);
        let public = if item.public { "andhariki " } else { "" };
        let start = item.span.start;
        match &item.kind {
            ItemKind::Import(imp) => {
                let m = path(&imp.module);
                let text = match &imp.kind {
                    ImportKind::Module => format!("{m} thechuko"),
                    ImportKind::Items(names) => format!("{m} nunchi {} thechuko", idents(names)),
                    ImportKind::Alias(a) => format!("{m} ni {} ga thechuko", a.name),
                };
                self.code(depth, &text, start, item.span.end);
            }
            ItemKind::Fn(f) => self.fn_decl(f, public, depth, start),
            ItemKind::Const { name, ty, value } => {
                let text =
                    format!("{public}{}: {} ante {}", name.name, self.ty(ty), self.expr(value, Ctx::Pipe));
                self.code(depth, &text, start, item.span.end);
            }
            ItemKind::Record(r) => {
                let head = format!("{public}rakam {}{} ante:", r.name.name, self.generics(&r.generics));
                self.code(depth, &head, start, r.name.span.end);
                self.no_blank = true;
                let col = r.fields.first().map(|f| self.col(f.name.span.start)).unwrap_or(0);
                for f in &r.fields {
                    self.annotations(&f.annotations, depth + 1);
                    let p = if f.public { "andhariki " } else { "" };
                    let text = format!("{p}{}: {}", f.name.name, self.ty(&f.ty));
                    self.code(depth + 1, &text, f.name.span.start, f.ty.span.end);
                }
                self.block_end(depth + 1, col, item.span.end);
            }
            ItemKind::Enum(e) => {
                let head = format!("{public}edho_okati {}{} ante:", e.name.name, self.generics(&e.generics));
                self.code(depth, &head, start, e.name.span.end);
                self.no_blank = true;
                let col = e.variants.first().map(|v| self.col(v.name.span.start)).unwrap_or(0);
                for v in &e.variants {
                    self.annotations(&v.annotations, depth + 1);
                    let text = if v.fields.is_empty() {
                        v.name.name.clone()
                    } else {
                        let fields: Vec<String> = v.fields.iter().map(|t| self.ty(t)).collect();
                        format!("{}({})", v.name.name, fields.join(", "))
                    };
                    let end = v.fields.last().map(|t| t.span.end).unwrap_or(v.name.span.end);
                    self.code(depth + 1, &text, v.name.span.start, end);
                }
                self.block_end(depth + 1, col, item.span.end);
            }
            ItemKind::Methods(m) => {
                let g = self.generics(&m.generics);
                let g = if g.is_empty() { g } else { format!("{g} ") };
                let head = format!("{g}{} panulu ante:", self.ty(&m.self_ty));
                self.code(depth, &head, start, m.self_ty.span.end);
                self.no_blank = true;
                let col = m.items.first().map(|i| self.col(i.span.start)).unwrap_or(0);
                for (k, it) in m.items.iter().enumerate() {
                    if k > 0 {
                        self.force_blank = Some(true);
                    }
                    self.item(it, depth + 1);
                }
                self.block_end(depth + 1, col, item.span.end);
            }
            ItemKind::Trait(t) => {
                let head = format!("{public}gunam {}{} ante:", t.name.name, self.generics(&t.generics));
                self.code(depth, &head, start, t.name.span.end);
                self.no_blank = true;
                let col = t.items.first().map(|i| self.col(i.sig.span.start)).unwrap_or(0);
                for (k, ti) in t.items.iter().enumerate() {
                    if k > 0 && (ti.default_body.is_some() || t.items[k - 1].default_body.is_some()) {
                        self.force_blank = Some(true);
                    }
                    match &ti.default_body {
                        None => {
                            let text = self.wrapped_sig(&ti.sig, "", "", depth + 1);
                            self.code(depth + 1, &text, ti.sig.span.start, ti.sig.span.end);
                        }
                        Some(body) => {
                            let head = self.wrapped_sig(&ti.sig, "", " ante:", depth + 1);
                            self.code(depth + 1, &head, ti.sig.span.start, body.span.start);
                            self.block(body, depth + 2);
                        }
                    }
                }
                self.block_end(depth + 1, col, item.span.end);
            }
            ItemKind::Impl(i) if i.naa_risk => {
                self.code(depth, "naa_risk:", start, start);
                self.no_blank = true;
                let plain = ImplDecl { naa_risk: false, ..i.clone() };
                let inner = Item { kind: ItemKind::Impl(plain), ..item.clone() };
                self.item(&inner, depth + 1);
            }
            ItemKind::Impl(i) => {
                let g = self.generics(&i.generics);
                let g = if g.is_empty() { g } else { format!("{g} ") };
                let head = format!("{g}{} ki {} gunam undhi", self.ty(&i.self_ty), self.ty(&i.trait_ref));
                if i.items.is_empty() {
                    self.code(depth, &head, start, item.span.end);
                } else {
                    self.code(depth, &format!("{head}:"), start, i.trait_ref.span.end);
                    self.no_blank = true;
                    let col = self.col(i.items[0].sig.span.start);
                    for (k, f) in i.items.iter().enumerate() {
                        if k > 0 {
                            self.force_blank = Some(true);
                        }
                        self.fn_decl(f, "", depth + 1, f.sig.span.start);
                    }
                    self.block_end(depth + 1, col, item.span.end);
                }
            }
            ItemKind::Extern(e) => {
                let head = format!("{public}bayata {} ante:", quote(&e.abi, '"'));
                self.code(depth, &head, start, start);
                self.no_blank = true;
                let col = e.fns.first().map(|f| self.col(f.span.start)).unwrap_or(0);
                for f in &e.fns {
                    let text = self.wrapped_sig(f, "", "", depth + 1);
                    self.code(depth + 1, &text, f.span.start, f.span.end);
                }
                self.block_end(depth + 1, col, item.span.end);
            }
        }
    }

    /// The first code (not space or comment) at or after `pos`.
    fn next_code(&self, pos: usize) -> usize {
        let b = self.text.as_bytes();
        let mut i = pos;
        while i < b.len() {
            match b[i] {
                b' ' | b'\t' | b'\r' | b'\n' => i += 1,
                b'#' => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => break,
            }
        }
        i
    }

    fn col(&self, pos: usize) -> usize {
        pos - self.line_starts[self.line_of(pos)]
    }

    fn fn_decl(&mut self, f: &FnDecl, public: &str, depth: usize, start: usize) {
        let head = self.wrapped_sig(&f.sig, public, " ante:", depth);
        self.code(depth, &head, start, f.body.span.start);
        self.block(&f.body, depth + 1);
    }

    fn generics(&self, gs: &[GenericParam]) -> String {
        if gs.is_empty() {
            return String::new();
        }
        let parts: Vec<String> = gs
            .iter()
            .map(|g| {
                if g.is_const {
                    format!("{}: ankam", g.name.name)
                } else if g.bounds.is_empty() {
                    g.name.name.clone()
                } else {
                    let b: Vec<String> = g.bounds.iter().map(|t| self.ty(t)).collect();
                    format!("{}: {}", g.name.name, b.join(" + "))
                }
            })
            .collect();
        format!("[{}]", parts.join(", "))
    }

    /// A signature line, or (when too long) one parameter per line inside
    /// the brackets, which join into one logical line (§4.2.3).
    fn wrapped_sig(&self, s: &FnSig, prefix: &str, suffix: &str, depth: usize) -> String {
        let flat = format!("{prefix}{}{suffix}", self.sig(s));
        if flat.chars().count() + depth * INDENT.len() <= MAX_WIDTH || s.params.is_empty() {
            return flat;
        }
        let inner = INDENT.repeat(depth + 1);
        let close = INDENT.repeat(depth);
        let params: String = s.params.iter().map(|p| format!("\n{inner}{},", self.param(p))).collect();
        format!(
            "{prefix}{}{}({params}\n{close}) -> {}{suffix}",
            s.name.name,
            self.generics(&s.generics),
            self.ty(&s.ret)
        )
    }

    fn sig(&self, s: &FnSig) -> String {
        let params: Vec<String> = s.params.iter().map(|p| self.param(p)).collect();
        format!("{}{}({}) -> {}", s.name.name, self.generics(&s.generics), params.join(", "), self.ty(&s.ret))
    }

    fn param(&self, p: &Param) -> String {
        let mut s = String::new();
        match p.kind {
            ParamKind::Variadic => s.push_str("migathavi "),
            ParamKind::NamedCollector => s.push_str("migatha_perlu "),
            ParamKind::Receiver | ParamKind::Normal => {}
        }
        s.push_str(&p.name.name);
        s.push_str(": ");
        s.push_str(&self.ty(&p.ty));
        if let Some(r) = p.role {
            s.push(' ');
            s.push_str(r.as_str());
        }
        if p.keyword_only {
            s.push_str(" peru_thone");
        }
        if let Some(d) = &p.default {
            s.push_str(" = ");
            s.push_str(&self.expr(d, Ctx::Bin(1, false)));
        }
        s
    }

    fn ty(&self, t: &Type) -> String {
        match &t.kind {
            TypeKind::Named { path: p, args } if args.is_empty() => path(p),
            TypeKind::Named { path: p, args } => {
                let a: Vec<String> = args.iter().map(|t| self.ty(t)).collect();
                format!("{}[{}]", path(p), a.join(", "))
            }
            TypeKind::Ref { mutable, inner } => {
                let inner = self.ty(inner);
                // `& &T`, not `&&T`, which lexes as the `&&` operator.
                let gap = if !*mutable && inner.starts_with('&') { " " } else { "" };
                format!("&{}{gap}{inner}", if *mutable { "maare " } else { "" })
            }
            TypeKind::Ptr { mutable, inner } => {
                format!("*{}{}", if *mutable { "maare " } else { "" }, self.ty(inner))
            }
            TypeKind::Array { elem, len } => format!("[{}; {}]", self.ty(elem), self.expr(len, Ctx::Pipe)),
            TypeKind::Fn { params, ret } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|(t, role)| match role {
                        Some(r) => format!("{} {}", self.ty(t), r.as_str()),
                        None => self.ty(t),
                    })
                    .collect();
                format!("pani[({}) -> {}]", ps.join(", "), self.ty(ret))
            }
        }
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn block(&mut self, b: &Block, depth: usize) {
        self.no_blank = true;
        let col = b.stmts.first().map(|s| self.col(s.span.start)).unwrap_or(0);
        for s in &b.stmts {
            self.stmt(s, depth);
        }
        self.block_end(depth, col, b.span.end);
    }

    fn stmt(&mut self, s: &Stmt, depth: usize) {
        let (start, end) = (s.span.start, s.span.end);
        match &s.kind {
            StmtKind::Binding { mutable, target, ty, value } => {
                let m = if *mutable { "maare " } else { "" };
                let target = match target {
                    BindTarget::Name(n) => n.name.clone(),
                    BindTarget::Tuple(ns) => format!("({})", idents(ns)),
                };
                let t = ty.as_ref().map(|t| format!(": {}", self.ty(t))).unwrap_or_default();
                let text = format!("{m}{target}{t} ante {}", self.expr(value, Ctx::Pipe));
                self.code(depth, &text, start, end);
            }
            StmtKind::Assign { place, value } => {
                let text = format!("{} lo {} pettu", self.expr_bare(place), self.expr(value, Ctx::Phrase));
                self.code(depth, &text, start, end);
            }
            StmtKind::Compound { place, op, value } => {
                let text =
                    format!("{} {}= {}", self.expr_bare(place), op.as_str(), self.expr(value, Ctx::Pipe));
                self.code(depth, &text, start, end);
            }
            StmtKind::Return(None) => self.code(depth, "icchey", start, end),
            StmtKind::Return(Some(e)) => {
                let text = format!("{} icchey", self.expr(e, Ctx::Phrase));
                self.code(depth, &text, start, end);
            }
            StmtKind::If { branches, else_block } => {
                for (k, (cond, body)) in branches.iter().enumerate() {
                    let word = if k == 0 { "" } else { "ledhante " };
                    let text = format!("{word}{} aithe:", self.expr(cond, Ctx::Pipe));
                    let at = if k == 0 { start } else { cond.span.start };
                    self.code(depth, &text, at, body.span.start);
                    self.block(body, depth + 1);
                }
                if let Some(body) = else_block {
                    self.code(depth, "ledhante:", body.span.start, body.span.start);
                    self.block(body, depth + 1);
                }
            }
            StmtKind::While { cond, body } => {
                let text = format!("{} unnanthasepu:", self.expr(cond, Ctx::Pipe));
                self.code(depth, &text, start, body.span.start);
                self.block(body, depth + 1);
            }
            StmtKind::For { iter, pattern, body } => {
                let text = match &iter.kind {
                    ExprKind::Range { .. } => {
                        format!("{} okko {} ki:", self.expr(iter, Ctx::Pipe), self.pattern(pattern))
                    }
                    _ => format!("{} lo okko {} ki:", self.expr(iter, Ctx::Pipe), self.pattern(pattern)),
                };
                self.code(depth, &text, start, body.span.start);
                self.block(body, depth + 1);
            }
            StmtKind::Match { scrutinee, arms } => {
                let text = format!("{} batti:", self.expr(scrutinee, Ctx::Pipe));
                self.code(depth, &text, start, scrutinee.span.end);
                self.no_blank = true;
                let col = arms.first().map(|a| self.col(a.pattern.span.start)).unwrap_or(0);
                for arm in arms {
                    let text = format!("{} aithe:", self.pattern(&arm.pattern));
                    self.code(depth + 1, &text, arm.pattern.span.start, arm.body.span.start);
                    self.block(&arm.body, depth + 2);
                }
                self.block_end(depth + 1, col, end);
            }
            StmtKind::Break => self.code(depth, "aapey", start, end),
            StmtKind::Continue => self.code(depth, "vadhiley", start, end),
            StmtKind::Defer(b) => {
                self.code(depth, "vellagane:", start, b.span.start);
                self.block(b, depth + 1);
            }
            StmtKind::Unsafe(b) => {
                self.code(depth, "naa_risk:", start, b.span.start);
                self.block(b, depth + 1);
            }
            StmtKind::Expr(e) => {
                let text = self.expr(e, Ctx::Pipe);
                self.code(depth, &text, start, end);
            }
        }
    }

    // ── expressions ────────────────────────────────────────────────────────

    fn expr(&self, e: &Expr, ctx: Ctx) -> String {
        let text = self.expr_bare(e);
        if needs_parens(e, ctx) {
            format!("({text})")
        } else {
            text
        }
    }

    fn expr_bare(&self, e: &Expr) -> String {
        match &e.kind {
            // Literals keep their spelling (`0xFF`, `1_000`, escapes).
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::FStr(_)
            | ExprKind::Sql(_) => self.literal(e.span),
            ExprKind::Bool(true) => "avunu".into(),
            ExprKind::Bool(false) => "kaadhu".into(),
            ExprKind::Ledhu => "ledhu".into(),
            ExprKind::BareSare => "sare".into(),
            ExprKind::Unit => "()".into(),
            ExprKind::Hole => "_".into(),
            ExprKind::Path(p) => path(p),
            ExprKind::Call { callee, type_args, phrases } => {
                let mut s = String::new();
                if !phrases.is_empty() {
                    let ps: Vec<String> = phrases.iter().map(|p| self.phrase(p)).collect();
                    s.push_str(&ps.join(", "));
                    s.push(' ');
                }
                // `(xs[0])()`: without the parentheses it's a generic call (D7).
                let callee_text = self.expr(callee, Ctx::Postfix);
                if matches!(callee.kind, ExprKind::Index { .. }) && type_args.is_empty() {
                    s.push_str(&format!("({callee_text})"));
                } else {
                    s.push_str(&callee_text);
                }
                if !type_args.is_empty() {
                    let ts: Vec<String> = type_args.iter().map(|t| self.ty(t)).collect();
                    s.push_str(&format!("[{}]", ts.join(", ")));
                }
                s.push_str("()");
                s
            }
            ExprKind::Try(inner) => {
                if is_phrase_call(inner) {
                    format!("{}?", self.expr_bare(inner))
                } else {
                    format!("{}?", self.expr(inner, Ctx::Postfix))
                }
            }
            ExprKind::Pipe { input, stage } => {
                format!("{} |> {}", self.expr(input, Ctx::Pipe), self.expr(stage, Ctx::Stage))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let p = prec(*op);
                format!(
                    "{} {} {}",
                    self.expr(lhs, Ctx::Bin(p, false)),
                    op.as_str(),
                    self.expr(rhs, Ctx::Bin(p, true))
                )
            }
            ExprKind::Unary { op, operand } => {
                let op = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                    UnOp::BitNot => "~",
                    UnOp::Deref => "*",
                    UnOp::Ref => "&",
                    UnOp::RefMut => "&maare ",
                };
                format!("{op}{}", self.expr(operand, Ctx::Unary))
            }
            ExprKind::Field { base, name } => format!("{}.{}", self.expr(base, Ctx::Postfix), name.name),
            ExprKind::TupleIndex { base, index } => format!("{}.{index}", self.expr(base, Ctx::Postfix)),
            ExprKind::Index { base, indices } => {
                format!("{}[{}]", self.expr(base, Ctx::Postfix), self.list(indices))
            }
            ExprKind::Range { lo, hi, inclusive } => format!(
                "{} nunchi {} {}",
                self.expr(lo, Ctx::Bin(1, false)),
                self.expr(hi, Ctx::Bin(1, false)),
                if *inclusive { "varaku" } else { "lopu" }
            ),
            ExprKind::Closure { moves, params, body } => {
                let ps: Vec<String> = params
                    .iter()
                    .map(|p| {
                        let mut s = p.name.name.clone();
                        if let Some(t) = &p.ty {
                            s.push_str(": ");
                            s.push_str(&self.ty(t));
                        }
                        if let Some(r) = p.role {
                            s.push(' ');
                            s.push_str(r.as_str());
                        }
                        s
                    })
                    .collect();
                let m = if *moves { "theesuko " } else { "" };
                format!("{m}({}) => {}", ps.join(", "), self.expr(body, Ctx::Pipe))
            }
            ExprKind::Ctor { path: p, args } => format!("{}({})", path(p), self.list(args)),
            ExprKind::Record { path: p, fields } => {
                let many = fields.len() > 1;
                let fs: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(k, (n, v))| {
                        let ctx = if many { Ctx::Element(k > 0) } else { Ctx::Pipe };
                        format!("{}: {}", n.name, self.expr(v, ctx))
                    })
                    .collect();
                format!("{}{{{}}}", path(p), fs.join(", "))
            }
            ExprKind::Tuple(items) if items.len() == 1 => format!("({},)", self.list(items)),
            ExprKind::Tuple(items) => format!("({})", self.list(items)),
            ExprKind::List(items) => format!("[{}]", self.list(items)),
            ExprKind::Map(pairs) => {
                let many = pairs.len() > 1;
                let ps: Vec<String> = pairs
                    .iter()
                    .enumerate()
                    .map(|(i, (k, v))| {
                        let ctx = if many { Ctx::Element(i > 0) } else { Ctx::Pipe };
                        // A key is followed by `:`, so it can't be a bare call clause.
                        format!("{}: {}", self.expr(k, Ctx::Element(true)), self.expr(v, ctx))
                    })
                    .collect();
                format!("{{{}}}", ps.join(", "))
            }
        }
    }

    /// Comma-separated list elements (D4): a call clause after the first
    /// element is parenthesized, or its phrases would swallow the elements
    /// before it.
    fn list(&self, items: &[Expr]) -> String {
        let parts: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(k, e)| {
                let ctx = if items.len() > 1 { Ctx::Element(k > 0) } else { Ctx::Pipe };
                self.expr(e, ctx)
            })
            .collect();
        parts.join(", ")
    }

    fn phrase(&self, p: &Phrase) -> String {
        let mut s = match &p.kind {
            PhraseKind::Value(e) => self.expr(e, Ctx::Phrase),
            PhraseKind::Spread(e) => format!("...{}", self.expr(e, Ctx::Phrase)),
            PhraseKind::Hole => "_".into(),
        };
        if let Some(r) = p.role {
            s.push(' ');
            s.push_str(r.as_str());
        }
        if let Some(l) = &p.label {
            s.push(' ');
            s.push_str(&l.name);
            s.push_str(" ga");
        }
        s
    }

    // ── patterns ───────────────────────────────────────────────────────────

    fn pattern(&self, p: &Pattern) -> String {
        match &p.kind {
            PatternKind::Wild => "_".into(),
            PatternKind::Literal(e) => self.literal(e.span),
            PatternKind::Range { lo, hi, inclusive } => format!(
                "{} nunchi {} {}",
                self.src(lo.span),
                self.src(hi.span),
                if *inclusive { "varaku" } else { "lopu" }
            ),
            PatternKind::Bind { mutable, name } => {
                format!("{}{}", if *mutable { "maare " } else { "" }, name.name)
            }
            PatternKind::Ctor { path: pa, args: None } => path(pa),
            PatternKind::Ctor { path: pa, args: Some(args) } => {
                let a: Vec<String> = args.iter().map(|p| self.pattern(p)).collect();
                format!("{}({})", path(pa), a.join(", "))
            }
            PatternKind::Record { path: pa, fields, rest } => {
                let mut fs: Vec<String> = fields
                    .iter()
                    .map(|(n, sub)| match sub {
                        Some(sub) => format!("{}: {}", n.name, self.pattern(sub)),
                        None => n.name.clone(),
                    })
                    .collect();
                if *rest {
                    fs.push("..".into());
                }
                format!("{}{{{}}}", path(pa), fs.join(", "))
            }
            PatternKind::Tuple(ps) if ps.len() == 1 => format!("({},)", self.pattern(&ps[0])),
            PatternKind::Tuple(ps) => {
                let a: Vec<String> = ps.iter().map(|p| self.pattern(p)).collect();
                format!("({})", a.join(", "))
            }
            PatternKind::Or(ps) => {
                let a: Vec<String> = ps.iter().map(|p| self.pattern(p)).collect();
                a.join(" | ")
            }
        }
    }
}

/// Where an expression appears, which decides the parentheses it needs.
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    /// A whole `pipe_expr`: a binding's value, a condition, a statement.
    Pipe,
    /// A pipe stage (must be a call clause).
    Stage,
    /// A phrase operand, or the value before `icchey`/`pettu`.
    Phrase,
    /// An operand of a binary operator of this binding power, left or right.
    Bin(u8, bool),
    /// The operand of a prefix operator.
    Unary,
    /// The base of `.f`, `[i]`, `()` or `?`.
    Postfix,
    /// An element of a list with several elements; `true` after the first.
    Element(bool),
}

fn prec(op: BinOp) -> u8 {
    use BinOp::*;
    match op {
        Mul | Div | Rem | MatMul => 10,
        Add | Sub => 9,
        Shl | Shr => 8,
        BitAnd => 7,
        BitXor => 6,
        BitOr => 5,
        Lt | Le | Gt | Ge => 4,
        Eq | Ne => 3,
        And => 2,
        Or => 1,
    }
}

fn is_phrase_call(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Call { phrases, .. } => !phrases.is_empty(),
        ExprKind::Try(inner) => is_phrase_call(inner),
        _ => false,
    }
}

fn needs_parens(e: &Expr, ctx: Ctx) -> bool {
    let pipe = matches!(e.kind, ExprKind::Pipe { .. });
    let clause = is_phrase_call(e);
    let wide = matches!(e.kind, ExprKind::Range { .. } | ExprKind::Closure { .. });
    match ctx {
        Ctx::Pipe => false,
        Ctx::Stage => pipe,
        // Closures go in parentheses (D8); binary operands too, so the case
        // marker reads as belonging to the whole value: `(a + b) ni f()`.
        Ctx::Phrase => pipe || clause || wide || matches!(e.kind, ExprKind::Binary { .. }),
        Ctx::Element(after_first) => {
            pipe || matches!(e.kind, ExprKind::Closure { .. }) || (after_first && clause)
        }
        Ctx::Bin(p, right) => {
            pipe || clause
                || wide
                || match &e.kind {
                    ExprKind::Binary { op, .. } => {
                        let q = prec(*op);
                        q < p || (q == p && (right || p == 3 || p == 4))
                    }
                    _ => false,
                }
        }
        Ctx::Unary => {
            pipe || clause || wide || matches!(e.kind, ExprKind::Binary { .. } | ExprKind::Unary { .. })
        }
        Ctx::Postfix => {
            pipe || clause
                || wide
                || matches!(
                    e.kind,
                    ExprKind::Binary { .. } | ExprKind::Unary { .. } | ExprKind::Int(_) | ExprKind::Float(_)
                )
        }
    }
}

fn path(p: &Path) -> String {
    p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

fn idents(ns: &[Ident]) -> String {
    ns.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", ")
}
