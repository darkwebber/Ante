//! The monomorphization check (§13.1, E0805): starting from every
//! non-generic body, walks each generic instantiation a program can reach,
//! as the native back end would generate them, and reports a call whose
//! type arguments keep growing (polymorphic recursion).

use std::collections::HashSet;

use super::*;
use crate::diag::Diagnostic;
use crate::typeck::Globals;

/// How large type arguments may grow (in type nodes) before instantiation
/// is taken to run away.
const MAX_SIZE: usize = 512;

pub fn check(prog: &Program, g: &Globals) -> Vec<(FileId, Diagnostic)> {
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
                return Some(Diagnostic::error(
                    "E0805",
                    span,
                    "ee generic function thanani thaane peddha type tho pilusthondhi, monomorphization ekkadiki aagadhu",
                ));
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
