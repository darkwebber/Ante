//! Runs the conformance suite in `tests/conformance` (spec Appendix C).
//!
//! Until later milestones land, only the stages implemented so far run:
//! compile-fail cases must report their expected code and span, and every
//! other category must pass those stages without errors.
//!
//! A test is either one `.ante` file, or a package folder (`ante.toml` +
//! `src/`) whose expectations live in `ante.toml`'s leading comments, with
//! spans written as `CODE @ src/file.ante:line:col`.

use std::fs;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("can't read {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Skip macOS AppleDouble files that exFAT volumes create.
        if name.starts_with("._") {
            continue;
        }
        if path.is_dir() && path.join("ante.toml").is_file() {
            out.push(path);
        } else if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ante") {
            out.push(path);
        }
    }
}

struct Expectations {
    category: String,
    errors: Vec<String>,
    warnings: Vec<String>,
}

fn parse_header(text: &str) -> Expectations {
    let mut category = String::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for line in text.lines() {
        let Some(comment) = line.strip_prefix('#') else { break };
        let comment = comment.trim();
        let normalize = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(value) = comment.strip_prefix("category:") {
            category = value.trim().to_string();
        } else if let Some(value) = comment.strip_prefix("expect-error:") {
            errors.push(normalize(value));
        } else if let Some(value) = comment.strip_prefix("expect-warning:") {
            warnings.push(normalize(value));
        }
    }
    Expectations { category, errors, warnings }
}

/// Runs one test (a file or a package folder) and returns its errors and
/// warnings as `CODE @ [file:]line:col`, plus the text holding its header.
fn run_test(path: &Path) -> (Vec<String>, Vec<String>, String) {
    let (diags, header): (Vec<(bool, String)>, String) = if path.is_dir() {
        let checked = antec::package::check_package(path).expect("readable package test");
        let diags = checked
            .diagnostics
            .iter()
            .map(|(file, d)| {
                let src = &checked.sources[*file];
                let (line, col) = src.line_col(d.span.start);
                (d.is_error(), format!("{} @ {}:{line}:{col}", d.code, src.name))
            })
            .collect();
        (diags, checked.sources[0].text.clone())
    } else {
        let (src, mut diags) = antec::load_source(path).expect("readable test file");
        if diags.is_empty() {
            diags = antec::check_source(&src);
        }
        let diags = diags
            .iter()
            .map(|d| {
                let (line, col) = src.line_col(d.span.start);
                (d.is_error(), format!("{} @ {line}:{col}", d.code))
            })
            .collect();
        (diags, src.text.clone())
    };
    let errors = diags.iter().filter(|(e, _)| *e).map(|(_, s)| s.clone()).collect();
    let warnings = diags.iter().filter(|(e, _)| !*e).map(|(_, s)| s.clone()).collect();
    (errors, warnings, header)
}

#[test]
fn conformance_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "no conformance tests found in {}", root.display());

    let mut failures = Vec::new();
    for path in &files {
        let (errors, warnings, header) = run_test(path);
        let expect = parse_header(&header);
        let name = path.strip_prefix(&root).unwrap_or(path).display();
        match expect.category.as_str() {
            "compile-fail" => {
                if expect.errors.is_empty() {
                    failures.push(format!("{name}: compile-fail test has no expect-error line"));
                }
                for wanted in &expect.errors {
                    if !errors.contains(wanted) {
                        failures.push(format!("{name}: expected {wanted}, found {errors:?}"));
                    }
                }
            }
            "" => failures.push(format!("{name}: missing `# category:` header")),
            _ => {
                if !errors.is_empty() {
                    failures.push(format!("{name}: expected no errors, found {errors:?}"));
                }
            }
        }
        for wanted in &expect.warnings {
            if !warnings.contains(wanted) {
                failures.push(format!("{name}: expected warning {wanted}, found {warnings:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} conformance failures:\n{}", failures.len(), failures.join("\n"));
}

/// Every program in the repository's `examples/` folder must pass the stages
/// implemented so far, so the README's welcome example can't silently rot.
#[test]
fn examples_pass_implemented_stages() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "no examples found in {}", root.display());

    let mut failures = Vec::new();
    for path in &files {
        let (src, mut diags) = antec::load_source(path).expect("readable example");
        if diags.is_empty() {
            diags = antec::check_source(&src);
        }
        for d in diags.iter().filter(|d| d.is_error()) {
            failures.push(d.render(&src));
        }
    }
    assert!(failures.is_empty(), "examples have errors:\n{}", failures.join("\n"));
}

/// `ante` code blocks from the spec and README. Blocks that are only
/// statements are wrapped in a function, since modules hold declarations.
fn doc_examples() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut blocks = Vec::new();
    for doc in ["spec/ante_language_spec_1_0.md", "README.md"] {
        let text = fs::read_to_string(root.join(doc)).expect("readable doc");
        let mut line_no = 0;
        let mut current: Option<(usize, String)> = None;
        for line in text.lines() {
            line_no += 1;
            match &mut current {
                None if line.trim_start() == "```ante" => current = Some((line_no, String::new())),
                Some(_) if line.trim_start() == "```" => {
                    let (start, code) = current.take().expect("open block");
                    blocks.push((format!("{doc}:{start}"), code));
                }
                Some((_, code)) => {
                    code.push_str(line);
                    code.push('\n');
                }
                None => {}
            }
        }
    }
    blocks
}

fn parse_text(name: &str, text: &str) -> Vec<antec::diag::Diagnostic> {
    let src = antec::source::SourceFile::new(name, text);
    antec::parse_source(&src).1
}

fn check_text(name: &str, text: &str) -> Vec<antec::diag::Diagnostic> {
    let src = antec::source::SourceFile::new(name, text);
    antec::check_source(&src)
}

/// Every example in the docs must parse. Many are fragments that mention
/// names they don't declare, so only syntax is checked here.
#[test]
fn doc_examples_parse() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, code) in doc_examples() {
        // Conformance-format snippets that are meant to fail are skipped.
        if code.contains("# expect-error") || code.contains("# invalid") {
            continue;
        }
        checked += 1;
        let mut diags = parse_text(&name, &code);
        let mut text = code.clone();
        if diags.iter().any(|d| d.code == "E0209") {
            text = format!(
                "wrapper() -> khali ante:\n{}",
                code.lines().map(|l| format!("    {l}\n")).collect::<String>()
            );
            diags = parse_text(&name, &text);
        }
        let src = antec::source::SourceFile::new(name.clone(), text);
        for d in diags.iter().filter(|d| d.is_error()) {
            failures.push(d.render(&src));
        }
    }
    assert!(checked >= 10, "expected the spec and README to have examples, found {checked}");
    assert!(failures.is_empty(), "doc examples don't parse:\n{}", failures.join("\n"));
}

/// Complete programs in the docs (spec Appendix D, README) must pass every
/// implemented stage, names included.
#[test]
fn doc_programs_check() {
    let spec =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/ante_language_spec_1_0.md"))
            .expect("readable spec");
    let line_of =
        |heading: &str| spec.lines().position(|l| l.starts_with(heading)).expect("heading exists") + 1;
    let (start, end) = (line_of("# Appendix D"), line_of("# Appendix E"));
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, code) in doc_examples() {
        let (doc, line) = name.rsplit_once(':').expect("doc:line");
        let line: usize = line.parse().expect("line number");
        let is_program = match doc {
            "README.md" => code.contains("modhalu()"),
            _ => (start..end).contains(&line),
        };
        if !is_program {
            continue;
        }
        checked += 1;
        let src = antec::source::SourceFile::new(name.clone(), code.clone());
        for d in check_text(&name, &code).iter().filter(|d| d.is_error()) {
            failures.push(d.render(&src));
        }
    }
    assert!(checked >= 8, "expected Appendix D and README programs, found {checked}");
    assert!(failures.is_empty(), "doc programs have errors:\n{}", failures.join("\n"));
}

/// Parsing every line-prefix of every example must never panic.
#[test]
fn truncated_examples_do_not_panic() {
    for (name, code) in doc_examples() {
        let lines: Vec<&str> = code.lines().collect();
        for n in 1..=lines.len() {
            let prefix = lines[..n].join("\n");
            let _ = check_text(&name, &prefix);
            // Also cut mid-line, at the nearest character boundary.
            let mut cut = prefix.len() / 2;
            while !prefix.is_char_boundary(cut) {
                cut -= 1;
            }
            let _ = check_text(&name, &prefix[..cut]);
        }
    }
}
