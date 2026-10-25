//! Runs the conformance suite in `tests/conformance` (spec Appendix C).
//!
//! Until later milestones land, only the stages implemented so far run:
//! compile-fail cases must report their expected code and span, and every
//! other category must pass those stages without errors.

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
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ante") {
            out.push(path);
        }
    }
}

struct Expectations {
    category: String,
    errors: Vec<String>,
}

fn parse_header(text: &str) -> Expectations {
    let mut category = String::new();
    let mut errors = Vec::new();
    for line in text.lines() {
        let Some(comment) = line.strip_prefix('#') else { break };
        let comment = comment.trim();
        if let Some(value) = comment.strip_prefix("category:") {
            category = value.trim().to_string();
        } else if let Some(value) = comment.strip_prefix("expect-error:") {
            errors.push(value.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    Expectations { category, errors }
}

#[test]
fn conformance_suite() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut files = Vec::new();
    collect(&root, &mut files);
    assert!(!files.is_empty(), "no conformance tests found in {}", root.display());

    let mut failures = Vec::new();
    for path in &files {
        let (src, mut diags) = antec::load_source(path).expect("readable test file");
        if diags.is_empty() {
            diags = antec::check_source(&src);
        }
        let found: Vec<String> = diags
            .iter()
            .filter(|d| d.is_error())
            .map(|d| {
                let (line, col) = src.line_col(d.span.start);
                format!("{} @ {line}:{col}", d.code)
            })
            .collect();
        let expect = parse_header(&src.text);
        let name = path.strip_prefix(&root).unwrap_or(path).display();
        match expect.category.as_str() {
            "compile-fail" => {
                if expect.errors.is_empty() {
                    failures.push(format!("{name}: compile-fail test has no expect-error line"));
                }
                for wanted in &expect.errors {
                    if !found.contains(wanted) {
                        failures.push(format!("{name}: expected {wanted}, found {found:?}"));
                    }
                }
            }
            "" => failures.push(format!("{name}: missing `# category:` header")),
            _ => {
                if !found.is_empty() {
                    failures.push(format!("{name}: expected no errors, found {found:?}"));
                }
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
