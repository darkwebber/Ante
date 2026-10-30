//! What the editor asks about, gathered once per check: every name
//! occurrence and what it refers to, the type of every expression, the
//! locals in scope where, and the members of every type seen. It owns all
//! its data, so queries never touch the compiler's tables.

use std::collections::HashMap;

use crate::ast::*;
use crate::package::Inspect;
use crate::resolve::{DefId, DefKind, FileId, Res, Resolution};
use crate::source::Span;
use crate::typeck::results::{expr_id, CallPlan, Results, Target};
use crate::typeck::{head_of, AdtKind, FnId, FnSig, Globals, RecvMode, Typeck};
use crate::types::{Con, Ty, VarKind};

/// Something a name can refer to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sym {
    Def(DefId),
    Fn(FnId),
    /// A local by its file and declaration offset.
    Local(FileId, usize),
    /// A record field or enum variant: the type and its index.
    Field(DefId, u32),
    Variant(DefId, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymKind {
    Function,
    Method,
    Local,
    Field,
    Variant,
    Record,
    Enum,
    Trait,
    Const,
    Module,
    Other,
}

pub struct SymInfo {
    pub name: String,
    /// Where it is declared, when that's in a file of this check.
    pub def: Option<(FileId, Span)>,
    /// Hover text (Markdown).
    pub hover: String,
    pub renamable: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Occ {
    pub span: Span,
    pub sym: Sym,
}

/// A completion candidate.
#[derive(Clone, Debug)]
pub struct Member {
    pub name: String,
    pub kind: SymKind,
    pub detail: String,
    /// For fields: the key of the field's type in `Index::members`.
    pub ty: Option<String>,
}

/// A local and where it can be seen: from its declaration to `until`.
pub struct LocalScope {
    pub name: String,
    pub decl: Span,
    pub until: usize,
    pub ty: Option<String>,
}

/// A function body, for what `naa` means inside it.
pub struct FnScope {
    pub body: Span,
    /// The key of `naa`'s type (`receiver`), or of `idhi` for pro-drop.
    pub naa: Option<String>,
}

#[derive(Default)]
pub struct FileIndex {
    pub text: String,
    /// A file of the program (not the standard library).
    pub user: bool,
    /// Sorted by start.
    pub occs: Vec<Occ>,
    /// Expression types, sorted by start.
    pub types: Vec<(Span, String)>,
    pub locals: Vec<LocalScope>,
    pub fns: Vec<FnScope>,
    /// Names visible anywhere in the file.
    pub globals: Vec<Member>,
}

#[derive(Default)]
pub struct Index {
    pub files: Vec<FileIndex>,
    pub syms: HashMap<Sym, SymInfo>,
    /// Members of each type, by its shown name.
    pub members: HashMap<String, Vec<Member>>,
    /// What `Name.` offers for a module, record or enum named `Name`.
    pub statics: HashMap<String, Vec<Member>>,
}

impl Index {
    pub fn build(i: &Inspect) -> Index {
        // With name errors the checker didn't run; try it anyway for hover
        // and completion, since a stray panic only costs those.
        let own;
        let tc: Option<&Typeck> = match i.typeck {
            Some(t) => Some(t),
            None => {
                own = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::typeck::check(i.modules, i.resolution)
                }))
                .ok();
                own.as_ref()
            }
        };
        let mut b = Builder {
            res: i.resolution,
            g: tc.map(|t| &t.globals),
            results: tc.map(|t| &t.results),
            ix: Index::default(),
            def_at: HashMap::new(),
            sig_text: HashMap::new(),
            field_spans: HashMap::new(),
            variant_spans: HashMap::new(),
            pending_types: Vec::new(),
        };
        let std_file = i.sources.iter().position(|s| s.name == crate::stdlib::ANTE_SOURCE_FILE);
        for (file, src) in i.sources.iter().enumerate() {
            b.ix.files.push(FileIndex {
                text: src.text.clone(),
                user: Some(file) != std_file,
                ..FileIndex::default()
            });
        }
        for (d, def) in b.res.defs.iter().enumerate() {
            if let Some(f) = def.file {
                b.def_at.insert((f, def.span.start), d);
            }
        }
        for m in i.modules {
            if let Some(ast) = m.ast {
                b.collect_decls(m.file, ast);
            }
        }
        for m in i.modules {
            if let (Some(ast), true) = (m.ast, b.ix.files[m.file].user) {
                let mut w = Walk { b: &mut b, file: m.file, naa: None, self_ty: None };
                for item in &ast.items {
                    w.item(item);
                }
                b.globals(m.file);
            }
        }
        b.expr_types();
        b.statics();
        b.drain_members();
        for f in &mut b.ix.files {
            f.occs.sort_by_key(|o| (o.span.start, o.span.end));
            f.occs.dedup_by_key(|o| o.span.start);
            f.types.sort_by_key(|(s, _)| (s.start, std::cmp::Reverse(s.end)));
        }
        b.ix
    }

    /// The occurrence under byte offset `at`, preferring one that contains
    /// it over one that ends there.
    pub fn occ_at(&self, file: FileId, at: usize) -> Option<Occ> {
        let occs = &self.files.get(file)?.occs;
        let inside = occs.iter().find(|o| o.span.start <= at && at < o.span.end);
        inside.or_else(|| occs.iter().find(|o| o.span.end == at && o.span.start < at)).copied()
    }

    /// The type of the smallest expression around `at`.
    pub fn type_at(&self, file: FileId, at: usize) -> Option<(Span, &str)> {
        let types = &self.files.get(file)?.types;
        types
            .iter()
            .filter(|(s, _)| s.start <= at && at < s.end.max(s.start + 1))
            .min_by_key(|(s, _)| s.end - s.start)
            .map(|(s, t)| (*s, t.as_str()))
    }

    /// Every occurrence of `sym`, as `(file, span)`.
    pub fn occurrences(&self, sym: Sym) -> Vec<(FileId, Span)> {
        let mut out = Vec::new();
        for (file, f) in self.files.iter().enumerate() {
            out.extend(f.occs.iter().filter(|o| o.sym == sym).map(|o| (file, o.span)));
        }
        out
    }
}

struct Builder<'x, 'a> {
    res: &'x Resolution,
    g: Option<&'x Globals<'a>>,
    results: Option<&'x Results>,
    ix: Index,
    /// Definitions by file and name offset.
    def_at: HashMap<(FileId, usize), DefId>,
    /// Signature source text of functions written in "Ante", by name.
    sig_text: HashMap<(FileId, usize), String>,
    field_spans: HashMap<DefId, Vec<Span>>,
    variant_spans: HashMap<DefId, Vec<Span>>,
    /// Types whose members are still to be listed.
    pending_types: Vec<Ty>,
}

/// At most this many types get member lists, to bound the work.
const MAX_MEMBER_TYPES: usize = 400;

impl<'x, 'a> Builder<'x, 'a> {
    // ── showing things ─────────────────────────────────────────────────────

    fn show(&self, ty: &Ty) -> String {
        match self.g {
            Some(g) => crate::typeck::show_ty(g, ty),
            None => "_".into(),
        }
    }

    /// A type's name, queuing it for a member list.
    fn key(&mut self, ty: &Ty) -> String {
        let key = self.show(ty);
        if !self.ix.members.contains_key(&key) && self.ix.members.len() < MAX_MEMBER_TYPES {
            self.ix.members.insert(key.clone(), Vec::new());
            self.pending_types.push(ty.clone());
        }
        key
    }

    fn show_in(&self, sig: &FnSig, ty: &Ty) -> String {
        let res = self.res;
        let adt_name = |d: DefId| res.defs[d].name.clone();
        let param_name =
            |i: u32| sig.generics.get(i as usize).map_or_else(|| format!("T{i}"), |g| g.name.clone());
        let var_kind = |_| VarKind::General;
        crate::types::Display { ty, adt_name: &adt_name, param_name: &param_name, var_kind: &var_kind }
            .to_string()
    }

    /// A function's signature: its source text when written in "Ante",
    /// else rebuilt from its type.
    fn signature(&self, fid: FnId) -> String {
        let Some(g) = self.g else { return String::new() };
        let sig = &g.fns[fid];
        if let Some(text) = sig.file.and_then(|f| self.sig_text.get(&(f, sig.span.start))) {
            return text.clone();
        }
        let mut params = Vec::new();
        if let Some(r) = &sig.receiver {
            params.push(match r.mode {
                RecvMode::Value => "naa: idhi".to_string(),
                RecvMode::Ref => "naa: &idhi".to_string(),
                RecvMode::RefMut => "naa: &maare idhi".to_string(),
            });
        }
        for p in &sig.params {
            let mut s = match p.kind {
                ParamKind::Variadic => "migathavi ".to_string(),
                ParamKind::NamedCollector => "migatha_perlu ".to_string(),
                _ => String::new(),
            };
            s.push_str(&format!("{}: {}", p.name, self.show_in(sig, &p.ty)));
            if let Some(r) = p.role {
                s.push(' ');
                s.push_str(r.as_str());
            }
            if p.keyword_only {
                s.push_str(" peru_thone");
            }
            if p.has_default {
                s.push_str(" = …");
            }
            params.push(s);
        }
        let own: Vec<&str> = sig.generics[sig.own_generics..].iter().map(|g| g.name.as_str()).collect();
        let generics = if own.is_empty() { String::new() } else { format!("[{}]", own.join(", ")) };
        format!("{}{generics}({}) -> {}", sig.name, params.join(", "), self.show_in(sig, &sig.ret))
    }

    /// `# …` comment lines just above line of `at` (skipping `@…` lines).
    fn doc_above(&self, file: FileId, at: usize) -> String {
        let text = &self.ix.files[file].text;
        let line_start = text[..at.min(text.len())].rfind('\n').map_or(0, |i| i + 1);
        let mut lines = Vec::new();
        for line in text[..line_start].lines().rev() {
            let t = line.trim();
            if t.starts_with('@') {
                continue;
            }
            match t.strip_prefix('#') {
                Some(rest) => lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string()),
                None => break,
            }
        }
        lines.reverse();
        lines.join("\n")
    }

    fn hover_text(&self, code: &str, note: &str, doc: &str) -> String {
        let mut out = format!("```ante\n{code}\n```");
        if !note.is_empty() {
            out.push_str("\n\n");
            out.push_str(note);
        }
        if !doc.is_empty() {
            out.push_str("\n\n---\n\n");
            out.push_str(doc);
        }
        out
    }

    fn user_file(&self, file: Option<FileId>) -> bool {
        file.is_some_and(|f| self.ix.files.get(f).is_some_and(|f| f.user))
    }

    // ── symbols ────────────────────────────────────────────────────────────

    fn sym_of_def(&self, d: DefId) -> Sym {
        match self.g.and_then(|g| g.fn_of_def.get(&d)) {
            Some(&fid) => Sym::Fn(fid),
            None => Sym::Def(d),
        }
    }

    /// Records `sym` at `span` and makes sure it has its info.
    fn occ(&mut self, file: FileId, span: Span, sym: Sym) {
        if span.end <= span.start {
            return;
        }
        self.ix.files[file].occs.push(Occ { span, sym });
        if !self.ix.syms.contains_key(&sym) {
            let info = self.info(sym);
            self.ix.syms.insert(sym, info);
        }
    }

    fn info(&mut self, sym: Sym) -> SymInfo {
        match sym {
            Sym::Local(file, decl) => {
                let name = self.word_at(file, decl);
                let span = Span::new(decl, decl + name.len());
                let ty = self.results.and_then(|r| r.local_ty.get(&(file, decl))).cloned();
                let code = match &ty {
                    Some((t, m)) => format!("{}{name}: {}", if *m { "maare " } else { "" }, self.show(t)),
                    None => name.clone(),
                };
                let hover = self.hover_text(&code, "local", "");
                SymInfo { name, def: Some((file, span)), hover, renamable: true }
            }
            Sym::Fn(fid) => {
                let g = self.g.expect("functions come from the type checker");
                let sig = &g.fns[fid];
                let in_trait = g.traits.values().any(|t| t.methods.iter().any(|(_, f)| *f == fid))
                    || g.impls.iter().any(|i| i.methods.values().any(|f| *f == fid));
                let note = match &sig.self_ty {
                    Some(t) => format!("`{}` method", self.show_in(sig, t)),
                    None => "function".to_string(),
                };
                let doc = sig.file.map(|f| self.doc_above(f, sig.span.start)).unwrap_or_default();
                let hover = self.hover_text(&self.signature(fid), &note, &doc);
                SymInfo {
                    name: sig.name.clone(),
                    def: sig.file.map(|f| (f, sig.span)),
                    hover,
                    renamable: self.user_file(sig.file) && !in_trait && sig.name != "modhalu",
                }
            }
            Sym::Def(d) => {
                let def = &self.res.defs[d];
                let (kind, code) = match def.kind {
                    DefKind::Fn => (SymKind::Function, def.name.clone()),
                    DefKind::Const => {
                        let ty = self.g.and_then(|g| g.consts.get(&d)).map(|t| self.show(t));
                        (SymKind::Const, format!("{}: {}", def.name, ty.as_deref().unwrap_or("_")))
                    }
                    DefKind::Record => (SymKind::Record, self.adt_text(d, "rakam")),
                    DefKind::Enum => (SymKind::Enum, self.adt_text(d, "edho_okati")),
                    DefKind::Trait | DefKind::StdTrait => (SymKind::Trait, format!("gunam {}", def.name)),
                    DefKind::Module(_) => (SymKind::Module, format!("{} thechuko", def.name)),
                    DefKind::StdType => (SymKind::Record, def.name.clone()),
                    DefKind::StdValue => (SymKind::Other, def.name.clone()),
                };
                let note = match kind {
                    SymKind::Module => "module",
                    SymKind::Const => "constant",
                    SymKind::Trait => "trait (gunam)",
                    _ if def.file.is_none() => "standard library",
                    _ => "",
                };
                let doc = def.file.map(|f| self.doc_above(f, def.span.start)).unwrap_or_default();
                let hover = self.hover_text(&code, note, &doc);
                let def_loc = def.file.filter(|_| def.span.end > def.span.start).map(|f| (f, def.span));
                let renamable = self.user_file(def.file)
                    && !matches!(def.kind, DefKind::Module(_))
                    && def.span.end > def.span.start;
                SymInfo { name: def.name.clone(), def: def_loc, hover, renamable }
            }
            Sym::Field(d, i) => {
                let def = &self.res.defs[d];
                let (name, ty) = match self.g.and_then(|g| g.adts.get(&d)).map(|a| &a.kind) {
                    Some(AdtKind::Record(fields)) => {
                        fields.get(i as usize).map(|f| (f.name.clone(), self.show(&f.ty))).unwrap_or_default()
                    }
                    _ => Default::default(),
                };
                let span = self.field_spans.get(&d).and_then(|s| s.get(i as usize)).copied();
                let doc = match (def.file, span) {
                    (Some(f), Some(s)) => self.doc_above(f, s.start),
                    _ => String::new(),
                };
                let hover = self.hover_text(&format!("{name}: {ty}"), &format!("`{}` field", def.name), &doc);
                SymInfo {
                    name,
                    def: def.file.zip(span),
                    hover,
                    renamable: self.user_file(def.file) && span.is_some(),
                }
            }
            Sym::Variant(d, i) => {
                let def = &self.res.defs[d];
                let (name, fields) = match self.g.and_then(|g| g.adts.get(&d)).map(|a| &a.kind) {
                    Some(AdtKind::Enum(vs)) => vs
                        .get(i as usize)
                        .map(|v| (v.name.clone(), v.fields.iter().map(|t| self.show(t)).collect::<Vec<_>>()))
                        .unwrap_or_default(),
                    _ => Default::default(),
                };
                let code = if fields.is_empty() {
                    format!("{}.{name}", def.name)
                } else {
                    format!("{}.{name}({})", def.name, fields.join(", "))
                };
                let span = self.variant_spans.get(&d).and_then(|s| s.get(i as usize)).copied();
                let doc = match (def.file, span) {
                    (Some(f), Some(s)) => self.doc_above(f, s.start),
                    _ => String::new(),
                };
                let hover = self.hover_text(&code, "variant", &doc);
                SymInfo {
                    name,
                    def: def.file.zip(span),
                    hover,
                    renamable: self.user_file(def.file) && span.is_some(),
                }
            }
        }
    }

    fn word_at(&self, file: FileId, at: usize) -> String {
        let text = &self.ix.files[file].text;
        let rest = text.get(at..).unwrap_or("");
        rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
    }

    fn adt_text(&self, d: DefId, word: &str) -> String {
        let def = &self.res.defs[d];
        let Some(adt) = self.g.and_then(|g| g.adts.get(&d)) else { return format!("{word} {}", def.name) };
        let generics: Vec<&str> = adt.generics.iter().map(|g| g.name.as_str()).collect();
        let mut out = format!("{word} {}", def.name);
        if !generics.is_empty() {
            out.push_str(&format!("[{}]", generics.join(", ")));
        }
        let show = |t: &Ty| {
            let res = self.res;
            let adt_name = |d: DefId| res.defs[d].name.clone();
            let param_name =
                |i: u32| adt.generics.get(i as usize).map_or_else(|| format!("T{i}"), |g| g.name.clone());
            let var_kind = |_| VarKind::General;
            crate::types::Display { ty: t, adt_name: &adt_name, param_name: &param_name, var_kind: &var_kind }
                .to_string()
        };
        out.push_str(" ante:");
        match &adt.kind {
            AdtKind::Record(fields) => {
                for f in fields.iter().take(20) {
                    out.push_str(&format!(
                        "\n    {}{}: {}",
                        if f.public { "andhariki " } else { "" },
                        f.name,
                        show(&f.ty)
                    ));
                }
            }
            AdtKind::Enum(vs) => {
                for v in vs.iter().take(20) {
                    let fields: Vec<String> = v.fields.iter().map(show).collect();
                    if fields.is_empty() {
                        out.push_str(&format!("\n    {}", v.name));
                    } else {
                        out.push_str(&format!("\n    {}({})", v.name, fields.join(", ")));
                    }
                }
            }
        }
        out
    }

    // ── declarations ───────────────────────────────────────────────────────

    /// Signature texts and field and variant spans of every parsed module.
    fn collect_decls(&mut self, file: FileId, ast: &Module) {
        let text = self.ix.files[file].text.clone();
        let mut sig = |s: &crate::ast::FnSig| {
            let raw = text.get(s.span.start..s.span.end).unwrap_or(&s.name.name);
            let raw = raw.trim_end().strip_suffix("ante:").unwrap_or(raw);
            let flat: Vec<&str> = raw.split_whitespace().collect();
            self.sig_text
                .insert((file, s.name.span.start), flat.join(" ").replace("( ", "(").replace(" )", ")"));
        };
        let mut fields = Vec::new();
        let mut variants = Vec::new();
        for item in &ast.items {
            match &item.kind {
                ItemKind::Fn(f) => sig(&f.sig),
                ItemKind::Methods(m) => {
                    for i in &m.items {
                        if let ItemKind::Fn(f) = &i.kind {
                            sig(&f.sig);
                        }
                    }
                }
                ItemKind::Trait(t) => t.items.iter().for_each(|i| sig(&i.sig)),
                ItemKind::Impl(i) => i.items.iter().for_each(|f| sig(&f.sig)),
                ItemKind::Extern(e) => e.fns.iter().for_each(&mut sig),
                ItemKind::Record(r) => {
                    fields.push((r.name.span, r.fields.iter().map(|f| f.name.span).collect()))
                }
                ItemKind::Enum(e) => {
                    variants.push((e.name.span, e.variants.iter().map(|v| v.name.span).collect()))
                }
                _ => {}
            }
        }
        for (name, spans) in fields {
            if let Some(&d) = self.def_at.get(&(file, name.start)) {
                self.field_spans.insert(d, spans);
            }
        }
        for (name, spans) in variants {
            if let Some(&d) = self.def_at.get(&(file, name.start)) {
                self.variant_spans.insert(d, spans);
            }
        }
    }

    // ── types and members ──────────────────────────────────────────────────

    fn expr_types(&mut self) {
        let Some(results) = self.results else { return };
        let mut seen: Vec<(FileId, Span, Ty)> = results
            .expr_ty
            .iter()
            .filter(|((f, _, _), _)| self.ix.files.get(*f).is_some_and(|f| f.user))
            .map(|(&(f, s, e), t)| (f, Span::new(s, e), t.clone()))
            .collect();
        seen.sort_by_key(|(f, s, _)| (*f, s.start, s.end));
        for (f, span, ty) in seen {
            let key = self.key(&ty);
            self.ix.files[f].types.push((span, key));
        }
    }

    /// Strips references and owning pointers.
    fn peel(&self, ty: &Ty) -> Ty {
        let mut t = ty.clone();
        for _ in 0..8 {
            t = match t {
                Ty::Ref(_, inner) => *inner,
                other => match self.g.and_then(|g| g.smart_deref(&other)) {
                    Some((inner, _)) => inner,
                    None => return other,
                },
            };
        }
        t
    }

    fn field_of(&self, ty: &Ty, name: &str) -> Option<(DefId, u32, Ty)> {
        let g = self.g?;
        let Ty::App(Con::Adt(d), args) = self.peel(ty) else { return None };
        let AdtKind::Record(fields) = &g.adts.get(&d)?.kind else { return None };
        let i = fields.iter().position(|f| f.name == name)?;
        Some((d, i as u32, fields[i].ty.subst(&args)))
    }

    fn drain_members(&mut self) {
        while let Some(ty) = self.pending_types.pop() {
            let key = self.show(&ty);
            let members = self.members_of(&ty);
            self.ix.members.insert(key, members);
        }
    }

    fn fn_member(&self, fid: FnId) -> Member {
        let g = self.g.expect("members come from the type checker");
        let kind = if g.fns[fid].receiver.is_some() { SymKind::Method } else { SymKind::Function };
        Member { name: g.fns[fid].name.clone(), kind, detail: self.signature(fid), ty: None }
    }

    fn members_of(&mut self, ty: &Ty) -> Vec<Member> {
        let Some(g) = self.g else { return Vec::new() };
        let t = self.peel(ty);
        let mut out: Vec<Member> = Vec::new();
        if let Ty::App(Con::Adt(d), args) = &t {
            if let Some(AdtKind::Record(fields)) = g.adts.get(d).map(|a| &a.kind) {
                for f in fields {
                    let fty = f.ty.subst(args);
                    let key = self.key(&fty);
                    out.push(Member {
                        name: f.name.clone(),
                        kind: SymKind::Field,
                        detail: key.clone(),
                        ty: Some(key),
                    });
                }
            }
        }
        let Some(head) = head_of(&t) else { return out };
        let mut fids: Vec<FnId> =
            g.inherent.get(&head).map(|m| m.values().copied().collect()).unwrap_or_default();
        for imp in &g.impls {
            if head_of(&imp.self_ty) == Some(head) {
                if let Some(tr) = g.traits.get(&imp.trait_ref.def) {
                    fids.extend(tr.methods.iter().map(|(_, f)| *f));
                }
            }
        }
        if matches!(head, crate::typeck::Head::Con(_) | crate::typeck::Head::Array | crate::typeck::Head::Str)
        {
            fids.extend(g.adapters.values().copied());
        }
        fids.retain(|&f| g.fns[f].receiver.is_some() || g.adapters.values().any(|&a| a == f));
        fids.sort_unstable();
        fids.dedup();
        for f in fids {
            let m = self.fn_member(f);
            if !out.iter().any(|o| o.name == m.name) {
                out.push(m);
            }
        }
        out
    }

    fn def_member(&self, d: DefId) -> Member {
        let def = &self.res.defs[d];
        let (kind, detail) = match def.kind {
            DefKind::Fn | DefKind::StdValue => match self.g.and_then(|g| g.fn_of_def.get(&d)) {
                Some(&fid) => (SymKind::Function, self.signature(fid)),
                None => (SymKind::Other, String::new()),
            },
            DefKind::Const => (SymKind::Const, String::new()),
            DefKind::Record | DefKind::StdType => (SymKind::Record, String::new()),
            DefKind::Enum => (SymKind::Enum, String::new()),
            DefKind::Trait | DefKind::StdTrait => (SymKind::Trait, String::new()),
            DefKind::Module(_) => (SymKind::Module, String::new()),
        };
        Member { name: def.name.clone(), kind, detail, ty: None }
    }

    /// Names a file sees without qualification.
    fn globals(&mut self, file: FileId) {
        let res = self.res;
        let mut out: Vec<Member> = Vec::new();
        let mut add = |b: &Self, map: &HashMap<String, DefId>, public_only: bool| {
            for (name, &d) in map {
                if (public_only && !res.defs[d].public) || name.starts_with('<') {
                    continue;
                }
                if !out.iter().any(|m| &m.name == name) {
                    out.push(b.def_member(d));
                }
            }
        };
        if let Some(m) = res.modules.iter().find(|m| m.file == Some(file)) {
            add(self, &m.values, false);
            add(self, &m.types, false);
            add(self, &m.imported_values, false);
            add(self, &m.imported_types, false);
        }
        if let Some(prelude) = res.modules.first() {
            add(self, &prelude.values, false);
            add(self, &prelude.types, false);
        }
        if let Some(&m) = res.module_by_name.get(crate::stdlib::ANTE_SOURCE_MODULE) {
            add(self, &res.modules[m].values, true);
            add(self, &res.modules[m].types, true);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        self.ix.files[file].globals = out;
    }

    /// `Name.` members: a module's items, an enum's variants, a type's
    /// associated functions.
    fn statics(&mut self) {
        let res = self.res;
        for m in &res.modules {
            let def = &res.defs[m.def];
            if m.name.starts_with('<') || def.name.is_empty() {
                continue;
            }
            let mut out: Vec<Member> = Vec::new();
            for &d in m.values.values().chain(m.types.values()) {
                if res.defs[d].public || m.file.is_none() {
                    out.push(self.def_member(d));
                }
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            self.ix.statics.insert(m.name.clone(), out);
        }
        let Some(g) = self.g else { return };
        for (&d, adt) in &g.adts {
            let mut out = Vec::new();
            if let AdtKind::Enum(vs) = &adt.kind {
                for v in vs {
                    out.push(Member {
                        name: v.name.clone(),
                        kind: SymKind::Variant,
                        detail: String::new(),
                        ty: None,
                    });
                }
            }
            if let Some(fns) = g.inherent.get(&crate::typeck::Head::Con(Con::Adt(d))) {
                let mut fids: Vec<FnId> =
                    fns.values().copied().filter(|&f| g.fns[f].receiver.is_none()).collect();
                fids.sort_unstable();
                out.extend(fids.into_iter().map(|f| self.fn_member(f)));
            }
            self.ix.statics.entry(adt.name.clone()).or_insert(out);
        }
    }
}

/// Walks one file's items, recording occurrences and scopes.
struct Walk<'w, 'x, 'a> {
    b: &'w mut Builder<'x, 'a>,
    file: FileId,
    /// `naa`'s type in the current method.
    naa: Option<Ty>,
    /// What `idhi` means: the type pro-dropped members come from.
    self_ty: Option<Ty>,
}

impl Walk<'_, '_, '_> {
    fn res(&self, span: Span) -> Option<Res> {
        self.b.res.resolutions.get(&(self.file, span.start)).copied()
    }

    fn occ(&mut self, span: Span, sym: Sym) {
        self.b.occ(self.file, span, sym);
    }

    fn ty_of(&self, span: Span) -> Option<Ty> {
        self.b.results?.expr_ty.get(&expr_id(self.file, span)).cloned()
    }

    fn call(&self, span: Span) -> Option<&CallPlan> {
        self.b.results?.calls.get(&expr_id(self.file, span))
    }

    fn local(&mut self, ident: &Ident, until: usize) {
        if self.res(ident.span) != Some(Res::Local(ident.span)) {
            return;
        }
        self.occ(ident.span, Sym::Local(self.file, ident.span.start));
        let ty = self.b.results.and_then(|r| r.local_ty.get(&(self.file, ident.span.start))).cloned();
        let ty = ty.map(|(t, _)| self.b.key(&t));
        self.b.ix.files[self.file].locals.push(LocalScope {
            name: ident.name.clone(),
            decl: ident.span,
            until,
            ty,
        });
    }

    fn item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Import(i) => {
                if let Some(&m) = self
                    .b
                    .res
                    .module_by_name
                    .get(&i.module.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join("."))
                {
                    let last = i.module.segments.last().map_or(i.module.span, |s| s.span);
                    let def = self.b.res.modules[m].def;
                    self.occ(last, Sym::Def(def));
                }
                if let ImportKind::Items(names) = &i.kind {
                    for n in names {
                        if let Some(Res::Def(d)) = self.res(n.span) {
                            let sym = self.b.sym_of_def(d);
                            self.occ(n.span, sym);
                        }
                    }
                }
            }
            ItemKind::Fn(f) => self.fn_decl(f),
            ItemKind::Const { name, ty, value } => {
                if let Some(&d) = self.b.def_at.get(&(self.file, name.span.start)) {
                    self.occ(name.span, Sym::Def(d));
                }
                self.ty(ty);
                self.expr(value);
            }
            ItemKind::Record(r) => {
                let d = self.b.def_at.get(&(self.file, r.name.span.start)).copied();
                if let Some(d) = d {
                    self.occ(r.name.span, Sym::Def(d));
                }
                self.generics(&r.generics);
                for (i, f) in r.fields.iter().enumerate() {
                    if let Some(d) = d {
                        self.occ(f.name.span, Sym::Field(d, i as u32));
                    }
                    self.ty(&f.ty);
                }
            }
            ItemKind::Enum(e) => {
                let d = self.b.def_at.get(&(self.file, e.name.span.start)).copied();
                if let Some(d) = d {
                    self.occ(e.name.span, Sym::Def(d));
                }
                self.generics(&e.generics);
                for (i, v) in e.variants.iter().enumerate() {
                    if let Some(d) = d {
                        self.occ(v.name.span, Sym::Variant(d, i as u32));
                    }
                    v.fields.iter().for_each(|t| self.ty(t));
                }
            }
            ItemKind::Methods(m) => {
                self.generics(&m.generics);
                self.ty(&m.self_ty);
                for i in &m.items {
                    if let ItemKind::Fn(f) = &i.kind {
                        self.fn_decl(f);
                    }
                }
            }
            ItemKind::Trait(t) => {
                if let Some(&d) = self.b.def_at.get(&(self.file, t.name.span.start)) {
                    self.occ(t.name.span, Sym::Def(d));
                }
                self.generics(&t.generics);
                for i in &t.items {
                    self.sig(&i.sig, i.default_body.as_ref());
                }
            }
            ItemKind::Impl(i) => {
                self.generics(&i.generics);
                self.ty(&i.self_ty);
                self.ty(&i.trait_ref);
                for f in &i.items {
                    self.fn_decl(f);
                }
            }
            ItemKind::Extern(e) => {
                for s in &e.fns {
                    self.sig(s, None);
                }
            }
        }
    }

    fn generics(&mut self, gs: &[GenericParam]) {
        for g in gs {
            g.bounds.iter().for_each(|t| self.ty(t));
        }
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        self.sig(&f.sig, Some(&f.body));
    }

    fn sig(&mut self, sig: &crate::ast::FnSig, body: Option<&Block>) {
        let fid = self.b.g.and_then(|g| g.fn_by_span.get(&(self.file, sig.name.span.start)).copied());
        if let Some(fid) = fid {
            self.occ(sig.name.span, Sym::Fn(fid));
        } else if let Some(&d) = self.b.def_at.get(&(self.file, sig.name.span.start)) {
            self.occ(sig.name.span, Sym::Def(d));
        }
        let fsig = fid.and_then(|f| self.b.g.map(|g| g.fns[f].clone()));
        let (naa, self_ty) = match &fsig {
            Some(s) => (s.receiver.as_ref().map(|r| r.ty.clone()), s.self_ty.clone()),
            None => (None, None),
        };
        let saved = (std::mem::replace(&mut self.naa, naa), std::mem::replace(&mut self.self_ty, self_ty));
        self.generics(&sig.generics);
        let until = body.map_or(sig.span.end, |b| b.span.end);
        for p in &sig.params {
            self.ty(&p.ty);
            if let Some(d) = &p.default {
                self.expr(d);
            }
            if p.kind != ParamKind::Receiver {
                self.local(&p.name, until);
            }
        }
        self.ty(&sig.ret);
        if let Some(body) = body {
            let naa_key = match (&self.naa, &self.self_ty) {
                (Some(t), _) | (None, Some(t)) => Some(self.b.key(t)),
                _ => None,
            };
            self.b.ix.files[self.file].fns.push(FnScope { body: body.span, naa: naa_key });
            self.block(body);
        }
        (self.naa, self.self_ty) = saved;
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s, b.span.end);
        }
    }

    fn stmt(&mut self, s: &Stmt, until: usize) {
        match &s.kind {
            StmtKind::Binding { target, ty, value, .. } => {
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
                match target {
                    BindTarget::Name(n) => self.local(n, until),
                    BindTarget::Tuple(ns) => ns.iter().for_each(|n| self.local(n, until)),
                }
            }
            StmtKind::Assign { place, value } | StmtKind::Compound { place, value, .. } => {
                self.expr(place);
                self.expr(value);
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            StmtKind::If { branches, else_block } => {
                for (c, b) in branches {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = else_block {
                    self.block(b);
                }
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::For { iter, pattern, body } => {
                self.expr(iter);
                self.pattern(pattern, body.span.end);
                self.block(body);
            }
            StmtKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for a in arms {
                    self.pattern(&a.pattern, a.body.span.end);
                    self.block(&a.body);
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Defer(b) | StmtKind::Unsafe(b) => self.block(b),
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    fn pattern(&mut self, p: &Pattern, until: usize) {
        match &p.kind {
            PatternKind::Wild => {}
            PatternKind::Literal(e) => self.expr(e),
            PatternKind::Range { lo, hi, .. } => {
                self.expr(lo);
                self.expr(hi);
            }
            PatternKind::Bind { name, .. } => self.local(name, until),
            PatternKind::Ctor { path, args } => {
                self.type_path(path);
                for a in args.iter().flatten() {
                    self.pattern(a, until);
                }
            }
            PatternKind::Record { path, fields, .. } => {
                let d = self.type_path(path);
                for (name, sub) in fields {
                    self.record_field(d, name);
                    match sub {
                        Some(sub) => self.pattern(sub, until),
                        None => self.local(name, until),
                    }
                }
            }
            PatternKind::Tuple(ps) | PatternKind::Or(ps) => ps.iter().for_each(|q| self.pattern(q, until)),
        }
    }

    fn record_field(&mut self, d: Option<DefId>, name: &Ident) {
        let Some(d) = d else { return };
        let Some(AdtKind::Record(fields)) = self.b.g.and_then(|g| g.adts.get(&d)).map(|a| &a.kind) else {
            return;
        };
        if let Some(i) = fields.iter().position(|f| f.name == name.name) {
            self.occ(name.span, Sym::Field(d, i as u32));
        }
    }

    fn ty(&mut self, t: &Type) {
        match &t.kind {
            TypeKind::Named { path, args } => {
                self.type_path(path);
                args.iter().for_each(|a| self.ty(a));
            }
            TypeKind::Ref { inner, .. } | TypeKind::Ptr { inner, .. } => self.ty(inner),
            TypeKind::Array { elem, len } => {
                self.ty(elem);
                self.expr(len);
            }
            TypeKind::Fn { params, ret } => {
                params.iter().for_each(|(p, _)| self.ty(p));
                self.ty(ret);
            }
        }
    }

    /// A path naming a type, module member or variant. Returns the record
    /// or enum it ends at.
    fn type_path(&mut self, path: &Path) -> Option<DefId> {
        let mut last = None;
        for seg in &path.segments {
            match self.res(seg.span) {
                Some(Res::Def(d)) => {
                    let sym = self.b.sym_of_def(d);
                    self.occ(seg.span, sym);
                    last = Some(d);
                }
                _ => {
                    let d = last?;
                    if let Some(i) = self.variant_index(d, &seg.name) {
                        self.occ(seg.span, Sym::Variant(d, i));
                    }
                    return Some(d);
                }
            }
        }
        last
    }

    fn variant_index(&self, d: DefId, name: &str) -> Option<u32> {
        match &self.b.g?.adts.get(&d)?.kind {
            AdtKind::Enum(vs) => vs.iter().position(|v| v.name == name).map(|i| i as u32),
            _ => None,
        }
    }

    fn method(&mut self, span: Span, plan: Option<&CallPlan>) {
        if let Some(Target::Fn { fid, .. } | Target::Trait { fid, .. }) = plan.map(|p| &p.target) {
            let fid = *fid;
            self.occ(span, Sym::Fn(fid));
        }
    }

    /// A path in an expression: a local, item or module member, then
    /// fields, ending perhaps at a method (`plan` is the call it heads).
    fn value_path(&mut self, path: &Path, plan: Option<&CallPlan>) {
        let segs = &path.segments;
        let first = &segs[0];
        let mut cur: Option<Ty> = None;
        let mut cur_def: Option<DefId> = None;
        let only = segs.len() == 1;
        match self.res(first.span) {
            Some(Res::Local(decl)) => {
                self.occ(first.span, Sym::Local(self.file, decl.start));
                cur = self
                    .b
                    .results
                    .and_then(|r| r.local_ty.get(&(self.file, decl.start)))
                    .map(|(t, _)| t.clone());
            }
            Some(Res::Def(d)) => {
                let sym = self.b.sym_of_def(d);
                self.occ(first.span, sym);
                cur_def = Some(d);
            }
            Some(Res::SelfMember) if first.name == "naa" => cur = self.naa.clone(),
            Some(Res::SelfMember) => {
                let field = self.self_ty.as_ref().and_then(|t| self.b.field_of(t, &first.name));
                match field {
                    Some((d, i, t)) => {
                        self.occ(first.span, Sym::Field(d, i));
                        cur = Some(t);
                    }
                    None if only => self.method(first.span, plan),
                    None => {}
                }
            }
            _ => {}
        }
        for (i, seg) in segs.iter().enumerate().skip(1) {
            let last = i + 1 == segs.len();
            if let Some(Res::Def(d)) = self.res(seg.span) {
                let sym = self.b.sym_of_def(d);
                self.occ(seg.span, sym);
                cur_def = Some(d);
                cur = None;
                continue;
            }
            if let Some(d) = cur_def.take() {
                if let Some(v) = self.variant_index(d, &seg.name) {
                    self.occ(seg.span, Sym::Variant(d, v));
                } else if last {
                    self.method(seg.span, plan);
                }
                return;
            }
            let field = cur.as_ref().and_then(|t| self.b.field_of(t, &seg.name));
            match field {
                Some((d, idx, t)) => {
                    self.occ(seg.span, Sym::Field(d, idx));
                    cur = Some(t);
                }
                None => {
                    if last {
                        self.method(seg.span, plan);
                    }
                    return;
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Path(p) => self.value_path(p, None),
            ExprKind::Call { callee, type_args, phrases } => {
                let plan = self.call(e.span).cloned();
                match &callee.kind {
                    ExprKind::Path(p) => self.value_path(p, plan.as_ref()),
                    ExprKind::Field { base, name } => {
                        self.expr(base);
                        let field = self.ty_of(base.span).and_then(|t| self.b.field_of(&t, &name.name));
                        match field {
                            Some((d, i, _)) => self.occ(name.span, Sym::Field(d, i)),
                            None => self.method(name.span, plan.as_ref()),
                        }
                    }
                    _ => self.expr(callee),
                }
                type_args.iter().for_each(|t| self.ty(t));
                for p in phrases {
                    if let PhraseKind::Value(v) | PhraseKind::Spread(v) = &p.kind {
                        self.expr(v);
                    }
                }
            }
            ExprKind::Field { base, name } => {
                self.expr(base);
                if let Some((d, i, _)) = self.ty_of(base.span).and_then(|t| self.b.field_of(&t, &name.name)) {
                    self.occ(name.span, Sym::Field(d, i));
                }
            }
            ExprKind::Closure { params, body, .. } => {
                for p in params {
                    if let Some(t) = &p.ty {
                        self.ty(t);
                    }
                    self.local(&p.name, body.span.end);
                }
                self.expr(body);
            }
            ExprKind::Ctor { path, args } => {
                self.type_path(path);
                args.iter().for_each(|a| self.expr(a));
            }
            ExprKind::Record { path, fields } => {
                let d = self.type_path(path);
                for (name, value) in fields {
                    self.record_field(d, name);
                    if value.span != name.span {
                        self.expr(value);
                    }
                }
            }
            _ => e.children().into_iter().for_each(|c| self.expr(c)),
        }
    }
}
