//! Lints: the warnings of spec Appendix B. They run after name resolution,
//! only when a program has no errors, and never stop a build.
//!
//! - W0001 unused binding (silence it with a leading `_`)
//! - W0002 `maare` on a binding that is never changed
//! - W0003 unreachable code after `icchey`, `aapey`, `vadhiley` or a panic
//! - W0005 unused import

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::resolve::{DefId, FileId, Res, Resolution, SourceModule};

pub fn lint(modules: &[SourceModule], res: &Resolution) -> Vec<(FileId, Diagnostic)> {
    let mut out = Vec::new();

    // Every resolution except a declaration's own entry counts as a use.
    let mut used_locals: HashSet<(FileId, usize)> = HashSet::new();
    let mut def_uses: HashMap<(FileId, DefId), Vec<usize>> = HashMap::new();
    for (&(file, start), r) in &res.resolutions {
        match r {
            Res::Local(decl) if decl.start != start => {
                used_locals.insert((file, decl.start));
            }
            Res::Def(d) => def_uses.entry((file, *d)).or_default().push(start),
            _ => {}
        }
    }

    for import in &res.imports {
        let used = def_uses
            .get(&(import.file, import.def))
            .is_some_and(|uses| uses.iter().any(|&start| start != import.span.start));
        if !used {
            out.push((
                import.file,
                Diagnostic::warning(
                    "W0005",
                    import.span,
                    format!("`{}` import chesaru kaani ekkada vaadaledhu. Theeseyandi 🧹", import.name),
                ),
            ));
        }
    }

    let mut walker = Walker { file: 0, res, mutated: HashSet::new(), out: Vec::new() };
    for module in modules {
        if let Some(ast) = module.ast {
            walker.file = module.file;
            walker.module(ast);
        }
    }
    out.append(&mut walker.out);

    for local in &res.locals {
        if local.name.starts_with('_') {
            continue;
        }
        let key = (local.file, local.span.start);
        if !used_locals.contains(&key) {
            out.push((
                local.file,
                Diagnostic::warning(
                    "W0001",
                    local.span,
                    format!(
                        "`{name}` define chesaru kaani vaadaledhu. Avasaram lekapothe theeseyandi, leka `_{name}` ani peru pettandi",
                        name = local.name
                    ),
                ),
            ));
        } else if local.mutable && !walker.mutated.contains(&key) {
            out.push((
                local.file,
                Diagnostic::warning(
                    "W0002",
                    local.span,
                    format!("`{}` eppudu maaradhu, `maare` avasaram ledhu", local.name),
                ),
            ));
        }
    }

    out.sort_by_key(|(file, d)| (*file, d.span.start));
    out
}

/// Walks function bodies to find mutated locals (W0002) and unreachable
/// statements (W0003).
struct Walker<'a> {
    file: FileId,
    res: &'a Resolution,
    mutated: HashSet<(FileId, usize)>,
    out: Vec<(FileId, Diagnostic)>,
}

impl Walker<'_> {
    fn module(&mut self, ast: &Module) {
        for item in &ast.items {
            self.item(item);
        }
    }

    fn item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(f) => self.block(&f.body),
            ItemKind::Methods(m) => {
                for member in &m.items {
                    self.item(member);
                }
            }
            ItemKind::Impl(i) => {
                for f in &i.items {
                    self.block(&f.body);
                }
            }
            ItemKind::Trait(t) => {
                for ti in &t.items {
                    if let Some(body) = &ti.default_body {
                        self.block(body);
                    }
                }
            }
            ItemKind::Const { value, .. } => self.expr(value),
            _ => {}
        }
    }

    /// The local a place expression is rooted in, by declaration offset.
    fn root_local(&self, e: &Expr) -> Option<usize> {
        match &e.kind {
            ExprKind::Path(p) => match self.res.resolutions.get(&(self.file, p.segments[0].span.start)) {
                Some(Res::Local(decl)) => Some(decl.start),
                _ => None,
            },
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => self.root_local(base),
            ExprKind::Unary { op: UnOp::Deref, operand } => self.root_local(operand),
            _ => None,
        }
    }

    fn mark_mutated(&mut self, e: &Expr) {
        if let Some(decl) = self.root_local(e) {
            self.mutated.insert((self.file, decl));
        }
    }

    /// Why a statement ends its block, if it does.
    fn diverges(stmt: &Stmt) -> Option<&'static str> {
        match &stmt.kind {
            StmtKind::Return(_) => Some("icchey"),
            StmtKind::Break => Some("aapey"),
            StmtKind::Continue => Some("vadhiley"),
            StmtKind::Expr(Expr { kind: ExprKind::Call { callee, .. }, .. }) => match &callee.kind {
                ExprKind::Path(p) if p.segments.len() == 1 && p.segments[0].name == "kompa_munigindhi" => {
                    Some("kompa_munigindhi()")
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn block(&mut self, block: &Block) {
        let mut ended_by: Option<&'static str> = None;
        let mut reported = false;
        for stmt in &block.stmts {
            // One warning per block, at the first statement that can't run.
            if let (Some(why), false) = (ended_by, reported) {
                self.out.push((
                    self.file,
                    Diagnostic::warning(
                        "W0003",
                        stmt.span,
                        format!("ee code eppudu run avvadhu: paina `{why}` undhi"),
                    ),
                ));
                reported = true;
            }
            self.stmt(stmt);
            if ended_by.is_none() {
                ended_by = Self::diverges(stmt);
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Binding { value, .. } => self.expr(value),
            StmtKind::Assign { place, value } | StmtKind::Compound { place, value, .. } => {
                self.mark_mutated(place);
                self.expr(place);
                self.expr(value);
            }
            StmtKind::Return(value) => {
                if let Some(e) = value {
                    self.expr(e);
                }
            }
            StmtKind::If { branches, else_block } => {
                for (cond, body) in branches {
                    self.expr(cond);
                    self.block(body);
                }
                if let Some(body) = else_block {
                    self.block(body);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::For { iter, body, .. } => {
                self.expr(iter);
                self.block(body);
            }
            StmtKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.block(&arm.body);
                }
            }
            StmtKind::Defer(body) | StmtKind::Unsafe(body) => self.block(body),
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call { callee, phrases, .. } => {
                // A method call may need a mutable receiver: `x.cherchu()`.
                if let ExprKind::Path(p) = &callee.kind {
                    if p.segments.len() > 1 {
                        self.mark_mutated(callee);
                    }
                } else if let ExprKind::Field { base, .. } = &callee.kind {
                    self.mark_mutated(base);
                }
                self.expr(callee);
                for phrase in phrases {
                    if let PhraseKind::Value(e) | PhraseKind::Spread(e) = &phrase.kind {
                        self.expr(e);
                    }
                }
            }
            ExprKind::Unary { op: UnOp::RefMut, operand } => {
                self.mark_mutated(operand);
                self.expr(operand);
            }
            ExprKind::Unary { operand, .. } | ExprKind::Try(operand) => self.expr(operand),
            ExprKind::FStr(parts) => {
                for part in parts {
                    if let FStrPart::Expr(e) = part {
                        self.expr(e);
                    }
                }
            }
            ExprKind::Pipe { input, stage } => {
                self.expr(input);
                self.expr(stage);
            }
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::Range { lo: lhs, hi: rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => self.expr(base),
            ExprKind::Index { base, indices } => {
                self.expr(base);
                for i in indices {
                    self.expr(i);
                }
            }
            ExprKind::Closure { body, .. } => self.expr(body),
            ExprKind::Ctor { args: items, .. } | ExprKind::Tuple(items) | ExprKind::List(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            ExprKind::Record { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            ExprKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.expr(k);
                    self.expr(v);
                }
            }
            _ => {}
        }
    }
}
