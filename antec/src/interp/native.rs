//! The standard library at run time (spec §21), built-in trait behavior
//! (§13.5–13.6), display, and iterators (§21.4).

use std::rc::Rc;

use super::value::{key_of, Addr, DeepBox, Elem, IterState, OrdMap};
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

/// Rust prints floats in the shortest form that reads back exactly, never
/// with an exponent; whole numbers then get `.0`, however big (§13.5).
fn whole_dot(s: String) -> String {
    if s.contains('.') {
        s
    } else {
        s + ".0"
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
    } else {
        whole_dot(format!("{x}"))
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
    } else {
        whole_dot(format!("{x}"))
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

    pub(super) fn release(&mut self, mark: usize) {
        self.heap.truncate(mark);
    }

    // ── intrinsics ─────────────────────────────────────────────────────────

    /// The next item of the iterator behind the reference `r`, as `undochu`.
    pub(super) fn next_through(&mut self, r: &Value, span: Span) -> R<Value> {
        let a = self.ref_addr(r);
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

    pub(super) fn intrinsic(&mut self, i: &Intrinsic, args: Vec<Value>, span: Span) -> R<Value> {
        match i {
            Intrinsic::IntoIter { copy, ty } => {
                let ty = self.subst(ty);
                let v = args.into_iter().next().unwrap_or(Value::Unit);
                let st = self.make_iter(v, *copy, &ty, span)?;
                Ok(Value::Iter(Box::new(st)))
            }
            Intrinsic::IterNext => self.next_through(&args[0], span),
            Intrinsic::Format { texts, tys } => {
                let mut out = String::new();
                for (i, t) in texts.iter().enumerate() {
                    out.push_str(t);
                    if let (Some(v), Some(ty)) = (args.get(i), tys.get(i)) {
                        let ty = self.subst(ty);
                        self.display_into(&mut out, v, &ty)?;
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
            Value::Vec(xs) | Value::Array(xs) => IterState::Owned(xs.into(), elem_ty(ty)),
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
            IterState::Owned(q, _) => q.pop_front(),
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
            IterState::Zip(a, b, at) => match self.iter_next(a, span)? {
                None => None,
                Some(x) => match self.iter_next(b, span)? {
                    Some(y) => Some(Value::Tuple(vec![x, y])),
                    None => {
                        // The first side's item has no partner: it is dropped.
                        let at = at.clone();
                        self.drop_value(x, &at)?;
                        None
                    }
                },
            },
            IterState::Take(inner, n) => {
                if *n == 0 {
                    None
                } else {
                    *n -= 1;
                    self.iter_next(inner, span)?
                }
            }
            IterState::Skip(inner, n, t) => {
                while *n > 0 {
                    *n -= 1;
                    match self.iter_next(inner, span)? {
                        None => return Ok(None),
                        Some(x) => {
                            let t = t.clone();
                            self.drop_value(x, &t)?;
                        }
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
            IterState::Filter(inner, f, t) => loop {
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
                        let t = t.clone();
                        self.drop_value(x, &t)?;
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
            other => IterState::Owned(vec![other].into(), Ty::Error),
        }
    }

    // ── display (§13.5 `Choopinchochu`) ────────────────────────────────────

    pub(super) fn display(&mut self, v: &Value, ty: &Ty) -> R<String> {
        let mut out = String::new();
        self.display_into(&mut out, v, ty)?;
        Ok(out)
    }

    /// Appends `v` shown (§19.1 built-in display) to `out`. Parts are
    /// borrowed, not copied, and written straight into `out`, so showing a
    /// long list is linear.
    fn display_into(&mut self, out: &mut String, v: &Value, ty: &Ty) -> R<()> {
        self.deeper()?;
        let r = self.display_at(out, v, ty);
        self.value_depth -= 1;
        r
    }

    /// One more level into a value; T08 past `MAX_VALUE_DEPTH`.
    fn deeper(&mut self) -> R<()> {
        self.value_depth += 1;
        if self.value_depth > super::MAX_VALUE_DEPTH {
            self.value_depth = 0;
            return Err(self.trap(
                "T08",
                "stack nindipoyindhi (ee data chaala lothuga undhi)",
                Span::default(),
            ));
        }
        Ok(())
    }

    fn display_at(&mut self, out: &mut String, v: &Value, ty: &Ty) -> R<()> {
        if let Value::Ref(a) = v {
            // Shown where it lives: taken out of its place for the while,
            // not copied (showing never writes, and nothing refers to itself).
            let a = a.clone();
            let t = std::mem::replace(self.read_mut(&a), Value::Uninit);
            let r = self.display_into(out, &t, ty);
            *self.read_mut(&a) = t;
            return r;
        }
        match ty {
            Ty::Int(_) => out.push_str(&v.int().to_string()),
            Ty::Float(32) => match v {
                Value::F32(x) => out.push_str(&fmt_f32(*x)),
                Value::F64(x) => out.push_str(&fmt_f64(*x)),
                _ => {}
            },
            Ty::Float(_) => match v {
                Value::F64(x) => out.push_str(&fmt_f64(*x)),
                Value::F32(x) => out.push_str(&fmt_f32(*x)),
                _ => {}
            },
            Ty::Bool => out.push_str(if matches!(v, Value::Bool(true)) { "avunu" } else { "kaadhu" }),
            // Text shows as itself, also inside records and lists (spec D.2).
            Ty::Char => {
                if let Value::Char(c) = v {
                    out.push(*c);
                }
            }
            Ty::Str => out.push_str(&v.str()),
            Ty::Unit => out.push_str("()"),
            Ty::Ref(_, inner) => self.display_into(out, v, inner)?,
            Ty::Tuple(items) => {
                let xs: &[Value] = match v {
                    Value::Tuple(xs) => xs,
                    _ => &[],
                };
                out.push('(');
                for (k, (x, t)) in xs.iter().zip(items).enumerate() {
                    if k > 0 {
                        out.push_str(", ");
                    }
                    self.display_into(out, x, t)?;
                }
                out.push_str(if xs.len() == 1 { ",)" } else { ")" });
            }
            Ty::Array(..) | Ty::App(Con::Varusa | Con::Mukka, _) => {
                let elem = match ty {
                    Ty::Array(e, _) => (**e).clone(),
                    Ty::App(_, a) => a[0].clone(),
                    _ => Ty::Error,
                };
                let copied;
                let xs: &[Value] = match v {
                    Value::Vec(xs) | Value::Array(xs) | Value::Tuple(xs) => xs,
                    _ => {
                        copied = self.seq(v);
                        &copied
                    }
                };
                out.push('[');
                for (k, x) in xs.iter().enumerate() {
                    if k > 0 {
                        out.push_str(", ");
                    }
                    self.display_into(out, x, &elem)?;
                }
                out.push(']');
            }
            Ty::App(Con::Jodi | Con::GajibijiJodi, a) => {
                let Value::Map(m) = v else {
                    out.push_str("{}");
                    return Ok(());
                };
                out.push('{');
                for (k, i) in m.positions().into_iter().enumerate() {
                    if k > 0 {
                        out.push_str(", ");
                    }
                    self.display_into(out, m.key(i), &a[0])?;
                    out.push_str(": ");
                    self.display_into(out, m.val(i), &a[1])?;
                }
                out.push('}');
            }
            Ty::App(Con::Gumpu, a) => {
                let Value::Map(m) = v else {
                    out.push_str("{}");
                    return Ok(());
                };
                out.push('{');
                for (k, i) in m.positions().into_iter().enumerate() {
                    if k > 0 {
                        out.push_str(", ");
                    }
                    self.display_into(out, m.key(i), &a[0])?;
                }
                out.push('}');
            }
            Ty::App(Con::Undochu, a) => match v {
                Value::Adt(0, xs) => {
                    out.push_str("undhi(");
                    self.display_into(out, &xs[0], &a[0])?;
                    out.push(')');
                }
                _ => out.push_str("ledhu"),
            },
            Ty::App(Con::SareThappu, a) => match v {
                Value::Adt(0, xs) => {
                    out.push_str("sare(");
                    self.display_into(out, &xs[0], &a[0])?;
                    out.push(')');
                }
                Value::Adt(_, xs) => {
                    out.push_str("thappu(");
                    self.display_into(out, &xs[0], &a[1])?;
                    out.push(')');
                }
                _ => {}
            },
            Ty::App(Con::Pette, a) => match v {
                Value::Box(b) => self.display_into(out, b, &a[0])?,
                other => self.display_into(out, other, &a[0])?,
            },
            Ty::App(Con::Paridhi, _) => {
                if let Value::Range(lo, hi, inc, _) = v {
                    out.push_str(&format!("{lo} nunchi {hi} {}", if *inc { "varaku" } else { "lopu" }));
                }
            }
            Ty::App(Con::Adt(_), _) => self.display_adt(out, v, ty)?,
            Ty::Fn(..) => out.push_str("<pani>"),
            _ => match v {
                Value::Int(i) => out.push_str(&i.to_string()),
                Value::Str(s) => out.push_str(s),
                _ => out.push('?'),
            },
        }
        Ok(())
    }

    fn display_adt(&mut self, out: &mut String, v: &Value, ty: &Ty) -> R<()> {
        let Ty::App(Con::Adt(d), args) = ty else { return Ok(()) };
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
                out.push_str(&s.str());
                return Ok(());
            }
        }
        let Some(adt) = g.adts.get(d) else { return Ok(()) };
        let name = adt.name.clone();
        let std_type = adt.file.is_none();
        match (&adt.kind.clone(), v) {
            (AdtKind::Record(fields), Value::Adt(_, xs)) => {
                if std_type && xs.len() == 1 {
                    match &xs[0] {
                        Value::Int(i) => out.push_str(&i.to_string()),
                        other => out.push_str(&other.str()),
                    }
                    return Ok(());
                }
                out.push_str(&name);
                out.push('{');
                for (k, (f, x)) in fields.iter().zip(xs).enumerate() {
                    if k > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&f.name);
                    out.push_str(": ");
                    self.display_into(out, x, &f.ty.subst(args))?;
                }
                out.push('}');
            }
            (AdtKind::Enum(variants), Value::Adt(i, xs)) => {
                let var = &variants[*i as usize];
                out.push_str(&name);
                out.push('.');
                out.push_str(&var.name);
                if !xs.is_empty() {
                    out.push('(');
                    for (k, (t, x)) in var.fields.iter().zip(xs).enumerate() {
                        if k > 0 {
                            out.push_str(", ");
                        }
                        self.display_into(out, x, &t.subst(args))?;
                    }
                    out.push(')');
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── equality, order, hashing (§13.5) ───────────────────────────────────

    /// Calls the user's implementation of a derivable trait, if there is
    /// one; the values are copied only then.
    #[inline(never)]
    fn adt_trait_call(&mut self, trait_name: &str, ty: &Ty, vals: &[&Value]) -> R<Option<Value>> {
        let g = &self.tc.globals;
        let Some(def) = g.std_trait(trait_name) else { return Ok(None) };
        let solver = crate::typeck::solve::Solver { g, generics: &[] };
        let tref = TraitRef { def, args: Vec::new() };
        if solver.find_impl(&mut InferTable::default(), ty, &tref, 0).is_none() {
            return Ok(None);
        }
        let fid = g.traits[&def].methods[0].1;
        let mark = self.heap.len();
        let refs: Vec<Value> = vals.iter().map(|&v| self.scratch(v.clone())).collect();
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

    /// A value's variant and parts, borrowed where they're in `v` itself.
    fn parts<'v>(&self, v: &'v Value) -> (i128, std::borrow::Cow<'v, [Value]>) {
        use std::borrow::Cow;
        match v {
            Value::Adt(i, xs) => (*i as i128, Cow::Borrowed(xs)),
            Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) => (0, Cow::Borrowed(xs)),
            Value::Box(b) => (0, Cow::Borrowed(std::slice::from_ref(&**b))),
            Value::Slice(..) | Value::Ref(_) => (0, Cow::Owned(self.seq(v))),
            _ => (0, Cow::Owned(Vec::new())),
        }
    }

    pub(super) fn values_eq(&mut self, a: &Value, b: &Value, ty: &Ty) -> R<bool> {
        self.deeper()?;
        let r = self.eq_at(a, b, ty);
        self.value_depth -= 1;
        r
    }

    /// `==` on two maps, kept out of `eq_at` so its frame stays small
    /// (that one recurses once per level of a value).
    #[inline(never)]
    fn maps_eq(&mut self, m: &OrdMap, n: &OrdMap, b: &Value, ty: &Ty) -> R<bool> {
        let ty = ty.clone();
        if m.len() != n.len() {
            return Ok(false);
        }
        let vt = match &ty {
            Ty::App(_, args) if args.len() > 1 => args[1].clone(),
            _ => Ty::Error,
        };
        let kt = match &ty {
            Ty::App(_, args) => args.first().cloned().unwrap_or(Ty::Error),
            _ => Ty::Error,
        };
        // Look each key of `a` up in `b`, held where `map_find` can see it.
        let mark = self.heap.len();
        let nb = self.alloc(b.clone());
        let mut same = true;
        for i in m.positions() {
            match self.map_find(&nb, m.key(i), &kt)? {
                (_, Some(j)) => {
                    if !self.values_eq(m.val(i), n.val(j), &vt)? {
                        same = false;
                        break;
                    }
                }
                (_, None) => {
                    same = false;
                    break;
                }
            }
        }
        self.release(mark);
        Ok(same)
    }

    fn eq_at(&mut self, a: &Value, b: &Value, ty: &Ty) -> R<bool> {
        // Parts are compared in place; only a reference is followed by a copy.
        let (ca, cb);
        let a = if let Value::Ref(_) = a {
            ca = self.target(a).clone();
            &ca
        } else {
            a
        };
        let b = if let Value::Ref(_) = b {
            cb = self.target(b).clone();
            &cb
        } else {
            b
        };
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("Polchochu", &ty, &[a, b])? {
                return Ok(matches!(r, Value::Bool(true)));
            }
        }
        Ok(match (&a, &b) {
            (Value::F32(x), Value::F32(y)) => x == y,
            (Value::F64(x), Value::F64(y)) => x == y,
            (Value::Int(_) | Value::Bool(_) | Value::Char(_) | Value::Unit, _) => key_of(a) == key_of(b),
            (Value::Str(_) | Value::Buf(_), _) => a.str() == b.str(),
            (Value::Map(m), Value::Map(n)) => self.maps_eq(m, n, b, &ty)?,
            _ => {
                let (ia, xa) = self.parts(a);
                let (ib, xb) = self.parts(b);
                if ia != ib || xa.len() != xb.len() {
                    return Ok(false);
                }
                let tys = self.field_types(&ty, a);
                for (k, (x, y)) in xa.iter().zip(xb.iter()).enumerate() {
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
        self.deeper()?;
        let r = self.cmp_at(a, b, ty);
        self.value_depth -= 1;
        r
    }

    fn cmp_at(&mut self, a: &Value, b: &Value, ty: &Ty) -> R<std::cmp::Ordering> {
        use std::cmp::Ordering;
        // Parts are compared in place; only a reference is followed by a copy.
        let (ca, cb);
        let a = if let Value::Ref(_) = a {
            ca = self.target(a).clone();
            &ca
        } else {
            a
        };
        let b = if let Value::Ref(_) = b {
            cb = self.target(b).clone();
            &cb
        } else {
            b
        };
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("VarusaPettochu", &ty, &[a, b])? {
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
                let (ia, xa) = self.parts(a);
                let (ib, xb) = self.parts(b);
                if ia != ib {
                    return Ok(ia.cmp(&ib));
                }
                let tys = self.field_types(&ty, a);
                for (k, (x, y)) in xa.iter().zip(xb.iter()).enumerate() {
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
        // Copying recurses on `antec`'s stack; measured first, without.
        if nesting(self.target(v)) > super::MAX_VALUE_DEPTH {
            return Err(self.trap(
                "T08",
                "stack nindipoyindhi (ee data chaala lothuga undhi)",
                Span::default(),
            ));
        }
        let v = self.target(v).clone();
        let ty = strip_ref(ty).clone();
        if matches!(ty, Ty::App(Con::Adt(_), _)) {
            if let Some(r) = self.adt_trait_call("XeroxTheeyochu", &ty, &[&v])? {
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
                // The built-in (and `@automatic`) hashing: by structure.
                let base = self.ref_addr(&arg(1));
                let h = match self.read(&base) {
                    Value::Adt(_, fs) => fs.first().map_or(0, Value::int) as u64,
                    _ => 0,
                };
                let v = self.target(&arg(0)).clone();
                let h = self.hash_structure(h, &v, &ty)?;
                if let Value::Adt(_, fs) = self.read_mut(&base) {
                    if let Some(f) = fs.first_mut() {
                        *f = Value::Int(i128::from(h));
                    }
                }
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
                    "Migulochu.migulu" => crate::ast::BinOp::Rem,
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

    // ── hashing (§13.5 `HashCheyochu`, §21.2 `jodi`) ───────────────────────
    //
    // The same algorithm as native code (codegen/map.rs), so `Hasher` results
    // agree: integers (as 64 bits), `ato_ito`, `aksharam` and float bits mix
    // in with `mix`; text is FNV-1a after mixing in its length; sequences mix
    // their length, then the items; enums their variant index, then fields.
    // A type with its own `HashCheyochu` hashes the way it says.

    /// Mixes `v` (of type `ty`) into `h`.
    pub(super) fn key_hash(&mut self, h: u64, v: &Value, ty: &Ty) -> R<u64> {
        let v = self.target(v).clone();
        let ty = strip_ref(ty).clone();
        if let Ty::App(Con::Adt(_), _) = ty {
            if let Some(h2) = self.hash_with_impl(h, &v, &ty)? {
                return Ok(h2);
            }
        }
        self.hash_structure(h, &v, &ty)
    }

    /// Runs the type's `hash_cheyyi` on a `Hasher` holding `h`, if it has one.
    fn hash_with_impl(&mut self, h: u64, v: &Value, ty: &Ty) -> R<Option<u64>> {
        let g = &self.tc.globals;
        let Some(def) = g.std_trait("HashCheyochu") else { return Ok(None) };
        let solver = crate::typeck::solve::Solver { g, generics: &[] };
        let tref = TraitRef { def, args: Vec::new() };
        if solver.find_impl(&mut InferTable::default(), ty, &tref, 0).is_none() {
            return Ok(None);
        }
        let fid = g.traits[&def].methods[0].1;
        let mark = self.heap.len();
        let this = self.scratch(v.clone());
        let hasher = self.alloc(Value::Adt(0, vec![Value::Int(i128::from(h))]));
        self.call_trait(fid, vec![ty.clone()], vec![this, Value::Ref(hasher.clone())], Span::default())?;
        let out = match self.read(&hasher) {
            Value::Adt(_, fs) => fs.first().map_or(0, Value::int) as u64,
            _ => h,
        };
        self.release(mark);
        Ok(Some(out))
    }

    /// Hashes by structure (the built-in and `@automatic` `HashCheyochu`).
    fn hash_structure(&mut self, h: u64, v: &Value, ty: &Ty) -> R<u64> {
        let mix = |h: u64, x: u64| (h.rotate_left(5) ^ x).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Ok(match (v, ty) {
            (Value::Int(x), Ty::Int(it)) => mix(h, if it.signed { *x as i64 as u64 } else { *x as u64 }),
            (Value::Int(x), _) => mix(h, *x as u64),
            (Value::Bool(b), _) => mix(h, u64::from(*b)),
            (Value::Char(c), _) => mix(h, u64::from(*c as u32)),
            (Value::F32(x), _) => mix(h, u64::from(x.to_bits())),
            (Value::F64(x), _) => mix(h, x.to_bits()),
            (Value::Unit, _) => h,
            (Value::Str(_) | Value::Buf(_), _) => {
                let text = v.str();
                let mut h = h ^ text.len() as u64;
                for b in text.bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0100_0000_01b3);
                }
                h
            }
            (Value::Box(b), _) => {
                let inner = match ty {
                    Ty::App(Con::Pette, a) => a[0].clone(),
                    other => other.clone(),
                };
                let b = (**b).clone();
                self.key_hash(h, &b, &inner)?
            }
            (Value::Tuple(_) | Value::Array(_) | Value::Vec(_) | Value::Slice(..), _) => {
                let xs = self.seq(v);
                let tys = self.field_types(ty, v);
                let mut h = if matches!(v, Value::Tuple(_)) { h } else { mix(h, xs.len() as u64) };
                for (i, x) in xs.iter().enumerate() {
                    let t = tys.get(i).or(tys.first()).cloned().unwrap_or(Ty::Error);
                    h = self.key_hash(h, x, &t)?;
                }
                h
            }
            (Value::Adt(tag, xs), _) => {
                let enum_like = match ty {
                    Ty::App(Con::Undochu | Con::SareThappu, _) => true,
                    Ty::App(Con::Adt(d), _) => {
                        matches!(self.tc.globals.adts.get(d).map(|a| &a.kind), Some(AdtKind::Enum(_)))
                    }
                    _ => false,
                };
                let mut h = if enum_like { mix(h, u64::from(*tag)) } else { h };
                let tys = self.field_types(ty, v);
                for (i, x) in xs.clone().iter().enumerate() {
                    let t = tys.get(i).cloned().unwrap_or(Ty::Error);
                    h = self.key_hash(h, x, &t)?;
                }
                h
            }
            _ => h,
        })
    }

    /// Where key `k` (of type `kt`) is in the map at `base`, and its hash.
    pub(super) fn map_find(&mut self, base: &Addr, k: &Value, kt: &Ty) -> R<(u64, Option<usize>)> {
        let h = self.key_hash(MAP_SEED, k, kt)?;
        let found: Vec<(usize, Value)> = match self.read(base) {
            Value::Map(m) => m.with_hash(h).into_iter().map(|i| (i, m.key(i).clone())).collect(),
            _ => Vec::new(),
        };
        for (i, c) in found {
            if self.values_eq(&c, k, kt)? {
                return Ok((h, Some(i)));
            }
        }
        Ok((h, None))
    }

    /// `cherchu`: inserts, or replaces the value of an equal key (which keeps
    /// its place); returns the old value.
    pub(super) fn map_insert(&mut self, base: &Addr, k: Value, v: Value, kt: &Ty) -> R<Option<Value>> {
        let (h, at) = self.map_find(base, &k, kt)?;
        let (old, unused) = match self.read_mut(base) {
            Value::Map(m) => match at {
                Some(i) => (Some(std::mem::replace(m.val_mut(i), v)), Some(k)),
                None => {
                    m.push(h, k, v);
                    (None, None)
                }
            },
            _ => (None, None),
        };
        // The key passed in isn't kept when an equal one is there already.
        if let Some(k) = unused {
            self.drop_value(k, kt)?;
        }
        Ok(old)
    }

    // ── standard functions (§21) ───────────────────────────────────────────

    /// A standard function. A `pani` passed by value is released afterwards:
    /// its captures drop unless the result kept a handle (§10.7.4).
    pub(super) fn native(&mut self, name: &str, tys: &[Ty], args: Vec<Value>, span: Span) -> R<Value> {
        let closures: Vec<Value> = args.iter().filter(|v| matches!(v, Value::Closure(_))).cloned().collect();
        let r = self.native_call(name, tys, args, span);
        for c in closures {
            self.drop_value(c, &Ty::Fn(Vec::new(), Box::new(Ty::Unit)))?;
        }
        r
    }

    /// Drops an iterator and whatever it still owns, as native code does.
    pub fn drop_iter(&mut self, it: IterState) -> R<()> {
        match it {
            IterState::Owned(q, t) => {
                for x in q {
                    self.drop_value(x, &t)?;
                }
            }
            IterState::Zip(a, b, _) => {
                self.drop_iter(*a)?;
                self.drop_iter(*b)?;
            }
            IterState::Take(i, _) | IterState::Skip(i, _, _) | IterState::Enumerate(i, _) => {
                self.drop_iter(*i)?
            }
            IterState::Map2(i, f) | IterState::Filter(i, f, _) => {
                self.drop_iter(*i)?;
                self.drop_value(f, &Ty::Fn(Vec::new(), Box::new(Ty::Unit)))?;
            }
            IterState::User { value, tys, .. } => {
                if let Some(t) = tys.first() {
                    let t = t.clone();
                    self.drop_value(*value, &t)?;
                }
            }
            IterState::Range { .. }
            | IterState::Seq { .. }
            | IterState::Chars { .. }
            | IterState::Map { .. } => {}
        }
        Ok(())
    }

    #[allow(clippy::cognitive_complexity)]
    fn native_call(&mut self, name: &str, tys: &[Ty], mut args: Vec<Value>, span: Span) -> R<Value> {
        macro_rules! arg {
            ($i:expr) => {
                args.get($i).cloned().unwrap_or(Value::Unit)
            };
        }
        // An argument the native keeps is moved out, not copied: copying a
        // long list on every push would be quadratic.
        macro_rules! take {
            ($i:expr) => {
                args.get_mut($i).map(|v| std::mem::replace(v, Value::Uninit)).unwrap_or(Value::Unit)
            };
        }
        let ty = |i: usize| tys.get(i).cloned().unwrap_or(Ty::Error);
        match name {
            // ── prelude ──
            "prelude.choopinchu" | "prelude.cheppu" | "prelude.gola_pettu" => {
                let mut s = self.display(&arg!(0), &ty(0))?;
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
                if matches!(arg!(0), Value::Bool(false)) {
                    let msg = arg!(1).str();
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
                let msg = arg!(0).str();
                Err(self.trap(
                    "T05",
                    if msg.is_empty() { "kompa_munigindhi()".to_string() } else { msg.to_string() },
                    span,
                ))
            }
            "prelude.maarchu" | "prelude.kudhirithe_maarchu" | "prelude.kolpothu_maarchu" => {
                self.convert(name, &ty(1), &ty(0), arg!(0), span)
            }
            "prelude.size_of" | "prelude.align_of" => Ok(Value::Int(size_of(&ty(0)))),
            "prelude.kalipi" => {
                let a = self.take_iter(arg!(0));
                let b = self.take_iter(arg!(1));
                Ok(Value::Iter(Box::new(IterState::Zip(Box::new(a), Box::new(b), ty(0)))))
            }
            "prelude.modhati" | "prelude.dhaatu" => {
                let it = self.take_iter(arg!(0));
                let n = arg!(1).int().max(0) as usize;
                let st = if name == "prelude.modhati" {
                    IterState::Take(Box::new(it), n)
                } else {
                    IterState::Skip(Box::new(it), n, ty(0))
                };
                Ok(Value::Iter(Box::new(st)))
            }
            "prelude.prathi_dhaaniki" => {
                let it = self.take_iter(arg!(0));
                Ok(Value::Iter(Box::new(IterState::Map2(Box::new(it), arg!(1)))))
            }
            "prelude.jalleda_pattu" => {
                let it = self.take_iter(arg!(0));
                Ok(Value::Iter(Box::new(IterState::Filter(Box::new(it), arg!(1), ty(0)))))
            }
            "prelude.number_vesi" => {
                let it = self.take_iter(arg!(0));
                Ok(Value::Iter(Box::new(IterState::Enumerate(Box::new(it), 0))))
            }
            "prelude.poguchey" => {
                let mut it = self.take_iter(arg!(0));
                let mut acc = arg!(1);
                let f = arg!(2);
                while let Some(x) = self.iter_next(&mut it, span)? {
                    acc = self.call_value(&f, vec![acc, x], span)?;
                }
                self.drop_iter(it)?;
                Ok(acc)
            }
            "prelude.varusa_ga" => {
                let mut it = self.take_iter(arg!(0));
                let mut out = Vec::new();
                while let Some(x) = self.iter_next(&mut it, span)? {
                    out.push(x);
                }
                self.drop_iter(it)?;
                Ok(Value::Vec(out))
            }
            "prelude.anni_theesuko" | "varusa.anni_theesuko" => match take!(0) {
                Value::Vec(xs) | Value::Array(xs) => {
                    // A method's first type is its receiver's (`idhi`).
                    let t = if name.starts_with("varusa.") { ty(1) } else { ty(0) };
                    Ok(Value::Iter(Box::new(IterState::Owned(xs.into(), t))))
                }
                other => Ok(other),
            },

            // ── padham ──
            "padham.enni" => Ok(Value::Int(self.arg_str(&arg!(0)).chars().count() as i128)),
            "padham.byte_len" => Ok(Value::Int(self.arg_str(&arg!(0)).len() as i128)),
            "padham.aksharaalu" => {
                let text = self.arg_str(&arg!(0));
                Ok(Value::Iter(Box::new(IterState::Chars { text, pos: 0 })))
            }
            "padham.vidagottu" => {
                let s = self.arg_str(&arg!(0));
                let sep = self.arg_str(&arg!(1));
                let parts: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::Str(Rc::from(c.to_string()))).collect()
                } else {
                    s.split(&*sep).map(|p| Value::Str(Rc::from(p))).collect()
                };
                Ok(Value::Vec(parts))
            }
            "padham.kathirinchu" => Ok(Value::Str(Rc::from(self.arg_str(&arg!(0)).trim()))),
            "padham.tho_modhalaindha" => {
                let (s, p) = (self.arg_str(&arg!(0)), self.arg_str(&arg!(1)));
                Ok(bool_v(s.starts_with(&*p)))
            }
            "padham.lo_undha" => {
                let (s, p) = (self.arg_str(&arg!(0)), self.arg_str(&arg!(1)));
                Ok(bool_v(s.contains(&*p)))
            }
            "padham.ankam_ga" | "padham.aasajeevi_ga" => {
                let s = self.arg_str(&arg!(0));
                let base = arg!(1).int() as u32;
                let it =
                    if name == "padham.ankam_ga" { IntTy::ANKAM } else { IntTy { signed: false, bits: 64 } };
                let parsed = (2..=36).contains(&base).then(|| i128::from_str_radix(&s, base).ok()).flatten();
                Ok(match parsed {
                    Some(v) if v >= it.range().0 && v <= it.range().1 => Value::ok(Value::Int(v)),
                    _ => Value::err(thappu_record(format!("`{s}` {} kaadhu", int_name(it)))),
                })
            }
            "padham.amsam_ga" => {
                let s = self.arg_str(&arg!(0));
                Ok(match s.trim().parse::<f64>() {
                    Ok(x) => Value::ok(Value::F64(x)),
                    Err(_) => Value::err(thappu_record(format!("`{s}` amsam kaadhu"))),
                })
            }
            "padham.baitlu" => {
                let s = self.arg_str(&arg!(0));
                let bytes: Vec<Value> = s.bytes().map(|b| Value::Int(i128::from(b))).collect();
                let n = bytes.len();
                let a = self.alloc(Value::Array(bytes));
                Ok(Value::Slice(a, 0, n))
            }
            "padham.baitlu_nunchi" => {
                let bytes: Vec<u8> = self.seq(&arg!(0)).iter().map(|v| v.int() as u8).collect();
                Ok(match String::from_utf8(bytes) {
                    Ok(s) => Value::ok(Value::Str(Rc::from(s))),
                    Err(e) => Value::err(thappu_record(format!("UTF-8 kaadhu: {e}"))),
                })
            }
            "padham.byte_mukka" => {
                let s = self.arg_str(&arg!(0));
                let (a, b) = (arg!(1).int(), arg!(2).int());
                let part = (a >= 0 && b >= a).then(|| s.get(a as usize..b as usize)).flatten();
                Ok(match part {
                    Some(p) => Value::ok(Value::Str(Rc::from(p))),
                    None => Value::err(thappu_record(format!("{a}..{b} akshara haddu meeda ledhu"))),
                })
            }

            // ── varusa and mukka ──
            "varusa.kotthadhi" => Ok(Value::Vec(Vec::new())),
            "varusa.enni" | "mukka.enni" => Ok(Value::Int(self.seq_len_of(&arg!(0)) as i128)),
            "varusa.khaaliyaa" | "mukka.khaaliyaa" => Ok(bool_v(self.seq_len_of(&arg!(0)) == 0)),
            "varusa.cherchu" => {
                let x = take!(1);
                self.with_target_mut(&arg!(0), |v| {
                    if let Value::Vec(xs) = v {
                        xs.push(x);
                    }
                });
                Ok(Value::Unit)
            }
            "varusa.theesey" => {
                let popped = self.with_target_mut(&arg!(0), |v| match v {
                    Value::Vec(xs) => xs.pop(),
                    _ => None,
                });
                Ok(popped.map_or_else(Value::none, Value::some))
            }
            "varusa.thechu" | "varusa.maare_thechu" | "mukka.thechu" => {
                Ok(match self.elem_addr(&arg!(0), arg!(1).int()) {
                    Some(a) => Value::some(Value::Ref(a)),
                    None => Value::none(),
                })
            }
            "varusa.sort_cheyyi" | "varusa.ila_sort_cheyyi" => {
                let a = self.ref_addr(&arg!(0));
                let xs = match std::mem::replace(self.read_mut(&a), Value::Uninit) {
                    Value::Vec(xs) => xs,
                    other => {
                        *self.read_mut(&a) = other;
                        return Ok(Value::Unit);
                    }
                };
                let elem = ty(1);
                let f = arg!(1);
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
            "jodi.cherchu" | "gajibiji_jodi.cherchu" | "gumpu.cherchu" => {
                let base = self.ref_addr(&arg!(0));
                let kt = map_key_ty(tys);
                let set = name == "gumpu.cherchu";
                let v = if set { Value::Unit } else { take!(2) };
                let old = self.map_insert(&base, take!(1), v, &kt)?;
                if set {
                    return Ok(bool_v(old.is_none()));
                }
                Ok(old.map_or_else(Value::none, Value::some))
            }
            "jodi.thechu" | "jodi.maare_thechu" | "gajibiji_jodi.thechu" => {
                let k = self.target(&arg!(1)).clone();
                let base = self.ref_addr(&arg!(0));
                let (_, found) = self.map_find(&base, &k, &map_key_ty(tys))?;
                Ok(match found {
                    Some(i) => Value::some(Value::Ref(base.with(Elem::MapVal(i)))),
                    None => Value::none(),
                })
            }
            "jodi.lekapothe_cherchu" => {
                let (k, d) = (take!(1), take!(2));
                let base = self.ref_addr(&arg!(0));
                let kt = map_key_ty(tys);
                let (h, found) = self.map_find(&base, &k, &kt)?;
                let i = match (found, self.read_mut(&base)) {
                    (Some(i), _) => {
                        // Neither the key nor the default is kept.
                        self.drop_value(k, &kt)?;
                        let vt = map_val_ty(tys);
                        self.drop_value(d, &vt)?;
                        i
                    }
                    (None, Value::Map(m)) => m.push(h, k, d),
                    _ => 0,
                };
                Ok(Value::Ref(base.with(Elem::MapVal(i))))
            }
            "jodi.theesey" | "gajibiji_jodi.theesey" | "gumpu.theesey" => {
                let k = self.target(&arg!(1)).clone();
                let base = self.ref_addr(&arg!(0));
                let (h, found) = self.map_find(&base, &k, &map_key_ty(tys))?;
                let old = match (found, self.read_mut(&base)) {
                    (Some(i), Value::Map(m)) => m.remove_at(h, i),
                    _ => None,
                };
                if name == "gumpu.theesey" {
                    return Ok(bool_v(old.is_some()));
                }
                Ok(old.map_or_else(Value::none, |(_, v)| Value::some(v)))
            }
            "jodi.undhaa" | "gajibiji_jodi.undhaa" | "gumpu.undhaa" => {
                let k = self.target(&arg!(1)).clone();
                let base = self.ref_addr(&arg!(0));
                Ok(bool_v(self.map_find(&base, &k, &map_key_ty(tys))?.1.is_some()))
            }
            "jodi.enni" | "gajibiji_jodi.enni" | "gumpu.enni" => {
                Ok(Value::Int(match self.target(&arg!(0)) {
                    Value::Map(m) => m.len() as i128,
                    _ => 0,
                }))
            }

            // ── undochu, sare_thappu, pette ──
            "undochu.undhaa" => Ok(bool_v(matches!(self.target(&arg!(0)), Value::Adt(0, _)))),
            "undochu.ledhaa" => Ok(bool_v(matches!(self.target(&arg!(0)), Value::Adt(1, _)))),
            "undochu.lekapothe" => Ok(match take!(0) {
                Value::Adt(0, mut xs) => {
                    // The unused default is dropped.
                    self.drop_value(take!(1), &ty(1))?;
                    xs.pop().unwrap_or(Value::Unit)
                }
                _ => take!(1),
            }),
            "undochu.kachithanga" => match take!(0) {
                Value::Adt(0, mut xs) => Ok(xs.pop().unwrap_or(Value::Unit)),
                _ => Err(self.trap("T06", "`ledhu` meeda kachithanga() pilicharu", span)),
            },
            "undochu.prathi_dhaaniki" => match take!(0) {
                Value::Adt(0, mut xs) => {
                    let x = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::some(self.call_value(&arg!(1), vec![x], span)?))
                }
                other => Ok(other),
            },
            "sare_thappu.sareyaa" => Ok(bool_v(matches!(self.target(&arg!(0)), Value::Adt(0, _)))),
            "sare_thappu.kachithanga" => match take!(0) {
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
            "sare_thappu.thappu_maarchu" => match take!(0) {
                Value::Adt(1, mut xs) => {
                    let e = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::err(self.call_value(&arg!(1), vec![e], span)?))
                }
                other => Ok(other),
            },
            "sare_thappu.prathi_dhaaniki" => match take!(0) {
                Value::Adt(0, mut xs) => {
                    let x = xs.pop().unwrap_or(Value::Unit);
                    Ok(Value::ok(self.call_value(&arg!(1), vec![x], span)?))
                }
                other => Ok(other),
            },
            "pette.kotthadhi" => Ok(Value::Box(DeepBox::new(take!(0)))),

            // ── scalars ──
            "aksharam.digit_aa" | "aksharam.letter_aa" | "aksharam.space_aa" | "aksharam.alnum_aa"
            | "aksharam.ascii_aa" => {
                let c = match self.target(&arg!(0)) {
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
                let v = self.target(&arg!(0)).int();
                let it = match ty(0) {
                    Ty::Int(it) => it,
                    _ => IntTy::ANKAM,
                };
                let bits = u32::from(it.bits);
                let unsigned = if bits >= 128 { v as u128 } else { (v as u128) & ((1u128 << bits) - 1) };
                Ok(Value::Str(Rc::from(format!("{unsigned:x}"))))
            }
            "amsam64.bits" => Ok(Value::Int(match self.target(&arg!(0)) {
                Value::F64(x) => i128::from(x.to_bits()),
                _ => 0,
            })),
            "amsam32.bits" => Ok(Value::Int(match self.target(&arg!(0)) {
                Value::F32(x) => i128::from(x.to_bits()),
                _ => 0,
            })),
            "amsam64.bits_nunchi" => Ok(Value::F64(f64::from_bits(arg!(0).int() as u64))),
            "amsam32.bits_nunchi" => Ok(Value::F32(f32::from_bits(arg!(0).int() as u32))),

            // ── Raatha, Hasher ──
            "Raatha.kotthadhi" => Ok(Value::Adt(0, vec![Value::Buf(String::new())])),
            "Raatha.raayi" | "Raatha.akshara_raayi" => {
                let text = if name == "Raatha.raayi" {
                    self.display(&arg!(1), &ty(1))?
                } else {
                    match arg!(1) {
                        Value::Char(c) => c.to_string(),
                        _ => String::new(),
                    }
                };
                self.with_target_mut(&arg!(0), |r| {
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
            "Raatha.enni" => Ok(Value::Int(match self.target(&arg!(0)) {
                Value::Adt(_, fs) => fs.first().map_or(0, |f| f.str().chars().count()) as i128,
                _ => 0,
            })),
            "Raatha.padham_ga" => {
                let s = self.with_target_mut(&arg!(0), |r| match r {
                    Value::Adt(_, fs) => match fs.first_mut() {
                        Some(f) => std::mem::replace(f, Value::Buf(String::new())).str(),
                        None => Rc::from(""),
                    },
                    _ => Rc::from(""),
                });
                Ok(Value::Str(s))
            }
            "Hasher.kalupu" => {
                let base = self.ref_addr(&arg!(0));
                let h = match self.read(&base) {
                    Value::Adt(_, fs) => fs.first().map_or(0, Value::int) as u64,
                    _ => 0,
                };
                let h = self.key_hash(h, &arg!(1), &ty(1))?;
                if let Value::Adt(_, fs) = self.read_mut(&base) {
                    if let Some(f) = fs.first_mut() {
                        *f = Value::Int(i128::from(h));
                    }
                }
                Ok(Value::Unit)
            }
            "Hasher.chaalu" => Ok(match self.target(&arg!(0)) {
                Value::Adt(_, fs) => fs.first().cloned().unwrap_or(Value::Int(0)),
                _ => Value::Int(0),
            }),

            // ── lekkalu ──
            n if n.starts_with("lekkalu.") => self.lekkalu(&n["lekkalu.".len()..], tys, args, span),

            // ── file ──
            "file.chadhuvu" => {
                let p = self.arg_str(&arg!(0));
                Ok(open_result(&p, std::fs::read_to_string(&*p).map(|s| Value::Str(Rc::from(s)))))
            }
            "file.raayi" => {
                let (p, t) = (self.arg_str(&arg!(0)), self.arg_str(&arg!(1)));
                Ok(open_result(&p, std::fs::write(&*p, t.as_bytes()).map(|_| Value::Unit)))
            }
            "file.baitlu_chadhuvu" => {
                let p = self.arg_str(&arg!(0));
                Ok(open_result(
                    &p,
                    std::fs::read(&*p)
                        .map(|b| Value::Vec(b.into_iter().map(|x| Value::Int(i128::from(x))).collect())),
                ))
            }
            "file.baitlu_raayi" => {
                let p = self.arg_str(&arg!(0));
                let bytes: Vec<u8> = self.seq(&arg!(1)).iter().map(|v| v.int() as u8).collect();
                Ok(open_result(&p, std::fs::write(&*p, bytes).map(|_| Value::Unit)))
            }
            "file.undhaa" => Ok(bool_v(std::path::Path::new(&*self.arg_str(&arg!(0))).exists())),
            "file.folderaa" => Ok(bool_v(std::path::Path::new(&*self.arg_str(&arg!(0))).is_dir())),
            "file.list_cheyyi" => {
                let p = self.arg_str(&arg!(0));
                Ok(file_result(std::fs::read_dir(&*p).map(|rd| {
                    let mut names: Vec<String> = rd
                        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                        .collect();
                    names.sort();
                    Value::Vec(names.into_iter().map(|n| Value::Str(Rc::from(n))).collect())
                })))
            }
            "file.folder_cheyyi" => {
                Ok(file_result(std::fs::create_dir_all(&*self.arg_str(&arg!(0))).map(|_| Value::Unit)))
            }
            "file.theesey" => {
                Ok(file_result(std::fs::remove_file(&*self.arg_str(&arg!(0))).map(|_| Value::Unit)))
            }
            "file.path_kalupu" => {
                // Joined with `/` on every system, the same as the native runtime.
                let (a, b) = (self.arg_str(&arg!(0)), self.arg_str(&arg!(1)));
                let sep = |c: char| c == '/' || (cfg!(windows) && c == '\\');
                let joined = if b.starts_with(sep) || a.is_empty() {
                    b.to_string()
                } else if a.ends_with(sep) {
                    format!("{a}{b}")
                } else {
                    format!("{a}/{b}")
                };
                Ok(Value::Str(Rc::from(joined)))
            }
            "file.path_peru" | "file.path_folder" | "file.path_extension" => {
                let p = self.arg_str(&arg!(0));
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
                Ok(Value::Vec(self.args.iter().map(|a| Value::Str(Rc::from(a.as_str()))).collect()))
            }
            "system.env" => Ok(match std::env::var(&*self.arg_str(&arg!(0))) {
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
            "system.vellipo" => Err(Halt::Exit(arg!(0).int() as i32)),
            "system.nadupu" => {
                let prog = self.arg_str(&arg!(0));
                let argv: Vec<String> =
                    self.seq(&arg!(1)).iter().map(|v| self.target(v).str().to_string()).collect();
                Ok(match std::process::Command::new(&*prog).args(&argv).output() {
                    Ok(o) => Value::ok(Value::Adt(
                        0,
                        vec![
                            Value::Int(i128::from(o.status.code().unwrap_or(-1))),
                            Value::Str(Rc::from(String::from_utf8_lossy(&o.stdout).into_owned())),
                            Value::Str(Rc::from(String::from_utf8_lossy(&o.stderr).into_owned())),
                        ],
                    )),
                    Err(e) => Value::err(thappu_record(io_text(&e))),
                })
            }
            "system.os" => Ok(Value::Str(Rc::from(std::env::consts::OS))),
            "system.arch" => Ok(Value::Str(Rc::from(std::env::consts::ARCH))),

            // ── samayam ──
            "samayam.ippudu" => {
                Ok(Value::Adt(0, vec![Value::Int(self.started.elapsed().as_nanos() as i128)]))
            }
            "Kshanam.gadichindhi" => {
                let then = match self.target(&arg!(0)) {
                    Value::Adt(_, fs) => fs.first().map_or(0, Value::int),
                    _ => 0,
                };
                let now = self.started.elapsed().as_nanos() as i128;
                Ok(Value::Adt(0, vec![Value::Int(now - then)]))
            }
            "Kaalam.sec" => {
                let n = arg!(0).int();
                match n.checked_mul(1_000_000_000).filter(|ns| (0..=u64::MAX as i128).contains(ns)) {
                    Some(ns) => Ok(Value::Adt(0, vec![Value::Int(ns)])),
                    None => Err(self.trap("T01", format!("`Kaalam` lo {n} sec pattadhu"), span)),
                }
            }
            "Kaalam.millisec" | "Kaalam.nanosec" => {
                let ns = match self.target(&arg!(0)) {
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

/// How deeply `v` nests, found without recursing.
fn nesting(v: &Value) -> usize {
    let mut deepest = 0;
    let mut work = vec![(v, 1usize)];
    while let Some((v, d)) = work.pop() {
        deepest = deepest.max(d);
        match v {
            Value::Box(b) => work.push((b, d + 1)),
            Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) | Value::Adt(_, xs) => {
                work.extend(xs.iter().map(|x| (x, d + 1)))
            }
            _ => {}
        }
    }
    deepest
}

fn file_result(r: std::io::Result<Value>) -> Value {
    match r {
        Ok(v) => Value::ok(v),
        Err(e) => Value::err(thappu_record(io_text(&e))),
    }
}

/// Like `file_result`, for opening `path` as a file. Windows refuses to open a
/// folder with "access denied" where POSIX says "is a directory"; both read as
/// the folder error, the same as `open_error` in `ante_rt.c`.
fn open_result(path: &str, r: std::io::Result<Value>) -> Value {
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && std::path::Path::new(path).is_dir() => {
            Value::err(thappu_record(io_text(&std::io::ErrorKind::IsADirectory.into())))
        }
        r => file_result(r),
    }
}

/// The text of an I/O error. The common ones have fixed wording, shared with
/// the native runtime (`io_error` in `ante_rt.c`); others use the system's
/// message, without Rust's ` (os error N)` suffix.
pub fn io_text(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "aa file ledhu".into(),
        K::PermissionDenied => "permission ledhu".into(),
        K::IsADirectory => "adhi file kaadhu, folder".into(),
        K::NotADirectory => "path lo oka bhaagam folder kaadhu".into(),
        K::AlreadyExists => "adhi mundhe undhi".into(),
        K::StorageFull => "disk nindipoyindhi".into(),
        K::InvalidData => "file lo text valid UTF-8 kaadhu".into(),
        _ => {
            let text = e.to_string();
            match text.rfind(" (os error ") {
                Some(i) => text[..i].to_string(),
                None => text,
            }
        }
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

/// The element type of an owned sequence type.
fn elem_ty(ty: &Ty) -> Ty {
    match ty {
        Ty::Array(e, _) => (**e).clone(),
        Ty::App(_, a) if !a.is_empty() => a[0].clone(),
        _ => Ty::Error,
    }
}

/// The seed for map keys: the FNV offset basis (the same in native code).
pub(super) const MAP_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// The value type of a map method's type arguments.
fn map_val_ty(tys: &[Ty]) -> Ty {
    for t in tys {
        if let Ty::App(Con::Jodi | Con::GajibijiJodi, a) = strip_ref(t) {
            return a.get(1).cloned().unwrap_or(Ty::Error);
        }
    }
    tys.get(2).cloned().unwrap_or(Ty::Error)
}

/// The key type of a map method's type arguments (the map type comes first).
fn map_key_ty(tys: &[Ty]) -> Ty {
    for t in tys {
        if let Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, a) = strip_ref(t) {
            return a.first().cloned().unwrap_or(Ty::Error);
        }
    }
    tys.get(1).cloned().unwrap_or(Ty::Error)
}
