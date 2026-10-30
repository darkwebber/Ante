//! Tasks and sharing (§24) in native code: the `task` module's calls,
//! `panchukunna`, and the drop and copy glue of their runtime handles.
//!
//! Every `task` type is a record holding one pointer into the runtime
//! (`_h`). A `Thaalam`'s block and a `panchukunna`'s block keep the value
//! 16 bytes in, after the runtime's own header.

use super::func::Fx;
use super::glue::call_drop;
use super::{Gen, Item};
use crate::source::Span;
use crate::types::{Con, IntTy, Ty};

/// Where the value sits inside a lock or `panchukunna` block.
const VALUE_AT: u32 = 16;

fn value_ptr(fx: &mut Fx, block: &str) -> String {
    let p = fx.r();
    fx.emit(format!("{p} = getelementptr i8, ptr {block}, i64 {VALUE_AT}"));
    p
}

/// The runtime handle of a `task` record held by value.
fn handle(fx: &mut Fx, ty: &Ty, v: &str) -> String {
    let lt = fx.ll(ty);
    let h = fx.r();
    fx.emit(format!("{h} = extractvalue {lt} {v}, 0"));
    h
}

/// The runtime handle of a `task` record behind a reference.
fn handle_at(fx: &mut Fx, p: &str) -> String {
    fx.load("ptr", p)
}

fn record(fx: &mut Fx, ty: &Ty, h: &str) -> String {
    let lt = fx.ll(ty);
    let r = fx.r();
    fx.emit(format!("{r} = insertvalue {lt} undef, ptr {h}, 0"));
    r
}

fn drop_fn(fx: &mut Fx, ty: &Ty) -> String {
    if fx.needs_drop(ty) {
        fx.gen.item(Item::Drop(ty.clone()))
    } else {
        "null".into()
    }
}

fn size(fx: &mut Fx, ty: &Ty) -> u64 {
    fx.gen.size_align(ty).0
}

/// An `Atomic`'s value widened to `i64`, and back.
fn widen(fx: &mut Fx, ty: &Ty, v: &str) -> String {
    match ty {
        Ty::Int(it) => fx.widen(v, *it),
        Ty::Bool => {
            let r = fx.r();
            fx.emit(format!("{r} = zext i1 {v} to i64"));
            r
        }
        _ => v.to_string(),
    }
}

fn narrow(fx: &mut Fx, ty: &Ty, v: &str) -> String {
    match ty {
        Ty::Int(it) => fx.narrow(v, *it),
        Ty::Bool => {
            let r = fx.r();
            fx.emit(format!("{r} = trunc i64 {v} to i1"));
            r
        }
        _ => v.to_string(),
    }
}

/// `cond ? ok(a) : err(b)` for a `sare_thappu`, through memory.
fn result_select(
    fx: &mut Fx,
    ret: &Ty,
    cond: &str,
    ok: &mut dyn FnMut(&mut Fx) -> String,
    err: &mut dyn FnMut(&mut Fx) -> String,
) -> String {
    let (ok_ty, err_ty) = match ret {
        Ty::App(Con::SareThappu, a) => (a[0].clone(), a[1].clone()),
        _ => (Ty::Unit, Ty::Unit),
    };
    let lt = fx.ll(ret);
    let slot = fx.alloca(&lt);
    let (yes, no, done) = (fx.label(), fx.label(), fx.label());
    fx.emit(format!("br i1 {cond}, label %{yes}, label %{no}"));
    fx.start(&yes);
    let x = ok(fx);
    let e = fx.build_enum(ret, 0, &[x], std::slice::from_ref(&ok_ty));
    fx.emit(format!("store {lt} {e}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&no);
    let y = err(fx);
    let e = fx.build_enum(ret, 1, &[y], std::slice::from_ref(&err_ty));
    fx.emit(format!("store {lt} {e}, ptr {slot}"));
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    fx.load(&lt, &slot)
}

/// The calls of §24.2–§24.4. `None` when `key` isn't one of them.
pub(super) fn call(
    fx: &mut Fx,
    key: &str,
    vals: &[String],
    ftys: &[Ty],
    tys: &[Ty],
    ret: &Ty,
    span: Span,
) -> Option<(String, Ty)> {
    let v = |i: usize| vals.get(i).cloned().unwrap_or_else(|| "undef".into());
    let t = |i: usize| ftys.get(i).cloned().unwrap_or(Ty::Unit);
    let _ = tys;
    // `T` of a `task` type, read off the receiver or the result: method
    // type arguments aren't always spelled out.
    fn elem(ty: &Ty) -> Ty {
        match ty {
            Ty::Ref(_, inner) => elem(inner),
            Ty::App(_, a) if !a.is_empty() => a[0].clone(),
            Ty::Tuple(a) if !a.is_empty() => elem(&a[0]),
            _ => Ty::Unit,
        }
    }
    let recv_t = elem(&t(0));
    let ret_t = elem(ret);
    let unit = || ("zeroinitializer".to_string(), Ty::Unit);
    let r = match key {
        "task.kalisi" => {
            let jattu = match &t(0) {
                Ty::Fn(ps, _) => match ps.first().map(|(p, _)| p) {
                    Some(Ty::Ref(_, j)) => (**j).clone(),
                    _ => Ty::Unit,
                },
                _ => Ty::Unit,
            };
            let team = fx.r();
            fx.emit(format!("{team} = call ptr @ante_team_new()"));
            let rec = record(fx, &jattu, &team);
            let jl = fx.ll(&jattu);
            let jp = fx.spill(&jl, &rec);
            let f = v(0);
            let fp = fx.r();
            fx.emit(format!("{fp} = extractvalue %pani {f}, 0"));
            let env = fx.r();
            fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
            let rt = fx.ll(ret);
            let r = fx.r();
            fx.emit(format!("{r} = call {rt} {fp}(ptr {env}, ptr {jp})"));
            fx.emit(format!("call void @ante_team_end(ptr {team})"));
            fx.emit(format!("call void @ante_pani_dec(ptr {env})"));
            (r, ret.clone())
        }
        "Jattu.pampu" => {
            let item = ret_t.clone();
            let team = handle_at(fx, &v(0));
            let pp = fx.spill("%pani", &v(1));
            let run = fx.gen.item(Item::TaskRun(item.clone()));
            let n = size(fx, &item);
            let drop = drop_fn(fx, &item);
            let h = fx.r();
            fx.emit(format!(
                "{h} = call ptr @ante_task_spawn(ptr {team}, ptr {run}, ptr {pp}, i64 {n}, ptr {drop})"
            ));
            (record(fx, ret, &h), ret.clone())
        }
        "Jattu.anni_cancel_cheyyi" => {
            let team = handle_at(fx, &v(0));
            fx.emit(format!("call void @ante_team_cancel(ptr {team})"));
            unit()
        }
        "Token.aagu" => {
            let item = recv_t.clone();
            let h = handle(fx, &t(0), &v(0));
            let it = fx.ll(&item);
            let out = fx.alloca(&it);
            let (code, msg, wh) = (fx.alloca("%str"), fx.alloca("%str"), fx.alloca("%str"));
            let n = size(fx, &item);
            let st = fx.r();
            fx.emit(format!(
                "{st} = call i32 @ante_task_wait(ptr {h}, ptr {out}, i64 {n}, ptr {code}, ptr {msg}, ptr {wh})"
            ));
            let ok = fx.r();
            fx.emit(format!("{ok} = icmp eq i32 {st}, 0"));
            let err_ty = match ret {
                Ty::App(Con::SareThappu, a) => a[1].clone(),
                _ => Ty::Unit,
            };
            let r = result_select(fx, ret, &ok, &mut |fx| fx.load(&it, &out), &mut |fx| {
                // `TaskThappu.Kompa(TrapVivaram{…})` or `TaskThappu.Cancel`.
                let et = fx.ll(&err_ty);
                let slot = fx.alloca(&et);
                let trapped = fx.r();
                fx.emit(format!("{trapped} = icmp eq i32 {st}, 1"));
                let (a, b, done) = (fx.label(), fx.label(), fx.label());
                fx.emit(format!("br i1 {trapped}, label %{a}, label %{b}"));
                fx.start(&a);
                let info_ty =
                    fx.gen.variants(&err_ty).first().and_then(|f| f.first().cloned()).unwrap_or(Ty::Unit);
                let where_ty =
                    fx.gen.variants(&info_ty).pop().and_then(|f| f.get(2).cloned()).unwrap_or(Ty::Unit);
                let c = fx.load("%str", &code);
                let m = fx.load("%str", &msg);
                let w = fx.load("%str", &wh);
                let wn = fx.r();
                fx.emit(format!("{wn} = extractvalue %str {w}, 1"));
                let has = fx.r();
                fx.emit(format!("{has} = icmp ne i64 {wn}, 0"));
                let wv = super::native::opt_select(fx, &where_ty, &has, &mut |_| w.clone());
                let il = fx.ll(&info_ty);
                let i1 = fx.r();
                fx.emit(format!("{i1} = insertvalue {il} undef, %str {c}, 0"));
                let i2 = fx.r();
                fx.emit(format!("{i2} = insertvalue {il} {i1}, %str {m}, 1"));
                let wl = fx.ll(&where_ty);
                let i3 = fx.r();
                fx.emit(format!("{i3} = insertvalue {il} {i2}, {wl} {wv}, 2"));
                let k = fx.build_enum(&err_ty, 0, &[i3], std::slice::from_ref(&info_ty));
                fx.emit(format!("store {et} {k}, ptr {slot}"));
                fx.emit(format!("br label %{done}"));
                fx.start(&b);
                let c = fx.build_enum(&err_ty, 1, &[], &[]);
                fx.emit(format!("store {et} {c}, ptr {slot}"));
                fx.emit(format!("br label %{done}"));
                fx.start(&done);
                fx.load(&et, &slot)
            });
            (r, ret.clone())
        }
        "Token.cancel_cheyyi" => {
            let h = handle_at(fx, &v(0));
            fx.emit(format!("call void @ante_task_cancel(ptr {h})"));
            unit()
        }
        "task.nidrapo" => {
            let kl = fx.ll(&t(0));
            let ns = fx.r();
            fx.emit(format!("{ns} = extractvalue {kl} {}, 0", v(0)));
            fx.emit(format!("call void @ante_task_sleep(i64 {ns})"));
            unit()
        }
        "task.cancel_ayyindhaa" => {
            let c = fx.r();
            fx.emit(format!("{c} = call i32 @ante_task_cancelled()"));
            let b = fx.r();
            fx.emit(format!("{b} = icmp ne i32 {c}, 0"));
            (b, Ty::Bool)
        }
        "task.cpu_lu" => {
            let n = fx.r();
            fx.emit(format!("{n} = call i64 @ante_cpus()"));
            (n, Ty::ANKAM)
        }
        "task.kaluva" => {
            let item = ret_t.clone();
            let cap = v(0);
            let bad = fx.r();
            fx.emit(format!("{bad} = icmp slt i64 {cap}, 0"));
            let loc = fx.loc(span);
            let m = fx.gen.str_const("`task.kaluva` ki capacity 0 leka ekkuva undali");
            fx.guard(&bad, |fx| {
                let sp = fx.spill("%str", &m);
                fx.emit(format!("call void @ante_trap(ptr {sp}, ptr {loc})"));
            });
            let n = size(fx, &item);
            let drop = drop_fn(fx, &item);
            let h = fx.r();
            fx.emit(format!("{h} = call ptr @ante_chan_new(i64 {cap}, i64 {n}, ptr {drop})"));
            let (send_ty, recv_ty) = match ret {
                Ty::Tuple(items) if items.len() == 2 => (items[0].clone(), items[1].clone()),
                _ => (Ty::Unit, Ty::Unit),
            };
            let s = record(fx, &send_ty, &h);
            let a = record(fx, &recv_ty, &h);
            let rl = fx.ll(ret);
            let (sl, al) = (fx.ll(&send_ty), fx.ll(&recv_ty));
            let p1 = fx.r();
            fx.emit(format!("{p1} = insertvalue {rl} undef, {sl} {s}, 0"));
            let p2 = fx.r();
            fx.emit(format!("{p2} = insertvalue {rl} {p1}, {al} {a}, 1"));
            (p2, ret.clone())
        }
        "Pampey.pampu" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let it = fx.ll(&item);
            let p = fx.spill(&it, &v(1));
            let st = fx.r();
            fx.emit(format!("{st} = call i32 @ante_chan_send(ptr {h}, ptr {p})"));
            let ok = fx.r();
            fx.emit(format!("{ok} = icmp eq i32 {st}, 0"));
            let err_ty = match ret {
                Ty::App(Con::SareThappu, a) => a[1].clone(),
                _ => Ty::Unit,
            };
            let r = result_select(fx, ret, &ok, &mut |_| "zeroinitializer".into(), &mut |fx| {
                let x = fx.load(&it, &p);
                let el = fx.ll(&err_ty);
                let m = fx.r();
                fx.emit(format!("{m} = insertvalue {el} undef, {it} {x}, 0"));
                m
            });
            (r, ret.clone())
        }
        "Andhuko.andhuko" | "Andhuko.ippude_andhuko" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let it = fx.ll(&item);
            let out = fx.alloca(&it);
            let wait = i32::from(key == "Andhuko.andhuko");
            let got = fx.r();
            fx.emit(format!("{got} = call i32 @ante_chan_recv(ptr {h}, ptr {out}, i32 {wait})"));
            let has = fx.r();
            fx.emit(format!("{has} = icmp ne i32 {got}, 0"));
            let r = super::native::opt_select(fx, ret, &has, &mut |fx| fx.load(&it, &out));
            (r, ret.clone())
        }
        "Thaalam.kotthadhi" => {
            let item = ret_t.clone();
            let n = size(fx, &item);
            let b = fx.r();
            fx.emit(format!("{b} = call ptr @ante_lock_new(i64 {n})"));
            let p = value_ptr(fx, &b);
            let it = fx.ll(&item);
            fx.emit(format!("store {it} {}, ptr {p}", v(0)));
            (record(fx, ret, &b), ret.clone())
        }
        "Thaalam.theruvu" | "Thaalam.theruvu_choosi" => {
            let b = handle_at(fx, &v(0));
            let loc = fx.loc(span);
            let check = i32::from(key == "Thaalam.theruvu_choosi");
            let poisoned = fx.r();
            fx.emit(format!("{poisoned} = call i32 @ante_lock_open(ptr {b}, i32 {check}, ptr {loc})"));
            if check == 0 {
                (record(fx, ret, &b), ret.clone())
            } else {
                let (key_ty, bad_ty) = match ret {
                    Ty::App(Con::SareThappu, a) => (a[0].clone(), a[1].clone()),
                    _ => (Ty::Unit, Ty::Unit),
                };
                let ok = fx.r();
                fx.emit(format!("{ok} = icmp eq i32 {poisoned}, 0"));
                let r = result_select(fx, ret, &ok, &mut |fx| record(fx, &key_ty, &b), &mut |fx| {
                    record(fx, &bad_ty, &b)
                });
                (r, ret.clone())
            }
        }
        "Virigindhi.lopala" => {
            let b = handle(fx, &t(0), &v(0));
            fx.emit(format!("call void @ante_lock_heal(ptr {b})"));
            (record(fx, ret, &b), ret.clone())
        }
        "Atomic.kotthadhi" => {
            let item = ret_t.clone();
            let w = widen(fx, &item, &v(0));
            let h = fx.r();
            fx.emit(format!("{h} = call ptr @ante_atomic_new(i64 {w})"));
            (record(fx, ret, &h), ret.clone())
        }
        "Atomic.chadhuvu" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let x = fx.r();
            fx.emit(format!("{x} = call i64 @ante_atomic_load(ptr {h})"));
            (narrow(fx, &item, &x), item)
        }
        "Atomic.raayi" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let w = widen(fx, &item, &v(1));
            fx.emit(format!("call void @ante_atomic_store(ptr {h}, i64 {w})"));
            unit()
        }
        "Atomic.kalupu" | "Atomic.marpidi" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let w = widen(fx, &item, &v(1));
            let old = fx.r();
            if key == "Atomic.kalupu" {
                let it = match &item {
                    Ty::Int(it) => *it,
                    _ => IntTy { signed: false, bits: 1 },
                };
                fx.emit(format!(
                    "{old} = call i64 @ante_atomic_add(ptr {h}, i64 {w}, i32 {}, i32 {})",
                    it.bits,
                    i32::from(it.signed)
                ));
            } else {
                fx.emit(format!("{old} = call i64 @ante_atomic_swap(ptr {h}, i64 {w})"));
            }
            (narrow(fx, &item, &old), item)
        }
        "Atomic.ilaa_unte_maarchu" => {
            let item = recv_t.clone();
            let h = handle_at(fx, &v(0));
            let (old, new) = (v(1), v(2));
            let wo = widen(fx, &item, &old);
            let wn = widen(fx, &item, &new);
            let seen = fx.alloca("i64");
            let st = fx.r();
            fx.emit(format!("{st} = call i32 @ante_atomic_cas(ptr {h}, i64 {wo}, i64 {wn}, ptr {seen})"));
            let ok = fx.r();
            fx.emit(format!("{ok} = icmp ne i32 {st}, 0"));
            let it = item.clone();
            let r = result_select(fx, ret, &ok, &mut |_| old.clone(), &mut |fx| {
                let s = fx.load("i64", &seen);
                narrow(fx, &it, &s)
            });
            (r, ret.clone())
        }
        "task.edhi_mundhu" => {
            // `&mukka[Edhuruchoopu]`: each element is one handle.
            let s = v(0);
            let d = fx.r();
            fx.emit(format!("{d} = extractvalue %slice {s}, 0"));
            let n = fx.r();
            fx.emit(format!("{n} = extractvalue %slice {s}, 1"));
            let loc = fx.loc(span);
            let i = fx.r();
            fx.emit(format!("{i} = call i64 @ante_select(ptr {d}, i64 {n}, ptr {loc})"));
            (i, Ty::ANKAM)
        }
        "task.cancel_ready" => {
            let h = fx.r();
            fx.emit(format!("{h} = call ptr @ante_expect_cancel()"));
            (record(fx, ret, &h), ret.clone())
        }
        "samayam.timer" => {
            let kl = fx.ll(&t(0));
            let ns = fx.r();
            fx.emit(format!("{ns} = extractvalue {kl} {}, 0", v(0)));
            let h = fx.r();
            fx.emit(format!("{h} = call ptr @ante_expect_timer(i64 {ns})"));
            (record(fx, ret, &h), ret.clone())
        }
        "Token.ready" | "Andhuko.andhuko_ready" => {
            let target = handle_at(fx, &v(0));
            let f = if key == "Token.ready" { "ante_expect_task" } else { "ante_expect_chan" };
            let h = fx.r();
            fx.emit(format!("{h} = call ptr @{f}(ptr {target})"));
            (record(fx, ret, &h), ret.clone())
        }
        "panchukunna.kotthadhi" => {
            let item = ret_t.clone();
            let n = size(fx, &item);
            let b = fx.r();
            fx.emit(format!("{b} = call ptr @ante_rc_new(i64 {n})"));
            let p = value_ptr(fx, &b);
            let it = fx.ll(&item);
            fx.emit(format!("store {it} {}, ptr {p}", v(0)));
            (b, ret.clone())
        }
        "panchukunna.okkatenaa" => {
            let a = fx.load("ptr", &v(0));
            let b = fx.load("ptr", &v(1));
            let r = fx.r();
            fx.emit(format!("{r} = icmp eq ptr {a}, {b}"));
            (r, Ty::Bool)
        }
        _ => return None,
    };
    Some(r)
}

/// `void run(ptr pani, ptr out)`: runs a task's closure and stores its value.
pub(super) fn run_fn(gen: &mut Gen, name: &str, item: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let f = fx.load("%pani", "%p");
    let fp = fx.r();
    fx.emit(format!("{fp} = extractvalue %pani {f}, 0"));
    let env = fx.r();
    fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
    let rt = fx.ll(item);
    let r = fx.r();
    fx.emit(format!("{r} = call {rt} {fp}(ptr {env})"));
    fx.emit(format!("store {rt} {r}, ptr %out"));
    format!("define internal void {name}(ptr %p, ptr %out) {{\nentry:\n{}  ret void\n}}\n", fx.code())
}

/// Drops a runtime handle or a `panchukunna`; `false` if `ty` isn't one.
pub(super) fn drop(fx: &mut Fx, ty: &Ty, v: &str) -> bool {
    match ty {
        Ty::App(Con::Panchukunna, a) => {
            let b = fx.load("ptr", v);
            let last = fx.r();
            fx.emit(format!("{last} = call i32 @ante_rc_dec(ptr {b})"));
            let is_last = fx.r();
            fx.emit(format!("{is_last} = icmp ne i32 {last}, 0"));
            let inner = a[0].clone();
            let (free, done) = (fx.label(), fx.label());
            fx.emit(format!("br i1 {is_last}, label %{free}, label %{done}"));
            fx.start(&free);
            let p = value_ptr(fx, &b);
            call_drop(fx, &inner, &p);
            fx.emit(format!("call void @ante_rc_free(ptr {b})"));
            fx.emit(format!("br label %{done}"));
            fx.start(&done);
            true
        }
        Ty::App(Con::Adt(d), args) => {
            let Some(name) = fx.gen.g.task_type(*d) else { return false };
            let f = match name {
                "Token" => "ante_task_drop",
                "Key" | "Virigindhi" => "ante_lock_close",
                "Atomic" => "ante_atomic_free",
                "Pampey" => "ante_chan_sender_drop",
                "Andhuko" => "ante_chan_receiver_drop",
                "Thaalam" => "ante_lock_free",
                "Edhuruchoopu" => "ante_expect_free",
                "Jattu" => return true,
                _ => return false,
            };
            let h = fx.load("ptr", v);
            if name == "Thaalam" {
                let inner = args.first().cloned().unwrap_or(Ty::Unit);
                let p = value_ptr(fx, &h);
                call_drop(fx, &inner, &p);
            }
            fx.emit(format!("call void @{f}(ptr {h})"));
            true
        }
        _ => false,
    }
}

/// `.xerox()` of a `Pampey` or a `panchukunna`: another handle, counted.
pub(super) fn xerox(fx: &mut Fx, ty: &Ty, v: &str) -> Option<String> {
    match ty {
        Ty::App(Con::Panchukunna, _) => {
            let b = fx.load("ptr", v);
            fx.emit(format!("call void @ante_rc_inc(ptr {b})"));
            Some(b)
        }
        Ty::App(Con::Adt(d), _) if fx.gen.g.task_type(*d) == Some("Pampey") => {
            let lt = fx.ll(ty);
            let rec = fx.load(&lt, v);
            let h = handle(fx, ty, &rec);
            fx.emit(format!("call void @ante_chan_sender_copy(ptr {h})"));
            Some(rec)
        }
        _ => None,
    }
}

/// The value inside a `panchukunna` or a lock's `Key`, for display and `==`.
pub(super) fn inner_ptr(fx: &mut Fx, ty: &Ty, v: &str) -> Option<(String, Ty)> {
    match ty {
        Ty::App(Con::Panchukunna, a) => {
            let b = fx.load("ptr", v);
            Some((value_ptr(fx, &b), a[0].clone()))
        }
        _ => None,
    }
}

/// Shown and compared like the value inside.
pub(super) fn show(fx: &mut Fx, ty: &Ty, buf: &str, v: &str) -> bool {
    let Some((p, inner)) = inner_ptr(fx, ty, v) else { return false };
    let f = fx.gen.item(Item::Show(inner));
    fx.emit(format!("call void {f}(ptr {buf}, ptr {p})"));
    true
}

/// A value at `VALUE_AT` in `block`, for code outside this module.
pub(super) fn smart_value_ptr(fx: &mut Fx, block: &str) -> String {
    value_ptr(fx, block)
}
