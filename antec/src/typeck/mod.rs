//! Type checking (spec §9–14): item signatures, trait implementations,
//! function bodies with local inference, call binding (§11–12), patterns and
//! exhaustiveness.
//!
//! Runs after name resolution and builds on its `resolutions` table:
//! 1. load the standard library signatures (`antec/std/*.ante`),
//! 2. collect every record, enum, trait, function, method and implementation,
//! 3. check items on their own (orphans, overlaps, missing trait methods,
//!    infinite types, stored references),
//! 4. check every body against its signature (`body.rs`).

mod body;
mod call;
mod items;
mod lower;
mod pattern;
mod solve;
mod std_lib;

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::resolve::{DefId, FileId, ModId, Res, Resolution, SourceModule};
use crate::source::Span;
use crate::types::{Con, Ty};
use lower::Lower;

pub type FnId = usize;

/// A generic parameter of an item. Index 0 of a trait method's list is `idhi`.
#[derive(Clone, Debug)]
pub struct Generic {
    pub name: String,
    pub bounds: Vec<TraitRef>,
    pub is_const: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitRef {
    pub def: DefId,
    pub args: Vec<Ty>,
}

#[derive(Clone, Debug)]
pub struct Formal {
    pub name: String,
    pub ty: Ty,
    pub role: Option<Role>,
    pub kind: ParamKind,
    pub keyword_only: bool,
    pub has_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvMode {
    Value,
    Ref,
    RefMut,
}

#[derive(Clone, Debug)]
pub struct Receiver {
    pub mode: RecvMode,
    /// The receiver's type without the borrow (`idhi`, `varusa[T]`, …).
    pub ty: Ty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnOrigin {
    /// A function written in "Ante".
    User,
    /// A `bayata "C"` import: calling it needs `naa_risk` (§22.1).
    Foreign,
    /// A standard library signature, implemented by the compiler.
    Std,
    /// A `memory` function: standard, but only inside `naa_risk`.
    StdUnsafe,
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub name: String,
    /// The owning type's or trait's generics first, then the function's own.
    pub generics: Vec<Generic>,
    /// Where the function's own generics start in `generics`.
    pub own_generics: usize,
    pub params: Vec<Formal>,
    pub receiver: Option<Receiver>,
    pub ret: Ty,
    /// What `idhi` means inside the body, for methods and implementations.
    pub self_ty: Option<Ty>,
    pub origin: FnOrigin,
    pub public: bool,
    pub module: ModId,
    pub file: Option<FileId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Ty,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<Ty>,
}

#[derive(Clone, Debug)]
pub enum AdtKind {
    Record(Vec<FieldInfo>),
    Enum(Vec<VariantInfo>),
}

#[derive(Clone, Debug)]
pub struct Adt {
    pub name: String,
    pub module: ModId,
    pub generics: Vec<Generic>,
    pub kind: AdtKind,
    /// Traits listed in `@automatic`.
    pub derives: Vec<DefId>,
    /// `@C`: the target's C layout (§22.3).
    pub c_layout: bool,
    pub file: Option<FileId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Trait {
    pub name: String,
    /// Index 0 is `idhi`.
    pub generics: Vec<Generic>,
    pub methods: Vec<(String, FnId)>,
    pub defaults: HashSet<String>,
    pub module: ModId,
    pub file: Option<FileId>,
}

#[derive(Clone, Debug)]
pub struct Impl {
    pub trait_ref: TraitRef,
    pub self_ty: Ty,
    pub generics: Vec<Generic>,
    pub methods: HashMap<String, FnId>,
    pub file: FileId,
    pub span: Span,
}

/// The outermost shape of a type, for method tables (§13.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Head {
    Int,
    Float(u8),
    Bool,
    Char,
    Unit,
    Str,
    Con(Con),
    Ptr,
    Array,
}

pub fn head_of(ty: &Ty) -> Option<Head> {
    Some(match ty {
        Ty::Int(_) => Head::Int,
        Ty::Float(b) => Head::Float(*b),
        Ty::Bool => Head::Bool,
        Ty::Char => Head::Char,
        Ty::Unit => Head::Unit,
        Ty::Str => Head::Str,
        Ty::App(c, _) => Head::Con(*c),
        Ty::Ptr(..) => Head::Ptr,
        Ty::Array(..) => Head::Array,
        _ => return None,
    })
}

/// Everything known about the program's items, shared by all bodies.
pub struct Globals<'a> {
    pub res: &'a Resolution,
    pub adts: HashMap<DefId, Adt>,
    pub fns: Vec<FnSig>,
    pub fn_of_def: HashMap<DefId, FnId>,
    pub traits: HashMap<DefId, Trait>,
    pub impls: Vec<Impl>,
    /// Inherent methods and associated functions, by type head.
    pub inherent: HashMap<Head, HashMap<String, FnId>>,
    pub consts: HashMap<DefId, Ty>,
    /// Prelude adapters usable with method syntax: `xs.varusa_ga()`.
    pub adapters: HashMap<String, FnId>,
    /// Standard traits by name.
    pub std_traits: HashMap<String, DefId>,
    pub kramam: Option<DefId>,
    /// `gumpu`, `gajibiji_jodi` and `Paridhi` are prelude names, not keywords.
    pub std_cons: HashMap<DefId, Con>,
    /// User functions by the file and start of their name, to find a body's signature.
    pub fn_by_span: HashMap<(FileId, usize), FnId>,
}

impl Globals<'_> {
    pub fn module_of_file(&self, file: FileId) -> ModId {
        self.res.modules.iter().position(|m| m.file == Some(file)).unwrap_or(0)
    }

    pub fn std_trait(&self, name: &str) -> Option<DefId> {
        self.std_traits.get(name).copied()
    }
}

/// Where a type written in source is lowered.
pub(crate) struct TyEnv<'x> {
    /// `None` for standard library files, which have no resolution entries.
    pub file: Option<FileId>,
    /// The module whose names a standard library file sees.
    pub std_module: ModId,
    pub generics: &'x [Generic],
    pub self_ty: Option<Ty>,
}

pub fn check(modules: &[SourceModule], res: &Resolution) -> Vec<(FileId, Diagnostic)> {
    let mut c = Collector::new(res);
    c.load_std();
    for m in modules {
        if let Some(ast) = m.ast {
            c.collect_types(m.file, ast);
        }
    }
    for m in modules {
        if let Some(ast) = m.ast {
            c.collect_signatures(m.file, ast);
        }
    }
    let mut diags = std::mem::take(&mut c.diags);
    let globals = c.globals;
    diags.extend(items::check_items(&globals, modules));
    for m in modules {
        if let Some(ast) = m.ast {
            diags.extend(body::check_module(&globals, m.file, ast));
        }
    }
    diags.sort_by_key(|(file, d)| (*file, d.span.start));
    diags.dedup_by(|a, b| a.0 == b.0 && a.1.code == b.1.code && a.1.span == b.1.span);
    diags
}

// ── collection ─────────────────────────────────────────────────────────────

struct Collector<'a> {
    globals: Globals<'a>,
    diags: Vec<(FileId, Diagnostic)>,
}

impl<'a> Collector<'a> {
    fn new(res: &'a Resolution) -> Self {
        let mut std_cons = HashMap::new();
        let prelude = &res.modules[0];
        for (name, con) in
            [("gumpu", Con::Gumpu), ("gajibiji_jodi", Con::GajibijiJodi), ("Paridhi", Con::Paridhi)]
        {
            if let Some(&d) = prelude.types.get(name) {
                std_cons.insert(d, con);
            }
        }
        let std_traits = crate::stdlib::PRELUDE_TRAITS
            .iter()
            .filter_map(|n| prelude.types.get(*n).map(|&d| (n.to_string(), d)))
            .collect();
        Collector {
            globals: Globals {
                res,
                adts: HashMap::new(),
                fns: Vec::new(),
                fn_of_def: HashMap::new(),
                traits: HashMap::new(),
                impls: Vec::new(),
                inherent: HashMap::new(),
                consts: HashMap::new(),
                adapters: HashMap::new(),
                std_traits,
                kramam: prelude.types.get("Kramam").copied(),
                std_cons,
                fn_by_span: HashMap::new(),
            },
            diags: Vec::new(),
        }
    }

    fn error(&mut self, file: FileId, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push((file, Diagnostic::error(code, span, message)));
    }

    fn res(&self) -> &'a Resolution {
        self.globals.res
    }

    fn module_of_file(&self, file: FileId) -> ModId {
        self.globals.module_of_file(file)
    }

    fn lower(&mut self) -> Lower<'_, 'a> {
        Lower { g: &self.globals, diags: &mut self.diags }
    }

    fn lower_type(&mut self, env: &TyEnv, t: &Type) -> Ty {
        self.lower().lower_type(env, t)
    }

    fn lower_generics(
        &mut self,
        file: Option<FileId>,
        std_module: ModId,
        outer: &[Generic],
        params: &[GenericParam],
        self_ty: Option<Ty>,
    ) -> Vec<Generic> {
        self.lower().lower_generics(file, std_module, outer, params, self_ty)
    }

    fn lower_trait_ref(&mut self, env: &TyEnv, t: &Type) -> Option<TraitRef> {
        self.lower().lower_trait_ref(env, t)
    }

    fn check_stored(&mut self, file: FileId, ty: &Ty, span: Span) {
        self.lower().check_stored(file, ty, span)
    }

    fn std_type_def(&self, module: ModId, name: &str) -> Option<DefId> {
        lower::std_type_def(self.globals.res, module, name)
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_sig(
        &mut self,
        file: Option<FileId>,
        std_module: ModId,
        sig: &FnSig_,
        outer: &[Generic],
        self_ty: Option<Ty>,
        origin: FnOrigin,
        public: bool,
        module: ModId,
    ) -> FnSig {
        let generics = self.lower_generics(file, std_module, outer, &sig.generics, self_ty.clone());
        let env = TyEnv { file, std_module, generics: &generics, self_ty: self_ty.clone() };
        let mut params = Vec::new();
        let mut receiver = None;
        for p in &sig.params {
            let ty = self.lower_type(&env, &p.ty);
            if p.kind == ParamKind::Receiver {
                let (mode, inner) = match ty {
                    Ty::Ref(true, inner) => (RecvMode::RefMut, *inner),
                    Ty::Ref(false, inner) => (RecvMode::Ref, *inner),
                    other => (RecvMode::Value, other),
                };
                receiver = Some(Receiver { mode, ty: inner });
                continue;
            }
            params.push(Formal {
                name: p.name.name.clone(),
                ty,
                role: p.role,
                kind: p.kind,
                keyword_only: p.keyword_only,
                has_default: p.default.is_some(),
            });
        }
        let ret = self.lower_type(&env, &sig.ret);
        if let Some(file) = file {
            self.check_rc_return(file, sig, &params, receiver.as_ref(), &ret);
        }
        FnSig {
            name: sig.name.name.clone(),
            own_generics: outer.len(),
            generics,
            params,
            receiver,
            ret,
            self_ty,
            origin,
            public,
            module,
            file,
            span: sig.name.span,
        }
    }

    /// E0608: a returned reference must borrow from exactly one parameter (§9.3.2c).
    fn check_rc_return(
        &mut self,
        file: FileId,
        sig: &FnSig_,
        params: &[Formal],
        recv: Option<&Receiver>,
        ret: &Ty,
    ) {
        if !is_rc(ret) {
            return;
        }
        let mut sources = params.iter().filter(|p| is_rc(&p.ty)).count();
        if recv.is_some_and(|r| r.mode != RecvMode::Value || is_rc(&r.ty)) {
            sources += 1;
        }
        if sources != 1 {
            self.error(
                file,
                "E0608",
                sig.ret.span,
                format!(
                    "`{}` reference return chesthondhi, kaani adhi ae parameter nunchi vasthundho cheppalem. Reference parameter okkate undali (ippudu {sources})",
                    sig.name.name
                ),
            );
        }
    }

    fn add_fn(&mut self, sig: FnSig) -> FnId {
        let id = self.globals.fns.len();
        if let Some(file) = sig.file {
            self.globals.fn_by_span.insert((file, sig.span.start), id);
        }
        self.globals.fns.push(sig);
        id
    }

    fn add_inherent(&mut self, head: Head, name: &str, f: FnId) -> bool {
        self.globals.inherent.entry(head).or_default().insert(name.to_string(), f).is_none()
    }

    // ── pass: types ────────────────────────────────────────────────────────

    /// Records and enums first, so signatures can name them in any order.
    fn collect_types(&mut self, file: FileId, ast: &Module) {
        let module = self.module_of_file(file);
        // Generic counts first, so field types can check arity.
        for item in &ast.items {
            let (name, generics) = match &item.kind {
                ItemKind::Record(r) => (&r.name, &r.generics),
                ItemKind::Enum(e) => (&e.name, &e.generics),
                _ => continue,
            };
            let Some(&def) = self.res().modules[module].types.get(&name.name) else { continue };
            if self.res().defs[def].span != name.span {
                continue;
            }
            let generics = generics
                .iter()
                .map(|g| Generic { name: g.name.name.clone(), bounds: Vec::new(), is_const: g.is_const })
                .collect();
            let kind = if matches!(item.kind, ItemKind::Record(_)) {
                AdtKind::Record(Vec::new())
            } else {
                AdtKind::Enum(Vec::new())
            };
            self.globals.adts.insert(
                def,
                Adt {
                    name: name.name.clone(),
                    module,
                    generics,
                    kind,
                    derives: Vec::new(),
                    c_layout: item.annotations.iter().any(|a| a.name.name == "C"),
                    file: Some(file),
                    span: name.span,
                },
            );
        }
        for item in &ast.items {
            let (name, generics) = match &item.kind {
                ItemKind::Record(r) => (&r.name, &r.generics),
                ItemKind::Enum(e) => (&e.name, &e.generics),
                _ => continue,
            };
            let Some(&def) = self.res().modules[module].types.get(&name.name) else { continue };
            if self.res().defs[def].span != name.span {
                continue;
            }
            let generics = self.lower_generics(Some(file), module, &[], generics, None);
            let self_ty = Ty::App(Con::Adt(def), (0..generics.len() as u32).map(Ty::Param).collect());
            let env =
                TyEnv { file: Some(file), std_module: module, generics: &generics, self_ty: Some(self_ty) };
            let kind = match &item.kind {
                ItemKind::Record(r) => AdtKind::Record(
                    r.fields
                        .iter()
                        .map(|f| {
                            let ty = self.lower_type(&env, &f.ty);
                            self.check_stored(file, &ty, f.ty.span);
                            FieldInfo { name: f.name.name.clone(), ty, public: f.public }
                        })
                        .collect(),
                ),
                ItemKind::Enum(e) => AdtKind::Enum(
                    e.variants
                        .iter()
                        .map(|v| VariantInfo {
                            name: v.name.name.clone(),
                            fields: v
                                .fields
                                .iter()
                                .map(|t| {
                                    let ty = self.lower_type(&env, t);
                                    self.check_stored(file, &ty, t.span);
                                    ty
                                })
                                .collect(),
                        })
                        .collect(),
                ),
                _ => unreachable!(),
            };
            let derives = self.derives(file, &item.annotations);
            let adt = self.globals.adts.get_mut(&def).expect("declared above");
            adt.generics = generics;
            adt.kind = kind;
            adt.derives = derives;
        }
    }

    fn derives(&mut self, file: FileId, annotations: &[Annotation]) -> Vec<DefId> {
        let mut out = Vec::new();
        for a in annotations {
            if a.name.name != "automatic" {
                continue;
            }
            if let AnnotationArg::Names(names) = &a.arg {
                for n in names {
                    if let Some(Res::Def(d)) = self.res().resolutions.get(&(file, n.span.start)) {
                        out.push(*d);
                    }
                }
            }
        }
        out
    }

    // ── pass: signatures ───────────────────────────────────────────────────

    fn collect_signatures(&mut self, file: FileId, ast: &Module) {
        let module = self.module_of_file(file);
        for item in &ast.items {
            match &item.kind {
                ItemKind::Fn(f) => {
                    let Some(&def) = self.res().modules[module].values.get(&f.sig.name.name) else {
                        continue;
                    };
                    if self.res().defs[def].span != f.sig.name.span {
                        continue;
                    }
                    let sig = self.lower_sig(
                        Some(file),
                        module,
                        &f.sig,
                        &[],
                        None,
                        FnOrigin::User,
                        item.public,
                        module,
                    );
                    let id = self.add_fn(sig);
                    self.globals.fn_of_def.insert(def, id);
                }
                ItemKind::Extern(block) => {
                    for s in &block.fns {
                        let Some(&def) = self.res().modules[module].values.get(&s.name.name) else {
                            continue;
                        };
                        if self.res().defs[def].span != s.name.span {
                            continue;
                        }
                        let sig = self.lower_sig(
                            Some(file),
                            module,
                            s,
                            &[],
                            None,
                            FnOrigin::Foreign,
                            item.public,
                            module,
                        );
                        let id = self.add_fn(sig);
                        self.globals.fn_of_def.insert(def, id);
                    }
                }
                ItemKind::Const { name, ty, .. } => {
                    let Some(&def) = self.res().modules[module].values.get(&name.name) else { continue };
                    let env = TyEnv { file: Some(file), std_module: module, generics: &[], self_ty: None };
                    let ty = self.lower_type(&env, ty);
                    self.globals.consts.insert(def, ty);
                }
                ItemKind::Trait(t) => {
                    let Some(&def) = self.res().modules[module].types.get(&t.name.name) else { continue };
                    if self.res().defs[def].span != t.name.span {
                        continue;
                    }
                    self.collect_trait(Some(file), module, def, t);
                }
                _ => {}
            }
        }
        // Methods and implementations need every trait's signatures.
        for item in &ast.items {
            match &item.kind {
                ItemKind::Methods(m) => self.collect_methods(file, module, m),
                ItemKind::Impl(i) => self.collect_impl(file, module, i, item.span),
                _ => {}
            }
        }
    }

    fn collect_trait(&mut self, file: Option<FileId>, module: ModId, def: DefId, t: &TraitDecl) {
        let idhi = vec![Generic { name: "idhi".into(), bounds: Vec::new(), is_const: false }];
        let generics = self.lower_generics(file, module, &idhi, &t.generics, Some(Ty::Param(0)));
        let mut methods = Vec::new();
        let mut defaults = HashSet::new();
        for ti in &t.items {
            let sig = self.lower_sig(
                file,
                module,
                &ti.sig,
                &generics,
                Some(Ty::Param(0)),
                if file.is_some() { FnOrigin::User } else { FnOrigin::Std },
                true,
                module,
            );
            let id = self.add_fn(sig);
            methods.push((ti.sig.name.name.clone(), id));
            if ti.default_body.is_some() {
                defaults.insert(ti.sig.name.name.clone());
            }
        }
        self.globals
            .traits
            .insert(def, Trait { name: t.name.name.clone(), generics, methods, defaults, module, file });
    }

    fn collect_methods(&mut self, file: FileId, module: ModId, m: &MethodsDecl) {
        let generics = self.lower_generics(Some(file), module, &[], &m.generics, None);
        let env = TyEnv { file: Some(file), std_module: module, generics: &generics, self_ty: None };
        let self_ty = self.lower_type(&env, &m.self_ty);
        let Some(head) = head_of(&self_ty) else { return };
        if !matches!(head, Head::Con(Con::Adt(d)) if self.globals.adts.get(&d).is_some_and(|a| a.file.is_some()))
        {
            self.error(
                file,
                "E0803",
                m.self_ty.span,
                "`panulu` mana package lo define chesina `rakam`/`edho_okati` ki matrame raayochu",
            );
            return;
        }
        for member in &m.items {
            let ItemKind::Fn(f) = &member.kind else { continue };
            let sig = self.lower_sig(
                Some(file),
                module,
                &f.sig,
                &generics,
                Some(self_ty.clone()),
                FnOrigin::User,
                member.public,
                module,
            );
            let id = self.add_fn(sig);
            if !self.add_inherent(head, &f.sig.name.name, id) {
                self.error(
                    file,
                    "E0302",
                    f.sig.name.span,
                    format!("`{}` method already undhi", f.sig.name.name),
                );
            }
        }
    }

    fn collect_impl(&mut self, file: FileId, module: ModId, i: &ImplDecl, span: Span) {
        let generics = self.lower_generics(Some(file), module, &[], &i.generics, None);
        let env = TyEnv { file: Some(file), std_module: module, generics: &generics, self_ty: None };
        let self_ty = self.lower_type(&env, &i.self_ty);
        let env = TyEnv {
            file: Some(file),
            std_module: module,
            generics: &generics,
            self_ty: Some(self_ty.clone()),
        };
        let Some(trait_ref) = self.lower_trait_ref(&env, &i.trait_ref) else { return };
        let mut methods = HashMap::new();
        for f in &i.items {
            let sig = self.lower_sig(
                Some(file),
                module,
                &f.sig,
                &generics,
                Some(self_ty.clone()),
                FnOrigin::User,
                true,
                module,
            );
            let id = self.add_fn(sig);
            methods.insert(f.sig.name.name.clone(), id);
        }
        self.globals.impls.push(Impl { trait_ref, self_ty, generics, methods, file, span });
    }
}

/// The local signature type `FnSig` is the AST's.
type FnSig_ = crate::ast::FnSig;

/// Reference-carrying types (§9.3.1).
pub fn is_rc(ty: &Ty) -> bool {
    match ty {
        Ty::Ref(..) => true,
        Ty::App(Con::Mukka, _) => true,
        Ty::Tuple(items) => items.iter().any(is_rc),
        Ty::App(Con::Undochu | Con::SareThappu, args) => args.iter().any(is_rc),
        _ => false,
    }
}
