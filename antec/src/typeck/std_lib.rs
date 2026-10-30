//! Loads the standard library signatures in `antec/std/` (spec §13.5, §21).
//! They are "Ante" source, parsed once with the compiler's own parser.

use std::sync::OnceLock;

use super::{Adt, AdtKind, Collector, FieldInfo, FnOrigin, Generic, Head, TyEnv, VariantInfo};
use crate::ast::{ItemKind, Module};
use crate::resolve::{DefKind, ModId};
use crate::source::SourceFile;
use crate::types::{Con, Ty};

pub const SOURCES: &[(&str, &str)] = &[
    ("prelude", include_str!("../../std/prelude.ante")),
    ("lekkalu", include_str!("../../std/lekkalu.ante")),
    ("file", include_str!("../../std/file.ante")),
    ("system", include_str!("../../std/system.ante")),
    ("samayam", include_str!("../../std/samayam.ante")),
    ("memory", include_str!("../../std/memory.ante")),
    ("task", include_str!("../../std/task.ante")),
    ("bhadram", include_str!("../../std/bhadram.ante")),
];

pub struct StdFile {
    pub module: &'static str,
    pub ast: Module,
}

/// The parsed standard library. A file that fails to parse is left out;
/// the test suite checks that none do.
pub fn std_files() -> &'static [StdFile] {
    static FILES: OnceLock<Vec<StdFile>> = OnceLock::new();
    FILES.get_or_init(|| {
        SOURCES
            .iter()
            .filter_map(|(module, text)| {
                let src = SourceFile::new(format!("std/{module}.ante"), *text);
                let (ast, diags) = crate::parse_source(&src);
                if diags.iter().any(|d| d.is_error()) {
                    return None;
                }
                ast.map(|ast| StdFile { module, ast })
            })
            .collect()
    })
}

/// The head a `gunam <name>_panulu` block adds methods to.
fn block_head(c: &Collector, module: ModId, name: &str) -> Option<Head> {
    Some(match name {
        "padham" => Head::Str,
        "varusa" => Head::Con(Con::Varusa),
        "mukka" => Head::Con(Con::Mukka),
        "jodi" => Head::Con(Con::Jodi),
        "gajibiji_jodi" => Head::Con(Con::GajibijiJodi),
        "gumpu" => Head::Con(Con::Gumpu),
        "undochu" => Head::Con(Con::Undochu),
        "sare_thappu" => Head::Con(Con::SareThappu),
        "pette" => Head::Con(Con::Pette),
        "panchukunna" => Head::Con(Con::Panchukunna),
        "aksharam" => Head::Char,
        "ankamlu" => Head::Int,
        "amsam64" => Head::Float(64),
        "amsam32" => Head::Float(32),
        "pointer" => Head::Ptr,
        other => Head::Con(Con::Adt(c.std_type_def(module, other)?)),
    })
}

impl Collector<'_> {
    pub(super) fn load_std(&mut self) {
        for file in std_files() {
            let module = if file.module == "prelude" {
                0
            } else {
                match self.res().module_by_name.get(file.module) {
                    Some(&m) => m,
                    None => continue,
                }
            };
            self.load_std_types(module, &file.ast);
        }
        for file in std_files() {
            let module = if file.module == "prelude" { 0 } else { self.res().module_by_name[file.module] };
            self.load_std_items(module, &file.ast);
        }
        // The marker traits (§13.5, §24.3).
        for marker in ["CopyKottochu", "Pampochu", "Panchukovochu"] {
            if let Some(&def) = self.globals.std_traits.get(marker) {
                self.globals.traits.entry(def).or_insert(super::Trait {
                    name: marker.into(),
                    generics: vec![Generic { name: "idhi".into(), bounds: Vec::new(), is_const: false }],
                    methods: Vec::new(),
                    defaults: Default::default(),
                    module: 0,
                    file: None,
                });
            }
        }
    }

    fn load_std_types(&mut self, module: ModId, ast: &Module) {
        for item in &ast.items {
            let (name, kind) = match &item.kind {
                ItemKind::Record(r) => (&r.name.name, Some(r)),
                ItemKind::Enum(e) => (&e.name.name, None),
                _ => continue,
            };
            let Some(def) = self.std_type_def(module, name) else { continue };
            let params = match &item.kind {
                ItemKind::Record(r) => r.generics.clone(),
                ItemKind::Enum(e) => e.generics.clone(),
                _ => Vec::new(),
            };
            let generics = self.lower_generics(None, module, &[], &params, None);
            let env = TyEnv { file: None, std_module: module, generics: &generics, self_ty: None };
            let kind = match (kind, &item.kind) {
                (Some(r), _) => AdtKind::Record(
                    r.fields
                        .iter()
                        .map(|f| FieldInfo {
                            name: f.name.name.clone(),
                            ty: self.lower_type(&env, &f.ty),
                            public: f.public,
                        })
                        .collect(),
                ),
                (None, ItemKind::Enum(e)) => AdtKind::Enum(
                    e.variants
                        .iter()
                        .map(|v| VariantInfo {
                            name: v.name.name.clone(),
                            fields: v.fields.iter().map(|t| self.lower_type(&env, t)).collect(),
                        })
                        .collect(),
                ),
                _ => continue,
            };
            self.globals.adts.insert(
                def,
                Adt {
                    name: name.clone(),
                    module,
                    generics,
                    kind,
                    derives: Vec::new(),
                    c_layout: false,
                    file: None,
                    span: Default::default(),
                },
            );
        }
    }

    fn load_std_items(&mut self, module: ModId, ast: &Module) {
        let origin =
            if self.res().modules[module].name == "memory" { FnOrigin::StdUnsafe } else { FnOrigin::Std };
        for item in &ast.items {
            match &item.kind {
                ItemKind::Trait(t) => {
                    if let Some(name) = t.name.name.strip_suffix("_panulu") {
                        let Some(head) = block_head(self, module, name) else { continue };
                        let idhi = [Generic { name: "idhi".into(), bounds: Vec::new(), is_const: false }];
                        for ti in &t.items {
                            let sig = self.lower_sig(
                                None,
                                module,
                                &ti.sig,
                                &idhi,
                                Some(Ty::Param(0)),
                                origin,
                                true,
                                module,
                            );
                            let mut sig = sig;
                            sig.owner = Some(name.to_string());
                            let id = self.add_fn(sig);
                            self.add_inherent(head, &ti.sig.name.name, id);
                        }
                    } else if let Some(def) = self.std_type_def(module, &t.name.name) {
                        if self.res().defs[def].kind == DefKind::StdTrait {
                            self.collect_trait(None, module, def, t);
                        }
                    }
                }
                ItemKind::Extern(block) => {
                    for s in &block.fns {
                        let Some(&def) = self.res().modules[module].values.get(&s.name.name) else {
                            continue;
                        };
                        let sig = self.lower_sig(None, module, s, &[], None, origin, true, module);
                        let adapter = module == 0
                            && sig.params.first().is_some_and(|p| matches!(p.ty, Ty::App(Con::Pravaham, _)))
                            || s.name.name == "anni_theesuko";
                        let name = sig.name.clone();
                        let id = self.add_fn(sig);
                        self.globals.fn_of_def.insert(def, id);
                        if adapter {
                            self.globals.adapters.insert(name, id);
                        }
                    }
                }
                ItemKind::Const { name, ty, .. } => {
                    let Some(&def) = self.res().modules[module].values.get(&name.name) else { continue };
                    let env = TyEnv { file: None, std_module: module, generics: &[], self_ty: None };
                    let ty = self.lower_type(&env, ty);
                    self.globals.consts.insert(def, ty);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib;

    fn declared(module: &str) -> Vec<String> {
        let file = std_files().iter().find(|f| f.module == module).expect("std file parses");
        let mut names = Vec::new();
        for item in &file.ast.items {
            match &item.kind {
                ItemKind::Extern(b) => names.extend(b.fns.iter().map(|s| s.name.name.clone())),
                ItemKind::Const { name, .. } => names.push(name.name.clone()),
                ItemKind::Record(r) => names.push(r.name.name.clone()),
                ItemKind::Enum(e) => names.push(e.name.name.clone()),
                ItemKind::Trait(t) => names.push(t.name.name.clone()),
                _ => {}
            }
        }
        names
    }

    #[test]
    fn every_std_file_parses() {
        assert_eq!(std_files().len(), SOURCES.len());
    }

    /// The standard library written in "Ante" parses without diagnostics
    /// (the parser recovers, so a mistake wouldn't stop it) and checks
    /// without errors.
    #[test]
    fn ante_source_is_clean() {
        let files = std::iter::once((stdlib::ANTE_SOURCE_FILE.to_string(), stdlib::ANTE_SOURCE))
            .chain(stdlib::ANTE_MODULES.iter().map(|(m, text)| (format!("<ante>/{m}.ante"), *text)));
        for (name, text) in files {
            let src = crate::source::SourceFile::new(name.clone(), text);
            let (_, diags) = crate::parse_source(&src);
            let shown: Vec<String> = diags.iter().map(|d| d.render(&src)).collect();
            assert!(shown.is_empty(), "{}", shown.join(""));
            let formatted = crate::fmt::format_source(&src).ok();
            assert!(formatted.as_deref() == Some(text), "run `antec fmt antec/std/ante` ({name})");
        }
        // Every module part checks, imported by a program that uses nothing.
        let imports: String = stdlib::ANTE_MODULES
            .iter()
            .filter(|(m, _)| !m.contains('/'))
            .map(|(m, _)| format!("{m} thechuko\n"))
            .collect();
        // The comment names a prelude part's word, so the part is checked too.
        let text = format!("{imports}# Raayochu\nx() -> khali ante:\n    ()\n");
        let lib = crate::source::SourceFile::new("lib.ante", text);
        let found = crate::on_big_stack(move || crate::package::check_single(&lib));
        let found: Vec<String> = found.iter().filter(|d| d.is_error()).map(|d| d.message.clone()).collect();
        assert!(found.is_empty(), "{found:?}");
    }

    /// The resolver's name tables and the signature files must agree.
    #[test]
    fn names_have_signatures() {
        let prelude = declared("prelude");
        for name in stdlib::PRELUDE_VALUES {
            assert!(prelude.contains(&name.to_string()), "prelude `{name}` has no signature");
        }
        for name in stdlib::PRELUDE_TRAITS {
            let marker = matches!(*name, "CopyKottochu" | "Pampochu" | "Panchukovochu");
            assert!(marker || prelude.contains(&name.to_string()), "trait `{name}` has no declaration");
        }
        // Modules written wholly in "Ante" may have no signature file.
        for m in
            stdlib::STD_MODULES.iter().filter(|m| !m.open && !(m.values.is_empty() && m.types.is_empty()))
        {
            let names = declared(m.name);
            for name in m.values.iter().chain(m.types) {
                assert!(names.contains(&name.to_string()), "`{}.{name}` has no signature", m.name);
            }
        }
    }
}
