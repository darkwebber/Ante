//! Name resolution (spec §7–8).
//!
//! Runs in passes over every module of a program:
//! 1. collect each module's declarations into its value and type namespaces,
//! 2. resolve imports (E0901, E0303, E0304),
//! 3. collect the methods of `panulu` and `gunam undhi` blocks (for `naa.` pro-drop),
//! 4. walk every signature and body with nested scopes (E0301, E0306, E0308),
//! 5. reject cycles between constants (E0902).
//!
//! What each name refers to is recorded in `Resolution::resolutions`, keyed by
//! the file and start offset of the name, for the type checker to build on.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::stdlib;

pub type FileId = usize;
pub type ModId = usize;
pub type DefId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind {
    Fn,
    Const,
    Record,
    Enum,
    Trait,
    Module(ModId),
    /// Standard library or generated-binding items, known by name only.
    StdValue,
    StdType,
    StdTrait,
}

impl DefKind {
    pub fn is_type(self) -> bool {
        !matches!(self, DefKind::Fn | DefKind::Const | DefKind::StdValue)
    }

    pub fn is_trait(self) -> bool {
        matches!(self, DefKind::Trait | DefKind::StdTrait)
    }
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: String,
    pub kind: DefKind,
    pub module: ModId,
    pub public: bool,
    pub file: Option<FileId>,
    pub span: Span,
}

/// What a name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    /// A parameter, binding or pattern variable, by its declaration span.
    Local(Span),
    Def(DefId),
    /// `naa`, or a field or method of `naa` reached by pro-drop (§13.4.4).
    SelfMember,
    /// A built-in type word or a generic parameter.
    Builtin,
}

pub struct ModuleData {
    pub name: String,
    pub file: Option<FileId>,
    /// Members aren't checked: generated bindings, or a file that failed to parse.
    pub open: bool,
    pub values: HashMap<String, DefId>,
    pub types: HashMap<String, DefId>,
    pub imported_values: HashMap<String, DefId>,
    pub imported_types: HashMap<String, DefId>,
    /// The module's own definition (kind `Module`).
    pub def: DefId,
}

pub struct SourceModule<'a> {
    pub name: String,
    pub file: FileId,
    /// `None` when the file didn't parse; the module is then left open.
    pub ast: Option<&'a Module>,
}

/// A local the program declares (bindings, pattern and closure variables;
/// not function parameters), for lints such as unused bindings.
#[derive(Clone, Debug)]
pub struct LocalDecl {
    pub file: FileId,
    pub name: String,
    pub span: Span,
    /// Declared with `maare` in a binding statement.
    pub mutable: bool,
}

/// A name brought into a module by an import.
#[derive(Clone, Debug)]
pub struct ImportedName {
    pub file: FileId,
    pub name: String,
    pub span: Span,
    pub def: DefId,
}

pub struct Resolution {
    pub diagnostics: Vec<(FileId, Diagnostic)>,
    pub resolutions: HashMap<(FileId, usize), Res>,
    pub defs: Vec<Def>,
    pub modules: Vec<ModuleData>,
    pub module_by_name: HashMap<String, ModId>,
    pub locals: Vec<LocalDecl>,
    pub imports: Vec<ImportedName>,
}

pub fn resolve(sources: &[SourceModule]) -> Resolution {
    let mut r = Resolver::new();
    r.run(sources);
    Resolution {
        diagnostics: r.diags,
        resolutions: r.resolutions,
        defs: r.defs,
        modules: r.modules,
        module_by_name: r.module_by_name,
        locals: r.locals,
        imports: r.imports,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ns {
    Value,
    Type,
}

struct Scope {
    names: HashMap<String, Span>,
    /// Names bound somewhere in this block, to tell E0306 from E0301.
    later: HashSet<String>,
}

const KEYWORD_CTORS: &[&str] = &["undhi", "ledhu", "sare", "thappu"];

fn is_keyword_ctor(path: &Path) -> bool {
    path.segments.len() == 1 && KEYWORD_CTORS.contains(&path.segments[0].name.as_str())
}

fn path_text(path: &Path) -> String {
    path.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

struct Resolver {
    defs: Vec<Def>,
    modules: Vec<ModuleData>,
    module_by_name: HashMap<String, ModId>,
    prelude: ModId,
    variants: HashMap<DefId, Vec<String>>,
    fields: HashMap<DefId, Vec<String>>,
    members: HashMap<DefId, HashSet<String>>,
    open_items: HashMap<(ModId, String), DefId>,
    diags: Vec<(FileId, Diagnostic)>,
    resolutions: HashMap<(FileId, usize), Res>,
    const_deps: HashMap<DefId, Vec<DefId>>,

    // State for the item being resolved.
    file: FileId,
    module: ModId,
    scopes: Vec<Scope>,
    generics: Vec<HashSet<String>>,
    self_def: Option<DefId>,
    in_impl: bool,
    receiver: bool,
    public_sig: bool,
    current_const: Option<DefId>,
    locals: Vec<LocalDecl>,
    imports: Vec<ImportedName>,
    /// Whether `declare` records the local for lints (off for parameters).
    track_locals: bool,
    /// Whether the binding being declared uses `maare`.
    binding_mutable: bool,
}

impl Resolver {
    fn new() -> Self {
        let mut r = Resolver {
            defs: Vec::new(),
            modules: Vec::new(),
            module_by_name: HashMap::new(),
            prelude: 0,
            variants: HashMap::new(),
            fields: HashMap::new(),
            members: HashMap::new(),
            open_items: HashMap::new(),
            diags: Vec::new(),
            resolutions: HashMap::new(),
            const_deps: HashMap::new(),
            file: 0,
            module: 0,
            scopes: Vec::new(),
            generics: Vec::new(),
            self_def: None,
            in_impl: false,
            receiver: false,
            public_sig: false,
            current_const: None,
            locals: Vec::new(),
            imports: Vec::new(),
            track_locals: true,
            binding_mutable: false,
        };
        let prelude = r.add_module("prelude".into(), None, false);
        r.prelude = prelude;
        for name in stdlib::PRELUDE_VALUES {
            r.add_std(prelude, name, DefKind::StdValue);
        }
        for name in stdlib::PRELUDE_TYPES {
            r.add_std(prelude, name, DefKind::StdType);
        }
        for name in stdlib::PRELUDE_TRAITS {
            r.add_std(prelude, name, DefKind::StdTrait);
        }
        let kramam = r.add_std(prelude, "Kramam", DefKind::Enum);
        r.variants.insert(kramam, stdlib::KRAMAM_VARIANTS.iter().map(|v| v.to_string()).collect());
        for module in stdlib::STD_MODULES {
            let m = r.add_module(module.name.into(), None, module.open);
            r.module_by_name.insert(module.name.into(), m);
            for name in module.values {
                r.add_std(m, name, DefKind::StdValue);
            }
            for name in module.types {
                r.add_std(m, name, DefKind::StdType);
            }
        }
        r
    }

    fn add_def(&mut self, name: &str, kind: DefKind, module: ModId, public: bool, span: Span) -> DefId {
        let file = self.modules.get(module).and_then(|m| m.file);
        self.defs.push(Def { name: name.to_string(), kind, module, public, file, span });
        self.defs.len() - 1
    }

    fn add_module(&mut self, name: String, file: Option<FileId>, open: bool) -> ModId {
        let id = self.modules.len();
        let def = self.defs.len();
        self.defs.push(Def {
            name: name.clone(),
            kind: DefKind::Module(id),
            module: id,
            public: true,
            file,
            span: Span::default(),
        });
        self.modules.push(ModuleData {
            name,
            file,
            open,
            values: HashMap::new(),
            types: HashMap::new(),
            imported_values: HashMap::new(),
            imported_types: HashMap::new(),
            def,
        });
        id
    }

    fn add_std(&mut self, module: ModId, name: &str, kind: DefKind) -> DefId {
        let d = self.add_def(name, kind, module, true, Span::default());
        let map =
            if kind.is_type() { &mut self.modules[module].types } else { &mut self.modules[module].values };
        map.insert(name.to_string(), d);
        d
    }

    fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push((self.file, Diagnostic::error(code, span, message)));
    }

    fn record(&mut self, span: Span, res: Res) {
        self.resolutions.insert((self.file, span.start), res);
    }

    fn run(&mut self, sources: &[SourceModule]) {
        let mut ids = Vec::new();
        for source in sources {
            let m = self.add_module(source.name.clone(), Some(source.file), source.ast.is_none());
            self.module_by_name.insert(source.name.clone(), m);
            ids.push(m);
        }
        let parsed: Vec<(ModId, FileId, &Module)> =
            sources.iter().zip(&ids).filter_map(|(s, &m)| s.ast.map(|ast| (m, s.file, ast))).collect();
        for &(m, file, ast) in &parsed {
            self.enter(m, file);
            self.collect_decls(ast);
        }
        for &(m, file, ast) in &parsed {
            self.enter(m, file);
            self.resolve_imports(ast);
        }
        for &(m, file, ast) in &parsed {
            self.enter(m, file);
            self.collect_members(ast);
        }
        for &(m, file, ast) in &parsed {
            self.enter(m, file);
            self.resolve_items(ast);
        }
        self.check_const_cycles();
    }

    fn enter(&mut self, module: ModId, file: FileId) {
        self.module = module;
        self.file = file;
    }

    // ── pass 1: declarations ───────────────────────────────────────────────

    fn declare_item(&mut self, ident: &Ident, kind: DefKind, public: bool, ns: Ns) -> Option<DefId> {
        let module = &self.modules[self.module];
        let map = if ns == Ns::Value { &module.values } else { &module.types };
        if map.contains_key(&ident.name) {
            if ident.name == "modhalu" && kind == DefKind::Fn {
                self.error("E0904", ident.span, "`modhalu()` okkate undali. Program ki oke start point 🙂");
            } else {
                self.error(
                    "E0302",
                    ident.span,
                    format!("`{}` ee module lo already define ayyindhi. Vere peru pettandi", ident.name),
                );
            }
            return None;
        }
        let d = self.add_def(&ident.name, kind, self.module, public, ident.span);
        let module = &mut self.modules[self.module];
        let map = if ns == Ns::Value { &mut module.values } else { &mut module.types };
        map.insert(ident.name.clone(), d);
        Some(d)
    }

    fn check_unique<'a>(&mut self, names: impl Iterator<Item = &'a Ident>, what: &str) -> Vec<String> {
        let mut seen = Vec::new();
        for ident in names {
            if seen.contains(&ident.name) {
                self.error("E0302", ident.span, format!("`{}` ane {what} rendu saarlu undhi", ident.name));
            } else {
                seen.push(ident.name.clone());
            }
        }
        seen
    }

    fn collect_decls(&mut self, ast: &Module) {
        for item in &ast.items {
            match &item.kind {
                ItemKind::Fn(f) => {
                    self.declare_item(&f.sig.name, DefKind::Fn, item.public, Ns::Value);
                }
                ItemKind::Const { name, .. } => {
                    self.declare_item(name, DefKind::Const, item.public, Ns::Value);
                }
                ItemKind::Record(r) => {
                    let names = self.check_unique(r.fields.iter().map(|f| &f.name), "field");
                    if let Some(d) = self.declare_item(&r.name, DefKind::Record, item.public, Ns::Type) {
                        self.fields.insert(d, names);
                    }
                }
                ItemKind::Enum(e) => {
                    let names = self.check_unique(e.variants.iter().map(|v| &v.name), "variant");
                    if let Some(d) = self.declare_item(&e.name, DefKind::Enum, item.public, Ns::Type) {
                        self.variants.insert(d, names);
                    }
                }
                ItemKind::Trait(t) => {
                    self.declare_item(&t.name, DefKind::Trait, item.public, Ns::Type);
                }
                ItemKind::Extern(block) => {
                    for sig in &block.fns {
                        self.declare_item(&sig.name, DefKind::Fn, item.public, Ns::Value);
                    }
                }
                ItemKind::Import(_) | ItemKind::Methods(_) | ItemKind::Impl(_) => {}
            }
        }
    }

    // ── pass 2: imports ────────────────────────────────────────────────────

    fn find_module(&self, path: &Path) -> Option<ModId> {
        if let Some(&m) = self.module_by_name.get(&path_text(path)) {
            return Some(m);
        }
        let first = self.module_by_name.get(&path.segments[0].name)?;
        self.modules[*first].open.then_some(*first)
    }

    /// Looks up `name` inside module `m`, checking visibility (E0303).
    fn module_member(&mut self, m: ModId, ident: &Ident, missing_code: &'static str) -> Option<DefId> {
        if self.modules[m].open {
            if let Some(&d) = self.open_items.get(&(m, ident.name.clone())) {
                return Some(d);
            }
            let starts_upper = ident.name.chars().next().is_some_and(char::is_uppercase);
            let kind = if starts_upper { DefKind::StdType } else { DefKind::StdValue };
            let d = self.add_def(&ident.name, kind, m, true, Span::default());
            self.open_items.insert((m, ident.name.clone()), d);
            return Some(d);
        }
        let module = &self.modules[m];
        let found = module.values.get(&ident.name).or_else(|| module.types.get(&ident.name)).copied();
        let Some(d) = found else {
            let module_name = self.modules[m].name.clone();
            self.error(missing_code, ident.span, format!("`{module_name}` module lo `{}` ledhu", ident.name));
            return None;
        };
        if self.defs[d].module != self.module && !self.defs[d].public {
            let module_name = self.modules[m].name.clone();
            self.error(
                "E0303",
                ident.span,
                format!(
                    "`{}` `{module_name}` module lo private. Bayata vaadalante daaniki `andhariki` pettandi",
                    ident.name
                ),
            );
        }
        Some(d)
    }

    fn bind_import(&mut self, ident: &Ident, d: DefId) {
        let ns = if self.defs[d].kind.is_type() { Ns::Type } else { Ns::Value };
        let module = &self.modules[self.module];
        let (own, imported) = match ns {
            Ns::Value => (&module.values, &module.imported_values),
            Ns::Type => (&module.types, &module.imported_types),
        };
        if own.contains_key(&ident.name) || imported.contains_key(&ident.name) {
            self.error(
                "E0302",
                ident.span,
                format!(
                    "`{}` ee module lo already undhi, import clash avthondhi. `ni … ga` tho alias pettandi",
                    ident.name
                ),
            );
            return;
        }
        let module = &mut self.modules[self.module];
        let map = if ns == Ns::Value { &mut module.imported_values } else { &mut module.imported_types };
        map.insert(ident.name.clone(), d);
        self.record(ident.span, Res::Def(d));
        self.imports.push(ImportedName {
            file: self.file,
            name: ident.name.clone(),
            span: ident.span,
            def: d,
        });
    }

    fn resolve_imports(&mut self, ast: &Module) {
        for item in &ast.items {
            let ItemKind::Import(import) = &item.kind else { continue };
            let Some(target) = self.find_module(&import.module) else {
                self.error(
                    "E0901",
                    import.module.span,
                    format!(
                        "`{}` ane module ledhu. Spelling, leka `src/` lo file undho choodandi",
                        path_text(&import.module)
                    ),
                );
                continue;
            };
            let module_def = self.modules[target].def;
            match &import.kind {
                ImportKind::Module => {
                    let last = import.module.segments.last().expect("non-empty path").clone();
                    self.bind_import(&last, module_def);
                }
                ImportKind::Alias(alias) => self.bind_import(alias, module_def),
                ImportKind::Items(names) => {
                    for name in names {
                        if let Some(d) = self.module_member(target, name, "E0304") {
                            self.bind_import(name, d);
                        }
                    }
                }
            }
        }
    }

    // ── pass 3: members for pro-drop ───────────────────────────────────────

    /// The local or imported type a `Type` names, without reporting errors.
    fn type_def_quiet(&self, t: &Type) -> Option<DefId> {
        let TypeKind::Named { path, .. } = &t.kind else { return None };
        let d = self.lookup_type(&path.segments[0].name)?;
        if path.segments.len() == 1 {
            return Some(d);
        }
        let DefKind::Module(m) = self.defs[d].kind else { return None };
        self.modules[m].types.get(&path.segments[1].name).copied()
    }

    fn collect_members(&mut self, ast: &Module) {
        for item in &ast.items {
            let (self_ty, names): (&Type, Vec<String>) = match &item.kind {
                ItemKind::Methods(m) => (
                    &m.self_ty,
                    m.items
                        .iter()
                        .filter_map(|i| match &i.kind {
                            ItemKind::Fn(f) => Some(f.sig.name.name.clone()),
                            _ => None,
                        })
                        .collect(),
                ),
                ItemKind::Impl(i) => (&i.self_ty, i.items.iter().map(|f| f.sig.name.name.clone()).collect()),
                _ => continue,
            };
            if let Some(d) = self.type_def_quiet(self_ty) {
                self.members.entry(d).or_default().extend(names);
            }
        }
    }

    // ── lookups ────────────────────────────────────────────────────────────

    fn lookup_local(&self, name: &str) -> Option<Span> {
        self.scopes.iter().rev().find_map(|s| s.names.get(name).copied())
    }

    fn declared_later(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.later.contains(name))
    }

    fn lookup_module_value(&self, name: &str) -> Option<DefId> {
        let module = &self.modules[self.module];
        module.values.get(name).or_else(|| module.imported_values.get(name)).copied()
    }

    fn lookup_type(&self, name: &str) -> Option<DefId> {
        let module = &self.modules[self.module];
        module
            .types
            .get(name)
            .or_else(|| module.imported_types.get(name))
            .or_else(|| self.modules[self.prelude].types.get(name))
            .copied()
    }

    fn is_generic(&self, name: &str) -> bool {
        self.generics.iter().any(|g| g.contains(name))
    }

    fn self_has_member(&self, name: &str) -> bool {
        let Some(d) = self.self_def else { return false };
        self.fields.get(&d).is_some_and(|f| f.iter().any(|n| n == name))
            || self.members.get(&d).is_some_and(|m| m.contains(name))
    }

    // ── scopes ─────────────────────────────────────────────────────────────

    fn push_scope(&mut self, later: HashSet<String>) {
        self.scopes.push(Scope { names: HashMap::new(), later });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, ident: &Ident) {
        let scope = self.scopes.last_mut().expect("a scope is open");
        if scope.names.contains_key(&ident.name) {
            self.error(
                "E0308",
                ident.span,
                format!(
                    "`{}` ee scope lo already undhi. Vere peru pettandi, leka lopala block lo shadow cheyyandi",
                    ident.name
                ),
            );
            return;
        }
        scope.names.insert(ident.name.clone(), ident.span);
        self.record(ident.span, Res::Local(ident.span));
        if self.track_locals {
            self.locals.push(LocalDecl {
                file: self.file,
                name: ident.name.clone(),
                span: ident.span,
                mutable: self.binding_mutable,
            });
        }
    }

    fn push_generics(&mut self, params: &[GenericParam]) {
        let mut names = HashSet::new();
        for p in params {
            if !names.insert(p.name.name.clone()) {
                self.error("E0308", p.name.span, format!("`{}` generic rendu saarlu undhi", p.name.name));
            }
        }
        self.generics.push(names);
        for p in params {
            for bound in &p.bounds {
                self.resolve_type(bound);
            }
        }
    }

    // ── pass 4: items and bodies ───────────────────────────────────────────

    /// Annotations on record fields and enum variants: all of them belong to
    /// features that this compiler doesn't implement yet (Part IX, Annex W).
    fn check_member_annotations(&mut self, annotations: &[Annotation]) {
        for a in annotations {
            let name = a.name.name.as_str();
            if stdlib::PLANNED_MEMBER_ANNOTATIONS.contains(&name) {
                self.not_yet(a.name.span, &format!("`@{name}`"));
            } else if stdlib::ANNOTATIONS.contains(&name) || stdlib::PLANNED_ANNOTATIONS.contains(&name) {
                self.error(
                    "E1307",
                    a.name.span,
                    format!("`@{name}` ikkada raayakoodadhu, declaration mundhu matrame"),
                );
            } else {
                self.error("E0301", a.name.span, format!("`@{name}` ane annotation ledhu"));
            }
        }
    }

    /// E1202: defined by the spec, not implemented by `antec` yet.
    fn not_yet(&mut self, span: Span, what: &str) {
        self.error(
            "E1202",
            span,
            format!("{what} spec lo undhi kaani `antec` lo inka raaledhu (Appendix E roadmap choodandi)"),
        );
    }

    fn check_annotations(&mut self, annotations: &[Annotation]) {
        for a in annotations {
            let renamed = stdlib::renamed(&a.name.name).filter(|n| stdlib::ANNOTATIONS.contains(n));
            if let Some(new) = renamed {
                let message = stdlib::renamed_message(&a.name.name, new);
                self.diags.push((self.file, Diagnostic::warning("W0100", a.name.span, message)));
                continue;
            }
            if stdlib::PLANNED_ANNOTATIONS.contains(&a.name.name.as_str()) {
                self.not_yet(a.name.span, &format!("`@{}`", a.name.name));
                continue;
            }
            if stdlib::PLANNED_MEMBER_ANNOTATIONS.contains(&a.name.name.as_str()) {
                self.error(
                    "E1307",
                    a.name.span,
                    format!("`@{}` field leka variant mundhu matrame raayali", a.name.name),
                );
                continue;
            }
            if !stdlib::ANNOTATIONS.contains(&a.name.name.as_str()) {
                self.error("E0301", a.name.span, format!("`@{}` ane annotation ledhu", a.name.name));
                continue;
            }
            if let AnnotationArg::Names(names) = &a.arg {
                for n in names {
                    if stdlib::PLANNED_TRAITS.contains(&n.name.as_str()) {
                        self.not_yet(n.span, &format!("`{}` gunam", n.name));
                        continue;
                    }
                    match self.lookup_type(&n.name) {
                        Some(d) if self.defs[d].kind.is_trait() => self.record(n.span, Res::Def(d)),
                        Some(_) => self.error("E0301", n.span, format!("`{}` gunam (trait) kaadhu", n.name)),
                        None => self.error("E0301", n.span, format!("`{}` ane gunam ledhu", n.name)),
                    }
                }
            }
        }
    }

    fn resolve_items(&mut self, ast: &Module) {
        for item in &ast.items {
            self.check_annotations(&item.annotations);
            match &item.kind {
                ItemKind::Import(_) => {}
                ItemKind::Fn(f) => self.resolve_fn(f, item.public),
                ItemKind::Const { name, ty, value } => {
                    self.public_sig = item.public;
                    self.resolve_type(ty);
                    self.public_sig = false;
                    self.current_const = self.modules[self.module].values.get(&name.name).copied();
                    self.scopes.clear();
                    self.resolve_expr(value);
                    self.current_const = None;
                }
                ItemKind::Record(r) => {
                    self.push_generics(&r.generics);
                    for field in &r.fields {
                        self.check_member_annotations(&field.annotations);
                        self.public_sig = item.public && field.public;
                        self.resolve_type(&field.ty);
                    }
                    self.public_sig = false;
                    self.generics.pop();
                }
                ItemKind::Enum(e) => {
                    self.push_generics(&e.generics);
                    self.public_sig = item.public;
                    for v in &e.variants {
                        self.check_member_annotations(&v.annotations);
                        for t in &v.fields {
                            self.resolve_type(t);
                        }
                    }
                    self.public_sig = false;
                    self.generics.pop();
                }
                ItemKind::Trait(t) => {
                    self.push_generics(&t.generics);
                    self.in_impl = true;
                    for ti in &t.items {
                        self.resolve_sig_and_body(&ti.sig, ti.default_body.as_ref(), item.public);
                    }
                    self.in_impl = false;
                    self.generics.pop();
                }
                ItemKind::Methods(m) => {
                    self.push_generics(&m.generics);
                    self.resolve_type(&m.self_ty);
                    self.self_def = self.type_def_quiet(&m.self_ty);
                    self.in_impl = true;
                    for member in &m.items {
                        self.check_annotations(&member.annotations);
                        if let ItemKind::Fn(f) = &member.kind {
                            self.resolve_fn(f, member.public);
                        }
                    }
                    self.in_impl = false;
                    self.self_def = None;
                    self.generics.pop();
                }
                ItemKind::Impl(i) => {
                    self.push_generics(&i.generics);
                    self.resolve_type(&i.self_ty);
                    self.resolve_type(&i.trait_ref);
                    if let Some(d) = self.type_def_quiet(&i.trait_ref) {
                        if !self.defs[d].kind.is_trait() {
                            let name = self.defs[d].name.clone();
                            self.error("E0301", i.trait_ref.span, format!("`{name}` gunam (trait) kaadhu"));
                        }
                    }
                    self.self_def = self.type_def_quiet(&i.self_ty);
                    self.in_impl = true;
                    for f in &i.items {
                        self.resolve_fn(f, false);
                    }
                    self.in_impl = false;
                    self.self_def = None;
                    self.generics.pop();
                }
                ItemKind::Extern(block) => {
                    for sig in &block.fns {
                        self.resolve_sig_and_body(sig, None, item.public);
                    }
                }
            }
        }
    }

    fn resolve_fn(&mut self, f: &FnDecl, public: bool) {
        self.resolve_sig_and_body(&f.sig, Some(&f.body), public);
    }

    fn resolve_sig_and_body(&mut self, sig: &FnSig, body: Option<&Block>, public: bool) {
        self.scopes.clear();
        self.push_generics(&sig.generics);
        self.receiver = false;
        self.push_scope(HashSet::new());
        self.track_locals = false;
        for p in &sig.params {
            self.public_sig = public;
            self.resolve_type(&p.ty);
            self.public_sig = false;
            if let Some(d) = &p.default {
                self.resolve_expr(d);
            }
            if p.kind == ParamKind::Receiver {
                if !self.in_impl {
                    self.error(
                        "E0301",
                        p.name.span,
                        "`naa` receiver `panulu` leka `gunam` block lo unna functions ki matrame",
                    );
                }
                self.receiver = true;
            } else {
                self.declare(&p.name);
            }
        }
        self.track_locals = true;
        self.public_sig = public;
        self.resolve_type(&sig.ret);
        self.public_sig = false;
        if let Some(body) = body {
            self.resolve_block(body);
        }
        self.pop_scope();
        self.generics.pop();
        self.receiver = false;
    }

    fn resolve_block(&mut self, block: &Block) {
        let later = block
            .stmts
            .iter()
            .flat_map(|s| match &s.kind {
                StmtKind::Binding { target: BindTarget::Name(n), .. } => vec![n.name.clone()],
                StmtKind::Binding { target: BindTarget::Tuple(ns), .. } => {
                    ns.iter().map(|n| n.name.clone()).collect()
                }
                _ => Vec::new(),
            })
            .collect();
        self.push_scope(later);
        for stmt in &block.stmts {
            self.resolve_stmt(stmt);
        }
        self.pop_scope();
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Binding { mutable, target, ty, value } => {
                if let Some(t) = ty {
                    self.resolve_type(t);
                }
                self.resolve_expr(value);
                self.binding_mutable = *mutable;
                match target {
                    BindTarget::Name(n) => self.declare(n),
                    BindTarget::Tuple(names) => {
                        for n in names {
                            self.declare(n);
                        }
                    }
                }
                self.binding_mutable = false;
            }
            StmtKind::Assign { place, value } | StmtKind::Compound { place, value, .. } => {
                self.resolve_expr(place);
                self.resolve_expr(value);
            }
            StmtKind::Return(value) => {
                if let Some(e) = value {
                    self.resolve_expr(e);
                }
            }
            StmtKind::If { branches, else_block } => {
                for (cond, body) in branches {
                    self.resolve_expr(cond);
                    self.resolve_block(body);
                }
                if let Some(body) = else_block {
                    self.resolve_block(body);
                }
            }
            StmtKind::While { cond, body } => {
                self.resolve_expr(cond);
                self.resolve_block(body);
            }
            StmtKind::For { iter, pattern, body } => {
                self.resolve_expr(iter);
                self.push_scope(HashSet::new());
                self.bind_pattern(pattern);
                self.resolve_block(body);
                self.pop_scope();
            }
            StmtKind::Match { scrutinee, arms } => {
                self.resolve_expr(scrutinee);
                for arm in arms {
                    self.push_scope(HashSet::new());
                    self.bind_pattern(&arm.pattern);
                    self.resolve_block(&arm.body);
                    self.pop_scope();
                }
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Defer(body) | StmtKind::Unsafe(body) => self.resolve_block(body),
            StmtKind::Expr(e) => self.resolve_expr(e),
        }
    }

    fn bind_pattern(&mut self, pattern: &Pattern) {
        let mut names = Vec::new();
        self.collect_pattern(pattern, &mut names);
        for n in &names {
            self.declare(n);
        }
    }

    fn collect_pattern(&mut self, pattern: &Pattern, out: &mut Vec<Ident>) {
        match &pattern.kind {
            PatternKind::Wild | PatternKind::Literal(_) | PatternKind::Range { .. } => {}
            PatternKind::Bind { name, .. } => out.push(name.clone()),
            PatternKind::Ctor { path, args } => {
                if !is_keyword_ctor(path) {
                    self.resolve_value_path(path);
                }
                for a in args.iter().flatten() {
                    self.collect_pattern(a, out);
                }
            }
            PatternKind::Record { path, fields, .. } => {
                self.resolve_type_path(path);
                for (name, sub) in fields {
                    match sub {
                        Some(p) => self.collect_pattern(p, out),
                        None => out.push(name.clone()),
                    }
                }
            }
            PatternKind::Tuple(items) => {
                for p in items {
                    self.collect_pattern(p, out);
                }
            }
            PatternKind::Or(alts) => {
                let mut first: Option<(Vec<Ident>, Vec<String>)> = None;
                for alt in alts {
                    let mut names = Vec::new();
                    self.collect_pattern(alt, &mut names);
                    let mut sorted: Vec<String> = names.iter().map(|n| n.name.clone()).collect();
                    sorted.sort();
                    match &first {
                        None => first = Some((names, sorted)),
                        Some((_, expected)) if *expected != sorted => self.error(
                            "E0706",
                            alt.span,
                            "OR-pattern lo prathi alternative same names ni bind cheyyali",
                        ),
                        Some(_) => {}
                    }
                }
                if let Some((names, _)) = first {
                    out.extend(names);
                }
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Ledhu
            | ExprKind::BareSare
            | ExprKind::Unit
            | ExprKind::Hole => {}
            ExprKind::FStr(parts) | ExprKind::Sql(parts) => {
                for part in parts {
                    if let FStrPart::Expr(e) = part {
                        self.resolve_expr(e);
                    }
                }
            }
            ExprKind::Path(path) => self.resolve_value_path(path),
            ExprKind::Call { callee, type_args, phrases } => {
                self.resolve_expr(callee);
                for t in type_args {
                    self.resolve_type(t);
                }
                for phrase in phrases {
                    match &phrase.kind {
                        PhraseKind::Value(e) | PhraseKind::Spread(e) => self.resolve_expr(e),
                        PhraseKind::Hole => {}
                    }
                }
            }
            ExprKind::Try(inner) => self.resolve_expr(inner),
            ExprKind::Pipe { input, stage } => {
                self.resolve_expr(input);
                self.resolve_expr(stage);
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.resolve_expr(lhs);
                self.resolve_expr(rhs);
            }
            ExprKind::Unary { operand, .. } => self.resolve_expr(operand),
            ExprKind::Field { base, .. } | ExprKind::TupleIndex { base, .. } => self.resolve_expr(base),
            ExprKind::Index { base, indices } => {
                self.resolve_expr(base);
                for i in indices {
                    self.resolve_expr(i);
                }
            }
            ExprKind::Range { lo, hi, .. } => {
                self.resolve_expr(lo);
                self.resolve_expr(hi);
            }
            ExprKind::Closure { params, body, .. } => {
                self.push_scope(HashSet::new());
                for p in params {
                    if let Some(t) = &p.ty {
                        self.resolve_type(t);
                    }
                    self.declare(&p.name);
                }
                self.resolve_expr(body);
                self.pop_scope();
            }
            ExprKind::Ctor { path, args } => {
                if !is_keyword_ctor(path) {
                    self.resolve_value_path(path);
                }
                for a in args {
                    self.resolve_expr(a);
                }
            }
            ExprKind::Record { path, fields } => {
                self.resolve_type_path(path);
                for (_, value) in fields {
                    self.resolve_expr(value);
                }
            }
            ExprKind::Tuple(items) | ExprKind::List(items) => {
                for e in items {
                    self.resolve_expr(e);
                }
            }
            ExprKind::Map(pairs) => {
                for (k, v) in pairs {
                    self.resolve_expr(k);
                    self.resolve_expr(v);
                }
            }
        }
    }

    /// Resolves a path in value position (§8, §13.4.4).
    fn resolve_value_path(&mut self, path: &Path) {
        let first = &path.segments[0];
        let name = first.name.as_str();
        if name == "naa" {
            if self.receiver {
                self.record(first.span, Res::SelfMember);
            } else {
                self.error(
                    "E0301",
                    first.span,
                    "`naa` receiver (`naa: &idhi`) unna method lopala matrame vaadochu",
                );
            }
            return;
        }
        if let Some(decl) = self.lookup_local(name) {
            self.record(first.span, Res::Local(decl));
            return;
        }
        if let Some(d) = self.lookup_module_value(name) {
            self.record(first.span, Res::Def(d));
            self.note_const_use(d);
            return;
        }
        if self.receiver && self.self_has_member(name) {
            self.record(first.span, Res::SelfMember);
            return;
        }
        if let Some(&d) = self.modules[self.prelude].values.get(name) {
            self.record(first.span, Res::Def(d));
            return;
        }
        if let Some(d) = self.lookup_type(name) {
            self.record(first.span, Res::Def(d));
            self.resolve_rest(d, &path.segments[1..]);
            return;
        }
        if stdlib::is_builtin_type(name) || self.is_generic(name) {
            self.record(first.span, Res::Builtin);
            return;
        }
        if self.declared_later(name) {
            self.error(
                "E0306",
                first.span,
                format!(
                    "`{name}` ni define cheyyaka mundhe vaadutunnaru. Define chesina line tharvatha vaadandi"
                ),
            );
        } else if self.module_by_name.contains_key(name) {
            self.error(
                "E0301",
                first.span,
                format!("`{name}` module vaadalante mundhu `{name} thechuko` ani import cheyyandi"),
            );
        } else {
            self.error(
                "E0301",
                first.span,
                format!("`{name}` ane peru ekkada ledhu. Spelling check cheyyandi 🔍"),
            );
        }
    }

    fn note_const_use(&mut self, d: DefId) {
        if let Some(c) = self.current_const {
            if self.defs[d].kind == DefKind::Const {
                self.const_deps.entry(c).or_default().push(d);
            }
        }
    }

    /// Resolves the segments after a type or module, returning the final
    /// definition when it is known.
    fn resolve_rest(&mut self, d: DefId, rest: &[Ident]) -> Option<DefId> {
        let Some(seg) = rest.first() else { return Some(d) };
        match self.defs[d].kind {
            DefKind::Module(m) => {
                let member = self.module_member(m, seg, "E0301")?;
                self.record(seg.span, Res::Def(member));
                self.note_const_use(member);
                self.resolve_rest(member, &rest[1..])
            }
            DefKind::Enum => {
                let known = self.variants.get(&d).is_some_and(|vs| vs.contains(&seg.name));
                if !known {
                    let enum_name = self.defs[d].name.clone();
                    self.error(
                        "E0301",
                        seg.span,
                        format!("`{enum_name}` lo `{}` ane variant ledhu", seg.name),
                    );
                }
                None
            }
            // Fields and associated functions are checked with types (M4).
            _ => None,
        }
    }

    fn resolve_type(&mut self, t: &Type) {
        match &t.kind {
            TypeKind::Named { path, args } => {
                self.resolve_type_path(path);
                for a in args {
                    self.resolve_type(a);
                }
            }
            TypeKind::Ref { inner, .. } | TypeKind::Ptr { inner, .. } => self.resolve_type(inner),
            TypeKind::Array { elem, len } => {
                self.resolve_type(elem);
                self.resolve_expr(len);
            }
            TypeKind::Fn { params, ret } => {
                for (p, _) in params {
                    self.resolve_type(p);
                }
                self.resolve_type(ret);
            }
        }
    }

    fn resolve_type_path(&mut self, path: &Path) {
        let first = &path.segments[0];
        let name = first.name.as_str();
        if path.segments.len() == 1 {
            if stdlib::is_builtin_type(name) || self.is_generic(name) {
                self.record(first.span, Res::Builtin);
                return;
            }
            if name == "idhi" {
                if !self.in_impl {
                    self.error(
                        "E0301",
                        first.span,
                        "`idhi` `panulu`, `gunam`, `gunam undhi` blocks lo matrame vaadochu",
                    );
                }
                return;
            }
        }
        let found = self.lookup_type(name).or_else(|| {
            let new = stdlib::renamed(name)?;
            let d = self.lookup_type(new)?;
            let message = stdlib::renamed_message(name, new);
            self.diags.push((self.file, Diagnostic::warning("W0100", first.span, message)));
            Some(d)
        });
        let Some(d) = found else {
            self.error(
                "E0301",
                first.span,
                format!("`{name}` ane type ledhu. Spelling, leka import check cheyyandi"),
            );
            return;
        };
        self.record(first.span, Res::Def(d));
        let last = self.resolve_rest(d, &path.segments[1..]).unwrap_or(d);
        self.check_public_sig(last, path.span);
    }

    fn check_public_sig(&mut self, d: DefId, span: Span) {
        let def = &self.defs[d];
        if self.public_sig
            && def.module == self.module
            && !def.public
            && matches!(def.kind, DefKind::Record | DefKind::Enum | DefKind::Trait)
        {
            let name = def.name.clone();
            self.error(
                "E0303",
                span,
                format!("private type `{name}` ni `andhariki` signature lo vaadalem. `{name}` ki kuda `andhariki` pettandi"),
            );
        }
    }

    // ── pass 5: constant cycles ────────────────────────────────────────────

    fn check_const_cycles(&mut self) {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            New,
            Active,
            Done,
        }
        let mut marks: HashMap<DefId, Mark> = HashMap::new();
        let mut reported: HashSet<DefId> = HashSet::new();
        let mut roots: Vec<DefId> = self.const_deps.keys().copied().collect();
        roots.sort();
        for root in roots {
            let mut stack: Vec<(DefId, usize)> = vec![(root, 0)];
            if marks.get(&root).copied().unwrap_or(Mark::New) != Mark::New {
                continue;
            }
            marks.insert(root, Mark::Active);
            while let Some(&(node, next)) = stack.last() {
                let deps = self.const_deps.get(&node).cloned().unwrap_or_default();
                if next < deps.len() {
                    let dep = deps[next];
                    stack.last_mut().expect("non-empty stack").1 += 1;
                    match marks.get(&dep).copied().unwrap_or(Mark::New) {
                        Mark::New => {
                            marks.insert(dep, Mark::Active);
                            stack.push((dep, 0));
                        }
                        Mark::Active => {
                            let cycle: Vec<DefId> =
                                stack.iter().map(|(n, _)| *n).skip_while(|n| *n != dep).collect();
                            if cycle.iter().all(|c| !reported.contains(c)) {
                                reported.extend(&cycle);
                                let names: Vec<String> =
                                    cycle.iter().map(|c| format!("`{}`", self.defs[*c].name)).collect();
                                let def = &self.defs[dep];
                                let (file, span) = (def.file.unwrap_or(0), def.span);
                                self.diags.push((
                                    file,
                                    Diagnostic::error(
                                        "E0902",
                                        span,
                                        format!(
                                            "constants circle lo tiruguthunnayi: {} → `{}`",
                                            names.join(" → "),
                                            self.defs[dep].name
                                        ),
                                    ),
                                ));
                            }
                        }
                        Mark::Done => {}
                    }
                } else {
                    marks.insert(node, Mark::Done);
                    stack.pop();
                }
            }
        }
    }
}
