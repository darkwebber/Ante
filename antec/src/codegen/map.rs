//! `jodi`, `gajibiji_jodi` and `gumpu` (§21.2) in native code. The runtime
//! keeps an insertion-ordered hash map (`ante_map_*` in ante_rt.c); the
//! code here hashes, compares, moves and drops the keys and values. Each
//! entry is `{ i64 hash, i8 live, K, V }`, with `V = khali` for `gumpu`.
//! Keys hash by structure and compare with `==`, as in the interpreter.

use super::func::Fx;
use super::glue::{call_drop, call_xerox, eq_call};
use super::Item;
use crate::types::{Con, IntTy, Ty};

const SEED: &str = "-3750763034362895579"; // 0xcbf29ce484222325, the FNV offset basis

/// The key and value types of a map type (`gumpu[T]` has `khali` values).
pub(super) fn key_val(ty: &Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::App(Con::Jodi | Con::GajibijiJodi, a) => Some((a[0].clone(), a[1].clone())),
        Ty::App(Con::Gumpu, a) => Some((a[0].clone(), Ty::Unit)),
        _ => None,
    }
}

/// An entry's layout: its LLVM type, size, and the key's offset.
struct Layout {
    ety: String,
    esize: u64,
    koff: u64,
    key: Ty,
    val: Ty,
}

fn layout(fx: &mut Fx, key: &Ty, val: &Ty) -> Layout {
    let entry = Ty::Tuple(vec![
        Ty::Int(IntTy { signed: false, bits: 64 }),
        Ty::Int(IntTy { signed: false, bits: 8 }),
        key.clone(),
        val.clone(),
    ]);
    let ety = fx.ll(&entry);
    let (esize, _) = fx.gen.size_align(&entry);
    let (_, ka) = fx.gen.size_align(key);
    Layout { ety, esize, koff: 9u64.div_ceil(ka) * ka, key: key.clone(), val: val.clone() }
}

fn layout_of(fx: &mut Fx, map_ty: &Ty) -> Layout {
    let (k, v) = key_val(map_ty).unwrap_or((Ty::Unit, Ty::Unit));
    layout(fx, &k, &v)
}

/// The hash of the key at `kp`.
fn hash(fx: &mut Fx, key: &Ty, kp: &str) -> String {
    let f = fx.gen.item(Item::Hash(key.clone()));
    let h = fx.r();
    fx.emit(format!("{h} = call i64 {f}(ptr {kp}, i64 {SEED})"));
    h
}

/// The index of the entry holding the key at `kp`, or -1.
fn find(fx: &mut Fx, l: &Layout, m: &str, h: &str, kp: &str) -> String {
    let eq = fx.gen.item(Item::Eq(l.key.clone()));
    let i = fx.r();
    fx.emit(format!(
        "{i} = call i64 @ante_map_find(ptr {m}, i64 {h}, ptr {kp}, i64 {}, i64 {}, ptr {eq})",
        l.esize, l.koff
    ));
    i
}

/// Pointer to entry `i`.
fn entry(fx: &mut Fx, l: &Layout, m: &str, i: &str) -> String {
    let dp = fx.gep("%map", m, &["0".into(), "0".into()]);
    let d = fx.load("ptr", &dp);
    fx.gep(&l.ety, &d, &[format!("i64 {i}")])
}

fn key_ptr(fx: &mut Fx, l: &Layout, e: &str) -> String {
    fx.gep(&l.ety, e, &["0".into(), "2".into()])
}

fn val_ptr(fx: &mut Fx, l: &Layout, e: &str) -> String {
    fx.gep(&l.ety, e, &["0".into(), "3".into()])
}

/// Adds an entry for a missing key and stores the key and value in it.
fn add(fx: &mut Fx, l: &Layout, m: &str, h: &str, k: &str, v: &str) -> String {
    let i = fx.r();
    fx.emit(format!("{i} = call i64 @ante_map_add(ptr {m}, i64 {h}, i64 {})", l.esize));
    let e = entry(fx, l, m, &i);
    let kp = key_ptr(fx, l, &e);
    let kl = fx.ll(&l.key);
    fx.emit(format!("store {kl} {k}, ptr {kp}"));
    let vp = val_ptr(fx, l, &e);
    let vl = fx.ll(&l.val);
    fx.emit(format!("store {vl} {v}, ptr {vp}"));
    e
}

/// `found ? yes() : no()`, each producing a value of LLVM type `lt`.
fn branch(
    fx: &mut Fx,
    lt: &str,
    found: &str,
    yes: &mut dyn FnMut(&mut Fx) -> String,
    no: &mut dyn FnMut(&mut Fx) -> String,
) -> String {
    let slot = fx.alloca(lt);
    let (y, n, done) = (fx.label(), fx.label(), fx.label());
    fx.emit(format!("br i1 {found}, label %{y}, label %{n}"));
    fx.start(&y);
    let a = yes(fx);
    fx.emit(format!("store {lt} {a}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&n);
    let b = no(fx);
    fx.emit(format!("store {lt} {b}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    fx.load(lt, &slot)
}

fn some(fx: &mut Fx, opt: &Ty, v: &str) -> String {
    let inner = match opt {
        Ty::App(Con::Undochu, a) => a[0].clone(),
        _ => Ty::Unit,
    };
    fx.build_enum(opt, 0, &[v.to_string()], std::slice::from_ref(&inner))
}

fn none(fx: &mut Fx, opt: &Ty) -> String {
    fx.build_enum(opt, 1, &[], &[])
}

/// A map method (§21.2). `vals[0]` points at the map; `tys` are the formal
/// types, receiver first.
pub(super) fn call(fx: &mut Fx, method: &str, vals: &[String], tys: &[Ty], ret: &Ty) -> Option<(String, Ty)> {
    if method == "kotthadhi" {
        return Some(("zeroinitializer".into(), ret.clone()));
    }
    let map_ty = match tys.first() {
        Some(Ty::Ref(_, inner)) => (**inner).clone(),
        Some(other) => other.clone(),
        None => return None,
    };
    let l = layout_of(fx, &map_ty);
    let m = vals.first().cloned().unwrap_or_default();
    let set = matches!(map_ty, Ty::App(Con::Gumpu, _));
    // The key argument as a pointer (passed by value or by reference).
    let key_arg = |fx: &mut Fx| -> String {
        match tys.get(1) {
            Some(Ty::Ref(..)) => vals[1].clone(),
            _ => {
                let kl = fx.ll(&l.key);
                fx.spill(&kl, &vals[1])
            }
        }
    };
    let r = match method {
        "enni" => {
            let p = fx.gep("%map", &m, &["0".into(), "5".into()]);
            (fx.load("i64", &p), Ty::ANKAM)
        }
        "undhaa" => {
            let kp = key_arg(fx);
            let h = hash(fx, &l.key, &kp);
            let i = find(fx, &l, &m, &h, &kp);
            let b = fx.r();
            fx.emit(format!("{b} = icmp sge i64 {i}, 0"));
            (b, Ty::Bool)
        }
        "thechu" | "maare_thechu" => {
            let kp = key_arg(fx);
            let h = hash(fx, &l.key, &kp);
            let i = find(fx, &l, &m, &h, &kp);
            let found = fx.r();
            fx.emit(format!("{found} = icmp sge i64 {i}, 0"));
            let lt = fx.ll(ret);
            let r = branch(
                fx,
                &lt,
                &found,
                &mut |fx| {
                    let e = entry(fx, &l, &m, &i);
                    let vp = val_ptr(fx, &l, &e);
                    some(fx, ret, &vp)
                },
                &mut |fx| none(fx, ret),
            );
            (r, ret.clone())
        }
        "cherchu" => {
            // A new key goes at the end; an existing one keeps its place and
            // gets the new value, and the key passed in is dropped.
            let kp = key_arg(fx);
            let h = hash(fx, &l.key, &kp);
            let i = find(fx, &l, &m, &h, &kp);
            let found = fx.r();
            fx.emit(format!("{found} = icmp sge i64 {i}, 0"));
            let lt = fx.ll(ret);
            let kl = fx.ll(&l.key);
            let vl = fx.ll(&l.val);
            let v = if set { "zeroinitializer".to_string() } else { vals[2].clone() };
            let r = branch(
                fx,
                &lt,
                &found,
                &mut |fx| {
                    let e = entry(fx, &l, &m, &i);
                    let key = l.key.clone();
                    call_drop(fx, &key, &kp);
                    if set {
                        return "false".into();
                    }
                    let vp = val_ptr(fx, &l, &e);
                    let old = fx.load(&vl, &vp);
                    fx.emit(format!("store {vl} {v}, ptr {vp}"));
                    some(fx, ret, &old)
                },
                &mut |fx| {
                    let k = fx.load(&kl, &kp);
                    add(fx, &l, &m, &h, &k, &v);
                    if set {
                        "true".into()
                    } else {
                        none(fx, ret)
                    }
                },
            );
            (r, ret.clone())
        }
        "lekapothe_cherchu" => {
            let kp = key_arg(fx);
            let h = hash(fx, &l.key, &kp);
            let i = find(fx, &l, &m, &h, &kp);
            let found = fx.r();
            fx.emit(format!("{found} = icmp sge i64 {i}, 0"));
            let kl = fx.ll(&l.key);
            let vl = fx.ll(&l.val);
            let d = vals[2].clone();
            let r = branch(
                fx,
                "ptr",
                &found,
                &mut |fx| {
                    // The key and default passed in aren't needed.
                    let key = l.key.clone();
                    call_drop(fx, &key, &kp);
                    let val = l.val.clone();
                    if fx.needs_drop(&val) {
                        let dp = fx.spill(&vl, &d);
                        call_drop(fx, &val, &dp);
                    }
                    let e = entry(fx, &l, &m, &i);
                    val_ptr(fx, &l, &e)
                },
                &mut |fx| {
                    let k = fx.load(&kl, &kp);
                    let e = add(fx, &l, &m, &h, &k, &d);
                    val_ptr(fx, &l, &e)
                },
            );
            (r, ret.clone())
        }
        "theesey" => {
            let kp = key_arg(fx);
            let h = hash(fx, &l.key, &kp);
            let i = find(fx, &l, &m, &h, &kp);
            let found = fx.r();
            fx.emit(format!("{found} = icmp sge i64 {i}, 0"));
            let lt = fx.ll(ret);
            let vl = fx.ll(&l.val);
            let r = branch(
                fx,
                &lt,
                &found,
                &mut |fx| {
                    // The stored key is dropped; the value moves out.
                    let e = entry(fx, &l, &m, &i);
                    let ekp = key_ptr(fx, &l, &e);
                    let key = l.key.clone();
                    call_drop(fx, &key, &ekp);
                    let vp = val_ptr(fx, &l, &e);
                    let old = fx.load(&vl, &vp);
                    fx.emit(format!("call void @ante_map_remove(ptr {m}, i64 {i}, i64 {})", l.esize));
                    if set {
                        "true".into()
                    } else {
                        some(fx, ret, &old)
                    }
                },
                &mut |fx| if set { "false".into() } else { none(fx, ret) },
            );
            (r, ret.clone())
        }
        _ => return None,
    };
    Some(r)
}

/// `{k1: v1, …}`: inserts the pairs in order, a later duplicate replacing
/// the value.
pub(super) fn literal(fx: &mut Fx, map_ty: &Ty, pairs: &[(String, String)]) -> String {
    let l = layout_of(fx, map_ty);
    let m = fx.alloca("%map");
    fx.emit(format!("store %map zeroinitializer, ptr {m}"));
    let kl = fx.ll(&l.key);
    let vl = fx.ll(&l.val);
    for (k, v) in pairs {
        let kp = fx.spill(&kl, k);
        let h = hash(fx, &l.key, &kp);
        let i = find(fx, &l, &m, &h, &kp);
        let found = fx.r();
        fx.emit(format!("{found} = icmp sge i64 {i}, 0"));
        let (yes, no, done) = (fx.label(), fx.label(), fx.label());
        fx.emit(format!("br i1 {found}, label %{yes}, label %{no}"));
        fx.start(&yes);
        let e = entry(fx, &l, &m, &i);
        let key = l.key.clone();
        call_drop(fx, &key, &kp);
        let vp = val_ptr(fx, &l, &e);
        let val = l.val.clone();
        call_drop(fx, &val, &vp);
        fx.emit(format!("store {vl} {v}, ptr {vp}"));
        fx.emit(format!("br label %{done}"));
        fx.start(&no);
        add(fx, &l, &m, &h, k, v);
        fx.emit(format!("br label %{done}"));
        fx.start(&done);
    }
    fx.load("%map", &m)
}

/// Calls `body(key ptr, value ptr)` for each live entry of the map at `m`,
/// in insertion order.
fn each_entry(fx: &mut Fx, l: &Layout, m: &str, body: &mut dyn FnMut(&mut Fx, &str, &str)) {
    let i = fx.alloca("i64");
    fx.emit(format!("store i64 0, ptr {i}"));
    let (head, inner, live, step, done) = (fx.label(), fx.label(), fx.label(), fx.label(), fx.label());
    fx.emit(format!("br label %{head}"));
    fx.start(&head);
    let iv = fx.load("i64", &i);
    let lp = fx.gep("%map", m, &["0".into(), "1".into()]);
    let n = fx.load("i64", &lp);
    let more = fx.r();
    fx.emit(format!("{more} = icmp ult i64 {iv}, {n}"));
    fx.emit(format!("br i1 {more}, label %{inner}, label %{done}"));
    fx.start(&inner);
    let e = entry(fx, l, m, &iv);
    let fp = fx.gep(&l.ety, &e, &["0".into(), "1".into()]);
    let flag = fx.load("i8", &fp);
    let on = fx.r();
    fx.emit(format!("{on} = icmp ne i8 {flag}, 0"));
    fx.emit(format!("br i1 {on}, label %{live}, label %{step}"));
    fx.start(&live);
    let kp = key_ptr(fx, l, &e);
    let vp = val_ptr(fx, l, &e);
    body(fx, &kp, &vp);
    fx.emit(format!("br label %{step}"));
    fx.start(&step);
    let next = fx.r();
    fx.emit(format!("{next} = add i64 {iv}, 1"));
    fx.emit(format!("store i64 {next}, ptr {i}"));
    fx.emit(format!("br label %{head}"));
    fx.start(&done);
}

/// Drop glue: the live keys and values, then the map's memory.
pub(super) fn drop_map(fx: &mut Fx, ty: &Ty, m: &str) {
    let l = layout_of(fx, ty);
    let (k, v) = (l.key.clone(), l.val.clone());
    if fx.needs_drop(&k) || fx.needs_drop(&v) {
        each_entry(fx, &l, m, &mut |fx, kp, vp| {
            call_drop(fx, &k, kp);
            call_drop(fx, &v, vp);
        });
    }
    fx.emit(format!("call void @ante_map_free(ptr {m})"));
}

/// Display: `{k: v, …}`, or `{x, …}` for a `gumpu`.
pub(super) fn show_map(fx: &mut Fx, ty: &Ty, buf: &str, m: &str) {
    let l = layout_of(fx, ty);
    let set = matches!(ty, Ty::App(Con::Gumpu, _));
    let first = fx.alloca("i1");
    fx.emit(format!("store i1 true, ptr {first}"));
    super::glue::push_text(fx, buf, "{");
    let (k, v) = (l.key.clone(), l.val.clone());
    each_entry(fx, &l, m, &mut |fx, kp, vp| {
        let f = fx.load("i1", &first);
        let (sep, item) = (fx.label(), fx.label());
        fx.emit(format!("br i1 {f}, label %{item}, label %{sep}"));
        fx.start(&sep);
        super::glue::push_text(fx, buf, ", ");
        fx.emit(format!("br label %{item}"));
        fx.start(&item);
        fx.emit(format!("store i1 false, ptr {first}"));
        let sk = fx.gen.item(Item::Show(k.clone()));
        fx.emit(format!("call void {sk}(ptr {buf}, ptr {kp})"));
        if !set {
            super::glue::push_text(fx, buf, ": ");
            let sv = fx.gen.item(Item::Show(v.clone()));
            fx.emit(format!("call void {sv}(ptr {buf}, ptr {vp})"));
        }
    });
    super::glue::push_text(fx, buf, "}");
}

/// Equality: the same number of entries, and every key of `a` in `b` with
/// an equal value. Order doesn't matter. Jumps to `fail` when they differ.
pub(super) fn eq_map(fx: &mut Fx, ty: &Ty, a: &str, b: &str, fail: &str) {
    let l = layout_of(fx, ty);
    let la = fx.gep("%map", a, &["0".into(), "5".into()]);
    let na = fx.load("i64", &la);
    let lb = fx.gep("%map", b, &["0".into(), "5".into()]);
    let nb = fx.load("i64", &lb);
    let same = fx.r();
    fx.emit(format!("{same} = icmp eq i64 {na}, {nb}"));
    let ok = fx.label();
    fx.emit(format!("br i1 {same}, label %{ok}, label %{fail}"));
    fx.start(&ok);
    let (k, v) = (l.key.clone(), l.val.clone());
    let fail = fail.to_string();
    let b = b.to_string();
    let l2 = layout(fx, &k, &v);
    each_entry(fx, &l, a, &mut |fx, kp, vp| {
        let h = hash(fx, &k, kp);
        let i = find(fx, &l2, &b, &h, kp);
        let has = fx.r();
        fx.emit(format!("{has} = icmp sge i64 {i}, 0"));
        let go = fx.label();
        fx.emit(format!("br i1 {has}, label %{go}, label %{fail}"));
        fx.start(&go);
        let e = entry(fx, &l2, &b, &i);
        let wp = val_ptr(fx, &l2, &e);
        let c = eq_call(fx, &v, vp, &wp);
        let next = fx.label();
        fx.emit(format!("br i1 {c}, label %{next}, label %{fail}"));
        fx.start(&next);
    });
}

/// A deep copy keeping the order.
pub(super) fn xerox_map(fx: &mut Fx, ty: &Ty, m: &str) -> String {
    let l = layout_of(fx, ty);
    let out = fx.alloca("%map");
    fx.emit(format!("store %map zeroinitializer, ptr {out}"));
    let (k, v) = (l.key.clone(), l.val.clone());
    let l2 = layout(fx, &k, &v);
    each_entry(fx, &l, m, &mut |fx, kp, vp| {
        let h = hash(fx, &k, kp);
        let kc = call_xerox(fx, &k, kp);
        let vc = call_xerox(fx, &v, vp);
        add(fx, &l2, &out, &h, &kc, &vc);
    });
    fx.load("%map", &out)
}

// ── iteration (§21.4.1) ──────────────────────────────────────────────────
//
// An `%iter` of kind 4: `p` is the map, `a` the next entry position, and
// `done` marks a map the iterator owns (moved to the heap with the loop).

/// The map type a kind-4 iterator with this item type walks.
pub(super) fn iter_map_ty(item: &Ty) -> Option<Ty> {
    match item {
        Ty::Tuple(kv) if kv.len() == 2 => match (&kv[0], &kv[1]) {
            (Ty::Ref(_, k), Ty::Ref(_, v)) => Some(Ty::App(Con::Jodi, vec![(**k).clone(), (**v).clone()])),
            _ => None,
        },
        Ty::Ref(_, k) => Some(Ty::App(Con::Gumpu, vec![(**k).clone()])),
        _ => None,
    }
}

/// The next item of a kind-4 iterator: stores `undochu[item]` at `out`.
pub(super) fn iter_next(fx: &mut Fx, it: &str, item: &Ty, opt: &Ty, out: &str, done: &str) {
    let Some(map_ty) = iter_map_ty(item) else {
        fx.emit(format!("br label %{done}"));
        return;
    };
    let l = layout_of(fx, &map_ty);
    let lt = fx.ll(opt);
    let pp = fx.gep("%iter", it, &["0".into(), "4".into()]);
    let m = fx.load("ptr", &pp);
    let ap = fx.gep("%iter", it, &["0".into(), "2".into()]);
    let head = fx.label();
    let inner = fx.label();
    let live = fx.label();
    fx.emit(format!("br label %{head}"));
    fx.start(&head);
    let i = fx.load("i64", &ap);
    let lp = fx.gep("%map", &m, &["0".into(), "1".into()]);
    let n = fx.load("i64", &lp);
    let more = fx.r();
    fx.emit(format!("{more} = icmp ult i64 {i}, {n}"));
    fx.emit(format!("br i1 {more}, label %{inner}, label %{done}"));
    fx.start(&inner);
    let i1 = fx.r();
    fx.emit(format!("{i1} = add i64 {i}, 1"));
    fx.emit(format!("store i64 {i1}, ptr {ap}"));
    let e = entry(fx, &l, &m, &i);
    let fp = fx.gep(&l.ety, &e, &["0".into(), "1".into()]);
    let flag = fx.load("i8", &fp);
    let on = fx.r();
    fx.emit(format!("{on} = icmp ne i8 {flag}, 0"));
    fx.emit(format!("br i1 {on}, label %{live}, label %{head}"));
    fx.start(&live);
    let kp = key_ptr(fx, &l, &e);
    let x = if let Ty::Tuple(_) = item {
        let vp = val_ptr(fx, &l, &e);
        let il = fx.ll(item);
        let a1 = fx.r();
        fx.emit(format!("{a1} = insertvalue {il} undef, ptr {kp}, 0"));
        let a2 = fx.r();
        fx.emit(format!("{a2} = insertvalue {il} {a1}, ptr {vp}, 1"));
        a2
    } else {
        kp
    };
    let s = fx.build_enum(opt, 0, &[x], std::slice::from_ref(item));
    fx.emit(format!("store {lt} {s}, ptr {out}"));
    fx.emit(format!("br label %{done}"));
}

/// Drops what a kind-4 iterator owns: a map moved into it.
pub(super) fn drop_iter(fx: &mut Fx, item: &Ty, it: &str) {
    let Some(map_ty) = iter_map_ty(item) else { return };
    let op = fx.gep("%iter", it, &["0".into(), "1".into()]);
    let owned = fx.load("i8", &op);
    let is_owned = fx.r();
    fx.emit(format!("{is_owned} = icmp ne i8 {owned}, 0"));
    fx.when(&is_owned, |fx| {
        let pp = fx.gep("%iter", it, &["0".into(), "4".into()]);
        let m = fx.load("ptr", &pp);
        call_drop(fx, &map_ty, &m);
        fx.emit(format!("call void @free(ptr {m})"));
    });
}

// ── hashing ────────────────────────────────────────────────────────────────

/// `i64 hash(ptr %v, i64 %h)`: mixes the value into `h` (see `key_hash` in
/// the interpreter for the algorithm both backends share).
pub(super) fn hash_glue(fx: &mut Fx, ty: &Ty) -> String {
    hash_into(fx, ty, "%v", "%h")
}

/// Mixes a value into the `Hasher` at `hp` (`Hasher.kalupu`, and the
/// built-in `hash_cheyyi` when `structural`).
pub(super) fn hasher_add(fx: &mut Fx, hp: &str, ty: &Ty, v: &str, structural: bool) {
    let sp = fx.gep("{ i64 }", hp, &["0".into(), "0".into()]);
    let h = fx.load("i64", &sp);
    let n = if structural { hash_structure(fx, ty, v, &h) } else { hash_call(fx, ty, v, &h) };
    fx.emit(format!("store i64 {n}, ptr {sp}"));
}

fn mix(fx: &mut Fx, h: &str, x: &str) -> String {
    let r = fx.r();
    fx.emit(format!("{r} = call i64 @llvm.fshl.i64(i64 {h}, i64 {h}, i64 5)"));
    fx.gen.intrinsic_decl("declare i64 @llvm.fshl.i64(i64, i64, i64)");
    let a = fx.r();
    fx.emit(format!("{a} = xor i64 {r}, {x}"));
    let m = fx.r();
    fx.emit(format!("{m} = mul i64 {a}, -7046029254386353131"));
    m
}

fn hash_call(fx: &mut Fx, ty: &Ty, v: &str, h: &str) -> String {
    let f = fx.gen.item(Item::Hash(ty.clone()));
    let r = fx.r();
    fx.emit(format!("{r} = call i64 {f}(ptr {v}, i64 {h})"));
    r
}

fn hash_into(fx: &mut Fx, ty: &Ty, v: &str, h: &str) -> String {
    // A type's own `hash_cheyyi` runs on a `Hasher { _sthithi: h }`.
    if let Ty::App(Con::Adt(_), _) = ty {
        if let Some((body, tys)) = super::glue::user_impl(fx.gen, "HashCheyochu", ty) {
            let hasher = fx.alloca("{ i64 }");
            let sp = fx.gep("{ i64 }", &hasher, &["0".into(), "0".into()]);
            fx.emit(format!("store i64 {h}, ptr {sp}"));
            fx.call_body_vals(body, &tys, &[v.to_string(), hasher.clone()]);
            return fx.load("i64", &sp);
        }
    }
    hash_structure(fx, ty, v, h)
}

/// Hashing by structure: the built-in and `@automatic` `HashCheyochu`.
pub(super) fn hash_structure(fx: &mut Fx, ty: &Ty, v: &str, h: &str) -> String {
    match ty {
        Ty::Int(it) => {
            let x = fx.load(&format!("i{}", it.bits), v);
            let w = fx.widen(&x, *it);
            mix(fx, h, &w)
        }
        Ty::Bool => {
            let x = fx.load("i1", v);
            let w = fx.r();
            fx.emit(format!("{w} = zext i1 {x} to i64"));
            mix(fx, h, &w)
        }
        Ty::Char => {
            let x = fx.load("i32", v);
            let w = fx.r();
            fx.emit(format!("{w} = zext i32 {x} to i64"));
            mix(fx, h, &w)
        }
        Ty::Float(b) => {
            let (ft, it) = if *b == 32 { ("float", "i32") } else { ("double", "i64") };
            let x = fx.load(ft, v);
            let bits = fx.r();
            fx.emit(format!("{bits} = bitcast {ft} {x} to {it}"));
            let w = if *b == 32 {
                let w = fx.r();
                fx.emit(format!("{w} = zext i32 {bits} to i64"));
                w
            } else {
                bits
            };
            mix(fx, h, &w)
        }
        Ty::Str => {
            let r = fx.r();
            fx.emit(format!("{r} = call i64 @ante_hash_text(i64 {h}, ptr {v})"));
            r
        }
        Ty::Unit | Ty::Never => h.to_string(),
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) if !matches!(**inner, Ty::App(Con::Mukka, _)) => {
            let p = fx.load("ptr", v);
            hash_call(fx, inner, &p, h)
        }
        Ty::App(Con::Pette, a) => {
            let p = fx.load("ptr", v);
            hash_call(fx, &a[0], &p, h)
        }
        Ty::Tuple(items) => {
            let lt = fx.ll(ty);
            let mut acc = h.to_string();
            for (i, t) in items.iter().enumerate() {
                let p = fx.gep(&lt, v, &["0".into(), i.to_string()]);
                acc = hash_call(fx, t, &p, &acc);
            }
            acc
        }
        Ty::Array(..) | Ty::App(Con::Varusa, _) | Ty::Ref(..) => {
            let Some((data, len, elem)) = super::glue::seq_at(fx, ty, v) else { return h.to_string() };
            let acc = fx.alloca("i64");
            let start = mix(fx, h, &len);
            fx.emit(format!("store i64 {start}, ptr {acc}"));
            super::glue::each(fx, &data, &len, &elem, &mut |fx, p, _| {
                let cur = fx.load("i64", &acc);
                let n = hash_call(fx, &elem, p, &cur);
                fx.emit(format!("store i64 {n}, ptr {acc}"));
            });
            fx.load("i64", &acc)
        }
        Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
            if !fx.gen.is_enum(ty) {
                let fields = fx.gen.variants(ty).pop().unwrap_or_default();
                return hash_into(fx, &Ty::Tuple(fields), v, h);
            }
            let et = fx.ll(ty);
            let tp = fx.gep(&et, v, &["0".into(), "0".into()]);
            let tag = fx.load("i32", &tp);
            let tw = fx.r();
            fx.emit(format!("{tw} = zext i32 {tag} to i64"));
            let start = mix(fx, h, &tw);
            let acc = fx.alloca("i64");
            fx.emit(format!("store i64 {start}, ptr {acc}"));
            super::glue::by_variant(fx, ty, v, &mut |fx, _, pay, fields| {
                let cur = fx.load("i64", &acc);
                let n = hash_into(fx, &Ty::Tuple(fields.to_vec()), pay, &cur);
                fx.emit(format!("store i64 {n}, ptr {acc}"));
            });
            fx.load("i64", &acc)
        }
        _ => h.to_string(),
    }
}
