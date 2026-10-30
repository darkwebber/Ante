//! Lowering types written in source to semantic types (spec §9).

use super::{is_rc, Generic, Globals, TraitRef, TyEnv};
use crate::ast::*;
use crate::diag::Diagnostic;
use crate::resolve::{DefId, DefKind, FileId, ModId, Res, Resolution};
use crate::source::Span;
use crate::types::{Con, IntTy, Ty};

pub fn std_type_def(res: &Resolution, module: ModId, name: &str) -> Option<DefId> {
    res.modules[module].types.get(name).or_else(|| res.modules[0].types.get(name)).copied()
}

pub struct Lower<'s, 'g> {
    pub g: &'s Globals<'g>,
    pub diags: &'s mut Vec<(FileId, Diagnostic)>,
}

impl Lower<'_, '_> {
    fn error(&mut self, file: FileId, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push((file, Diagnostic::error(code, span, message)));
    }

    /// The definition a source path names, following module segments.
    /// Returns the definition and how many segments it used.
    pub fn path_def(&self, file: FileId, path: &Path) -> Option<(DefId, usize)> {
        let mut used = 0;
        let mut def = None;
        for seg in &path.segments {
            match self.g.res.resolutions.get(&(file, seg.span.start)) {
                Some(Res::Def(d)) => {
                    def = Some(*d);
                    used += 1;
                    if !matches!(self.g.res.defs[*d].kind, DefKind::Module(_)) {
                        break;
                    }
                }
                _ => break,
            }
        }
        def.map(|d| (d, used))
    }

    // ── lowering source types ──────────────────────────────────────────────

    pub fn lower_type(&mut self, env: &TyEnv, t: &Type) -> Ty {
        match &t.kind {
            TypeKind::Ref { mutable, inner } => Ty::reference(*mutable, self.lower_type(env, inner)),
            TypeKind::Ptr { mutable, inner } => Ty::Ptr(*mutable, Box::new(self.lower_type(env, inner))),
            TypeKind::Array { elem, len } => {
                let elem = self.lower_type(env, elem);
                let n = match &len.kind {
                    ExprKind::Int(lit) => u64::from_str_radix(&lit.digits, lit.radix).ok(),
                    _ => None,
                };
                if let Some(file) = env.file {
                    self.check_stored(file, &elem, t.span);
                }
                Ty::Array(Box::new(elem), n)
            }
            TypeKind::Fn { params, ret } => Ty::Fn(
                params.iter().map(|(p, r)| (self.lower_type(env, p), *r)).collect(),
                Box::new(self.lower_type(env, ret)),
            ),
            TypeKind::Named { path, args } => self.lower_named(env, path, args, t.span),
        }
    }

    pub fn lower_named(&mut self, env: &TyEnv, path: &Path, args: &[Type], span: Span) -> Ty {
        let args: Vec<Ty> = args.iter().map(|a| self.lower_type(env, a)).collect();
        let name = path.segments[0].name.as_str();
        if path.segments.len() == 1 {
            if let Some(i) = env.generics.iter().rposition(|g| g.name == name) {
                return Ty::Param(i as u32);
            }
            if name == "idhi" {
                return env.self_ty.clone().unwrap_or(Ty::Error);
            }
            if let Some(t) = self.builtin_type(env, name, &args, span) {
                return t;
            }
        }
        let def = match env.file {
            Some(file) => match self.path_def(file, path) {
                Some((d, used)) if used == path.segments.len() => Some(d),
                Some((d, _)) if self.is_open_module(d) => return Ty::Error,
                _ => None,
            },
            // A standard file may name another standard module's type, as
            // `samayam.Kaalam`; it exists when the program imports that module.
            None if path.segments.len() == 2 => self
                .g
                .res
                .module_by_name
                .get(&path.segments[0].name)
                .and_then(|&m| self.g.res.modules[m].types.get(&path.segments[1].name).copied()),
            None => self.std_type_def(env.std_module, name),
        };
        let Some(def) = def else {
            if name == "Pravaham" && env.file.is_none() {
                return self.arity(env, Ty::App(Con::Pravaham, args), 1, name, span);
            }
            return Ty::Error;
        };
        if let Some(&con) = self.g.std_cons.get(&def) {
            let n = if con == Con::Gumpu || con == Con::Paridhi { 1 } else { 2 };
            return self.arity(env, Ty::App(con, args), n, name, span);
        }
        match self.g.res.defs[def].kind {
            DefKind::Record | DefKind::Enum | DefKind::StdType => {
                let n = self.adt_generic_count(def);
                self.arity(env, Ty::App(Con::Adt(def), args), n, name, span)
            }
            DefKind::Module(_) => Ty::Error,
            _ => {
                if let Some(file) = env.file {
                    self.error(file, "E0401", span, format!("`{name}` type kaadhu"));
                }
                Ty::Error
            }
        }
    }

    pub fn is_open_module(&self, d: DefId) -> bool {
        matches!(self.g.res.defs[d].kind, DefKind::Module(m) if self.g.res.modules[m].open)
    }

    pub fn std_type_def(&self, module: ModId, name: &str) -> Option<DefId> {
        std_type_def(self.g.res, module, name)
    }

    pub fn adt_generic_count(&self, def: DefId) -> usize {
        self.g.adts.get(&def).map_or(0, |a| a.generics.len())
    }

    /// Checks the number of type arguments.
    pub fn arity(&mut self, env: &TyEnv, ty: Ty, want: usize, name: &str, span: Span) -> Ty {
        let got = match &ty {
            Ty::App(_, args) => args.len(),
            _ => 0,
        };
        if got != want {
            if let Some(file) = env.file {
                self.error(
                    file,
                    "E0401",
                    span,
                    format!("`{name}` ki {want} type argument(s) kavali, {got} ichharu"),
                );
            }
            return Ty::Error;
        }
        if let (Some(file), Ty::App(con, args)) = (env.file, &ty) {
            if matches!(
                con,
                Con::Varusa | Con::Jodi | Con::GajibijiJodi | Con::Gumpu | Con::Pette | Con::Panchukunna
            ) {
                for a in args {
                    self.check_stored(file, a, span);
                }
            }
        }
        ty
    }

    /// E0415: a reference-carrying type can't be stored (§9.3.2b).
    pub fn check_stored(&mut self, file: FileId, ty: &Ty, span: Span) {
        if is_rc(ty) {
            self.error(file,
                "E0415",
                span,
                "reference (`&…`) ni collection lo, field lo, leka `pette` lo store cheyyalem. Owned value vaadandi",
            );
        }
    }

    pub fn builtin_type(&mut self, env: &TyEnv, name: &str, args: &[Ty], span: Span) -> Option<Ty> {
        let scalar = |t: Ty| Some(t);
        match name {
            "ankam" | "ankam64" => scalar(Ty::ANKAM),
            "ankam8" => scalar(Ty::Int(IntTy { signed: true, bits: 8 })),
            "ankam16" => scalar(Ty::Int(IntTy { signed: true, bits: 16 })),
            "ankam32" => scalar(Ty::Int(IntTy { signed: true, bits: 32 })),
            "aasajeevi ankam" | "aasajeevi ankam64" => scalar(Ty::Int(IntTy { signed: false, bits: 64 })),
            "aasajeevi ankam8" => scalar(Ty::Int(IntTy { signed: false, bits: 8 })),
            "aasajeevi ankam16" => scalar(Ty::Int(IntTy { signed: false, bits: 16 })),
            "aasajeevi ankam32" => scalar(Ty::Int(IntTy { signed: false, bits: 32 })),
            "amsam" | "amsam64" => scalar(Ty::Float(64)),
            "amsam32" => scalar(Ty::Float(32)),
            "ato_ito" => scalar(Ty::Bool),
            "aksharam" => scalar(Ty::Char),
            "khali" => scalar(Ty::Unit),
            "raadhu" => scalar(Ty::Never),
            "padham" => scalar(Ty::Str),
            // `baitlu` = `varusa[aasajeevi ankam8]` (§25.1).
            "baitlu" => Some(Ty::App(Con::Varusa, vec![Ty::Int(IntTy { signed: false, bits: 8 })])),
            "jatha" => Some(Ty::Tuple(args.to_vec())),
            "varusa" => Some(self.arity(env, Ty::App(Con::Varusa, args.to_vec()), 1, name, span)),
            "mukka" => Some(self.arity(env, Ty::App(Con::Mukka, args.to_vec()), 1, name, span)),
            "jodi" => Some(self.arity(env, Ty::App(Con::Jodi, args.to_vec()), 2, name, span)),
            "undochu" => Some(self.arity(env, Ty::App(Con::Undochu, args.to_vec()), 1, name, span)),
            "sare_thappu" => Some(self.arity(env, Ty::App(Con::SareThappu, args.to_vec()), 2, name, span)),
            "pette" => Some(self.arity(env, Ty::App(Con::Pette, args.to_vec()), 1, name, span)),
            "panchukunna" => Some(self.arity(env, Ty::App(Con::Panchukunna, args.to_vec()), 1, name, span)),
            _ => None,
        }
    }

    pub fn lower_generics(
        &mut self,
        env_file: Option<FileId>,
        std_module: ModId,
        outer: &[Generic],
        params: &[GenericParam],
        self_ty: Option<Ty>,
    ) -> Vec<Generic> {
        let mut all: Vec<Generic> = outer.to_vec();
        for p in params {
            all.push(Generic { name: p.name.name.clone(), bounds: Vec::new(), is_const: p.is_const });
        }
        for (i, p) in params.iter().enumerate() {
            let mut bounds = Vec::new();
            for b in &p.bounds {
                let env = TyEnv { file: env_file, std_module, generics: &all, self_ty: self_ty.clone() };
                if let Some(tr) = self.lower_trait_ref(&env, b) {
                    bounds.push(tr);
                }
            }
            all[outer.len() + i].bounds = bounds;
        }
        all
    }

    pub fn lower_trait_ref(&mut self, env: &TyEnv, t: &Type) -> Option<TraitRef> {
        let TypeKind::Named { path, args } = &t.kind else { return None };
        let def = match env.file {
            Some(file) => self.path_def(file, path).map(|(d, _)| d),
            None => self.std_type_def(env.std_module, &path.segments[0].name),
        }?;
        if !self.g.res.defs[def].kind.is_trait() {
            if let Some(file) = env.file {
                let name = self.g.res.defs[def].name.clone();
                self.error(file, "E0401", t.span, format!("`{name}` gunam (trait) kaadhu"));
            }
            return None;
        }
        let args = args.iter().map(|a| self.lower_type(env, a)).collect();
        Some(TraitRef { def, args })
    }
}
