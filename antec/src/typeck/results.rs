//! What the type checker decided, for MIR lowering: the type of every
//! expression, what every path and call refers to, how phrases bind to
//! formals (§11.2), automatic borrows (§11.3) and pattern binding modes (§14.2).
//!
//! Expressions are identified by their file and span (`ExprId`); the parser
//! gives every expression node a distinct span.

use std::collections::HashMap;

use super::body::Conv;
use super::{FnId, RecvMode};
use crate::ast::Expr;
use crate::resolve::{DefId, FileId};
use crate::source::Span;
use crate::types::{InferTable, Ty};

pub type ExprId = (FileId, usize, usize);

pub fn expr_id(file: FileId, span: Span) -> ExprId {
    (file, span.start, span.end)
}

/// One field access after `derefs` automatic dereferences (`&`, `pette`).
#[derive(Clone, Debug, PartialEq)]
pub struct FieldStep {
    pub derefs: u8,
    pub index: u32,
}

#[derive(Clone, Debug)]
pub enum PathRes {
    /// A local by its declaration offset, then fields.
    Local {
        decl: usize,
        steps: Vec<FieldStep>,
    },
    /// `naa` (or a pro-dropped field of it), then fields.
    Naa {
        steps: Vec<FieldStep>,
    },
    /// A function used as a value.
    Fn {
        fid: FnId,
        tys: Vec<Ty>,
    },
    Const {
        def: DefId,
        steps: Vec<FieldStep>,
    },
    /// A variant without fields: `Rangu.Erupu`, `Kramam.Thakkuva`.
    Variant {
        adt: DefId,
        variant: u32,
    },
    /// A member of an open module (`torch`, `python`).
    Opaque,
}

/// The enum a constructor builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdtRef {
    Undochu,
    SareThappu,
    Adt(DefId),
}

#[derive(Clone, Debug)]
pub enum Target {
    /// A function or method; `tys` instantiates all of its generics.
    Fn { fid: FnId, tys: Vec<Ty> },
    /// A trait method: `tys` is `[idhi, trait args…, own generics…]`.
    Trait { fid: FnId, tys: Vec<Ty> },
    /// A `pani` value: the callee expression itself.
    Value,
    /// A member of an open module.
    Opaque,
}

#[derive(Clone, Debug)]
pub enum ArgSrc {
    Expr(ExprId),
    /// The value a pipe passes in (§12).
    Injected,
    /// The receiver of an adapter called as a method: `xs.varusa_ga()`.
    Pre,
    /// Element `index` of a fixed-length spread, or (`None`) every element
    /// of a runtime-length one.
    Spread {
        expr: ExprId,
        index: Option<u32>,
    },
}

#[derive(Clone, Debug)]
pub struct ArgRef {
    pub src: ArgSrc,
    /// Take an automatic shared borrow (§11.3).
    pub borrow: bool,
    /// The formal is an iterable (`Pravaham`): convert the value to an iterator.
    pub iter: Option<Ty>,
}

#[derive(Clone, Debug)]
pub enum ArgPlan {
    One(ArgRef),
    /// The formal's default value.
    Default,
    /// The `migathavi` collector, in source order.
    Many(Vec<ArgRef>),
    /// The `migatha_perlu` collector.
    Named(Vec<(String, ArgRef)>),
    /// Already reported.
    Missing,
}

#[derive(Clone, Debug)]
pub struct RecvPlan {
    /// The receiver expression (for a method path `a.b.m`, the path `a.b`).
    pub expr: Expr,
    pub derefs: u8,
    pub mode: RecvMode,
}

#[derive(Clone, Debug)]
pub struct CallPlan {
    pub target: Target,
    pub recv: Option<RecvPlan>,
    /// One entry per formal, in declaration order.
    pub args: Vec<ArgPlan>,
    pub ret: Ty,
    /// `maarchu` and friends: conversion kind, source and target types.
    pub conv: Option<(Conv, Ty, Ty)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindMode {
    Move,
    Copy,
    Ref(bool),
}

#[derive(Default)]
pub struct Results {
    pub expr_ty: HashMap<ExprId, Ty>,
    /// Locals by file and declaration offset.
    pub local_ty: HashMap<(FileId, usize), (Ty, bool)>,
    pub paths: HashMap<ExprId, PathRes>,
    /// `e.f` and `e.0`.
    pub fields: HashMap<ExprId, FieldStep>,
    /// Automatic dereferences of an indexed base.
    pub index_derefs: HashMap<ExprId, u8>,
    pub calls: HashMap<ExprId, CallPlan>,
    pub ctors: HashMap<ExprId, (AdtRef, u32)>,
    pub records: HashMap<ExprId, DefId>,
    /// `for` loops: the item type, keyed by the iterable expression.
    pub for_items: HashMap<ExprId, Ty>,
    /// The type each pattern node matches (before peeling references).
    pub pat_ty: HashMap<ExprId, Ty>,
    pub pat_ctors: HashMap<ExprId, (AdtRef, u32)>,
    /// Binding modes by file and identifier offset.
    pub binds: HashMap<(FileId, usize), BindMode>,
}

impl Results {
    /// Moves already-resolved results in.
    pub fn merge(&mut self, other: Results) {
        self.expr_ty.extend(other.expr_ty);
        self.local_ty.extend(other.local_ty);
        self.paths.extend(other.paths);
        self.fields.extend(other.fields);
        self.index_derefs.extend(other.index_derefs);
        self.calls.extend(other.calls);
        self.ctors.extend(other.ctors);
        self.records.extend(other.records);
        self.for_items.extend(other.for_items);
        self.pat_ty.extend(other.pat_ty);
        self.pat_ctors.extend(other.pat_ctors);
        self.binds.extend(other.binds);
    }

    /// Moves `other` in, resolving every type through `t`.
    pub fn absorb(&mut self, other: Results, t: &mut InferTable) {
        let mut r = |ty: &Ty| t.resolve(ty);
        for (k, v) in other.expr_ty {
            self.expr_ty.insert(k, r(&v));
        }
        for (k, (v, m)) in other.local_ty {
            self.local_ty.insert(k, (r(&v), m));
        }
        for (k, v) in other.paths {
            let v = match v {
                PathRes::Fn { fid, tys } => PathRes::Fn { fid, tys: tys.iter().map(&mut r).collect() },
                other => other,
            };
            self.paths.insert(k, v);
        }
        for (k, mut v) in other.calls {
            v.target = match v.target {
                Target::Fn { fid, tys } => Target::Fn { fid, tys: tys.iter().map(&mut r).collect() },
                Target::Trait { fid, tys } => Target::Trait { fid, tys: tys.iter().map(&mut r).collect() },
                other => other,
            };
            v.ret = r(&v.ret);
            v.conv = v.conv.map(|(k, a, b)| (k, r(&a), r(&b)));
            for a in &mut v.args {
                let fix = |x: &mut ArgRef, r: &mut dyn FnMut(&Ty) -> Ty| {
                    x.iter = x.iter.as_ref().map(&mut *r);
                };
                match a {
                    ArgPlan::One(x) => fix(x, &mut r),
                    ArgPlan::Many(xs) => xs.iter_mut().for_each(|x| fix(x, &mut r)),
                    ArgPlan::Named(xs) => xs.iter_mut().for_each(|(_, x)| fix(x, &mut r)),
                    _ => {}
                }
            }
            self.calls.insert(k, v);
        }
        for (k, v) in other.for_items {
            self.for_items.insert(k, r(&v));
        }
        for (k, v) in other.pat_ty {
            self.pat_ty.insert(k, r(&v));
        }
        self.fields.extend(other.fields);
        self.index_derefs.extend(other.index_derefs);
        self.ctors.extend(other.ctors);
        self.records.extend(other.records);
        self.pat_ctors.extend(other.pat_ctors);
        self.binds.extend(other.binds);
    }
}
