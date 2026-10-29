//! Functions generated per type: display (§13.5 built-in `padham_ga`),
//! equality, ordering, `xerox`, drop glue (§19.3), sort callbacks and
//! thunks for named functions used as `pani` values.

use super::func::Fx;
use super::{Gen, Item};
use crate::mir::Resolved;
use crate::typeck::{AdtKind, FnId};
use crate::types::{Con, Ty};

/// The implementation of a standard trait method for `ty`: a body, or
/// `None` for the built-in behavior.
pub(super) fn user_impl(gen: &Gen, trait_name: &str, ty: &Ty) -> Option<(crate::mir::BodyId, Vec<Ty>)> {
    let def = gen.g.std_trait(trait_name)?;
    let fid = gen.g.traits.get(&def)?.methods.first()?.1;
    match gen.resolve(fid, std::slice::from_ref(ty))? {
        Resolved::Body(b, tys) => Some((b, tys)),
        Resolved::Native(_) => None,
    }
}

/// Element data pointer and length of a sequence value at `v`.
pub(super) fn seq_at(fx: &mut Fx, ty: &Ty, v: &str) -> Option<(String, String, Ty)> {
    match ty {
        Ty::Array(e, n) => Some((v.to_string(), n.unwrap_or(0).to_string(), (**e).clone())),
        Ty::App(Con::Varusa, a) => {
            let dp = fx.gep("%vec", v, &["0".into(), "0".into()]);
            let data = fx.load("ptr", &dp);
            let lp = fx.gep("%vec", v, &["0".into(), "1".into()]);
            let len = fx.load("i64", &lp);
            Some((data, len, a[0].clone()))
        }
        Ty::Ref(_, inner) => match &**inner {
            Ty::App(Con::Mukka, a) => {
                let s = fx.load("%slice", v);
                let data = fx.r();
                fx.emit(format!("{data} = extractvalue %slice {s}, 0"));
                let len = fx.r();
                fx.emit(format!("{len} = extractvalue %slice {s}, 1"));
                Some((data, len, a[0].clone()))
            }
            _ => None,
        },
        _ => None,
    }
}

/// `for i in 0..len { body(elem_ptr) }`; `body` may jump to `exit`.
pub(super) fn each(fx: &mut Fx, data: &str, len: &str, elem: &Ty, body: &mut dyn FnMut(&mut Fx, &str, &str)) {
    let et = fx.ll(elem);
    let i = fx.alloca("i64");
    fx.emit(format!("store i64 0, ptr {i}"));
    let head = fx.label();
    let step = fx.label();
    let done = fx.label();
    fx.emit(format!("br label %{head}"));
    fx.start(&head);
    let iv = fx.load("i64", &i);
    let more = fx.r();
    fx.emit(format!("{more} = icmp ult i64 {iv}, {len}"));
    let inner = fx.label();
    fx.emit(format!("br i1 {more}, label %{inner}, label %{done}"));
    fx.start(&inner);
    let p = fx.gep(&et, data, &[format!("i64 {iv}")]);
    body(fx, &p, &done);
    fx.emit(format!("br label %{step}"));
    fx.start(&step);
    let next = fx.r();
    fx.emit(format!("{next} = add i64 {iv}, 1"));
    fx.emit(format!("store i64 {next}, ptr {i}"));
    fx.emit(format!("br label %{head}"));
    fx.start(&done);
}

pub(super) fn push_text(fx: &mut Fx, buf: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let (p, n) = fx.gen.str_ptr(text);
    fx.emit(format!("call void @ante_buf_push(ptr {buf}, ptr {p}, i64 {n})"));
}

fn field_ptrs(fx: &mut Fx, st: &Ty, base: &str, n: usize) -> Vec<String> {
    let lt = fx.ll(st);
    (0..n).map(|i| fx.gep(&lt, base, &["0".into(), i.to_string()])).collect()
}

/// Calls `f(variant index, payload pointer, field types)` for each variant
/// of the enum at `v`, after a switch on its tag.
/// Called per variant with its index, payload pointer and field types.
pub(super) type VariantFn<'f> = dyn FnMut(&mut Fx, u32, &str, &[Ty]) + 'f;

pub(super) fn by_variant(fx: &mut Fx, ty: &Ty, v: &str, f: &mut VariantFn) {
    let et = fx.ll(ty);
    let tp = fx.gep(&et, v, &["0".into(), "0".into()]);
    let tag = fx.load("i32", &tp);
    let pay = fx.gep(&et, v, &["0".into(), "1".into()]);
    let variants = fx.gen.variants(ty);
    let done = fx.label();
    let labels: Vec<String> = variants.iter().map(|_| fx.label()).collect();
    let cases: Vec<String> = labels.iter().enumerate().map(|(i, l)| format!("i32 {i}, label %{l}")).collect();
    fx.emit(format!("switch i32 {tag}, label %{done} [ {} ]", cases.join(" ")));
    for (i, (l, fields)) in labels.iter().zip(&variants).enumerate() {
        fx.start(l);
        f(fx, i as u32, &pay, fields);
        fx.emit(format!("br label %{done}"));
    }
    fx.start(&done);
}

// ── display ────────────────────────────────────────────────────────────────

pub(super) fn show(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    show_into(&mut fx, ty, "%buf", "%v");
    format!("define internal void {name}(ptr %buf, ptr %v) {{\nentry:\n{}  ret void\n}}\n", fx.code())
}

fn show_call(fx: &mut Fx, ty: &Ty, buf: &str, v: &str) {
    let f = fx.gen.item(Item::Show(ty.clone()));
    fx.emit(format!("call void {f}(ptr {buf}, ptr {v})"));
}

fn show_into(fx: &mut Fx, ty: &Ty, buf: &str, v: &str) {
    match ty {
        Ty::Int(it) => {
            let x = fx.load(&format!("i{}", it.bits), v);
            let w = fx.widen(&x, *it);
            let f = if it.signed { "ante_buf_i64" } else { "ante_buf_u64" };
            fx.emit(format!("call void @{f}(ptr {buf}, i64 {w})"));
        }
        Ty::Float(32) => {
            let x = fx.load("float", v);
            fx.emit(format!("call void @ante_buf_f32(ptr {buf}, float {x})"));
        }
        Ty::Float(_) => {
            let x = fx.load("double", v);
            fx.emit(format!("call void @ante_buf_f64(ptr {buf}, double {x})"));
        }
        Ty::Bool => {
            let x = fx.load("i1", v);
            let yes = fx.label();
            let no = fx.label();
            let done = fx.label();
            fx.emit(format!("br i1 {x}, label %{yes}, label %{no}"));
            fx.start(&yes);
            push_text(fx, buf, "avunu");
            fx.emit(format!("br label %{done}"));
            fx.start(&no);
            push_text(fx, buf, "kaadhu");
            fx.emit(format!("br label %{done}"));
            fx.start(&done);
        }
        Ty::Char => {
            let x = fx.load("i32", v);
            fx.emit(format!("call void @ante_buf_char(ptr {buf}, i32 {x})"));
        }
        Ty::Str => {
            let dp = fx.gep("%str", v, &["0".into(), "0".into()]);
            let data = fx.load("ptr", &dp);
            let lp = fx.gep("%str", v, &["0".into(), "1".into()]);
            let len = fx.load("i64", &lp);
            fx.emit(format!("call void @ante_buf_push(ptr {buf}, ptr {data}, i64 {len})"));
        }
        Ty::Unit | Ty::Never => push_text(fx, buf, "()"),
        Ty::Tuple(items) => {
            push_text(fx, buf, "(");
            let ptrs = field_ptrs(fx, ty, v, items.len());
            for (i, (p, t)) in ptrs.iter().zip(items).enumerate() {
                if i > 0 {
                    push_text(fx, buf, ", ");
                }
                show_call(fx, t, buf, p);
            }
            push_text(fx, buf, if items.len() == 1 { ",)" } else { ")" });
        }
        Ty::Array(..) | Ty::App(Con::Varusa, _) => show_seq(fx, ty, buf, v),
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) => {
            if matches!(**inner, Ty::App(Con::Mukka, _)) {
                show_seq(fx, ty, buf, v);
            } else {
                let p = fx.load("ptr", v);
                show_call(fx, inner, buf, &p);
            }
        }
        Ty::App(Con::Pette, a) => {
            let p = fx.load("ptr", v);
            show_call(fx, &a[0], buf, &p);
        }
        Ty::App(Con::Paridhi, a) => {
            let st = Ty::Tuple(vec![a[0].clone(), a[0].clone(), Ty::Bool]);
            let ptrs = field_ptrs(fx, &st, v, 3);
            show_call(fx, &a[0], buf, &ptrs[0]);
            push_text(fx, buf, " nunchi ");
            show_call(fx, &a[0], buf, &ptrs[1]);
            let inc = fx.load("i1", &ptrs[2]);
            let yes = fx.label();
            let no = fx.label();
            let done = fx.label();
            fx.emit(format!("br i1 {inc}, label %{yes}, label %{no}"));
            fx.start(&yes);
            push_text(fx, buf, " varaku");
            fx.emit(format!("br label %{done}"));
            fx.start(&no);
            push_text(fx, buf, " lopu");
            fx.emit(format!("br label %{done}"));
            fx.start(&done);
        }
        Ty::App(Con::Undochu | Con::SareThappu, _) => {
            let names: [&str; 2] =
                if matches!(ty, Ty::App(Con::Undochu, _)) { ["undhi", "ledhu"] } else { ["sare", "thappu"] };
            by_variant(fx, ty, v, &mut |fx, i, pay, fields| {
                push_text(fx, buf, names[i as usize]);
                if let Some(t) = fields.first() {
                    push_text(fx, buf, "(");
                    let st = Ty::Tuple(fields.to_vec());
                    let p = field_ptrs(fx, &st, pay, 1);
                    show_call(fx, t, buf, &p[0]);
                    push_text(fx, buf, ")");
                }
            });
        }
        Ty::App(Con::Adt(d), args) => {
            if let Some((b, tys)) = user_impl(fx.gen, "Choopinchochu", ty) {
                let (s, _) = fx.call_body_vals(b, &tys, &[v.to_string()]);
                let sp = fx.spill("%str", &s);
                show_into(fx, &Ty::Str, buf, &sp);
                // The text the user's `padham_ga` returned is ours to free.
                fx.emit(format!("call void @ante_buf_free(ptr {sp})"));
                return;
            }
            let Some(adt) = fx.gen.g.adts.get(d).cloned() else { return };
            match &adt.kind {
                AdtKind::Record(fields) => {
                    let ftys: Vec<Ty> = fields.iter().map(|f| f.ty.subst(args)).collect();
                    let st = Ty::Tuple(ftys.clone());
                    let ptrs = field_ptrs(fx, &st, v, ftys.len());
                    if adt.file.is_none() && ftys.len() == 1 {
                        show_call(fx, &ftys[0], buf, &ptrs[0]);
                        return;
                    }
                    push_text(fx, buf, &format!("{}{{", adt.name));
                    for (i, ((f, p), t)) in fields.iter().zip(&ptrs).zip(&ftys).enumerate() {
                        if i > 0 {
                            push_text(fx, buf, ", ");
                        }
                        push_text(fx, buf, &format!("{}: ", f.name));
                        show_call(fx, t, buf, p);
                    }
                    push_text(fx, buf, "}");
                }
                AdtKind::Enum(variants) => {
                    let vnames: Vec<String> = variants.iter().map(|x| x.name.clone()).collect();
                    by_variant(fx, ty, v, &mut |fx, i, pay, fields| {
                        push_text(fx, buf, &format!("{}.{}", adt.name, vnames[i as usize]));
                        if !fields.is_empty() {
                            push_text(fx, buf, "(");
                            let st = Ty::Tuple(fields.to_vec());
                            let ptrs = field_ptrs(fx, &st, pay, fields.len());
                            for (k, (p, t)) in ptrs.iter().zip(fields).enumerate() {
                                if k > 0 {
                                    push_text(fx, buf, ", ");
                                }
                                show_call(fx, t, buf, p);
                            }
                            push_text(fx, buf, ")");
                        }
                    });
                }
            }
        }
        Ty::Fn(..) => push_text(fx, buf, "<pani>"),
        Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => super::map::show_map(fx, ty, buf, v),
        _ => push_text(fx, buf, "?"),
    }
}

fn show_seq(fx: &mut Fx, ty: &Ty, buf: &str, v: &str) {
    let Some((data, len, elem)) = seq_at(fx, ty, v) else { return };
    push_text(fx, buf, "[");
    each(fx, &data, &len, &elem, &mut |fx, p, _| {
        // A separator before every element but the first.
        let first = fx.r();
        fx.emit(format!("{first} = icmp eq ptr {p}, {data}"));
        let sep = fx.label();
        let item = fx.label();
        fx.emit(format!("br i1 {first}, label %{item}, label %{sep}"));
        fx.start(&sep);
        push_text(fx, buf, ", ");
        fx.emit(format!("br label %{item}"));
        fx.start(&item);
        show_call(fx, &elem, buf, p);
    });
    push_text(fx, buf, "]");
}

// ── equality and order ─────────────────────────────────────────────────────

pub(super) fn eq(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let res = fx.alloca("i1");
    fx.emit(format!("store i1 true, ptr {res}"));
    let fail = fx.label();
    eq_into(&mut fx, ty, "%a", "%b", &res, &fail);
    let end = fx.label();
    fx.emit(format!("br label %{end}"));
    fx.start(&fail);
    fx.emit(format!("store i1 false, ptr {res}"));
    fx.emit(format!("br label %{end}"));
    fx.start(&end);
    let r = fx.load("i1", &res);
    format!("define internal i1 {name}(ptr %a, ptr %b) {{\nentry:\n{}  ret i1 {r}\n}}\n", fx.code())
}

/// Compares `a` and `b`, jumping to `fail` when they differ.
fn eq_into(fx: &mut Fx, ty: &Ty, a: &str, b: &str, _res: &str, fail: &str) {
    let differ = |fx: &mut Fx, c: &str| {
        let ok = fx.label();
        fx.emit(format!("br i1 {c}, label %{ok}, label %{fail}"));
        fx.start(&ok);
    };
    match ty {
        Ty::Int(_) | Ty::Bool | Ty::Char | Ty::Float(_) => {
            let lt = fx.ll(ty);
            let x = fx.load(&lt, a);
            let y = fx.load(&lt, b);
            let c = fx.r();
            if matches!(ty, Ty::Float(_)) {
                fx.emit(format!("{c} = fcmp oeq {lt} {x}, {y}"));
            } else {
                fx.emit(format!("{c} = icmp eq {lt} {x}, {y}"));
            }
            differ(fx, &c);
        }
        Ty::Str => {
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @ante_str_cmp(ptr {a}, ptr {b})"));
            let c = fx.r();
            fx.emit(format!("{c} = icmp eq i32 {r}, 0"));
            differ(fx, &c);
        }
        Ty::Unit | Ty::Never => {}
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) if !matches!(**inner, Ty::App(Con::Mukka, _)) => {
            let pa = fx.load("ptr", a);
            let pb = fx.load("ptr", b);
            let c = call_eq(fx, inner, &pa, &pb);
            differ(fx, &c);
        }
        Ty::App(Con::Pette, x) => {
            let pa = fx.load("ptr", a);
            let pb = fx.load("ptr", b);
            let c = call_eq(fx, &x[0], &pa, &pb);
            differ(fx, &c);
        }
        Ty::Tuple(items) => {
            let pa = field_ptrs(fx, ty, a, items.len());
            let pb = field_ptrs(fx, ty, b, items.len());
            for ((x, y), t) in pa.iter().zip(&pb).zip(items) {
                let c = call_eq(fx, t, x, y);
                differ(fx, &c);
            }
        }
        Ty::Array(..) | Ty::App(Con::Varusa, _) | Ty::Ref(..) => {
            let (Some((da, la, elem)), Some((db, lb, _))) = (seq_at(fx, ty, a), seq_at(fx, ty, b)) else {
                return;
            };
            let c = fx.r();
            fx.emit(format!("{c} = icmp eq i64 {la}, {lb}"));
            differ(fx, &c);
            let et = fx.ll(&elem);
            each(fx, &da, &la, &elem, &mut |fx, pa, _| {
                let idx = fx.r();
                fx.emit(format!("{idx} = ptrtoint ptr {pa} to i64"));
                let base = fx.r();
                fx.emit(format!("{base} = ptrtoint ptr {da} to i64"));
                let off = fx.r();
                fx.emit(format!("{off} = sub i64 {idx}, {base}"));
                let pb = fx.r();
                fx.emit(format!("{pb} = getelementptr i8, ptr {db}, i64 {off}"));
                let _ = &et;
                let c = call_eq(fx, &elem, pa, &pb);
                let ok = fx.label();
                fx.emit(format!("br i1 {c}, label %{ok}, label %{fail}"));
                fx.start(&ok);
            });
        }
        Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
            if let Some((bd, tys)) = user_impl(fx.gen, "Polchochu", ty) {
                let (c, _) = fx.call_body_vals(bd, &tys, &[a.to_string(), b.to_string()]);
                differ(fx, &c);
                return;
            }
            if !fx.gen.is_enum(ty) {
                let fields = fx.gen.variants(ty).pop().unwrap_or_default();
                let st = Ty::Tuple(fields.clone());
                let pa = field_ptrs(fx, &st, a, fields.len());
                let pb = field_ptrs(fx, &st, b, fields.len());
                for ((x, y), t) in pa.iter().zip(&pb).zip(&fields) {
                    let c = call_eq(fx, t, x, y);
                    differ(fx, &c);
                }
                return;
            }
            let et = fx.ll(ty);
            let ta = fx.gep(&et, a, &["0".into(), "0".into()]);
            let tb = fx.gep(&et, b, &["0".into(), "0".into()]);
            let x = fx.load("i32", &ta);
            let y = fx.load("i32", &tb);
            let c = fx.r();
            fx.emit(format!("{c} = icmp eq i32 {x}, {y}"));
            differ(fx, &c);
            let payb = fx.gep(&et, b, &["0".into(), "1".into()]);
            by_variant(fx, ty, a, &mut |fx, _, pay, fields| {
                let st = Ty::Tuple(fields.to_vec());
                let pa = field_ptrs(fx, &st, pay, fields.len());
                let pb = field_ptrs(fx, &st, &payb, fields.len());
                for ((x, y), t) in pa.iter().zip(&pb).zip(fields) {
                    let c = call_eq(fx, t, x, y);
                    let ok = fx.label();
                    fx.emit(format!("br i1 {c}, label %{ok}, label %{fail}"));
                    fx.start(&ok);
                }
            });
        }
        Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => super::map::eq_map(fx, ty, a, b, fail),
        _ => {}
    }
}

fn call_eq(fx: &mut Fx, ty: &Ty, a: &str, b: &str) -> String {
    let f = fx.gen.item(Item::Eq(ty.clone()));
    let c = fx.r();
    fx.emit(format!("{c} = call i1 {f}(ptr {a}, ptr {b})"));
    c
}

pub(super) fn call_cmp(fx: &mut Fx, ty: &Ty, a: &str, b: &str) -> String {
    let f = fx.gen.item(Item::Cmp(ty.clone()));
    let c = fx.r();
    fx.emit(format!("{c} = call i32 {f}(ptr {a}, ptr {b})"));
    c
}

pub(super) fn eq_call(fx: &mut Fx, ty: &Ty, a: &str, b: &str) -> String {
    call_eq(fx, ty, a, b)
}

pub(super) fn cmp(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let res = fx.alloca("i32");
    fx.emit(format!("store i32 0, ptr {res}"));
    let done = fx.label();
    cmp_into(&mut fx, ty, "%a", "%b", &res, &done);
    fx.emit(format!("br label %{done}"));
    fx.start(&done);
    let r = fx.load("i32", &res);
    format!("define internal i32 {name}(ptr %a, ptr %b) {{\nentry:\n{}  ret i32 {r}\n}}\n", fx.code())
}

/// Writes -1/0/1 to `res`; jumps to `done` as soon as the order is known.
fn cmp_into(fx: &mut Fx, ty: &Ty, a: &str, b: &str, res: &str, done: &str) {
    // Stores `c` and finishes if it is not 0.
    let decide = |fx: &mut Fx, c: &str| {
        fx.emit(format!("store i32 {c}, ptr {res}"));
        let z = fx.r();
        fx.emit(format!("{z} = icmp ne i32 {c}, 0"));
        let more = fx.label();
        fx.emit(format!("br i1 {z}, label %{done}, label %{more}"));
        fx.start(&more);
    };
    let scalar = |fx: &mut Fx, lt: &str, lt_op: &str, gt_op: &str, x: &str, y: &str| {
        let lt_c = fx.r();
        fx.emit(format!("{lt_c} = {lt_op} {lt} {x}, {y}"));
        let gt_c = fx.r();
        fx.emit(format!("{gt_c} = {gt_op} {lt} {x}, {y}"));
        let g1 = fx.r();
        fx.emit(format!("{g1} = select i1 {gt_c}, i32 1, i32 0"));
        let r = fx.r();
        fx.emit(format!("{r} = select i1 {lt_c}, i32 -1, i32 {g1}"));
        r
    };
    match ty {
        Ty::Int(it) => {
            let lt = fx.ll(ty);
            let x = fx.load(&lt, a);
            let y = fx.load(&lt, b);
            let (l, g) = if it.signed { ("icmp slt", "icmp sgt") } else { ("icmp ult", "icmp ugt") };
            let r = scalar(fx, &lt, l, g, &x, &y);
            decide(fx, &r);
        }
        Ty::Bool | Ty::Char => {
            let lt = fx.ll(ty);
            let x = fx.load(&lt, a);
            let y = fx.load(&lt, b);
            let r = scalar(fx, &lt, "icmp ult", "icmp ugt", &x, &y);
            decide(fx, &r);
        }
        Ty::Float(_) => {
            let lt = fx.ll(ty);
            let x = fx.load(&lt, a);
            let y = fx.load(&lt, b);
            let r = scalar(fx, &lt, "fcmp olt", "fcmp ogt", &x, &y);
            decide(fx, &r);
        }
        Ty::Str => {
            let r = fx.r();
            fx.emit(format!("{r} = call i32 @ante_str_cmp(ptr {a}, ptr {b})"));
            decide(fx, &r);
        }
        Ty::Ref(_, inner) | Ty::Ptr(_, inner) if !matches!(**inner, Ty::App(Con::Mukka, _)) => {
            let pa = fx.load("ptr", a);
            let pb = fx.load("ptr", b);
            let r = call_cmp(fx, inner, &pa, &pb);
            decide(fx, &r);
        }
        Ty::App(Con::Pette, x) => {
            let pa = fx.load("ptr", a);
            let pb = fx.load("ptr", b);
            let r = call_cmp(fx, &x[0], &pa, &pb);
            decide(fx, &r);
        }
        Ty::Tuple(items) => {
            let pa = field_ptrs(fx, ty, a, items.len());
            let pb = field_ptrs(fx, ty, b, items.len());
            for ((x, y), t) in pa.iter().zip(&pb).zip(items) {
                let r = call_cmp(fx, t, x, y);
                decide(fx, &r);
            }
        }
        Ty::Array(..) | Ty::App(Con::Varusa, _) | Ty::Ref(..) => {
            let (Some((da, la, elem)), Some((db, lb, _))) = (seq_at(fx, ty, a), seq_at(fx, ty, b)) else {
                return;
            };
            let shorter = fx.r();
            fx.emit(format!("{shorter} = icmp ult i64 {la}, {lb}"));
            let n = fx.r();
            fx.emit(format!("{n} = select i1 {shorter}, i64 {la}, i64 {lb}"));
            each(fx, &da, &n, &elem, &mut |fx, pa, _| {
                let idx = fx.r();
                fx.emit(format!("{idx} = ptrtoint ptr {pa} to i64"));
                let base = fx.r();
                fx.emit(format!("{base} = ptrtoint ptr {da} to i64"));
                let off = fx.r();
                fx.emit(format!("{off} = sub i64 {idx}, {base}"));
                let pb = fx.r();
                fx.emit(format!("{pb} = getelementptr i8, ptr {db}, i64 {off}"));
                let r = call_cmp(fx, &elem, pa, &pb);
                decide(fx, &r);
            });
            let r = scalar(fx, "i64", "icmp ult", "icmp ugt", &la, &lb);
            decide(fx, &r);
        }
        Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
            if let Some((bd, tys)) = user_impl(fx.gen, "VarusaPettochu", ty) {
                let (k, kt) = fx.call_body_vals(bd, &tys, &[a.to_string(), b.to_string()]);
                let kl = fx.ll(&kt);
                let slot = fx.spill(&kl, &k);
                let tp = fx.gep(&kl, &slot, &["0".into(), "0".into()]);
                let tag = fx.load("i32", &tp);
                let r = fx.r();
                fx.emit(format!("{r} = sub i32 {tag}, 1"));
                decide(fx, &r);
                return;
            }
            if !fx.gen.is_enum(ty) {
                let fields = fx.gen.variants(ty).pop().unwrap_or_default();
                let st = Ty::Tuple(fields.clone());
                let pa = field_ptrs(fx, &st, a, fields.len());
                let pb = field_ptrs(fx, &st, b, fields.len());
                for ((x, y), t) in pa.iter().zip(&pb).zip(&fields) {
                    let r = call_cmp(fx, t, x, y);
                    decide(fx, &r);
                }
                return;
            }
            let et = fx.ll(ty);
            let ta = fx.gep(&et, a, &["0".into(), "0".into()]);
            let tb = fx.gep(&et, b, &["0".into(), "0".into()]);
            let x = fx.load("i32", &ta);
            let y = fx.load("i32", &tb);
            let r = scalar(fx, "i32", "icmp ult", "icmp ugt", &x, &y);
            decide(fx, &r);
            let payb = fx.gep(&et, b, &["0".into(), "1".into()]);
            by_variant(fx, ty, a, &mut |fx, _, pay, fields| {
                let st = Ty::Tuple(fields.to_vec());
                let pa = field_ptrs(fx, &st, pay, fields.len());
                let pb = field_ptrs(fx, &st, &payb, fields.len());
                for ((x, y), t) in pa.iter().zip(&pb).zip(fields) {
                    let r = call_cmp(fx, t, x, y);
                    decide(fx, &r);
                }
            });
        }
        _ => {}
    }
}

// ── xerox (deep copy) ──────────────────────────────────────────────────────

pub(super) fn xerox(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let lt = fx.ll(ty);
    let v = xerox_value(&mut fx, ty, "%v");
    format!("define internal {lt} {name}(ptr %v) {{\nentry:\n{}  ret {lt} {v}\n}}\n", fx.code())
}

pub(super) fn call_xerox(fx: &mut Fx, ty: &Ty, v: &str) -> String {
    let f = fx.gen.item(Item::Xerox(ty.clone()));
    let lt = fx.ll(ty);
    let r = fx.r();
    fx.emit(format!("{r} = call {lt} {f}(ptr {v})"));
    r
}

fn xerox_value(fx: &mut Fx, ty: &Ty, v: &str) -> String {
    let lt = fx.ll(ty);
    if let Ty::Fn(..) = ty {
        let f = fx.load("%pani", v);
        let env = fx.r();
        fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
        fx.emit(format!("call void @ante_pani_inc(ptr {env})"));
        return f;
    }
    if let Ty::App(Con::Adt(_), _) = ty {
        if let Some((bd, tys)) = user_impl(fx.gen, "XeroxTheeyochu", ty) {
            return fx.call_body_vals(bd, &tys, &[v.to_string()]).0;
        }
    }
    match ty {
        Ty::Str => {
            let out = fx.alloca("%str");
            fx.emit(format!("call void @ante_str_copy(ptr {out}, ptr {v})"));
            fx.load("%str", &out)
        }
        Ty::App(Con::Varusa, a) => {
            let elem = a[0].clone();
            let Some((data, len, _)) = seq_at(fx, ty, v) else { return "zeroinitializer".into() };
            let (size, _) = fx.gen.size_align(&elem);
            let bytes = fx.r();
            fx.emit(format!("{bytes} = mul i64 {len}, {size}"));
            let nd = fx.r();
            fx.emit(format!("{nd} = call ptr @ante_vec_alloc(i64 {len}, i64 {bytes})"));
            let et = fx.ll(&elem);
            each(fx, &data, &len, &elem, &mut |fx, p, _| {
                let c = call_xerox(fx, &elem, p);
                let idx = fx.r();
                fx.emit(format!("{idx} = ptrtoint ptr {p} to i64"));
                let base = fx.r();
                fx.emit(format!("{base} = ptrtoint ptr {data} to i64"));
                let off = fx.r();
                fx.emit(format!("{off} = sub i64 {idx}, {base}"));
                let q = fx.r();
                fx.emit(format!("{q} = getelementptr i8, ptr {nd}, i64 {off}"));
                fx.emit(format!("store {et} {c}, ptr {q}"));
            });
            let a1 = fx.r();
            fx.emit(format!("{a1} = insertvalue %vec undef, ptr {nd}, 0"));
            let a2 = fx.r();
            fx.emit(format!("{a2} = insertvalue %vec {a1}, i64 {len}, 1"));
            let a3 = fx.r();
            fx.emit(format!("{a3} = insertvalue %vec {a2}, i64 {len}, 2"));
            a3
        }
        Ty::App(Con::Pette, a) => {
            let p = fx.load("ptr", v);
            let inner = call_xerox(fx, &a[0], &p);
            fx.boxed(&a[0], &inner)
        }
        Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => super::map::xerox_map(fx, ty, v),
        Ty::Tuple(items) => {
            let ptrs = field_ptrs(fx, ty, v, items.len());
            let vals: Vec<String> = ptrs.iter().zip(items).map(|(p, t)| call_xerox(fx, t, p)).collect();
            fx.build_struct_vals(&lt, &vals, items)
        }
        Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) if needs_deep(fx.gen, ty) => {
            if !fx.gen.is_enum(ty) {
                let fields = fx.gen.variants(ty).pop().unwrap_or_default();
                let st = Ty::Tuple(fields.clone());
                let ptrs = field_ptrs(fx, &st, v, fields.len());
                let vals: Vec<String> = ptrs.iter().zip(&fields).map(|(p, t)| call_xerox(fx, t, p)).collect();
                return fx.build_struct_vals(&lt, &vals, &fields);
            }
            let out = fx.alloca(&lt);
            let whole = fx.load(&lt, v);
            fx.emit(format!("store {lt} {whole}, ptr {out}"));
            let et = lt.clone();
            let out_pay = fx.gep(&et, &out, &["0".into(), "1".into()]);
            by_variant(fx, ty, v, &mut |fx, _, pay, fields| {
                let st = Ty::Tuple(fields.to_vec());
                let src = field_ptrs(fx, &st, pay, fields.len());
                let dst = field_ptrs(fx, &st, &out_pay, fields.len());
                for ((s, d), t) in src.iter().zip(&dst).zip(fields) {
                    let c = call_xerox(fx, t, s);
                    let ft = fx.ll(t);
                    fx.emit(format!("store {ft} {c}, ptr {d}"));
                }
            });
            fx.load(&lt, &out)
        }
        _ => fx.load(&lt, v),
    }
}

/// Whether copying a value takes more than copying its bytes.
fn needs_deep(gen: &Gen, ty: &Ty) -> bool {
    fn go(gen: &Gen, ty: &Ty, depth: u32) -> bool {
        if depth > 8 {
            return true;
        }
        match ty {
            Ty::Str
            | Ty::Fn(..)
            | Ty::App(Con::Varusa | Con::Pette | Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => true,
            Ty::Tuple(items) => items.iter().any(|t| go(gen, t, depth + 1)),
            Ty::Array(e, _) => go(gen, e, depth + 1),
            Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
                gen.variants(ty).iter().flatten().any(|t| go(gen, t, depth + 1))
            }
            _ => false,
        }
    }
    go(gen, ty, 0)
}

// ── drop glue (§19.3) ──────────────────────────────────────────────────────

/// Drops the captures of a closure environment, last capture first; the
/// runtime frees the memory.
pub(super) fn env_drop(gen: &mut Gen, name: &str, caps: &[Ty]) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let et = fx.ll(&super::func::env_ty(caps));
    for (k, t) in caps.iter().enumerate().rev() {
        if fx.needs_drop(t) {
            let p = fx.gep(&et, "%env", &["0".into(), (k + 2).to_string()]);
            call_drop(&mut fx, t, &p);
        }
    }
    format!("define internal void {name}(ptr %env) {{\nentry:\n{}  ret void\n}}\n", fx.code())
}

pub(super) fn drop_glue(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    drop_into(&mut fx, ty, "%v");
    format!("define internal void {name}(ptr %v) {{\nentry:\n{}  ret void\n}}\n", fx.code())
}

pub(super) fn call_drop(fx: &mut Fx, ty: &Ty, v: &str) {
    if !fx.needs_drop(ty) {
        return;
    }
    let f = fx.gen.item(Item::Drop(ty.clone()));
    fx.emit(format!("call void {f}(ptr {v})"));
}

fn drop_into(fx: &mut Fx, ty: &Ty, v: &str) {
    if let Ty::Fn(..) = ty {
        let f = fx.load("%pani", v);
        let env = fx.r();
        fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
        fx.emit(format!("call void @ante_pani_dec(ptr {env})"));
        return;
    }
    if let Ty::App(Con::Adt(_), _) = ty {
        // `sardhu` first, then the fields (§19.3).
        if let Some((bd, tys)) = user_impl(fx.gen, "Sardhukochu", ty) {
            fx.call_body_vals(bd, &tys, &[v.to_string()]);
        }
    }
    match ty {
        Ty::Tuple(items) => {
            let ptrs = field_ptrs(fx, ty, v, items.len());
            for (p, t) in ptrs.iter().zip(items).rev() {
                call_drop(fx, t, p);
            }
        }
        Ty::Str => fx.emit(format!("call void @ante_buf_free(ptr {v})")),
        Ty::Array(..) | Ty::App(Con::Varusa, _) => {
            if let Some((data, len, elem)) = seq_at(fx, ty, v) {
                if fx.needs_drop(&elem) {
                    each(fx, &data, &len, &elem, &mut |fx, p, _| call_drop(fx, &elem, p));
                }
            }
            if matches!(ty, Ty::App(..)) {
                fx.emit(format!("call void @ante_buf_free(ptr {v})"));
            }
        }
        Ty::App(Con::Pette, a) => {
            let p = fx.load("ptr", v);
            call_drop(fx, &a[0], &p);
            fx.emit(format!("call void @free(ptr {p})"));
        }
        Ty::App(Con::Undochu | Con::SareThappu | Con::Adt(_), _) => {
            if !fx.gen.is_enum(ty) {
                let fields = fx.gen.variants(ty).pop().unwrap_or_default();
                let st = Ty::Tuple(fields.clone());
                let ptrs = field_ptrs(fx, &st, v, fields.len());
                for (p, t) in ptrs.iter().zip(&fields).rev() {
                    call_drop(fx, t, p);
                }
                return;
            }
            by_variant(fx, ty, v, &mut |fx, _, pay, fields| {
                let st = Ty::Tuple(fields.to_vec());
                let ptrs = field_ptrs(fx, &st, pay, fields.len());
                for (p, t) in ptrs.iter().zip(fields).rev() {
                    call_drop(fx, t, p);
                }
            });
        }
        Ty::App(Con::Pravaham, a) => drop_iter(fx, &a[0], v),
        Ty::App(Con::Jodi | Con::GajibijiJodi | Con::Gumpu, _) => super::map::drop_map(fx, ty, v),
        _ => {}
    }
}

/// Drops an iterator (`%iter`, see native.rs): an owned sequence drops the
/// items not yet taken and frees its buffer; an adapter runs its own drop;
/// a map iterator drops a map it owns.
fn drop_iter(fx: &mut Fx, item: &Ty, v: &str) {
    let kp = fx.gep("%iter", v, &["0".into(), "0".into()]);
    let kind = fx.load("i32", &kp);
    let seq = fx.label();
    let adapter = fx.label();
    let end = fx.label();
    let map = fx.label();
    fx.emit(format!(
        "switch i32 {kind}, label %{end} [ i32 1, label %{seq} i32 3, label %{adapter} i32 4, label %{map} ]"
    ));
    fx.start(&map);
    super::map::drop_iter(fx, item, v);
    fx.emit(format!("br label %{end}"));
    fx.start(&seq);
    let op = fx.gep("%iter", v, &["0".into(), "1".into()]);
    let owned = fx.load("i8", &op);
    let is_owned = fx.r();
    fx.emit(format!("{is_owned} = icmp ne i8 {owned}, 0"));
    let item = item.clone();
    fx.when(&is_owned, |fx| {
        let pp = fx.gep("%iter", v, &["0".into(), "4".into()]);
        let data = fx.load("ptr", &pp);
        if !matches!(item, Ty::Ref(..)) && fx.needs_drop(&item) {
            let ap = fx.gep("%iter", v, &["0".into(), "2".into()]);
            let taken = fx.load("i64", &ap);
            let np = fx.gep("%iter", v, &["0".into(), "5".into()]);
            let n = fx.load("i64", &np);
            let et = fx.ll(&item);
            let rest = fx.gep(&et, &data, &[format!("i64 {taken}")]);
            let left = fx.r();
            fx.emit(format!("{left} = sub i64 {n}, {taken}"));
            each(fx, &rest, &left, &item, &mut |fx, p, _| call_drop(fx, &item, p));
        }
        fx.emit(format!("call void @free(ptr {data})"));
    });
    fx.emit(format!("br label %{end}"));
    fx.start(&adapter);
    let sp = fx.gep("%iter", v, &["0".into(), "4".into()]);
    let state = fx.load("ptr", &sp);
    let dp = fx.gep("{ ptr, ptr }", &state, &["0".into(), "1".into()]);
    let d = fx.load("ptr", &dp);
    fx.emit(format!("call void {d}(ptr {state})"));
    fx.emit(format!("br label %{end}"));
    fx.start(&end);
}

/// `i64 hash(ptr %v, i64 %h)` for a map key type.
pub(super) fn hash(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let r = super::map::hash_glue(&mut fx, ty);
    format!("define internal i64 {name}(ptr %v, i64 %h) {{\nentry:\n{}  ret i64 {r}\n}}\n", fx.code())
}

// ── sort callbacks and thunks ──────────────────────────────────────────────

pub(super) fn sort_cmp(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let r = call_cmp(&mut fx, ty, "%a", "%b");
    format!(
        "define internal i32 {name}(ptr %a, ptr %b, ptr %ctx) {{\nentry:\n{}  ret i32 {r}\n}}\n",
        fx.code()
    )
}

/// A sort callback calling the `pani` comparator at `ctx` with two references.
pub(super) fn sort_by(gen: &mut Gen, name: &str, ty: &Ty) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let kramam = fx.gen.g.kramam.map_or(Ty::Error, |k| Ty::App(Con::Adt(k), Vec::new()));
    let kt = fx.ll(&kramam);
    let f = fx.load("%pani", "%ctx");
    let fp = fx.r();
    fx.emit(format!("{fp} = extractvalue %pani {f}, 0"));
    let env = fx.r();
    fx.emit(format!("{env} = extractvalue %pani {f}, 1"));
    let k = fx.r();
    fx.emit(format!("{k} = call {kt} {fp}(ptr {env}, ptr %a, ptr %b)"));
    let slot = fx.spill(&kt, &k);
    let tp = fx.gep(&kt, &slot, &["0".into(), "0".into()]);
    let tag = fx.load("i32", &tp);
    let r = fx.r();
    fx.emit(format!("{r} = sub i32 {tag}, 1"));
    let _ = ty;
    format!(
        "define internal i32 {name}(ptr %a, ptr %b, ptr %ctx) {{\nentry:\n{}  ret i32 {r}\n}}\n",
        fx.code()
    )
}

/// `(env, args…)` → a named function, so it can be a `pani` value (§10.7.3).
pub(super) fn thunk(gen: &mut Gen, name: &str, fid: FnId, tys: &[Ty]) -> String {
    let mut fx = Fx::new_for_glue(gen);
    let Ty::Fn(params, ret) = fx.fn_value_ty(fid, tys) else { return String::new() };
    let rt = fx.ll(&ret);
    let mut decl = vec!["ptr %env".to_string()];
    let mut vals = Vec::new();
    for (i, (t, _)) in params.iter().enumerate() {
        let lt = fx.ll(t);
        decl.push(format!("{lt} %a{i}"));
        vals.push(format!("%a{i}"));
    }
    let r = match fx.gen.prog.fn_bodies.get(&fid).copied() {
        Some(body) => fx.call_body_vals(body, tys, &vals).0,
        None => {
            let fname = fx.gen.g.fns[fid].name.clone();
            fx.gen.error(format!("native backend: `{fname}` ni value ga vaadalem (standard function)"));
            "undef".into()
        }
    };
    format!("define internal {rt} {name}({}) {{\nentry:\n{}  ret {rt} {r}\n}}\n", decl.join(", "), fx.code())
}
