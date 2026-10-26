//! The standard library at run time (spec §21), built-in trait behavior
//! (§13.5–13.6), display, and iterators (§21.4).

use std::rc::Rc;

use super::value::{key_of, Addr, Elem, IterState};
use super::{int_name, wrap, Halt, Machine, Value, R};
use crate::mir::Intrinsic;
use crate::source::Span;
use crate::typeck::{AdtKind, TraitRef};
use crate::types::{Con, InferTable, IntTy, Ty};

/// Standard library constants that have no "Ante" body (`lekkalu.PI`, …).
pub fn std_const(name: &str) -> Value {
    match name {
        "PI" => Value::F64(std::f64::consts::PI),
        "E" => Value::F64(std::f64::consts::E),
        "INF" => Value::F64(f64::INFINITY),
        "NAN" => Value::F64(f64::NAN),
        _ => Value::Unit,
    }
}

pub fn fmt_f64(x: f64) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        }
    } else if x == x.trunc() && x.abs() < 1e16 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

pub fn fmt_f32(x: f32) -> String {
    if x.is_nan() {
        "NaN".into()
    } else if x.is_infinite() {
        if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        }
    } else if x == x.trunc() && x.abs() < 1e7 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

fn kramam(o: std::cmp::Ordering) -> Value {
    Value::Adt(
        match o {
            std::cmp::Ordering::Less => 0,
            std::cmp::Ordering::Equal => 1,
            std::cmp::Ordering::Greater => 2,
        },
        Vec::new(),
    )
}

fn thappu_record(msg: impl Into<String>) -> Value {
    Value::Adt(0, vec![Value::Str(Rc::from(msg.into()))])
}

fn bool_v(b: bool) -> Value {
    Value::Bool(b)
}

impl Machine<'_> {
    // ── reading arguments ─────────────────────────────────────────────────

    /// Follows references to the value they point at.
    fn target<'s>(&'s self, v: &'s Value) -> &'s Value {
        let mut v = v;
        while let Value::Ref(a) = v {
            v = self.read(a);
        }
        v
    }

    fn arg_str(&self, v: &Value) -> Rc<str> {
        self.target(v).str()
    }

    fn ref_addr(&self, v: &Value) -> Addr {
        match v {
            Value::Ref(a) => match self.read(a) {
                Value::Ref(inner) => inner.clone(),
                _ => a.clone(),
            },
            Value::Slice(a, _, _) => a.clone(),
            _ => self.heap_addr_unused(),
        }
    }

    fn heap_addr_unused(&self) -> Addr {
        Addr { root: super::value::Root::Heap(usize::MAX), path: Vec::new() }
    }

    /// The elements of a sequence argument (owned, referenced or sliced).
    fn seq(&self, v: &Value) -> Vec<Value> {
        match v {
            Value::Slice(a, s, n) => match self.read(a) {
                Value::Vec(xs) | Value::Array(xs) => xs[*s..*s + *n].to_vec(),
                _ => Vec::new(),
            },
            Value::Ref(a) => self.seq(&self.read(a).clone()),
            Value::Vec(xs) | Value::Array(xs) | Value::Tuple(xs) => xs.clone(),
            _ => Vec::new(),
        }
    }

    fn seq_len_of(&self, v: &Value) -> usize {
        match v {
            Value::Slice(_, _, n) => *n,
            Value::Ref(a) => match self.read(a) {
                Value::Vec(xs) | Value::Array(xs) => xs.len(),
                Value::Map(m) => m.len(),
                Value::Ref(_) | Value::Slice(..) => self.seq_len_of(&self.read(a).clone()),
                _ => 0,
            },
            Value::Vec(xs) | Value::Array(xs) => xs.len(),
            _ => 0,
        }
    }

    /// The address of element `i` of a sequence argument, if in range.
    fn elem_addr(&self, v: &Value, i: i128) -> Option<Addr> {
        let (base, start, len) = match v {
            Value::Slice(a, s, n) => (a.clone(), *s, *n),
            Value::Ref(a) => match self.read(a) {
                Value::Slice(b, s, n) => (b.clone(), *s, *n),
                Value::Ref(b) => return self.elem_addr(&Value::Ref(b.clone()), i),
                other => (a.clone(), 0, other.seq_len().unwrap_or(0)),
            },
            _ => return None,
        };
        (i >= 0 && (i as usize) < len).then(|| base.with(Elem::Index(start + i as usize)))
    }

    fn with_target_mut<T>(&mut self, v: &Value, f: impl FnOnce(&mut Value) -> T) -> T {
        let a = match v {
            Value::Ref(a) => a.clone(),
            _ => self.heap_addr_unused(),
        };
        let a = match self.read(&a) {
            Value::Ref(inner) => inner.clone(),
            _ => a,
        };
        f(self.read_mut(&a))
    }

    /// Puts a value in a scratch heap slot and returns a reference to it.
    fn scratch(&mut self, v: Value) -> Value {
        Value::Ref(self.alloc(v))
    }

    fn release(&mut self, mark: usize) {
        self.heap.truncate(mark);
    }

    // ── intrinsics ─────────────────────────────────────────────────────────

    pub(super) fn intrinsic(&mut self, i: &Intrinsic, args: Vec<Value>, span: Span) -> R<Value> {
        match i {
            Intrinsic::IntoIter { copy, ty } => {
                let ty = self.subst(ty);
                let v = args.into_iter().next().unwrap_or(Value::Unit);
                let st = self.make_iter(v, *copy, &ty, span)?;
                Ok(Value::Iter(Box::new(st)))
            }
            Intrinsic::IterNext => {
                let a = self.ref_addr(&args[0]);
                let mut it = match std::mem::replace(self.read_mut(&a), Value::Uninit) {
                    Value::Iter(b) => b,
                    other => {
                        *self.read_mut(&a) = other;
                        return Ok(Value::none());
                    }
                };
                let next = self.iter_next(&mut it, span);
                *self.read_mut(&a) = Value::Iter(it);
                Ok(match next? {
                    Some(v) => Value::some(v),
                    None => Value::none(),
                })
            }
            Intrinsic::Format { texts, tys } => {
                let mut out = String::new();
                for (i, t) in texts.iter().enumerate() {
                    out.push_str(t);
                    if let (Some(v), Some(ty)) = (args.get(i), tys.get(i)) {
                        let ty = self.subst(ty);
                        let v = self.target(v).clone();
                        out.push_str(&self.display(&v, &ty)?);
                    }
                }
                Ok(Value::Str(Rc::from(out)))
            }
            Intrinsic::Collect(flags) => {
                let mut items = Vec::new();
                for (v, splice) in args.into_iter().zip(flags) {
                    if *splice {
                        items.extend(self.seq(&v));
                    } else {
                        items.push(v);
                    }
                }
                Ok(Value::Vec(items))
            }
            Intrinsic::Const(def) => self.const_value(*def, span),
            Intrinsic::Unsupported(what) => Err(self.trap(
                "T05",
                format!("{what} ippatiki interpreter lo pani cheyyadhu (taruvatha milestones lo vasthundhi)"),
                span,
            )),
        }
    }

    // ── iterators (§21.4) ──────────────────────────────────────────────────

    fn make_iter(&mut self, v: Value, copy: bool, ty: &Ty, span: Span) -> R<IterState> {
        Ok(match v {
            Value::Range(lo, hi, inclusive, _) => {
                let last = if inclusive { hi } else { hi - 1 };
                IterState::Range { next: lo, last, done: lo > last }
            }
            Value::Iter(b) => *b,
            Value::Slice(base, start, len) => IterState::Seq { base, start, len, pos: 0, copy },
            Value::Ref(a) => match self.read(&a) {
                Value::Vec(xs) | Value::Array(xs) => {
                    IterState::Seq { base: a.clone(), start: 0, len: xs.len(), pos: 0, copy }
                }
                Value::Map(m) => {
                    let keys_only = matches!(strip_ref(ty), Ty::App(Con::Gumpu, _));
                    IterState::Map { base: a.clone(), positions: m.positions().into(), keys_only }
                }
                Value::Slice(b, s, n) => IterState::Seq { base: b.clone(), start: *s, len: *n, pos: 0, copy },
                Value::Ref(_) => {
                    let inner = self.read(&a).clone();
                    return self.make_iter(inner, copy, ty, span);
                }
                _ => return self.user_iter(Value::Ref(a), ty, span),
            },
            Value::Vec(xs) | Value::Array(xs) => IterState::Owned(xs.into()),
            Value::Map(m) => {
                let keys_only = matches!(ty, Ty::App(Con::Gumpu, _));
                let positions = m.positions().into();
                let base = self.alloc(Value::Map(m));
                IterState::Map { base, positions, keys_only }
            }
            other => return self.user_iter(other, ty, span),
        })
    }

    /// A value of a type implementing `Okkokkati`.
    fn user_iter(&mut self, v: Value, ty: &Ty, span: Span) -> R<IterState> {
        let g = &self.tc.globals;
        let Some(def) = g.std_trait("Okkokkati") else {
            return Err(self.trap("T00", "antec bug: no Okkokkati", span));
        };
        let ty = strip_ref(ty).clone();
        let mut table = InferTable::default();
        let item = table.fresh(crate::types::VarKind::General);
        let solver = crate::typeck::solve::Solver { g, generics: &[] };
        let found = solver.find_impl(&mut table, &ty, &TraitRef { def, args: vec![item.clone()] }, 0);
        let next = g.traits[&def].methods[0].1;
        let item = table.resolve(&item);
        let _ = found;
        let value = match v {
            Value::Ref(a) => self.read(&a).clone(),
            other => other,
        };
        Ok(IterState::User { value: Box::new(value), next, tys: vec![ty, item] })
    }

    fn iter_next(&mut self, it: &mut IterState, span: Span) -> R<Option<Value>> {
        Ok(match it {
            IterState::Range { next, last, done } => {
                if *done {
                    None
                } else {
                    let v = *next;
                    if v == *last {
                        *done = true;
                    } else {
                        *next += 1;
                    }
                    Some(Value::Int(v))
                }
            }
            IterState::Seq { base, start, len, pos, copy } => {
                if *pos >= *len {
                    None
                } else {
                    let a = base.with(Elem::Index(*start + *pos));
                    *pos += 1;
                    Some(if *copy { self.read(&a).clone() } else { Value::Ref(a) })
                }
            }
            IterState::Owned(q) => q.pop_front(),
            IterState::Chars { text, pos } => {
                let c = text[*pos..].chars().next();
                if let Some(c) = c {
                    *pos += c.len_utf8();
                }
                c.map(Value::Char)
            }
            IterState::Map { base, positions, keys_only } => match positions.pop_front() {
                None => None,
                Some(i) => {
                    let k = Value::Ref(base.with(Elem::MapKey(i)));
                    if *keys_only {
                        Some(k)
                    } else {
                        Some(Value::Tuple(vec![k, Value::Ref(base.with(Elem::MapVal(i)))]))
                    }
                }
            },
            IterState::Zip(a, b) => match (self.iter_next(a, span)?, self.iter_next(b, span)?) {
                (Some(x), Some(y)) => Some(Value::Tuple(vec![x, y])),
                _ => None,
            },
            IterState::Take(inner, n) => {
                if *n == 0 {
                    None
                } else {
                    *n -= 1;
                    self.iter_next(inner, span)?
                }
            }
            IterState::Skip(inner, n) => {
                while *n > 0 {
                    *n -= 1;
                    if self.iter_next(inner, span)?.is_none() {
                        return Ok(None);
                    }
                }
                self.iter_next(inner, span)?
            }
            IterState::Map2(inner, f) => match self.iter_next(inner, span)? {
                Some(x) => {
                    let f = f.clone();
                    Some(self.call_value(&f, vec![x], span)?)
                }
                None => None,
            },
            IterState::Filter(inner, f) => loop {
                match self.iter_next(inner, span)? {
                    None => break None,
                    Some(x) => {
                        let mark = self.heap.len();
                        let slot = self.alloc(x);
                        let f = f.clone();
                        let keep = self.call_value(&f, vec![Value::Ref(slot.clone())], span)?;
                        let x = std::mem::replace(self.read_mut(&slot), Value::Uninit);
                        self.release(mark);
                        if matches!(keep, Value::Bool(true)) {
                            break Some(x);
                        }
                    }
                }
            },
            IterState::Enumerate(inner, i) => match self.iter_next(inner, span)? {
                Some(x) => {
                    let n = *i;
                    *i += 1;
                    Some(Value::Tuple(vec![Value::Int(n), x]))
                }
                None => None,
            },
            IterState::User { value, next, tys } => {
                let slot = self.alloc(std::mem::replace(value.as_mut(), Value::Uninit));
                let r = self.call_trait(*next, tys.clone(), vec![Value::Ref(slot.clone())], span);
                **value = std::mem::replace(self.read_mut(&slot), Value::Uninit);
                match r? {
                    Value::Adt(0, mut f) => f.pop(),
                    _ => None,
                }
            }
        })
    }

    fn take_iter(&mut self, v: Value) -> IterState {
        match v {
            Value::Iter(b) => *b,
            other => IterState::Owned(vec![other].into()),
        }
    }

    // ── display (§13.5 `Choopinchochu`) ────────────────────────────────────

    pub(super) fn display(&mut self, v: &Value, ty: &Ty) -> R<String> {
        let v = self.target(v).clone();
        Ok(match ty {
            Ty::Int(_) => v.int().to_string(),
            Ty::Float(32) => match v {
                Value::F32(x) => fmt_f32(x),
                Value::F64(x) => fmt_f64(x),
                _ => String::new(),
            },
            Ty::Float(_) => match v {
                Value::F64(x) => fmt_f64(x),
                Value::F32(x) => fmt_f32(x),
                _ => String::new(),
            },
            Ty::Bool => (if matches!(v, Value::Bool(true)) { "avunu" } else { "kaadhu" }).into(),
            // Text shows as itself, also inside records and lists (spec D.2).
            Ty::Char => match v {
                Value::Char(c) => c.to_string(),
                _ => String::new(),
            },
            Ty::Str => v.str().to_string(),
            Ty::Unit => "()".into(),
            Ty::Ref(_, inner) => self.display(&v, inner)?,
            Ty::Tuple(items) => {
                let xs = match v {
                    Value::Tuple(xs) => xs,
                    _ => Vec::new(),
                };
                let mut parts = Vec::new();
                for (x, t) in xs.iter().zip(items) {
                    parts.push(self.display(x, t)?);
                }
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            Ty::Array(..) | Ty::App(Con::Varusa | Con::Mukka, _) => {
                let elem = match ty {
                    Ty::Array(e, _) => (**e).clone(),
                    Ty::App(_, a) => a[0].clone(),
                    _ => Ty::Error,
                };
                let xs = self.seq(&v);
                let mut parts = Vec::new();
                for x in &xs {
                    parts.push(self.display(x, &elem)?);
                }
                format!("[{}]", parts.join(", "))
            }
            Ty::App(Con::Jodi | Con::GajibijiJodi, a) => {
                let Value::Map(m) = v else { return Ok("{}".into()) };
                let mut parts = Vec::new();
                for i in m.positions() {
                    let k = self.display(m.key(i), &a[0])?;
                    let x = self.display(m.val(i), &a[1])?;
                    parts.push(format!("{k}: {x}"));
                }
                format!("{{{}}}", parts.join(", "))
            }
            Ty::App(Con::Gumpu, a) => {
                let Value::Map(m) = v else { return Ok("{}".into()) };
                let mut parts = Vec::new();
                for i in m.positions() {
                    parts.push(self.display(m.key(i), &a[0])?);
                }
                format!("{{{}}}", parts.join(", "))
            }
            Ty::App(Con::Undochu, a) => match v {
                Value::Adt(0, xs) => format!("undhi({})", self.display(&xs[0], &a[0])?),
                _ => "ledhu".into(),
            },
            Ty::App(Con::SareThappu, a) => match v {
                Value::Adt(0, xs) => format!("sare({})", self.display(&xs[0], &a[0])?),
                Value::Adt(_, xs) => format!("thappu({})", self.display(&xs[0], &a[1])?),
                _ => String::new(),
            },
            Ty::App(Con::Pette, a) => match v {
                Value::Box(b) => self.display(&b, &a[0])?,
                other => self.display(&other, &a[0])?,
            },
            Ty::App(Con::Paridhi, _) => match v {
                Value::Range(lo, hi, inc, _) => {
                    format!("{lo} nunchi {hi} {}", if inc { "varaku" } else { "lopu" })
                }
                _ => String::new(),
            },
            Ty::App(Con::Adt(_), _) => self.display_adt(&v, ty)?,
            Ty::Fn(..) => "<pani>".into(),
            _ => match v {
                Value::Int(i) => i.to_string(),
                Value::Str(s) => s.to_string(),
                _ => "?".into(),
            },
        })
    }

    fn display_adt(&mut self, v: &Value, ty: &Ty) -> R<String> {
        let Ty::App(Con::Adt(d), args) = ty else { return Ok(String::new()) };
        let g = &self.tc.globals;
        // A user implementation of `Choopinchochu` wins.
        if let Some(def) = g.std_trait("Choopinchochu") {
            let solver = crate::typeck::solve::Solver { g, generics: &[] };
            let tref = TraitRef { def, args: Vec::new() };
            if solver.find_impl(&mut InferTable::default(), ty, &tref, 0).is_some() {
                let fid = g.traits[&def].methods[0].1;
                let mark = self.heap.len();
                let r = self.scratch(v.clone());
                let s = self.call_trait(fid, vec![ty.clone()], vec![r], Span::default())?;
                self.release(mark);
                return Ok(s.str().to_string());
            }
        }
        let Some(adt) = g.adts.get(d) else { return Ok(String::new()) };
        let name = adt.name.clone();
        let std_type = adt.file.is_none();
        match (&adt.kind.clone(), v) {
            (AdtKind::Record(fields), Value::Adt(_, xs)) => {
                if std_type && xs.len() == 1 {
                    return Ok(match &xs[0] {
                        Value::Int(i) => i.to_string(),
                        other => other.str().to_string(),
                    });
                }
                let mut parts = Vec::new();
                for (f, x) in fields.iter().zip(xs) {
                    parts.push(format!("{}: {}", f.name, self.display(x, &f.ty.subst(args))?));
                }
                Ok(format!("{name}{{{}}}", parts.join(", ")))
            }
            (AdtKind::Enum(variants), Value::Adt(i, xs)) => {
                let var = &variants[*i as usize];
                if xs.is_empty() {
                    return Ok(format!("{name}.{}", var.name));
                }
                let mut parts = Vec::new();
                for (t, x) in var.fields.iter().zip(xs) {
                    parts.push(self.display(x, &t.subst(args))?);
                }
                Ok(format!("{name}.{}({})", var.name, parts.join(", ")))
            }
            _ => Ok(String::new()),
        }
    }

    // ── equality, order, hashing (§13.5) ───────────────────────────────────

    fn adt_trait_call(&mut self, trait_name: &str, ty: &Ty, vals: Vec<Value>) -> R<Option<Value>> {
        let g = &self.tc.globals;
        let Some(def) = g.std_trait(trait_name) else { return Ok(None) };
        let solver = crate::typeck::solve::Solver { g, generics: &[] };
        let tref = TraitRef { def, args: Vec::new() };
        if solver.find_impl(&mut InferTable::default(), ty, &tref, 0).is_none() {
            return Ok(None);
        }
        let fid = g.traits[&def].methods[0].1;
        let mark = self.heap.len();
        let refs: Vec<Value> = vals.into_iter().map(|v| self.scratch(v)).collect();
        let r = self.call_trait(fid, vec![ty.clone()], refs, Span::default())?;
        self.release(mark);
        Ok(Some(r))
    }

    fn field_types(&self, ty: &Ty, v: &Value) -> Vec<Ty> {
        match ty {
            Ty::Tuple(items) => items.clone(),
            Ty::Array(e, _) => vec![(**e).clone(); self.seq(v).len()],
            Ty::App(Con::Varusa | Con::Mukka | Con::Pette, a) => vec![a[0].clone(); self.seq(v).len().max(1)],
            Ty::App(Con::Undochu, a) => vec![a[0].clone()],
            Ty::App(Con::SareThappu, a) => match v {
                Value::Adt(0, _) => vec![a[0].clone()],
                _ => vec![a[1].clone()],
            },
            Ty::App(Con::Adt(d), args) => match self.tc.globals.adts.get(d).map(|a| &a.kind) {
                Some(AdtKind::Record(fs)) => fs.iter().map(|f| f.ty.subst(args)).collect(),
                Some(AdtKind::Enum(vs)) => match v {
                    Value::Adt(i, _) => vs
                        .get(*i as usize)
                        .map(|var| var.fields.iter().map(|f| f.subst(args)).collect())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                },
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn parts(&self, v: &Value) -> (i128, Vec<Value>) {
        match v {
            Value::Adt(i, xs) => (*i as i128, xs.clone()),
            Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) => (0, xs.clone()),
            Value::Box(b) => (0, vec![(**b).clone()]),
            Value::Slice(..) | Value::Ref(_) => (0, self.seq(v)),
            _ => (0, Vec::new()),
        }
    }

    pub(super) fn values_eq(&mut self, a: &Value, b: &Value, ty: &Ty) -> R<bool> {
        let a = self.target(a).clone();
        let b = self.target(b).clone();
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("Polchochu", &ty, vec![a.clone(), b.clone()])? {
                return Ok(matches!(r, Value::Bool(true)));
            }
        }
        Ok(match (&a, &b) {
            (Value::F32(x), Value::F32(y)) => x == y,
            (Value::F64(x), Value::F64(y)) => x == y,
            (Value::Int(_) | Value::Bool(_) | Value::Char(_) | Value::Unit, _) => key_of(&a) == key_of(&b),
            (Value::Str(_) | Value::Buf(_), _) => a.str() == b.str(),
            (Value::Map(m), Value::Map(n)) => {
                if m.len() != n.len() {
                    return Ok(false);
                }
                let vt = match &ty {
                    Ty::App(_, args) if args.len() > 1 => args[1].clone(),
                    _ => Ty::Error,
                };
                for i in m.positions() {
                    match n.find(m.key(i)) {
                        Some(j) => {
                            if !self.values_eq(m.val(i), n.val(j), &vt)? {
                                return Ok(false);
                            }
                        }
                        None => return Ok(false),
                    }
                }
                true
            }
            _ => {
                let (ia, xa) = self.parts(&a);
                let (ib, xb) = self.parts(&b);
                if ia != ib || xa.len() != xb.len() {
                    return Ok(false);
                }
                let tys = self.field_types(&ty, &a);
                for (k, (x, y)) in xa.iter().zip(&xb).enumerate() {
                    let t = tys.get(k).cloned().unwrap_or(Ty::Error);
                    if !self.values_eq(x, y, &t)? {
                        return Ok(false);
                    }
                }
                true
            }
        })
    }

    pub(super) fn values_cmp(&mut self, a: &Value, b: &Value, ty: &Ty) -> R<std::cmp::Ordering> {
        use std::cmp::Ordering;
        let a = self.target(a).clone();
        let b = self.target(b).clone();
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("VarusaPettochu", &ty, vec![a.clone(), b.clone()])? {
                return Ok(match r {
                    Value::Adt(0, _) => Ordering::Less,
                    Value::Adt(1, _) => Ordering::Equal,
                    _ => Ordering::Greater,
                });
            }
        }
        Ok(match (&a, &b) {
            (Value::F32(x), Value::F32(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Value::F64(x), Value::F64(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (Value::Int(x), Value::Int(y)) => x.cmp(y),
            (Value::Char(x), Value::Char(y)) => x.cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            (Value::Str(_) | Value::Buf(_), _) => a.str().cmp(&b.str()),
            _ => {
                let (ia, xa) = self.parts(&a);
                let (ib, xb) = self.parts(&b);
                if ia != ib {
                    return Ok(ia.cmp(&ib));
                }
                let tys = self.field_types(&ty, &a);
                for (k, (x, y)) in xa.iter().zip(&xb).enumerate() {
                    let t = tys.get(k).cloned().unwrap_or(Ty::Error);
                    let o = self.values_cmp(x, y, &t)?;
                    if o != Ordering::Equal {
                        return Ok(o);
                    }
                }
                xa.len().cmp(&xb.len())
            }
        })
    }

    fn xerox(&mut self, v: &Value, ty: &Ty) -> R<Value> {
        let v = self.target(v).clone();
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("XeroxTheeyochu", &ty, vec![v.clone()])? {
                return Ok(r);
            }
        }
        Ok(v)
    }

    /// Stable merge sort with a fallible comparison.
    fn sort_by(
        &mut self,
        xs: Vec<Value>,
        cmp: &mut dyn FnMut(&mut Self, &Value, &Value) -> R<std::cmp::Ordering>,
    ) -> R<Vec<Value>> {
        if xs.len() <= 1 {
            return Ok(xs);
        }
        let mut xs = xs;
        let right = xs.split_off(xs.len() / 2);
        let left = self.sort_by(xs, cmp)?;
        let right = self.sort_by(right, cmp)?;
        let mut out = Vec::with_capacity(left.len() + right.len());
        let mut l = left.into_iter().peekable();
        let mut r = right.into_iter().peekable();
        while let (Some(a), Some(b)) = (l.peek(), r.peek()) {
            if cmp(self, b, a)? == std::cmp::Ordering::Less {
                out.push(r.next().expect("peeked"));
            } else {
                out.push(l.next().expect("peeked"));
            }
        }
        out.extend(l);
        out.extend(r);
        Ok(out)
    }

    // ── built-in trait methods ─────────────────────────────────────────────

    pub(super) fn trait_native(&mut self, name: &str, tys: &[Ty], args: Vec<Value>, span: Span) -> R<Value> {
        let ty = tys.first().cloned().unwrap_or(Ty::Error);
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Unit);
        match name {
            "XeroxTheeyochu.xerox" => self.xerox(&arg(0), &ty),
            "Polchochu.okateyna" => Ok(bool_v(self.values_eq(&arg(0), &arg(1), &ty)?)),
            "VarusaPettochu.polchu" => Ok(kramam(self.values_cmp(&arg(0), &arg(1), &ty)?)),
            "Choopinchochu.padham_ga" => {
                let s = self.display(&arg(0), &ty)?;
                Ok(Value::Str(Rc::from(s)))
            }
            "HashCheyochu.hash_cheyyi" => {
                let k = key_of(self.target(&arg(0)));
                self.hash_into(&arg(1), &k);
                Ok(Value::Unit)
            }
            "Sardhukochu.sardhu" => Ok(Value::Unit),
            "Okkokkati.tharvatha" => {
                let a = self.ref_addr(&arg(0));
                let mut it = match std::mem::replace(self.read_mut(&a), Value::Uninit) {
                    Value::Iter(b) => b,
                    _ => return Ok(Value::none()),
                };
                let next = self.iter_next(&mut it, span);
                *self.read_mut(&a) = Value::Iter(it);
                Ok(next?.map_or_else(Value::none, Value::some))
            }
            _ => {
                let op = match name {
                    "Kalupochu.kalupu" => crate::ast::BinOp::Add,
                    "Theeseyochu.theesey" => crate::ast::BinOp::Sub,
                    "Guninchochu.guninchu" => crate::ast::BinOp::Mul,
                    "Panchochu.panchu" => crate::ast::BinOp::Div,
                    "Sheshamochu.shesham" => crate::ast::BinOp::Rem,
                    "Thippochu.thippu" => {
                        let x = self.target(&arg(0)).clone();
                        let t = crate::mir::build_op_ty(&ty);
                        return match t {
                            Some(t) => self.unary(crate::ast::UnOp::Neg, t, x, span),
                            None => Err(self.trap("T05", format!("`{name}` ki implementation ledhu"), span)),
                        };
                    }
                    _ => return Err(self.trap("T05", format!("`{name}` ki implementation ledhu"), span)),
                };
                let x = self.target(&arg(0)).clone();
                let y = self.target(&arg(1)).clone();
                match crate::mir::build_op_ty(&ty) {
                    Some(t) => self.binary(op, t, x, y, span),
                    None => Err(self.trap("T05", format!("`{name}` ki implementation ledhu"), span)),
                }
            }
        }
    }

    fn hash_into(&mut self, h: &Value, k: &super::value::Key) {
        use std::hash::{Hash, Hasher};
        let mut s = std::collections::hash_map::DefaultHasher::new();
        k.hash(&mut s);
        let bits = s.finish();
        self.with_target_mut(h, |v| {
            if let Value::Adt(_, fs) = v {
                let old = fs.first().map(|f| f.int()).unwrap_or(0) as u64;
                let mixed = (old ^ bits).wrapping_mul(0x100_0000_01b3).rotate_left(5);
                if let Some(f) = fs.first_mut() {
                    *f = Value::Int(i128::from(mixed));
                }
            }
        });
    }

    // ── standard functions (§21) ───────────────────────────────────────────

    #[allow(clippy::cognitive_complexity)]
    pub(super) fn native(&mut self, name: &str, tys: &[Ty], args: Vec<Value>, span: Span) -> R<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Unit);
        let ty = |i: usize| tys.get(i).cloned().unwrap_or(Ty::Error);
        match name {
            // ── prelude ──
            "prelude.choopinchu" | "prelude.cheppu" | "prelude.gola_pettu" => {
                let mut s = self.display(&arg(0), &ty(0))?;
                if name != "prelude.cheppu" {
                    s.push('\n');
                }
                if name == "prelude.gola_pettu" {
                    self.stderr.push_str(&s);
                } else {
                    self.stdout.push_str(&s);
                    if self.stream && self.stdout.len() > 1 << 16 {
                        self.flush();
                    }
                }
                Ok(Value::Unit)
            }
            "prelude.nijame" => {
                if matches!(arg(0), Value::Bool(false)) {
                    let msg = arg(1).str();
                    let text = if msg.is_empty() {
                        "nijame() fail ayyindhi".to_string()
                    } else {
                        format!("nijame() fail ayyindhi: {msg}")
                    };
                    return Err(self.trap("T07", text, span));
                }
                Ok(Value::Unit)
            }
            "prelude.kompa_munigindhi" => {
                let msg = arg(0).str();
                Err(self.trap(
                    "T05",
                    if msg.is_empty() { "kompa_munigindhi()".to_string() } else { msg.to_string() },
                    span,
                ))
            }
            "prelude.maarchu" | "prelude.kudhirithe_maarchu" | "prelude.kolpothu_maarchu" => {
                self.convert(name, &ty(1), &ty(0), arg(0), span)
            }
            "prelude.size_of" | "prelude.align_of" => Ok(Value::Int(size_of(&ty(0)))),
            "prelude.kalipi" => {
                let a = self.take_iter(arg(0));
                let b = self.take_iter(arg(1));
                Ok(Value::Iter(Box::new(IterState::Zip(Box::new(a), Box::new(b)))))
            }
            "prelude.modhati" | "prelude.dhaatu" => {
                let it = self.take_iter(arg(0));
                let n = arg(1).int().max(0) as usize;
                let st = if name == "prelude.modhati" {
                    IterState::Take(Box::new(it), n)
                } else {
                    IterState::Skip(Box::new(it), n)
                };
                Ok(Value::Iter(Box::new(st)))
            }
            "prelude.prathi_dhaaniki" => {
                let it = self.take_iter(arg(0));
                Ok(Value::Iter(Box::new(IterState::Map2(Box::new(it), arg(1)))))
            }
            "prelude.jalleda_pattu" => {
                let it = self.take_iter(arg(0));
                Ok(Value::Iter(Box::new(IterState::Filter(Box::new(it), arg(1)))))
            }
            "prelude.number_vesi" => {
                let it = self.take_iter(arg(0));
                Ok(Value::Iter(Box::new(IterState::Enumerate(Box::new(it), 0))))
            }
            "prelude.poguchey" => {
                let mut it = self.take_iter(arg(0));
                let mut acc = arg(1);
                let f = arg(2);
                while let Some(x) = self.iter_next(&mut it, span)? {
                    acc = self.call_value(&f, vec![acc, x], span)?;
                }
                Ok(acc)
            }
            "prelude.varusa_ga" => {
                let mut it = self.take_iter(arg(0));
                let mut out = Vec::new();
                while let Some(x) = self.iter_next(&mut it, span)? {
                    out.push(x);
                }
                Ok(Value::Vec(out))
            }
            "prelude.anni_theesuko" | "varusa.anni_theesuko" => match arg(0) {
                Value::Vec(xs) | Value::Array(xs) => Ok(Value::Iter(Box::new(IterState::Owned(xs.into())))),
                other => Ok(other),
            },

            // ── padham ──
            "padham.enni" => Ok(Value::Int(self.arg_str(&arg(0)).chars().count() as i128)),
            "padham.byte_len" => Ok(Value::Int(self.arg_str(&arg(0)).len() as i128)),
            "padham.aksharaalu" => {
                let text = self.arg_str(&arg(0));
                Ok(Value::Iter(Box::new(IterState::Chars { text, pos: 0 })))
            }
            "padham.vidagottu" => {
                let s = self.arg_str(&arg(0));
                let sep = self.arg_str(&arg(1));
                let parts: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::Str(Rc::from(c.to_string()))).collect()
                } else {
                    s.split(&*sep).map(|p| Value::Str(Rc::from(p))).collect()
                };
                Ok(Value::Vec(parts))
            }
            "padham.kathirinchu" => Ok(Value::Str(Rc::from(self.arg_str(&arg(0)).trim()))),
            "padham.tho_modhalaindha" => {
                let (s, p) = (self.arg_str(&arg(0)), self.arg_str(&arg(1)));
                Ok(bool_v(s.starts_with(&*p)))
            }
            "padham.lo_undha" => {
                let (s, p) = (self.arg_str(&arg(0)), self.arg_str(&arg(1)));
                Ok(bool_v(s.contains(&*p)))
            }
            "padham.ankam_ga" | "padham.aasajeevi_ga" => {
                let s = self.arg_str(&arg(0));
                let base = arg(1).int() as u32;
                let it =
                    if name == "padham.ankam_ga" { IntTy::ANKAM } else { IntTy { signed: false, bits: 64 } };
                let parsed = (2..=36).contains(&base).then(|| i128::from_str_radix(&s, base).ok()).flatten();
                Ok(match parsed {
                    Some(v) if v >= it.range().0 && v <= it.range().1 => Value::ok(Value::Int(v)),
                    _ => Value::err(thappu_record(format!("`{s}` {} kaadhu", int_name(it)))),
                })
            }
            "padham.amsam_ga" => {
                let s = self.arg_str(&arg(0));
                Ok(match s.trim().parse::<f64>() {
                    Ok(x) => Value::ok(Value::F64(x)),
                    Err(_) => Value::err(thappu_record(format!("`{s}` amsam kaadhu"))),
                })
            }
            "padham.baitlu" => {
                let s = self.arg_str(&arg(0));
                let bytes: Vec<Value> = s.bytes().map(|b| Value::Int(i128::from(b))).collect();
                let n = bytes.len();
                let a = self.alloc(Value::Array(bytes));
                Ok(Value::Slice(a, 0, n))
            }
            "padham.baitlu_nunchi" => {
                let bytes: Vec<u8> = self.seq(&arg(0)).iter().map(|v| v.int() as u8).collect();
                Ok(match String::from_utf8(bytes) {
                    Ok(s) => Value::ok(Value::Str(Rc::from(s))),
                    Err(e) => Value::err(thappu_record(format!("UTF-8 kaadhu: {e}"))),
                })
            }
            "padham.byte_bhaagam" => {
                let s = self.arg_str(&arg(0));
                let (a, b) = (arg(1).int(), arg(2).int());
                let part = (a >= 0 && b >= a).then(|| s.get(a as usize..b as usize)).flatten();
                Ok(match part {
                    Some(p) => Value::ok(Value::Str(Rc::from(p))),
                    None => Value::err(thappu_record(format!("{a}..{b} akshara haddu meeda ledhu"))),
                })
            }

            // ── varusa and mukka ──
            "varusa.kotthadhi" => Ok(Value::Vec(Vec::new())),
            "varusa.enni" | "mukka.enni" => Ok(Value::Int(self.seq_len_of(&arg(0)) as i128)),
            "varusa.khaaliyaa" | "mukka.khaaliyaa" => Ok(bool_v(self.seq_len_of(&arg(0)) == 0)),
            "varusa.cherchu" => {
                let x = arg(1);
                self.with_target_mut(&arg(0), |v| {
                    if let Value::Vec(xs) = v {
                        xs.push(x);
                    }
                });
                Ok(Value::Unit)
            }
            "varusa.theesey" => {
                let popped = self.with_target_mut(&arg(0), |v| match v {
                    Value::Vec(xs) => xs.pop(),
                    _ => None,
                });
                Ok(popped.map_or_else(Value::none, Value::some))
            }
            "varusa.thechu" | "varusa.maare_thechu" | "mukka.thechu" => {
                Ok(match self.elem_addr(&arg(0), arg(1).int()) {
                    Some(a) => Value::some(Value::Ref(a)),
                    None => Value::none(),
                })
            }
            "varusa.sort_cheyyi" | "varusa.ila_sort_cheyyi" => {
                let a = self.ref_addr(&arg(0));
                let xs = match std::mem::replace(self.read_mut(&a), Value::Uninit) {
                    Value::Vec(xs) => xs,
                    other => {
                        *self.read_mut(&a) = other;
                        return Ok(Value::Unit);
                    }
                };
                let elem = ty(1);
                let f = arg(1);
                let by_fn = name == "varusa.ila_sort_cheyyi";
                let sorted = self.sort_by(xs, &mut |m, x, y| {
                    if by_fn {
                        let mark = m.heap.len();
                        let rx = m.scratch(x.clone());
                        let ry = m.scratch(y.clone());
                        let r = m.call_value(&f, vec![rx, ry], span)?;
                        m.release(mark);
                        Ok(match r {
                            Value::Adt(0, _) => std::cmp::Ordering::Less,
                            Value::Adt(1, _) => std::cmp::Ordering::Equal,
                            _ => std::cmp::Ordering::Greater,
                        })
                    } else {
                        m.values_cmp(x, y, &elem)
                    }
                });
                match sorted {
                    Ok(v) => {
                        *self.read_mut(&a) = Value::Vec(v);
                        Ok(Value::Unit)
                    }
                    Err(e) => Err(e),
                }
            }
            "mukka.pointer" | "pointer.jarugu" => {
                Err(self.trap("T05", "raw pointers interpreter lo ledhu", span))
            }

            // ── jodi, gajibiji_jodi, gumpu ──
            "jodi.kotthadhi" | "gajibiji_jodi.kotthadhi" | "gumpu.kotthadhi" => {
                Ok(Value::Map(Box::default()))
            }
            "jodi.cherchu" | "gajibiji_jodi.cherchu" => {
                let (k, v) = (arg(1), arg(2));
                let old = self.with_target_mut(&arg(0), |m| match m {
                    Value::Map(m) => m.insert(k, v),
                    _ => None,
                });
                Ok(old.map_or_else(Value::none, Value::some))
            }
            "gumpu.cherchu" => {
                let k = arg(1);
                let old = self.with_target_mut(&arg(0), |m| match m {
                    Value::Map(m) => m.insert(k, Value::Unit),
                    _ => None,
                });
                Ok(bool_v(old.is_none()))
            }
            "jodi.thechu" | "jodi.maare_thechu" | "gajibiji_jodi.thechu" => {
                let k = self.target(&arg(1)).clone();
                let base = self.ref_addr(&arg(0));
                let found = match self.read(&base) {
                    Value::Map(m) => m.find(&k),
                    _ => None,
                };
                Ok(match found {
                    Some(i) => Value::some(Value::Ref(base.with(Elem::MapVal(i)))),
                    None => Value::none(),
                })
            }
            "jodi.lekapothe_cherchu" => {
                let (k, d) = (arg(1), arg(2));
                let base = self.ref_addr(&arg(0));
                let i = match self.read_mut(&base) {
                    Value::Map(m) => match m.find(&k) {
                        Some(i) => i,
                        None => {
                            m.insert(k.clone(), d);
                            m.find(&k).unwrap_or(0)
                        }
                    },
                    _ => 0,
                };
                Ok(Value::Ref(base.with(Elem::MapVal(i))))
            }
            "jodi.theesey" | "gajibiji_jodi.theesey" | "gumpu.theesey" => {
                let k = self.target(&arg(1)).clone();
                let old = self.with_target_mut(&arg(0), |m| match m {
                    Value::Map(m) => m.remove(&k),
                    _ => None,
                });
                if name == "gumpu.theesey" {
                    return Ok(bool_v(old.is_some()));
                }
                Ok(old.map_or_else(Value::none, Value::some))
            }
            "jodi.undhaa" | "gajibiji_jodi.undhaa" | "gumpu.undhaa" => {
                let k = self.target(&arg(1)).clone();
                Ok(bool_v(matches!(self.target(&arg(0)), Value::Map(m) if m.find(&k).is_some())))
            }
            "jodi.enni" | "gajibiji_jodi.enni" | "gumpu.enni" => Ok(Value::Int(match self.target(&arg(0)) {
                Value::Map(m) => m.len() as i128,
                _ => 0,
            })),

            // ── undochu, sare_thappu, pette ──
            "undochu.undhaa" => Ok(bool_v(matches!(self.target(&arg(0)), Value::Adt(0, _)))),
            "undochu.ledhaa" => Ok(bool_v(matches!(self.target(&arg(0)), Value::Adt(1, _)))),
            "undochu.lekapothe" => Ok(match arg(0) {
                Value::Adt(0, mut xs) => xs.pop().unwrap_or(Value::Unit),
                _ => arg(1),
            }),
            "undochu.kachithanga" => match arg(0) {
                Value::Adt(0, mut xs) => Ok(xs.pop().unwrap_or(Value::Unit)),
                _ => Err(self.trap("T06", "`ledhu` meeda kachithanga() pilicharu", span)),
            },
            "undochu.prathi_dhaaniki" => match arg(0) {
                Value::Adt(0, mut xs) => {
                    let x = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::some(self.call_value(&arg(1), vec![x], span)?))
                }
                other => Ok(other),
            },
            "sare_thappu.sareyaa" => Ok(bool_v(matches!(self.target(&arg(0)), Value::Adt(0, _)))),
            "sare_thappu.kachithanga" => match arg(0) {
                Value::Adt(0, mut xs) => Ok(xs.pop().unwrap_or(Value::Unit)),
                Value::Adt(_, xs) => {
                    let et = match &ty(0) {
                        Ty::App(Con::SareThappu, a) => a[1].clone(),
                        _ => ty(2),
                    };
                    let shown = match xs.first() {
                        Some(e) => self.display(e, &et).unwrap_or_default(),
                        None => String::new(),
                    };
                    Err(self.trap("T06", format!("`thappu({shown})` meeda kachithanga() pilicharu"), span))
                }
                _ => Err(self.trap("T06", "kachithanga()", span)),
            },
            "sare_thappu.thappu_maarchu" => match arg(0) {
                Value::Adt(1, mut xs) => {
                    let e = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::err(self.call_value(&arg(1), vec![e], span)?))
                }
                other => Ok(other),
            },
            "sare_thappu.prathi_dhaaniki" => match arg(0) {
                Value::Adt(0, mut xs) => {
                    let x = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::ok(self.call_value(&arg(1), vec![x], span)?))
                }
                other => Ok(other),
            },
            "pette.kotthadhi" => Ok(Value::Box(Box::new(arg(0)))),

            // ── scalars ──
            "aksharam.digit_aa" | "aksharam.letter_aa" | "aksharam.space_aa" | "aksharam.alnum_aa"
            | "aksharam.ascii_aa" => {
                let c = match self.target(&arg(0)) {
                    Value::Char(c) => *c,
                    _ => '\0',
                };
                Ok(bool_v(match name {
                    "aksharam.digit_aa" => c.is_numeric(),
                    "aksharam.letter_aa" => c.is_alphabetic(),
                    "aksharam.space_aa" => c.is_whitespace(),
                    "aksharam.alnum_aa" => c.is_alphanumeric(),
                    _ => c.is_ascii(),
                }))
            }
            "ankamlu.hex_ga" => {
                let v = self.target(&arg(0)).int();
                let it = match ty(0) {
                    Ty::Int(it) => it,
                    _ => IntTy::ANKAM,
                };
                let bits = u32::from(it.bits);
                let unsigned = if bits >= 128 { v as u128 } else { (v as u128) & ((1u128 << bits) - 1) };
                Ok(Value::Str(Rc::from(format!("{unsigned:x}"))))
            }
            "amsam64.bits" => Ok(Value::Int(match self.target(&arg(0)) {
                Value::F64(x) => i128::from(x.to_bits()),
                _ => 0,
            })),
            "amsam32.bits" => Ok(Value::Int(match self.target(&arg(0)) {
                Value::F32(x) => i128::from(x.to_bits()),
                _ => 0,
            })),
            "amsam64.bits_nunchi" => Ok(Value::F64(f64::from_bits(arg(0).int() as u64))),
            "amsam32.bits_nunchi" => Ok(Value::F32(f32::from_bits(arg(0).int() as u32))),

            // ── Raatha, Hasher ──
            "Raatha.kotthadhi" => Ok(Value::Adt(0, vec![Value::Buf(String::new())])),
            "Raatha.raayi" | "Raatha.akshara_raayi" => {
                let text = if name == "Raatha.raayi" {
                    self.display(&arg(1), &ty(1))?
                } else {
                    match arg(1) {
                        Value::Char(c) => c.to_string(),
                        _ => String::new(),
                    }
                };
                self.with_target_mut(&arg(0), |r| {
                    if let Value::Adt(_, fs) = r {
                        match fs.first_mut() {
                            Some(Value::Buf(b)) => b.push_str(&text),
                            Some(other) => {
                                let mut s = other.str().to_string();
                                s.push_str(&text);
                                *other = Value::Buf(s);
                            }
                            None => {}
                        }
                    }
                });
                Ok(Value::Unit)
            }
            "Raatha.enni" => Ok(Value::Int(match self.target(&arg(0)) {
                Value::Adt(_, fs) => fs.first().map_or(0, |f| f.str().chars().count()) as i128,
                _ => 0,
            })),
            "Raatha.padham_ga" => {
                let s = self.with_target_mut(&arg(0), |r| match r {
                    Value::Adt(_, fs) => match fs.first_mut() {
                        Some(f) => std::mem::replace(f, Value::Buf(String::new())).str(),
                        None => Rc::from(""),
                    },
                    _ => Rc::from(""),
                });
                Ok(Value::Str(s))
            }
            "Hasher.kalupu" => {
                let k = key_of(self.target(&arg(1)));
                self.hash_into(&arg(0), &k);
                Ok(Value::Unit)
            }
            "Hasher.mugimpu" => Ok(match self.target(&arg(0)) {
                Value::Adt(_, fs) => fs.first().cloned().unwrap_or(Value::Int(0)),
                _ => Value::Int(0),
            }),

            // ── lekkalu ──
            n if n.starts_with("lekkalu.") => self.lekkalu(&n["lekkalu.".len()..], tys, args, span),

            // ── file ──
            "file.chadhuvu" => Ok(file_result(
                std::fs::read_to_string(&*self.arg_str(&arg(0))).map(|s| Value::Str(Rc::from(s))),
            )),
            "file.raayi" => {
                let (p, t) = (self.arg_str(&arg(0)), self.arg_str(&arg(1)));
                Ok(file_result(std::fs::write(&*p, t.as_bytes()).map(|_| Value::Unit)))
            }
            "file.baitlu_chadhuvu" => Ok(file_result(
                std::fs::read(&*self.arg_str(&arg(0)))
                    .map(|b| Value::Vec(b.into_iter().map(|x| Value::Int(i128::from(x))).collect())),
            )),
            "file.baitlu_raayi" => {
                let p = self.arg_str(&arg(0));
                let bytes: Vec<u8> = self.seq(&arg(1)).iter().map(|v| v.int() as u8).collect();
                Ok(file_result(std::fs::write(&*p, bytes).map(|_| Value::Unit)))
            }
            "file.undhaa" => Ok(bool_v(std::path::Path::new(&*self.arg_str(&arg(0))).exists())),
            "file.folderaa" => Ok(bool_v(std::path::Path::new(&*self.arg_str(&arg(0))).is_dir())),
            "file.list_cheyyi" => {
                let p = self.arg_str(&arg(0));
                Ok(file_result(std::fs::read_dir(&*p).map(|rd| {
                    let mut names: Vec<String> = rd
                        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                        .collect();
                    names.sort();
                    Value::Vec(names.into_iter().map(|n| Value::Str(Rc::from(n))).collect())
                })))
            }
            "file.folder_cheyyi" => {
                Ok(file_result(std::fs::create_dir_all(&*self.arg_str(&arg(0))).map(|_| Value::Unit)))
            }
            "file.theesey" => {
                Ok(file_result(std::fs::remove_file(&*self.arg_str(&arg(0))).map(|_| Value::Unit)))
            }
            "file.path_kalupu" => {
                let (a, b) = (self.arg_str(&arg(0)), self.arg_str(&arg(1)));
                Ok(Value::Str(Rc::from(std::path::Path::new(&*a).join(&*b).to_string_lossy().into_owned())))
            }
            "file.path_peru" | "file.path_folder" | "file.path_extension" => {
                let p = self.arg_str(&arg(0));
                let path = std::path::Path::new(&*p);
                let part = match name {
                    "file.path_peru" => path.file_name(),
                    "file.path_folder" => path.parent().map(|x| x.as_os_str()),
                    _ => path.extension(),
                }
                .map(|s| s.to_string_lossy().into_owned());
                Ok(match (name, part) {
                    ("file.path_extension", Some(s)) => Value::some(Value::Str(Rc::from(s))),
                    ("file.path_extension", None) => Value::none(),
                    (_, s) => Value::Str(Rc::from(s.unwrap_or_default())),
                })
            }

            // ── system ──
            "system.args" => {
                Ok(Value::Vec(std::env::args().skip(1).map(|a| Value::Str(Rc::from(a))).collect()))
            }
            "system.env" => Ok(match std::env::var(&*self.arg_str(&arg(0))) {
                Ok(v) => Value::some(Value::Str(Rc::from(v))),
                Err(_) => Value::none(),
            }),
            "system.line_chadhuvu" => {
                let mut line = String::new();
                Ok(match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => Value::none(),
                    Ok(_) => Value::some(Value::Str(Rc::from(line.trim_end_matches(['\n', '\r'])))),
                })
            }
            "system.flush" => {
                self.flush();
                Ok(Value::Unit)
            }
            "system.vellipo" => Err(Halt::Exit(arg(0).int() as i32)),
            "system.nadupu" => {
                let prog = self.arg_str(&arg(0));
                let argv: Vec<String> =
                    self.seq(&arg(1)).iter().map(|v| self.target(v).str().to_string()).collect();
                Ok(match std::process::Command::new(&*prog).args(&argv).output() {
                    Ok(o) => Value::ok(Value::Adt(
                        0,
                        vec![
                            Value::Int(i128::from(o.status.code().unwrap_or(-1))),
                            Value::Str(Rc::from(String::from_utf8_lossy(&o.stdout).into_owned())),
                            Value::Str(Rc::from(String::from_utf8_lossy(&o.stderr).into_owned())),
                        ],
                    )),
                    Err(e) => Value::err(thappu_record(e.to_string())),
                })
            }
            "system.os" => Ok(Value::Str(Rc::from(std::env::consts::OS))),
            "system.arch" => Ok(Value::Str(Rc::from(std::env::consts::ARCH))),

            // ── samayam ──
            "samayam.ippudu" => {
                Ok(Value::Adt(0, vec![Value::Int(self.started.elapsed().as_nanos() as i128)]))
            }
            "Kshanam.gadichindhi" => {
                let then = match self.target(&arg(0)) {
                    Value::Adt(_, fs) => fs.first().map_or(0, Value::int),
                    _ => 0,
                };
                let now = self.started.elapsed().as_nanos() as i128;
                Ok(Value::Adt(0, vec![Value::Int(now - then)]))
            }
            "Kaalam.millisec" | "Kaalam.nanosec" => {
                let ns = match self.target(&arg(0)) {
                    Value::Adt(_, fs) => fs.first().map_or(0, Value::int),
                    _ => 0,
                };
                Ok(Value::Int(if name == "Kaalam.millisec" { ns / 1_000_000 } else { ns }))
            }
            n if n.starts_with("memory.") => {
                Err(self.trap("T05", "`memory` functions interpreter lo levu (M11)", span))
            }
            _ => Err(self.trap("T05", format!("`{name}` inka interpreter lo implement avvaledhu"), span)),
        }
    }

    fn lekkalu(&mut self, name: &str, tys: &[Ty], args: Vec<Value>, span: Span) -> R<Value> {
        let t = tys.first().cloned().unwrap_or(Ty::Error);
        let a = args.first().cloned().unwrap_or(Value::Unit);
        let b = args.get(1).cloned().unwrap_or(Value::Unit);
        if let Some(op) = name
            .strip_prefix("chuttu_")
            .or(name.strip_prefix("trupthiga_"))
            .or(name.strip_prefix("kudhirithe_"))
        {
            let Ty::Int(it) = t else { return Ok(Value::Unit) };
            let (x, y) = (a.int(), b.int());
            let exact = match op {
                "kalupu" => x.checked_add(y),
                "theesey" => x.checked_sub(y),
                "guninchu" => x.checked_mul(y),
                _ => {
                    if y == 0 || (it.signed && y == -1 && x == it.range().0) {
                        None
                    } else {
                        Some(x / y)
                    }
                }
            };
            let (lo, hi) = it.range();
            let fits = exact.filter(|v| *v >= lo && *v <= hi);
            return Ok(if name.starts_with("kudhirithe_") {
                fits.map_or_else(Value::none, |v| Value::some(Value::Int(v)))
            } else if name.starts_with("chuttu_") {
                let wide = match op {
                    "kalupu" => x.wrapping_add(y),
                    "theesey" => x.wrapping_sub(y),
                    _ => x.wrapping_mul(y),
                };
                Value::Int(wrap(it, wide))
            } else {
                Value::Int(match exact {
                    Some(v) => v.clamp(lo, hi),
                    None => {
                        if (x < 0) ^ (y < 0) && op == "guninchu" {
                            lo
                        } else {
                            hi
                        }
                    }
                })
            });
        }
        let float = |v: &Value| match v {
            Value::F64(x) => Some(*x),
            Value::F32(x) => Some(f64::from(*x)),
            _ => None,
        };
        let back = |x: f64, like: &Value| match like {
            Value::F32(_) => Value::F32(x as f32),
            _ => Value::F64(x),
        };
        match name {
            "sqrt" | "sin" | "cos" | "tan" | "exp" | "log" | "floor" | "ceil" | "round" => {
                let x = float(&a).unwrap_or(0.0);
                let r = match name {
                    "sqrt" => x.sqrt(),
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "exp" => x.exp(),
                    "log" => x.ln(),
                    "floor" => x.floor(),
                    "ceil" => x.ceil(),
                    _ => x.round(),
                };
                Ok(back(r, &a))
            }
            "abs" => match (&t, &a) {
                (Ty::Int(it), Value::Int(x)) => self.fit(*it, x.abs(), format_args!("abs({x})"), span),
                _ => Ok(back(float(&a).unwrap_or(0.0).abs(), &a)),
            },
            "min" | "max" => {
                let o = self.values_cmp(&a, &b, &t)?;
                let pick_a = (name == "min") == (o != std::cmp::Ordering::Greater);
                Ok(if pick_a { a } else { b })
            }
            "pow" => match (&t, &a, &b) {
                (Ty::Int(it), Value::Int(x), Value::Int(y)) => {
                    if *y < 0 {
                        return Err(self.trap("T05", "pow: negative exponent", span));
                    }
                    let mut r: i128 = 1;
                    for _ in 0..*y {
                        r = match r.checked_mul(*x) {
                            Some(v) if v >= it.range().0 && v <= it.range().1 => v,
                            _ => return Err(self.overflow(*it, format_args!("{x} ^ {y}"), span)),
                        };
                    }
                    Ok(Value::Int(r))
                }
                _ => Ok(back(float(&a).unwrap_or(0.0).powf(float(&b).unwrap_or(0.0)), &a)),
            },
            "motham" | "dot" => {
                let xs = self.seq(&a);
                let ys = if name == "dot" { self.seq(&b) } else { Vec::new() };
                if name == "dot" && xs.len() != ys.len() {
                    return Err(self.trap(
                        "T05",
                        format!("dot: podavulu veru ({} , {})", xs.len(), ys.len()),
                        span,
                    ));
                }
                let elem = match &t {
                    Ty::Int(_) | Ty::Float(_) => t.clone(),
                    _ => Ty::Float(64),
                };
                if let Ty::Int(it) = elem {
                    let mut s: i128 = 0;
                    for (i, x) in xs.iter().enumerate() {
                        let term = if name == "dot" { x.int() * ys[i].int() } else { x.int() };
                        s += term;
                        if s < it.range().0 || s > it.range().1 {
                            return Err(self.overflow(it, "motham", span));
                        }
                    }
                    return Ok(Value::Int(s));
                }
                // The documented blocked order: eight lanes, then the lanes in order.
                let mut lanes = [0.0f64; 8];
                for (i, x) in xs.iter().enumerate() {
                    let v = float(x).unwrap_or(0.0)
                        * if name == "dot" { float(&ys[i]).unwrap_or(0.0) } else { 1.0 };
                    lanes[i % 8] += v;
                }
                let total: f64 = lanes.iter().sum();
                Ok(match xs.first() {
                    Some(Value::F32(_)) => Value::F32(total as f32),
                    _ => Value::F64(total),
                })
            }
            "polchu_amsam" => {
                let (x, y) = (float(&a).unwrap_or(0.0), float(&b).unwrap_or(0.0));
                Ok(kramam(x.total_cmp(&y)))
            }
            _ => Err(self.trap("T05", format!("`lekkalu.{name}` inka implement avvaledhu"), span)),
        }
    }

    /// `maarchu`, `kudhirithe_maarchu`, `kolpothu_maarchu` (§18.4).
    fn convert(&mut self, name: &str, from: &Ty, to: &Ty, v: Value, span: Span) -> R<Value> {
        let checked = name == "prelude.kudhirithe_maarchu";
        let lossy = name == "prelude.kolpothu_maarchu";
        let as_f64 = |v: &Value| match v {
            Value::F64(x) => *x,
            Value::F32(x) => f64::from(*x),
            Value::Int(i) => *i as f64,
            _ => 0.0,
        };
        let result: Option<Value> = match (from, to) {
            (Ty::Int(_) | Ty::Char, Ty::Int(it)) => {
                let x = v.int();
                let (lo, hi) = it.range();
                if x >= lo && x <= hi {
                    Some(Value::Int(x))
                } else if lossy {
                    Some(Value::Int(wrap(*it, x)))
                } else {
                    None
                }
            }
            (Ty::Float(_), Ty::Int(it)) => {
                let x = as_f64(&v);
                let (lo, hi) = it.range();
                if lossy {
                    Some(Value::Int(if x.is_nan() {
                        0
                    } else if x <= lo as f64 {
                        lo
                    } else if x >= hi as f64 {
                        hi
                    } else {
                        x.trunc() as i128
                    }))
                } else if x.fract() == 0.0 && x >= lo as f64 && x <= hi as f64 {
                    Some(Value::Int(x as i128))
                } else {
                    None
                }
            }
            (Ty::Int(_), Ty::Float(bits)) => {
                let x = v.int();
                if *bits == 32 {
                    let f = x as f32;
                    (lossy || f as i128 == x).then_some(Value::F32(f))
                } else {
                    let f = x as f64;
                    (lossy || f as i128 == x).then_some(Value::F64(f))
                }
            }
            (Ty::Float(_), Ty::Float(32)) => {
                let x = as_f64(&v);
                let f = x as f32;
                (lossy || f64::from(f) == x || x.is_nan()).then_some(Value::F32(f))
            }
            (Ty::Float(_), Ty::Float(_)) => Some(Value::F64(as_f64(&v))),
            (Ty::Int(_), Ty::Char) => char::from_u32(v.int() as u32).map(Value::Char),
            _ => Some(v),
        };
        match (checked, result) {
            (true, r) => Ok(r.map_or_else(Value::none, Value::some)),
            (false, Some(r)) => Ok(r),
            (false, None) => Err(self.trap("T01", format!("`{}` ki maarchalem", super::show_ty(to)), span)),
        }
    }
}

fn file_result(r: std::io::Result<Value>) -> Value {
    match r {
        Ok(v) => Value::ok(v),
        Err(e) => Value::err(thappu_record(e.to_string())),
    }
}

fn strip_ref(t: &Ty) -> &Ty {
    match t {
        Ty::Ref(_, inner) => strip_ref(inner),
        other => other,
    }
}

/// Implementation-defined sizes (§20.6).
fn size_of(t: &Ty) -> i128 {
    match t {
        Ty::Int(i) => i128::from(i.bits / 8),
        Ty::Float(b) => i128::from(b / 8),
        Ty::Bool => 1,
        Ty::Char => 4,
        Ty::Unit | Ty::Never => 0,
        Ty::Tuple(items) => items.iter().map(size_of).sum(),
        Ty::Array(e, n) => size_of(e) * n.unwrap_or(0) as i128,
        _ => 8,
    }
}
