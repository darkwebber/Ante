//! Packages (spec §7.1): an `ante.toml` manifest plus `src/`, one module per
//! file. Also checks lone files that aren't part of a package.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::diag::Diagnostic;
use crate::resolve::{self, FileId, SourceModule};
use crate::source::{SourceFile, Span};

/// Every source file read during a check, and the diagnostics for them.
pub struct Checked {
    pub sources: Vec<SourceFile>,
    pub diagnostics: Vec<(FileId, Diagnostic)>,
}

impl Checked {
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|(_, d)| d.is_error()).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.len() - self.error_count()
    }
}

/// The module name for a lone file: its file stem, if that's a valid name.
pub fn module_name_for(file_name: &str) -> String {
    Path::new(file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("main").to_string()
}

/// Checks a file on its own, without a package around it.
pub fn check_single(src: &SourceFile) -> Vec<Diagnostic> {
    let (ast, mut diags) = crate::parse_source(src);
    if diags.iter().any(Diagnostic::is_error) {
        return diags;
    }
    let module = SourceModule { name: module_name_for(&src.name), file: 0, ast: ast.as_ref() };
    let module = &module;
    let resolution = resolve::resolve(std::slice::from_ref(module));
    let clean = resolution.diagnostics.iter().all(|(_, d)| !d.is_error());
    let warnings =
        if clean { crate::lint::lint(std::slice::from_ref(module), &resolution) } else { Vec::new() };
    diags.extend(resolution.diagnostics.into_iter().map(|(_, d)| d));
    diags.extend(warnings.into_iter().map(|(_, d)| d));
    diags.sort_by_key(|d| d.span.start);
    diags
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
    if path.is_dir() {
        return match find_package_root(path) {
            Some(root) => check_package(&root),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("`{}` lo `ante.toml` dorakaledhu. Package root lo run cheyyandi", path.display()),
            )),
        };
    }
    if let Some(root) = find_package_root(path) {
        let in_src = path.canonicalize().is_ok_and(|p| p.starts_with(root.join("src")));
        if in_src {
            return check_package(&root);
        }
    }
    let (src, mut diags) = crate::load_source(path)?;
    if diags.is_empty() {
        diags = check_single(&src);
    }
    Ok(Checked { sources: vec![src], diagnostics: diags.into_iter().map(|d| (0, d)).collect() })
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
        let display = Path::new("src").join(path.strip_prefix(&src_dir).unwrap_or(path));
        let (src, mut diags) = crate::source_from_bytes(display.display().to_string(), fs::read(path)?);
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

    let modules: Vec<SourceModule> = parsed
        .iter()
        .map(|(name, file, ast)| SourceModule { name: name.clone(), file: *file, ast: ast.as_ref() })
        .collect();
    let resolution = resolve::resolve(&modules);
    diagnostics.extend(resolution.diagnostics.iter().cloned());

    // Entry point (§7.5).
    let entry = if kind == "lib" { "lib" } else { "modhalu" };
    match parsed.iter().find(|(name, _, _)| name == entry) {
        None => diagnostics.push((
            0,
            Diagnostic::error(
                "E0903",
                Span::point(0),
                format!(
                    "`src/{entry}.ante` file ledhu. {} ki adhi kavali",
                    if kind == "lib" { "Library" } else { "Executable" }
                ),
            ),
        )),
        Some((_, file, Some(_))) if kind == "bin" => {
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

    if diagnostics.iter().all(|(_, d)| !d.is_error()) {
        diagnostics.extend(crate::lint::lint(&modules, &resolution));
    }
    diagnostics.sort_by_key(|(file, d)| (*file, d.span.start));
    Ok(Checked { sources, diagnostics })
}
