//! Patterns (spec §14): typing with binding modes, irrefutability, and
//! exhaustiveness by the usefulness algorithm (E0701, W0004).

use super::body::BodyCx;
use super::AdtKind;
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::types::{Con, Ty, VarKind};

impl BodyCx<'_, '_> {
    /// Types `p` against `ty` and declares its bindings. `irrefutable` is
    /// set for bindings and `for` loops (§14.1).
    pub fn check_pattern(&mut self, p: &Pattern, ty: &Ty, irrefutable: bool) {
        if irrefutable && is_refutable(p) {
            self.error(
                "E0703",
                p.span,
                "ee pattern anni values ki match avvadhu. `batti` tho anni cases handle cheyyandi",
            );
        }
        self.pat(p, ty, None);
    }

    /// `through`: `None` when matching a value, `Some(mutable)` when matching
    /// through a reference (§14.2).
    fn pat(&mut self, p: &Pattern, ty: &Ty, through: Option<bool>) {
        let t = self.shallow(ty);
        if !matches!(p.kind, PatternKind::Bind { .. } | PatternKind::Wild) {
            if let Ty::Ref(m, inner) = &t {
                let mode = Some(through.map_or(*m, |o| o && *m));
                return self.pat(p, inner, mode);
            }
        }
        match &p.kind {
            PatternKind::Wild => {}
            PatternKind::Bind { mutable, name } => {
                let bt = match through {
                    None => t,
                    Some(m) => {
                        if self.is_copy(&t) {
                            t
                        } else {
                            Ty::reference(m, t)
                        }
                    }
                };
                self.declare(name, bt, *mutable);
            }
            PatternKind::Literal(e) => {
                let lt = self.check_expr(e, Some(&t));
                self.unify_or_report(e.span, &t, &lt);
            }
            PatternKind::Range { lo, hi, .. } => {
                let lt = self.check_expr(lo, Some(&t));
                self.unify_or_report(lo.span, &t, &lt);
                let ht = self.check_expr(hi, Some(&t));
                self.unify_or_report(hi.span, &t, &ht);
            }
            PatternKind::Ctor { path, args } => self.pat_ctor(p.span, path, args.as_deref(), &t, through),
            PatternKind::Record { path, fields, rest } => {
                self.pat_record(p.span, path, fields, *rest, &t, through)
            }
            PatternKind::Tuple(items) => {
                let parts: Vec<Ty> = items.iter().map(|_| self.fresh()).collect();
                if !self.unify_or_report(p.span, &t, &Ty::Tuple(parts.clone())) {
                    return;
                }
                for (item, pt) in items.iter().zip(parts) {
                    self.pat(item, &pt, through);
                }
            }
            PatternKind::Or(alts) => {
                for a in alts {
                    self.pat(a, &t, through);
                }
            }
        }
    }

    fn pat_ctor(&mut self, span: Span, path: &Path, args: Option<&[Pattern]>, t: &Ty, through: Option<bool>) {
        let name = path.segments[0].name.as_str();
        if path.segments.len() == 1 && matches!(name, "undhi" | "ledhu" | "sare" | "thappu") {
            let (con, slot, arity) = match name {
                "undhi" => (Con::Undochu, 0, 1),
                "ledhu" => (Con::Undochu, 0, 0),
                "sare" => (Con::SareThappu, 0, 1),
                _ => (Con::SareThappu, 1, 1),
            };
            let n = if con == Con::Undochu { 1 } else { 2 };
            let parts: Vec<Ty> = (0..n).map(|_| self.infer.fresh(VarKind::General)).collect();
            if !self.unify_or_report(span, t, &Ty::App(con, parts.clone())) {
                return;
            }
            let given = args.map_or(0, <[Pattern]>::len);
            if given != arity && !(name == "sare" && given == 0) {
                self.error("E0704", span, format!("`{name}` pattern ki {arity} value(s) kavali"));
            }
            for a in args.unwrap_or(&[]) {
                self.pat(a, &parts[slot], through);
            }
            return;
        }
        let Some((d, vi)) = self.variant_of(path) else { return };
        let g = self.g;
        let adt = &g.adts[&d];
        let AdtKind::Enum(variants) = &adt.kind else { return };
        let v = &variants[vi];
        let fresh: Vec<Ty> = adt.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect();
        if !self.unify_or_report(span, t, &Ty::App(Con::Adt(d), fresh.clone())) {
            return;
        }
        let given = args.unwrap_or(&[]);
        if given.len() != v.fields.len() {
            self.error(
                "E0704",
                span,
                format!(
                    "`{}.{}` ki {} value(s) unnayi, pattern lo {}",
                    adt.name,
                    v.name,
                    v.fields.len(),
                    given.len()
                ),
            );
        }
        for (a, f) in given.iter().zip(&v.fields) {
            self.pat(a, &f.subst(&fresh), through);
        }
    }

    fn pat_record(
        &mut self,
        span: Span,
        path: &Path,
        fields: &[(Ident, Option<Pattern>)],
        rest: bool,
        t: &Ty,
        through: Option<bool>,
    ) {
        let lower = super::lower::Lower { g: self.g, diags: &mut self.diags };
        let Some((d, _)) = lower.path_def(self.file, path) else { return };
        let g = self.g;
        let Some(adt) = g.adts.get(&d) else { return };
        let AdtKind::Record(decl) = &adt.kind else {
            self.error("E0401", path.span, format!("`{}` rakam kaadhu", adt.name));
            return;
        };
        let fresh: Vec<Ty> = adt.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect();
        if !self.unify_or_report(span, t, &Ty::App(Con::Adt(d), fresh.clone())) {
            return;
        }
        for (name, sub) in fields {
            let Some(f) = decl.iter().find(|f| f.name == name.name) else {
                self.error("E0405", name.span, format!("`{}` lo `{}` ane field ledhu", adt.name, name.name));
                continue;
            };
            let ft = f.ty.subst(&fresh);
            match sub {
                Some(p) => self.pat(p, &ft, through),
                None => {
                    let bind = Pattern {
                        kind: PatternKind::Bind { mutable: false, name: name.clone() },
                        span: name.span,
                    };
                    self.pat(&bind, &ft, through);
                }
            }
        }
        if !rest {
            let missing: Vec<&str> = decl
                .iter()
                .filter(|f| !fields.iter().any(|(n, _)| n.name == f.name))
                .map(|f| f.name.as_str())
                .collect();
            if !missing.is_empty() {
                self.error(
                    "E0416",
                    span,
                    format!("ee fields pattern lo levu: {}. Vaddhu ante `..` raayandi", missing.join(", ")),
                );
            }
        }
    }

    // ── exhaustiveness (§14.3) ─────────────────────────────────────────────

    pub fn check_exhaustive(&mut self, scrutinee: &Ty, arms: &[Arm], span: Span) {
        let ty = self.infer.resolve(scrutinee);
        if ty.has_error() {
            return;
        }
        let mut rows: Vec<Vec<SP>> = Vec::new();
        for arm in arms {
            let sp = self.simplify(&arm.pattern, &ty);
            if self.useful(&rows, std::slice::from_ref(&sp), std::slice::from_ref(&ty)).is_none() {
                self.diags.push((
                    self.file,
                    Diagnostic::warning(
                        "W0004",
                        arm.pattern.span,
                        "ee arm ki eppudu raadhu: paina arms already anni cases cover chesayi",
                    ),
                ));
            }
            rows.push(vec![sp]);
        }
        if let Some(w) = self.useful(&rows, &[SP::Wild], std::slice::from_ref(&ty)) {
            let missing = w.first().cloned().unwrap_or_else(|| "_".into());
            self.error(
                "E0701",
                span,
                format!("`{missing}` case marchipoyav! Daaniki oka arm raayandi (leka `_ aithe:`)"),
            );
        }
    }

    /// The constructors of a finite type with their field types.
    fn ctors_of(&mut self, ty: &Ty) -> Option<Vec<(CKey, Vec<Ty>)>> {
        let t = self.infer.resolve(ty);
        Some(match t {
            Ty::Ref(_, inner) => return self.ctors_of(&inner),
            Ty::Bool => vec![(CKey::Bool(true), vec![]), (CKey::Bool(false), vec![])],
            Ty::Unit => vec![(CKey::Single, vec![])],
            Ty::Tuple(items) => vec![(CKey::Single, items)],
            Ty::App(Con::Undochu, a) => vec![(CKey::Undhi, vec![a[0].clone()]), (CKey::Ledhu, vec![])],
            Ty::App(Con::SareThappu, a) => {
                vec![(CKey::Sare, vec![a[0].clone()]), (CKey::Thappu, vec![a[1].clone()])]
            }
            Ty::App(Con::Adt(d), args) => match &self.g.adts.get(&d)?.kind {
                AdtKind::Enum(vs) => vs
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (CKey::Variant(i), v.fields.iter().map(|f| f.subst(&args)).collect()))
                    .collect(),
                AdtKind::Record(fs) => vec![(CKey::Single, fs.iter().map(|f| f.ty.subst(&args)).collect())],
            },
            _ => return None,
        })
    }

    fn simplify(&mut self, p: &Pattern, ty: &Ty) -> SP {
        let t = self.infer.resolve(ty);
        let t = match t {
            Ty::Ref(_, inner) => *inner,
            other => other,
        };
        match &p.kind {
            PatternKind::Wild | PatternKind::Bind { .. } => SP::Wild,
            PatternKind::Literal(e) => match &e.kind {
                ExprKind::Bool(b) => SP::Ctor(CKey::Bool(*b), vec![]),
                _ => match lit_value(e) {
                    Some(v) => SP::Lit(v, v),
                    None => SP::Opaque(format!("{:?}", e.kind)),
                },
            },
            PatternKind::Range { lo, hi, inclusive } => match (lit_value(lo), lit_value(hi)) {
                (Some(a), Some(b)) => SP::Lit(a, if *inclusive { b } else { b - 1 }),
                _ => SP::Opaque(String::new()),
            },
            PatternKind::Tuple(items) => {
                let tys = match &t {
                    Ty::Tuple(ts) => ts.clone(),
                    _ => vec![Ty::Error; items.len()],
                };
                SP::Ctor(CKey::Single, items.iter().zip(&tys).map(|(i, t)| self.simplify(i, t)).collect())
            }
            PatternKind::Or(alts) => SP::Or(alts.iter().map(|a| self.simplify(a, &t)).collect()),
            PatternKind::Ctor { path, args } => {
                let name = path.segments[0].name.as_str();
                let ctors = self.ctors_of(&t).unwrap_or_default();
                let key = if path.segments.len() == 1 {
                    match name {
                        "undhi" => CKey::Undhi,
                        "ledhu" => CKey::Ledhu,
                        "sare" => CKey::Sare,
                        "thappu" => CKey::Thappu,
                        _ => return SP::Wild,
                    }
                } else {
                    let vname = &path.segments.last().expect("non-empty").name;
                    match &t {
                        Ty::App(Con::Adt(d), _) => match self.g.adts.get(d).map(|a| &a.kind) {
                            Some(AdtKind::Enum(vs)) => match vs.iter().position(|v| &v.name == vname) {
                                Some(i) => CKey::Variant(i),
                                None => return SP::Wild,
                            },
                            _ => return SP::Wild,
                        },
                        _ => return SP::Wild,
                    }
                };
                let sub_tys =
                    ctors.iter().find(|(k, _)| *k == key).map(|(_, ts)| ts.clone()).unwrap_or_default();
                let given = args.as_deref().unwrap_or(&[]);
                let subs = sub_tys
                    .iter()
                    .enumerate()
                    .map(|(i, st)| given.get(i).map_or(SP::Wild, |g| self.simplify(g, st)))
                    .collect();
                SP::Ctor(key, subs)
            }
            PatternKind::Record { fields, .. } => {
                let Ty::App(Con::Adt(d), args) = &t else { return SP::Wild };
                let Some(AdtKind::Record(decl)) = self.g.adts.get(d).map(|a| a.kind.clone()) else {
                    return SP::Wild;
                };
                let subs = decl
                    .iter()
                    .map(|f| match fields.iter().find(|(n, _)| n.name == f.name) {
                        Some((_, Some(p))) => self.simplify(p, &f.ty.subst(args)),
                        _ => SP::Wild,
                    })
                    .collect();
                SP::Ctor(CKey::Single, subs)
            }
        }
    }

    /// Returns a witness (one text per column) if `q` matches a value no row matches.
    fn useful(&mut self, rows: &[Vec<SP>], q: &[SP], tys: &[Ty]) -> Option<Vec<String>> {
        if q.is_empty() {
            return rows.is_empty().then(Vec::new);
        }
        // Expand OR-patterns in the first column of the rows.
        let mut expanded: Vec<Vec<SP>> = Vec::new();
        for r in rows {
            match &r[0] {
                SP::Or(alts) => {
                    for a in alts {
                        let mut nr = vec![a.clone()];
                        nr.extend_from_slice(&r[1..]);
                        expanded.push(nr);
                    }
                }
                _ => expanded.push(r.clone()),
            }
        }
        let rows = expanded;
        match &q[0] {
            SP::Or(alts) => {
                for a in alts {
                    let mut nq = vec![a.clone()];
                    nq.extend_from_slice(&q[1..]);
                    if let Some(w) = self.useful(&rows, &nq, tys) {
                        return Some(w);
                    }
                }
                None
            }
            SP::Ctor(key, subs) => {
                let sub_tys = self.sub_tys(&tys[0], key, subs.len());
                let spec = specialize(&rows, key, subs.len());
                let mut nq = subs.clone();
                nq.extend_from_slice(&q[1..]);
                let mut nt = sub_tys;
                nt.extend_from_slice(&tys[1..]);
                let w = self.useful(&spec, &nq, &nt)?;
                Some(self.rebuild(&tys[0], key, subs.len(), w))
            }
            SP::Lit(a, b) => {
                let (a, b) = (*a, *b);
                let rest: Vec<Vec<SP>> = rows
                    .iter()
                    .filter(|r| match &r[0] {
                        SP::Wild => true,
                        SP::Lit(x, y) => *x <= a && b <= *y,
                        _ => false,
                    })
                    .map(|r| r[1..].to_vec())
                    .collect();
                let mut w = self.useful(&rest, &q[1..], &tys[1..])?;
                w.insert(0, a.to_string());
                Some(w)
            }
            SP::Opaque(text) => {
                let rest: Vec<Vec<SP>> = rows
                    .iter()
                    .filter(|r| {
                        matches!(&r[0], SP::Wild)
                            || matches!(&r[0], SP::Opaque(t) if t == text && !t.is_empty())
                    })
                    .map(|r| r[1..].to_vec())
                    .collect();
                let mut w = self.useful(&rest, &q[1..], &tys[1..])?;
                w.insert(0, "_".into());
                Some(w)
            }
            SP::Wild => {
                let ctors = self.ctors_of(&tys[0]);
                let present: Vec<CKey> = rows
                    .iter()
                    .filter_map(|r| match &r[0] {
                        SP::Ctor(k, _) => Some(k.clone()),
                        _ => None,
                    })
                    .collect();
                if let Some(cs) = &ctors {
                    if !cs.is_empty() && cs.iter().all(|(k, _)| present.contains(k)) {
                        for (k, sub_tys) in cs.clone() {
                            let arity = sub_tys.len();
                            let spec = specialize(&rows, &k, arity);
                            let mut nq = vec![SP::Wild; arity];
                            nq.extend_from_slice(&q[1..]);
                            let mut nt = sub_tys;
                            nt.extend_from_slice(&tys[1..]);
                            if let Some(w) = self.useful(&spec, &nq, &nt) {
                                return Some(self.rebuild(&tys[0], &k, arity, w));
                            }
                        }
                        return None;
                    }
                }
                let default: Vec<Vec<SP>> =
                    rows.iter().filter(|r| matches!(r[0], SP::Wild)).map(|r| r[1..].to_vec()).collect();
                let mut w = self.useful(&default, &q[1..], &tys[1..])?;
                let head = match &ctors {
                    Some(cs) if !cs.is_empty() => {
                        let (k, sub) =
                            cs.iter().find(|(k, _)| !present.contains(k)).expect("a missing constructor");
                        let wild = vec!["_".to_string(); sub.len()];
                        self.ctor_text(&tys[0], k, &wild)
                    }
                    _ => "_".into(),
                };
                w.insert(0, head);
                Some(w)
            }
        }
    }

    fn sub_tys(&mut self, ty: &Ty, key: &CKey, arity: usize) -> Vec<Ty> {
        self.ctors_of(ty)
            .and_then(|cs| cs.into_iter().find(|(k, _)| k == key).map(|(_, ts)| ts))
            .unwrap_or_else(|| vec![Ty::Error; arity])
    }

    fn rebuild(&mut self, ty: &Ty, key: &CKey, arity: usize, mut w: Vec<String>) -> Vec<String> {
        let rest = w.split_off(arity.min(w.len()));
        let mut out = vec![self.ctor_text(ty, key, &w)];
        out.extend(rest);
        out
    }

    fn ctor_text(&mut self, ty: &Ty, key: &CKey, subs: &[String]) -> String {
        let args = |name: &str| {
            if subs.is_empty() {
                name.to_string()
            } else {
                format!("{name}({})", subs.join(", "))
            }
        };
        let t = self.infer.resolve(ty);
        let t = match t {
            Ty::Ref(_, inner) => *inner,
            other => other,
        };
        match key {
            CKey::Bool(true) => "avunu".into(),
            CKey::Bool(false) => "kaadhu".into(),
            CKey::Undhi => args("undhi"),
            CKey::Ledhu => "ledhu".into(),
            CKey::Sare => args("sare"),
            CKey::Thappu => args("thappu"),
            CKey::Variant(i) => match &t {
                Ty::App(Con::Adt(d), _) => match self.g.adts.get(d) {
                    Some(adt) => match &adt.kind {
                        AdtKind::Enum(vs) => args(&format!("{}.{}", adt.name, vs[*i].name)),
                        _ => "_".into(),
                    },
                    None => "_".into(),
                },
                _ => "_".into(),
            },
            CKey::Single => match &t {
                Ty::App(Con::Adt(d), _) => match self.g.adts.get(d) {
                    Some(adt) => format!("{}{{..}}", adt.name),
                    None => "_".into(),
                },
                Ty::Unit => "()".into(),
                _ => format!("({})", subs.join(", ")),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum CKey {
    Bool(bool),
    Undhi,
    Ledhu,
    Sare,
    Thappu,
    Variant(usize),
    /// The one constructor of tuples, records and `()`.
    Single,
}

/// A simplified pattern.
#[derive(Clone, Debug)]
enum SP {
    Wild,
    Ctor(CKey, Vec<SP>),
    /// An integer or character literal or range, inclusive.
    Lit(i128, i128),
    /// A string literal (compared by text).
    Opaque(String),
    Or(Vec<SP>),
}

fn specialize(rows: &[Vec<SP>], key: &CKey, arity: usize) -> Vec<Vec<SP>> {
    rows.iter()
        .filter_map(|r| {
            let head: Vec<SP> = match &r[0] {
                SP::Ctor(k, subs) if k == key => subs.clone(),
                SP::Wild => vec![SP::Wild; arity],
                _ => return None,
            };
            let mut out = head;
            out.extend_from_slice(&r[1..]);
            Some(out)
        })
        .collect()
}

fn lit_value(e: &Expr) -> Option<i128> {
    match &e.kind {
        ExprKind::Int(lit) => i128::from_str_radix(&lit.digits, lit.radix).ok(),
        ExprKind::Char(c) => Some(*c as i128),
        ExprKind::Unary { op: UnOp::Neg, operand } => lit_value(operand).map(|v| -v),
        _ => None,
    }
}

fn is_refutable(p: &Pattern) -> bool {
    match &p.kind {
        PatternKind::Wild | PatternKind::Bind { .. } => false,
        PatternKind::Tuple(items) => items.iter().any(is_refutable),
        PatternKind::Record { fields, .. } => {
            fields.iter().any(|(_, p)| p.as_ref().is_some_and(is_refutable))
        }
        _ => true,
    }
}
