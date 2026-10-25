//! Calls (spec §11): callee resolution, method lookup (§13.4), the phrase
//! binding algorithm (§11.2), automatic borrows (§11.3) and pipes (§12).

use super::body::{builtin_head, is_place, BodyCx, Conv};
use super::solve::{self, Solver};
use super::{FnId, FnOrigin, Formal, Head, RecvMode, TraitRef};
use crate::ast::*;
use crate::resolve::{DefKind, Res};
use crate::source::Span;
use crate::types::{Con, Ty, VarKind};

/// The value a pipe passes into its stage (§12.1).
pub struct Injected {
    pub ty: Ty,
    pub span: Span,
    pub place: bool,
}

/// A callee with its signature instantiated.
struct Prepared {
    name: String,
    formals: Vec<Formal>,
    ret: Ty,
    /// An adapter called with method syntax: the receiver fills formal 0.
    pre: Option<Injected>,
    conv: Option<(Conv, Ty, Ty)>,
}

enum Pick {
    Fn(FnId, Ty),
    Trait { fid: FnId, self_ty: Ty, args: Vec<Ty> },
    Adapter(FnId),
}

/// Where a bound argument comes from.
enum Src<'e> {
    Expr(&'e Expr),
    Injected,
    Spread(Ty, Span),
}

struct Entry<'e> {
    src: Src<'e>,
    role: Option<Role>,
    label: Option<&'e Ident>,
    span: Span,
    /// A runtime-length spread: may only feed the `migathavi` collector.
    runtime: bool,
}

impl BodyCx<'_, '_> {
    pub fn check_call(
        &mut self,
        span: Span,
        callee: &Expr,
        type_args: &[Type],
        phrases: &[Phrase],
        injected: Option<Injected>,
        expected: Option<&Ty>,
    ) -> Ty {
        let type_args: Vec<Ty> = type_args.iter().map(|t| self.lower_ty(t)).collect();
        let prepared = match &callee.kind {
            ExprKind::Path(p) => self.callee_path(p, &type_args, callee.span),
            ExprKind::Field { base, name } => {
                let bt = self.check_expr(base, None);
                self.method(bt, Some(base), name, &type_args)
            }
            _ => {
                let t = self.check_expr(callee, None);
                self.value_callee(t, callee.span)
            }
        };
        let Some(mut prepared) = prepared else {
            for ph in phrases {
                if let PhraseKind::Value(e) | PhraseKind::Spread(e) = &ph.kind {
                    self.check_expr(e, None);
                }
            }
            return Ty::Error;
        };
        if let Some(exp) = expected {
            // Hint only: the result type is checked where it is used.
            let snap = self.infer.snapshot();
            if solve::unify(&mut self.infer, &prepared.ret, exp) {
                self.infer.commit(snap);
            } else {
                self.infer.rollback_to(snap);
            }
        }
        let pre = prepared.pre.take();
        self.bind_and_check(&prepared, phrases, injected, pre, span);
        if let Some((kind, src, dst)) = prepared.conv.take() {
            self.conversions.push((kind, src, dst, span));
        }
        prepared.ret
    }

    // ── callees ────────────────────────────────────────────────────────────

    fn value_callee(&mut self, t: Ty, span: Span) -> Option<Prepared> {
        match self.shallow(&t) {
            Ty::Fn(params, ret) => Some(Prepared {
                name: "ee pani".into(),
                formals: params
                    .iter()
                    .enumerate()
                    .map(|(i, (ty, role))| Formal {
                        name: format!("#{i}"),
                        ty: ty.clone(),
                        role: *role,
                        kind: ParamKind::Normal,
                        keyword_only: false,
                        has_default: false,
                    })
                    .collect(),
                ret: *ret,
                pre: None,
                conv: None,
            }),
            Ty::Error => None,
            Ty::Var(_) => {
                self.need_annotation(span);
                None
            }
            other => {
                let shown = self.show(&other);
                self.error("E0401", span, format!("`{shown}` function kaadhu, call cheyyalem"));
                None
            }
        }
    }

    fn callee_path(&mut self, p: &Path, type_args: &[Ty], span: Span) -> Option<Prepared> {
        let segs = &p.segments;
        let first = &segs[0];
        let last = segs.last().expect("non-empty path");
        match self.res_at(first.span) {
            Some(Res::Local(decl)) => {
                let ty = self.locals.get(&decl.start).map_or(Ty::Error, |l| l.ty.clone());
                if segs.len() == 1 {
                    return self.value_callee(ty, span);
                }
                let base = self.path_fields(ty, &segs[1..segs.len() - 1]);
                let recv = prefix_expr(p);
                self.method(base, Some(&recv), last, type_args)
            }
            Some(Res::SelfMember) => {
                let naa = self.naa_ty();
                if first.name == "naa" {
                    if segs.len() == 1 {
                        return self.value_callee(naa, span);
                    }
                    let base = self.path_fields(naa, &segs[1..segs.len() - 1]);
                    let recv = prefix_expr(p);
                    return self.method(base, Some(&recv), last, type_args);
                }
                if segs.len() == 1 {
                    // Pro-drop: a method of `idhi`, or a field holding a `pani`.
                    let self_ty = self.receiver.as_ref().map(|(_, t)| t.clone()).unwrap_or(Ty::Error);
                    if self.has_method(&self_ty, &first.name) {
                        let recv = naa_expr(first.span);
                        return self.method(naa, Some(&recv), first, type_args);
                    }
                    let t = self.field(&naa, first);
                    return self.value_callee(t, span);
                }
                let base = self.path_fields(naa, &segs[..segs.len() - 1]);
                let recv = prefix_expr(p);
                self.method(base, Some(&recv), last, type_args)
            }
            Some(Res::Def(d)) => self.callee_def(d, p, 1, type_args, span),
            Some(Res::Builtin) => {
                if segs.len() != 2 {
                    self.error("E0401", span, "idhi call cheyyalem");
                    return None;
                }
                let head = builtin_head(&first.name);
                self.assoc(head, &segs[1], type_args)
            }
            None => None,
        }
    }

    fn path_fields(&mut self, mut ty: Ty, segs: &[Ident]) -> Ty {
        for s in segs {
            ty = self.field(&ty, s);
        }
        ty
    }

    fn callee_def(
        &mut self,
        d: usize,
        p: &Path,
        idx: usize,
        type_args: &[Ty],
        span: Span,
    ) -> Option<Prepared> {
        let g = self.g;
        let segs = &p.segments;
        match g.res.defs[d].kind {
            DefKind::Module(m) => {
                if g.res.modules[m].open {
                    return None;
                }
                match segs.get(idx).and_then(|s| self.res_at(s.span)) {
                    Some(Res::Def(member)) => self.callee_def(member, p, idx + 1, type_args, span),
                    _ => None,
                }
            }
            DefKind::Fn | DefKind::StdValue | DefKind::Const => {
                if idx == segs.len() {
                    if let Some(&fid) = g.fn_of_def.get(&d) {
                        return self.prepare_fn(fid, None, type_args, segs[idx - 1].span);
                    }
                }
                // A constant (or function value) followed by fields and a method.
                let ty = g.consts.get(&d).cloned().unwrap_or(Ty::Error);
                if idx == segs.len() {
                    return self.value_callee(ty, span);
                }
                let base = self.path_fields(ty, &segs[idx..segs.len() - 1]);
                self.method(base, None, segs.last().expect("non-empty"), type_args)
            }
            DefKind::Record | DefKind::Enum | DefKind::StdType => {
                let Some(seg) = segs.get(idx) else {
                    self.error("E0401", span, "type ni call cheyyalem. Record kosam `Peru{…}` raayandi");
                    return None;
                };
                let head = match g.std_cons.get(&d) {
                    Some(c) => Head::Con(*c),
                    None => Head::Con(Con::Adt(d)),
                };
                self.assoc(Some(head), seg, type_args)
            }
            _ => None,
        }
    }

    /// `Type.f()`: an associated function.
    fn assoc(&mut self, head: Option<Head>, seg: &Ident, type_args: &[Ty]) -> Option<Prepared> {
        let found = head.and_then(|h| self.g.inherent.get(&h)).and_then(|m| m.get(&seg.name)).copied();
        match found {
            Some(fid) => {
                self.check_visible(fid, seg.span);
                self.prepare_fn(fid, None, type_args, seg.span)
            }
            None => {
                self.error("E0406", seg.span, format!("`{}` ane function ee type lo ledhu", seg.name));
                None
            }
        }
    }

    fn check_visible(&mut self, fid: FnId, span: Span) {
        let sig = &self.g.fns[fid];
        if !sig.public && sig.module != self.module {
            let name = sig.name.clone();
            self.error("E0303", span, format!("`{name}` private method. `andhariki` pettandi"));
        }
    }

    /// Instantiates a function's generics; `self_ty` is the receiver's type.
    fn prepare_fn(
        &mut self,
        fid: FnId,
        self_ty: Option<Ty>,
        type_args: &[Ty],
        span: Span,
    ) -> Option<Prepared> {
        let g = self.g;
        let sig = &g.fns[fid];
        let mut fresh: Vec<Ty> = sig.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect();
        let own = &mut fresh[sig.own_generics..];
        if type_args.len() > own.len() {
            self.error("E0401", span, format!("`{}` ki {} type argument(s) matrame", sig.name, own.len()));
        }
        for (slot, t) in own.iter_mut().zip(type_args) {
            *slot = t.clone();
        }
        if let (Some(st), Some(r)) = (&self_ty, &sig.receiver) {
            let want = r.ty.subst(&fresh);
            solve::unify(&mut self.infer, &want, st);
        }
        if BodyCx::origin_unsafe(sig.origin) {
            let what = format!("`{}` call", sig.name);
            self.require_unsafe(span, &what);
        }
        for (i, gen) in sig.generics.iter().enumerate() {
            for b in &gen.bounds {
                let tr = TraitRef { def: b.def, args: b.args.iter().map(|a| a.subst(&fresh)).collect() };
                self.obligations.push((fresh[i].clone(), tr, span, "E0801"));
            }
        }
        let conv = if sig.origin == FnOrigin::Std && sig.module == 0 {
            match sig.name.as_str() {
                "maarchu" => Some(Conv::Lossless),
                "kudhirithe_maarchu" => Some(Conv::Checked),
                "kolpothu_maarchu" => Some(Conv::Lossy),
                _ => None,
            }
            .map(|k| (k, fresh[1].clone(), fresh[0].clone()))
        } else {
            None
        };
        if conv.is_some() && type_args.is_empty() {
            self.error(
                "E0404",
                span,
                format!("`{}` ki target type cheppandi: `x ni {}[ankam32]()`", sig.name, sig.name),
            );
        }
        Some(Prepared {
            name: sig.name.clone(),
            formals: sig.params.iter().map(|f| Formal { ty: f.ty.subst(&fresh), ..f.clone() }).collect(),
            ret: sig.ret.subst(&fresh),
            pre: None,
            conv,
        })
    }

    // ── methods (§13.4) ────────────────────────────────────────────────────

    fn has_method(&mut self, ty: &Ty, name: &str) -> bool {
        let t = self.autoderef(ty);
        head_of_ty(&t).and_then(|h| self.g.inherent.get(&h)).is_some_and(|m| m.contains_key(name))
            || self.g.traits.values().any(|tr| tr.file.is_some() && tr.methods.iter().any(|(n, _)| n == name))
    }

    fn method(
        &mut self,
        recv_ty: Ty,
        recv: Option<&Expr>,
        name: &Ident,
        type_args: &[Ty],
    ) -> Option<Prepared> {
        let pick = self.lookup_method(&recv_ty, name)?;
        match pick {
            Pick::Fn(fid, self_ty) => {
                self.check_visible(fid, name.span);
                let sig = &self.g.fns[fid];
                let Some(r) = &sig.receiver else {
                    self.error(
                        "E0406",
                        name.span,
                        format!(
                            "`{}` associated function, method kaadhu. `Type.{}()` ani pilavandi",
                            name.name, name.name
                        ),
                    );
                    return None;
                };
                let mode = r.mode;
                self.receiver_mode(mode, recv, &recv_ty, name.span);
                self.prepare_fn(fid, Some(self_ty), type_args, name.span)
            }
            Pick::Trait { fid, self_ty, args } => {
                let g = self.g;
                let sig = &g.fns[fid];
                if let Some(r) = &sig.receiver {
                    self.receiver_mode(r.mode, recv, &recv_ty, name.span);
                }
                let mut fresh: Vec<Ty> =
                    sig.generics.iter().map(|_| self.infer.fresh(VarKind::General)).collect();
                fresh[0] = self_ty;
                for (i, a) in args.into_iter().enumerate() {
                    if i + 1 < fresh.len() {
                        fresh[i + 1] = a;
                    }
                }
                for (slot, t) in fresh[sig.own_generics..].iter_mut().zip(type_args) {
                    *slot = t.clone();
                }
                Some(Prepared {
                    name: sig.name.clone(),
                    formals: sig
                        .params
                        .iter()
                        .map(|f| Formal { ty: f.ty.subst(&fresh), ..f.clone() })
                        .collect(),
                    ret: sig.ret.subst(&fresh),
                    pre: None,
                    conv: None,
                })
            }
            Pick::Adapter(fid) => {
                let mut p = self.prepare_fn(fid, None, type_args, name.span)?;
                let place = recv.is_some_and(is_place);
                p.pre = Some(Injected { ty: recv_ty, span: name.span, place });
                Some(p)
            }
        }
    }

    /// Receiver adjustment (§13.4.3): `&maare idhi` needs a mutable place.
    fn receiver_mode(&mut self, mode: RecvMode, recv: Option<&Expr>, recv_ty: &Ty, _span: Span) {
        if mode != RecvMode::RefMut {
            return;
        }
        if let Ty::Ref(m, _) = self.shallow(recv_ty) {
            if !m {
                if let Some(e) = recv {
                    let name = match &e.kind {
                        ExprKind::Path(p) => p.segments[0].name.clone(),
                        _ => "idhi".into(),
                    };
                    self.error(
                        "E0607",
                        e.span,
                        format!("`{name}` `&` reference matrame, maarchalem. `&maare` kavali"),
                    );
                }
            }
            return;
        }
        if let Some(e) = recv {
            self.require_mutable(e, "E0607");
        }
    }

    fn lookup_method(&mut self, recv_ty: &Ty, name: &Ident) -> Option<Pick> {
        let g = self.g;
        let mut cand = self.shallow(recv_ty);
        loop {
            cand = self.infer.resolve(&cand);
            // An undecided literal looks up methods as its default type.
            if let Ty::Var(v) = cand {
                match self.infer.kind(v) {
                    VarKind::Int => cand = Ty::ANKAM,
                    VarKind::Float => cand = Ty::Float(64),
                    VarKind::General => {}
                }
            }
            match &cand {
                Ty::Error => return None,
                Ty::Var(_) => {
                    self.need_annotation(name.span);
                    return None;
                }
                // A reference passes straight to its target: `a.xerox()` on
                // `a: &T` copies the `T`, not the reference.
                Ty::Ref(_, inner) => {
                    cand = (**inner).clone();
                    continue;
                }
                _ => {}
            }
            if let Some(h) = head_of_ty(&cand) {
                if let Some(&fid) = g.inherent.get(&h).and_then(|m| m.get(&name.name)) {
                    return Some(Pick::Fn(fid, cand));
                }
            }
            // Vectors and arrays also have the slice methods.
            let elem = match &cand {
                Ty::App(Con::Varusa, a) => Some(a[0].clone()),
                Ty::Array(e, _) => Some((**e).clone()),
                _ => None,
            };
            if let Some(e) = elem {
                if let Some(&fid) = g.inherent.get(&Head::Con(Con::Mukka)).and_then(|m| m.get(&name.name)) {
                    return Some(Pick::Fn(fid, Ty::App(Con::Mukka, vec![e])));
                }
            }
            if let Some(p) = self.trait_method(&cand, name) {
                return p;
            }
            cand = match cand {
                Ty::Ref(_, inner) => *inner,
                Ty::App(Con::Pette, args) => args[0].clone(),
                _ => break,
            };
        }
        if let Some(&fid) = g.adapters.get(&name.name) {
            return Some(Pick::Adapter(fid));
        }
        let shown = self.show(recv_ty);
        self.error("E0406", name.span, format!("`{shown}` ki `{}` ane method ledhu", name.name));
        None
    }

    /// A method from a trait the candidate implements. `Some(None)` means an
    /// error was reported.
    fn trait_method(&mut self, cand: &Ty, name: &Ident) -> Option<Option<Pick>> {
        let g = self.g;
        let mut defs: Vec<usize> = g.traits.keys().copied().collect();
        defs.sort_unstable();
        let mut found: Vec<Pick> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for def in defs {
            let tr = &g.traits[&def];
            let Some(&(_, fid)) = tr.methods.iter().find(|(n, _)| *n == name.name) else { continue };
            let args: Vec<Ty> = if let Ty::Param(i) = cand {
                match self.generics.get(*i as usize).and_then(|gp| gp.bounds.iter().find(|b| b.def == def)) {
                    Some(b) => b.args.clone(),
                    None => continue,
                }
            } else {
                let args: Vec<Ty> =
                    (1..tr.generics.len()).map(|_| self.infer.fresh(VarKind::General)).collect();
                let snap = self.infer.snapshot();
                let solver = Solver { g, generics: &self.generics };
                if !solver.implements(&mut self.infer, cand, &TraitRef { def, args: args.clone() }, 0) {
                    self.infer.rollback_to(snap);
                    continue;
                }
                self.infer.commit(snap);
                args
            };
            names.push(tr.name.clone());
            found.push(Pick::Trait { fid, self_ty: cand.clone(), args });
        }
        match found.len() {
            0 => None,
            1 => Some(found.pop()),
            _ => {
                self.error(
                    "E0305",
                    name.span,
                    format!(
                        "`{}` method {} gunaalu rendinti lo undhi. Edhi kavalo teliyatledhu",
                        name.name,
                        names.join(", ")
                    ),
                );
                Some(None)
            }
        }
    }

    // ── binding (§11.2) ────────────────────────────────────────────────────

    fn bind_and_check(
        &mut self,
        p: &Prepared,
        phrases: &[Phrase],
        injected: Option<Injected>,
        pre: Option<Injected>,
        span: Span,
    ) {
        let formals = &p.formals;
        let diags_before = self.diags.len();
        let mut entries: Vec<Entry> = Vec::new();
        let has_hole = phrases.iter().any(|ph| matches!(ph.kind, PhraseKind::Hole));
        for ph in phrases {
            match &ph.kind {
                PhraseKind::Value(e) => entries.push(Entry {
                    src: Src::Expr(e),
                    role: ph.role,
                    label: ph.label.as_ref(),
                    span: ph.span,
                    runtime: false,
                }),
                PhraseKind::Hole => entries.push(Entry {
                    src: Src::Injected,
                    role: ph.role,
                    label: ph.label.as_ref(),
                    span: ph.span,
                    runtime: false,
                }),
                PhraseKind::Spread(e) => {
                    let t = self.check_expr(e, None);
                    match self.shallow(&t) {
                        Ty::Tuple(items) => {
                            for it in items {
                                entries.push(Entry {
                                    src: Src::Spread(it, e.span),
                                    role: ph.role,
                                    label: None,
                                    span: ph.span,
                                    runtime: false,
                                });
                            }
                        }
                        Ty::Array(elem, Some(n)) => {
                            for _ in 0..n {
                                entries.push(Entry {
                                    src: Src::Spread((*elem).clone(), e.span),
                                    role: ph.role,
                                    label: None,
                                    span: ph.span,
                                    runtime: false,
                                });
                            }
                        }
                        other => {
                            let elem = match &other {
                                Ty::App(Con::Varusa | Con::Mukka, a) => a[0].clone(),
                                Ty::Ref(_, inner) => match self.shallow(inner) {
                                    Ty::App(Con::Varusa | Con::Mukka, a) => a[0].clone(),
                                    Ty::Array(e, _) => *e,
                                    _ => Ty::Error,
                                },
                                Ty::Array(e, _) => (**e).clone(),
                                _ => Ty::Error,
                            };
                            entries.push(Entry {
                                src: Src::Spread(elem, e.span),
                                role: ph.role,
                                label: None,
                                span: ph.span,
                                runtime: true,
                            });
                        }
                    }
                }
            }
        }

        // Which formal (or collector) each entry binds to.
        let mut bound: Vec<Option<usize>> = vec![None; formals.len()];
        let mut target: Vec<Option<usize>> = vec![None; entries.len()];
        let variadic = formals.iter().position(|f| f.kind == ParamKind::Variadic);
        let named_coll = formals.iter().position(|f| f.kind == ParamKind::NamedCollector);
        let mut pre_slot = None;
        if pre.is_some() {
            if let Some(i) = formals.iter().position(|f| f.kind == ParamKind::Normal) {
                bound[i] = Some(usize::MAX);
                pre_slot = Some(i);
            }
        }
        let role_name =
            |r: Option<Role>| r.map_or("(marker lekunda)".to_string(), |r| format!("`{}`", r.as_str()));

        // Named phrases first.
        for (ei, en) in entries.iter().enumerate() {
            let Some(label) = en.label else { continue };
            match formals.iter().position(|f| f.name == label.name && f.kind == ParamKind::Normal) {
                Some(fi) => {
                    if formals[fi].role != en.role {
                        let msg = format!(
                            "`{}` ki {} kavali, kaani {} ichharu",
                            label.name,
                            role_name(formals[fi].role),
                            role_name(en.role)
                        );
                        self.error("E0502", en.span, msg);
                    }
                    if bound[fi].is_some() {
                        self.error("E0503", en.span, format!("`{}` ki rendu values ichharu", label.name));
                    } else {
                        bound[fi] = Some(ei);
                    }
                    target[ei] = Some(fi);
                }
                None => match named_coll {
                    Some(ci) => target[ei] = Some(ci),
                    None => {
                        let msg = format!("`{}` ki `{}` ane parameter ledhu", p.name, label.name);
                        self.error("E0504", label.span, msg);
                    }
                },
            }
        }
        // Unnamed phrases, by role, in source order.
        for (ei, en) in entries.iter().enumerate() {
            if en.label.is_some() {
                continue;
            }
            if matches!(en.src, Src::Injected) && !has_hole {
                continue;
            }
            if en.runtime {
                match variadic {
                    Some(vi) if formals[vi].role == en.role => target[ei] = Some(vi),
                    _ => self.error(
                        "E0506",
                        en.span,
                        "`...` tho varusa/mukka ni `migathavi` parameter ki matrame ivvochu",
                    ),
                }
                continue;
            }
            let slot = formals.iter().enumerate().position(|(fi, f)| {
                f.kind == ParamKind::Normal && !f.keyword_only && bound[fi].is_none() && f.role == en.role
            });
            match slot {
                Some(fi) => {
                    bound[fi] = Some(ei);
                    target[ei] = Some(fi);
                }
                None => match variadic {
                    Some(vi) if formals[vi].role == en.role => target[ei] = Some(vi),
                    _ => {
                        let kw = formals.iter().enumerate().find(|(fi, f)| {
                            f.kind == ParamKind::Normal
                                && f.keyword_only
                                && bound[*fi].is_none()
                                && f.role == en.role
                        });
                        if let Some((_, f)) = kw {
                            let msg = format!(
                                "`{}` peru tho matrame ivvali: `x {} {} ga`",
                                f.name,
                                en.role.map_or("", |r| r.as_str()),
                                f.name
                            );
                            self.error("E0505", en.span, msg);
                        } else if matches!(en.src, Src::Injected) {
                            self.error(
                                "E0508",
                                en.span,
                                format!("`_` ki {} parameter `{}` lo ledhu", role_name(en.role), p.name),
                            );
                        } else {
                            let msg =
                                format!("`{}` ki {} tho inko parameter ledhu", p.name, role_name(en.role));
                            self.error("E0502", en.span, msg);
                        }
                    }
                },
            }
        }
        // The implicit pipe slot (§12.1b).
        let mut implicit_slot = None;
        if let Some(inj) = &injected {
            if !has_hole {
                match formals.iter().position(|f| matches!(f.kind, ParamKind::Normal)) {
                    Some(fi) if bound[fi].is_some() => {
                        let msg = format!("`{}` modhati parameter (`{}`) ki already value undhi, pipe value ekkadiki velthundhi? `_` vaadandi", p.name, formals[fi].name);
                        self.error("E0507", inj.span, msg);
                    }
                    Some(fi) => {
                        bound[fi] = Some(usize::MAX - 1);
                        implicit_slot = Some(fi);
                    }
                    None if variadic.is_some() => implicit_slot = variadic,
                    None => self.error(
                        "E0507",
                        inj.span,
                        format!("`{}` ki parameters levu, pipe value pettalem", p.name),
                    ),
                }
            }
        }
        // Missing formals (§11.2.6), unless a phrase already failed to bind.
        let bind_failed = self.diags.len() > diags_before;
        for (fi, f) in formals.iter().enumerate() {
            if f.kind == ParamKind::Normal && bound[fi].is_none() && !f.has_default && !bind_failed {
                let msg = if f.role.is_some() {
                    format!("`{}` ki {} evaru ivvaledhu (`{}`)", p.name, role_name(f.role), f.name)
                } else {
                    format!("`{}` ki `{}` evaru ivvaledhu", p.name, f.name)
                };
                self.error("E0501", span, msg);
            }
        }

        // Check every operand in source order (§11.5).
        let formal_ty = |fi: usize| -> Ty {
            let f = &formals[fi];
            match (f.kind, &f.ty) {
                (ParamKind::NamedCollector, Ty::App(Con::Jodi, a)) => a[1].clone(),
                _ => f.ty.clone(),
            }
        };
        if let (Some(inj), Some(fi)) = (&pre, pre_slot) {
            let want = formal_ty(fi);
            self.check_arg_ty(inj.ty.clone(), &want, inj.span, inj.place);
        }
        if let (Some(inj), Some(fi)) = (&injected, implicit_slot) {
            let want = formal_ty(fi);
            self.check_arg_ty(inj.ty.clone(), &want, inj.span, inj.place);
        }
        for (ei, en) in entries.iter().enumerate() {
            let want = target[ei].map(formal_ty);
            match (&en.src, want) {
                (Src::Expr(e), Some(w)) => {
                    let hint = match self.shallow(&w) {
                        Ty::Ref(_, inner)
                            if !matches!(e.kind, ExprKind::Unary { op: UnOp::Ref | UnOp::RefMut, .. }) =>
                        {
                            Some(*inner)
                        }
                        Ty::App(Con::Pravaham, _) => None,
                        other => Some(other),
                    };
                    let t = self.check_expr(e, hint.as_ref());
                    self.check_arg_ty(t, &w, e.span, is_place(e));
                }
                (Src::Expr(e), None) => {
                    self.check_expr(e, None);
                }
                (Src::Injected, Some(w)) => {
                    if let Some(inj) = &injected {
                        self.check_arg_ty(inj.ty.clone(), &w, inj.span, inj.place);
                    }
                }
                (Src::Spread(t, sp), Some(w)) => self.check_arg_ty(t.clone(), &w, *sp, false),
                _ => {}
            }
        }
    }

    /// Checks one argument against its formal, with automatic borrows (§11.3).
    fn check_arg_ty(&mut self, actual: Ty, formal: &Ty, span: Span, place: bool) {
        let f = self.shallow(formal);
        let a = self.shallow(&actual);
        if let Ty::App(Con::Pravaham, args) = &f {
            if !matches!(a, Ty::App(Con::Pravaham, _)) {
                let item = self.iter_item(&a, place, span);
                self.unify_or_report(span, &args[0], &item);
                return;
            }
        }
        if let Ty::Ref(m, inner) = &f {
            match &a {
                Ty::Ref(..) | Ty::Error | Ty::Never => {}
                Ty::Var(v) if self.infer.kind(*v) == VarKind::General => {}
                _ if *m => {
                    let shown = self.show(&a);
                    self.error(
                        "E0610",
                        span,
                        format!("ee parameter `&maare` reference adugutondhi, `{shown}` value kaadhu. `&maare x` ani raayi"),
                    );
                    return;
                }
                _ => {
                    // Automatic shared borrow.
                    if !solve::coerce(&mut self.infer, &Ty::reference(false, a.clone()), &f) {
                        self.mismatch(span, inner, &a);
                    }
                    return;
                }
            }
        }
        if !solve::coerce(&mut self.infer, &a, &f) {
            self.mismatch(span, &f, &a);
        }
    }
}

fn head_of_ty(t: &Ty) -> Option<Head> {
    super::head_of(t)
}

/// The receiver part of a method path `a.b.m`: `a.b`.
fn prefix_expr(p: &Path) -> Expr {
    let segs = p.segments[..p.segments.len() - 1].to_vec();
    let span = Span::new(segs[0].span.start, segs.last().expect("non-empty").span.end);
    Expr { kind: ExprKind::Path(Path { segments: segs, span }), span }
}

fn naa_expr(span: Span) -> Expr {
    let ident = Ident { name: "naa".into(), span };
    Expr { kind: ExprKind::Path(Path { segments: vec![ident], span }), span }
}
