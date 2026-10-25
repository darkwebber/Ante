//! S-expression printer for the AST, used by `antec emit ast` and parser tests.
//!
//! Expressions print on one line; items and blocks print one statement per
//! line, indented two spaces per level.

use crate::ast::*;
use crate::token::quote;

pub fn module(m: &Module) -> String {
    let mut out = String::new();
    for item in &m.items {
        write_item(&mut out, item, 0);
    }
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn line(out: &mut String, depth: usize, text: &str) {
    indent(out, depth);
    out.push_str(text);
    out.push('\n');
}

fn path(p: &Path) -> String {
    p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

pub fn ty(t: &Type) -> String {
    match &t.kind {
        TypeKind::Named { path: p, args } if args.is_empty() => path(p),
        TypeKind::Named { path: p, args } => {
            format!("{}[{}]", path(p), args.iter().map(ty).collect::<Vec<_>>().join(", "))
        }
        TypeKind::Ref { mutable, inner } => format!("&{}{}", if *mutable { "maare " } else { "" }, ty(inner)),
        TypeKind::Ptr { mutable, inner } => format!("*{}{}", if *mutable { "maare " } else { "" }, ty(inner)),
        TypeKind::Array { elem, len } => format!("[{}; {}]", ty(elem), expr(len)),
        TypeKind::Fn { params, ret } => {
            let params: Vec<String> = params
                .iter()
                .map(|(t, role)| match role {
                    Some(r) => format!("{} {}", ty(t), r.as_str()),
                    None => ty(t),
                })
                .collect();
            format!("pani[({}) -> {}]", params.join(", "), ty(ret))
        }
    }
}

fn generics(gs: &[GenericParam]) -> String {
    if gs.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = gs
        .iter()
        .map(|g| {
            if g.is_const {
                format!("{}: ankam", g.name.name)
            } else if g.bounds.is_empty() {
                g.name.name.clone()
            } else {
                format!("{}: {}", g.name.name, g.bounds.iter().map(ty).collect::<Vec<_>>().join(" + "))
            }
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

fn param(p: &Param) -> String {
    let mut s = String::from("(");
    match p.kind {
        ParamKind::Receiver => {}
        ParamKind::Normal => {}
        ParamKind::Variadic => s.push_str("migathavi "),
        ParamKind::NamedCollector => s.push_str("migatha_perlu "),
    }
    s.push_str(&p.name.name);
    s.push(' ');
    s.push_str(&ty(&p.ty));
    if let Some(r) = p.role {
        s.push(' ');
        s.push_str(r.as_str());
    }
    if p.keyword_only {
        s.push_str(" peru_thone");
    }
    if let Some(d) = &p.default {
        s.push_str(" = ");
        s.push_str(&expr(d));
    }
    s.push(')');
    s
}

fn sig(s: &FnSig) -> String {
    format!(
        "{}{} ({}) -> {}",
        s.name.name,
        generics(&s.generics),
        s.params.iter().map(param).collect::<Vec<_>>().join(" "),
        ty(&s.ret)
    )
}

fn write_item(out: &mut String, item: &Item, depth: usize) {
    for a in &item.annotations {
        let arg = match &a.arg {
            AnnotationArg::None => String::new(),
            AnnotationArg::Str(s) => format!(" {}", quote(s, '"')),
            AnnotationArg::Names(ns) => {
                format!(" {}", ns.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", "))
            }
        };
        line(out, depth, &format!("@{}{}", a.name.name, arg));
    }
    let public = if item.public { "andhariki " } else { "" };
    match &item.kind {
        ItemKind::Import(imp) => {
            let text = match &imp.kind {
                ImportKind::Module => format!("(import {})", path(&imp.module)),
                ImportKind::Items(names) => format!(
                    "(import {} ({}))",
                    path(&imp.module),
                    names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(" ")
                ),
                ImportKind::Alias(alias) => format!("(import {} as {})", path(&imp.module), alias.name),
            };
            line(out, depth, &text);
        }
        ItemKind::Fn(f) => write_fn(out, f, public, depth),
        ItemKind::Const { name, ty: t, value } => {
            line(out, depth, &format!("({public}const {} {} {})", name.name, ty(t), expr(value)))
        }
        ItemKind::Record(r) => {
            line(out, depth, &format!("({public}rakam {}{}", r.name.name, generics(&r.generics)));
            for f in &r.fields {
                let p = if f.public { "andhariki " } else { "" };
                line(out, depth + 1, &format!("({p}{} {})", f.name.name, ty(&f.ty)));
            }
            close(out);
        }
        ItemKind::Enum(e) => {
            line(out, depth, &format!("({public}edho_okati {}{}", e.name.name, generics(&e.generics)));
            for v in &e.variants {
                if v.fields.is_empty() {
                    line(out, depth + 1, &v.name.name);
                } else {
                    let fields = v.fields.iter().map(ty).collect::<Vec<_>>().join(" ");
                    line(out, depth + 1, &format!("({} {})", v.name.name, fields));
                }
            }
            close(out);
        }
        ItemKind::Methods(m) => {
            line(out, depth, &format!("(panulu{} {}", generics(&m.generics), ty(&m.self_ty)));
            for item in &m.items {
                write_item(out, item, depth + 1);
            }
            close(out);
        }
        ItemKind::Trait(t) => {
            line(out, depth, &format!("({public}gunam {}{}", t.name.name, generics(&t.generics)));
            for item in &t.items {
                match &item.default_body {
                    None => line(out, depth + 1, &format!("(sig {})", sig(&item.sig))),
                    Some(body) => {
                        line(out, depth + 1, &format!("(fn {}", sig(&item.sig)));
                        write_block(out, body, depth + 2);
                        close(out);
                    }
                }
            }
            close(out);
        }
        ItemKind::Impl(i) => {
            line(
                out,
                depth,
                &format!("(impl{} {} ki {}", generics(&i.generics), ty(&i.self_ty), ty(&i.trait_ref)),
            );
            for f in &i.items {
                write_fn(out, f, "", depth + 1);
            }
            close(out);
        }
        ItemKind::Extern(e) => {
            line(out, depth, &format!("(bayata {}", quote(&e.abi, '"')));
            for s in &e.fns {
                line(out, depth + 1, &format!("(sig {})", sig(s)));
            }
            close(out);
        }
    }
}

/// Appends `)` to the last line printed.
fn close(out: &mut String) {
    if out.ends_with('\n') {
        out.pop();
    }
    out.push_str(")\n");
}

fn write_fn(out: &mut String, f: &FnDecl, public: &str, depth: usize) {
    line(out, depth, &format!("({public}fn {}", sig(&f.sig)));
    write_block(out, &f.body, depth + 1);
    close(out);
}

fn write_block(out: &mut String, b: &Block, depth: usize) {
    for s in &b.stmts {
        write_stmt(out, s, depth);
    }
}

fn write_stmt(out: &mut String, s: &Stmt, depth: usize) {
    match &s.kind {
        StmtKind::Binding { mutable, target, ty: t, value } => {
            let m = if *mutable { "maare " } else { "" };
            let target = match target {
                BindTarget::Name(n) => n.name.clone(),
                BindTarget::Tuple(ns) => {
                    format!("({})", ns.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(" "))
                }
            };
            let t = t.as_ref().map(|t| format!(": {}", ty(t))).unwrap_or_default();
            line(out, depth, &format!("(let {m}{target}{t} {})", expr(value)));
        }
        StmtKind::Assign { place, value } => {
            line(out, depth, &format!("(pettu {} {})", expr(place), expr(value)))
        }
        StmtKind::Compound { place, op, value } => {
            line(out, depth, &format!("({}= {} {})", op.as_str(), expr(place), expr(value)))
        }
        StmtKind::Return(None) => line(out, depth, "(icchey)"),
        StmtKind::Return(Some(e)) => line(out, depth, &format!("(icchey {})", expr(e))),
        StmtKind::If { branches, else_block } => {
            line(out, depth, "(aithe");
            for (cond, body) in branches {
                line(out, depth + 1, &format!("({}", expr(cond)));
                write_block(out, body, depth + 2);
                close(out);
            }
            if let Some(body) = else_block {
                line(out, depth + 1, "(ledhante");
                write_block(out, body, depth + 2);
                close(out);
            }
            close(out);
        }
        StmtKind::While { cond, body } => {
            line(out, depth, &format!("(unnanthasepu {}", expr(cond)));
            write_block(out, body, depth + 1);
            close(out);
        }
        StmtKind::For { iter, pattern: p, body } => {
            line(out, depth, &format!("(okko {} {}", pattern(p), expr(iter)));
            write_block(out, body, depth + 1);
            close(out);
        }
        StmtKind::Match { scrutinee, arms } => {
            line(out, depth, &format!("(batti {}", expr(scrutinee)));
            for arm in arms {
                line(out, depth + 1, &format!("({}", pattern(&arm.pattern)));
                write_block(out, &arm.body, depth + 2);
                close(out);
            }
            close(out);
        }
        StmtKind::Break => line(out, depth, "(aapey)"),
        StmtKind::Continue => line(out, depth, "(vadhiley)"),
        StmtKind::Defer(b) => {
            line(out, depth, "(vellagane");
            write_block(out, b, depth + 1);
            close(out);
        }
        StmtKind::Unsafe(b) => {
            line(out, depth, "(naa_risk");
            write_block(out, b, depth + 1);
            close(out);
        }
        StmtKind::Expr(e) => line(out, depth, &expr(e)),
    }
}

fn phrase(p: &Phrase) -> String {
    let base = match &p.kind {
        PhraseKind::Value(e) => expr(e),
        PhraseKind::Spread(e) => format!("...{}", expr(e)),
        PhraseKind::Hole => "_".into(),
    };
    if p.role.is_none() && p.label.is_none() {
        return base;
    }
    let role = p.role.map(Role::as_str).unwrap_or("none");
    match &p.label {
        Some(label) => format!("({role} {base} :{})", label.name),
        None => format!("({role} {base})"),
    }
}

pub fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(lit) => match lit.radix {
            2 => format!("0b{}", lit.digits),
            8 => format!("0o{}", lit.digits),
            16 => format!("0x{}", lit.digits),
            _ => lit.digits.clone(),
        },
        ExprKind::Float(text) => text.clone(),
        ExprKind::Str(s) => quote(s, '"'),
        ExprKind::Char(c) => quote(c.encode_utf8(&mut [0; 4]), '\''),
        ExprKind::FStr(parts) => {
            let parts: Vec<String> = parts
                .iter()
                .map(|p| match p {
                    FStrPart::Text(t) => quote(t, '"'),
                    FStrPart::Expr(e) => expr(e),
                })
                .collect();
            format!("(fstr {})", parts.join(" "))
        }
        ExprKind::Bool(true) => "avunu".into(),
        ExprKind::Bool(false) => "kaadhu".into(),
        ExprKind::Ledhu => "ledhu".into(),
        ExprKind::BareSare => "sare".into(),
        ExprKind::Unit => "()".into(),
        ExprKind::Hole => "_".into(),
        ExprKind::Path(p) => path(p),
        ExprKind::Call { callee, type_args, phrases } => {
            let mut s = format!("(call {}", expr(callee));
            if !type_args.is_empty() {
                s.push_str(&format!("[{}]", type_args.iter().map(ty).collect::<Vec<_>>().join(", ")));
            }
            for p in phrases {
                s.push(' ');
                s.push_str(&phrase(p));
            }
            s.push(')');
            s
        }
        ExprKind::Try(inner) => format!("(? {})", expr(inner)),
        ExprKind::Pipe { input, stage } => format!("(|> {} {})", expr(input), expr(stage)),
        ExprKind::Binary { op, lhs, rhs } => format!("({} {} {})", op.as_str(), expr(lhs), expr(rhs)),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnOp::Neg => "neg",
                UnOp::Not => "!",
                UnOp::BitNot => "~",
                UnOp::Deref => "*",
                UnOp::Ref => "&",
                UnOp::RefMut => "&maare",
            };
            format!("({op} {})", expr(operand))
        }
        ExprKind::Field { base, name } => format!("(. {} {})", expr(base), name.name),
        ExprKind::TupleIndex { base, index } => format!("(. {} {index})", expr(base)),
        ExprKind::Index { base, indices } => {
            format!("(index {} {})", expr(base), indices.iter().map(expr).collect::<Vec<_>>().join(" "))
        }
        ExprKind::Range { lo, hi, inclusive } => {
            format!("({} {} {})", if *inclusive { "varaku" } else { "lopu" }, expr(lo), expr(hi))
        }
        ExprKind::Closure { moves, params, body } => {
            let params: Vec<String> = params
                .iter()
                .map(|p| {
                    let mut s = p.name.name.clone();
                    if let Some(t) = &p.ty {
                        s.push_str(": ");
                        s.push_str(&ty(t));
                    }
                    if let Some(r) = p.role {
                        s.push(' ');
                        s.push_str(r.as_str());
                    }
                    s
                })
                .collect();
            let moves = if *moves { "theesuko " } else { "" };
            format!("({moves}closure ({}) {})", params.join(", "), expr(body))
        }
        ExprKind::Ctor { path: p, args } => {
            format!("({} {})", path(p), args.iter().map(expr).collect::<Vec<_>>().join(" "))
        }
        ExprKind::Record { path: p, fields } => {
            let fields: Vec<String> =
                fields.iter().map(|(n, v)| format!("({} {})", n.name, expr(v))).collect();
            format!("(record {} {})", path(p), fields.join(" ")).replace(" )", ")")
        }
        ExprKind::Tuple(es) => seq("tuple", es),
        ExprKind::List(es) => seq("list", es),
        ExprKind::Map(pairs) => {
            let pairs: Vec<String> =
                pairs.iter().map(|(k, v)| format!("({} {})", expr(k), expr(v))).collect();
            format!("(map {})", pairs.join(" ")).replace(" )", ")")
        }
    }
}

fn seq(name: &str, es: &[Expr]) -> String {
    if es.is_empty() {
        return format!("({name})");
    }
    format!("({name} {})", es.iter().map(expr).collect::<Vec<_>>().join(" "))
}

pub fn pattern(p: &Pattern) -> String {
    match &p.kind {
        PatternKind::Wild => "_".into(),
        PatternKind::Literal(e) => expr(e),
        PatternKind::Range { lo, hi, inclusive } => {
            format!("({} {} {})", if *inclusive { "varaku" } else { "lopu" }, expr(lo), expr(hi))
        }
        PatternKind::Bind { mutable: true, name } => format!("(maare {})", name.name),
        PatternKind::Bind { mutable: false, name } => name.name.clone(),
        PatternKind::Ctor { path: p, args: None } => path(p),
        PatternKind::Ctor { path: p, args: Some(args) } => {
            format!("({} {})", path(p), args.iter().map(pattern).collect::<Vec<_>>().join(" "))
        }
        PatternKind::Record { path: p, fields, rest } => {
            let mut parts: Vec<String> = fields
                .iter()
                .map(|(n, pat)| match pat {
                    Some(pat) => format!("({} {})", n.name, pattern(pat)),
                    None => n.name.clone(),
                })
                .collect();
            if *rest {
                parts.push("..".into());
            }
            format!("(record {} {})", path(p), parts.join(" "))
        }
        PatternKind::Tuple(ps) => {
            format!("(tuple {})", ps.iter().map(pattern).collect::<Vec<_>>().join(" ")).replace(" )", ")")
        }
        PatternKind::Or(ps) => format!("(| {})", ps.iter().map(pattern).collect::<Vec<_>>().join(" ")),
    }
}
