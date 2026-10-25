//! Packages (spec §7.1): an `ante.toml` manifest plus `src/`, one module per
//! file; or a lone file. This module drives every stage after parsing:
//! names, types, lints, MIR, constants, and running the program.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ast::Module;
use crate::diag::Diagnostic;
use crate::interp::RunOutput;
use crate::resolve::{self, FileId, SourceModule};
use crate::source::{SourceFile, Span};

/// Every source file read, and the diagnostics for them.
pub struct Checked {
    pub sources: Vec<SourceFile>,
    pub diagnostics: Vec<(FileId, Diagnostic)>,
    /// The program's result, for `antec run`.
    pub run: Option<RunOutput>,
    /// The MIR (`antec emit mir`) or LLVM IR (`antec emit llvm`) text.
    pub mir: Option<String>,
    /// The native executable written by `antec build`.
    pub built: Option<PathBuf>,
    /// Problems in the native backend or the C toolchain.
    pub backend_errors: Vec<String>,
}

impl Checked {
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|(_, d)| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.len() - self.error_count()
    }
}

/// What to do once the program checks.
#[derive(Clone, PartialEq, Eq)]
pub enum Action {
    Check,
    /// Run `modhalu`; `stream` writes output as it is produced.
    Run {
        stream: bool,
    },
    EmitMir,
    EmitLlvm,
    /// Compile to a native executable at this path (M7).
    Build {
        output: PathBuf,
    },
}

/// Parsed input for the later stages.
struct Loaded {
    sources: Vec<SourceFile>,
    modules: Vec<(String, FileId, Option<Module>)>,
    diagnostics: Vec<(FileId, Diagnostic)>,
    /// The entry module of a package (`modhalu` or `lib`); `None` for a lone file.
    entry: Option<String>,
}

/// The module name for a lone file: its file stem, if that's a valid name.
pub fn module_name_for(file_name: &str) -> String {
    Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("main").to_string()
}

fn load_single(src: SourceFile) -> Loaded {
    let (ast, diags) = crate::parse_source(&src);
    let ast = if diags.iter().any(Diagnostic::is_error) { None } else { ast };
    let name = module_name_for(&src.name);
    Loaded {
        modules: vec![(name, 0, ast)],
        diagnostics: diags.into_iter().map(|d| (0, d)).collect(),
        sources: vec![src],
        entry: None,
    }
}

/// Checks a file on its own, without a package around it.
pub fn check_single(src: &SourceFile) -> Vec<Diagnostic> {
    let loaded = load_single(SourceFile::new(src.name.clone(), src.text.clone()));
    compile(loaded, Action::Check).diagnostics.into_iter().map(|(_, d)| d).collect()
}

/// Checks and runs a lone file's `modhalu`, capturing its output.
pub fn run_source(src: &SourceFile) -> Checked {
    let loaded = load_single(SourceFile::new(src.name.clone(), src.text.clone()));
    compile(loaded, Action::Run { stream: false })
}

/// The stages after parsing. Each runs only when the ones before it found
/// no errors, so one mistake doesn't cascade; warnings are kept only when
/// there are no errors at all.
fn compile(loaded: Loaded, action: Action) -> Checked {
    let Loaded { sources, modules: parsed, mut diagnostics, entry } = loaded;
    let failed = |d: &[(FileId, Diagnostic)]| d.iter().any(|(_, d)| d.is_error());
    if failed(&diagnostics) && entry.is_none() {
        return finish(sources, diagnostics, None, None);
    }
    let modules: Vec<SourceModule> = parsed
        .iter()
        .map(|(name, file, ast)| SourceModule { name: name.clone(), file: *file, ast: ast.as_ref() })
        .collect();
    let resolution = resolve::resolve(&modules);
    diagnostics.extend(resolution.diagnostics.iter().cloned());
    if let Some(entry) = &entry {
        check_entry(entry, &parsed, &resolution, &mut diagnostics);
    }
    if failed(&diagnostics) {
        return finish(sources, diagnostics, None, None);
    }
    let tc = crate::typeck::check(&modules, &resolution);
    diagnostics.extend(tc.diagnostics.iter().cloned());
    if failed(&diagnostics) {
        diagnostics.retain(|(_, d)| d.is_error());
        return finish(sources, diagnostics, None, None);
    }
    let program = crate::mir::build_program(&modules, &tc);
    diagnostics.extend(crate::interp::eval_consts(&program, &tc, &sources));
    if failed(&diagnostics) {
        diagnostics.retain(|(_, d)| d.is_error());
        return finish(sources, diagnostics, None, None);
    }
    diagnostics.extend(crate::lint::lint(&modules, &resolution));
    let mut run = None;
    let mut mir = None;
    let mut built = None;
    let mut backend_errors = Vec::new();
    let entry_fn = || {
        let module = match &entry {
            Some(e) => resolution.module_by_name.get(e).copied(),
            None => parsed.first().and_then(|(name, _, _)| resolution.module_by_name.get(name).copied()),
        };
        module
            .and_then(|m| resolution.modules[m].values.get("modhalu").copied())
            .and_then(|d| tc.globals.fn_of_def.get(&d).copied())
    };
    match action {
        Action::EmitLlvm | Action::Build { .. } => match entry_fn() {
            None => backend_errors.push("`modhalu()` function ledhu, native program build cheyyalem".into()),
            Some(fid) => match crate::codegen::emit_llvm(&program, &tc, &sources, fid) {
                Err(errs) => backend_errors = errs,
                Ok(ir) => match &action {
                    Action::Build { output } => match link_native(&ir, output) {
                        Ok(()) => built = Some(output.clone()),
                        Err(e) => backend_errors.push(e),
                    },
                    _ => mir = Some(ir),
                },
            },
        },
        Action::Check => {}
        Action::EmitMir => {
            let names = |fid: usize| tc.globals.fns[fid].name.clone();
            let text: Vec<String> = program.bodies.iter().map(|b| b.render(&names)).collect();
            mir = Some(text.join("\n"));
        }
        Action::Run { stream } => {
            let module = match &entry {
                Some(e) => resolution.module_by_name.get(e).copied(),
                None => parsed.first().and_then(|(name, _, _)| resolution.module_by_name.get(name).copied()),
            };
            let fid = module
                .and_then(|m| resolution.modules[m].values.get("modhalu").copied())
                .and_then(|d| tc.globals.fn_of_def.get(&d).copied());
            match fid {
                Some(fid) => run = Some(crate::interp::run(&program, &tc, &sources, fid, stream)),
                None => diagnostics.push((
                    0,
                    Diagnostic::error(
                        "E0903",
                        Span::point(0),
                        "`modhalu()` function ledhu. Program ekkada start avvali? 🤔 `modhalu() -> khali ante:` raayandi",
                    ),
                )),
            }
        }
    }
    let mut checked = finish(sources, diagnostics, run, mir);
    checked.built = built;
    checked.backend_errors = backend_errors;
    checked
}

fn finish(
    sources: Vec<SourceFile>,
    mut diagnostics: Vec<(FileId, Diagnostic)>,
    run: Option<RunOutput>,
    mir: Option<String>,
) -> Checked {
    diagnostics.sort_by_key(|(file, d)| (*file, d.span.start));
    Checked { sources, diagnostics, run, mir, built: None, backend_errors: Vec::new() }
}

/// Compiles LLVM IR plus the runtime with the C toolchain (`$ANTE_CC`, else `clang`).
pub fn link_native(ir: &str, output: &Path) -> Result<(), String> {
    let stem = output.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let dir = std::env::temp_dir().join(format!("antec-{}-{stem}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| format!("`{}` create cheyyalem: {e}", dir.display()))?;
    let ll = dir.join("program.ll");
    let rt = dir.join("ante_rt.c");
    fs::write(&ll, ir).map_err(|e| e.to_string())?;
    fs::write(&rt, crate::codegen::RUNTIME_C).map_err(|e| e.to_string())?;
    let cc = std::env::var("ANTE_CC").unwrap_or_else(|_| "clang".into());
    let mut cmd = std::process::Command::new(&cc);
    cmd.arg("-O2").arg("-w").arg("-Wno-override-module").arg("-o").arg(output).arg(&ll).arg(&rt);
    if !cfg!(windows) {
        cmd.arg("-lm");
    }
    let result = cmd.output();
    let _ = fs::remove_dir_all(&dir);
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!("`{cc}` fail ayyindhi:\n{}", String::from_utf8_lossy(&out.stderr))),
        Err(e) => Err(format!(
            "`{cc}` dorakaledhu ({e}). Native build ki clang kavali: macOS lo Xcode tools, Linux lo `clang` package, Windows lo LLVM install cheyyandi; leka `ANTE_CC` set cheyyandi"
        )),
    }
}

/// The entry point of a package (§7.5).
fn check_entry(
    entry: &str,
    parsed: &[(String, FileId, Option<Module>)],
    resolution: &resolve::Resolution,
    diagnostics: &mut Vec<(FileId, Diagnostic)>,
) {
    match parsed.iter().find(|(name, _, _)| name == entry) {
        None => diagnostics.push((
            0,
            Diagnostic::error(
                "E0903",
                Span::point(0),
                format!(
                    "`src/{entry}.ante` file ledhu. {} ki adhi kavali",
                    if entry == "lib" { "Library" } else { "Executable" }
                ),
            ),
        )),
        Some((_, file, Some(_))) if entry == "modhalu" => {
            let module = resolution.module_by_name[entry];
            if !resolution.modules[module].values.contains_key("modhalu") {
                diagnostics.push((
                    *file,
                    Diagnostic::error(
                        "E0903",
                        Span::point(0),
                        "`modhalu()` function ledhu. Program ekkada start avvali? 🤔 `modhalu() -> khali ante:` raayandi",
                    ),
                ));
            }
        }
        Some(_) => {}
    }
}

/// The nearest directory at or above `path` that holds an `ante.toml`.
pub fn find_package_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    let start = if start.as_os_str().is_empty() { PathBuf::from(".") } else { start };
    let start = start.canonicalize().ok()?;
    start.ancestors().find(|dir| dir.join("ante.toml").is_file()).map(Path::to_path_buf)
}

/// Checks a package directory, or a file (as part of its package if it's
/// inside one's `src/`, otherwise on its own).
pub fn check_path(path: &Path) -> io::Result<Checked> {
    process(path, Action::Check)
}

/// Loads `path` (a package or a file) and carries out `action`.
pub fn process(path: &Path, action: Action) -> io::Result<Checked> {
    if path.is_dir() {
        return match find_package_root(path) {
            Some(root) => Ok(compile(load_package(&root)?, action)),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("`{}` lo `ante.toml` dorakaledhu. Package root lo run cheyyandi", path.display()),
            )),
        };
    }
    if let Some(root) = find_package_root(path) {
        let in_src = path.canonicalize().is_ok_and(|p| p.starts_with(root.join("src")));
        if in_src {
            return Ok(compile(load_package(&root)?, action));
        }
    }
    let (src, diags) = crate::load_source(path)?;
    if !diags.is_empty() {
        return Ok(finish(vec![src], diags.into_iter().map(|d| (0, d)).collect(), None, None));
    }
    Ok(compile(load_single(src), action))
}

/// Reads `key = "value"` from a `[section]` of a TOML manifest. Only the few
/// keys `antec` needs are read, so this stays a tiny line-based reader.
pub fn manifest_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_string();
        } else if current == section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}

fn is_module_name(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && crate::token::Keyword::from_word(segment).is_none()
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries: Vec<PathBuf> =
        fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<io::Result<_>>()?;
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("._") || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "ante") {
            out.push(path);
        }
    }
    Ok(())
}

pub fn check_package(root: &Path) -> io::Result<Checked> {
    process_package(root, Action::Check)
}

pub fn process_package(root: &Path, action: Action) -> io::Result<Checked> {
    Ok(compile(load_package(root)?, action))
}

fn load_package(root: &Path) -> io::Result<Loaded> {
    let manifest_path = root.join("ante.toml");
    let manifest_text = fs::read_to_string(&manifest_path)?;
    let mut sources = vec![SourceFile::new("ante.toml", manifest_text.clone())];
    let mut diagnostics: Vec<(FileId, Diagnostic)> = Vec::new();

    let kind = manifest_value(&manifest_text, "package", "kind").unwrap_or_else(|| "bin".into());
    if kind != "bin" && kind != "lib" {
        diagnostics.push((
            0,
            Diagnostic::error(
                "E0905",
                Span::point(0),
                format!("`kind = \"{kind}\"` ardham kaaledhu. `\"bin\"` leka `\"lib\"` ivvandi"),
            ),
        ));
    }

    let src_dir = root.join("src");
    let mut files = Vec::new();
    if src_dir.is_dir() {
        collect_sources(&src_dir, &mut files)?;
    } else {
        diagnostics.push((
            0,
            Diagnostic::error("E0905", Span::point(0), "`src/` folder ledhu. Code `src/` lopala pettandi"),
        ));
    }

    let mut parsed = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&src_dir).unwrap_or(path).with_extension("");
        let segments: Vec<String> = rel.iter().map(|s| s.to_string_lossy().into_owned()).collect();
        // Always `/`, so diagnostics read the same on every OS.
        let display = format!("src/{}.ante", segments.join("/"));
        let (src, mut diags) = crate::source_from_bytes(display, fs::read(path)?);
        let file = sources.len();
        if let Some(bad) = segments.iter().find(|s| !is_module_name(s)) {
            diags.push(Diagnostic::error(
                "E0905",
                Span::point(0),
                format!(
                    "`{bad}` module peru ga valid kaadhu. Letters, digits, `_` matrame; keywords kaakoodadhu"
                ),
            ));
        }
        let ast = if diags.is_empty() {
            let (ast, parse_diags) = crate::parse_source(&src);
            diags = parse_diags;
            if diags.iter().any(Diagnostic::is_error) {
                None
            } else {
                ast
            }
        } else {
            None
        };
        diagnostics.extend(diags.into_iter().map(|d| (file, d)));
        parsed.push((segments.join("."), file, ast));
        sources.push(src);
    }
    let entry = if kind == "lib" { "lib" } else { "modhalu" };
    Ok(Loaded { sources, modules: parsed, diagnostics, entry: Some(entry.to_string()) })
}
