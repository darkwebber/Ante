//! Semantic types (spec §9) and the inference table the type checker uses.

use std::fmt;

use crate::ast::Role;
use crate::resolve::DefId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntTy {
    pub signed: bool,
    pub bits: u8,
}

impl IntTy {
    pub const ANKAM: IntTy = IntTy { signed: true, bits: 64 };

    /// The smallest and largest value of the type.
    pub fn range(self) -> (i128, i128) {
        let bits = u32::from(self.bits);
        if self.signed {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        }
    }
}

/// Type constructors that take type arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Con {
    Varusa,
    Mukka,
    Jodi,
    GajibijiJodi,
    Gumpu,
    Undochu,
    SareThappu,
    Pette,
    /// `panchukunna[T]`: a shared, counted handle (§24.4).
    Panchukunna,
    /// `Paridhi[I]`, an integer range (§21.4).
    Paridhi,
    /// A lazy iterator yielding `I` (the adapters of §21.4.2). As a formal
    /// type of a standard function, it accepts anything iterable.
    Pravaham,
    /// A user or standard record or enum.
    Adt(DefId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Int(IntTy),
    Float(u8),
    Bool,
    Char,
    Unit,
    Never,
    Str,
    Tuple(Vec<Ty>),
    /// `[T; N]`; `None` when the length isn't a known constant.
    Array(Box<Ty>, Option<u64>),
    App(Con, Vec<Ty>),
    Ref(bool, Box<Ty>),
    Ptr(bool, Box<Ty>),
    Fn(Vec<(Ty, Option<Role>)>, Box<Ty>),
    /// A generic parameter, by its index in the enclosing generic list.
    Param(u32),
    Var(u32),
    /// Something already reported, or a member of an open module (`torch`,
    /// `python`). Unifies with everything so one mistake reports once.
    Error,
}

impl Ty {
    pub const ANKAM: Ty = Ty::Int(IntTy::ANKAM);

    pub fn app(con: Con, args: Vec<Ty>) -> Ty {
        Ty::App(con, args)
    }

    pub fn reference(mutable: bool, inner: Ty) -> Ty {
        Ty::Ref(mutable, Box::new(inner))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int(_) | Ty::Float(_))
    }

    /// Whether any part of the type is the error type.
    pub fn has_error(&self) -> bool {
        let mut found = false;
        self.walk(&mut |t| found |= t.is_error());
        found
    }

    /// Calls `f` on this type and every type inside it.
    pub fn walk(&self, f: &mut impl FnMut(&Ty)) {
        f(self);
        match self {
            Ty::Tuple(items) | Ty::App(_, items) => items.iter().for_each(|t| t.walk(f)),
            Ty::Array(t, _) | Ty::Ref(_, t) | Ty::Ptr(_, t) => t.walk(f),
            Ty::Fn(params, ret) => {
                params.iter().for_each(|(t, _)| t.walk(f));
                ret.walk(f);
            }
            _ => {}
        }
    }

    /// Replaces every `Param(i)` with `args[i]`.
    pub fn subst(&self, args: &[Ty]) -> Ty {
        if args.is_empty() {
            return self.clone();
        }
        self.map(&|t| match t {
            Ty::Param(i) => args.get(*i as usize).cloned(),
            _ => None,
        })
    }

    /// Rebuilds the type bottom-up, replacing any node for which `f` returns `Some`.
    pub fn map(&self, f: &impl Fn(&Ty) -> Option<Ty>) -> Ty {
        if let Some(t) = f(self) {
            return t;
        }
        match self {
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|t| t.map(f)).collect()),
            Ty::App(c, items) => Ty::App(*c, items.iter().map(|t| t.map(f)).collect()),
            Ty::Array(t, n) => Ty::Array(Box::new(t.map(f)), *n),
            Ty::Ref(m, t) => Ty::Ref(*m, Box::new(t.map(f))),
            Ty::Ptr(m, t) => Ty::Ptr(*m, Box::new(t.map(f))),
            Ty::Fn(params, ret) => {
                Ty::Fn(params.iter().map(|(t, r)| (t.map(f), *r)).collect(), Box::new(ret.map(f)))
            }
            other => other.clone(),
        }
    }
}

/// How a type is shown in messages. ADT names come from `names`.
pub struct Display<'a> {
    pub ty: &'a Ty,
    pub adt_name: &'a dyn Fn(DefId) -> String,
    pub param_name: &'a dyn Fn(u32) -> String,
    pub var_kind: &'a dyn Fn(u32) -> VarKind,
}

impl fmt::Display for Display<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |f: &mut fmt::Formatter<'_>, items: &[Ty]| -> fmt::Result {
            for (i, t) in items.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", with_ty(self, t))?;
            }
            Ok(())
        };
        match self.ty {
            Ty::Int(IntTy { signed, bits }) => {
                write!(f, "{}ankam{bits}", if *signed { "" } else { "aasajeevi " })
            }
            Ty::Float(bits) => write!(f, "amsam{bits}"),
            Ty::Bool => write!(f, "ato_ito"),
            Ty::Char => write!(f, "aksharam"),
            Ty::Unit => write!(f, "khali"),
            Ty::Never => write!(f, "raadhu"),
            Ty::Str => write!(f, "padham"),
            Ty::Tuple(items) => {
                write!(f, "jatha[")?;
                list(f, items)?;
                write!(f, "]")
            }
            Ty::Array(t, Some(n)) => write!(f, "[{}; {n}]", with_ty(self, t)),
            Ty::Array(t, None) => write!(f, "[{}; N]", with_ty(self, t)),
            Ty::App(con, args) => {
                let name = match con {
                    Con::Varusa => "varusa".to_string(),
                    Con::Mukka => "mukka".to_string(),
                    Con::Jodi => "jodi".to_string(),
                    Con::GajibijiJodi => "gajibiji_jodi".to_string(),
                    Con::Gumpu => "gumpu".to_string(),
                    Con::Undochu => "undochu".to_string(),
                    Con::SareThappu => "sare_thappu".to_string(),
                    Con::Pette => "pette".to_string(),
                    Con::Panchukunna => "panchukunna".to_string(),
                    Con::Paridhi => "Paridhi".to_string(),
                    Con::Pravaham => "Pravaham".to_string(),
                    Con::Adt(d) => (self.adt_name)(*d),
                };
                write!(f, "{name}")?;
                if !args.is_empty() {
                    write!(f, "[")?;
                    list(f, args)?;
                    write!(f, "]")?;
                }
                Ok(())
            }
            Ty::Ref(m, t) => write!(f, "&{}{}", if *m { "maare " } else { "" }, with_ty(self, t)),
            Ty::Ptr(m, t) => write!(f, "*{}{}", if *m { "maare " } else { "" }, with_ty(self, t)),
            Ty::Fn(params, ret) => {
                write!(f, "pani[(")?;
                for (i, (t, role)) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", with_ty(self, t))?;
                    if let Some(r) = role {
                        write!(f, " {}", r.as_str())?;
                    }
                }
                write!(f, ") -> {}]", with_ty(self, ret))
            }
            Ty::Param(i) => write!(f, "{}", (self.param_name)(*i)),
            Ty::Var(v) => match (self.var_kind)(*v) {
                VarKind::Int => write!(f, "ankam"),
                VarKind::Float => write!(f, "amsam"),
                VarKind::General => write!(f, "_"),
            },
            Ty::Error => write!(f, "_"),
        }
    }
}

fn with_ty<'b>(d: &'b Display<'_>, ty: &'b Ty) -> Display<'b> {
    Display { ty, adt_name: d.adt_name, param_name: d.param_name, var_kind: d.var_kind }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    General,
    /// An integer literal whose type isn't decided yet; defaults to `ankam64`.
    Int,
    /// A float literal; defaults to `amsam64`.
    Float,
}

/// Union-find over inference variables (§10.1).
///
/// Speculative unification (trying a trait implementation, say) runs inside
/// a snapshot: changes are logged and `rollback_to` undoes them, so a failed
/// attempt costs only what it changed.
#[derive(Default)]
pub struct InferTable {
    parent: Vec<u32>,
    kind: Vec<VarKind>,
    value: Vec<Option<Ty>>,
    undo: Vec<Undo>,
    open_snapshots: usize,
}

enum Undo {
    Parent(u32, u32),
    Kind(u32, VarKind),
    Value(u32),
}

/// A point `InferTable::rollback_to` can return to.
#[must_use]
pub struct Snapshot {
    undo_len: usize,
    vars: usize,
}

impl InferTable {
    pub fn fresh(&mut self, kind: VarKind) -> Ty {
        let v = self.parent.len() as u32;
        self.parent.push(v);
        self.kind.push(kind);
        self.value.push(None);
        Ty::Var(v)
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.open_snapshots += 1;
        Snapshot { undo_len: self.undo.len(), vars: self.parent.len() }
    }

    /// Undoes every change made since `s`.
    pub fn rollback_to(&mut self, s: Snapshot) {
        while self.undo.len() > s.undo_len {
            match self.undo.pop().expect("non-empty log") {
                Undo::Parent(v, old) => self.parent[v as usize] = old,
                Undo::Kind(v, old) => self.kind[v as usize] = old,
                Undo::Value(v) => self.value[v as usize] = None,
            }
        }
        self.parent.truncate(s.vars);
        self.kind.truncate(s.vars);
        self.value.truncate(s.vars);
        self.end_snapshot();
    }

    /// Keeps every change made since `s`.
    pub fn commit(&mut self, _s: Snapshot) {
        self.end_snapshot();
    }

    fn end_snapshot(&mut self) {
        self.open_snapshots -= 1;
        if self.open_snapshots == 0 {
            self.undo.clear();
        }
    }

    pub fn find(&mut self, v: u32) -> u32 {
        // A variable from another table (a type left undecided by inference)
        // is unbound here.
        if v as usize >= self.parent.len() {
            return v;
        }
        let mut root = v;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        if self.open_snapshots > 0 {
            return root;
        }
        let mut cur = v;
        while self.parent[cur as usize] != root {
            let next = self.parent[cur as usize];
            self.parent[cur as usize] = root;
            cur = next;
        }
        root
    }

    pub fn kind(&mut self, v: u32) -> VarKind {
        let r = self.find(v);
        self.kind.get(r as usize).copied().unwrap_or(VarKind::General)
    }

    pub fn probe(&mut self, v: u32) -> Option<Ty> {
        let r = self.find(v);
        self.value.get(r as usize).cloned().flatten()
    }

    /// Follows bound variables at the top of `ty` only.
    pub fn shallow(&mut self, ty: &Ty) -> Ty {
        let mut ty = ty.clone();
        while let Ty::Var(v) = ty {
            match self.probe(v) {
                Some(t) => ty = t,
                None => return Ty::Var(self.find(v)),
            }
        }
        ty
    }

    /// Replaces every bound variable inside `ty` with its value.
    pub fn resolve(&mut self, ty: &Ty) -> Ty {
        match self.shallow(ty) {
            Ty::Tuple(items) => Ty::Tuple(items.iter().map(|t| self.resolve(t)).collect()),
            Ty::App(c, items) => Ty::App(c, items.iter().map(|t| self.resolve(t)).collect()),
            Ty::Array(t, n) => Ty::Array(Box::new(self.resolve(&t)), n),
            Ty::Ref(m, t) => Ty::Ref(m, Box::new(self.resolve(&t))),
            Ty::Ptr(m, t) => Ty::Ptr(m, Box::new(self.resolve(&t))),
            Ty::Fn(params, ret) => Ty::Fn(
                params.iter().map(|(t, r)| (self.resolve(t), *r)).collect(),
                Box::new(self.resolve(&ret)),
            ),
            other => other,
        }
    }

    pub fn occurs(&mut self, v: u32, ty: &Ty) -> bool {
        let ty = self.resolve(ty);
        let mut found = false;
        ty.walk(&mut |t| found |= *t == Ty::Var(v));
        found
    }

    /// Binds an unbound root variable. The caller has checked compatibility.
    pub fn bind(&mut self, v: u32, ty: Ty) {
        let r = self.find(v);
        if r as usize >= self.value.len() {
            return;
        }
        if self.open_snapshots > 0 {
            self.undo.push(Undo::Value(r));
        }
        self.value[r as usize] = Some(ty);
    }

    /// Joins two unbound root variables, keeping the more specific kind.
    pub fn union(&mut self, a: u32, b: u32) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return true;
        }
        if ra as usize >= self.parent.len() || rb as usize >= self.parent.len() {
            return false;
        }
        let (ka, kb) = (self.kind[ra as usize], self.kind[rb as usize]);
        let kind = match (ka, kb) {
            (VarKind::General, k) | (k, VarKind::General) => k,
            (x, y) if x == y => x,
            _ => return false,
        };
        if self.open_snapshots > 0 {
            self.undo.push(Undo::Parent(ra, self.parent[ra as usize]));
            self.undo.push(Undo::Kind(rb, self.kind[rb as usize]));
        }
        self.parent[ra as usize] = rb;
        self.kind[rb as usize] = kind;
        true
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }
}
