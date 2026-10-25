//! Checking function bodies (spec §10, §15.3 mutability, §16 constants).

use std::collections::HashMap;

use super::lower::Lower;
use super::results::{expr_id, AdtRef, FieldStep, PathRes, Results};
use super::solve::{self, Solver};
use super::{AdtKind, FnId, FnOrigin, Generic, Globals, RecvMode, TraitRef, TyEnv};
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::resolve::{DefKind, FileId, ModId, Res};
use crate::source::Span;
use crate::types::{self, Con, InferTable, IntTy, Ty, VarKind};

pub(super) struct Local {
    pub ty: Ty,
    pub mutable: bool,
    pub name: String,
    pub span: Span,
}

/// The three conversion functions of §18.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    Lossless,
    Checked,
    Lossy,
}

pub(super) struct BodyCx<'s, 'g> {
    pub g: &'s Globals<'g>,
    pub file: FileId,
    pub module: ModId,
    pub generics: Vec<Generic>,
    pub self_ty: Option<Ty>,
    pub ret: Ty,
    pub receiver: Option<(RecvMode, Ty)>,
    pub infer: InferTable,
    pub locals: HashMap<usize, Local>,
    pub diags: Vec<(FileId, Diagnostic)>,
    /// Trait bounds to check once inference is done: type, trait, span, code.
    pub obligations: Vec<(Ty, TraitRef, Span, &'static str)>,
    pub int_lits: Vec<(Ty, i128, Span)>,
    pub conversions: Vec<(Conv, Ty, Ty, Span)>,
    pub expr_types: HashMap<(usize, usize), Ty>,
    /// One flag per enclosing loop: has it seen an `aapey`?
    pub loops: Vec<bool>,
    pub unsafe_depth: u32,
    /// Decisions recorded for MIR lowering, resolved by `finish`.
    pub out: Results,
}

pub fn check_module(g: &Globals, file: FileId, ast: &Module) -> (Vec<(FileId, Diagnostic)>, Results) {
    let mut out = Vec::new();
    let mut results = Results::default();
    let mut take = |(d, r): (Vec<(FileId, Diagnostic)>, Results)| {
        out.extend(d);
        results.merge(r);
    };
    let module = g.module_of_file(file);
    for item in &ast.items {
        match &item.kind {
            ItemKind::Fn(f) => take(check_fn(g, file, module, &f.sig, &f.body, None)),
            ItemKind::Methods(m) => {
                for member in &m.items {
                    if let ItemKind::Fn(f) = &member.kind {
                        take(check_fn(g, file, module, &f.sig, &f.body, None));
                    }
                }
            }
            ItemKind::Impl(i) => {
                for f in &i.items {
                    take(check_fn(g, file, module, &f.sig, &f.body, None));
                }
            }
            ItemKind::Trait(t) => {
                let Some(&def) = g.res.modules[module].types.get(&t.name.name) else { continue };
                for ti in &t.items {
                    if let Some(body) = &ti.default_body {
                        take(check_fn(g, file, module, &ti.sig, body, Some(def)));
                    }
                }
            }
            ItemKind::Const { name, value, .. } => {
                let Some(def) = g.res.modules[module].values.get(&name.name) else { continue };
                let Some(ty) = g.consts.get(def) else { continue };
                let mut cx = BodyCx::new(g, file, module, Vec::new(), None, Ty::Unit);
                cx.check_coerce(value, ty);
                cx.check_const_expr(value);
                let r = cx.finish();
                take((cx.diags, r));
            }
            _ => {}
        }
    }
    (out, results)
}

fn check_fn(
    g: &Globals,
    file: FileId,
    module: ModId,
    sig_ast: &FnSig,
    body: &Block,
    trait_def: Option<usize>,
) -> (Vec<(FileId, Diagnostic)>, Results) {
    let Some(&fid) = g.fn_by_span.get(&(file, sig_ast.name.span.start)) else {
        return (Vec::new(), Results::default());
    };
    let sig = &g.fns[fid];
    let mut generics = sig.generics.clone();
    if let Some(def) = trait_def {
        // Inside a default body, `idhi` implements the trait itself.
        let n = g.traits.get(&def).map_or(1, |t| t.generics.len());
        generics[0].bounds.push(TraitRef { def, args: (1..n as u32).map(Ty::Param).collect() });
    }
    let mut cx = BodyCx::new(g, file, module, generics, sig.self_ty.clone(), sig.ret.clone());
    cx.receiver = sig.receiver.as_ref().map(|r| (r.mode, r.ty.clone()));
    let formals = &sig.params;
    for (p, f) in sig_ast.params.iter().filter(|p| p.kind != ParamKind::Receiver).zip(formals) {
        let ty = match p.kind {
            ParamKind::Variadic => Ty::reference(false, Ty::App(Con::Mukka, vec![f.ty.clone()])),
            _ => f.ty.clone(),
        };
        cx.declare(&p.name, ty, false);
        if let Some(d) = &p.default {
            cx.check_coerce(d, &f.ty);
            cx.check_const_expr(d);
        }
    }
    let diverges = cx.check_block(body);
    let ret = cx.infer.shallow(&cx.ret);
    let unit_like = match &ret {
        Ty::Unit | Ty::Error => true,
        Ty::App(Con::SareThappu, args) => args[0] == Ty::Unit,
        _ => false,
    };
    if !diverges && !unit_like {
        let shown = cx.show(&ret);
        cx.error(
            "E0410",
            sig_ast.name.span,
            format!(
                "`{}` `{shown}` return cheyyali, kaani konni dharullo `icchey` ledhu. Chivarlo `… icchey` raayandi",
                sig_ast.name.name
            ),
        );
    }
    let r = cx.finish();
    (cx.diags, r)
}

impl<'s, 'g> BodyCx<'s, 'g> {
    pub fn new(
        g: &'s Globals<'g>,
        file: FileId,
        module: ModId,
        generics: Vec<Generic>,
        self_ty: Option<Ty>,
        ret: Ty,
    ) -> Self {
        BodyCx {
            g,
            file,
            module,
            generics,
            self_ty,
            ret,
            receiver: None,
            infer: InferTable::default(),
            locals: HashMap::new(),
            diags: Vec::new(),
            obligations: Vec::new(),
            int_lits: Vec::new(),
            conversions: Vec::new(),
            expr_types: HashMap::new(),
            loops: Vec::new(),
            unsafe_depth: 0,
            out: Results::default(),
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    pub fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push((self.file, Diagnostic::error(code, span, message)));
    }

    pub fn res_at(&self, span: Span) -> Option<Res> {
        self.g.res.resolutions.get(&(self.file, span.start)).copied()
    }

    pub fn implements(&mut self, ty: &Ty, tr: &TraitRef) -> bool {
        let g = self.g;
        let solver = Solver { g, generics: &self.generics };
        solver.implements(&mut self.infer, ty, tr, 0)
    }

    pub fn is_copy(&mut self, ty: &Ty) -> bool {
        let g = self.g;
        let solver = Solver { g, generics: &self.generics };
        solver.is_copy(&mut self.infer, ty)
    }

    pub fn shallow(&mut self, ty: &Ty) -> Ty {
        self.infer.shallow(ty)
    }

    pub fn fresh(&mut self) -> Ty {
        self.infer.fresh(VarKind::General)
    }

    pub fn show(&mut self, ty: &Ty) -> String {
        let ty = self.infer.resolve(ty);
        let g = self.g;
        let generics = &self.generics;
        let infer = std::cell::RefCell::new(&mut self.infer);
        let adt_name = |d: usize| g.res.defs[d].name.clone();
        let param_name = |i: u32| generics.get(i as usize).map_or("_".to_string(), |g| g.name.clone());
        let var_kind = |v: u32| infer.borrow_mut().kind(v);
        let display =
            types::Display { ty: &ty, adt_name: &adt_name, param_name: &param_name, var_kind: &var_kind };
        display.to_string()
    }

    pub fn trait_name(&self, def: usize) -> String {
        self.g.res.defs[def].name.clone()
    }

    pub fn std_bound(&self, name: &str, args: Vec<Ty>) -> Option<TraitRef> {
        self.g.std_trait(name).map(|def| TraitRef { def, args })
    }

    pub fn oblige(&mut self, ty: &Ty, trait_name: &str, span: Span, code: &'static str) {
        if let Some(tr) = self.std_bound(trait_name, Vec::new()) {
            self.obligations.push((ty.clone(), tr, span, code));
        }
    }

    pub fn lower_ty(&mut self, t: &Type) -> Ty {
        let env = TyEnv {
            file: Some(self.file),
            std_module: self.module,
            generics: &self.generics,
            self_ty: self.self_ty.clone(),
        };
        Lower { g: self.g, diags: &mut self.diags }.lower_type(&env, t)
    }

    pub fn declare(&mut self, ident: &Ident, ty: Ty, mutable: bool) {
        self.locals
            .insert(ident.span.start, Local { ty, mutable, name: ident.name.clone(), span: ident.span });
    }

    /// Reports E0401 unless either side is already an error.
    pub fn mismatch(&mut self, span: Span, expected: &Ty, actual: &Ty) {
        let e = self.infer.resolve(expected);
        let a = self.infer.resolve(actual);
        if e.has_error() || a.has_error() {
            return;
        }
        if let (Ty::Int(x), Ty::Int(y)) = (&e, &a) {
            if x.signed != y.signed {
                let (es, as_) = (self.show(&e), self.show(&a));
                self.error(
                    "E0408",
                    span,
                    format!("`{es}` kavali kaani `{as_}` vachindhi. Signed, aasajeevi kalapakoodadhu; `maarchu` vaadandi"),
                );
                return;
            }
        }
        let (es, as_) = (self.show(&e), self.show(&a));
        self.error("E0401", span, format!("type match kaaledhu: `{es}` kavali, kaani `{as_}` vachindhi"));
    }

    /// Checks `e` against `expected`, applying coercions (§9.4.2).
    pub fn check_coerce(&mut self, e: &Expr, expected: &Ty) -> Ty {
        let actual = self.check_expr(e, Some(expected));
        if !solve::coerce(&mut self.infer, &actual, expected) {
            self.mismatch(e.span, expected, &actual);
        }
        expected.clone()
    }

    pub fn unify_or_report(&mut self, span: Span, expected: &Ty, actual: &Ty) -> bool {
        if solve::unify(&mut self.infer, expected, actual) {
            return true;
        }
        self.mismatch(span, expected, actual);
        false
    }

    /// `&ankam` and friends behave like the scalar in operators (§10.5.1).
    pub fn deref_scalar(&mut self, ty: Ty) -> Ty {
        if let Ty::Ref(_, inner) = self.shallow(&ty) {
            let inner = self.shallow(&inner);
            let scalar = match &inner {
                Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char => true,
                Ty::Var(v) => self.infer.kind(*v) != VarKind::General,
                _ => false,
            };
            if scalar {
                return inner;
            }
        }
        ty
    }

    fn int_like(&mut self, ty: &Ty) -> bool {
        match self.shallow(ty) {
            Ty::Int(_) | Ty::Error => true,
            Ty::Var(v) => self.infer.kind(v) == VarKind::Int,
            _ => false,
        }
    }

    fn require_int(&mut self, ty: &Ty, span: Span, code: &'static str) {
        let t = self.shallow(ty);
        if let Ty::Var(v) = t {
            if self.infer.kind(v) == VarKind::General {
                let int = self.infer.fresh(VarKind::Int);
                solve::unify(&mut self.infer, &t, &int);
                return;
            }
        }
        if !self.int_like(&t) {
            let shown = self.show(&t);
            self.error(code, span, format!("ikkada integer (`ankam…`) kavali, kaani `{shown}` undhi"));
        }
    }

    // ── statements ─────────────────────────────────────────────────────────

    /// Checks a block; returns whether it always leaves early (§10.3).
    pub fn check_block(&mut self, block: &Block) -> bool {
        let mut diverges = false;
        for stmt in &block.stmts {
            diverges |= self.check_stmt(stmt);
        }
        diverges
    }

    fn check_cond(&mut self, cond: &Expr) {
        let t = self.check_expr(cond, Some(&Ty::Bool));
        if !solve::unify(&mut self.infer, &t, &Ty::Bool) {
            let t = self.infer.resolve(&t);
            if !t.has_error() {
                let shown = self.show(&t);
                self.error(
                    "E0403",
                    cond.span,
                    format!("condition `ato_ito` (avunu/kaadhu) ayyi undali, kaani `{shown}` undhi"),
                );
            }
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Binding { mutable, target, ty, value } => {
                let value_ty = match ty {
                    Some(t) => {
                        let ann = self.lower_ty(t);
                        self.check_coerce(value, &ann)
                    }
                    None => self.check_expr(value, None),
                };
                match target {
                    BindTarget::Name(n) => self.declare(n, value_ty.clone(), *mutable),
                    BindTarget::Tuple(names) => {
                        let parts: Vec<Ty> = names.iter().map(|_| self.fresh()).collect();
                        let tuple = Ty::Tuple(parts.clone());
                        if !solve::unify(&mut self.infer, &tuple, &value_ty) {
                            let shown = self.show(&value_ty);
                            if !self.infer.resolve(&value_ty).has_error() {
                                self.error(
                                    "E0401",
                                    value.span,
                                    format!(
                                        "{} values unna `jatha` kavali, kaani `{shown}` vachindhi",
                                        names.len()
                                    ),
                                );
                            }
                        }
                        for (n, t) in names.iter().zip(parts) {
                            self.declare(n, t, *mutable);
                        }
                    }
                }
                self.shallow(&value_ty) == Ty::Never
            }
            StmtKind::Assign { place, value } => {
                let pt = self.check_expr(place, None);
                self.check_coerce(value, &pt);
                self.require_mutable(place, "E0602");
                false
            }
            StmtKind::Compound { place, op, value } => {
                let pt = self.check_expr(place, None);
                let pt = self.deref_scalar(pt);
                let vt = self.check_expr(value, Some(&pt));
                let result = self.arith(*op, pt.clone(), vt, place.span, value.span, stmt.span);
                self.unify_or_report(stmt.span, &pt, &result);
                self.require_mutable(place, "E0602");
                false
            }
            StmtKind::Return(value) => {
                let ret = self.ret.clone();
                match value {
                    Some(e) => {
                        self.check_coerce(e, &ret);
                    }
                    None => {
                        let r = self.infer.resolve(&ret);
                        if r != Ty::Unit && !r.has_error() {
                            let shown = self.show(&r);
                            self.error(
                                "E0401",
                                stmt.span,
                                format!("ee function `{shown}` return cheyyali. `value icchey` ani raayandi"),
                            );
                        }
                    }
                }
                true
            }
            StmtKind::If { branches, else_block } => {
                let mut all = else_block.is_some();
                for (cond, body) in branches {
                    self.check_cond(cond);
                    all &= self.check_block(body);
                }
                if let Some(body) = else_block {
                    all &= self.check_block(body);
                }
                all
            }
            StmtKind::While { cond, body } => {
                self.check_cond(cond);
                self.loops.push(false);
                self.check_block(body);
                let broke = self.loops.pop().unwrap_or(true);
                matches!(cond.kind, ExprKind::Bool(true)) && !broke
            }
            StmtKind::For { iter, pattern, body } => {
                let it = self.check_expr(iter, None);
                let item = self.iter_item(&it, is_place(iter), iter.span);
                self.out.for_items.insert(expr_id(self.file, iter.span), item.clone());
                self.check_pattern(pattern, &item, true);
                self.loops.push(false);
                self.check_block(body);
                self.loops.pop();
                false
            }
            StmtKind::Match { scrutinee, arms } => {
                let st = self.check_expr(scrutinee, None);
                let mut all = !arms.is_empty();
                for arm in arms {
                    self.check_pattern(&arm.pattern, &st, false);
                    all &= self.check_block(&arm.body);
                }
                self.check_exhaustive(&st, arms, scrutinee.span);
                all
            }
            StmtKind::Break => {
                if let Some(flag) = self.loops.last_mut() {
                    *flag = true;
                }
                true
            }
            StmtKind::Continue => true,
            StmtKind::Defer(body) => {
                self.check_block(body);
                false
            }
            StmtKind::Unsafe(body) => {
                self.unsafe_depth += 1;
                let d = self.check_block(body);
                self.unsafe_depth -= 1;
                d
            }
            StmtKind::Expr(e) => {
                let t = self.check_expr(e, None);
                self.shallow(&t) == Ty::Never
            }
        }
    }

    // ── places and mutability (§15.2–15.3) ─────────────────────────────────

    pub fn naa_ty(&self) -> Ty {
        match &self.receiver {
            Some((RecvMode::Ref, t)) => Ty::reference(false, t.clone()),
            Some((RecvMode::RefMut, t)) => Ty::reference(true, t.clone()),
            Some((RecvMode::Value, t)) => t.clone(),
            None => Ty::Error,
        }
    }

    /// Reports `code` (E0602 or E0607) unless `e` is a place that may change.
    pub fn require_mutable(&mut self, e: &Expr, code: &'static str) {
        if let Err((name, span)) = self.place_mutable(e) {
            let msg = if code == "E0607" {
                format!(
                    "`{name}` maare kaadhu, `&maare` ivvalem. `maare {name} ante …` ani declare cheyyandi"
                )
            } else {
                format!("`{name}` maare kaadhu, maarchalevu.")
            };
            self.error(code, span, msg);
        }
    }

    fn place_mutable(&mut self, e: &Expr) -> Result<(), (String, Span)> {
        match &e.kind {
            ExprKind::Path(p) => {
                let first = &p.segments[0];
                let (mut ty, mut ok, rest) = match self.res_at(first.span) {
                    Some(Res::Local(decl)) => match self.locals.get(&decl.start) {
                        Some(l) => (l.ty.clone(), l.mutable, &p.segments[1..]),
                        None => return Ok(()),
                    },
                    Some(Res::SelfMember) if first.name == "naa" => (self.naa_ty(), false, &p.segments[1..]),
                    Some(Res::SelfMember) => (self.naa_ty(), false, &p.segments[..]),
                    _ => return Ok(()),
                };
                for seg in rest {
                    loop {
                        match self.shallow(&ty) {
                            Ty::Ref(m, inner) => {
                                ok = m;
                                ty = *inner;
                            }
                            Ty::App(Con::Pette, args) => ty = args[0].clone(),
                            _ => break,
                        }
                    }
                    ty = self.field_quiet(&ty, &seg.name);
                }
                if ok {
                    Ok(())
                } else {
                    Err((first.name.clone(), e.span))
                }
            }
            ExprKind::Field { base, .. }
            | ExprKind::TupleIndex { base, .. }
            | ExprKind::Index { base, .. } => {
                let bt = self.expr_types.get(&(base.span.start, base.span.end)).cloned().unwrap_or(Ty::Error);
                let mut t = bt;
                let mut through = None;
                loop {
                    match self.shallow(&t) {
                        Ty::Ref(m, inner) => {
                            through = Some(m);
                            t = *inner;
                        }
                        Ty::App(Con::Pette, args) => t = args[0].clone(),
                        _ => break,
                    }
                }
                match through {
                    Some(true) => Ok(()),
                    Some(false) => Err((describe(base), e.span)),
                    None => self.place_mutable(base),
                }
            }
            ExprKind::Unary { op: UnOp::Deref, operand } => {
                let t = self
                    .expr_types
                    .get(&(operand.span.start, operand.span.end))
                    .cloned()
                    .unwrap_or(Ty::Error);
                match self.shallow(&t) {
                    Ty::Ref(true, _) | Ty::Ptr(true, _) | Ty::Error => Ok(()),
                    Ty::App(Con::Pette, _) => self.place_mutable(operand),
                    _ => Err((describe(operand), e.span)),
                }
            }
            _ => Ok(()),
        }
    }

    fn field_quiet(&mut self, ty: &Ty, name: &str) -> Ty {
        let t = self.shallow(ty);
        if let Ty::App(Con::Adt(d), args) = &t {
            if let Some(AdtKind::Record(fields)) = self.g.adts.get(d).map(|a| &a.kind) {
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    return f.ty.subst(args);
                }
            }
        }
        Ty::Error
    }

    // ── expressions ────────────────────────────────────────────────────────

    pub fn check_expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        let ty = self.check_expr_inner(e, expected);
        self.expr_types.insert((e.span.start, e.span.end), ty.clone());
        ty
    }

    fn check_expr_inner(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        let expected = expected.map(|t| self.shallow(t));
        match &e.kind {
            ExprKind::Int(lit) => self.int_literal(lit, false, expected.as_ref(), e.span),
            ExprKind::Unary { op: UnOp::Neg, operand } if matches!(operand.kind, ExprKind::Int(_)) => {
                let ExprKind::Int(lit) = &operand.kind else { unreachable!() };
                let t = self.int_literal(lit, true, expected.as_ref(), e.span);
                if let Ty::Int(IntTy { signed: false, .. }) = self.shallow(&t) {
                    self.error("E0402", e.span, "aasajeevi ankam eppudu negative avvadhu 😄");
                }
                t
            }
            ExprKind::Float(_) => match expected {
                Some(Ty::Float(b)) => Ty::Float(b),
                _ => self.infer.fresh(VarKind::Float),
            },
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Unit => Ty::Unit,
            ExprKind::FStr(parts) => {
                for part in parts {
                    if let FStrPart::Expr(x) = part {
                        let t = self.check_expr(x, None);
                        self.oblige(&t, "Choopinchochu", x.span, "E0801");
                    }
                }
                Ty::Str
            }
            ExprKind::Ledhu => {
                let inner = match &expected {
                    Some(Ty::App(Con::Undochu, args)) => args[0].clone(),
                    _ => self.fresh(),
                };
                Ty::App(Con::Undochu, vec![inner])
            }
            ExprKind::BareSare => {
                let err = match &expected {
                    Some(Ty::App(Con::SareThappu, args)) => args[1].clone(),
                    _ => self.fresh(),
                };
                Ty::App(Con::SareThappu, vec![Ty::Unit, err])
            }
            ExprKind::Hole => Ty::Error,
            ExprKind::Path(path) => self.check_path(path),
            ExprKind::Call { callee, type_args, phrases } => {
                self.check_call(e.span, callee, type_args, phrases, None, expected.as_ref())
            }
            ExprKind::Try(inner) => {
                let t = self.check_expr(inner, None);
                self.try_result(t, e.span)
            }
            ExprKind::Pipe { input, stage } => {
                let it = self.check_expr(input, None);
                let injected = super::call::Injected { ty: it, span: input.span, place: is_place(input) };
                match &stage.kind {
                    ExprKind::Call { callee, type_args, phrases } => {
                        let t = self.check_call(
                            stage.span,
                            callee,
                            type_args,
                            phrases,
                            Some(injected),
                            expected.as_ref(),
                        );
                        self.expr_types.insert((stage.span.start, stage.span.end), t.clone());
                        t
                    }
                    ExprKind::Try(inner) => match &inner.kind {
                        ExprKind::Call { callee, type_args, phrases } => {
                            let t =
                                self.check_call(inner.span, callee, type_args, phrases, Some(injected), None);
                            self.try_result(t, stage.span)
                        }
                        _ => Ty::Error,
                    },
                    _ => Ty::Error,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.check_binary(*op, lhs, rhs, e.span, expected.as_ref()),
            ExprKind::Unary { op, operand } => self.check_unary(*op, operand, e.span, expected.as_ref()),
            ExprKind::Field { base, name } => {
                let bt = self.check_expr(base, None);
                let (t, step) = self.field_step(&bt, name);
                if let Some(step) = step {
                    self.out.fields.insert(expr_id(self.file, e.span), step);
                }
                t
            }
            ExprKind::TupleIndex { base, index } => {
                let bt = self.check_expr(base, None);
                let (t, derefs) = self.autoderef_n(&bt);
                self.out.fields.insert(expr_id(self.file, e.span), FieldStep { derefs, index: *index });
                match self.shallow(&t) {
                    Ty::Tuple(items) if (*index as usize) < items.len() => items[*index as usize].clone(),
                    Ty::Error => Ty::Error,
                    Ty::Var(_) => self.need_annotation(base.span),
                    other => {
                        let shown = self.show(&other);
                        self.error("E0405", e.span, format!("`{shown}` lo `.{index}` ledhu"));
                        Ty::Error
                    }
                }
            }
            ExprKind::Index { base, indices } => self.check_index(base, indices, e.span),
            ExprKind::Range { lo, hi, .. } => {
                let hint = match &expected {
                    Some(Ty::App(Con::Paridhi, args)) => Some(args[0].clone()),
                    _ => None,
                };
                let lt = self.check_expr(lo, hint.as_ref());
                let lt = self.deref_scalar(lt);
                let ht = self.check_expr(hi, Some(&lt));
                let ht = self.deref_scalar(ht);
                self.unify_or_report(hi.span, &lt, &ht);
                self.require_int(&lt, lo.span, "E0401");
                Ty::App(Con::Paridhi, vec![lt])
            }
            ExprKind::Closure { params, body, .. } => self.check_closure(params, body, expected.as_ref()),
            ExprKind::Ctor { path, args } => self.check_ctor(path, args, e.span, expected.as_ref()),
            ExprKind::Record { path, fields } => self.check_record(path, fields, e.span, expected.as_ref()),
            ExprKind::Tuple(items) => {
                let hints: Vec<Option<Ty>> = match &expected {
                    Some(Ty::Tuple(ts)) if ts.len() == items.len() => ts.iter().cloned().map(Some).collect(),
                    _ => vec![None; items.len()],
                };
                Ty::Tuple(
                    items
                        .iter()
                        .zip(hints)
                        .map(|(x, h)| match h {
                            Some(h) => self.check_coerce(x, &h),
                            None => self.check_expr(x, None),
                        })
                        .collect(),
                )
            }
            ExprKind::List(items) => match &expected {
                Some(Ty::Array(elem, n)) => {
                    let elem = (**elem).clone();
                    for x in items {
                        self.check_coerce(x, &elem);
                    }
                    if let Some(n) = n {
                        if *n != items.len() as u64 {
                            self.error(
                                "E0401",
                                e.span,
                                format!("{n} elements kavali, kaani {} unnayi", items.len()),
                            );
                        }
                    }
                    Ty::Array(Box::new(elem), Some(items.len() as u64))
                }
                _ => {
                    let elem = match &expected {
                        Some(Ty::App(Con::Varusa, args)) => args[0].clone(),
                        _ => self.fresh(),
                    };
                    for x in items {
                        self.check_coerce(x, &elem);
                    }
                    Ty::App(Con::Varusa, vec![elem])
                }
            },
            ExprKind::Map(pairs) => {
                let (k, v) = match &expected {
                    Some(Ty::App(Con::Jodi, args)) => (args[0].clone(), args[1].clone()),
                    _ => (self.fresh(), self.fresh()),
                };
                let mut seen: Vec<String> = Vec::new();
                for (key, value) in pairs {
                    self.check_coerce(key, &k);
                    self.check_coerce(value, &v);
                    if let Some(text) = const_key(key) {
                        if seen.contains(&text) {
                            self.error("E0419", key.span, format!("`{text}` key rendu saarlu undhi"));
                        } else {
                            seen.push(text);
                        }
                    }
                }
                self.oblige(&k, "Polchochu", e.span, "E0801");
                self.oblige(&k, "HashCheyochu", e.span, "E0801");
                Ty::App(Con::Jodi, vec![k, v])
            }
        }
    }

    pub fn need_annotation(&mut self, span: Span) -> Ty {
        self.error(
            "E0404",
            span,
            "ee value type inka teliyadhu. Type cheppu: `x: varusa[ankam] ante …` laaga",
        );
        Ty::Error
    }

    fn int_literal(
        &mut self,
        lit: &crate::token::IntLit,
        negative: bool,
        expected: Option<&Ty>,
        span: Span,
    ) -> Ty {
        let value = i128::from_str_radix(&lit.digits, lit.radix).ok();
        let ty = match expected {
            Some(Ty::Int(it)) => Ty::Int(*it),
            Some(t @ Ty::Var(v)) if self.infer.kind(*v) != VarKind::Float => {
                let int = self.infer.fresh(VarKind::Int);
                solve::unify(&mut self.infer, &int, t);
                int
            }
            _ => self.infer.fresh(VarKind::Int),
        };
        match value {
            Some(v) => self.int_lits.push((ty.clone(), if negative { -v } else { v }, span)),
            None => self.error("E0402", span, "ee number chaala pedhdhadhi, ae integer type lonu pattadhu"),
        }
        ty
    }

    fn try_result(&mut self, t: Ty, span: Span) -> Ty {
        match self.shallow(&t) {
            Ty::App(Con::SareThappu, args) => {
                let (ok, err) = (args[0].clone(), args[1].clone());
                let ret = self.shallow(&self.ret.clone());
                match ret {
                    Ty::App(Con::SareThappu, rargs) => {
                        let snap = self.infer.snapshot();
                        if solve::unify(&mut self.infer, &rargs[1], &err) {
                            self.infer.commit(snap);
                        } else {
                            self.infer.rollback_to(snap);
                            let tr = self.std_bound("NunchiRavochu", vec![err.clone()]);
                            let converts = tr.is_some_and(|tr| self.implements(&rargs[1], &tr));
                            if !converts {
                                let (f, e) = (self.show(&rargs[1]), self.show(&err));
                                self.error(
                                    "E0412",
                                    span,
                                    format!("`?` error `{e}` ni `{f}` ga maarchalem. `{f} ki NunchiRavochu[{e}] gunam undhi` raayandi"),
                                );
                            }
                        }
                    }
                    Ty::Error => {}
                    _ => {
                        self.error("E0411", span, "`?` vaadalante function `sare_thappu[…]` return cheyyali")
                    }
                }
                ok
            }
            Ty::Error => Ty::Error,
            Ty::Var(_) => self.need_annotation(span),
            other => {
                let shown = self.show(&other);
                self.error("E0401", span, format!("`?` `sare_thappu` ki matrame, idhi `{shown}`"));
                Ty::Error
            }
        }
    }

    // ── paths ──────────────────────────────────────────────────────────────

    fn check_path(&mut self, path: &Path) -> Ty {
        let (ty, res) = self.path_value(path);
        self.out.paths.insert(expr_id(self.file, path.span), res);
        ty
    }

    fn path_value(&mut self, path: &Path) -> (Ty, PathRes) {
        let first = &path.segments[0];
        let (ty, rest, local) = match self.res_at(first.span) {
            Some(Res::Local(decl)) => match self.locals.get(&decl.start) {
                Some(l) => (l.ty.clone(), &path.segments[1..], Some(decl.start)),
                None => return (Ty::Error, PathRes::Opaque),
            },
            Some(Res::SelfMember) if first.name == "naa" => (self.naa_ty(), &path.segments[1..], None),
            Some(Res::SelfMember) => (self.naa_ty(), &path.segments[..], None),
            Some(Res::Def(d)) => return self.def_path(d, path, 1),
            Some(Res::Builtin) => {
                if path.segments.len() > 1 {
                    let head = builtin_head(&first.name);
                    return self.assoc_value(head, &path.segments[1]);
                }
                self.error("E0401", first.span, format!("`{}` oka type, value kaadhu", first.name));
                return (Ty::Error, PathRes::Opaque);
            }
            None => return (Ty::Error, PathRes::Opaque),
        };
        let (ty, steps) = self.fields(ty, rest);
        let res = match local {
            Some(decl) => PathRes::Local { decl, steps },
            None => PathRes::Naa { steps },
        };
        (ty, res)
    }

    fn fields(&mut self, mut ty: Ty, segs: &[Ident]) -> (Ty, Vec<FieldStep>) {
        let mut steps = Vec::new();
        for seg in segs {
            let (t, step) = self.field_step(&ty, seg);
            ty = t;
            steps.push(step.unwrap_or(FieldStep { derefs: 0, index: 0 }));
        }
        (ty, steps)
    }

    /// The value a path starting at definition `d` names (segment `idx` onwards).
    fn def_path(&mut self, d: usize, path: &Path, idx: usize) -> (Ty, PathRes) {
        let g = self.g;
        let def = &g.res.defs[d];
        match def.kind {
            DefKind::Module(m) => {
                if g.res.modules[m].open {
                    return (Ty::Error, PathRes::Opaque);
                }
                match path.segments.get(idx).and_then(|s| self.res_at(s.span)) {
                    Some(Res::Def(member)) => self.def_path(member, path, idx + 1),
                    _ => (Ty::Error, PathRes::Opaque),
                }
            }
            DefKind::Fn | DefKind::StdValue | DefKind::Const => {
                if let Some(&fid) = g.fn_of_def.get(&d) {
                    let (ty, tys) = self.fn_value(fid);
                    return (ty, PathRes::Fn { fid, tys });
                }
                let ty = g.consts.get(&d).cloned().unwrap_or(Ty::Error);
                let (ty, steps) = self.fields(ty, &path.segments[idx..]);
                (ty, PathRes::Const { def: d, steps })
            }
            DefKind::Enum | DefKind::Record | DefKind::StdType => {
                let Some(seg) = path.segments.get(idx) else {
                    self.error("E0401", path.span, format!("`{}` oka type, value kaadhu", def.name));
                    return (Ty::Error, PathRes::Opaque);
                };
                if let Some(super::Adt { kind: AdtKind::Enum(variants), generics, .. }) = g.adts.get(&d) {
                    if let Some(vi) = variants.iter().position(|v| v.name == seg.name) {
                        let v = &variants[vi];
                        if !v.fields.is_empty() {
                            self.error(
                                "E0704",
                                seg.span,
                                format!(
                                    "`{}.{}` ki {} value(s) kavali: `{}.{}(…)`",
                                    def.name,
                                    v.name,
                                    v.fields.len(),
                                    def.name,
                                    v.name
                                ),
                            );
                        }
                        let args = generics.iter().map(|_| self.fresh()).collect();
                        let ty = Ty::App(Con::Adt(d), args);
                        return (ty, PathRes::Variant { adt: d, variant: vi as u32 });
                    }
                }
                let head = match g.std_cons.get(&d) {
                    Some(c) => super::Head::Con(*c),
                    None => super::Head::Con(Con::Adt(d)),
                };
                self.assoc_value(Some(head), seg)
            }
            _ => (Ty::Error, PathRes::Opaque),
        }
    }

    /// `Type.f` used as a value.
    fn assoc_value(&mut self, head: Option<super::Head>, seg: &Ident) -> (Ty, PathRes) {
        let found = head.and_then(|h| self.g.inherent.get(&h)).and_then(|m| m.get(&seg.name)).copied();
        match found {
            Some(fid) => {
                let (ty, tys) = self.fn_value(fid);
                (ty, PathRes::Fn { fid, tys })
            }
            None => {
                self.error("E0406", seg.span, format!("`{}` ane function ee type lo ledhu", seg.name));
                (Ty::Error, PathRes::Opaque)
            }
        }
    }

    /// A named function used as a value: `pani[(…) -> R]` (§10.7.3).
    fn fn_value(&mut self, fid: FnId) -> (Ty, Vec<Ty>) {
        let sig = &self.g.fns[fid];
        let fresh: Vec<Ty> = sig.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect();
        let sig = &self.g.fns[fid];
        let mut params: Vec<(Ty, Option<Role>)> = Vec::new();
        if let Some(r) = &sig.receiver {
            let t = r.ty.subst(&fresh);
            params.push((
                match r.mode {
                    RecvMode::Value => t,
                    RecvMode::Ref => Ty::reference(false, t),
                    RecvMode::RefMut => Ty::reference(true, t),
                },
                None,
            ));
        }
        params.extend(sig.params.iter().map(|p| (p.ty.subst(&fresh), p.role)));
        (Ty::Fn(params, Box::new(sig.ret.subst(&fresh))), fresh)
    }

    pub fn autoderef(&mut self, ty: &Ty) -> Ty {
        self.autoderef_n(ty).0
    }

    /// Dereferences `&` and `pette` until neither is left; returns the count.
    pub fn autoderef_n(&mut self, ty: &Ty) -> (Ty, u8) {
        let mut t = self.shallow(ty);
        let mut n = 0u8;
        loop {
            match t {
                Ty::Ref(_, inner) => t = self.shallow(&inner),
                Ty::App(Con::Pette, args) => t = self.shallow(&args[0]),
                other => return (other, n),
            }
            n = n.saturating_add(1);
        }
    }

    /// `e.f` with auto-dereference (§10.6).
    pub fn field_step(&mut self, base: &Ty, name: &Ident) -> (Ty, Option<FieldStep>) {
        let (t, derefs) = self.autoderef_n(base);
        match &t {
            Ty::Error => return (Ty::Error, None),
            Ty::Var(_) => return (self.need_annotation(name.span), None),
            Ty::App(Con::Adt(d), args) => {
                if let Some(adt) = self.g.adts.get(d) {
                    if let AdtKind::Record(fields) = &adt.kind {
                        if let Some(index) = fields.iter().position(|f| f.name == name.name) {
                            let f = &fields[index];
                            if !f.public && adt.module != self.module {
                                self.error(
                                    "E0303",
                                    name.span,
                                    format!(
                                        "`{}` field `{}` lo private. `andhariki` pettandi",
                                        f.name, adt.name
                                    ),
                                );
                            }
                            return (f.ty.subst(args), Some(FieldStep { derefs, index: index as u32 }));
                        }
                    }
                }
            }
            _ => {}
        }
        let shown = self.show(&t);
        self.error("E0405", name.span, format!("`{shown}` lo `{}` ane field ledhu", name.name));
        (Ty::Error, None)
    }

    // ── operators (§10.5) ──────────────────────────────────────────────────

    fn check_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span, expected: Option<&Ty>) -> Ty {
        use BinOp::*;
        match op {
            And | Or => {
                self.check_cond(lhs);
                self.check_cond(rhs);
                Ty::Bool
            }
            Shl | Shr => {
                let l = self.check_expr(lhs, expected);
                let l = self.deref_scalar(l);
                let r = self.check_expr(rhs, None);
                let r = self.deref_scalar(r);
                self.require_int(&l, lhs.span, "E0401");
                self.require_int(&r, rhs.span, "E0401");
                l
            }
            Eq | Ne | Lt | Le | Gt | Ge => {
                let l = self.check_expr(lhs, None);
                let l = self.deref_scalar(l);
                let r = self.check_expr(rhs, Some(&l));
                let r = self.deref_scalar(r);
                if !solve::unify(&mut self.infer, &l, &r) {
                    self.mismatch(rhs.span, &l, &r);
                    return Ty::Bool;
                }
                let lt = self.shallow(&l);
                let builtin = match &lt {
                    Ty::Int(_) | Ty::Float(_) | Ty::Char | Ty::Error => true,
                    Ty::Bool | Ty::Unit => matches!(op, Eq | Ne),
                    Ty::Var(v) => self.infer.kind(*v) != VarKind::General,
                    _ => false,
                };
                if !builtin {
                    let tr = if matches!(op, Eq | Ne) { "Polchochu" } else { "VarusaPettochu" };
                    self.oblige(&l, tr, span, "E0407");
                }
                Ty::Bool
            }
            _ => {
                let l = self.check_expr(lhs, expected);
                let l = self.deref_scalar(l);
                let r = self.check_expr(rhs, Some(&l));
                let r = self.deref_scalar(r);
                self.arith(op, l, r, lhs.span, rhs.span, span)
            }
        }
    }

    /// `+ - * / % @ & | ^` on two operand types; returns the result type.
    fn arith(&mut self, op: BinOp, l: Ty, r: Ty, _lspan: Span, rspan: Span, span: Span) -> Ty {
        use BinOp::*;
        if !solve::unify(&mut self.infer, &l, &r) {
            self.mismatch(rspan, &l, &r);
            return l;
        }
        let lt = self.shallow(&l);
        let numeric = match &lt {
            Ty::Int(_) | Ty::Float(_) | Ty::Error => true,
            Ty::Var(v) => self.infer.kind(*v) != VarKind::General,
            _ => false,
        };
        match op {
            BitAnd | BitOr | BitXor => {
                self.require_int(&l, span, "E0407");
            }
            MatMul => self.oblige(&l, "MatmulCheyochu", span, "E0407"),
            _ if numeric => {}
            _ => {
                let tr = match op {
                    Add => "Kalupochu",
                    Sub => "Theeseyochu",
                    Mul => "Guninchochu",
                    Div => "Panchochu",
                    _ => "Sheshamochu",
                };
                self.oblige(&l, tr, span, "E0407");
            }
        }
        l
    }

    fn check_unary(&mut self, op: UnOp, operand: &Expr, span: Span, expected: Option<&Ty>) -> Ty {
        match op {
            UnOp::Neg => {
                let t = self.check_expr(operand, expected);
                let t = self.deref_scalar(t);
                match self.shallow(&t) {
                    Ty::Int(IntTy { signed: false, .. }) => {
                        self.error("E0407", span, "aasajeevi ankam ki minus ledhu, adhi eppudu positive 😄");
                    }
                    Ty::Int(_) | Ty::Float(_) | Ty::Error => {}
                    Ty::Var(v) if self.infer.kind(v) != VarKind::General => {}
                    _ => self.oblige(&t, "Thippochu", span, "E0407"),
                }
                t
            }
            UnOp::Not => {
                self.check_cond(operand);
                Ty::Bool
            }
            UnOp::BitNot => {
                let t = self.check_expr(operand, expected);
                let t = self.deref_scalar(t);
                self.require_int(&t, span, "E0407");
                t
            }
            UnOp::Deref => {
                let t = self.check_expr(operand, None);
                match self.shallow(&t) {
                    Ty::Ref(_, inner) => *inner,
                    Ty::App(Con::Pette, args) => args[0].clone(),
                    Ty::Ptr(_, inner) => {
                        self.require_unsafe(span, "raw pointer ni `*` tho chadavadam");
                        *inner
                    }
                    Ty::Error => Ty::Error,
                    Ty::Var(_) => self.need_annotation(operand.span),
                    other => {
                        let shown = self.show(&other);
                        self.error(
                            "E0401",
                            span,
                            format!("`*` reference ki matrame, `{shown}` reference kaadhu"),
                        );
                        Ty::Error
                    }
                }
            }
            UnOp::Ref | UnOp::RefMut => {
                let mutable = op == UnOp::RefMut;
                let hint = match expected {
                    Some(Ty::Ref(_, inner)) => Some((**inner).clone()),
                    _ => None,
                };
                let t = self.check_expr(operand, hint.as_ref());
                if mutable {
                    self.require_mutable(operand, "E0607");
                }
                Ty::reference(mutable, t)
            }
        }
    }

    pub fn require_unsafe(&mut self, span: Span, what: &str) {
        if self.unsafe_depth == 0 {
            self.error(
                "E1001",
                span,
                format!("{what} `naa_risk:` block lopala matrame. Mee risk, mee bhaadhyatha 😅"),
            );
        }
    }

    fn check_index(&mut self, base: &Expr, indices: &[Expr], span: Span) -> Ty {
        let bt = self.check_expr(base, None);
        let (t, derefs) = self.autoderef_n(&bt);
        self.out.index_derefs.insert(expr_id(self.file, span), derefs);
        if indices.len() != 1 || matches!(indices[0].kind, ExprKind::Hole) {
            for i in indices {
                if !matches!(i.kind, ExprKind::Hole) {
                    self.check_expr(i, None);
                }
            }
            if !t.is_error() {
                self.error("E0414", span, "okka index matrame ivvandi: `xs[i]`");
            }
            return Ty::Error;
        }
        let elem = match &t {
            Ty::Array(e, _) => (**e).clone(),
            Ty::App(Con::Varusa | Con::Mukka, args) => args[0].clone(),
            Ty::Str => {
                self.error(
                    "E0413",
                    span,
                    "`padham` ni index cheyyalem (UTF-8 kada). `s.aksharaalu()` leka `a ni, b tho s.byte_bhaagam()` vaadandi",
                );
                Ty::Error
            }
            Ty::Error => Ty::Error,
            Ty::Var(_) => self.need_annotation(base.span),
            other => {
                let shown = self.show(other);
                self.error("E0401", span, format!("`{shown}` ni `[i]` tho index cheyyalem"));
                Ty::Error
            }
        };
        let idx = &indices[0];
        if let ExprKind::Range { .. } = idx.kind {
            self.check_expr(idx, None);
            return Ty::App(Con::Mukka, vec![elem]);
        }
        let it = self.check_expr(idx, None);
        let it = self.deref_scalar(it);
        if !self.int_like(&it) {
            if let Ty::Var(v) = self.shallow(&it) {
                if self.infer.kind(v) == VarKind::General {
                    return elem;
                }
            }
            let shown = self.show(&it);
            self.error("E0414", idx.span, format!("index integer ayyi undali, kaani `{shown}` undhi"));
        }
        elem
    }

    fn check_closure(&mut self, params: &[ClosureParam], body: &Expr, expected: Option<&Ty>) -> Ty {
        let exp = match expected {
            Some(Ty::Fn(ps, r)) if ps.len() == params.len() => Some((ps.clone(), (**r).clone())),
            _ => None,
        };
        let mut tys = Vec::new();
        for (i, p) in params.iter().enumerate() {
            let ty = match (&p.ty, &exp) {
                (Some(t), _) => {
                    let t = self.lower_ty(t);
                    if let Some((ps, _)) = &exp {
                        self.unify_or_report(p.name.span, &ps[i].0, &t);
                    }
                    t
                }
                (None, Some((ps, _))) => ps[i].0.clone(),
                (None, None) => self.fresh(),
            };
            let role = p.role.or_else(|| exp.as_ref().and_then(|(ps, _)| ps[i].1));
            self.declare(&p.name, ty.clone(), false);
            tys.push((ty, role));
        }
        let ret = match &exp {
            Some((_, r)) => self.check_coerce(body, r),
            None => self.check_expr(body, None),
        };
        Ty::Fn(tys, Box::new(ret))
    }

    fn check_ctor(&mut self, path: &Path, args: &[Expr], span: Span, expected: Option<&Ty>) -> Ty {
        let name = path.segments[0].name.as_str();
        if path.segments.len() == 1 && matches!(name, "undhi" | "sare" | "thappu") {
            if args.len() != 1 {
                self.error("E0704", span, format!("`{name}` ki okka value matrame kavali"));
            }
            let (con, slot) = match name {
                "undhi" => (Con::Undochu, 0),
                "sare" => (Con::SareThappu, 0),
                _ => (Con::SareThappu, 1),
            };
            let mut parts: Vec<Ty> = match expected {
                Some(Ty::App(c, a)) if *c == con => a.clone(),
                _ => (0..if con == Con::Undochu { 1 } else { 2 }).map(|_| self.fresh()).collect(),
            };
            if let Some(a) = args.first() {
                let want = parts[slot].clone();
                parts[slot] = self.check_coerce(a, &want);
            }
            for a in args.iter().skip(1) {
                self.check_expr(a, None);
            }
            let adt = if con == Con::Undochu { AdtRef::Undochu } else { AdtRef::SareThappu };
            self.out.ctors.insert(expr_id(self.file, span), (adt, slot as u32));
            return Ty::App(con, parts);
        }
        let Some((d, variant)) = self.variant_of(path) else {
            for a in args {
                self.check_expr(a, None);
            }
            return Ty::Error;
        };
        let g = self.g;
        let adt = &g.adts[&d];
        let AdtKind::Enum(variants) = &adt.kind else { return Ty::Error };
        let v = &variants[variant];
        let fresh: Vec<Ty> = match expected {
            Some(Ty::App(Con::Adt(e), a)) if *e == d => a.clone(),
            _ => adt.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect(),
        };
        if v.fields.len() != args.len() {
            self.error(
                "E0704",
                span,
                format!(
                    "`{}.{}` ki {} value(s) kavali, {} ichharu",
                    adt.name,
                    v.name,
                    v.fields.len(),
                    args.len()
                ),
            );
        }
        for (a, f) in args.iter().zip(&v.fields) {
            let want = f.subst(&fresh);
            self.check_coerce(a, &want);
        }
        for a in args.iter().skip(v.fields.len()) {
            self.check_expr(a, None);
        }
        self.out.ctors.insert(expr_id(self.file, span), (AdtRef::Adt(d), variant as u32));
        Ty::App(Con::Adt(d), fresh)
    }

    /// The enum and variant index a path such as `Rangu.Erupu` names.
    pub fn variant_of(&mut self, path: &Path) -> Option<(usize, usize)> {
        let lower = Lower { g: self.g, diags: &mut self.diags };
        let (d, used) = lower.path_def(self.file, path)?;
        if self.g.res.defs[d].kind == DefKind::Module(0) {
            return None;
        }
        let seg = path.segments.get(used)?;
        match &self.g.adts.get(&d)?.kind {
            AdtKind::Enum(vs) => vs.iter().position(|v| v.name == seg.name).map(|i| (d, i)),
            AdtKind::Record(_) => {
                self.error("E0401", path.span, format!("`{}` enum kaadhu", self.g.res.defs[d].name));
                None
            }
        }
    }

    fn check_record(
        &mut self,
        path: &Path,
        fields: &[(Ident, Expr)],
        span: Span,
        expected: Option<&Ty>,
    ) -> Ty {
        let lower = Lower { g: self.g, diags: &mut self.diags };
        let def =
            lower.path_def(self.file, path).filter(|(_, used)| *used == path.segments.len()).map(|(d, _)| d);
        let g = self.g;
        let Some(adt) = def.and_then(|d| g.adts.get(&d).map(|a| (d, a))) else {
            for (_, v) in fields {
                self.check_expr(v, None);
            }
            return Ty::Error;
        };
        let (d, adt) = adt;
        self.out.records.insert(expr_id(self.file, span), d);
        let AdtKind::Record(decl) = &adt.kind else {
            self.error("E0401", path.span, format!("`{}` rakam (record) kaadhu", adt.name));
            return Ty::Error;
        };
        let fresh: Vec<Ty> = match expected {
            Some(Ty::App(Con::Adt(e), a)) if *e == d => a.clone(),
            _ => adt.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect(),
        };
        let mut seen: Vec<&str> = Vec::new();
        for (name, value) in fields {
            let Some(f) = decl.iter().find(|f| f.name == name.name) else {
                self.error("E0405", name.span, format!("`{}` lo `{}` ane field ledhu", adt.name, name.name));
                self.check_expr(value, None);
                continue;
            };
            if seen.contains(&name.name.as_str()) {
                self.error("E0416", name.span, format!("`{}` field rendu saarlu ichharu", name.name));
            }
            seen.push(&f.name);
            if !f.public && adt.module != self.module {
                self.error("E0303", name.span, format!("`{}` field private", f.name));
            }
            let want = f.ty.subst(&fresh);
            self.check_coerce(value, &want);
        }
        let missing: Vec<&str> =
            decl.iter().filter(|f| !seen.contains(&f.name.as_str())).map(|f| f.name.as_str()).collect();
        if !missing.is_empty() {
            self.error(
                "E0416",
                span,
                format!("`{}` lo ee fields ivvaledhu: {}", adt.name, missing.join(", ")),
            );
        }
        Ty::App(Con::Adt(d), fresh)
    }

    // ── iteration (§21.4) ──────────────────────────────────────────────────

    pub fn iter_item(&mut self, ty: &Ty, place: bool, span: Span) -> Ty {
        let t = self.shallow(ty);
        let by_ref = |cx: &mut Self, elem: Ty, mutable: bool| {
            if mutable {
                Ty::reference(true, elem)
            } else if cx.is_copy(&elem) {
                elem
            } else {
                Ty::reference(false, elem)
            }
        };
        match &t {
            Ty::App(Con::Paridhi | Con::Pravaham, args) => args[0].clone(),
            Ty::Ref(m, inner) => match self.shallow(inner) {
                Ty::Array(e, _) => by_ref(self, *e, *m),
                Ty::App(Con::Varusa | Con::Mukka, args) => by_ref(self, args[0].clone(), *m),
                Ty::App(Con::Jodi | Con::GajibijiJodi, args) => {
                    Ty::Tuple(vec![Ty::reference(false, args[0].clone()), Ty::reference(*m, args[1].clone())])
                }
                Ty::App(Con::Gumpu, args) => Ty::reference(false, args[0].clone()),
                inner => self.iter_trait(&inner, span),
            },
            Ty::Array(e, _) if place => by_ref(self, (**e).clone(), false),
            Ty::Array(e, _) => (**e).clone(),
            Ty::App(Con::Varusa, args) if place => by_ref(self, args[0].clone(), false),
            Ty::App(Con::Varusa, args) => args[0].clone(),
            Ty::App(Con::Jodi | Con::GajibijiJodi, args) => {
                Ty::Tuple(vec![Ty::reference(false, args[0].clone()), Ty::reference(false, args[1].clone())])
            }
            Ty::App(Con::Gumpu, args) => Ty::reference(false, args[0].clone()),
            Ty::Error => Ty::Error,
            Ty::Var(_) => self.need_annotation(span),
            other => {
                let other = other.clone();
                self.iter_trait(&other, span)
            }
        }
    }

    /// Items of a type implementing `Okkokkati[I]`.
    fn iter_trait(&mut self, ty: &Ty, span: Span) -> Ty {
        let item = self.fresh();
        if let Some(tr) = self.std_bound("Okkokkati", vec![item.clone()]) {
            let g = self.g;
            let solver = Solver { g, generics: &self.generics };
            if solver.implements(&mut self.infer, ty, &tr, 0) {
                return item;
            }
        }
        let shown = self.show(ty);
        self.error(
            "E0801",
            span,
            format!("`{shown}` mida `okko` cheyyalem: adhi okkokkati ivvadhu (`Okkokkati` gunam ledhu)"),
        );
        Ty::Error
    }

    // ── constants (§16) ────────────────────────────────────────────────────

    /// E1101: only literals, constants, operators and aggregates.
    pub fn check_const_expr(&mut self, e: &Expr) {
        let ok = match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Unit
            | ExprKind::Ledhu => true,
            ExprKind::Path(p) => match self.res_at(p.segments[0].span) {
                Some(Res::Def(d)) => {
                    let mut d = d;
                    for seg in &p.segments[1..] {
                        if let Some(Res::Def(m)) = self.res_at(seg.span) {
                            d = m;
                        }
                    }
                    matches!(self.g.res.defs[d].kind, DefKind::Const | DefKind::StdValue | DefKind::Enum)
                        || self.g.consts.contains_key(&d)
                        || p.segments.len() > 1
                }
                _ => false,
            },
            ExprKind::Binary { lhs, rhs, .. } => {
                self.check_const_expr(lhs);
                self.check_const_expr(rhs);
                true
            }
            ExprKind::Unary { op, operand } if !matches!(op, UnOp::Ref | UnOp::RefMut | UnOp::Deref) => {
                self.check_const_expr(operand);
                true
            }
            ExprKind::Tuple(items) | ExprKind::List(items) | ExprKind::Ctor { args: items, .. } => {
                items.iter().for_each(|i| self.check_const_expr(i));
                true
            }
            ExprKind::Record { fields, .. } => {
                fields.iter().for_each(|(_, v)| self.check_const_expr(v));
                true
            }
            ExprKind::Call { callee, phrases, .. } => {
                let conversion = matches!(&callee.kind, ExprKind::Path(p)
                    if p.segments.len() == 1
                        && matches!(p.segments[0].name.as_str(), "maarchu" | "kolpothu_maarchu"));
                for ph in phrases {
                    if let PhraseKind::Value(v) = &ph.kind {
                        self.check_const_expr(v);
                    }
                }
                conversion
            }
            _ => false,
        };
        if !ok {
            self.error(
                "E1101",
                e.span,
                "idhi constant kaadhu. Constants lo literals, itara constants, operators matrame (function calls ledhu)",
            );
        }
    }

    // ── end of body ────────────────────────────────────────────────────────

    pub fn finish(&mut self) -> Results {
        // §10.1: leftover literal variables take their default types.
        for v in 0..self.infer.len() as u32 {
            if self.infer.probe(v).is_none() {
                match self.infer.kind(v) {
                    VarKind::Int => self.infer.bind(v, Ty::ANKAM),
                    VarKind::Float => self.infer.bind(v, Ty::Float(64)),
                    VarKind::General => {}
                }
            }
        }
        for (ty, value, span) in std::mem::take(&mut self.int_lits) {
            if let Ty::Int(it) = self.infer.resolve(&ty) {
                let (lo, hi) = it.range();
                if value < lo || value > hi {
                    let shown = self.show(&Ty::Int(it));
                    self.error(
                        "E0402",
                        span,
                        format!("`{value}` `{shown}` lo pattadhu ({lo} nunchi {hi} varaku matrame)"),
                    );
                }
            }
        }
        for (src, dst, kind, span) in
            std::mem::take(&mut self.conversions).into_iter().map(|(k, s, d, sp)| (s, d, k, sp))
        {
            let s = self.infer.resolve(&src);
            let d = self.infer.resolve(&dst);
            if s.has_error() || d.has_error() || has_var(&s) || has_var(&d) {
                continue;
            }
            self.check_conversion(kind, &s, &d, span);
        }
        let had_errors = self.diags.iter().any(|(_, d)| d.is_error());
        for (ty, tr, span, code) in std::mem::take(&mut self.obligations) {
            let t = self.infer.resolve(&ty);
            if t.has_error() || has_var(&t) {
                continue;
            }
            if !self.implements(&t, &tr) {
                let shown = self.show(&t);
                let name = self.trait_name(tr.def);
                let msg = if code == "E0407" {
                    format!("`{shown}` ki ee operator ledhu (`{name}` gunam kavali)")
                } else {
                    format!("`{shown}` ki `{name}` gunam ledhu")
                };
                self.error(code, span, msg);
            }
        }
        if !had_errors {
            let mut unknown: Vec<(Span, String)> = Vec::new();
            for l in self.locals.values() {
                unknown.push((l.span, l.name.clone()));
            }
            unknown.sort_by_key(|(s, _)| s.start);
            for (span, name) in unknown {
                let ty = self.locals[&span.start].ty.clone();
                let t = self.infer.resolve(&ty);
                if has_var(&t) && !t.has_error() {
                    let shown = self.show(&t);
                    self.error(
                        "E0404",
                        span,
                        format!("`{name}` type inka teliyadhu (`{shown}`). Type cheppu: `{name}: … ante`"),
                    );
                    break;
                }
            }
        }
        let mut raw = std::mem::take(&mut self.out);
        for ((start, end), ty) in std::mem::take(&mut self.expr_types) {
            raw.expr_ty.insert((self.file, start, end), ty);
        }
        for (decl, l) in &self.locals {
            raw.local_ty.insert((self.file, *decl), (l.ty.clone(), l.mutable));
        }
        let mut done = Results::default();
        done.absorb(raw, &mut self.infer);
        done
    }

    fn check_conversion(&mut self, kind: Conv, s: &Ty, d: &Ty, span: Span) {
        let numeric = |t: &Ty| matches!(t, Ty::Int(_) | Ty::Float(_));
        let ok = match kind {
            Conv::Lossless => match (s, d) {
                (Ty::Int(a), Ty::Int(b)) => {
                    (a.signed == b.signed && b.bits >= a.bits) || (!a.signed && b.signed && b.bits > a.bits)
                }
                (Ty::Int(a), Ty::Float(32)) => a.bits <= 24 - u8::from(a.signed),
                (Ty::Int(a), Ty::Float(64)) => a.bits <= 32,
                (Ty::Float(a), Ty::Float(b)) => a <= b,
                (Ty::Char, Ty::Int(IntTy { signed: false, bits })) => *bits >= 32,
                _ => false,
            },
            Conv::Checked => {
                (numeric(s) && numeric(d))
                    || matches!((s, d), (Ty::Int(IntTy { signed: false, bits: 32 }), Ty::Char))
            }
            Conv::Lossy => {
                let ptr_int = matches!((s, d), (Ty::Ptr(..), Ty::Int(_)) | (Ty::Int(_), Ty::Ptr(..)));
                if ptr_int {
                    self.require_unsafe(span, "pointer ↔ integer maarpu");
                }
                (numeric(s) && numeric(d)) || ptr_int || matches!((s, d), (Ty::Ptr(..), Ty::Ptr(..)))
            }
        };
        if ok {
            return;
        }
        let (ss, ds) = (self.show(s), self.show(d));
        if kind == Conv::Lossless && (numeric(s) || *s == Ty::Char) && numeric(d) {
            self.error(
                "E0409",
                span,
                format!(
                    "`{ss}` nunchi `{ds}` ki `maarchu` value kolpovachu. `kudhirithe_maarchu` (undochu isthundhi) leka `kolpothu_maarchu` vaadu"
                ),
            );
        } else {
            self.error("E0401", span, format!("`{ss}` ni `{ds}` ga maarchalem"));
        }
    }

    /// A function's origin decides whether calling it is unsafe (§22).
    pub fn origin_unsafe(origin: FnOrigin) -> bool {
        matches!(origin, FnOrigin::Foreign | FnOrigin::StdUnsafe)
    }
}

pub fn has_var(t: &Ty) -> bool {
    let mut found = false;
    t.walk(&mut |x| found |= matches!(x, Ty::Var(_)));
    found
}

pub fn is_place(e: &Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Path(_)
            | ExprKind::Field { .. }
            | ExprKind::TupleIndex { .. }
            | ExprKind::Index { .. }
            | ExprKind::Unary { op: UnOp::Deref, .. }
    )
}

fn describe(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Path(p) => p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."),
        _ => "idhi".to_string(),
    }
}

/// A literal map key, for duplicate detection (E0419).
fn const_key(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Int(lit) => i128::from_str_radix(&lit.digits, lit.radix).ok().map(|v| v.to_string()),
        ExprKind::Str(s) => Some(format!("{s:?}")),
        ExprKind::Char(c) => Some(format!("{c:?}")),
        ExprKind::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The method table head for a built-in type word in value position.
pub fn builtin_head(name: &str) -> Option<super::Head> {
    use super::Head;
    Some(match name {
        "padham" => Head::Str,
        "varusa" => Head::Con(Con::Varusa),
        "mukka" => Head::Con(Con::Mukka),
        "jodi" => Head::Con(Con::Jodi),
        "undochu" => Head::Con(Con::Undochu),
        "sare_thappu" => Head::Con(Con::SareThappu),
        "pette" => Head::Con(Con::Pette),
        "aksharam" => Head::Char,
        "amsam64" | "amsam" => Head::Float(64),
        "amsam32" => Head::Float(32),
        n if n.starts_with("ankam") || n.starts_with("aasajeevi") => Head::Int,
        _ => return None,
    })
}
