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

use antec::package::Action;

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
    stdout: Vec<String>,
    trap: Option<String>,
    exit: Option<i32>,
}

/// `# skip-on: windows`: behavior this platform doesn't have yet (a trapped
/// task's values are dropped by the C unwinder, which Windows doesn't use).
fn skipped_here(path: &Path) -> bool {
    let text = if path.is_dir() {
        fs::read_to_string(path.join("ante.toml")).unwrap_or_default()
    } else {
        fs::read_to_string(path).unwrap_or_default()
    };
    text.lines()
        .take_while(|l| l.starts_with('#'))
        .filter_map(|l| l.strip_prefix('#').map(str::trim).and_then(|l| l.strip_prefix("skip-on:")))
        .any(|os| os.trim() == std::env::consts::OS)
}

fn parse_header(text: &str) -> Expectations {
    let mut e = Expectations {
        category: String::new(),
        errors: Vec::new(),
        warnings: Vec::new(),
        stdout: Vec::new(),
        trap: None,
        exit: None,
    };
    for line in text.lines() {
        let Some(comment) = line.strip_prefix('#') else { break };
        let comment = comment.trim_start();
        let normalize = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(value) = comment.strip_prefix("category:") {
            e.category = value.trim().to_string();
        } else if let Some(value) = comment.strip_prefix("expect-error:") {
            e.errors.push(normalize(value));
        } else if let Some(value) = comment.strip_prefix("expect-warning:") {
            e.warnings.push(normalize(value));
        } else if let Some(value) = comment.strip_prefix("expect-stdout:") {
            e.stdout.push(value.strip_prefix(' ').unwrap_or(value).to_string());
        } else if let Some(value) = comment.strip_prefix("expect-trap:") {
            e.trap = Some(normalize(value));
        } else if let Some(value) = comment.strip_prefix("expect-exit:") {
            e.exit = value.trim().parse().ok();
        }
    }
    e
}

struct Outcome {
    errors: Vec<String>,
    warnings: Vec<String>,
    header: String,
    run: Option<antec::interp::RunOutput>,
    /// Code generation or link errors, for cases that run natively (§22.6).
    backend: Vec<String>,
}

/// Runs one test (a file or a package folder): checks it, and runs it when
/// its category says so. Diagnostics read `CODE @ [file:]line:col`.
fn run_test(path: &Path) -> Outcome {
    let text = if path.is_dir() {
        fs::read_to_string(path.join("ante.toml")).unwrap_or_default()
    } else {
        fs::read_to_string(path).unwrap_or_default()
    };
    let category = parse_header(&text).category;
    let action = if matches!(category.as_str(), "run-pass" | "runtime-trap") {
        Action::Run { stream: false, args: Vec::new(), native: false }
    } else {
        Action::Check
    };
    let checked = antec::on_big_stack(|| antec::package::process(path, action)).expect("readable test");
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (file, d) in &checked.diagnostics {
        let src = &checked.sources[*file];
        let (line, col) = src.line_col(d.span.start);
        let at = if path.is_dir() { format!("{}:{line}:{col}", src.name) } else { format!("{line}:{col}") };
        let entry = format!("{} @ {at}", d.code);
        if d.is_error() {
            errors.push(entry);
        } else {
            warnings.push(entry);
        }
    }
    Outcome { errors, warnings, header: text, run: checked.run, backend: checked.backend_errors }
}

#[test]
fn conformance_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "no conformance tests found in {}", root.display());

    let mut failures = Vec::new();
    for path in &files {
        if skipped_here(path) {
            continue;
        }
        let out = run_test(path);
        let expect = parse_header(&out.header);
        let errors = &out.errors;
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
                // Every error must be expected: a stray one is a bug too.
                for got in errors {
                    if !expect.errors.contains(got) {
                        failures.push(format!(
                            "{name}: unexpected error {got} (expected only {:?})",
                            expect.errors
                        ));
                    }
                }
            }
            "" => failures.push(format!("{name}: missing `# category:` header")),
            category => {
                if !errors.is_empty() {
                    failures.push(format!("{name}: expected no errors, found {errors:?}"));
                } else if let Some(run) = &out.run {
                    check_run(&name.to_string(), category, &expect, run, &mut failures);
                } else if matches!(category, "run-pass" | "runtime-trap") {
                    failures.push(format!("{name}: didn't run: {:?}", out.backend));
                }
            }
        }
        for wanted in &expect.warnings {
            if !out.warnings.contains(wanted) {
                failures.push(format!("{name}: expected warning {wanted}, found {:?}", out.warnings));
            }
        }
    }
    assert!(failures.is_empty(), "{} conformance failures:\n{}", failures.len(), failures.join("\n"));
}

/// Compares a run with `expect-stdout`, `expect-trap` and `expect-exit`.
fn check_run(
    name: &str,
    category: &str,
    expect: &Expectations,
    run: &antec::interp::RunOutput,
    failures: &mut Vec<String>,
) {
    let lines: Vec<&str> = run.stdout.lines().collect();
    if !expect.stdout.is_empty() && lines != expect.stdout {
        failures.push(format!("{name}: stdout {lines:?}, expected {:?}", expect.stdout));
    }
    let want_exit = expect.exit.unwrap_or(if category == "runtime-trap" { 101 } else { 0 });
    if run.exit_code != want_exit {
        failures.push(format!(
            "{name}: exit {} (stderr: {}), expected {want_exit}",
            run.exit_code,
            run.stderr.trim()
        ));
    }
    if let Some(want) = &expect.trap {
        let got = run.trap.as_ref().map(|(code, _, line, col)| format!("{code} @ {line}:{col}"));
        if got.as_deref() != Some(want.as_str()) {
            failures.push(format!("{name}: trap {got:?}, expected {want}"));
        }
    }
}

/// The README's welcome program prints exactly what the README says it does.
#[test]
fn readme_welcome_output_matches() {
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md")).expect("README");
    let code = readme.split("```ante\n").nth(1).and_then(|r| r.split("```").next()).expect("an ante block");
    let expected =
        readme.split("```text\n").nth(1).and_then(|r| r.split("```").next()).expect("a text block");
    let src = antec::source::SourceFile::new("welcome.ante", code);
    let checked = antec::on_big_stack(|| antec::package::run_source(&src));
    let errors: Vec<String> = checked.diagnostics.iter().map(|(_, d)| d.message.clone()).collect();
    let run = checked.run.unwrap_or_else(|| panic!("welcome program didn't run: {errors:?}"));
    assert_eq!(run.stdout, expected, "README output and real output differ");
    assert_eq!(run.exit_code, 0);
}

/// Every program in the repository's `examples/` folder checks, and runs
/// successfully when it has a `modhalu`, so the examples can't silently rot.
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
        if src.text.contains("modhalu()") {
            let checked = antec::on_big_stack(|| antec::package::run_source(&src));
            if let Some(run) = checked.run {
                if run.exit_code != 0 {
                    failures.push(format!(
                        "{}: exit {} ({})",
                        path.display(),
                        run.exit_code,
                        run.stderr.trim()
                    ));
                }
            }
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
        // The spec describes features `antec` doesn't have yet (E1202).
        for d in diags.iter().filter(|d| d.is_error() && d.code != "E1202") {
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

/// The spec's revision header matches the newest row of Appendix G.
#[test]
fn spec_revision_matches_history() {
    let spec =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/ante_language_spec_1_0.md"))
            .expect("spec");
    let header = spec
        .lines()
        .find(|l| l.starts_with("**Revision:**"))
        .and_then(|l| l.split("draft ").nth(1))
        .and_then(|r| r.split('.').next())
        .expect("a `**Revision:** … draft rN.` header");
    let history = spec.split("# Appendix G").nth(1).expect("Appendix G");
    let newest = history.lines().find_map(|l| l.strip_prefix("| r")).and_then(|r| r.split(' ').next());
    assert_eq!(Some(header.trim_start_matches('r')), newest, "spec header and Appendix G disagree");
}

/// Every conformance test is listed in Appendix C under its ID.
#[test]
fn every_test_is_in_appendix_c() {
    let spec =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/ante_language_spec_1_0.md"))
            .expect("spec");
    let appendix = spec.split("# Appendix C").nth(1).and_then(|r| r.split("# Appendix D").next()).expect("C");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut missing = Vec::new();
    for f in files {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let id: String = name.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let id = id.trim_end_matches(|c: char| c.is_ascii_lowercase()).to_string();
        if !appendix.contains(&format!("| {id} |")) {
            missing.push(name.to_string());
        }
    }
    assert!(missing.is_empty(), "not in spec Appendix C: {missing:?}");
}
