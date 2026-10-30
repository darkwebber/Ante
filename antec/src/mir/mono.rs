//! The monomorphization check (§13.1, E0805): starting from every
//! non-generic body, walks each generic instantiation a program can reach,
//! as the native back end would generate them, and reports a call whose
//! type arguments keep growing (polymorphic recursion).

use std::collections::HashSet;

use super::*;
use crate::diag::Diagnostic;
use crate::typeck::Globals;
use crate::types::Con;

/// How large type arguments may grow (in type nodes) before instantiation
/// is taken to run away.
const MAX_SIZE: usize = 512;

pub fn check(prog: &Program, g: &Globals) -> Vec<(FileId, Diagnostic)> {
    let mut shapes = Vec::new();
    let mut out = walk(prog, g, &mut shapes);
    if out.is_empty() {
        out = shapes;
    }
    out
}

/// `form` and `query` (§25.3) take only flat records (E1305), checked here
/// where their type arguments are known.
fn form_call(g: &Globals, fid: crate::typeck::FnId, tys: &[Ty], span: Span) -> Option<Diagnostic> {
    let sig = &g.fns[fid];
    let module = g.res.modules[sig.module].name.as_str();
    if !matches!(module, "<ante>/form" | "<ante>/query") || !sig.public || sig.generics.is_empty() {
        return None;
    }
    let t = tys.first()?;
    if has_param(t) {
        return None;
    }
    let flat = |t: &Ty| match t {
        Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str => true,
        // `Rahasyam[padham]`: a password field.
        Ty::App(Con::Adt(d), a) => {
            g.adts.get(d).is_some_and(|x| x.name == "Rahasyam") && a.iter().all(|x| matches!(x, Ty::Str))
        }
        _ => false,
    };
    let fields = match t {
        Ty::App(Con::Adt(d), args) => match g.adts.get(d).map(|a| &a.kind) {
            Some(crate::typeck::AdtKind::Record(fs)) => {
                Some(fs.iter().map(|f| f.ty.subst(args)).collect::<Vec<_>>())
            }
            _ => None,
        },
        _ => None,
    };
    let what = &module["<ante>/".len()..];
    let shown = crate::typeck::items::show(g, t);
    let bad = match fields {
        None => Some(format!("`{shown}` record kaadhu")),
        Some(fs) => fs
            .iter()
            .find(|f| match f {
                Ty::App(Con::Undochu | Con::Varusa, a) => !flat(&a[0]),
                f => !flat(f),
            })
            .map(|f| format!("`{}` field lo pettalem", crate::typeck::items::show(g, f))),
    }?;
    Some(Diagnostic::error(
        "E1305",
        span,
        format!(
            "`{what}` ki flat record kavali (numbers, padham, undochu leka varusa vaatiki): `{shown}`: {bad}"
        ),
    ))
}

fn walk(prog: &Program, g: &Globals, shapes: &mut Vec<(FileId, Diagnostic)>) -> Vec<(FileId, Diagnostic)> {
    let mut seen: HashSet<(BodyId, Vec<Ty>)> = HashSet::new();
    let mut work: Vec<(BodyId, Vec<Ty>)> = Vec::new();
    for (i, b) in prog.bodies.iter().enumerate() {
        let generic = match b.kind {
            BodyKind::Fn(fid) => !g.fns[fid].generics.is_empty(),
            BodyKind::Closure { .. } => true,
            BodyKind::Const(_) => false,
        };
        if !generic {
            work.push((i as BodyId, Vec::new()));
        }
    }
    while let Some((bid, tys)) = work.pop() {
        if !seen.insert((bid, tys.clone())) {
            continue;
        }
        let body = &prog.bodies[bid as usize];
        let mut reach = |callee: BodyId, ctys: Vec<Ty>, span: Span| -> Option<Diagnostic> {
            let mut budget = MAX_SIZE;
            if ctys.iter().any(|t| !fits(t, &mut budget)) {
                let message = if callee == bid {
                    "ee generic function thanani thaane peddha type tho pilusthondhi, monomorphization ekkadiki aagadhu"
                } else {
                    "ee generic call ki type chaala pedhdhadhi ayipoyindhi (512 parts daatindhi). Madhyalo oka `rakam` pettandi"
                };
                return Some(Diagnostic::error("E0805", span, message));
            }
            work.push((callee, ctys));
            None
        };
        for b in &body.blocks {
            for (s, span) in b.stmts.iter().zip(&b.spans) {
                if let Stmt::Assign(_, rv) | Stmt::Replace(_, rv, _) = s {
                    if let Rvalue::Aggregate(Agg::Closure(c), _) = rv {
                        if let Some(d) = reach(*c, tys.clone(), *span) {
                            return vec![(body.file.unwrap_or(0), d)];
                        }
                    }
                    for o in rv.operands() {
                        if let Operand::Const(Const::Fn(fid, ftys)) = o {
                            if let Some(&cb) = prog.fn_bodies.get(fid) {
                                let ctys = ftys.iter().map(|t| t.subst(&tys)).collect();
                                if let Some(d) = reach(cb, ctys, *span) {
                                    return vec![(body.file.unwrap_or(0), d)];
                                }
                            }
                        }
                    }
                }
            }
            if let Term::Call { callee, span, .. } = &b.term {
                let std_body =
                    body.file.is_some_and(|f| g.res.modules[g.module_of_file(f)].name.starts_with("<ante>"));
                if let (Callee::Fn(fid, ftys), false) = (callee, std_body) {
                    let ctys: Vec<Ty> = ftys.iter().map(|t| t.subst(&tys)).collect();
                    if let Some(d) = form_call(g, *fid, &ctys, *span) {
                        let file = body.file.unwrap_or(0);
                        if !shapes.iter().any(|(f, x)| *f == file && x.span == d.span) {
                            shapes.push((file, d));
                        }
                    }
                }
                let target = match callee {
                    Callee::Fn(fid, ftys) => {
                        prog.fn_bodies.get(fid).map(|&cb| (cb, ftys.iter().map(|t| t.subst(&tys)).collect()))
                    }
                    Callee::Trait(fid, ftys) => {
                        let ctys: Vec<Ty> = ftys.iter().map(|t| t.subst(&tys)).collect();
                        // Unresolvable until the types are concrete; skip
                        // what still mentions generic parameters.
                        if ctys.iter().any(has_param) {
                            None
                        } else {
                            match resolve_trait(prog, g, *fid, &ctys) {
                                Some(Resolved::Body(cb, btys)) => Some((cb, btys)),
                                _ => None,
                            }
                        }
                    }
                    _ => None,
                };
                if let Some((cb, ctys)) = target {
                    if let Some(d) = reach(cb, ctys, *span) {
                        return vec![(body.file.unwrap_or(0), d)];
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Counts the type's nodes against `budget`, stopping as soon as it runs out
/// (types can grow exponentially, so they are never walked in full).
fn fits(t: &Ty, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match t {
        Ty::Tuple(items) | Ty::App(_, items) => items.iter().all(|x| fits(x, budget)),
        Ty::Array(e, _) | Ty::Ref(_, e) | Ty::Ptr(_, e) => fits(e, budget),
        Ty::Fn(ps, r) => ps.iter().all(|(p, _)| fits(p, budget)) && fits(r, budget),
        _ => true,
    }
}

fn has_param(t: &Ty) -> bool {
    match t {
        Ty::Param(_) => true,
        Ty::Tuple(items) | Ty::App(_, items) => items.iter().any(has_param),
        Ty::Array(e, _) | Ty::Ref(_, e) | Ty::Ptr(_, e) => has_param(e),
        Ty::Fn(ps, r) => ps.iter().any(|(p, _)| has_param(p)) || has_param(r),
        _ => false,
    }
}
