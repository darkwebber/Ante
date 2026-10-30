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
    /// Run `modhalu`; `stream` writes output as it is produced, `args` are
    /// the program's own arguments.
    Run {
        stream: bool,
        args: Vec<String>,
    },
    EmitMir,
    EmitLlvm,
    /// Compile to a native executable at this path (M7); `release`
    /// optimizes.
    Build {
        output: PathBuf,
        release: bool,
    },
}

/// Parsed input for the later stages.
struct Loaded {
    sources: Vec<SourceFile>,
    modules: Vec<(String, FileId, Option<Module>)>,
    diagnostics: Vec<(FileId, Diagnostic)>,
    /// The entry module of a package (`modhalu` or `lib`); `None` for a lone file.
    entry: Option<String>,
    build: BuildSettings,
}

/// The `[build]` table of `ante.toml` (§7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSettings {
    /// Stack size of the thread that runs `modhalu`, in bytes.
    pub stack: u64,
    /// Stack size of every other task (§24.5.4), in bytes.
    pub task_stack: u64,
    /// LLVM target triple; `None` builds for this machine.
    pub target: Option<String>,
    /// C files compiled into the program (§22.6), resolved against the package.
    pub c_sources: Vec<PathBuf>,
    /// Libraries to link (`-l`), and folders to find them in (`-L`).
    pub libs: Vec<String>,
    pub lib_dirs: Vec<PathBuf>,
}

impl BuildSettings {
    /// Whether the target unwinds a trapped task with the C unwinder
    /// (§24.5.2). Windows uses its own exception model; there a trapped
    /// task's values aren't dropped yet.
    pub fn unwinds(&self) -> bool {
        match &self.target {
            Some(t) => !t.contains("windows"),
            None => !cfg!(windows),
        }
    }
}

/// The smallest (and default) `[build] stack`: 8 MB.
pub const MIN_STACK: u64 = 8 << 20;

/// `[build] task_stack`: 1 MB unless set; at least 256 KB (§24.5.4).
pub const DEFAULT_TASK_STACK: u64 = 1 << 20;
const MIN_TASK_STACK: u64 = 256 << 10;

impl Default for BuildSettings {
    fn default() -> Self {
        BuildSettings {
            stack: MIN_STACK,
            task_stack: DEFAULT_TASK_STACK,
            target: None,
            c_sources: Vec::new(),
            libs: Vec::new(),
            lib_dirs: Vec::new(),
        }
    }
}

/// `"64MB"`, `"512 KB"`, `"1GB"` or plain bytes. Units are powers of 1024.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u64 = num.parse().ok()?;
    let mul: u64 = match unit.trim().to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "KB" | "K" => 1 << 10,
        "MB" | "M" => 1 << 20,
        "GB" | "G" => 1 << 30,
        _ => return None,
    };
    n.checked_mul(mul)
}

/// Reads `[build]`, reporting bad values (E0905) at their line. Paths are
/// relative to the package `root`.
fn build_settings(text: &str, root: &Path) -> (BuildSettings, Vec<Diagnostic>) {
    let mut settings = BuildSettings::default();
    let mut diags = Vec::new();
    let at = |key: &str| Span::point(manifest_offset(text, "build", key).unwrap_or(0));
    if let Some(v) = manifest_value(text, "build", "stack") {
        match parse_size(&v) {
            Some(n) if (MIN_STACK..=(64 << 30)).contains(&n) => settings.stack = n,
            Some(_) => diags.push(Diagnostic::error(
                "E0905",
                at("stack"),
                format!("`stack = \"{v}\"`: 8MB nunchi 64GB madhyalo undali"),
            )),
            None => diags.push(Diagnostic::error(
                "E0905",
                at("stack"),
                format!("`stack = \"{v}\"` ardham kaaledhu. `\"64MB\"` laaga raayandi"),
            )),
        }
    }
    if let Some(v) = manifest_value(text, "build", "task_stack") {
        match parse_size(&v) {
            Some(n) if (MIN_TASK_STACK..=(1 << 30)).contains(&n) => settings.task_stack = n,
            _ => diags.push(Diagnostic::error(
                "E0905",
                at("task_stack"),
                format!("`task_stack = \"{v}\"`: 256KB nunchi 1GB madhyalo undali, `\"1MB\"` laaga"),
            )),
        }
    }
    if let Some(v) = manifest_value(text, "build", "target") {
        let arch = v.split('-').next().unwrap_or("");
        if matches!(arch, "x86_64" | "aarch64" | "arm64") && v.split('-').count() >= 3 {
            settings.target = Some(v);
        } else {
            diags.push(Diagnostic::error(
                "E0905",
                at("target"),
                format!(
                    "`target = \"{v}\"` support ledhu. 64-bit x86_64 leka aarch64 triple ivvandi, ex: `\"aarch64-unknown-linux-gnu\"`"
                ),
            ));
        }
    }
    let list = |key: &str, diags: &mut Vec<Diagnostic>| -> Vec<String> {
        let Some(raw) = manifest_raw(text, "build", key) else { return Vec::new() };
        match parse_string_list(&raw) {
            Some(items) => items,
            None => {
                diags.push(Diagnostic::error(
                    "E0905",
                    at(key),
                    format!("`{key} = {raw}` ardham kaaledhu. `[\"a\", \"b\"]` laaga oke line lo raayandi"),
                ));
                Vec::new()
            }
        }
    };
    let inside = |p: &str| {
        let path = Path::new(p);
        !p.is_empty()
            && path.is_relative()
            && path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
    };
    for c in list("c_sources", &mut diags) {
        if !inside(&c) || !c.ends_with(".c") {
            diags.push(Diagnostic::error(
                "E0905",
                at("c_sources"),
                format!("`{c}`: package lopala unna `.c` file kavali (`..` leka `/` tho start avvakoodadhu)"),
            ));
        } else if !root.join(&c).is_file() {
            diags.push(Diagnostic::error("E0905", at("c_sources"), format!("`{c}` file dorakaledhu")));
        } else {
            settings.c_sources.push(root.join(&c));
        }
    }
    for l in list("libs", &mut diags) {
        let ok = !l.is_empty()
            && !l.starts_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'));
        if ok {
            settings.libs.push(l);
        } else {
            diags.push(Diagnostic::error(
                "E0905",
                at("libs"),
                format!("`{l}` library peru kaadhu. `\"sqlite3\"` laaga ivvandi (`-l` lekunda)"),
            ));
        }
    }
    for d in list("lib_dirs", &mut diags) {
        let path = Path::new(&d);
        settings.lib_dirs.push(if path.is_absolute() { path.to_path_buf() } else { root.join(path) });
    }
    (settings, diags)
}

/// `["a", "b"]` on one line, as written in `ante.toml`.
fn parse_string_list(raw: &str) -> Option<Vec<String>> {
    let inner = raw.trim().strip_prefix('[')?.strip_suffix(']')?;
    let mut out = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let item = part.strip_prefix('"')?.strip_suffix('"')?;
        if item.contains('"') || item.contains('\\') {
            return None;
        }
        out.push(item.to_string());
    }
    Some(out)
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
        build: BuildSettings::default(),
    }
}

/// Checks a file on its own, without a package around it.
pub fn check_single(src: &SourceFile) -> Vec<Diagnostic> {
    let loaded = load_single(SourceFile::new(src.name.clone(), src.text.clone()));
    compile(loaded, Action::Check).diagnostics.into_iter().map(|(_, d)| d).collect()
}

/// Checks a lone file and generates its LLVM IR (in `Checked::mir`).
pub fn emit_llvm_source(src: &SourceFile) -> Checked {
    let loaded = load_single(SourceFile::new(src.name.clone(), src.text.clone()));
    compile(loaded, Action::EmitLlvm)
}

/// Checks and runs a lone file's `modhalu`, capturing its output.
pub fn run_source(src: &SourceFile) -> Checked {
    let loaded = load_single(SourceFile::new(src.name.clone(), src.text.clone()));
    compile(loaded, Action::Run { stream: false, args: Vec::new() })
}

/// The stages after parsing. Each runs only when the ones before it found
/// no errors, so one mistake doesn't cascade; warnings are kept only when
/// there are no errors at all.
fn compile(loaded: Loaded, action: Action) -> Checked {
    let Loaded { sources, modules: parsed, mut diagnostics, entry, build } = loaded;
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
    let mut program = crate::mir::build_program(&modules, &tc);
    diagnostics.extend(program.errors.iter().cloned());
    if failed(&diagnostics) {
        return finish(sources, diagnostics, None, None);
    }
    diagnostics.extend(crate::mir::borrow_check(&program, &tc.globals, &sources));
    diagnostics.extend(crate::mir::mono_check(&program, &tc.globals));
    if failed(&diagnostics) {
        return finish(sources, diagnostics, None, None);
    }
    crate::mir::elaborate_drops(&mut program, &tc.globals);
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
            Some(fid) => {
                match crate::codegen::emit_llvm(
                    &program,
                    &tc,
                    &sources,
                    fid,
                    build.stack,
                    build.task_stack,
                    build.unwinds(),
                ) {
                    Err(errs) => backend_errors = errs,
                    Ok(emitted) => match &action {
                        Action::Build { output, release } => {
                            match link_native(&emitted, output, &build, *release) {
                                Ok(()) => built = Some(output.clone()),
                                Err(e) => backend_errors.push(e),
                            }
                        }
                        _ => {
                            let mut text = emitted.ir;
                            if let Some(glue) = emitted.glue {
                                text.push_str("\n; ── FFI glue (C) ──\n");
                                for line in glue.lines() {
                                    text.push_str("; ");
                                    text.push_str(line);
                                    text.push('\n');
                                }
                            }
                            mir = Some(text)
                        }
                    },
                }
            }
        },
        Action::Check => {}
        Action::EmitMir => {
            let names = |fid: usize| tc.globals.fns[fid].name.clone();
            let ty_name = |t: &crate::types::Ty| crate::typeck::show_ty(&tc.globals, t);
            let text: Vec<String> = program.bodies.iter().map(|b| b.render(&names, &ty_name)).collect();
            mir = Some(text.join("\n"));
        }
        Action::Run { stream, args } if needs_native(&program, &tc.globals, &build) => match entry_fn() {
            None => backend_errors.push("`modhalu()` function ledhu, program run cheyyalem".into()),
            Some(fid) => {
                match crate::codegen::emit_llvm(
                    &program,
                    &tc,
                    &sources,
                    fid,
                    build.stack,
                    build.task_stack,
                    build.unwinds(),
                ) {
                    Err(errs) => backend_errors = errs,
                    Ok(emitted) => match run_native(&emitted, &sources, &build, stream, &args) {
                        Ok(out) => run = Some(out),
                        Err(e) => backend_errors.push(e),
                    },
                }
            }
        },
        Action::Run { stream, args } => {
            let module = match &entry {
                Some(e) => resolution.module_by_name.get(e).copied(),
                None => parsed.first().and_then(|(name, _, _)| resolution.module_by_name.get(name).copied()),
            };
            let fid = module
                .and_then(|m| resolution.modules[m].values.get("modhalu").copied())
                .and_then(|d| tc.globals.fn_of_def.get(&d).copied());
            match fid {
                Some(fid) => {
                    let opts = crate::interp::RunOptions { stream, stack: build.stack, args };
                    run = Some(crate::interp::run(&program, &tc, &sources, fid, &opts))
                }
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

/// Whether the program reaches outside what the interpreter can model: C
/// functions, raw memory, or C code of its own. `antec run` builds those
/// natively (§22.6); the semantics are the same either way.
fn needs_native(program: &crate::mir::Program, g: &crate::typeck::Globals, build: &BuildSettings) -> bool {
    use crate::mir::{Callee, Term};
    use crate::typeck::FnOrigin;
    if !build.c_sources.is_empty() || g.fns.iter().any(|f| f.export.is_some()) {
        return true;
    }
    // Tasks and sharing (§24) run on threads, which the interpreter doesn't model.
    let shared = |t: &crate::types::Ty| {
        let mut found = false;
        t.walk(&mut |x| {
            if let crate::types::Ty::App(c, _) = x {
                found |= match c {
                    crate::types::Con::Panchukunna => true,
                    crate::types::Con::Adt(d) => g.task_type(*d).is_some(),
                    _ => false,
                };
            }
        });
        found
    };
    if program.bodies.iter().flat_map(|b| &b.locals).any(|l| shared(&l.ty)) {
        return true;
    }
    program.bodies.iter().flat_map(|b| &b.blocks).any(|block| match &block.term {
        Term::Call { callee: Callee::Fn(fid, tys), .. } => {
            let sig = &g.fns[*fid];
            if g.res.modules[sig.module].name == "task"
                || sig.owner.as_deref() == Some("panchukunna")
                || (g.res.modules[sig.module].name == "samayam" && sig.name == "timer")
            {
                return true;
            }
            match sig.origin {
                FnOrigin::Foreign | FnOrigin::StdUnsafe => true,
                FnOrigin::Std => match (sig.owner.as_deref(), sig.name.as_str()) {
                    (Some("mukka"), "pointer") | (Some("pointer"), "jarugu") => true,
                    (None, "kolpothu_maarchu") => tys.iter().any(|t| matches!(t, crate::types::Ty::Ptr(..))),
                    _ => false,
                },
                _ => false,
            }
        }
        _ => false,
    })
}

/// Builds the program in a private folder and runs it there.
fn run_native(
    emitted: &crate::codegen::Emitted,
    sources: &[SourceFile],
    build: &BuildSettings,
    stream: bool,
    args: &[String],
) -> Result<RunOutput, String> {
    let dir = private_temp_dir()?;
    let result = (|| {
        let exe = dir.join(if cfg!(windows) { "program.exe" } else { "program" });
        link_native(emitted, &exe, build, false)?;
        let report = dir.join("trap.txt");
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(args).env("ANTE_TRAP_REPORT", &report);
        let (stdout, stderr, status) = if stream {
            let status = cmd.status().map_err(|e| format!("program start avvaledhu: {e}"))?;
            (String::new(), String::new(), status)
        } else {
            let out = cmd.output().map_err(|e| format!("program start avvaledhu: {e}"))?;
            (
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
                out.status,
            )
        };
        // A signal (a crash in C code) has no exit code; report it like a trap.
        let exit_code = status.code().unwrap_or(101);
        // `CODE line col file`, written by the runtime when the program trapped.
        let trap = fs::read_to_string(&report).ok().and_then(|text| {
            let mut parts = text.trim_end().splitn(4, ' ');
            let code = parts.next()?.to_string();
            let line = parts.next()?.parse().ok()?;
            let col = parts.next()?.parse().ok()?;
            let file = parts.next().and_then(|f| sources.iter().position(|s| s.name == f));
            Some((code, file, line, col))
        });
        Ok(RunOutput { stdout, stderr, exit_code, trap })
    })();
    let _ = fs::remove_dir_all(&dir);
    result
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

/// Compiles LLVM IR plus the runtime with the C toolchain (`$ANTE_CC`, else
/// `clang`). `release` turns on optimization (§7.4); the runtime itself is
/// always optimized, and is compiled once and cached.
pub fn link_native(
    emitted: &crate::codegen::Emitted,
    output: &Path,
    build: &BuildSettings,
    release: bool,
) -> Result<(), String> {
    if output.is_dir() {
        return Err(format!("`{}` oka folder. `-o` ki file peru ivvandi", output.display()));
    }
    if output.extension().is_some_and(|e| e == "ante" || e == "toml") {
        return Err(format!(
            "`{}` meeda program raasthe source file pothundhi! Vere peru ivvandi (`-o`)",
            output.display()
        ));
    }
    let cc = std::env::var("ANTE_CC").unwrap_or_else(|_| "clang".into());
    let dir = private_temp_dir()?;
    let result = (|| {
        let ll = dir.join("program.ll");
        fs::write(&ll, &emitted.ir).map_err(|e| e.to_string())?;
        let runtime = runtime_object(&cc, build.target.as_deref(), &dir)?;
        // The FFI glue (§22) declares C functions its own way, so clang's
        // idea of library builtins must stay out of it.
        let glue = match &emitted.glue {
            Some(text) => {
                let c = dir.join("ffi_glue.c");
                fs::write(&c, text).map_err(|e| e.to_string())?;
                let o = dir.join("ffi_glue.o");
                let mut cmd = std::process::Command::new(&cc);
                cmd.args(["-c", "-O2", "-w", "-fno-builtin"]);
                if let Some(t) = &build.target {
                    cmd.arg(format!("--target={t}"));
                }
                cmd.arg("-o").arg(&o).arg(&c);
                run_cc(&cc, &mut cmd)?;
                Some(o)
            }
            None => None,
        };
        let mut cmd = std::process::Command::new(&cc);
        cmd.arg(if release { "-O2" } else { "-O0" }).arg("-w").arg("-Wno-override-module");
        if !release {
            // AArch64's default -O0 selector (GlobalISel) is quadratic on long
            // functions that reuse constants; the classic one is linear.
            cmd.args(["-mllvm", "-global-isel=0"]);
        }
        if let Some(t) = &build.target {
            cmd.arg(format!("--target={t}"));
        }
        cmd.arg("-o").arg(output).arg(&ll).arg(&runtime);
        cmd.args(&glue);
        cmd.args(&build.c_sources);
        for d in &build.lib_dirs {
            cmd.arg("-L").arg(d);
        }
        for l in &build.libs {
            cmd.arg(format!("-l{l}"));
        }
        if !cfg!(windows) {
            cmd.arg("-lm").arg("-pthread");
        }
        run_cc(&cc, &mut cmd)
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn run_cc(cc: &str, cmd: &mut std::process::Command) -> Result<(), String> {
    match cmd.output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!("`{cc}` fail ayyindhi:\n{}", String::from_utf8_lossy(&out.stderr))),
        Err(e) => Err(format!(
            "`{cc}` dorakaledhu ({e}). Native build ki clang kavali: macOS lo Xcode tools, Linux lo `clang` package, Windows lo LLVM install cheyyandi; leka `ANTE_CC` set cheyyandi"
        )),
    }
}

/// A new folder that only this user can read, with a name nobody can guess
/// ahead of time; creating it fails rather than reusing one that exists.
fn private_temp_dir() -> Result<PathBuf, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let base = std::env::temp_dir();
    let mut last = String::new();
    for attempt in 0..16u32 {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let seed = format!("{}-{nanos}-{attempt}-{:p}", std::process::id(), &last);
        let dir = base.join(format!("antec-{:016x}", fnv1a(seed.as_bytes())));
        #[cfg(unix)]
        let builder = {
            let mut b = fs::DirBuilder::new();
            std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
            b
        };
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("temp folder create cheyyalem: {last}"))
}

/// The runtime compiled to an object file, from the cache when possible.
/// The cache key covers the runtime source, the compiler and the target.
fn runtime_object(cc: &str, target: Option<&str>, scratch: &Path) -> Result<PathBuf, String> {
    let version = cc_version(cc);
    let key = fnv1a(format!("{}\0{cc}\0{version}\0{target:?}", crate::codegen::RUNTIME_C).as_bytes());
    let compile = |obj: &Path| -> Result<(), String> {
        let src = scratch.join("ante_rt.c");
        fs::write(&src, crate::codegen::RUNTIME_C).map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(cc);
        cmd.arg("-O2").arg("-w").arg("-c");
        // Cleanups in the runtime run when a trapped task unwinds through
        // it (§24.5.2); Windows doesn't unwind that way.
        if !target.map_or(cfg!(windows), |t| t.contains("windows")) {
            cmd.arg("-fexceptions");
        }
        if let Some(t) = target {
            cmd.arg(format!("--target={t}"));
        }
        cmd.arg("-o").arg(obj).arg(&src);
        run_cc(cc, &mut cmd)
    };
    let Some(cache) = cache_dir() else {
        let obj = scratch.join("ante_rt.o");
        compile(&obj)?;
        return Ok(obj);
    };
    let obj = cache.join(format!("ante_rt-{key:016x}.o"));
    if obj.is_file() {
        return Ok(obj);
    }
    let fresh = scratch.join("ante_rt.o");
    compile(&fresh)?;
    // Copy next to the final name, then rename: other builds never see half a file.
    static PARTIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = PARTIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let partial = cache.join(format!("ante_rt-{key:016x}.{}-{n}.tmp", std::process::id()));
    if fs::create_dir_all(&cache).is_ok()
        && fs::copy(&fresh, &partial).is_ok()
        && fs::rename(&partial, &obj).is_ok()
    {
        return Ok(obj);
    }
    let _ = fs::remove_file(&partial);
    Ok(fresh)
}

/// `cc --version`, asked once per compiler per process (it's part of the
/// runtime's cache key, and starting a process for every build is slow).
fn cc_version(cc: &str) -> String {
    static SEEN: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
    if let Some((_, v)) = SEEN.lock().ok().and_then(|s| s.iter().find(|(c, _)| c == cc).cloned()) {
        return v;
    }
    let version = std::process::Command::new(cc)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    if let Ok(mut s) = SEEN.lock() {
        s.push((cc.to_string(), version.clone()));
    }
    version
}

/// `$ANTE_CACHE_DIR`, else the platform's per-user cache folder.
fn cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ANTE_CACHE_DIR") {
        return Some(PathBuf::from(d));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.map(|b| b.join("antec"))
}

/// FNV-1a: a stable 64-bit hash (std's hasher may change between releases).
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
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

/// The byte offset of `key` in a `[section]` of a manifest.
fn manifest_offset(text: &str, section: &str, key: &str) -> Option<usize> {
    let mut current = String::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let code = line.split('#').next().unwrap_or("").trim();
        if let Some(name) = code.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_string();
        } else if current == section && code.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            return Some(offset + line.len() - line.trim_start().len());
        }
        offset += line.len();
    }
    None
}

/// Reads `key = "value"` from a `[section]` of a TOML manifest. Only the few
/// keys `antec` needs are read, so this stays a tiny line-based reader.
pub fn manifest_value(text: &str, section: &str, key: &str) -> Option<String> {
    manifest_raw(text, section, key).map(|v| v.trim_matches('"').to_string())
}

/// The text after `key =`, unparsed.
fn manifest_raw(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_string();
        } else if current == section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_string());
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
    let (build, build_diags) = build_settings(&manifest_text, root);
    diagnostics.extend(build_diags.into_iter().map(|d| (0, d)));
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
    Ok(Loaded { sources, modules: parsed, diagnostics, entry: Some(entry.to_string()), build })
}
