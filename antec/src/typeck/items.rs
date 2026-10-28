//! Checks on items as a whole (spec §9.2, §13.3, §13.5–13.6, §22.1):
//! implementations, derives, infinite types and foreign signatures.

use std::collections::HashSet;

use super::solve::{self, Solver};
use super::{AdtKind, FnOrigin, Globals, RecvMode, TraitRef};
use crate::diag::Diagnostic;
use crate::resolve::{FileId, SourceModule};
use crate::source::Span;
use crate::types::{Con, InferTable, Ty, VarKind};

pub const DERIVABLE: &[&str] =
    &["CopyKottochu", "XeroxTheeyochu", "Polchochu", "VarusaPettochu", "HashCheyochu", "Choopinchochu"];

pub fn check_items(g: &Globals, modules: &[SourceModule]) -> Vec<(FileId, Diagnostic)> {
    let mut out = Vec::new();
    let mut err = |file: FileId, code: &'static str, span: Span, msg: String| {
        out.push((file, Diagnostic::error(code, span, msg)));
    };

    // Implementations (§13.3).
    for (i, imp) in g.impls.iter().enumerate() {
        let Some(tr) = g.traits.get(&imp.trait_ref.def) else { continue };
        let local_trait = tr.file.is_some();
        let local_type = match &imp.self_ty {
            Ty::App(Con::Adt(d), _) => g.adts.get(d).is_some_and(|a| a.file.is_some()),
            _ => false,
        };
        if !local_trait && !local_type {
            err(
                imp.file,
                "E0803",
                imp.span,
                format!(
                    "`{}` gunam, ee type rendu mana package vi kaadhu. Mana `rakam` leka mana `gunam` lo okati kavali",
                    tr.name
                ),
            );
            continue;
        }
        for (name, fid) in &tr.methods {
            match imp.methods.get(name) {
                Some(&impl_fid) => {
                    if let Some(msg) = sig_mismatch(g, *fid, impl_fid, &imp.self_ty, &imp.trait_ref.args) {
                        let span = g.fns[impl_fid].span;
                        err(
                            imp.file,
                            "E0401",
                            span,
                            format!("`{name}` signature `{}` gunam tho match avvaledhu: {msg}", tr.name),
                        );
                    }
                }
                None if !tr.defaults.contains(name) => err(
                    imp.file,
                    "E0802",
                    imp.span,
                    format!("`{}` gunam ki `{name}` method kavali, adhi raayaledhu", tr.name),
                ),
                None => {}
            }
        }
        for (name, &fid) in &imp.methods {
            if !tr.methods.iter().any(|(n, _)| n == name) {
                err(
                    imp.file,
                    "E0406",
                    g.fns[fid].span,
                    format!("`{}` gunam lo `{name}` ane method ledhu", tr.name),
                );
            }
        }
        // Overlap with an earlier implementation (E0804).
        for prev in &g.impls[..i] {
            if prev.trait_ref.def != imp.trait_ref.def {
                continue;
            }
            let mut t = InferTable::default();
            let a: Vec<Ty> = prev.generics.iter().map(|_| t.fresh(VarKind::General)).collect();
            let b: Vec<Ty> = imp.generics.iter().map(|_| t.fresh(VarKind::General)).collect();
            let overlap = solve::unify(&mut t, &prev.self_ty.subst(&a), &imp.self_ty.subst(&b))
                && prev
                    .trait_ref
                    .args
                    .iter()
                    .zip(&imp.trait_ref.args)
                    .all(|(x, y)| solve::unify(&mut t, &x.subst(&a), &y.subst(&b)));
            if overlap {
                err(
                    imp.file,
                    "E0804",
                    imp.span,
                    format!("`{}` gunam ee type ki already implement ayyindhi", tr.name),
                );
                break;
            }
        }
        // `CopyKottochu` needs copyable fields and no destructor (E0806).
        if g.std_trait("CopyKottochu") == Some(imp.trait_ref.def) {
            if let Some(msg) = copy_problem(g, &imp.self_ty, &imp.generics) {
                err(imp.file, "E0806", imp.span, msg);
            }
        }
    }

    // `@automatic` derives (§13.6) and infinite types (E0418).
    for (&d, adt) in &g.adts {
        let Some(file) = adt.file else { continue };
        for &tdef in &adt.derives {
            let name = g.res.defs[tdef].name.as_str();
            if !DERIVABLE.contains(&name) || g.std_trait(name) != Some(tdef) {
                err(file, "E0807", adt.span, format!("`{name}` ni `@automatic` tho derive cheyyalem"));
                continue;
            }
            let args: Vec<Ty> = (0..adt.generics.len() as u32).map(Ty::Param).collect();
            let mut generics = adt.generics.clone();
            for gp in &mut generics {
                gp.bounds.push(TraitRef { def: tdef, args: Vec::new() });
            }
            let solver = Solver { g, generics: &generics };
            let mut t = InferTable::default();
            for f in super::solve::adt_field_types(g, d, &args) {
                if !solver.implements(&mut t, &f, &TraitRef { def: tdef, args: Vec::new() }, 0) {
                    err(
                        file,
                        "E0807",
                        adt.span,
                        format!("`{}` lo oka field ki `{name}` ledhu, kaabatti derive cheyyalem", adt.name),
                    );
                    break;
                }
            }
            if name == "CopyKottochu" {
                if let Some(msg) = copy_problem(g, &Ty::App(Con::Adt(d), args.clone()), &adt.generics) {
                    err(file, "E0806", adt.span, msg);
                }
            }
        }
        let mut seen = HashSet::new();
        if contains_directly(g, d, d, &mut seen) {
            err(
                file,
                "E0418",
                adt.span,
                format!(
                    "`{}` thanalo thaane unnadhi, size anantham avthundhi. `pette[{}]` vaadandi",
                    adt.name, adt.name
                ),
            );
        }
    }

    // Foreign functions (§22.1) and the entry point (§7.5).
    for sig in &g.fns {
        let Some(file) = sig.file else { continue };
        if sig.origin == FnOrigin::Foreign {
            let bad =
                sig.params.iter().map(|p| &p.ty).chain(std::iter::once(&sig.ret)).find(|t| !ffi_safe(g, t));
            if let Some(t) = bad {
                let shown = show(g, t);
                err(
                    file,
                    "E1002",
                    sig.span,
                    format!("`{shown}` C ki pampalem. Integers, floats, pointers vaadandi"),
                );
            }
            if sig.params.iter().any(|p| p.role.is_some()) {
                err(file, "E1002", sig.span, "`bayata` functions ki roles undavu".into());
            }
        }
    }
    for m in modules {
        if m.name != "modhalu" && modules.len() > 1 {
            continue;
        }
        let Some(module) = g.res.module_by_name.get(&m.name) else { continue };
        let Some(&def) = g.res.modules[*module].values.get("modhalu") else { continue };
        let Some(&fid) = g.fn_of_def.get(&def) else { continue };
        let sig = &g.fns[fid];
        let ret_ok = match &sig.ret {
            Ty::Unit | Ty::Error => true,
            Ty::App(Con::SareThappu, a) => a[0] == Ty::Unit,
            _ => false,
        };
        if !sig.params.is_empty() || !ret_ok {
            err(
                m.file,
                "E0401",
                sig.span,
                "`modhalu` ki parameters undakoodadhu, return type `khali` leka `sare_thappu[khali, E]` undali".into(),
            );
        }
    }
    out
}

/// Why `ty` can't be `CopyKottochu`, if it can't (§13.5).
fn copy_problem(g: &Globals, ty: &Ty, generics: &[super::Generic]) -> Option<String> {
    let Ty::App(Con::Adt(d), args) = ty else { return None };
    let adt = g.adts.get(d)?;
    let mut generics = generics.to_vec();
    if let Some(copy) = g.std_trait("CopyKottochu") {
        for gp in &mut generics {
            gp.bounds.push(TraitRef { def: copy, args: Vec::new() });
        }
    }
    let solver = Solver { g, generics: &generics };
    let mut t = InferTable::default();
    if let Some(sardhu) = g.std_trait("Sardhukochu") {
        if solver.find_impl(&mut t, ty, &TraitRef { def: sardhu, args: Vec::new() }, 0).is_some() {
            return Some(format!("`{}` ki `Sardhukochu` undhi, kaabatti `CopyKottochu` kaadhu", adt.name));
        }
    }
    for f in super::solve::adt_field_types(g, *d, args) {
        if !solver.is_copy(&mut t, &f) {
            return Some(format!(
                "`{}` lo `{}` field copy avvadhu, kaabatti `CopyKottochu` kaadhu. `XeroxTheeyochu` vaadandi",
                adt.name,
                show(g, &f)
            ));
        }
    }
    None
}

/// Whether ADT `d` holds `target` without indirection.
fn contains_directly(g: &Globals, d: usize, target: usize, seen: &mut HashSet<usize>) -> bool {
    if !seen.insert(d) {
        return false;
    }
    let Some(adt) = g.adts.get(&d) else { return false };
    let fields: Vec<&Ty> = match &adt.kind {
        AdtKind::Record(fs) => fs.iter().map(|f| &f.ty).collect(),
        AdtKind::Enum(vs) => vs.iter().flat_map(|v| v.fields.iter()).collect(),
    };
    fields
        .into_iter()
        .any(|t| direct_adts(t).into_iter().any(|x| x == target || contains_directly(g, x, target, seen)))
}

fn direct_adts(t: &Ty) -> Vec<usize> {
    match t {
        Ty::App(Con::Adt(d), args) => {
            let mut v = vec![*d];
            v.extend(args.iter().flat_map(direct_adts));
            v
        }
        Ty::Tuple(items) | Ty::App(Con::Undochu | Con::SareThappu, items) => {
            items.iter().flat_map(direct_adts).collect()
        }
        Ty::Array(e, _) => direct_adts(e),
        _ => Vec::new(),
    }
}

fn ffi_safe(g: &Globals, t: &Ty) -> bool {
    match t {
        Ty::Int(_) | Ty::Float(_) | Ty::Unit | Ty::Ptr(..) | Ty::Error | Ty::Never => true,
        Ty::App(Con::Adt(d), _) => g.adts.get(d).is_some_and(|a| a.c_layout),
        _ => false,
    }
}

/// Compares an implementation's method with the trait's (§13.3).
fn sig_mismatch(
    g: &Globals,
    trait_fid: usize,
    impl_fid: usize,
    self_ty: &Ty,
    trait_args: &[Ty],
) -> Option<String> {
    let tsig = &g.fns[trait_fid];
    let isig = &g.fns[impl_fid];
    if tsig.params.len() != isig.params.len() {
        return Some(format!("{} parameters kavali, {} unnayi", tsig.params.len(), isig.params.len()));
    }
    let tmode = tsig.receiver.as_ref().map(|r| r.mode);
    let imode = isig.receiver.as_ref().map(|r| r.mode);
    if tmode != imode {
        let show_mode = |m: Option<RecvMode>| match m {
            None => "`naa` lekunda",
            Some(RecvMode::Value) => "`naa: idhi`",
            Some(RecvMode::Ref) => "`naa: &idhi`",
            Some(RecvMode::RefMut) => "`naa: &maare idhi`",
        };
        return Some(format!("receiver {} kavali, {} undhi", show_mode(tmode), show_mode(imode)));
    }
    for (a, b) in tsig.params.iter().zip(&isig.params) {
        if a.role != b.role {
            return Some(format!("`{}` role tappu", b.name));
        }
    }
    // Types are compared only for methods without their own generics.
    if tsig.generics.len() > tsig.own_generics || isig.generics.len() > isig.own_generics {
        return None;
    }
    let mut subst = vec![self_ty.clone()];
    subst.extend(trait_args.iter().cloned());
    let mut t = InferTable::default();
    let impl_params: Vec<Ty> = isig.generics.iter().map(|_| t.fresh(VarKind::General)).collect();
    for (a, b) in tsig.params.iter().zip(&isig.params) {
        let ta = a.ty.subst(&subst);
        let tb = b.ty.subst(&impl_params);
        if !solve::unify(&mut t, &ta.subst(&impl_params), &tb) {
            return Some(format!("`{}` type tappu", b.name));
        }
    }
    if !solve::unify(&mut t, &tsig.ret.subst(&subst).subst(&impl_params), &isig.ret.subst(&impl_params)) {
        return Some("return type tappu".into());
    }
    None
}

/// A type as the programmer writes it (`varusa[ankam64]`), for messages.
pub fn show(g: &Globals, t: &Ty) -> String {
    let adt_name = |d: usize| g.res.defs[d].name.clone();
    let param_name = |i: u32| format!("T{i}");
    let var_kind = |_| VarKind::General;
    crate::types::Display { ty: t, adt_name: &adt_name, param_name: &param_name, var_kind: &var_kind }
        .to_string()
}
