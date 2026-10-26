//! One MIR body → one LLVM function. Every local lives in an `alloca`;
//! LLVM's `mem2reg`/SROA turn them into registers at `-O2`.

use std::collections::HashMap;
use std::fmt::Write as _;

use super::{float_lit, Gen, Item};
use crate::ast::{BinOp, UnOp};
use crate::mir::*;
use crate::resolve::FileId;
use crate::source::Span;
use crate::typeck::{FnOrigin, RecvMode};
use crate::types::{Con, IntTy, Ty};

pub(super) struct Fx<'g, 'a> {
    pub gen: &'g mut Gen<'a>,
    pub body: Option<&'a Body>,
    pub tys: Vec<Ty>,
    pub out: String,
    n: usize,
    pub locals: Vec<Ty>,
    flags: HashMap<Local, String>,
    pub file: Option<FileId>,
}

/// A place: a pointer and its type, or a slice view (data pointer and length).
#[derive(Clone)]
pub(super) struct PRef {
    pub ptr: String,
    pub ty: Ty,
    pub len: Option<String>,
    /// After `Proj::Variant`: the variant's field types; `ptr` is the payload.
    pub variant: Option<Vec<Ty>>,
}

pub(super) fn emit_body(gen: &mut Gen, name: &str, b: BodyId, tys: &[Ty]) -> String {
    let body = &gen.prog.bodies[b as usize];
    let locals: Vec<Ty> =
        body.locals.iter().map(|l| if tys.is_empty() { l.ty.clone() } else { l.ty.subst(tys) }).collect();
    let file = body.file;
    let mut fx = Fx {
        gen,
        body: Some(body),
        tys: tys.to_vec(),
        out: String::new(),
        n: 0,
        locals,
        flags: HashMap::new(),
        file,
    };
    fx.function(name);
    fx.out
}

impl<'g, 'a> Fx<'g, 'a> {
    pub fn new_for_glue(gen: &'g mut Gen<'a>) -> Self {
        Fx {
            gen,
            body: None,
            tys: Vec::new(),
            out: String::new(),
            n: 0,
            locals: Vec::new(),
            flags: HashMap::new(),
            file: None,
        }
    }

    // ── small helpers ──────────────────────────────────────────────────────

    pub fn r(&mut self) -> String {
        self.n += 1;
        format!("%v{}", self.n)
    }

    pub fn label(&mut self) -> String {
        self.n += 1;
        format!("L{}", self.n)
    }

    pub fn emit(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.out, "  {}", line.as_ref());
    }

    pub fn start(&mut self, label: &str) {
        let _ = writeln!(self.out, "{label}:");
    }

    pub fn ll(&mut self, t: &Ty) -> String {
        self.gen.ll(t)
    }

    pub fn subst(&self, t: &Ty) -> Ty {
        if self.tys.is_empty() {
            t.clone()
        } else {
            t.subst(&self.tys)
        }
    }

    pub fn loc(&mut self, span: Span) -> String {
        self.gen.loc(self.file, span)
    }

    /// Branches to a trap block when `cond` holds; continues otherwise.
    pub fn guard(&mut self, cond: &str, trap: impl FnOnce(&mut Self)) {
        let bad = self.label();
        let ok = self.label();
        self.emit(format!("br i1 {cond}, label %{bad}, label %{ok}"));
        self.start(&bad);
        trap(self);
        self.emit("unreachable");
        self.start(&ok);
    }

    pub fn alloca(&mut self, ll: &str) -> String {
        let p = self.r();
        self.emit(format!("{p} = alloca {ll}"));
        p
    }

    /// Stores a value in a fresh stack slot and returns its address.
    pub fn spill(&mut self, ll: &str, v: &str) -> String {
        let p = self.alloca(ll);
        self.emit(format!("store {ll} {v}, ptr {p}"));
        p
    }

    pub fn load(&mut self, ll: &str, ptr: &str) -> String {
        let v = self.r();
        self.emit(format!("{v} = load {ll}, ptr {ptr}"));
        v
    }

    pub fn gep(&mut self, ll: &str, ptr: &str, idx: &[String]) -> String {
        let v = self.r();
        let parts: Vec<String> =
            idx.iter()
                .map(|i| {
                    if i.starts_with("i64 ") || i.starts_with("i32 ") {
                        i.clone()
                    } else {
                        format!("i32 {i}")
                    }
                })
                .collect();
        self.emit(format!("{v} = getelementptr {ll}, ptr {ptr}, {}", parts.join(", ")));
        v
    }

    /// Widens an integer of type `it` to `i64` (signed or unsigned).
    pub fn widen(&mut self, v: &str, it: IntTy) -> String {
        if it.bits == 64 {
            return v.to_string();
        }
        let r = self.r();
        let op = if it.signed { "sext" } else { "zext" };
        self.emit(format!("{r} = {op} i{} {v} to i64", it.bits));
        r
    }

    pub fn narrow(&mut self, v: &str, it: IntTy) -> String {
        if it.bits == 64 {
            return v.to_string();
        }
        let r = self.r();
        self.emit(format!("{r} = trunc i64 {v} to i{}", it.bits));
        r
    }

    // ── the function ───────────────────────────────────────────────────────

    fn function(&mut self, name: &str) {
        let Some(body) = self.body else { return };
        let ret = self.locals[0].clone();
        let rt = self.ll(&ret);
        let closure_caps = match &body.kind {
            BodyKind::Closure { captures } => Some(*captures as usize),
            _ => None,
        };
        let nparams = body.params as usize;
        let mut params = Vec::new();
        if closure_caps.is_some() {
            params.push("ptr %env".to_string());
        }
        let first_real = 1 + closure_caps.unwrap_or(0);
        for i in first_real..=nparams {
            let t = self.locals[i].clone();
            let lt = self.ll(&t);
            params.push(format!("{lt} %p{i}"));
        }
        let header = format!("define internal {rt} {name}({}) {{\nentry:\n", params.join(", "));
        // Locals that `Drop` statements name get a drop flag (§19.3).
        let mut flagged: Vec<Local> = Vec::new();
        for b in &body.blocks {
            for s in &b.stmts {
                if let Stmt::Drop(p, _) = s {
                    if p.proj.is_empty() && !flagged.contains(&p.local) {
                        flagged.push(p.local);
                    }
                }
            }
        }
        for i in 0..self.locals.len() {
            let t = self.locals[i].clone();
            let lt = self.ll(&t);
            self.emit(format!("%l{i} = alloca {lt}"));
        }
        for &l in &flagged {
            let f = format!("%f{l}");
            self.emit(format!("{f} = alloca i1"));
            let init = if (l as usize) >= 1 && (l as usize) <= nparams { "true" } else { "false" };
            self.emit(format!("store i1 {init}, ptr {f}"));
            self.flags.insert(l, f);
        }
        if let Some(caps) = closure_caps {
            let cap_tys: Vec<Ty> = self.locals[1..=caps].to_vec();
            let env_ty = self.ll(&Ty::Tuple(cap_tys.clone()));
            for (k, t) in cap_tys.iter().enumerate() {
                let lt = self.ll(t);
                let p = self.gep(&env_ty, "%env", &["0".into(), k.to_string()]);
                let v = self.load(&lt, &p);
                self.emit(format!("store {lt} {v}, ptr %l{}", k + 1));
            }
        }
        for i in first_real..=nparams {
            let t = self.locals[i].clone();
            let lt = self.ll(&t);
            self.emit(format!("store {lt} %p{i}, ptr %l{i}"));
        }
        self.emit("br label %bb0");
        let prologue = std::mem::take(&mut self.out);
        for (i, b) in body.blocks.iter().enumerate() {
            self.start(&format!("bb{i}"));
            for s in &b.stmts {
                self.stmt(s);
            }
            self.term(&b.term);
        }
        let blocks = std::mem::take(&mut self.out);
        self.out = format!("{header}{prologue}{blocks}}}\n");
    }

    // ── places ─────────────────────────────────────────────────────────────

    pub fn place(&mut self, p: &Place) -> PRef {
        let mut cur = PRef {
            ptr: format!("%l{}", p.local),
            ty: self.locals[p.local as usize].clone(),
            len: None,
            variant: None,
        };
        for proj in &p.proj {
            cur = self.project(cur, proj);
        }
        cur
    }

    fn project(&mut self, cur: PRef, proj: &Proj) -> PRef {
        match proj {
            Proj::Variant(v) => {
                let et = self.ll(&cur.ty);
                let payload = self.gep(&et, &cur.ptr, &["0".into(), "1".into()]);
                let fields = self.gen.variants(&cur.ty).get(*v as usize).cloned().unwrap_or_default();
                PRef { ptr: payload, ty: cur.ty, len: None, variant: Some(fields) }
            }
            Proj::Field(i) => {
                let fields = match cur.variant {
                    Some(f) => f,
                    None => match &cur.ty {
                        Ty::Tuple(items) => items.clone(),
                        Ty::App(Con::Paridhi, a) => vec![a[0].clone(), a[0].clone(), Ty::Bool],
                        other => self.gen.variants(other).pop().unwrap_or_default(),
                    },
                };
                let st = self.ll(&Ty::Tuple(fields.clone()));
                let ptr = self.gep(&st, &cur.ptr, &["0".into(), i.to_string()]);
                PRef {
                    ptr,
                    ty: fields.get(*i as usize).cloned().unwrap_or(Ty::Unit),
                    len: None,
                    variant: None,
                }
            }
            Proj::Deref => {
                let inner = match &cur.ty {
                    Ty::Ref(_, inner) | Ty::Ptr(_, inner) => (**inner).clone(),
                    Ty::App(Con::Pette, a) => a[0].clone(),
                    _ => return cur,
                };
                {
                    if let Ty::App(Con::Mukka, _) = inner {
                        let s = self.load("%slice", &cur.ptr);
                        let data = self.r();
                        self.emit(format!("{data} = extractvalue %slice {s}, 0"));
                        let len = self.r();
                        self.emit(format!("{len} = extractvalue %slice {s}, 1"));
                        return PRef { ptr: data, ty: inner, len: Some(len), variant: None };
                    }
                    let ptr = self.load("ptr", &cur.ptr);
                    PRef { ptr, ty: inner, len: None, variant: None }
                }
            }
            Proj::Index(l, span) => {
                let it = match &self.locals[*l as usize] {
                    Ty::Int(it) => *it,
                    _ => IntTy::ANKAM,
                };
                let raw = self.load(&format!("i{}", it.bits), &format!("%l{l}"));
                let idx = self.widen(&raw, it);
                let (data, len, elem) = self.seq_parts_place(&cur);
                let ok = self.r();
                self.emit(format!("{ok} = icmp uge i64 {idx}, {len}"));
                let loc = self.loc(*span);
                let (i2, l2) = (idx.clone(), len.clone());
                self.guard(&ok, |fx| {
                    fx.emit(format!("call void @ante_trap_index(i64 {i2}, i64 {l2}, ptr {loc})"))
                });
                let et = self.ll(&elem);
                let ptr = self.gep(&et, &data, &[format!("i64 {idx}")]);
                PRef { ptr, ty: elem, len: None, variant: None }
            }
        }
    }

    /// Data pointer, length and element type of a sequence place.
    pub fn seq_parts_place(&mut self, cur: &PRef) -> (String, String, Ty) {
        if let (Some(len), Ty::App(Con::Mukka, a)) = (&cur.len, &cur.ty) {
            return (cur.ptr.clone(), len.clone(), a[0].clone());
        }
        match &cur.ty {
            Ty::Array(e, n) => (cur.ptr.clone(), n.unwrap_or(0).to_string(), (**e).clone()),
            Ty::App(Con::Varusa, a) => {
                let dp = self.gep("%vec", &cur.ptr, &["0".into(), "0".into()]);
                let data = self.load("ptr", &dp);
                let lp = self.gep("%vec", &cur.ptr, &["0".into(), "1".into()]);
                let len = self.load("i64", &lp);
                (data, len, a[0].clone())
            }
            Ty::Str => {
                let dp = self.gep("%str", &cur.ptr, &["0".into(), "0".into()]);
                let data = self.load("ptr", &dp);
                let lp = self.gep("%str", &cur.ptr, &["0".into(), "1".into()]);
                let len = self.load("i64", &lp);
                (data, len, Ty::Int(IntTy { signed: false, bits: 8 }))
            }
            other => {
                let shown = format!("{other:?}");
                self.gen.error(format!("native backend: `{shown}` index cheyyalem"));
                ("null".into(), "0".into(), Ty::Unit)
            }
        }
    }

    // ── operands ───────────────────────────────────────────────────────────

    pub fn operand_ty(&mut self, op: &Operand) -> Option<Ty> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => Some(self.place_ty(p)),
            Operand::Const(Const::Bool(_)) => Some(Ty::Bool),
            Operand::Const(Const::Char(_)) => Some(Ty::Char),
            Operand::Const(Const::Str(_)) => Some(Ty::Str),
            Operand::Const(Const::Unit) => Some(Ty::Unit),
            Operand::Const(Const::F64(_)) => Some(Ty::Float(64)),
            Operand::Const(Const::F32(_)) => Some(Ty::Float(32)),
            Operand::Const(Const::Fn(fid, tys)) => {
                let tys: Vec<Ty> = tys.iter().map(|t| self.subst(t)).collect();
                Some(self.fn_value_ty(*fid, &tys))
            }
            Operand::Const(Const::Int(_)) => None,
        }
    }

    /// The static type of a place, without emitting code.
    pub fn place_ty(&mut self, p: &Place) -> Ty {
        let mut ty = self.locals[p.local as usize].clone();
        let mut variant: Option<Vec<Ty>> = None;
        for proj in &p.proj {
            match proj {
                Proj::Variant(v) => variant = self.gen.variants(&ty).get(*v as usize).cloned(),
                Proj::Field(i) => {
                    let fields = match variant.take() {
                        Some(f) => f,
                        None => match &ty {
                            Ty::Tuple(items) => items.clone(),
                            Ty::App(Con::Paridhi, a) => vec![a[0].clone(), a[0].clone(), Ty::Bool],
                            other => self.gen.variants(other).pop().unwrap_or_default(),
                        },
                    };
                    ty = fields.get(*i as usize).cloned().unwrap_or(Ty::Unit);
                }
                Proj::Deref => {
                    ty = match ty {
                        Ty::Ref(_, inner) | Ty::Ptr(_, inner) => *inner,
                        Ty::App(Con::Pette, a) => a[0].clone(),
                        other => other,
                    }
                }
                Proj::Index(..) => {
                    ty = match ty {
                        Ty::Array(e, _) => *e,
                        Ty::App(Con::Varusa | Con::Mukka, a) => a[0].clone(),
                        other => other,
                    }
                }
            }
        }
        ty
    }

    pub fn fn_value_ty(&mut self, fid: crate::typeck::FnId, tys: &[Ty]) -> Ty {
        let sig = &self.gen.g.fns[fid];
        let mut params: Vec<(Ty, Option<crate::ast::Role>)> = Vec::new();
        if let Some(r) = &sig.receiver {
            let t = r.ty.subst(tys);
            params.push((
                match r.mode {
                    RecvMode::Value => t,
                    RecvMode::Ref => Ty::reference(false, t),
                    RecvMode::RefMut => Ty::reference(true, t),
                },
                None,
            ));
        }
        params.extend(sig.params.iter().map(|p| (p.ty.subst(tys), p.role)));
        Ty::Fn(params, Box::new(sig.ret.subst(tys)))
    }

    /// The value of `op`, as type `want`.
    pub fn operand(&mut self, op: &Operand, want: &Ty) -> String {
        match op {
            Operand::Const(c) => self.constant(c, want),
            Operand::Copy(p) | Operand::Move(p) => {
                let pr = self.place(p);
                if pr.len.is_some() {
                    // A whole `mukka` place used as a value: its slice.
                    let s = self.slice_value(&pr.ptr, pr.len.as_deref().unwrap_or("0"));
                    let from = Ty::reference(false, pr.ty.clone());
                    return self.coerce(&s, &from, want);
                }
                let lt = self.ll(&pr.ty);
                let v = self.load(&lt, &pr.ptr);
                if matches!(op, Operand::Move(_)) {
                    if let Some(f) = self.flags.get(&p.local).cloned() {
                        self.emit(format!("store i1 false, ptr {f}"));
                    }
                }
                self.coerce(&v, &pr.ty, want)
            }
        }
    }

    pub fn slice_value(&mut self, data: &str, len: &str) -> String {
        let a = self.r();
        self.emit(format!("{a} = insertvalue %slice undef, ptr {data}, 0"));
        let b = self.r();
        self.emit(format!("{b} = insertvalue %slice {a}, i64 {len}, 1"));
        b
    }

    fn constant(&mut self, c: &Const, want: &Ty) -> String {
        match c {
            Const::Unit => "zeroinitializer".into(),
            Const::Bool(b) => (if *b { "true" } else { "false" }).into(),
            Const::Char(c) => (*c as u32).to_string(),
            Const::Int(v) => match want {
                Ty::Int(it) => crate::interp::wrap(*it, *v).to_string(),
                Ty::Float(b) => float_lit(*b, *v as f64),
                Ty::Char => v.to_string(),
                _ => v.to_string(),
            },
            Const::F32(x) => float_lit(if matches!(want, Ty::Float(64)) { 64 } else { 32 }, f64::from(*x)),
            Const::F64(x) => float_lit(if matches!(want, Ty::Float(32)) { 32 } else { 64 }, *x),
            Const::Str(s) => {
                let s = s.to_string();
                self.gen.str_const(&s)
            }
            Const::Fn(fid, tys) => {
                let tys: Vec<Ty> = tys.iter().map(|t| self.subst(t)).collect();
                let thunk = self.gen.item(Item::Thunk(*fid, tys));
                format!("{{ ptr {thunk}, ptr null }}")
            }
        }
    }

    /// Converts a value of type `from` to `to` (the implicit coercions of §9.4.2).
    pub fn coerce(&mut self, v: &str, from: &Ty, to: &Ty) -> String {
        if from == to {
            return v.to_string();
        }
        match (from, to) {
            (Ty::Never, _) => "undef".into(),
            (Ty::Ref(_, a), Ty::Ref(_, b)) => match (&**a, &**b) {
                (Ty::App(Con::Varusa, _), Ty::App(Con::Mukka, _)) => {
                    let dp = self.gep("%vec", v, &["0".into(), "0".into()]);
                    let data = self.load("ptr", &dp);
                    let lp = self.gep("%vec", v, &["0".into(), "1".into()]);
                    let len = self.load("i64", &lp);
                    self.slice_value(&data, &len)
                }
                (Ty::Array(_, n), Ty::App(Con::Mukka, _)) => self.slice_value(v, &n.unwrap_or(0).to_string()),
                (Ty::App(Con::Pette, inner), other) if !matches!(other, Ty::App(Con::Pette, _)) => {
                    let p = self.load("ptr", v);
                    let inner = Ty::reference(false, inner[0].clone());
                    self.coerce(&p, &inner, to)
                }
                _ => v.to_string(),
            },
            _ => v.to_string(),
        }
    }

    // ── statements ─────────────────────────────────────────────────────────

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Assign(p, rv) => {
                let want = self.place_ty(p);
                let v = self.rvalue(rv, &want);
                let pr = self.place(p);
                let lt = self.ll(&pr.ty);
                self.emit(format!("store {lt} {v}, ptr {}", pr.ptr));
                if p.proj.is_empty() {
                    if let Some(f) = self.flags.get(&p.local).cloned() {
                        self.emit(format!("store i1 true, ptr {f}"));
                    }
                }
            }
            Stmt::Replace(p, rv, ty) => {
                let want = self.place_ty(p);
                let v = self.rvalue(rv, &want);
                let pr = self.place(p);
                let lt = self.ll(&pr.ty);
                let old = self.load(&lt, &pr.ptr);
                self.emit(format!("store {lt} {v}, ptr {}", pr.ptr));
                let ty = self.subst(ty);
                if self.needs_drop(&ty) {
                    let slot = self.spill(&lt, &old);
                    let d = self.gen.item(Item::Drop(ty));
                    self.emit(format!("call void {d}(ptr {slot})"));
                }
            }
            Stmt::Drop(p, ty) => {
                let ty = self.subst(ty);
                if !self.needs_drop(&ty) {
                    return;
                }
                let pr = self.place(p);
                let d = self.gen.item(Item::Drop(ty));
                match (p.proj.is_empty(), self.flags.get(&p.local).cloned()) {
                    (true, Some(f)) => {
                        let on = self.load("i1", &f);
                        let yes = self.label();
                        let done = self.label();
                        self.emit(format!("br i1 {on}, label %{yes}, label %{done}"));
                        self.start(&yes);
                        self.emit(format!("call void {d}(ptr {})", pr.ptr));
                        self.emit(format!("store i1 false, ptr {f}"));
                        self.emit(format!("br label %{done}"));
                        self.start(&done);
                    }
                    _ => self.emit(format!("call void {d}(ptr {})", pr.ptr)),
                }
            }
        }
    }

    pub fn needs_drop(&mut self, ty: &Ty) -> bool {
        let cache = std::cell::RefCell::new(HashMap::new());
        crate::mir::needs_drop_ty(self.gen.g, &[], &cache, ty)
    }

    // ── rvalues ────────────────────────────────────────────────────────────

    fn rvalue(&mut self, rv: &Rvalue, want: &Ty) -> String {
        match rv {
            Rvalue::Use(op) => self.operand(op, want),
            Rvalue::Ref(_, p) => {
                let pr = self.place(p);
                if let Some(len) = &pr.len {
                    let s = self.slice_value(&pr.ptr, len);
                    let from = Ty::reference(false, pr.ty.clone());
                    return self.coerce(&s, &from, want);
                }
                let from = Ty::reference(false, pr.ty.clone());
                self.coerce(&pr.ptr, &from, want)
            }
            Rvalue::Slice { place, lo, hi, inclusive, span, .. } => {
                let pr = self.place(place);
                let (data, len, elem) = self.seq_parts_place(&pr);
                let lo = self.index_operand(lo);
                let hi0 = self.index_operand(hi);
                let hi = if *inclusive {
                    let h = self.r();
                    self.emit(format!("{h} = add i64 {hi0}, 1"));
                    h
                } else {
                    hi0
                };
                let a = self.r();
                self.emit(format!("{a} = icmp ugt i64 {lo}, {hi}"));
                let b = self.r();
                self.emit(format!("{b} = icmp ugt i64 {hi}, {len}"));
                let bad = self.r();
                self.emit(format!("{bad} = or i1 {a}, {b}"));
                let loc = self.loc(*span);
                let (l2, h2, n2) = (lo.clone(), hi.clone(), len.clone());
                self.guard(&bad, |fx| {
                    fx.emit(format!("call void @ante_trap_slice(i64 {l2}, i64 {h2}, i64 {n2}, ptr {loc})"))
                });
                let et = self.ll(&elem);
                let start = self.gep(&et, &data, &[format!("i64 {lo}")]);
                let n = self.r();
                self.emit(format!("{n} = sub i64 {hi}, {lo}"));
                self.slice_value(&start, &n)
            }
            Rvalue::Binary(op, t, a, b, span) => self.binary(*op, *t, a, b, *span),
            Rvalue::Unary(op, t, a, span) => self.unary(*op, *t, a, *span),
            Rvalue::Aggregate(agg, ops) => self.aggregate(agg, ops, want),
            Rvalue::Discriminant(p) => {
                let pr = self.place(p);
                if self.gen.is_enum(&pr.ty) {
                    let et = self.ll(&pr.ty);
                    let tp = self.gep(&et, &pr.ptr, &["0".into(), "0".into()]);
                    let tag = self.load("i32", &tp);
                    match want {
                        Ty::Int(it) if it.bits > 32 => {
                            let r = self.r();
                            self.emit(format!("{r} = zext i32 {tag} to i{}", it.bits));
                            r
                        }
                        _ => tag,
                    }
                } else {
                    "0".into()
                }
            }
            Rvalue::Range(a, b, inclusive, it) => {
                let t = Ty::Int(*it);
                let lt = self.ll(want);
                let x = self.operand(a, &t);
                let y = self.operand(b, &t);
                let i = format!("i{}", it.bits);
                let r1 = self.r();
                self.emit(format!("{r1} = insertvalue {lt} undef, {i} {x}, 0"));
                let r2 = self.r();
                self.emit(format!("{r2} = insertvalue {lt} {r1}, {i} {y}, 1"));
                let r3 = self.r();
                self.emit(format!("{r3} = insertvalue {lt} {r2}, i1 {inclusive}, 2"));
                r3
            }
        }
    }

    /// An index or bound, as `i64`.
    fn index_operand(&mut self, op: &Operand) -> String {
        match self.operand_ty(op) {
            Some(Ty::Int(it)) => {
                let v = self.operand(op, &Ty::Int(it));
                self.widen(&v, it)
            }
            _ => self.operand(op, &Ty::ANKAM),
        }
    }

    fn aggregate(&mut self, agg: &Agg, ops: &[Operand], want: &Ty) -> String {
        let lt = self.ll(want);
        match agg {
            Agg::Tuple | Agg::Array => {
                let fields: Vec<Ty> = match want {
                    Ty::Tuple(items) => items.clone(),
                    Ty::Array(e, _) => vec![(**e).clone(); ops.len()],
                    _ => Vec::new(),
                };
                self.build_struct(&lt, ops, &fields)
            }
            Agg::Adt(_, v) => {
                if self.gen.is_enum(want) {
                    let fields = self.gen.variants(want).get(*v as usize).cloned().unwrap_or_default();
                    let vals: Vec<String> =
                        ops.iter().zip(&fields).map(|(o, t)| self.operand(o, t)).collect();
                    self.build_enum(want, *v, &vals, &fields)
                } else {
                    let fields = self.gen.variants(want).pop().unwrap_or_default();
                    self.build_struct(&lt, ops, &fields)
                }
            }
            Agg::Varusa => {
                let elem = match want {
                    Ty::App(Con::Varusa, a) => a[0].clone(),
                    _ => Ty::Unit,
                };
                let vals: Vec<String> = ops.iter().map(|o| self.operand(o, &elem)).collect();
                self.build_vec(&elem, &vals)
            }
            Agg::Pette => {
                let inner = match want {
                    Ty::App(Con::Pette, a) => a[0].clone(),
                    _ => Ty::Unit,
                };
                let v = match ops.first() {
                    Some(o) => self.operand(o, &inner),
                    None => "zeroinitializer".into(),
                };
                self.boxed(&inner, &v)
            }
            Agg::Closure(body) => {
                let caps = match &self.gen.prog.bodies[*body as usize].kind {
                    BodyKind::Closure { captures } => *captures as usize,
                    _ => 0,
                };
                let cbody = &self.gen.prog.bodies[*body as usize];
                let cap_tys: Vec<Ty> = cbody.locals[1..=caps].iter().map(|l| self.subst(&l.ty)).collect();
                let env_ty = Ty::Tuple(cap_tys.clone());
                let et = self.ll(&env_ty);
                let (size, _) = self.gen.size_align(&env_ty);
                let env = self.r();
                self.emit(format!("{env} = call ptr @ante_alloc(i64 {size})"));
                for (k, (o, t)) in ops.iter().zip(&cap_tys).enumerate() {
                    let v = self.operand(o, t);
                    let ct = self.ll(t);
                    let p = self.gep(&et, &env, &["0".into(), k.to_string()]);
                    self.emit(format!("store {ct} {v}, ptr {p}"));
                }
                let f = self.gen.item(Item::Body(*body, self.tys.clone()));
                let a = self.r();
                self.emit(format!("{a} = insertvalue %pani undef, ptr {f}, 0"));
                let b = self.r();
                self.emit(format!("{b} = insertvalue %pani {a}, ptr {env}, 1"));
                b
            }
            Agg::Jodi => {
                self.gen.error("native backend lo `jodi` ippatiki ledhu; `antec run` vaadandi");
                "zeroinitializer".into()
            }
        }
    }

    pub fn build_struct(&mut self, lt: &str, ops: &[Operand], fields: &[Ty]) -> String {
        let mut acc = "undef".to_string();
        if fields.is_empty() {
            return "zeroinitializer".into();
        }
        for (i, (o, t)) in ops.iter().zip(fields).enumerate() {
            let v = self.operand(o, t);
            let ft = self.ll(t);
            let r = self.r();
            self.emit(format!("{r} = insertvalue {lt} {acc}, {ft} {v}, {i}"));
            acc = r;
        }
        acc
    }

    pub fn build_struct_vals(&mut self, lt: &str, vals: &[String], fields: &[Ty]) -> String {
        if fields.is_empty() {
            return "zeroinitializer".into();
        }
        let mut acc = "undef".to_string();
        for (i, (v, t)) in vals.iter().zip(fields).enumerate() {
            let ft = self.ll(t);
            let r = self.r();
            self.emit(format!("{r} = insertvalue {lt} {acc}, {ft} {v}, {i}"));
            acc = r;
        }
        acc
    }

    /// An enum value of variant `v` with field values.
    pub fn build_enum(&mut self, ty: &Ty, v: u32, vals: &[String], fields: &[Ty]) -> String {
        let et = self.ll(ty);
        let slot = self.alloca(&et);
        let tp = self.gep(&et, &slot, &["0".into(), "0".into()]);
        self.emit(format!("store i32 {v}, ptr {tp}"));
        if !fields.is_empty() {
            let pay = self.gep(&et, &slot, &["0".into(), "1".into()]);
            let st = self.ll(&Ty::Tuple(fields.to_vec()));
            for (i, (x, t)) in vals.iter().zip(fields).enumerate() {
                let ft = self.ll(t);
                let p = self.gep(&st, &pay, &["0".into(), i.to_string()]);
                self.emit(format!("store {ft} {x}, ptr {p}"));
            }
        }
        self.load(&et, &slot)
    }

    pub fn build_vec(&mut self, elem: &Ty, vals: &[String]) -> String {
        let (size, _) = self.gen.size_align(elem);
        let n = vals.len();
        let data = self.r();
        self.emit(format!("{data} = call ptr @ante_alloc(i64 {})", (size as usize * n).max(1)));
        let et = self.ll(elem);
        for (i, v) in vals.iter().enumerate() {
            let p = self.gep(&et, &data, &[format!("i64 {i}")]);
            self.emit(format!("store {et} {v}, ptr {p}"));
        }
        let a = self.r();
        self.emit(format!("{a} = insertvalue %vec undef, ptr {data}, 0"));
        let b = self.r();
        self.emit(format!("{b} = insertvalue %vec {a}, i64 {n}, 1"));
        let c = self.r();
        self.emit(format!("{c} = insertvalue %vec {b}, i64 {n}, 2"));
        c
    }

    pub fn boxed(&mut self, inner: &Ty, v: &str) -> String {
        let (size, _) = self.gen.size_align(inner);
        let p = self.r();
        self.emit(format!("{p} = call ptr @ante_alloc(i64 {size})"));
        let it = self.ll(inner);
        self.emit(format!("store {it} {v}, ptr {p}"));
        p
    }

    // ── operators (§10.5, §18) ─────────────────────────────────────────────

    fn binary(&mut self, op: BinOp, t: OpTy, a: &Operand, b: &Operand, span: Span) -> String {
        match t {
            OpTy::Int(it) => {
                let ty = Ty::Int(it);
                let x = self.operand(a, &ty);
                if matches!(op, BinOp::Shl | BinOp::Shr) {
                    let (y, yt) = match self.operand_ty(b) {
                        Some(Ty::Int(bt)) => (self.operand(b, &Ty::Int(bt)), bt),
                        _ => (self.operand(b, &ty), it),
                    };
                    return self.shift(op, it, &x, &y, yt, span);
                }
                let y = self.operand(b, &ty);
                self.int_binary(op, it, &x, &y, span)
            }
            OpTy::Float(bits) => {
                let ty = Ty::Float(bits);
                let x = self.operand(a, &ty);
                let y = self.operand(b, &ty);
                let ft = if bits == 32 { "float" } else { "double" };
                let r = self.r();
                let line = match op {
                    BinOp::Add => format!("{r} = fadd {ft} {x}, {y}"),
                    BinOp::Sub => format!("{r} = fsub {ft} {x}, {y}"),
                    BinOp::Mul => format!("{r} = fmul {ft} {x}, {y}"),
                    BinOp::Div => format!("{r} = fdiv {ft} {x}, {y}"),
                    BinOp::Rem => format!("{r} = frem {ft} {x}, {y}"),
                    _ => format!("{r} = fcmp {} {ft} {x}, {y}", fcmp(op)),
                };
                self.emit(line);
                r
            }
            OpTy::Bool | OpTy::Char => {
                let ty = if t == OpTy::Bool { Ty::Bool } else { Ty::Char };
                let lt = if t == OpTy::Bool { "i1" } else { "i32" };
                let x = self.operand(a, &ty);
                let y = self.operand(b, &ty);
                let r = self.r();
                let line = match op {
                    BinOp::BitAnd => format!("{r} = and {lt} {x}, {y}"),
                    BinOp::BitOr => format!("{r} = or {lt} {x}, {y}"),
                    BinOp::BitXor => format!("{r} = xor {lt} {x}, {y}"),
                    _ => format!("{r} = icmp {} {lt} {x}, {y}", icmp(op, false)),
                };
                self.emit(line);
                r
            }
            OpTy::Str => {
                let x = self.operand(a, &Ty::Str);
                let y = self.operand(b, &Ty::Str);
                let px = self.spill("%str", &x);
                let py = self.spill("%str", &y);
                if op == BinOp::Add {
                    let out = self.alloca("%str");
                    self.emit(format!("call void @ante_str_concat(ptr {out}, ptr {px}, ptr {py})"));
                    return self.load("%str", &out);
                }
                let c = self.r();
                self.emit(format!("{c} = call i32 @ante_str_cmp(ptr {px}, ptr {py})"));
                let r = self.r();
                self.emit(format!("{r} = icmp {} i32 {c}, 0", icmp(op, true)));
                r
            }
            OpTy::Unit => {
                (if matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge) { "true" } else { "false" }).into()
            }
        }
    }

    pub fn int_binary(&mut self, op: BinOp, it: IntTy, x: &str, y: &str, span: Span) -> String {
        let i = format!("i{}", it.bits);
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul => {
                let (name, sym) = match op {
                    BinOp::Add => ("add", "+"),
                    BinOp::Sub => ("sub", "-"),
                    _ => ("mul", "*"),
                };
                let s = if it.signed { 's' } else { 'u' };
                let pair = self.r();
                self.emit(format!(
                    "{pair} = call {{ {i}, i1 }} @llvm.{s}{name}.with.overflow.{i}({i} {x}, {i} {y})"
                ));
                self.gen.intrinsic_decl(&format!(
                    "declare {{ {i}, i1 }} @llvm.{s}{name}.with.overflow.{i}({i}, {i})"
                ));
                let v = self.r();
                self.emit(format!("{v} = extractvalue {{ {i}, i1 }} {pair}, 0"));
                let ov = self.r();
                self.emit(format!("{ov} = extractvalue {{ {i}, i1 }} {pair}, 1"));
                self.overflow_guard(&ov, it, sym, x, y, false, span);
                v
            }
            BinOp::Div | BinOp::Rem => {
                let zero = self.r();
                self.emit(format!("{zero} = icmp eq {i} {y}, 0"));
                let loc = self.loc(span);
                self.guard(&zero, |fx| fx.emit(format!("call void @ante_trap_code(i32 2, ptr {loc})")));
                if it.signed {
                    let (min, _) = it.range();
                    let a = self.r();
                    self.emit(format!("{a} = icmp eq {i} {x}, {min}"));
                    let b = self.r();
                    self.emit(format!("{b} = icmp eq {i} {y}, -1"));
                    let both = self.r();
                    self.emit(format!("{both} = and i1 {a}, {b}"));
                    let sym = if op == BinOp::Div { "/" } else { "%" };
                    self.overflow_guard(&both, it, sym, x, y, false, span);
                }
                let r = self.r();
                let ins = match (op, it.signed) {
                    (BinOp::Div, true) => "sdiv",
                    (BinOp::Div, false) => "udiv",
                    (_, true) => "srem",
                    (_, false) => "urem",
                };
                self.emit(format!("{r} = {ins} {i} {x}, {y}"));
                r
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                let ins = match op {
                    BinOp::BitAnd => "and",
                    BinOp::BitOr => "or",
                    _ => "xor",
                };
                let r = self.r();
                self.emit(format!("{r} = {ins} {i} {x}, {y}"));
                r
            }
            _ => {
                let r = self.r();
                self.emit(format!("{r} = icmp {} {i} {x}, {y}", icmp(op, !it.signed)));
                r
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn overflow_guard(
        &mut self,
        cond: &str,
        it: IntTy,
        sym: &str,
        x: &str,
        y: &str,
        unary: bool,
        span: Span,
    ) {
        let loc = self.loc(span);
        let tyname = self.gen.cstr(&crate::interp::int_name(it));
        let opname = self.gen.cstr(sym);
        let (x, y) = (x.to_string(), y.to_string());
        self.guard(cond, |fx| {
            let a = fx.widen(&x, it);
            let b = if unary { "0".to_string() } else { fx.widen(&y, it) };
            fx.emit(format!(
                "call void @ante_trap_overflow(ptr {tyname}, ptr {opname}, i64 {a}, i64 {b}, i32 {}, i32 {}, ptr {loc})",
                i32::from(unary),
                i32::from(!it.signed)
            ));
        });
    }

    fn shift(&mut self, op: BinOp, it: IntTy, x: &str, y: &str, yt: IntTy, span: Span) -> String {
        let c64 = self.widen(y, yt);
        let bad = self.r();
        self.emit(format!("{bad} = icmp uge i64 {c64}, {}", it.bits));
        let loc = self.loc(span);
        let c2 = c64.clone();
        let bits = it.bits;
        self.guard(&bad, |fx| {
            fx.emit(format!("call void @ante_trap_shift(i64 {c2}, i32 {bits}, ptr {loc})"))
        });
        let c = self.narrow(&c64, it);
        let i = format!("i{}", it.bits);
        let r = self.r();
        let ins = match (op, it.signed) {
            (BinOp::Shl, _) => "shl",
            (_, true) => "ashr",
            (_, false) => "lshr",
        };
        self.emit(format!("{r} = {ins} {i} {x}, {c}"));
        r
    }

    fn unary(&mut self, op: UnOp, t: OpTy, a: &Operand, span: Span) -> String {
        match (op, t) {
            (UnOp::Neg, OpTy::Int(it)) => {
                let x = self.operand(a, &Ty::Int(it));
                let i = format!("i{}", it.bits);
                let pair = self.r();
                self.emit(format!(
                    "{pair} = call {{ {i}, i1 }} @llvm.ssub.with.overflow.{i}({i} 0, {i} {x})"
                ));
                self.gen
                    .intrinsic_decl(&format!("declare {{ {i}, i1 }} @llvm.ssub.with.overflow.{i}({i}, {i})"));
                let v = self.r();
                self.emit(format!("{v} = extractvalue {{ {i}, i1 }} {pair}, 0"));
                let ov = self.r();
                self.emit(format!("{ov} = extractvalue {{ {i}, i1 }} {pair}, 1"));
                self.overflow_guard(&ov, it, "-", &x, "0", true, span);
                v
            }
            (UnOp::Neg, OpTy::Float(bits)) => {
                let x = self.operand(a, &Ty::Float(bits));
                let r = self.r();
                self.emit(format!("{r} = fneg {} {x}", if bits == 32 { "float" } else { "double" }));
                r
            }
            (UnOp::BitNot, OpTy::Int(it)) => {
                let x = self.operand(a, &Ty::Int(it));
                let r = self.r();
                self.emit(format!("{r} = xor i{} {x}, -1", it.bits));
                r
            }
            _ => {
                let x = self.operand(a, &Ty::Bool);
                let r = self.r();
                self.emit(format!("{r} = xor i1 {x}, true"));
                r
            }
        }
    }

    // ── terminators and calls ──────────────────────────────────────────────

    fn term(&mut self, t: &Term) {
        match t {
            Term::Goto(b) => self.emit(format!("br label %bb{b}")),
            Term::Switch { discr, targets, otherwise } => {
                let dt = self.operand_ty(discr).unwrap_or(Ty::ANKAM);
                let v = self.operand(discr, &dt);
                let (v, lt) = match &dt {
                    Ty::Bool => {
                        let r = self.r();
                        self.emit(format!("{r} = zext i1 {v} to i64"));
                        (r, "i64".to_string())
                    }
                    other => {
                        let lt = self.ll(other);
                        (v, lt)
                    }
                };
                let cases: Vec<String> =
                    targets.iter().map(|(k, b)| format!("{lt} {k}, label %bb{b}")).collect();
                self.emit(format!("switch {lt} {v}, label %bb{otherwise} [ {} ]", cases.join(" ")));
            }
            Term::Return => {
                let rt = self.locals[0].clone();
                let lt = self.ll(&rt);
                let v = self.load(&lt, "%l0");
                self.emit(format!("ret {lt} {v}"));
            }
            Term::Unreachable => {
                let loc = self.loc(Span::default());
                self.emit(format!("call void @ante_trap_code(i32 99, ptr {loc})"));
                self.emit("unreachable");
            }
            Term::Call { callee, args, dest, next, span } => {
                let dest_ty = self.place_ty(dest);
                let (v, vt) = self.call(callee, args, &dest_ty, *span);
                if vt == Ty::Never {
                    self.emit("unreachable");
                    return;
                }
                let v = self.coerce(&v, &vt, &dest_ty);
                let pr = self.place(dest);
                let lt = self.ll(&pr.ty);
                self.emit(format!("store {lt} {v}, ptr {}", pr.ptr));
                if dest.proj.is_empty() {
                    if let Some(f) = self.flags.get(&dest.local).cloned() {
                        self.emit(format!("store i1 true, ptr {f}"));
                    }
                }
                self.emit(format!("br label %bb{next}"));
            }
        }
    }

    /// Emits a call; returns the result and its type.
    fn call(&mut self, callee: &Callee, args: &[Operand], dest_ty: &Ty, span: Span) -> (String, Ty) {
        match callee {
            Callee::Fn(fid, tys) => {
                let tys: Vec<Ty> = tys.iter().map(|t| self.subst(t)).collect();
                if let Some(&body) = self.gen.prog.fn_bodies.get(fid) {
                    return self.call_body(body, &tys, args);
                }
                let origin = self.gen.g.fns[*fid].origin;
                if matches!(origin, FnOrigin::Std | FnOrigin::StdUnsafe) {
                    return super::native::std_call(self, *fid, &tys, args, dest_ty, span);
                }
                if crate::mir::trait_method_of(self.gen.g, *fid).is_some() {
                    return self.call_trait(*fid, &tys, args, dest_ty, span);
                }
                let name = self.gen.g.fns[*fid].name.clone();
                self.gen.error(format!("`{name}` (bayata \"C\") native backend lo inka ledhu (M11)"));
                ("undef".into(), dest_ty.clone())
            }
            Callee::Trait(fid, tys) => {
                let tys: Vec<Ty> = tys.iter().map(|t| self.subst(t)).collect();
                self.call_trait(*fid, &tys, args, dest_ty, span)
            }
            Callee::Value(op) => {
                let fty = self.operand_ty(op).unwrap_or(Ty::Error);
                let Ty::Fn(params, ret) = fty.clone() else {
                    self.gen.error("native backend: call of a non-function");
                    return ("undef".into(), dest_ty.clone());
                };
                let f = self.operand(op, &fty);
                let fp = self.r();
                self.emit(format!("{fp} = extractvalue %pani {f}, 0"));
                let env = self.r();
                self.emit(format!("{env} = extractvalue %pani {f}, 1"));
                let mut parts = vec![format!("ptr {env}")];
                for (o, (t, _)) in args.iter().zip(&params) {
                    let v = self.operand(o, t);
                    let lt = self.ll(t);
                    parts.push(format!("{lt} {v}"));
                }
                let rt = self.ll(&ret);
                let r = self.r();
                self.emit(format!("{r} = call {rt} {fp}({})", parts.join(", ")));
                (r, *ret)
            }
            Callee::Intrinsic(i) => super::native::intrinsic(self, i, args, dest_ty, span),
        }
    }

    pub fn call_trait(
        &mut self,
        fid: crate::typeck::FnId,
        tys: &[Ty],
        args: &[Operand],
        dest_ty: &Ty,
        span: Span,
    ) -> (String, Ty) {
        match self.gen.resolve(fid, tys) {
            Some(Resolved::Body(body, btys)) => self.call_body(body, &btys, args),
            Some(Resolved::Native(name)) => super::native::trait_call(self, &name, tys, args, dest_ty, span),
            None => {
                self.gen.error("native backend: unknown trait method");
                ("undef".into(), dest_ty.clone())
            }
        }
    }

    /// Parameter types and return type of a body instance.
    pub fn body_sig(&mut self, body: BodyId, tys: &[Ty]) -> (Vec<Ty>, Ty) {
        let b = &self.gen.prog.bodies[body as usize];
        let sub = |t: &Ty| if tys.is_empty() { t.clone() } else { t.subst(tys) };
        let params = (1..=b.params as usize).map(|i| sub(&b.locals[i].ty)).collect();
        (params, sub(&b.locals[0].ty))
    }

    pub fn call_body(&mut self, body: BodyId, tys: &[Ty], args: &[Operand]) -> (String, Ty) {
        let (params, ret) = self.body_sig(body, tys);
        let f = self.gen.item(Item::Body(body, tys.to_vec()));
        let mut parts = Vec::new();
        for (o, t) in args.iter().zip(&params) {
            let v = self.operand(o, t);
            let lt = self.ll(t);
            parts.push(format!("{lt} {v}"));
        }
        let rt = self.ll(&ret);
        let r = self.r();
        self.emit(format!("{r} = call {rt} {f}({})", parts.join(", ")));
        (r, ret)
    }

    /// Calls a body instance with values already computed.
    pub fn call_body_vals(&mut self, body: BodyId, tys: &[Ty], vals: &[String]) -> (String, Ty) {
        let (params, ret) = self.body_sig(body, tys);
        let f = self.gen.item(Item::Body(body, tys.to_vec()));
        let mut parts = Vec::new();
        for (v, t) in vals.iter().zip(&params) {
            let lt = self.ll(t);
            parts.push(format!("{lt} {v}"));
        }
        let rt = self.ll(&ret);
        let r = self.r();
        self.emit(format!("{r} = call {rt} {f}({})", parts.join(", ")));
        (r, ret)
    }
}

fn icmp(op: BinOp, unsigned: bool) -> &'static str {
    match (op, unsigned) {
        (BinOp::Eq, _) => "eq",
        (BinOp::Ne, _) => "ne",
        (BinOp::Lt, false) => "slt",
        (BinOp::Le, false) => "sle",
        (BinOp::Gt, false) => "sgt",
        (BinOp::Ge, false) => "sge",
        (BinOp::Lt, true) => "ult",
        (BinOp::Le, true) => "ule",
        (BinOp::Gt, true) => "ugt",
        _ => "uge",
    }
}

fn fcmp(op: BinOp) -> &'static str {
    match op {
        BinOp::Eq => "oeq",
        BinOp::Ne => "une",
        BinOp::Lt => "olt",
        BinOp::Le => "ole",
        BinOp::Gt => "ogt",
        _ => "oge",
    }
}
