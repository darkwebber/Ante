//! `@automatic Raayochu, Chadhavochu, Kotthadhi` (spec §25.2). These traits
//! are written in "Ante" (`std/ante/data.ante`), so their derived
//! implementations are "Ante" source too: they are written after the end of
//! the file that declares the type, and checked like the rest of it.
//! Diagnostics inside the written part are moved to the `@automatic`
//! annotation that asked for it (`remap`).

use crate::ast::{
    Annotation, AnnotationArg, EnumDecl, GenericParam, Item, ItemKind, Module, RecordDecl, Type, TypeKind,
};
use crate::diag::Diagnostic;
use crate::pretty;
use crate::source::Span;
use crate::stdlib;

/// A written implementation: its byte range in the extended text, and the
/// annotation it came from.
#[derive(Clone, Debug)]
pub struct Written {
    pub start: usize,
    pub end: usize,
    pub annotation: Span,
}

/// The text to append to a file, and what each part of it implements.
/// `None` when no item derives one of these traits.
pub fn expand(text: &str, ast: &Module) -> Option<(String, Vec<Written>)> {
    let mut out = String::new();
    let mut written = Vec::new();
    for item in &ast.items {
        for (traits, annotation) in derived(item) {
            for tr in traits {
                let code = match (&item.kind, tr) {
                    (ItemKind::Record(r), "Raayochu") => record_write(r),
                    (ItemKind::Record(r), "Chadhavochu") => record_read(item, r),
                    (ItemKind::Record(r), "Kotthadhi") => record_default(r),
                    (ItemKind::Enum(e), "Raayochu") => enum_write(e),
                    (ItemKind::Enum(e), "Chadhavochu") => enum_read(e),
                    // Enums have no default (§25.2.5); the type checker says so.
                    _ => continue,
                };
                let start = text.len() + 1 + out.len();
                out.push_str(&code);
                out.push('\n');
                written.push(Written { start, end: text.len() + 1 + out.len(), annotation });
            }
        }
    }
    (!written.is_empty()).then_some((out, written))
}

/// The traits an item derives with source, per `@automatic` annotation.
fn derived(item: &Item) -> Vec<(Vec<&'static str>, Span)> {
    item.annotations
        .iter()
        .filter(|a| a.name.name == "automatic")
        .filter_map(|a| match &a.arg {
            AnnotationArg::Names(names) => {
                let traits: Vec<&'static str> = stdlib::SOURCE_DERIVED_TRAITS
                    .iter()
                    .copied()
                    .filter(|t| names.iter().any(|n| n.name == *t))
                    .collect();
                (!traits.is_empty()).then_some((traits, a.span))
            }
            _ => None,
        })
        .collect()
}

/// Moves diagnostics inside written implementations to their annotation.
/// Warnings there are dropped (the programmer can't act on them), and an
/// error is dropped when its annotation already has one.
pub fn remap(diags: &mut Vec<Diagnostic>, written: &[Written]) {
    let mut kept: Vec<Diagnostic> = Vec::new();
    for mut d in diags.drain(..) {
        let Some(w) = written.iter().find(|w| d.span.start >= w.start && d.span.start < w.end) else {
            kept.push(d);
            continue;
        };
        if !d.is_error() {
            continue;
        }
        if kept.iter().any(|k| k.is_error() && k.span == w.annotation) {
            continue;
        }
        d.span = w.annotation;
        kept.push(d);
    }
    *diags = kept;
}

/// The first line of an implementation: `[T: Tr] Name[T] ki Tr gunam undhi:`.
fn header(name: &str, generics: &[GenericParam], tr: &str) -> String {
    if generics.is_empty() {
        return format!("{name} ki {tr} gunam undhi:\n");
    }
    let params: Vec<String> = generics
        .iter()
        .map(|g| {
            if g.is_const {
                return format!("{}: ankam", g.name.name);
            }
            let mut bounds: Vec<String> = g.bounds.iter().map(pretty::ty).collect();
            bounds.push(tr.to_string());
            format!("{}: {}", g.name.name, bounds.join(" + "))
        })
        .collect();
    let args: Vec<&str> = generics.iter().map(|g| g.name.name.as_str()).collect();
    format!("[{}] {name}[{}] ki {tr} gunam undhi:\n", params.join(", "), args.join(", "))
}

fn has(annotations: &[Annotation], name: &str) -> bool {
    annotations.iter().any(|a| a.name.name == name)
}

/// The key a field or variant uses: its `@peru`, else its own name.
fn key(annotations: &[Annotation], name: &str) -> String {
    annotations
        .iter()
        .find_map(|a| match (&*a.name.name, &a.arg) {
            ("peru", AnnotationArg::Str(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| name.to_string())
}

/// A text literal.
fn lit(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn is_undochu(t: &Type) -> bool {
    matches!(&t.kind, TypeKind::Named { path, .. } if path.segments.len() == 1 && path.segments[0].name == "undochu")
}

fn record_write(r: &RecordDecl) -> String {
    let mut s = header(&r.name.name, &r.generics, "Raayochu");
    s.push_str("    raayi_cheyyi(naa: &idhi, w_: &maare Raasevaadu ni) -> khali ante:\n");
    s.push_str("        w_.jodi_modhalu()\n");
    for f in &r.fields {
        if has(&f.annotations, "dhaachu") {
            continue;
        }
        let k = lit(&key(&f.annotations, &f.name.name));
        if is_undochu(&f.ty) {
            // `ledhu` leaves the key out (§25.2.2).
            s.push_str(&format!("        &naa.{} batti:\n", f.name.name));
            s.push_str("            undhi(v_) aithe:\n");
            s.push_str(&format!("                {k} ni, v_ tho w_.field()\n"));
            s.push_str("            ledhu aithe:\n");
            s.push_str("                ()\n");
        } else {
            s.push_str(&format!("        {k} ni, &naa.{} tho w_.field()\n", f.name.name));
        }
    }
    s.push_str("        w_.jodi_mugimpu()\n");
    s
}

fn record_read(item: &Item, r: &RecordDecl) -> String {
    let mut s = header(&r.name.name, &r.generics, "Chadhavochu");
    s.push_str(
        "    chadhuvu_cheyyi(r_: &maare Chadhivevaadu ni) -> sare_thappu[idhi, ChadhuvuThappu] ante:\n",
    );
    s.push_str("        maare m_ ante r_.jodi_chadhuvu()?\n");
    let mut inits = Vec::new();
    for f in &r.fields {
        let local = format!("f_{}", f.name.name);
        let t = pretty::ty(&f.ty);
        let k = lit(&key(&f.annotations, &f.name.name));
        let value = if has(&f.annotations, "dhaachu") {
            "kotthadhi_ivvu()".to_string()
        } else if has(&f.annotations, "lekapothe_kotthadhi") {
            format!("({k} ni m_.lekapothe())?")
        } else if is_undochu(&f.ty) {
            format!("({k} ni m_.unte())?")
        } else {
            format!("({k} ni m_.field())?")
        };
        s.push_str(&format!("        {local}: {t} ante {value}\n"));
        inits.push(format!("{}: {local}", f.name.name));
    }
    let vadhiley = if has(&item.annotations, "theliyanivi_vadhiley") { "avunu" } else { "kaadhu" };
    s.push_str(&format!("        ({vadhiley} ni m_.mugimpu())?\n"));
    s.push_str(&format!("        sare({}{{{}}}) icchey\n", r.name.name, inits.join(", ")));
    s
}

fn record_default(r: &RecordDecl) -> String {
    let mut s = header(&r.name.name, &r.generics, "Kotthadhi");
    s.push_str("    kotthadhi() -> idhi ante:\n");
    let mut inits = Vec::new();
    for f in &r.fields {
        let local = format!("f_{}", f.name.name);
        s.push_str(&format!("        {local}: {} ante kotthadhi_ivvu()\n", pretty::ty(&f.ty)));
        inits.push(format!("{}: {local}", f.name.name));
    }
    s.push_str(&format!("        {}{{{}}} icchey\n", r.name.name, inits.join(", ")));
    s
}

fn enum_write(e: &EnumDecl) -> String {
    let mut s = header(&e.name.name, &e.generics, "Raayochu");
    s.push_str("    raayi_cheyyi(naa: &idhi, w_: &maare Raasevaadu ni) -> khali ante:\n");
    s.push_str("        naa batti:\n");
    for v in &e.variants {
        let k = lit(&key(&v.annotations, &v.name.name));
        if v.fields.is_empty() {
            s.push_str(&format!("            {}.{} aithe:\n", e.name.name, v.name.name));
            s.push_str(&format!("                {k} ni w_.padham_raayi()\n"));
            continue;
        }
        let binds: Vec<String> = (0..v.fields.len()).map(|i| format!("a{i}_")).collect();
        s.push_str(&format!("            {}.{}({}) aithe:\n", e.name.name, v.name.name, binds.join(", ")));
        s.push_str("                w_.jodi_modhalu()\n");
        s.push_str(&format!(
            "                \"rakam\" ni w_.key()\n                {k} ni w_.padham_raayi()\n"
        ));
        for (i, b) in binds.iter().enumerate() {
            s.push_str(&format!("                \"{i}\" ni, {b} tho w_.field()\n"));
        }
        s.push_str("                w_.jodi_mugimpu()\n");
    }
    s
}

fn enum_read(e: &EnumDecl) -> String {
    let mut s = header(&e.name.name, &e.generics, "Chadhavochu");
    s.push_str(
        "    chadhuvu_cheyyi(r_: &maare Chadhivevaadu ni) -> sare_thappu[idhi, ChadhuvuThappu] ante:\n",
    );
    s.push_str("        maare (peru_, m_) ante r_.variant_chadhuvu()?\n");
    for v in &e.variants {
        let k = lit(&key(&v.annotations, &v.name.name));
        s.push_str(&format!("        peru_ == {k} aithe:\n"));
        let mut args = Vec::new();
        for (i, t) in v.fields.iter().enumerate() {
            s.push_str(&format!("            a{i}_: {} ante (\"{i}\" ni m_.field())?\n", pretty::ty(t)));
            args.push(format!("a{i}_"));
        }
        s.push_str("            (kaadhu ni m_.mugimpu())?\n");
        if args.is_empty() {
            s.push_str(&format!("            sare({}.{}) icchey\n", e.name.name, v.name.name));
        } else {
            s.push_str(&format!(
                "            sare({}.{}({})) icchey\n",
                e.name.name,
                v.name.name,
                args.join(", ")
            ));
        }
    }
    s.push_str("        e_ ante ChadhuvuThappuVidham.Theliyani(peru_) ni r_.thappu_ga()\n");
    s.push_str("        thappu(e_) icchey\n");
    s
}
