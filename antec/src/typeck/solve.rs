//! Unification and trait solving (spec §9.4, §13.3, §13.5).

use super::{AdtKind, Generic, Globals, TraitRef};
use crate::types::{Con, InferTable, IntTy, Ty, VarKind};

/// Makes two types equal, binding inference variables. Returns false on a
/// mismatch; variables bound before the mismatch stay bound, which only
/// sharpens later messages.
pub fn unify(t: &mut InferTable, a: &Ty, b: &Ty) -> bool {
    let a = t.shallow(a);
    let b = t.shallow(b);
    match (&a, &b) {
        (Ty::Error, _) | (_, Ty::Error) => true,
        (Ty::Var(x), Ty::Var(y)) => t.union(*x, *y),
        (Ty::Var(v), other) | (other, Ty::Var(v)) => bind(t, *v, other),
        (Ty::Int(x), Ty::Int(y)) => x == y,
        (Ty::Float(x), Ty::Float(y)) => x == y,
        (Ty::Tuple(xs), Ty::Tuple(ys)) | (Ty::App(_, xs), Ty::App(_, ys)) => {
            let same_con = match (&a, &b) {
                (Ty::App(c, _), Ty::App(d, _)) => c == d,
                _ => true,
            };
            same_con && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| unify(t, x, y))
        }
        (Ty::Array(x, n), Ty::Array(y, m)) => (n.is_none() || m.is_none() || n == m) && unify(t, x, y),
        (Ty::Ref(m1, x), Ty::Ref(m2, y)) | (Ty::Ptr(m1, x), Ty::Ptr(m2, y)) => m1 == m2 && unify(t, x, y),
        (Ty::Fn(ps, r), Ty::Fn(qs, s)) => {
            ps.len() == qs.len() && ps.iter().zip(qs).all(|((x, _), (y, _))| unify(t, x, y)) && unify(t, r, s)
        }
        _ => a == b,
    }
}

fn bind(t: &mut InferTable, v: u32, ty: &Ty) -> bool {
    let ok = match t.kind(v) {
        VarKind::General => true,
        VarKind::Int => matches!(ty, Ty::Int(_)),
        VarKind::Float => matches!(ty, Ty::Float(_)),
    };
    if !ok || t.occurs(v, ty) {
        return false;
    }
    t.bind(v, ty.clone());
    true
}

/// Whether a value of type `actual` may be used where `expected` is wanted,
/// applying the implicit coercions of §9.4.2.
pub fn coerce(t: &mut InferTable, actual: &Ty, expected: &Ty) -> bool {
    let a = t.shallow(actual);
    let e = t.shallow(expected);
    match (&a, &e) {
        (Ty::Never, _) => true,
        (Ty::Ref(m1, x), Ty::Ref(m2, y)) if *m1 || !*m2 => coerce_pointee(t, x, y),
        // `&x` where a raw pointer is expected (§22.2).
        (Ty::Ref(m1, x), Ty::Ptr(m2, y)) if *m1 || !*m2 => unify(t, x, y),
        // `*maare T` → `*T` (§9.4.2f).
        (Ty::Ptr(true, x), Ty::Ptr(false, y)) => unify(t, x, y),
        _ => unify(t, &a, &e),
    }
}

/// `&varusa[T]`, `&[T; N]` → `&mukka[T]`; `&pette[T]` → `&T` (§9.4.2b–c).
fn coerce_pointee(t: &mut InferTable, x: &Ty, y: &Ty) -> bool {
    let x = t.shallow(x);
    let y = t.shallow(y);
    match (&x, &y) {
        (Ty::App(Con::Varusa, a), Ty::App(Con::Mukka, b)) => unify(t, &a[0], &b[0]),
        (Ty::Array(a, _), Ty::App(Con::Mukka, b)) => unify(t, a, &b[0]),
        (Ty::App(Con::Pette, a), other) if !matches!(other, Ty::App(Con::Pette, _) | Ty::Var(_)) => {
            coerce_pointee(t, &a[0], &y)
        }
        _ => unify(t, &x, &y),
    }
}

/// Answers "does this type implement that trait?" for one body.
pub struct Solver<'s, 'g> {
    pub g: &'s Globals<'g>,
    /// The generic parameters in scope, for `Param` bounds.
    pub generics: &'s [Generic],
}

impl Solver<'_, '_> {
    fn trait_name(&self, def: usize) -> &str {
        &self.g.res.defs[def].name
    }

    fn is_std(&self, def: usize, name: &str) -> bool {
        self.g.std_traits.get(name) == Some(&def)
    }

    pub fn is_copy(&self, t: &mut InferTable, ty: &Ty) -> bool {
        match self.g.std_traits.get("CopyKottochu") {
            Some(&d) => self.implements(t, ty, &TraitRef { def: d, args: Vec::new() }, 0),
            None => false,
        }
    }

    /// `ty: Trait[args]`. Unresolved variables are assumed to satisfy the
    /// bound; bounds are checked again once inference has finished.
    pub fn implements(&self, t: &mut InferTable, ty: &Ty, tr: &TraitRef, depth: u32) -> bool {
        if depth > 32 {
            return false;
        }
        let ty = t.shallow(ty);
        let name = self.trait_name(tr.def).to_string();
        let std = self.is_std(tr.def, &name);
        match &ty {
            Ty::Error | Ty::Var(_) => return true,
            Ty::Never => return true,
            Ty::Param(i) => {
                return self.generics.get(*i as usize).is_some_and(|g| {
                    g.bounds.iter().any(|b| b.def == tr.def && b.args.len() == tr.args.len())
                });
            }
            _ => {}
        }
        if std {
            if let Some(answer) = self.builtin_impl(t, &ty, &name, tr, depth) {
                return answer;
            }
        }
        // `@automatic` derives.
        if let Ty::App(Con::Adt(d), args) = &ty {
            if let Some(adt) = self.g.adts.get(d) {
                if adt.derives.contains(&tr.def) {
                    let fields = adt_field_types(self.g, *d, args);
                    return fields.iter().all(|f| self.implements(t, f, tr, depth + 1));
                }
            }
        }
        self.find_impl(t, &ty, tr, depth).is_some()
    }

    /// The index of a user implementation that applies, with its generics
    /// instantiated. A failed attempt leaves the table as it was.
    pub fn find_impl(
        &self,
        t: &mut InferTable,
        ty: &Ty,
        tr: &TraitRef,
        depth: u32,
    ) -> Option<(usize, Vec<Ty>)> {
        for (i, imp) in self.g.impls.iter().enumerate() {
            if imp.trait_ref.def != tr.def {
                continue;
            }
            let snap = t.snapshot();
            let fresh: Vec<Ty> = imp.generics.iter().map(|_| t.fresh(VarKind::General)).collect();
            let ok = unify(t, &imp.self_ty.subst(&fresh), ty)
                && imp.trait_ref.args.len() == tr.args.len()
                && imp.trait_ref.args.iter().zip(&tr.args).all(|(a, b)| unify(t, &a.subst(&fresh), b))
                && imp.generics.iter().enumerate().all(|(gi, g)| {
                    g.bounds.iter().all(|b| {
                        let b =
                            TraitRef { def: b.def, args: b.args.iter().map(|a| a.subst(&fresh)).collect() };
                        self.implements(t, &fresh[gi], &b, depth + 1)
                    })
                });
            if ok {
                t.commit(snap);
                return Some((i, fresh));
            }
            t.rollback_to(snap);
        }
        None
    }

    fn builtin_impl(
        &self,
        t: &mut InferTable,
        ty: &Ty,
        name: &str,
        tr: &TraitRef,
        depth: u32,
    ) -> Option<bool> {
        // Standard library types (§21.3): every one can be shown, copied
        // with `xerox` and compared; `Kramam` is also ordered, hashable and `CopyKottochu`.
        if let Ty::App(Con::Adt(d), _) = ty {
            if self.g.adts.get(d).is_some_and(|a| a.file.is_none()) {
                let kramam = self.g.kramam == Some(*d);
                return Some(match name {
                    "Choopinchochu" | "XeroxTheeyochu" | "Polchochu" => true,
                    "VarusaPettochu" | "HashCheyochu" | "CopyKottochu" => kramam,
                    _ => return None,
                });
            }
        }
        let all =
            |t: &mut InferTable, items: &[Ty]| items.iter().all(|x| self.implements(t, x, tr, depth + 1));
        let scalar = matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Unit);
        let answer = match name {
            "CopyKottochu" => match ty {
                _ if scalar => true,
                Ty::Ref(false, _) | Ty::Ptr(..) => true,
                // A `pani` shares its environment; copies are counted (§10.7.4).
                Ty::Ref(true, _) | Ty::Str | Ty::Fn(..) => false,
                Ty::Tuple(items) => all(t, items),
                Ty::Array(e, _) => all(t, std::slice::from_ref(e)),
                Ty::App(Con::Undochu | Con::SareThappu | Con::Paridhi, args) => all(t, args),
                Ty::App(Con::Adt(_), _) => return None,
                _ => false,
            },
            "XeroxTheeyochu" | "Polchochu" | "Choopinchochu" | "VarusaPettochu" | "HashCheyochu" => {
                let ordered = matches!(name, "VarusaPettochu" | "HashCheyochu");
                match ty {
                    Ty::Float(_) if ordered => false,
                    _ if scalar => true,
                    Ty::Str => true,
                    Ty::Ref(_, inner) if name != "XeroxTheeyochu" => all(t, std::slice::from_ref(inner)),
                    Ty::Ref(false, _) => true,
                    Ty::Tuple(items) => all(t, items),
                    Ty::Array(e, _) => all(t, std::slice::from_ref(e)),
                    Ty::App(Con::Jodi | Con::GajibijiJodi, _) if ordered => false,
                    Ty::App(
                        Con::Varusa
                        | Con::Undochu
                        | Con::SareThappu
                        | Con::Jodi
                        | Con::GajibijiJodi
                        | Con::Gumpu
                        | Con::Pette
                        | Con::Mukka,
                        args,
                    ) => all(t, args),
                    Ty::App(Con::Adt(_), _) => return None,
                    Ty::Fn(..) => name == "XeroxTheeyochu",
                    _ => false,
                }
            }
            // Arithmetic on the built-in numbers, and `padham + padham` (§10.5.2).
            "Kalupochu" | "Theeseyochu" | "Guninchochu" | "Panchochu" | "Migulochu" | "Thippochu" => {
                let builtin = match name {
                    "Kalupochu" => matches!(ty, Ty::Int(_) | Ty::Float(_) | Ty::Str),
                    "Thippochu" => matches!(ty, Ty::Int(IntTy { signed: true, .. }) | Ty::Float(_)),
                    _ => matches!(ty, Ty::Int(_) | Ty::Float(_)),
                };
                if !builtin {
                    return None;
                }
                true
            }
            "Okkokkati" => match ty {
                Ty::App(Con::Pravaham | Con::Paridhi, args) => {
                    tr.args.len() == 1 && unify(t, &tr.args[0], &args[0])
                }
                _ => return None,
            },
            _ => return None,
        };
        Some(answer)
    }
}

/// Field (or variant field) types of an ADT instance.
pub fn adt_field_types(g: &Globals, def: usize, args: &[Ty]) -> Vec<Ty> {
    match g.adts.get(&def).map(|a| &a.kind) {
        Some(AdtKind::Record(fields)) => fields.iter().map(|f| f.ty.subst(args)).collect(),
        Some(AdtKind::Enum(variants)) => {
            variants.iter().flat_map(|v| v.fields.iter().map(|f| f.subst(args))).collect()
        }
        None => Vec::new(),
    }
}
