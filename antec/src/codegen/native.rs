//! The standard library and compiler intrinsics in native code. What isn't
//! here yet reports a clear error (`antec run` still runs it).

use super::func::Fx;
use super::glue::{call_cmp, call_xerox, eq_call};
use super::{float_lit, Adapter, Gen, Item};
use crate::interp::Value;
use crate::mir::{Intrinsic, Operand};
use crate::source::Span;
use crate::typeck::{FnId, RecvMode};
use crate::types::{Con, IntTy, Ty};

/// The formal types of a standard function (receiver first), instantiated.
fn formals(fx: &Fx, fid: FnId, tys: &[Ty]) -> (Vec<Ty>, Ty) {
    let sig = &fx.gen.g.fns[fid];
    let mut out = Vec::new();
    if let Some(r) = &sig.receiver {
        let t = r.ty.subst(tys);
        out.push(match r.mode {
            RecvMode::Value => t,
            RecvMode::Ref => Ty::reference(false, t),
            RecvMode::RefMut => Ty::reference(true, t),
        });
    }
    out.extend(sig.params.iter().map(|p| p.ty.subst(tys)));
    (out, sig.ret.subst(tys))
}

fn std_key(fx: &Fx, fid: FnId) -> String {
    let sig = &fx.gen.g.fns[fid];
    let owner = sig.owner.clone().unwrap_or_else(|| {
        if sig.module == 0 {
            "prelude".into()
        } else {
            fx.gen.g.res.modules[sig.module].name.clone()
        }
    });
    format!("{owner}.{}", sig.name)
}

fn unsupported(fx: &mut Fx, what: &str, ty: &Ty) -> (String, Ty) {
    fx.gen.error(format!("native backend lo `{what}` inka ledhu; ippatiki `antec run` vaadandi"));
    ("undef".into(), ty.clone())
}

/// Data pointer and length of a sequence *value* of type `ty`
/// (`&mukka`, a reference to a vector or array, or a vector).
fn seq_val(fx: &mut Fx, v: &str, ty: &Ty) -> Option<(String, String, Ty)> {
    match ty {
        Ty::Ref(_, inner) => match &**inner {
            Ty::App(Con::Mukka, a) => {
                let d = fx.r();
                fx.emit(format!("{d} = extractvalue %slice {v}, 0"));
                let n = fx.r();
                fx.emit(format!("{n} = extractvalue %slice {v}, 1"));
                Some((d, n, a[0].clone()))
            }
            Ty::App(Con::Varusa, a) => {
                let dp = fx.gep("%vec", v, &["0".into(), "0".into()]);
                let d = fx.load("ptr", &dp);
                let lp = fx.gep("%vec", v, &["0".into(), "1".into()]);
                let n = fx.load("i64", &lp);
                Some((d, n, a[0].clone()))
            }
            Ty::Array(e, n) => Some((v.to_string(), n.unwrap_or(0).to_string(), (**e).clone())),
            Ty::Ref(..) => {
                let lt = fx.ll(inner);
                let p = fx.load(&lt, v);
                seq_val(fx, &p, inner)
            }
            _ => None,
        },
        Ty::App(Con::Varusa, a) => {
            let d = fx.r();
            fx.emit(format!("{d} = extractvalue %vec {v}, 0"));
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %vec {v}, 1"));
            Some((d, n, a[0].clone()))
        }
        _ => None,
    }
}

fn some(fx: &mut Fx, ty: &Ty, v: &str, inner: &Ty) -> String {
    fx.build_enum(ty, 0, &[v.to_string()], std::slice::from_ref(inner))
}

fn none(fx: &mut Fx, ty: &Ty) -> String {
    fx.build_enum(ty, 1, &[], &[])
}

fn opt_inner(ty: &Ty) -> Ty {
    match ty {
        Ty::App(Con::Undochu | Con::SareThappu, a) => a[0].clone(),
        _ => Ty::Unit,
    }
}

/// `cond ? some(v) : none` for an `undochu`, through memory.
pub(super) fn opt_select(
    fx: &mut Fx,
    ty: &Ty,
    cond: &str,
    make: &mut dyn FnMut(&mut Fx) -> String,
) -> String {
    let lt = fx.ll(ty);
    let slot = fx.alloca(&lt);
    let yes = fx.label();
    let no = fx.label();
    let done = fx.label();
    fx.emit(format!("br i1 {cond}, label %{yes}, label %{no}"));
    fx.start(&yes);
    let inner = opt_inner(ty);
    let v = make(fx);
    let s = some(fx, ty, &v, &inner);
    fx.emit(format!("store {lt} {s}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&no);
    let n = none(fx, ty);
    fx.emit(format!("store {lt} {n}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    fx.load(&lt, &slot)
}

/// Writes `v` (of type `ty`) into a buffer the way `choopinchu` shows it.
fn show_value(fx: &mut Fx, buf: &str, v: &str, ty: &Ty) {
    let lt = fx.ll(ty);
    let p = fx.spill(&lt, v);
    let f = fx.gen.item(Item::Show(ty.clone()));
    fx.emit(format!("call void {f}(ptr {buf}, ptr {p})"));
}

/// Shows the value a `&T` argument points at. An unsized `T` (`mukka`)
/// arrives as the slice itself.
fn show_ref(fx: &mut Fx, buf: &str, arg: &str, t: &Ty) {
    if let Ty::App(Con::Mukka, _) = t {
        let p = fx.spill("%slice", arg);
        let f = fx.gen.item(Item::Show(Ty::reference(false, t.clone())));
        fx.emit(format!("call void {f}(ptr {buf}, ptr {p})"));
        return;
    }
    let f = fx.gen.item(Item::Show(t.clone()));
    fx.emit(format!("call void {f}(ptr {buf}, ptr {arg})"));
}

fn trap_text(fx: &mut Fx, msg: &str, span: Span) {
    let loc = fx.loc(span);
    let s = fx.spill("%str", msg);
    fx.emit(format!("call void @ante_trap(ptr {s}, ptr {loc})"));
    fx.emit("unreachable");
}

fn new_buf(fx: &mut Fx) -> String {
    let b = fx.alloca("%str");
    fx.emit(format!("store %str zeroinitializer, ptr {b}"));
    b
}

fn push_lit(fx: &mut Fx, buf: &str, text: &str) {
    let (p, n) = fx.gen.str_ptr(text);
    fx.emit(format!("call void @ante_buf_push(ptr {buf}, ptr {p}, i64 {n})"));
}

/// `sare(..)` or `thappu(E{text})` from a runtime call that returns `i32`
/// 1 on success. `call` gets the error-text slot and emits the call; `ok`
/// builds the success value.
fn c_result(
    fx: &mut Fx,
    ret: &Ty,
    call: &mut dyn FnMut(&mut Fx, &str) -> String,
    ok: &mut dyn FnMut(&mut Fx) -> String,
) -> String {
    let err = fx.alloca("%str");
    let flag = call(fx, &err);
    let good = fx.r();
    fx.emit(format!("{good} = icmp ne i32 {flag}, 0"));
    let ok_ty = opt_inner(ret);
    let err_ty = match ret {
        Ty::App(Con::SareThappu, a) => a[1].clone(),
        _ => Ty::Unit,
    };
    let lt = fx.ll(ret);
    let slot = fx.alloca(&lt);
    let (yes, no, done) = (fx.label(), fx.label(), fx.label());
    fx.emit(format!("br i1 {good}, label %{yes}, label %{no}"));
    fx.start(&yes);
    let x = ok(fx);
    let e = fx.build_enum(ret, 0, &[x], std::slice::from_ref(&ok_ty));
    fx.emit(format!("store {lt} {e}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&no);
    let m = fx.load("%str", &err);
    let et = fx.ll(&err_ty);
    let rec = fx.r();
    fx.emit(format!("{rec} = insertvalue {et} undef, %str {m}, 0"));
    let e = fx.build_enum(ret, 1, &[rec], std::slice::from_ref(&err_ty));
    fx.emit(format!("store {lt} {e}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    fx.load(&lt, &slot)
}

/// Calls a runtime function `@f(args…, ptr out, ptr err)` filling `out` of
/// LLVM type `out_lt` on success, as a `sare_thappu`.
fn c_result_out(fx: &mut Fx, ret: &Ty, f: &str, args: &str, out_lt: &str) -> String {
    let out = fx.alloca(out_lt);
    let (f, args, o) = (f.to_string(), args.to_string(), out.clone());
    let lt = out_lt.to_string();
    c_result(
        fx,
        ret,
        &mut |fx, err| {
            let r = fx.r();
            let sep = if args.is_empty() { "" } else { ", " };
            fx.emit(format!("{r} = call i32 @{f}({args}{sep}ptr {o}, ptr {err})"));
            r
        },
        &mut |fx| fx.load(&lt, &out),
    )
}

/// Like `c_result_out` for calls with no success value (`khali`).
fn c_result_unit(fx: &mut Fx, ret: &Ty, f: &str, args: &str) -> String {
    let (f, args) = (f.to_string(), args.to_string());
    c_result(
        fx,
        ret,
        &mut |fx, err| {
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @{f}({args}, ptr {err})"));
            r
        },
        &mut |_| "zeroinitializer".into(),
    )
}

// ── intrinsics ─────────────────────────────────────────────────────────────

pub(super) fn intrinsic(
    fx: &mut Fx,
    i: &Intrinsic,
    args: &[Operand],
    dest: &Ty,
    _span: Span,
) -> (String, Ty) {
    match i {
        Intrinsic::IntoIter { ty, .. } => {
            let ty = fx.subst(ty);
            let item = match dest {
                Ty::App(Con::Pravaham, a) => a[0].clone(),
                _ => Ty::Unit,
            };
            let at = fx.operand_ty(&args[0]).unwrap_or(ty.clone());
            let v = fx.operand(&args[0], &at);
            (into_iter(fx, &v, &at, &item), dest.clone())
        }
        Intrinsic::IterNext => {
            let item = match dest {
                Ty::App(Con::Undochu, a) => a[0].clone(),
                _ => Ty::Unit,
            };
            let it = fx.operand(&args[0], &Ty::reference(true, Ty::App(Con::Pravaham, vec![item.clone()])));
            (call_next(fx, &it, &item), dest.clone())
        }
        Intrinsic::Format { texts, tys } => {
            let buf = new_buf(fx);
            for (k, t) in texts.iter().enumerate() {
                if !t.is_empty() {
                    push_lit(fx, &buf, t);
                }
                if let (Some(a), Some(ty)) = (args.get(k), tys.get(k)) {
                    let ty = fx.subst(ty);
                    let p = fx.operand(a, &Ty::reference(false, ty.clone()));
                    let f = fx.gen.item(Item::Show(ty));
                    fx.emit(format!("call void {f}(ptr {buf}, ptr {p})"));
                }
            }
            (fx.load("%str", &buf), Ty::Str)
        }
        Intrinsic::Collect(flags) => {
            let elem = match dest {
                Ty::App(Con::Varusa, a) => a[0].clone(),
                _ => Ty::Unit,
            };
            let (size, _) = fx.gen.size_align(&elem);
            let et = fx.ll(&elem);
            let vec = fx.alloca("%vec");
            fx.emit(format!("store %vec zeroinitializer, ptr {vec}"));
            for (a, splice) in args.iter().zip(flags) {
                if *splice {
                    let at = fx.operand_ty(a).unwrap_or(Ty::Unit);
                    let v = fx.operand(a, &at);
                    if let Some((d, n, _)) = seq_val(fx, &v, &at) {
                        fx.emit(format!(
                            "call void @ante_vec_extend(ptr {vec}, ptr {d}, i64 {n}, i64 {size})"
                        ));
                    }
                } else {
                    let v = fx.operand(a, &elem);
                    let p = fx.spill(&et, &v);
                    fx.emit(format!("call void @ante_vec_push(ptr {vec}, ptr {p}, i64 {size})"));
                }
            }
            (fx.load("%vec", &vec), dest.clone())
        }
        Intrinsic::Const(def) => {
            let value = crate::interp::const_value(fx.gen.prog, fx.gen.tc, fx.gen.files, *def);
            match value {
                Some(v) => (materialize(fx, &v, dest), dest.clone()),
                None => unsupported(fx, "ee constant", dest),
            }
        }
        Intrinsic::Unsupported(what) => unsupported(fx, what, dest),
    }
}

/// Builds a constant computed by the interpreter as code.
fn materialize(fx: &mut Fx, v: &Value, ty: &Ty) -> String {
    match (v, ty) {
        (Value::Int(i), Ty::Int(it)) => crate::interp::wrap(*it, *i).to_string(),
        (Value::F64(x), Ty::Float(b)) => float_lit(*b, *x),
        (Value::F32(x), Ty::Float(b)) => float_lit(*b, f64::from(*x)),
        (Value::Bool(b), _) => b.to_string(),
        (Value::Char(c), _) => (*c as u32).to_string(),
        (Value::Str(s), _) => fx.gen.str_const(s),
        (Value::Unit, _) => "zeroinitializer".into(),
        (Value::Tuple(xs) | Value::Array(xs), Ty::Tuple(_) | Ty::Array(..)) => {
            let fields: Vec<Ty> = match ty {
                Ty::Tuple(items) => items.clone(),
                Ty::Array(e, _) => vec![(**e).clone(); xs.len()],
                _ => Vec::new(),
            };
            let vals: Vec<String> = xs.iter().zip(&fields).map(|(x, t)| materialize(fx, x, t)).collect();
            let lt = fx.ll(ty);
            fx.build_struct_vals(&lt, &vals, &fields)
        }
        (Value::Adt(variant, xs), _) => {
            let fields = fx.gen.variants(ty).get(*variant as usize).cloned().unwrap_or_default();
            let vals: Vec<String> = xs.iter().zip(&fields).map(|(x, t)| materialize(fx, x, t)).collect();
            if fx.gen.is_enum(ty) {
                fx.build_enum(ty, *variant, &vals, &fields)
            } else {
                let lt = fx.ll(ty);
                fx.build_struct_vals(&lt, &vals, &fields)
            }
        }
        (Value::Vec(xs), Ty::App(Con::Varusa, a)) => {
            let vals: Vec<String> = xs.iter().map(|x| materialize(fx, x, &a[0])).collect();
            fx.build_vec(&a[0], &vals)
        }
        _ => {
            fx.gen.error("native backend: ee constant value ni build cheyyalem");
            "undef".into()
        }
    }
}

// ── iterators (§21.4) ──────────────────────────────────────────────────────
//
// `%iter = { i32 kind, i8 done, i64 a, i64 b, ptr p, i64 len }`:
// kind 0 range (a = next, b = last), kind 1 sequence (p, len, a = position),
// kind 2 text characters (p, len, a = byte position), kind 3 adapters,
// kind 4 maps (map.rs).

fn make_iter(fx: &mut Fx, kind: u32, done: &str, a: &str, b: &str, p: &str, len: &str) -> String {
    let parts = [
        format!("i32 {kind}"),
        format!("i8 {done}"),
        format!("i64 {a}"),
        format!("i64 {b}"),
        format!("ptr {p}"),
        format!("i64 {len}"),
    ];
    let mut acc = "undef".to_string();
    for (i, part) in parts.iter().enumerate() {
        let r = fx.r();
        fx.emit(format!("{r} = insertvalue %iter {acc}, {part}, {i}"));
        acc = r;
    }
    acc
}

fn into_iter(fx: &mut Fx, v: &str, ty: &Ty, item: &Ty) -> String {
    match ty {
        Ty::App(Con::Pravaham, _) => v.to_string(),
        Ty::App(Con::Paridhi, a) => {
            let Ty::Int(it) = a[0] else { return unsupported(fx, "ee range", ty).0 };
            let lt = fx.ll(ty);
            let lo = fx.r();
            fx.emit(format!("{lo} = extractvalue {lt} {v}, 0"));
            let hi = fx.r();
            fx.emit(format!("{hi} = extractvalue {lt} {v}, 1"));
            let inc = fx.r();
            fx.emit(format!("{inc} = extractvalue {lt} {v}, 2"));
            let lo64 = fx.widen(&lo, it);
            let hi64 = fx.widen(&hi, it);
            let cmp = if it.signed { "sgt" } else { "ugt" };
            let cmpe = if it.signed { "sge" } else { "uge" };
            // Inclusive: empty when lo > hi; exclusive: empty when lo >= hi.
            let e1 = fx.r();
            fx.emit(format!("{e1} = icmp {cmp} i64 {lo64}, {hi64}"));
            let e2 = fx.r();
            fx.emit(format!("{e2} = icmp {cmpe} i64 {lo64}, {hi64}"));
            let empty = fx.r();
            fx.emit(format!("{empty} = select i1 {inc}, i1 {e1}, i1 {e2}"));
            let last_ex = fx.r();
            fx.emit(format!("{last_ex} = sub i64 {hi64}, 1"));
            let last = fx.r();
            fx.emit(format!("{last} = select i1 {inc}, i64 {hi64}, i64 {last_ex}"));
            let done = fx.r();
            fx.emit(format!("{done} = zext i1 {empty} to i8"));
            make_iter(fx, 0, &done, &lo64, &last, "null", "0")
        }
        // `done` marks a sequence the iterator owns: its drop frees the buffer
        // and drops the items not taken.
        _ => match seq_val(fx, v, ty) {
            Some((d, n, _)) => {
                let owned = if matches!(ty, Ty::Ref(..)) { "0" } else { "1" };
                make_iter(fx, 1, owned, "0", "0", &d, &n)
            }
            None => match ty {
                Ty::Array(e, n) => {
                    // An owned array: move it to the heap so the iterator owns it.
                    let lt = fx.ll(ty);
                    let (size, _) = fx.gen.size_align(ty);
                    let p = fx.r();
                    fx.emit(format!("{p} = call ptr @ante_alloc(i64 {size})"));
                    fx.emit(format!("store {lt} {v}, ptr {p}"));
                    let _ = e;
                    make_iter(fx, 1, "1", "0", "0", &p, &n.unwrap_or(0).to_string())
                }
                Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => {
                    // An owned map moves to the heap; the iterator frees it.
                    let p = fx.r();
                    fx.emit(format!("{p} = call ptr @ante_alloc(i64 48)"));
                    fx.emit(format!("store %map {v}, ptr {p}"));
                    make_iter(fx, 4, "1", "0", "0", &p, "0")
                }
                Ty::Ref(_, inner) if super::map::key_val(inner).is_some() => {
                    make_iter(fx, 4, "0", "0", "0", v, "0")
                }
                _ => match user_iterator(fx, ty, item) {
                    Some(_) => {
                        let ptr = match ty {
                            // `&maare it`: step the caller's iterator in place.
                            Ty::Ref(..) => v.to_string(),
                            // An owned iterator moves to the heap with the loop.
                            other => {
                                let lt = fx.ll(other);
                                let (size, _) = fx.gen.size_align(other);
                                let p = fx.r();
                                fx.emit(format!("{p} = call ptr @ante_alloc(i64 {size})"));
                                fx.emit(format!("store {lt} {v}, ptr {p}"));
                                p
                            }
                        };
                        make_adapter(
                            fx,
                            Adapter::User,
                            vec![ty.clone(), item.clone()],
                            &[("ptr".into(), ptr)],
                        )
                    }
                    None => unsupported(fx, "ee iterable (jodi, gumpu)", ty).0,
                },
            },
        },
    }
}

fn iter_next(fx: &mut Fx, it: &str, item: &Ty, opt: &Ty) -> String {
    let lt = fx.ll(opt);
    let out = fx.alloca(&lt);
    let n = none(fx, opt);
    fx.emit(format!("store {lt} {n}, ptr {out}"));
    let kp = fx.gep("%iter", it, &["0".into(), "0".into()]);
    let kind = fx.load("i32", &kp);
    let dp = fx.gep("%iter", it, &["0".into(), "1".into()]);
    let ap = fx.gep("%iter", it, &["0".into(), "2".into()]);
    let bp = fx.gep("%iter", it, &["0".into(), "3".into()]);
    let pp = fx.gep("%iter", it, &["0".into(), "4".into()]);
    let np = fx.gep("%iter", it, &["0".into(), "5".into()]);
    let done = fx.label();
    let k0 = fx.label();
    let k1 = fx.label();
    let k2 = fx.label();
    let k3 = fx.label();
    let k4 = fx.label();
    fx.emit(format!(
        "switch i32 {kind}, label %{done} [ i32 0, label %{k0} i32 1, label %{k1} i32 2, label %{k2} i32 3, label %{k3} i32 4, label %{k4} ]"
    ));
    fx.start(&k4);
    super::map::iter_next(fx, it, item, opt, &out, &done);
    // Adapters: a heap state whose first field is its `next` function.
    fx.start(&k3);
    {
        let state = fx.load("ptr", &pp);
        let f = fx.load("ptr", &state);
        let il = fx.ll(item);
        let slot = fx.alloca(&il);
        let got = fx.r();
        fx.emit(format!("{got} = call i1 {f}(ptr {state}, ptr {slot})"));
        let go = fx.label();
        fx.emit(format!("br i1 {got}, label %{go}, label %{done}"));
        fx.start(&go);
        let x = fx.load(&il, &slot);
        let s = some(fx, opt, &x, item);
        fx.emit(format!("store {lt} {s}, ptr {out}"));
        fx.emit(format!("br label %{done}"));
    }
    // Ranges.
    fx.start(&k0);
    if let Ty::Int(ii) = item {
        let d = fx.load("i8", &dp);
        let fin = fx.r();
        fx.emit(format!("{fin} = icmp ne i8 {d}, 0"));
        let go = fx.label();
        fx.emit(format!("br i1 {fin}, label %{done}, label %{go}"));
        fx.start(&go);
        let a = fx.load("i64", &ap);
        let b = fx.load("i64", &bp);
        let last = fx.r();
        fx.emit(format!("{last} = icmp eq i64 {a}, {b}"));
        let nd = fx.r();
        fx.emit(format!("{nd} = zext i1 {last} to i8"));
        fx.emit(format!("store i8 {nd}, ptr {dp}"));
        let a1 = fx.r();
        fx.emit(format!("{a1} = add i64 {a}, 1"));
        let a2 = fx.r();
        fx.emit(format!("{a2} = select i1 {last}, i64 {a}, i64 {a1}"));
        fx.emit(format!("store i64 {a2}, ptr {ap}"));
        let v = fx.narrow(&a, *ii);
        let s = some(fx, opt, &v, item);
        fx.emit(format!("store {lt} {s}, ptr {out}"));
    }
    fx.emit(format!("br label %{done}"));
    // Sequences: values (a copy, or a move out of an owned vector) or references.
    fx.start(&k1);
    {
        let a = fx.load("i64", &ap);
        let n = fx.load("i64", &np);
        let more = fx.r();
        fx.emit(format!("{more} = icmp ult i64 {a}, {n}"));
        let go = fx.label();
        fx.emit(format!("br i1 {more}, label %{go}, label %{done}"));
        fx.start(&go);
        let a1 = fx.r();
        fx.emit(format!("{a1} = add i64 {a}, 1"));
        fx.emit(format!("store i64 {a1}, ptr {ap}"));
        let p = fx.load("ptr", &pp);
        let (elem, by_ref) = match item {
            Ty::Ref(_, inner) => ((**inner).clone(), true),
            other => (other.clone(), false),
        };
        let et = fx.ll(&elem);
        let ep = fx.gep(&et, &p, &[format!("i64 {a}")]);
        let v = if by_ref { ep } else { fx.load(&et, &ep) };
        let s = some(fx, opt, &v, item);
        fx.emit(format!("store {lt} {s}, ptr {out}"));
    }
    fx.emit(format!("br label %{done}"));
    // Characters of a text.
    fx.start(&k2);
    if *item == Ty::Char {
        let p = fx.load("ptr", &pp);
        let n = fx.load("i64", &np);
        let s1 = fx.r();
        fx.emit(format!("{s1} = insertvalue %str undef, ptr {p}, 0"));
        let s2 = fx.r();
        fx.emit(format!("{s2} = insertvalue %str {s1}, i64 {n}, 1"));
        let s3 = fx.r();
        fx.emit(format!("{s3} = insertvalue %str {s2}, i64 0, 2"));
        let sp = fx.spill("%str", &s3);
        let cp = fx.alloca("i32");
        let got = fx.r();
        fx.emit(format!("{got} = call i32 @ante_str_next_char(ptr {sp}, ptr {ap}, ptr {cp})"));
        let has = fx.r();
        fx.emit(format!("{has} = icmp ne i32 {got}, 0"));
        let go = fx.label();
        fx.emit(format!("br i1 {has}, label %{go}, label %{done}"));
        fx.start(&go);
        let c = fx.load("i32", &cp);
        let s = some(fx, opt, &c, item);
        fx.emit(format!("store {lt} {s}, ptr {out}"));
    }
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    fx.load(&lt, &out)
}

/// `undochu[item] next(ptr %iter)`: one function per item type.
pub(super) fn next_fn(gen: &mut Gen, name: &str, item: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let opt = Ty::App(Con::Undochu, vec![item.clone()]);
    let lt = fx.ll(&opt);
    let r = iter_next(&mut fx, "%it", item, &opt);
    format!("define internal {lt} {name}(ptr %it) {{\nentry:\n{}  ret {lt} {r}\n}}\n", fx.code())
}

fn call_next(fx: &mut Fx, it: &str, item: &Ty) -> String {
    let f = fx.gen.item(Item::Next(item.clone()));
    let opt = Ty::App(Con::Undochu, vec![item.clone()]);
    let lt = fx.ll(&opt);
    let r = fx.r();
    fx.emit(format!("{r} = call {lt} {f}(ptr {it})"));
    r
}

/// Calls `next` on the iterator at `it`; branches to `none` at the end,
/// otherwise continues with the item value.
fn next_or(fx: &mut Fx, it: &str, item: &Ty, none: &str) -> String {
    let opt = Ty::App(Con::Undochu, vec![item.clone()]);
    let lt = fx.ll(&opt);
    let r = call_next(fx, it, item);
    let slot = fx.spill(&lt, &r);
    let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
    let tag = fx.load("i32", &tp);
    let has = fx.r();
    fx.emit(format!("{has} = icmp eq i32 {tag}, 0"));
    let go = fx.label();
    fx.emit(format!("br i1 {has}, label %{go}, label %{none}"));
    fx.start(&go);
    let pay = fx.gep(&lt, &slot, &["0".into(), "1".into()]);
    let st = fx.ll(&Ty::Tuple(vec![item.clone()]));
    let p = fx.gep(&st, &pay, &["0".into(), "0".into()]);
    let il = fx.ll(item);
    fx.load(&il, &p)
}

/// The `tharvatha` body of a user iterator type (or a reference to one)
/// yielding `item`.
fn user_iterator(fx: &mut Fx, ty: &Ty, item: &Ty) -> Option<(crate::mir::BodyId, Vec<Ty>)> {
    let target = match ty {
        Ty::Ref(_, inner) => (**inner).clone(),
        other => other.clone(),
    };
    let def = fx.gen.g.std_trait("Okkokkati")?;
    let fid = fx.gen.g.traits.get(&def)?.methods.first()?.1;
    match fx.gen.resolve(fid, &[target, item.clone()])? {
        crate::mir::Resolved::Body(b, tys) => Some((b, tys)),
        crate::mir::Resolved::Native(_) => None,
    }
}

/// The state struct of an adapter: `{ ptr next, ptr drop, … }`.
fn adapter_state(a: Adapter) -> &'static str {
    match a {
        Adapter::Take | Adapter::Skip | Adapter::Enumerate => "{ ptr, ptr, %iter, i64 }",
        Adapter::Map | Adapter::Filter => "{ ptr, ptr, %iter, %pani }",
        Adapter::Zip => "{ ptr, ptr, %iter, %iter }",
        Adapter::User => "{ ptr, ptr, ptr }",
    }
}

/// Wraps an adapter state in an `%iter` of kind 3.
fn make_adapter(fx: &mut Fx, a: Adapter, tys: Vec<Ty>, fields: &[(String, String)]) -> String {
    let st = adapter_state(a);
    let f = fx.gen.item(Item::Adapter(a, tys.clone()));
    let d = fx.gen.item(Item::AdapterDrop(a, tys));
    let state = fx.r();
    fx.emit(format!("{state} = call ptr @ante_alloc(i64 96)"));
    let p0 = fx.gep(st, &state, &["0".into(), "0".into()]);
    fx.emit(format!("store ptr {f}, ptr {p0}"));
    let p1 = fx.gep(st, &state, &["0".into(), "1".into()]);
    fx.emit(format!("store ptr {d}, ptr {p1}"));
    for (i, (ty, v)) in fields.iter().enumerate() {
        let p = fx.gep(st, &state, &["0".into(), (i + 2).to_string()]);
        fx.emit(format!("store {ty} {v}, ptr {p}"));
    }
    make_iter(fx, 3, "0", "0", "0", &state, "0")
}

/// An adapter's `i1 next(ptr state, ptr out)`.
pub(super) fn adapter_fn(gen: &mut Gen, name: &str, a: Adapter, tys: &[Ty]) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let st = adapter_state(a);
    let none = fx.label();
    let inner = fx.gep(st, "%s", &["0".into(), "2".into()]);
    let ty = |i: usize| tys.get(i).cloned().unwrap_or(Ty::Unit);
    match a {
        Adapter::Take => {
            let np = fx.gep(st, "%s", &["0".into(), "3".into()]);
            let n = fx.load("i64", &np);
            let zero = fx.r();
            fx.emit(format!("{zero} = icmp eq i64 {n}, 0"));
            let go = fx.label();
            fx.emit(format!("br i1 {zero}, label %{none}, label %{go}"));
            fx.start(&go);
            let m = fx.r();
            fx.emit(format!("{m} = sub i64 {n}, 1"));
            fx.emit(format!("store i64 {m}, ptr {np}"));
            let x = next_or(&mut fx, &inner, &ty(0), &none);
            let il = fx.ll(&ty(0));
            fx.emit(format!("store {il} {x}, ptr %out"));
        }
        Adapter::Skip => {
            let np = fx.gep(st, "%s", &["0".into(), "3".into()]);
            let head = fx.label();
            let skip = fx.label();
            let take = fx.label();
            fx.emit(format!("br label %{head}"));
            fx.start(&head);
            let n = fx.load("i64", &np);
            let more = fx.r();
            fx.emit(format!("{more} = icmp ne i64 {n}, 0"));
            fx.emit(format!("br i1 {more}, label %{skip}, label %{take}"));
            fx.start(&skip);
            let m = fx.r();
            fx.emit(format!("{m} = sub i64 {n}, 1"));
            fx.emit(format!("store i64 {m}, ptr {np}"));
            // A skipped item is dropped.
            let skipped = next_or(&mut fx, &inner, &ty(0), &none);
            drop_ssa(&mut fx, &ty(0), &skipped);
            fx.emit(format!("br label %{head}"));
            fx.start(&take);
            let x = next_or(&mut fx, &inner, &ty(0), &none);
            let il = fx.ll(&ty(0));
            fx.emit(format!("store {il} {x}, ptr %out"));
        }
        Adapter::Map | Adapter::Filter => {
            let fp = fx.gep(st, "%s", &["0".into(), "3".into()]);
            let f = fx.load("%pani", &fp);
            let code = fx.r();
            fx.emit(format!("{code} = extractvalue %pani {f}, 0"));
            let env = fx.r();
            fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
            let head = fx.label();
            fx.emit(format!("br label %{head}"));
            fx.start(&head);
            let x = next_or(&mut fx, &inner, &ty(0), &none);
            let il = fx.ll(&ty(0));
            if a == Adapter::Map {
                let ol = fx.ll(&ty(1));
                let y = fx.r();
                fx.emit(format!("{y} = call {ol} {code}(ptr {env}, {il} {x})"));
                fx.emit(format!("store {ol} {y}, ptr %out"));
            } else {
                let slot = fx.spill(&il, &x);
                let keep = fx.r();
                fx.emit(format!("{keep} = call i1 {code}(ptr {env}, ptr {slot})"));
                let yes = fx.label();
                let no = fx.label();
                fx.emit(format!("br i1 {keep}, label %{yes}, label %{no}"));
                // A rejected item is dropped.
                fx.start(&no);
                super::glue::call_drop(&mut fx, &ty(0), &slot);
                fx.emit(format!("br label %{head}"));
                fx.start(&yes);
                fx.emit(format!("store {il} {x}, ptr %out"));
            }
        }
        Adapter::Zip => {
            let second = fx.gep(st, "%s", &["0".into(), "3".into()]);
            let x = next_or(&mut fx, &inner, &ty(0), &none);
            // When the second side ends first, the first side's item is dropped.
            let lone = fx.label();
            let y = next_or(&mut fx, &second, &ty(1), &lone);
            let paired = fx.label();
            fx.emit(format!("br label %{paired}"));
            fx.start(&lone);
            drop_ssa(&mut fx, &ty(0), &x);
            fx.emit(format!("br label %{none}"));
            fx.start(&paired);
            let pair = Ty::Tuple(vec![ty(0), ty(1)]);
            let pl = fx.ll(&pair);
            let (xl, yl) = (fx.ll(&ty(0)), fx.ll(&ty(1)));
            let a1 = fx.r();
            fx.emit(format!("{a1} = insertvalue {pl} undef, {xl} {x}, 0"));
            let a2 = fx.r();
            fx.emit(format!("{a2} = insertvalue {pl} {a1}, {yl} {y}, 1"));
            fx.emit(format!("store {pl} {a2}, ptr %out"));
        }
        Adapter::User => {
            let target = fx.load("ptr", &inner);
            let Some((body, btys)) = user_iterator(&mut fx, &ty(0), &ty(1)) else {
                fx.gen.error("native backend: `Okkokkati` implementation dorakaledhu");
                return String::new();
            };
            let (r, opt) = fx.call_body_vals(body, &btys, &[target]);
            let lt = fx.ll(&opt);
            let slot = fx.spill(&lt, &r);
            let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
            let tag = fx.load("i32", &tp);
            let has = fx.r();
            fx.emit(format!("{has} = icmp eq i32 {tag}, 0"));
            let go = fx.label();
            fx.emit(format!("br i1 {has}, label %{go}, label %{none}"));
            fx.start(&go);
            let pay = fx.gep(&lt, &slot, &["0".into(), "1".into()]);
            let stt = fx.ll(&Ty::Tuple(vec![ty(1)]));
            let p = fx.gep(&stt, &pay, &["0".into(), "0".into()]);
            let il = fx.ll(&ty(1));
            let x = fx.load(&il, &p);
            fx.emit(format!("store {il} {x}, ptr %out"));
        }
        Adapter::Enumerate => {
            let ip = fx.gep(st, "%s", &["0".into(), "3".into()]);
            let x = next_or(&mut fx, &inner, &ty(0), &none);
            let i = fx.load("i64", &ip);
            let j = fx.r();
            fx.emit(format!("{j} = add i64 {i}, 1"));
            fx.emit(format!("store i64 {j}, ptr {ip}"));
            let pair = Ty::Tuple(vec![Ty::ANKAM, ty(0)]);
            let pl = fx.ll(&pair);
            let xl = fx.ll(&ty(0));
            let a1 = fx.r();
            fx.emit(format!("{a1} = insertvalue {pl} undef, i64 {i}, 0"));
            let a2 = fx.r();
            fx.emit(format!("{a2} = insertvalue {pl} {a1}, {xl} {x}, 1"));
            fx.emit(format!("store {pl} {a2}, ptr %out"));
        }
    }
    fx.emit("ret i1 true");
    fx.start(&none);
    fx.emit("ret i1 false");
    format!("define internal i1 {name}(ptr %s, ptr %out) {{\nentry:\n{}}}\n", fx.code())
}

/// An adapter's `void drop(ptr state)`: drops what the state owns (inner
/// iterators, the `pani`, an owned user iterator), then frees it.
pub(super) fn adapter_drop(gen: &mut Gen, name: &str, a: Adapter, tys: &[Ty]) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let st = adapter_state(a);
    let ty = |i: usize| tys.get(i).cloned().unwrap_or(Ty::Unit);
    let iter = |t: Ty| Ty::App(Con::Pravaham, vec![t]);
    let field = |fx: &mut Fx, i: u32| fx.gep(st, "%s", &["0".into(), i.to_string()]);
    match a {
        Adapter::Take
        | Adapter::Skip
        | Adapter::Enumerate
        | Adapter::Map
        | Adapter::Filter
        | Adapter::Zip => {
            let p = field(&mut fx, 2);
            super::glue::call_drop(&mut fx, &iter(ty(0)), &p);
            match a {
                Adapter::Map | Adapter::Filter => {
                    let fp = field(&mut fx, 3);
                    let f = fx.load("%pani", &fp);
                    pani_dec(&mut fx, &f);
                }
                Adapter::Zip => {
                    let q = field(&mut fx, 3);
                    super::glue::call_drop(&mut fx, &iter(ty(1)), &q);
                }
                _ => {}
            }
        }
        Adapter::User => {
            // `tys[0]` is the iterable's type: owned iterators were moved to the heap.
            if !matches!(ty(0), Ty::Ref(..)) {
                let pp = field(&mut fx, 2);
                let p = fx.load("ptr", &pp);
                super::glue::call_drop(&mut fx, &ty(0), &p);
                fx.emit(format!("call void @free(ptr {p})"));
            }
        }
    }
    fx.emit("call void @free(ptr %s)");
    format!("define internal void {name}(ptr %s) {{\nentry:\n{}  ret void\n}}\n", fx.code())
}

/// Releases one handle to a closure (§10.7.4).
fn pani_dec(fx: &mut Fx, f: &str) {
    let env = fx.r();
    fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
    fx.emit(format!("call void @ante_pani_dec(ptr {env})"));
}

/// Drops a value held in a register.
fn drop_ssa(fx: &mut Fx, ty: &Ty, v: &str) {
    if fx.needs_drop(ty) {
        let lt = fx.ll(ty);
        let p = fx.spill(&lt, v);
        super::glue::call_drop(fx, ty, &p);
    }
}

// ── built-in trait methods ─────────────────────────────────────────────────

pub(super) fn trait_call(
    fx: &mut Fx,
    name: &str,
    tys: &[Ty],
    args: &[Operand],
    dest: &Ty,
    span: Span,
) -> (String, Ty) {
    let ty = tys.first().cloned().unwrap_or(Ty::Error);
    let r = Ty::reference(false, ty.clone());
    match name {
        "XeroxTheeyochu.xerox" => {
            let p = fx.operand(&args[0], &r);
            (call_xerox(fx, &ty, &p), ty)
        }
        "Polchochu.okateyna" => {
            let a = fx.operand(&args[0], &r);
            let b = fx.operand(&args[1], &r);
            (eq_call(fx, &ty, &a, &b), Ty::Bool)
        }
        "VarusaPettochu.polchu" => {
            let a = fx.operand(&args[0], &r);
            let b = fx.operand(&args[1], &r);
            let c = call_cmp(fx, &ty, &a, &b);
            let tag = fx.r();
            fx.emit(format!("{tag} = add i32 {c}, 1"));
            let kt = dest.clone();
            let lt = fx.ll(&kt);
            let slot = fx.alloca(&lt);
            let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
            fx.emit(format!("store i32 {tag}, ptr {tp}"));
            (fx.load(&lt, &slot), kt)
        }
        "Choopinchochu.padham_ga" => {
            let p = fx.operand(&args[0], &r);
            let buf = new_buf(fx);
            let f = fx.gen.item(Item::Show(ty));
            fx.emit(format!("call void {f}(ptr {buf}, ptr {p})"));
            (fx.load("%str", &buf), Ty::Str)
        }
        "Sardhukochu.sardhu" => ("zeroinitializer".into(), Ty::Unit),
        "HashCheyochu.hash_cheyyi" => {
            let p = fx.operand(&args[0], &r);
            let hasher = fx.operand_ty(&args[1]).unwrap_or(Ty::Unit);
            let hp = fx.operand(&args[1], &hasher);
            super::map::hasher_add(fx, &hp, &ty, &p, true);
            ("zeroinitializer".into(), Ty::Unit)
        }
        "Kalupochu.kalupu"
        | "Theeseyochu.theesey"
        | "Guninchochu.guninchu"
        | "Panchochu.panchu"
        | "Migulochu.migulu" => {
            let pa = fx.operand(&args[0], &r);
            let pb = fx.operand(&args[1], &r);
            let lt = fx.ll(&ty);
            let x = fx.load(&lt, &pa);
            let y = fx.load(&lt, &pb);
            let op = match name {
                "Kalupochu.kalupu" => crate::ast::BinOp::Add,
                "Theeseyochu.theesey" => crate::ast::BinOp::Sub,
                "Guninchochu.guninchu" => crate::ast::BinOp::Mul,
                "Panchochu.panchu" => crate::ast::BinOp::Div,
                _ => crate::ast::BinOp::Rem,
            };
            match &ty {
                Ty::Int(it) => (fx.int_binary(op, *it, &x, &y, span), ty),
                Ty::Float(b) => {
                    let ft = if *b == 32 { "float" } else { "double" };
                    let ins = match op {
                        crate::ast::BinOp::Add => "fadd",
                        crate::ast::BinOp::Sub => "fsub",
                        crate::ast::BinOp::Mul => "fmul",
                        crate::ast::BinOp::Div => "fdiv",
                        _ => "frem",
                    };
                    let v = fx.r();
                    fx.emit(format!("{v} = {ins} {ft} {x}, {y}"));
                    (v, ty)
                }
                Ty::Str => {
                    let out = fx.alloca("%str");
                    fx.emit(format!("call void @ante_str_concat(ptr {out}, ptr {pa}, ptr {pb})"));
                    (fx.load("%str", &out), Ty::Str)
                }
                _ => unsupported(fx, name, dest),
            }
        }
        _ => unsupported(fx, name, dest),
    }
}

// ── standard functions (§21) ───────────────────────────────────────────────

#[allow(clippy::cognitive_complexity)]
pub(super) fn std_call(
    fx: &mut Fx,
    fid: FnId,
    tys: &[Ty],
    args: &[Operand],
    dest: &Ty,
    span: Span,
) -> (String, Ty) {
    let key = std_key(fx, fid);
    let (ftys, ret) = formals(fx, fid, tys);
    let mut vals = Vec::new();
    for (o, t) in args.iter().zip(&ftys) {
        vals.push(fx.operand(o, t));
    }
    let v = |i: usize| vals.get(i).cloned().unwrap_or_else(|| "undef".into());
    let t = |i: usize| ftys.get(i).cloned().unwrap_or(Ty::Unit);
    let tyarg = |i: usize| tys.get(i).cloned().unwrap_or(Ty::Unit);
    let unit = || ("zeroinitializer".to_string(), Ty::Unit);
    if let Some(r) = super::task::call(fx, &key, &vals, &ftys, tys, &ret, span) {
        return r;
    }
    match key.as_str() {
        // ── prelude ──
        "prelude.choopinchu" | "prelude.cheppu" | "prelude.gola_pettu" => {
            let buf = new_buf(fx);
            show_ref(fx, &buf, &v(0), &tyarg(0));
            let fd = if key == "prelude.gola_pettu" { 2 } else { 1 };
            let nl = i32::from(key != "prelude.cheppu");
            fx.emit(format!("call void @ante_write(i32 {fd}, ptr {buf}, i32 {nl})"));
            fx.emit(format!("call void @ante_buf_free(ptr {buf})"));
            unit()
        }
        "prelude.nijame" => {
            let bad = fx.r();
            fx.emit(format!("{bad} = xor i1 {}, true", v(0)));
            let msg = v(1);
            fx.guard(&bad, |fx| {
                let n = fx.r();
                fx.emit(format!("{n} = extractvalue %str {msg}, 1"));
                let empty = fx.r();
                fx.emit(format!("{empty} = icmp eq i64 {n}, 0"));
                let plain = fx.label();
                let with = fx.label();
                fx.emit(format!("br i1 {empty}, label %{plain}, label %{with}"));
                fx.start(&plain);
                let loc = fx.loc(span);
                fx.emit(format!("call void @ante_trap_code(i32 7, ptr {loc})"));
                fx.emit("unreachable");
                fx.start(&with);
                let buf = new_buf(fx);
                push_lit(fx, &buf, "nijame() fail ayyindhi: ");
                let mp = fx.spill("%str", &msg);
                let d = fx.gep("%str", &mp, &["0".into(), "0".into()]);
                let dv = fx.load("ptr", &d);
                fx.emit(format!("call void @ante_buf_push(ptr {buf}, ptr {dv}, i64 {n})"));
                let s = fx.load("%str", &buf);
                trap_text(fx, &s, span);
                // `guard` closes the block with `unreachable`; open a dead one for it.
                let dead = fx.label();
                fx.start(&dead);
            });
            unit()
        }
        "prelude.kompa_munigindhi" => {
            let msg = v(0);
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %str {msg}, 1"));
            let empty = fx.r();
            fx.emit(format!("{empty} = icmp eq i64 {n}, 0"));
            let default = fx.gen.str_const("kompa_munigindhi()");
            let m = fx.r();
            fx.emit(format!("{m} = select i1 {empty}, %str {default}, %str {msg}"));
            trap_text(fx, &m, span);
            let dead = fx.label();
            fx.start(&dead);
            ("undef".into(), Ty::Never)
        }
        "prelude.maarchu" | "prelude.kudhirithe_maarchu" | "prelude.kolpothu_maarchu" => {
            convert(fx, &key, &v(0), &tyarg(1), &tyarg(0), dest, span)
        }
        "prelude.modhati" | "prelude.dhaatu" => {
            let a = if key == "prelude.modhati" { Adapter::Take } else { Adapter::Skip };
            let n = v(1);
            let neg = fx.r();
            fx.emit(format!("{neg} = icmp slt i64 {n}, 0"));
            let n0 = fx.r();
            fx.emit(format!("{n0} = select i1 {neg}, i64 0, i64 {n}"));
            let it = make_adapter(fx, a, vec![tyarg(0)], &[("%iter".into(), v(0)), ("i64".into(), n0)]);
            (it, ret)
        }
        "prelude.prathi_dhaaniki" => {
            let it = make_adapter(
                fx,
                Adapter::Map,
                vec![tyarg(0), tyarg(1)],
                &[("%iter".into(), v(0)), ("%pani".into(), v(1))],
            );
            (it, ret)
        }
        "prelude.jalleda_pattu" => {
            let it = make_adapter(
                fx,
                Adapter::Filter,
                vec![tyarg(0)],
                &[("%iter".into(), v(0)), ("%pani".into(), v(1))],
            );
            (it, ret)
        }
        "prelude.kalipi" => {
            let it = make_adapter(
                fx,
                Adapter::Zip,
                vec![tyarg(0), tyarg(1)],
                &[("%iter".into(), v(0)), ("%iter".into(), v(1))],
            );
            (it, ret)
        }
        "prelude.number_vesi" => {
            let it = make_adapter(
                fx,
                Adapter::Enumerate,
                vec![tyarg(0)],
                &[("%iter".into(), v(0)), ("i64".into(), "0".into())],
            );
            (it, ret)
        }
        "prelude.varusa_ga" | "prelude.poguchey" => {
            // Drains the iterator: into a vector, or folding with the `pani`.
            let item = tyarg(0);
            let it = fx.spill("%iter", &v(0));
            let fold = key == "prelude.poguchey";
            let acc_ty = if fold { tyarg(1) } else { Ty::App(Con::Varusa, vec![item.clone()]) };
            let al = fx.ll(&acc_ty);
            let acc = fx.alloca(&al);
            let init = if fold { v(1) } else { "zeroinitializer".into() };
            fx.emit(format!("store {al} {init}, ptr {acc}"));
            let head = fx.label();
            let done = fx.label();
            fx.emit(format!("br label %{head}"));
            fx.start(&head);
            let x = next_or(fx, &it, &item, &done);
            let il = fx.ll(&item);
            if fold {
                let f = v(2);
                let code = fx.r();
                fx.emit(format!("{code} = extractvalue %pani {f}, 0"));
                let env = fx.r();
                fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
                let cur = fx.load(&al, &acc);
                let nv = fx.r();
                fx.emit(format!("{nv} = call {al} {code}(ptr {env}, {al} {cur}, {il} {x})"));
                fx.emit(format!("store {al} {nv}, ptr {acc}"));
            } else {
                let (size, _) = fx.gen.size_align(&item);
                let slot = fx.spill(&il, &x);
                fx.emit(format!("call void @ante_vec_push(ptr {acc}, ptr {slot}, i64 {size})"));
            }
            fx.emit(format!("br label %{head}"));
            fx.start(&done);
            super::glue::call_drop(fx, &Ty::App(Con::Pravaham, vec![item]), &it);
            if fold {
                pani_dec(fx, &v(2));
            }
            (fx.load(&al, &acc), acc_ty)
        }
        "prelude.size_of" | "prelude.align_of" => {
            let (s, a) = fx.gen.size_align(&tyarg(0));
            ((if key == "prelude.size_of" { s } else { a }).to_string(), Ty::ANKAM)
        }

        // ── padham ──
        "padham.enni" => {
            let r = fx.r();
            fx.emit(format!("{r} = call i64 @ante_str_chars(ptr {})", v(0)));
            (r, Ty::ANKAM)
        }
        "padham.byte_len" => {
            let p = fx.gep("%str", &v(0), &["0".into(), "1".into()]);
            (fx.load("i64", &p), Ty::ANKAM)
        }
        "padham.aksharaalu" => {
            let dp = fx.gep("%str", &v(0), &["0".into(), "0".into()]);
            let d = fx.load("ptr", &dp);
            let lp = fx.gep("%str", &v(0), &["0".into(), "1".into()]);
            let n = fx.load("i64", &lp);
            (make_iter(fx, 2, "0", "0", "0", &d, &n), ret)
        }
        "padham.vidagottu" => {
            let out = fx.alloca("%vec");
            fx.emit(format!("call void @ante_str_split(ptr {out}, ptr {}, ptr {})", v(0), v(1)));
            (fx.load("%vec", &out), ret)
        }
        "padham.baitlu" => {
            let dp = fx.gep("%str", &v(0), &["0".into(), "0".into()]);
            let d = fx.load("ptr", &dp);
            let lp = fx.gep("%str", &v(0), &["0".into(), "1".into()]);
            let n = fx.load("i64", &lp);
            (fx.slice_value(&d, &n), ret)
        }
        "padham.baitlu_nunchi" => {
            let d = fx.r();
            fx.emit(format!("{d} = extractvalue %slice {}, 0", v(0)));
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %slice {}, 1", v(0)));
            (c_result_out(fx, &ret, "ante_str_from_bytes", &format!("ptr {d}, i64 {n}"), "%str"), ret)
        }
        "padham.byte_mukka" => {
            let args = format!("ptr {}, i64 {}, i64 {}", v(0), v(1), v(2));
            (c_result_out(fx, &ret, "ante_str_byte_part", &args, "%str"), ret)
        }
        "padham.kathirinchu" => {
            let out = fx.alloca("%str");
            fx.emit(format!("call void @ante_str_trim(ptr {out}, ptr {})", v(0)));
            (fx.load("%str", &out), Ty::Str)
        }
        "padham.tho_modhalaindha" | "padham.lo_undha" => {
            let f = if key == "padham.tho_modhalaindha" { "ante_str_starts" } else { "ante_str_contains" };
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @{f}(ptr {}, ptr {})", v(0), v(1)));
            let b = fx.r();
            fx.emit(format!("{b} = icmp ne i32 {r}, 0"));
            (b, Ty::Bool)
        }
        "padham.ankam_ga" | "padham.aasajeevi_ga" | "padham.amsam_ga" => {
            let float = key == "padham.amsam_ga";
            let (val_ty, store_ty) = if float { (Ty::Float(64), "double") } else { (opt_inner(&ret), "i64") };
            let out = fx.alloca(store_ty);
            let ok = fx.r();
            if float {
                fx.emit(format!("{ok} = call i32 @ante_str_parse_float(ptr {}, ptr {out})", v(0)));
            } else {
                let base = fx.r();
                fx.emit(format!("{base} = add i32 {}, 0", v(1)));
                let unsigned = i32::from(key == "padham.aasajeevi_ga");
                fx.emit(format!(
                    "{ok} = call i32 @ante_str_parse_int(ptr {}, i32 {base}, i32 {unsigned}, ptr {out})",
                    v(0)
                ));
            }
            let good = fx.r();
            fx.emit(format!("{good} = icmp ne i32 {ok}, 0"));
            let lt = fx.ll(&ret);
            let slot = fx.alloca(&lt);
            let yes = fx.label();
            let no = fx.label();
            let done = fx.label();
            fx.emit(format!("br i1 {good}, label %{yes}, label %{no}"));
            fx.start(&yes);
            let x = fx.load(store_ty, &out);
            let e = fx.build_enum(&ret, 0, &[x], std::slice::from_ref(&val_ty));
            fx.emit(format!("store {lt} {e}, ptr {slot}"));
            fx.emit(format!("br label %{done}"));
            fx.start(&no);
            let err_ty = match &ret {
                Ty::App(Con::SareThappu, a) => a[1].clone(),
                _ => Ty::Unit,
            };
            let buf = new_buf(fx);
            push_lit(fx, &buf, "`");
            let sv = fx.load("%str", &v(0));
            show_value(fx, &buf, &sv, &Ty::Str);
            let what = if float {
                "amsam".to_string()
            } else {
                crate::interp::int_name(match val_ty {
                    Ty::Int(it) => it,
                    _ => IntTy::ANKAM,
                })
            };
            push_lit(fx, &buf, &format!("` {what} kaadhu"));
            let msg = fx.load("%str", &buf);
            let et = fx.ll(&err_ty);
            let rec = fx.r();
            fx.emit(format!("{rec} = insertvalue {et} undef, %str {msg}, 0"));
            let e = fx.build_enum(&ret, 1, &[rec], std::slice::from_ref(&err_ty));
            fx.emit(format!("store {lt} {e}, ptr {slot}"));
            fx.emit(format!("br label %{done}"));
            fx.start(&done);
            (fx.load(&lt, &slot), ret)
        }

        // ── varusa and mukka ──
        "varusa.kotthadhi" => ("zeroinitializer".into(), ret),
        // ── raw pointers (§22.2) ──
        "mukka.pointer" => {
            let Some((d, _, _)) = seq_val(fx, &v(0), &t(0)) else { return unsupported(fx, &key, dest) };
            (d, ret)
        }
        "pointer.jarugu" => {
            // Steps by whole elements; `*khali` steps by bytes, like C compilers do.
            let elem = match &t(0) {
                Ty::Ptr(_, e) => (**e).clone(),
                _ => Ty::Unit,
            };
            let size = fx.gen.size_align(&elem).0.max(1);
            let off = fx.r();
            fx.emit(format!("{off} = mul i64 {}, {size}", v(1)));
            let r = fx.r();
            fx.emit(format!("{r} = getelementptr i8, ptr {}, i64 {off}", v(0)));
            (r, ret)
        }
        "varusa.enni" | "mukka.enni" | "varusa.khaaliyaa" | "mukka.khaaliyaa" => {
            let Some((_, n, _)) = seq_val(fx, &v(0), &t(0)) else { return unsupported(fx, &key, dest) };
            if key.ends_with("enni") {
                (n, Ty::ANKAM)
            } else {
                let b = fx.r();
                fx.emit(format!("{b} = icmp eq i64 {n}, 0"));
                (b, Ty::Bool)
            }
        }
        "varusa.cherchu" => {
            let elem = match &t(0) {
                Ty::Ref(_, a) => match &**a {
                    Ty::App(Con::Varusa, e) => e[0].clone(),
                    _ => Ty::Unit,
                },
                _ => Ty::Unit,
            };
            let (size, _) = fx.gen.size_align(&elem);
            let et = fx.ll(&elem);
            let p = fx.spill(&et, &v(1));
            fx.emit(format!("call void @ante_vec_push(ptr {}, ptr {p}, i64 {size})", v(0)));
            unit()
        }
        "varusa.theesey" => {
            let elem = opt_inner(&ret);
            let lp = fx.gep("%vec", &v(0), &["0".into(), "1".into()]);
            let n = fx.load("i64", &lp);
            let has = fx.r();
            fx.emit(format!("{has} = icmp ne i64 {n}, 0"));
            let vec = v(0);
            let r = opt_select(fx, &ret, &has, &mut |fx| {
                let m = fx.r();
                fx.emit(format!("{m} = sub i64 {n}, 1"));
                fx.emit(format!("store i64 {m}, ptr {lp}"));
                let dp = fx.gep("%vec", &vec, &["0".into(), "0".into()]);
                let d = fx.load("ptr", &dp);
                let et = fx.ll(&elem);
                let ep = fx.gep(&et, &d, &[format!("i64 {m}")]);
                fx.load(&et, &ep)
            });
            (r, ret)
        }
        "varusa.thechu" | "varusa.maare_thechu" | "mukka.thechu" => {
            let Some((d, n, elem)) = seq_val(fx, &v(0), &t(0)) else { return unsupported(fx, &key, dest) };
            let idx = v(1);
            let ok = fx.r();
            fx.emit(format!("{ok} = icmp ult i64 {idx}, {n}"));
            let r = opt_select(fx, &ret, &ok, &mut |fx| {
                let et = fx.ll(&elem);
                fx.gep(&et, &d, &[format!("i64 {idx}")])
            });
            (r, ret)
        }
        "varusa.sort_cheyyi" | "varusa.ila_sort_cheyyi" => {
            let Some((d, n, elem)) = seq_val(fx, &v(0), &t(0)) else { return unsupported(fx, &key, dest) };
            let (size, _) = fx.gen.size_align(&elem);
            if key == "varusa.sort_cheyyi" {
                let cb = fx.gen.item(Item::SortCmp(elem));
                fx.emit(format!("call void @ante_sort(ptr {d}, i64 {n}, i64 {size}, ptr {cb}, ptr null)"));
            } else {
                let cb = fx.gen.item(Item::SortBy(elem));
                let ctx = fx.spill("%pani", &v(1));
                fx.emit(format!("call void @ante_sort(ptr {d}, i64 {n}, i64 {size}, ptr {cb}, ptr {ctx})"));
                pani_dec(fx, &v(1));
            }
            unit()
        }
        "varusa.anni_theesuko" | "prelude.anni_theesuko" => {
            let Some((d, n, _)) = seq_val(fx, &v(0), &t(0)) else { return unsupported(fx, &key, dest) };
            let owned = if matches!(t(0), Ty::Ref(..)) { "0" } else { "1" };
            (make_iter(fx, 1, owned, "0", "0", &d, &n), ret)
        }

        // ── jodi, gajibiji_jodi, gumpu ──
        k if k.starts_with("jodi.") || k.starts_with("gajibiji_jodi.") || k.starts_with("gumpu.") => {
            let method = k.split_once('.').map_or("", |(_, m)| m);
            match super::map::call(fx, method, &vals, &ftys, &ret) {
                Some(r) => r,
                None => unsupported(fx, &key, dest),
            }
        }

        // ── undochu, sare_thappu, pette ──
        "undochu.undhaa" | "undochu.ledhaa" | "sare_thappu.sareyaa" => {
            let inner = match &t(0) {
                Ty::Ref(_, a) => (**a).clone(),
                other => other.clone(),
            };
            let lt = fx.ll(&inner);
            let tp = fx.gep(&lt, &v(0), &["0".into(), "0".into()]);
            let tag = fx.load("i32", &tp);
            let want = if key == "undochu.ledhaa" { 1 } else { 0 };
            let b = fx.r();
            fx.emit(format!("{b} = icmp eq i32 {tag}, {want}"));
            (b, Ty::Bool)
        }
        "undochu.lekapothe" | "undochu.kachithanga" | "sare_thappu.kachithanga" => {
            let ot = t(0);
            let lt = fx.ll(&ot);
            let slot = fx.spill(&lt, &v(0));
            let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
            let tag = fx.load("i32", &tp);
            let inner = opt_inner(&ot);
            let it = fx.ll(&inner);
            let pay = fx.gep(&lt, &slot, &["0".into(), "1".into()]);
            let st = fx.ll(&Ty::Tuple(vec![inner.clone()]));
            let fp = fx.gep(&st, &pay, &["0".into(), "0".into()]);
            let is_some = fx.r();
            fx.emit(format!("{is_some} = icmp eq i32 {tag}, 0"));
            if key == "undochu.lekapothe" {
                let x = fx.load(&it, &fp);
                let d = v(1);
                if fx.needs_drop(&inner) {
                    // The unused default is dropped.
                    let ds = fx.spill(&it, &d);
                    let inner2 = inner.clone();
                    fx.when(&is_some, |fx| super::glue::call_drop(fx, &inner2, &ds));
                }
                let r = fx.r();
                fx.emit(format!("{r} = select i1 {is_some}, {it} {x}, {it} {d}"));
                return (r, inner);
            }
            let bad = fx.r();
            fx.emit(format!("{bad} = xor i1 {is_some}, true"));
            let sare = key.starts_with("sare");
            let err_ty = match &ot {
                Ty::App(Con::SareThappu, a) => a[1].clone(),
                _ => Ty::Unit,
            };
            let (pay2, lt2) = (pay.clone(), lt.clone());
            fx.guard(&bad, |fx| {
                if sare {
                    let buf = new_buf(fx);
                    push_lit(fx, &buf, "`thappu(");
                    let est = fx.ll(&Ty::Tuple(vec![err_ty.clone()]));
                    let ep = fx.gep(&est, &pay2, &["0".into(), "0".into()]);
                    let f = fx.gen.item(Item::Show(err_ty.clone()));
                    fx.emit(format!("call void {f}(ptr {buf}, ptr {ep})"));
                    push_lit(fx, &buf, ")` meeda kachithanga() pilicharu");
                    let s = fx.load("%str", &buf);
                    let loc = fx.loc(span);
                    let sp = fx.spill("%str", &s);
                    fx.emit(format!("call void @ante_trap(ptr {sp}, ptr {loc})"));
                } else {
                    let loc = fx.loc(span);
                    fx.emit(format!("call void @ante_trap_code(i32 6, ptr {loc})"));
                }
                let _ = &lt2;
            });
            (fx.load(&it, &fp), inner)
        }
        "undochu.prathi_dhaaniki" | "sare_thappu.prathi_dhaaniki" | "sare_thappu.thappu_maarchu" => {
            // Applies the `pani` to the payload of variant 0 (or 1 for `thappu_maarchu`).
            let ot = t(0);
            let lt = fx.ll(&ot);
            let slot = fx.spill(&lt, &v(0));
            let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
            let tag = fx.load("i32", &tp);
            let target = u32::from(key == "sare_thappu.thappu_maarchu");
            let variants = fx.gen.variants(&ot);
            let in_ty = variants.get(target as usize).and_then(|f| f.first()).cloned().unwrap_or(Ty::Unit);
            let out_variants = fx.gen.variants(&ret);
            let out_ty =
                out_variants.get(target as usize).and_then(|f| f.first()).cloned().unwrap_or(Ty::Unit);
            let rl = fx.ll(&ret);
            let rslot = fx.alloca(&rl);
            let hit = fx.r();
            fx.emit(format!("{hit} = icmp eq i32 {tag}, {target}"));
            let yes = fx.label();
            let no = fx.label();
            let done = fx.label();
            fx.emit(format!("br i1 {hit}, label %{yes}, label %{no}"));
            fx.start(&yes);
            let pay = fx.gep(&lt, &slot, &["0".into(), "1".into()]);
            let ist = fx.ll(&Ty::Tuple(vec![in_ty.clone()]));
            let ip = fx.gep(&ist, &pay, &["0".into(), "0".into()]);
            let il = fx.ll(&in_ty);
            let x = fx.load(&il, &ip);
            let f = v(1);
            let fp = fx.r();
            fx.emit(format!("{fp} = extractvalue %pani {f}, 0"));
            let env = fx.r();
            fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
            let ol = fx.ll(&out_ty);
            let y = fx.r();
            fx.emit(format!("{y} = call {ol} {fp}(ptr {env}, {il} {x})"));
            let e = fx.build_enum(&ret, target, &[y], std::slice::from_ref(&out_ty));
            fx.emit(format!("store {rl} {e}, ptr {rslot}"));
            fx.emit(format!("br label %{done}"));
            fx.start(&no);
            // The other variant keeps its payload (`ledhu` has none).
            let other = 1 - target;
            let other_fields = out_variants.get(other as usize).cloned().unwrap_or_default();
            let vals: Vec<String> = if let Some(of) = other_fields.first() {
                let pay = fx.gep(&lt, &slot, &["0".into(), "1".into()]);
                let st = fx.ll(&Ty::Tuple(vec![of.clone()]));
                let p = fx.gep(&st, &pay, &["0".into(), "0".into()]);
                let ofl = fx.ll(of);
                vec![fx.load(&ofl, &p)]
            } else {
                Vec::new()
            };
            let e = fx.build_enum(&ret, other, &vals, &other_fields);
            fx.emit(format!("store {rl} {e}, ptr {rslot}"));
            fx.emit(format!("br label %{done}"));
            fx.start(&done);
            pani_dec(fx, &v(1));
            (fx.load(&rl, &rslot), ret)
        }
        "pette.kotthadhi" => {
            let inner = t(0);
            (fx.boxed(&inner, &v(0)), ret)
        }

        // ── scalars ──
        "aksharam.digit_aa" | "aksharam.letter_aa" | "aksharam.space_aa" | "aksharam.alnum_aa"
        | "aksharam.ascii_aa" => {
            let c = v(0);
            let pred: fn(char) -> bool = match key.as_str() {
                "aksharam.digit_aa" => char::is_numeric,
                "aksharam.letter_aa" => char::is_alphabetic,
                "aksharam.space_aa" => char::is_whitespace,
                "aksharam.alnum_aa" => char::is_alphanumeric,
                _ => |c| c.is_ascii(),
            };
            let r = fx.r();
            if key == "aksharam.ascii_aa" {
                fx.emit(format!("{r} = icmp ult i32 {c}, 128"));
            } else {
                let (table, n) = fx.gen.char_table(&key, pred);
                fx.emit(format!("{r} = call i1 @ante_char_in(i32 {c}, ptr {table}, i64 {n})"));
            }
            (r, Ty::Bool)
        }
        "ankamlu.hex_ga" => {
            let Ty::Int(it) = tyarg(0) else { return unsupported(fx, &key, dest) };
            let w = if it.bits == 64 {
                v(0)
            } else {
                let r = fx.r();
                fx.emit(format!("{r} = zext i{} {} to i64", it.bits, v(0)));
                r
            };
            let buf = new_buf(fx);
            fx.emit(format!("call void @ante_buf_hex(ptr {buf}, i64 {w})"));
            (fx.load("%str", &buf), Ty::Str)
        }
        "amsam64.bits" | "amsam32.bits" => {
            let (ft, it) = if key == "amsam64.bits" { ("double", "i64") } else { ("float", "i32") };
            let r = fx.r();
            fx.emit(format!("{r} = bitcast {ft} {} to {it}", v(0)));
            (r, ret)
        }
        "amsam64.bits_nunchi" | "amsam32.bits_nunchi" => {
            let (ft, it) = if key == "amsam64.bits_nunchi" { ("double", "i64") } else { ("float", "i32") };
            let r = fx.r();
            fx.emit(format!("{r} = bitcast {it} {} to {ft}", v(0)));
            (r, ret)
        }

        // ── Hasher ──
        "Hasher.kalupu" => {
            let t1 = tyarg(1);
            let lt = fx.ll(&t1);
            let p = fx.spill(&lt, &v(1));
            super::map::hasher_add(fx, &v(0), &t1, &p, false);
            super::glue::call_drop(fx, &t1, &p);
            unit()
        }
        "Hasher.chaalu" => {
            let sp = fx.gep("{ i64 }", &v(0), &["0".into(), "0".into()]);
            (fx.load("i64", &sp), ret)
        }

        // ── Raatha ──
        "Raatha.kotthadhi" => ("zeroinitializer".into(), ret),
        "Raatha.raayi" | "Raatha.akshara_raayi" => {
            let rt = match &t(0) {
                Ty::Ref(_, a) => (**a).clone(),
                other => other.clone(),
            };
            let rl = fx.ll(&rt);
            let buf = fx.gep(&rl, &v(0), &["0".into(), "0".into()]);
            if key == "Raatha.raayi" {
                show_ref(fx, &buf, &v(1), &tyarg(1));
            } else {
                fx.emit(format!("call void @ante_buf_char(ptr {buf}, i32 {})", v(1)));
            }
            unit()
        }
        "Raatha.enni" | "Raatha.padham_ga" => {
            let rt = match &t(0) {
                Ty::Ref(_, a) => (**a).clone(),
                other => other.clone(),
            };
            let rl = fx.ll(&rt);
            let sp = fx.gep(&rl, &v(0), &["0".into(), "0".into()]);
            if key == "Raatha.enni" {
                let r = fx.r();
                fx.emit(format!("{r} = call i64 @ante_str_chars(ptr {sp})"));
                return (r, Ty::ANKAM);
            }
            let s = fx.load("%str", &sp);
            fx.emit(format!("store %str zeroinitializer, ptr {sp}"));
            (s, Ty::Str)
        }

        // ── lekkalu ──
        k if k.starts_with("lekkalu.") => lekkalu(fx, &k["lekkalu.".len()..], &tyarg(0), &vals, &ret, span),

        // ── file, system, samayam ──
        "file.chadhuvu" => (c_result_out(fx, &ret, "ante_file_read", &format!("ptr {}", v(0)), "%str"), ret),
        "file.baitlu_chadhuvu" => {
            (c_result_out(fx, &ret, "ante_file_read_bytes", &format!("ptr {}", v(0)), "%vec"), ret)
        }
        "file.list_cheyyi" => {
            (c_result_out(fx, &ret, "ante_file_list", &format!("ptr {}", v(0)), "%vec"), ret)
        }
        "file.raayi" => {
            (c_result_unit(fx, &ret, "ante_file_write", &format!("ptr {}, ptr {}", v(0), v(1))), ret)
        }
        "file.baitlu_raayi" => {
            let d = fx.r();
            fx.emit(format!("{d} = extractvalue %slice {}, 0", v(1)));
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %slice {}, 1", v(1)));
            let args = format!("ptr {}, ptr {d}, i64 {n}", v(0));
            (c_result_unit(fx, &ret, "ante_file_write_bytes", &args), ret)
        }
        "file.folder_cheyyi" => (c_result_unit(fx, &ret, "ante_file_mkdirs", &format!("ptr {}", v(0))), ret),
        "file.theesey" => (c_result_unit(fx, &ret, "ante_file_remove", &format!("ptr {}", v(0))), ret),
        "file.folderaa" => {
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @ante_file_is_folder(ptr {})", v(0)));
            let b = fx.r();
            fx.emit(format!("{b} = icmp ne i32 {r}, 0"));
            (b, Ty::Bool)
        }
        "file.path_kalupu" | "file.path_peru" | "file.path_folder" => {
            let out = fx.alloca("%str");
            let (f, args) = match key.as_str() {
                "file.path_kalupu" => ("ante_path_join", format!("ptr {}, ptr {}", v(0), v(1))),
                "file.path_peru" => ("ante_path_name", format!("ptr {}", v(0))),
                _ => ("ante_path_folder", format!("ptr {}", v(0))),
            };
            fx.emit(format!("call void @{f}(ptr {out}, {args})"));
            (fx.load("%str", &out), Ty::Str)
        }
        "file.path_extension" => {
            let out = fx.alloca("%str");
            let ok = fx.r();
            fx.emit(format!("{ok} = call i32 @ante_path_extension(ptr {out}, ptr {})", v(0)));
            let has = fx.r();
            fx.emit(format!("{has} = icmp ne i32 {ok}, 0"));
            let r = opt_select(fx, &ret, &has, &mut |fx| fx.load("%str", &out));
            (r, ret)
        }
        "system.nadupu" => {
            // `Phalitham{status, stdout, stderr}`.
            let rec_ty = opt_inner(&ret);
            let rl = fx.ll(&rec_ty);
            let st = fx.alloca("i32");
            let so = fx.alloca("%str");
            let se = fx.alloca("%str");
            let d = fx.r();
            fx.emit(format!("{d} = extractvalue %slice {}, 0", v(1)));
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %slice {}, 1", v(1)));
            // The runtime reads the arguments as a vector header.
            let hdr = fx.alloca("%vec");
            let h1 = fx.r();
            fx.emit(format!("{h1} = insertvalue %vec zeroinitializer, ptr {d}, 0"));
            let h2 = fx.r();
            fx.emit(format!("{h2} = insertvalue %vec {h1}, i64 {n}, 1"));
            fx.emit(format!("store %vec {h2}, ptr {hdr}"));
            let prog = v(0);
            let r = c_result(
                fx,
                &ret,
                &mut |fx, err| {
                    let r = fx.r();
                    fx.emit(format!(
                        "{r} = call i32 @ante_run(ptr {prog}, ptr {hdr}, ptr {st}, ptr {so}, ptr {se}, ptr {err})"
                    ));
                    r
                },
                &mut |fx| {
                    let a = fx.load("i32", &st);
                    let b = fx.load("%str", &so);
                    let c = fx.load("%str", &se);
                    fx.build_struct_vals(
                        &rl,
                        &[a, b, c],
                        &[Ty::Int(IntTy { signed: true, bits: 32 }), Ty::Str, Ty::Str],
                    )
                },
            );
            (r, ret)
        }
        "file.undhaa" => {
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @ante_file_exists(ptr {})", v(0)));
            let b = fx.r();
            fx.emit(format!("{b} = icmp ne i32 {r}, 0"));
            (b, Ty::Bool)
        }
        // ── memory (`naa_risk` only) ──
        "memory.allocate" => {
            let r = fx.r();
            fx.emit(format!("{r} = call ptr @ante_mem_alloc(i64 {}, i64 {})", v(0), v(1)));
            (r, ret)
        }
        "memory.release" => {
            fx.emit(format!("call void @ante_mem_release(ptr {}, i64 {}, i64 {})", v(0), v(1), v(2)));
            unit()
        }
        "memory.reallocate" => {
            let r = fx.r();
            fx.emit(format!(
                "{r} = call ptr @ante_mem_realloc(ptr {}, i64 {}, i64 {}, i64 {})",
                v(0),
                v(1),
                v(2),
                v(3)
            ));
            (r, ret)
        }
        "memory.copy" => {
            fx.emit(format!(
                "call void @llvm.memmove.p0.p0.i64(ptr {}, ptr {}, i64 {}, i1 false)",
                v(1),
                v(0),
                v(2)
            ));
            unit()
        }
        "system.vellipo" => {
            fx.emit(format!("call void @ante_vellipo(i32 {})", v(0)));
            fx.emit("unreachable");
            let dead = fx.label();
            fx.start(&dead);
            ("undef".into(), Ty::Never)
        }
        "system.flush" => {
            fx.emit("call void @ante_flush()");
            unit()
        }
        "system.args" => {
            let out = fx.alloca("%vec");
            fx.emit(format!("call void @ante_args(ptr {out})"));
            (fx.load("%vec", &out), ret)
        }
        "system.os" | "system.arch" => {
            let out = fx.alloca("%str");
            fx.emit(format!("call void @ante_{}(ptr {out})", &key["system.".len()..]));
            (fx.load("%str", &out), Ty::Str)
        }
        "system.env" | "system.line_chadhuvu" => {
            let out = fx.alloca("%str");
            let ok = fx.r();
            if key == "system.env" {
                fx.emit(format!("{ok} = call i32 @ante_env(ptr {}, ptr {out})", v(0)));
            } else {
                fx.emit(format!("{ok} = call i32 @ante_read_line(ptr {out})"));
            }
            let has = fx.r();
            fx.emit(format!("{has} = icmp ne i32 {ok}, 0"));
            let r = opt_select(fx, &ret, &has, &mut |fx| fx.load("%str", &out));
            (r, ret)
        }
        "samayam.ippudu" => {
            let n = fx.r();
            fx.emit(format!("{n} = call i64 @ante_now_ns()"));
            let lt = fx.ll(&ret);
            let r = fx.r();
            fx.emit(format!("{r} = insertvalue {lt} undef, i64 {n}, 0"));
            (r, ret)
        }
        "Kshanam.gadichindhi" => {
            let kt = match &t(0) {
                Ty::Ref(_, a) => (**a).clone(),
                other => other.clone(),
            };
            let kl = fx.ll(&kt);
            let p = fx.gep(&kl, &v(0), &["0".into(), "0".into()]);
            let then = fx.load("i64", &p);
            let now = fx.r();
            fx.emit(format!("{now} = call i64 @ante_now_ns()"));
            let d = fx.r();
            fx.emit(format!("{d} = sub i64 {now}, {then}"));
            let lt = fx.ll(&ret);
            let r = fx.r();
            fx.emit(format!("{r} = insertvalue {lt} undef, i64 {d}, 0"));
            (r, ret)
        }
        "Kaalam.sec" => {
            // Whole seconds; negative or past `aasajeevi ankam64` nanoseconds is T01.
            let n = v(0);
            let wide = fx.r();
            fx.emit(format!("{wide} = sext i64 {n} to i128"));
            let ns = fx.r();
            fx.emit(format!("{ns} = mul i128 {wide}, 1000000000"));
            let neg = fx.r();
            fx.emit(format!("{neg} = icmp slt i128 {ns}, 0"));
            let big = fx.r();
            fx.emit(format!("{big} = icmp ugt i128 {ns}, 18446744073709551615"));
            let bad = fx.r();
            fx.emit(format!("{bad} = or i1 {neg}, {big}"));
            let m = fx.gen.str_const("`Kaalam` lo ee sec pattadhu");
            let loc = fx.loc(span);
            fx.guard(&bad, |fx| {
                let sp = fx.spill("%str", &m);
                fx.emit(format!("call void @ante_trap(ptr {sp}, ptr {loc})"));
            });
            let lo = fx.r();
            fx.emit(format!("{lo} = trunc i128 {ns} to i64"));
            let kl = fx.ll(&ret);
            let r = fx.r();
            fx.emit(format!("{r} = insertvalue {kl} undef, i64 {lo}, 0"));
            (r, ret)
        }
        "Kaalam.millisec" | "Kaalam.nanosec" => {
            let kt = match &t(0) {
                Ty::Ref(_, a) => (**a).clone(),
                other => other.clone(),
            };
            let kl = fx.ll(&kt);
            let p = fx.gep(&kl, &v(0), &["0".into(), "0".into()]);
            let ns = fx.load("i64", &p);
            if key == "Kaalam.nanosec" {
                return (ns, Ty::ANKAM);
            }
            let r = fx.r();
            fx.emit(format!("{r} = sdiv i64 {ns}, 1000000"));
            (r, Ty::ANKAM)
        }
        _ => unsupported(fx, &key, dest),
    }
}

/// `maarchu` (lossless), `kudhirithe_maarchu` (checked), `kolpothu_maarchu` (lossy) (§18.4).
fn convert(fx: &mut Fx, key: &str, v: &str, from: &Ty, to: &Ty, dest: &Ty, span: Span) -> (String, Ty) {
    let checked = key == "prelude.kudhirithe_maarchu";
    let lossy = key == "prelude.kolpothu_maarchu";
    // The exact conversion, plus (for checked) a flag saying it fits.
    let (val, fits): (String, Option<String>) = match (from, to) {
        (Ty::Int(_) | Ty::Char, Ty::Int(b)) => {
            let a = match from {
                Ty::Int(a) => *a,
                _ => IntTy { signed: false, bits: 32 },
            };
            let wide = fx.widen(v, a);
            let r = fx.narrow(&wide, *b);
            let fits =
                if lossy || (b.bits >= a.bits && (a.signed == b.signed || (!a.signed && b.bits > a.bits))) {
                    None
                } else {
                    // Fits when converting back gives the same value with the same sign.
                    let back = fx.widen(&r, *b);
                    let same = fx.r();
                    fx.emit(format!("{same} = icmp eq i64 {back}, {wide}"));
                    let sign_ok = if a.signed != b.signed {
                        let neg = fx.r();
                        fx.emit(format!("{neg} = icmp slt i64 {wide}, 0"));
                        let negb = fx.r();
                        fx.emit(format!("{negb} = icmp slt i64 {back}, 0"));
                        let both = fx.r();
                        fx.emit(format!("{both} = icmp eq i1 {neg}, {negb}"));
                        let ok = fx.r();
                        fx.emit(format!("{ok} = and i1 {same}, {both}"));
                        ok
                    } else {
                        same
                    };
                    Some(sign_ok)
                };
            (r, fits)
        }
        (Ty::Float(fb), Ty::Int(b)) => {
            let ft = if *fb == 32 { "float" } else { "double" };
            let s = if b.signed { 's' } else { 'u' };
            if lossy {
                let i = format!("i{}", b.bits);
                let r = fx.r();
                fx.emit(format!(
                    "{r} = call {i} @llvm.fpto{s}i.sat.{i}.{}({ft} {v})",
                    if *fb == 32 { "f32" } else { "f64" }
                ));
                fx.gen.intrinsic_decl(&format!(
                    "declare {i} @llvm.fpto{s}i.sat.{i}.{}({ft})",
                    if *fb == 32 { "f32" } else { "f64" }
                ));
                (r, None)
            } else {
                let i = format!("i{}", b.bits);
                let r = fx.r();
                fx.emit(format!("{r} = fpto{s}i {ft} {v} to {i}"));
                let back = fx.r();
                fx.emit(format!("{back} = {s}itofp {i} {r} to {ft}"));
                let same = fx.r();
                fx.emit(format!("{same} = fcmp oeq {ft} {back}, {v}"));
                // In range: check the bounds before trusting the round trip.
                let (lo, hi) = b.range();
                let lo_ok = fx.r();
                fx.emit(format!("{lo_ok} = fcmp oge {ft} {v}, {}", float_lit(*fb, lo as f64)));
                let hi_ok = fx.r();
                fx.emit(format!("{hi_ok} = fcmp olt {ft} {v}, {}", float_lit(*fb, (hi as f64) + 1.0)));
                let a1 = fx.r();
                fx.emit(format!("{a1} = and i1 {lo_ok}, {hi_ok}"));
                let ok = fx.r();
                fx.emit(format!("{ok} = and i1 {a1}, {same}"));
                (r, Some(ok))
            }
        }
        (Ty::Int(a), Ty::Float(fb)) => {
            let ft = if *fb == 32 { "float" } else { "double" };
            let s = if a.signed { 's' } else { 'u' };
            let r = fx.r();
            fx.emit(format!("{r} = {s}itofp i{} {v} to {ft}", a.bits));
            let fits = if lossy || a.bits <= if *fb == 32 { 16 } else { 32 } {
                None
            } else {
                let back = fx.r();
                fx.emit(format!("{back} = fpto{s}i {ft} {r} to i{}", a.bits));
                let same = fx.r();
                fx.emit(format!("{same} = icmp eq i{} {back}, {v}", a.bits));
                Some(same)
            };
            (r, fits)
        }
        (Ty::Float(a), Ty::Float(b)) => {
            if a == b {
                (v.to_string(), None)
            } else if *b == 64 {
                let r = fx.r();
                fx.emit(format!("{r} = fpext float {v} to double"));
                (r, None)
            } else {
                let r = fx.r();
                fx.emit(format!("{r} = fptrunc double {v} to float"));
                let back = fx.r();
                fx.emit(format!("{back} = fpext float {r} to double"));
                let same = fx.r();
                fx.emit(format!("{same} = fcmp ueq double {back}, {v}"));
                (r, if lossy { None } else { Some(same) })
            }
        }
        (Ty::Int(a), Ty::Char) => {
            let w = fx.widen(v, *a);
            let r = fx.narrow(&w, IntTy { signed: false, bits: 32 });
            let lo = fx.r();
            fx.emit(format!("{lo} = icmp ult i64 {w}, 55296"));
            let mid = fx.r();
            fx.emit(format!("{mid} = icmp ugt i64 {w}, 57343"));
            let hi = fx.r();
            fx.emit(format!("{hi} = icmp ule i64 {w}, 1114111"));
            let m2 = fx.r();
            fx.emit(format!("{m2} = and i1 {mid}, {hi}"));
            let ok = fx.r();
            fx.emit(format!("{ok} = or i1 {lo}, {m2}"));
            (r, Some(ok))
        }
        // Raw pointers (§22.2): the typechecker allows these only in `kolpothu_maarchu`.
        (Ty::Ptr(..), Ty::Ptr(..)) => (v.to_string(), None),
        (Ty::Ptr(..), Ty::Int(b)) => {
            let r = fx.r();
            fx.emit(format!("{r} = ptrtoint ptr {v} to i{}", b.bits));
            (r, None)
        }
        (Ty::Int(a), Ty::Ptr(..)) => {
            let w = fx.widen(v, *a);
            let r = fx.r();
            fx.emit(format!("{r} = inttoptr i64 {w} to ptr"));
            (r, None)
        }
        _ => return unsupported(fx, key, dest),
    };
    if checked {
        let opt = dest.clone();
        let r = match fits {
            Some(ok) => opt_select(fx, &opt, &ok, &mut |_| val.clone()),
            None => {
                let inner = opt_inner(&opt);
                some(fx, &opt, &val, &inner)
            }
        };
        return (r, opt);
    }
    if let (false, Some(ok)) = (lossy, fits) {
        // `maarchu` is checked at compile time (E0409); this guards runtime data anyway.
        let bad = fx.r();
        fx.emit(format!("{bad} = xor i1 {ok}, true"));
        let loc = fx.loc(span);
        fx.guard(&bad, |fx| fx.emit(format!("call void @ante_trap_code(i32 99, ptr {loc})")));
    }
    (val, to.clone())
}

fn lekkalu(fx: &mut Fx, name: &str, t: &Ty, vals: &[String], ret: &Ty, span: Span) -> (String, Ty) {
    let a = vals.first().cloned().unwrap_or_default();
    let b = vals.get(1).cloned().unwrap_or_default();
    if let Some(op) =
        name.strip_prefix("chuttu_").or(name.strip_prefix("trupthiga_")).or(name.strip_prefix("kudhirithe_"))
    {
        let Ty::Int(it) = t else { return unsupported(fx, name, ret) };
        let i = format!("i{}", it.bits);
        let s = if it.signed { 's' } else { 'u' };
        if name.starts_with("chuttu_") {
            let ins = match op {
                "kalupu" => "add",
                "theesey" => "sub",
                _ => "mul",
            };
            let r = fx.r();
            fx.emit(format!("{r} = {ins} {i} {a}, {b}"));
            return (r, t.clone());
        }
        if op == "panchu" {
            // `kudhirithe_panchu`: `ledhu` for division by zero or MIN / -1.
            let zero = fx.r();
            fx.emit(format!("{zero} = icmp eq {i} {b}, 0"));
            let bad = if it.signed {
                let (min, _) = it.range();
                let x = fx.r();
                fx.emit(format!("{x} = icmp eq {i} {a}, {min}"));
                let y = fx.r();
                fx.emit(format!("{y} = icmp eq {i} {b}, -1"));
                let both = fx.r();
                fx.emit(format!("{both} = and i1 {x}, {y}"));
                let any = fx.r();
                fx.emit(format!("{any} = or i1 {zero}, {both}"));
                any
            } else {
                zero
            };
            let good = fx.r();
            fx.emit(format!("{good} = xor i1 {bad}, true"));
            let ins = if it.signed { "sdiv" } else { "udiv" };
            let (a2, b2, i2) = (a.clone(), b.clone(), i.clone());
            let r = opt_select(fx, ret, &good, &mut |fx| {
                // Divide by 1 when the result is discarded, so nothing traps.
                let safe = fx.r();
                fx.emit(format!("{safe} = select i1 {good}, {i2} {b2}, {i2} 1"));
                let q = fx.r();
                fx.emit(format!("{q} = {ins} {i2} {a2}, {safe}"));
                q
            });
            return (r, ret.clone());
        }
        let base = match op {
            "kalupu" => "add",
            "theesey" => "sub",
            _ => "mul",
        };
        if name.starts_with("trupthiga_") && base != "mul" {
            let r = fx.r();
            fx.emit(format!("{r} = call {i} @llvm.{s}{base}.sat.{i}({i} {a}, {i} {b})"));
            fx.gen.intrinsic_decl(&format!("declare {i} @llvm.{s}{base}.sat.{i}({i}, {i})"));
            return (r, t.clone());
        }
        let pair = fx.r();
        fx.emit(format!("{pair} = call {{ {i}, i1 }} @llvm.{s}{base}.with.overflow.{i}({i} {a}, {i} {b})"));
        fx.gen.intrinsic_decl(&format!("declare {{ {i}, i1 }} @llvm.{s}{base}.with.overflow.{i}({i}, {i})"));
        let v = fx.r();
        fx.emit(format!("{v} = extractvalue {{ {i}, i1 }} {pair}, 0"));
        let ov = fx.r();
        fx.emit(format!("{ov} = extractvalue {{ {i}, i1 }} {pair}, 1"));
        if name.starts_with("kudhirithe_") {
            let ok = fx.r();
            fx.emit(format!("{ok} = xor i1 {ov}, true"));
            let v2 = v.clone();
            return (opt_select(fx, ret, &ok, &mut |_| v2.clone()), ret.clone());
        }
        // Saturating multiply: the bound in the direction of the true result.
        let (lo, hi) = it.range();
        let neg = if it.signed {
            let x = fx.r();
            fx.emit(format!("{x} = icmp slt {i} {a}, 0"));
            let y = fx.r();
            fx.emit(format!("{y} = icmp slt {i} {b}, 0"));
            let n = fx.r();
            fx.emit(format!("{n} = xor i1 {x}, {y}"));
            n
        } else {
            "false".into()
        };
        let bound = fx.r();
        fx.emit(format!("{bound} = select i1 {neg}, {i} {lo}, {i} {}", crate::interp::wrap(*it, hi)));
        let r = fx.r();
        fx.emit(format!("{r} = select i1 {ov}, {i} {bound}, {i} {v}"));
        let _ = span;
        return (r, t.clone());
    }
    match (name, t) {
        ("sqrt" | "sin" | "cos" | "exp" | "log" | "floor" | "ceil" | "round" | "abs", Ty::Float(fb)) => {
            let (ft, sfx) = if *fb == 32 { ("float", "f32") } else { ("double", "f64") };
            let intr = if name == "abs" { "fabs" } else { name };
            let r = fx.r();
            fx.emit(format!("{r} = call {ft} @llvm.{intr}.{sfx}({ft} {a})"));
            fx.gen.intrinsic_decl(&format!("declare {ft} @llvm.{intr}.{sfx}({ft})"));
            (r, t.clone())
        }
        ("tan", Ty::Float(fb)) => {
            let (ft, f) = if *fb == 32 { ("float", "tanf") } else { ("double", "tan") };
            let r = fx.r();
            fx.emit(format!("{r} = call {ft} @{f}({ft} {a})"));
            (r, t.clone())
        }
        ("pow", Ty::Float(fb)) => {
            let (ft, sfx) = if *fb == 32 { ("float", "f32") } else { ("double", "f64") };
            let r = fx.r();
            fx.emit(format!("{r} = call {ft} @llvm.pow.{sfx}({ft} {a}, {ft} {b})"));
            fx.gen.intrinsic_decl(&format!("declare {ft} @llvm.pow.{sfx}({ft}, {ft})"));
            (r, t.clone())
        }
        ("abs", Ty::Int(it)) => {
            let i = format!("i{}", it.bits);
            let neg = fx.r();
            fx.emit(format!("{neg} = icmp slt {i} {a}, 0"));
            let (min, _) = it.range();
            let is_min = fx.r();
            fx.emit(format!("{is_min} = icmp eq {i} {a}, {min}"));
            fx.overflow_guard(&is_min, *it, "-", &a, "0", true, span);
            let m = fx.r();
            fx.emit(format!("{m} = sub {i} 0, {a}"));
            let r = fx.r();
            fx.emit(format!("{r} = select i1 {neg}, {i} {m}, {i} {a}"));
            (r, t.clone())
        }
        ("min" | "max", Ty::Int(_) | Ty::Float(_)) => {
            let lt = fx.ll(t);
            let cmp = match (t, name) {
                (Ty::Int(it), "min") => format!("icmp {} {lt}", if it.signed { "sle" } else { "ule" }),
                (Ty::Int(it), _) => format!("icmp {} {lt}", if it.signed { "sge" } else { "uge" }),
                (_, "min") => format!("fcmp ole {lt}"),
                _ => format!("fcmp oge {lt}"),
            };
            let c = fx.r();
            fx.emit(format!("{c} = {cmp} {a}, {b}"));
            let r = fx.r();
            fx.emit(format!("{r} = select i1 {c}, {lt} {a}, {lt} {b}"));
            (r, t.clone())
        }
        ("motham" | "dot", Ty::Int(_) | Ty::Float(_)) => sum(fx, name == "dot", t, &a, &b, span),
        ("polchu_amsam", _) => {
            // IEEE totalOrder: flip the magnitude bits of negatives, compare signed.
            let key = |fx: &mut Fx, x: &str| {
                let bits = fx.r();
                fx.emit(format!("{bits} = bitcast double {x} to i64"));
                let sign = fx.r();
                fx.emit(format!("{sign} = ashr i64 {bits}, 63"));
                let mask = fx.r();
                fx.emit(format!("{mask} = lshr i64 {sign}, 1"));
                let k = fx.r();
                fx.emit(format!("{k} = xor i64 {bits}, {mask}"));
                k
            };
            let (x, y) = (key(fx, &a), key(fx, &b));
            let lt_c = fx.r();
            fx.emit(format!("{lt_c} = icmp slt i64 {x}, {y}"));
            let gt_c = fx.r();
            fx.emit(format!("{gt_c} = icmp sgt i64 {x}, {y}"));
            let g = fx.r();
            fx.emit(format!("{g} = select i1 {gt_c}, i32 2, i32 1"));
            let tag = fx.r();
            fx.emit(format!("{tag} = select i1 {lt_c}, i32 0, i32 {g}"));
            let lt = fx.ll(ret);
            let slot = fx.alloca(&lt);
            let tp = fx.gep(&lt, &slot, &["0".into(), "0".into()]);
            fx.emit(format!("store i32 {tag}, ptr {tp}"));
            (fx.load(&lt, &slot), ret.clone())
        }
        _ => unsupported(fx, &format!("lekkalu.{name}"), ret),
    }
}

/// `lekkalu.motham` and `lekkalu.dot` over `&mukka` values. Integers sum
/// exactly and trap T01 when a partial sum leaves the type; floats sum in
/// `amsam64` over eight lanes (element `i` into lane `i % 8`), then the
/// lanes in order: the documented blocked order (§21.3).
fn sum(fx: &mut Fx, dot: bool, t: &Ty, a: &str, b: &str, span: Span) -> (String, Ty) {
    let parts = |fx: &mut Fx, s: &str| {
        let d = fx.r();
        fx.emit(format!("{d} = extractvalue %slice {s}, 0"));
        let n = fx.r();
        fx.emit(format!("{n} = extractvalue %slice {s}, 1"));
        (d, n)
    };
    let (da, na) = parts(fx, a);
    let db = if dot {
        let (db, nb) = parts(fx, b);
        let differ = fx.r();
        fx.emit(format!("{differ} = icmp ne i64 {na}, {nb}"));
        let (na2, nb2) = (na.clone(), nb.clone());
        fx.guard(&differ, |fx| {
            let buf = new_buf(fx);
            push_lit(fx, &buf, "dot: podavulu veru (");
            fx.emit(format!("call void @ante_buf_i64(ptr {buf}, i64 {na2})"));
            push_lit(fx, &buf, " , ");
            fx.emit(format!("call void @ante_buf_i64(ptr {buf}, i64 {nb2})"));
            push_lit(fx, &buf, ")");
            let loc = fx.loc(span);
            fx.emit(format!("call void @ante_trap(ptr {buf}, ptr {loc})"));
        });
        db
    } else {
        String::new()
    };
    let et = fx.ll(t);
    match t {
        Ty::Int(it) => {
            let ext = if it.signed { "sext" } else { "zext" };
            let acc = fx.alloca("i128");
            fx.emit(format!("store i128 0, ptr {acc}"));
            let (lo, hi) = it.range();
            let msg = format!("`{}` lo motham pattadhu", crate::interp::int_name(*it));
            let elem = t.clone();
            super::glue::each(fx, &da, &na, &elem, &mut |fx, p, _| {
                let x = fx.load(&et, p);
                let wx = fx.r();
                fx.emit(format!("{wx} = {ext} {et} {x} to i128"));
                let term = if dot {
                    let off = elem_offset(fx, p, &da);
                    let q = fx.r();
                    fx.emit(format!("{q} = getelementptr i8, ptr {db}, i64 {off}"));
                    let y = fx.load(&et, &q);
                    let wy = fx.r();
                    fx.emit(format!("{wy} = {ext} {et} {y} to i128"));
                    let m = fx.r();
                    fx.emit(format!("{m} = mul i128 {wx}, {wy}"));
                    m
                } else {
                    wx
                };
                let cur = fx.load("i128", &acc);
                let nv = fx.r();
                fx.emit(format!("{nv} = add i128 {cur}, {term}"));
                fx.emit(format!("store i128 {nv}, ptr {acc}"));
                let below = fx.r();
                fx.emit(format!("{below} = icmp slt i128 {nv}, {lo}"));
                let above = fx.r();
                fx.emit(format!("{above} = icmp sgt i128 {nv}, {hi}"));
                let bad = fx.r();
                fx.emit(format!("{bad} = or i1 {below}, {above}"));
                let m = fx.gen.str_const(&msg);
                let loc = fx.loc(span);
                fx.guard(&bad, |fx| {
                    let sp = fx.spill("%str", &m);
                    fx.emit(format!("call void @ante_trap(ptr {sp}, ptr {loc})"));
                });
            });
            let total = fx.load("i128", &acc);
            let r = fx.r();
            fx.emit(format!("{r} = trunc i128 {total} to {et}"));
            (r, t.clone())
        }
        _ => {
            let lanes = fx.alloca("[8 x double]");
            fx.emit(format!("store [8 x double] zeroinitializer, ptr {lanes}"));
            let wide = |fx: &mut Fx, x: &str| {
                if et == "float" {
                    let w = fx.r();
                    fx.emit(format!("{w} = fpext float {x} to double"));
                    w
                } else {
                    x.to_string()
                }
            };
            let elem = t.clone();
            super::glue::each(fx, &da, &na, &elem, &mut |fx, p, _| {
                let x = fx.load(&et, p);
                let wx = wide(fx, &x);
                let off = elem_offset(fx, p, &da);
                let v = if dot {
                    let q = fx.r();
                    fx.emit(format!("{q} = getelementptr i8, ptr {db}, i64 {off}"));
                    let y = fx.load(&et, &q);
                    let wy = wide(fx, &y);
                    let m = fx.r();
                    fx.emit(format!("{m} = fmul double {wx}, {wy}"));
                    m
                } else {
                    wx
                };
                let (size, _) = fx.gen.size_align(&elem);
                let i = fx.r();
                fx.emit(format!("{i} = udiv i64 {off}, {size}"));
                let lane = fx.r();
                fx.emit(format!("{lane} = and i64 {i}, 7"));
                let lp = fx.gep("[8 x double]", &lanes, &["0".into(), format!("i64 {lane}")]);
                let cur = fx.load("double", &lp);
                let nv = fx.r();
                fx.emit(format!("{nv} = fadd double {cur}, {v}"));
                fx.emit(format!("store double {nv}, ptr {lp}"));
            });
            let mut total = "0.0".to_string();
            for k in 0..8 {
                let lp = fx.gep("[8 x double]", &lanes, &["0".into(), k.to_string()]);
                let x = fx.load("double", &lp);
                let r = fx.r();
                fx.emit(format!("{r} = fadd double {total}, {x}"));
                total = r;
            }
            if et == "float" {
                let r = fx.r();
                fx.emit(format!("{r} = fptrunc double {total} to float"));
                total = r;
            }
            (total, t.clone())
        }
    }
}

/// The byte offset of element pointer `p` from `base`.
fn elem_offset(fx: &mut Fx, p: &str, base: &str) -> String {
    let pi = fx.r();
    fx.emit(format!("{pi} = ptrtoint ptr {p} to i64"));
    let bi = fx.r();
    fx.emit(format!("{bi} = ptrtoint ptr {base} to i64"));
    let off = fx.r();
    fx.emit(format!("{off} = sub i64 {pi}, {bi}"));
    off
}
