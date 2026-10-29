//! The C boundary (§22): calls to `bayata "C"` functions and `@bayata`
//! exports.
//!
//! The C calling convention (which registers, how small records travel) is
//! clang's business, not ours. So every crossing goes through a tiny C glue
//! function that clang compiles next to the program: the LLVM side passes
//! only pointers to values laid out exactly like C's (integers, floats,
//! pointers and `@C` records), and the glue does the real C call.

use std::fmt::Write as _;

use super::func::Fx;
use super::{Gen, Item};
use crate::mir::Operand;
use crate::typeck::FnId;
use crate::types::{Con, Ty};

/// The C spelling of an FFI-safe type, defining `@C` records as needed.
pub(super) fn c_type(gen: &mut Gen, ty: &Ty) -> String {
    match ty {
        Ty::Int(i) => format!("{}int{}_t", if i.signed { "" } else { "u" }, i.bits),
        Ty::Float(32) => "float".into(),
        Ty::Float(_) => "double".into(),
        Ty::Ptr(..) => "void *".into(),
        Ty::Unit | Ty::Never => "void".into(),
        Ty::App(Con::Adt(_), _) => c_struct(gen, ty),
        // The typechecker allowed nothing else (E1002).
        _ => "void".into(),
    }
}

fn c_struct(gen: &mut Gen, ty: &Ty) -> String {
    if let Some(n) = gen.c_structs.get(ty) {
        return n.clone();
    }
    let name = format!("struct ante_c{}", gen.c_structs.len());
    gen.c_structs.insert(ty.clone(), name.clone());
    let fields = gen.variants(ty).pop().unwrap_or_default();
    let mut body = String::new();
    for (i, f) in fields.iter().enumerate() {
        let decl = match f {
            Ty::Array(e, n) => format!("{} f{i}[{}]", c_type(gen, e), n.unwrap_or(0)),
            t => format!("{} f{i}", c_type(gen, t)),
        };
        let _ = writeln!(body, "    {decl};");
    }
    // Nested records were written while filling `body`, so they come first.
    let _ = writeln!(gen.c_glue, "{name} {{\n{body}}};");
    name
}

/// Calls a `bayata "C"` function through its glue function.
pub(super) fn import_call(fx: &mut Fx, fid: FnId, args: &[Operand]) -> (String, Ty) {
    let sig = &fx.gen.g.fns[fid];
    let (name, params, ret) =
        (sig.name.clone(), sig.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>(), sig.ret.clone());
    let glue = match fx.gen.ffi.get(&fid) {
        Some(g) => g.clone(),
        None => {
            let glue = format!("ante_ffi_{}", fx.gen.ffi.len());
            fx.gen.ffi.insert(fid, glue.clone());
            let ps: Vec<(usize, String)> = params
                .iter()
                .enumerate()
                .filter(|(_, t)| !is_unit(t))
                .map(|(i, t)| (i, c_type(fx.gen, t)))
                .collect();
            let rc = c_type(fx.gen, &ret);
            let proto = if ps.is_empty() {
                "void".to_string()
            } else {
                ps.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>().join(", ")
            };
            let mut formals: Vec<String> = ps.iter().map(|(i, t)| format!("{t} *a{i}")).collect();
            let call = format!(
                "{name}({})",
                ps.iter().map(|(i, _)| format!("*a{i}")).collect::<Vec<_>>().join(", ")
            );
            let body = if is_unit(&ret) {
                format!("{call};")
            } else {
                formals.push(format!("{rc} *r"));
                format!("*r = {call};")
            };
            if formals.is_empty() {
                formals.push("void".into());
            }
            let _ = writeln!(fx.gen.c_glue, "extern {rc} {name}({proto});");
            let _ = writeln!(fx.gen.c_glue, "void {glue}({}) {{ {body} }}", formals.join(", "));
            glue
        }
    };
    let mut ptrs = Vec::new();
    for (o, t) in args.iter().zip(&params) {
        let v = fx.operand(o, t);
        if is_unit(t) {
            continue;
        }
        let lt = fx.ll(t);
        ptrs.push(format!("ptr {}", fx.spill(&lt, &v)));
    }
    let out = if is_unit(&ret) {
        None
    } else {
        let lt = fx.ll(&ret);
        let p = fx.alloca(&lt);
        ptrs.push(format!("ptr {p}"));
        Some((lt, p))
    };
    fx.gen.intrinsic_decl(&format!("declare void @{glue}({})", vec!["ptr"; ptrs.len()].join(", ")));
    fx.emit(format!("call void @{glue}({})", ptrs.join(", ")));
    match out {
        Some((lt, p)) => (fx.load(&lt, &p), ret),
        None if ret == Ty::Never => {
            fx.emit("unreachable");
            let dead = fx.label();
            fx.start(&dead);
            ("undef".into(), ret)
        }
        None => ("zeroinitializer".into(), ret),
    }
}

/// Emits every `@bayata "name"` function and its C entry point.
pub(super) fn exports(gen: &mut Gen) {
    let mut list: Vec<(FnId, String)> = gen
        .g
        .fns
        .iter()
        .enumerate()
        .filter_map(|(f, s)| s.export.as_ref().map(|(n, _)| (f, n.clone())))
        .collect();
    list.sort();
    for (k, (fid, symbol)) in list.into_iter().enumerate() {
        let Some(&body) = gen.prog.fn_bodies.get(&fid) else { continue };
        let target = gen.item(Item::Body(body, Vec::new()));
        let sig = &gen.g.fns[fid];
        let (params, ret): (Vec<Ty>, Ty) =
            (sig.params.iter().map(|p| p.ty.clone()).collect(), sig.ret.clone());
        let bridge = format!("ante_exp_{k}");

        // The LLVM side: unpack pointers, call the body, store the result.
        let mut fx = Fx::new_for_glue(gen);
        let mut decl = Vec::new();
        let mut call_args = Vec::new();
        for (i, t) in params.iter().enumerate() {
            let lt = fx.ll(t);
            if is_unit(t) {
                call_args.push(format!("{lt} zeroinitializer"));
                continue;
            }
            decl.push(format!("ptr %a{i}"));
            let v = fx.load(&lt, &format!("%a{i}"));
            call_args.push(format!("{lt} {v}"));
        }
        let rt = fx.ll(&ret);
        let r = fx.r();
        fx.emit(format!("{r} = call {rt} {target}({})", call_args.join(", ")));
        if !is_unit(&ret) {
            decl.push("ptr %r".into());
            fx.emit(format!("store {rt} {r}, ptr %r"));
        }
        let code = fx.code();
        let _ = write!(
            gen.funcs,
            "define void @{bridge}({}) {{\nentry:\n{code}  ret void\n}}\n\n",
            decl.join(", ")
        );

        // The C side: the exported symbol itself.
        let cps: Vec<(usize, String)> = params
            .iter()
            .enumerate()
            .filter(|(_, t)| !is_unit(t))
            .map(|(i, t)| (i, c_type(gen, t)))
            .collect();
        let rc = c_type(gen, &ret);
        let mut bridge_formals: Vec<String> = cps.iter().map(|(_, t)| format!("{t} *")).collect();
        let mut pass: Vec<String> = cps.iter().map(|(i, _)| format!("&a{i}")).collect();
        let formals = if cps.is_empty() {
            "void".to_string()
        } else {
            cps.iter().map(|(i, t)| format!("{t} a{i}")).collect::<Vec<_>>().join(", ")
        };
        let body = if is_unit(&ret) {
            format!("{bridge}({});", pass.join(", "))
        } else {
            bridge_formals.push(format!("{rc} *"));
            pass.push("&r".into());
            format!("{rc} r; {bridge}({}); return r;", pass.join(", "))
        };
        if bridge_formals.is_empty() {
            bridge_formals.push("void".into());
        }
        let _ = writeln!(gen.c_glue, "extern void {bridge}({});", bridge_formals.join(", "));
        let _ = writeln!(gen.c_glue, "{rc} {symbol}({formals}) {{ {body} }}");
    }
}

fn is_unit(t: &Ty) -> bool {
    matches!(t, Ty::Unit | Ty::Never)
}
