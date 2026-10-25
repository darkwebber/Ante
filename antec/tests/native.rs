//! Differential tests for the native backend (M7): every `run-pass` and
//! `runtime-trap` conformance program is compiled with `antec build` and run,
//! and must print the expected output, exit with the expected status, and
//! trap at the expected position — exactly like the interpreter.
//!
//! Needs `clang` (or `$ANTE_CC`); without it the test only prints a note.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use antec::package::Action;

/// Programs that use features the native backend doesn't have yet.
const NOT_YET_NATIVE: &[&str] = &["D001_jodi_order", "D002_jodi_replace"];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> =
        fs::read_dir(dir).expect("readable dir").map(|e| e.expect("entry").path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("._") {
            continue;
        }
        if p.is_dir() && !p.join("ante.toml").is_file() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e == "ante") {
            out.push(p);
        }
    }
}

fn header(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .take_while(|l| l.starts_with('#'))
        .filter_map(|l| l.strip_prefix('#').map(str::trim_start))
        .filter_map(|l| l.strip_prefix(key))
        .map(|v| v.strip_prefix(' ').unwrap_or(v).to_string())
        .collect()
}

fn clang_available() -> bool {
    let cc = std::env::var("ANTE_CC").unwrap_or_else(|_| "clang".into());
    Command::new(cc).arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn native_matches_expectations() {
    if !clang_available() {
        assert!(std::env::var_os("ANTE_REQUIRE_NATIVE").is_none(), "ANTE_REQUIRE_NATIVE set, kaani clang ledhu");
        eprintln!("note: clang dorakaledhu, native tests skip chesthunnam");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let out_dir = std::env::temp_dir().join(format!("antec-native-tests-{}", std::process::id()));
    fs::create_dir_all(&out_dir).expect("temp dir");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut failures = Vec::new();
    let mut ran = 0;
    for path in files {
        let text = fs::read_to_string(&path).expect("readable test");
        let category = header(&text, "category:").first().cloned().unwrap_or_default();
        if !matches!(category.trim(), "run-pass" | "runtime-trap") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let exe = out_dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.clone() });
        let checked =
            antec::package::process(&path, Action::Build { output: exe.clone() }).expect("readable");
        if NOT_YET_NATIVE.contains(&name.as_str()) {
            if checked.built.is_some() {
                failures.push(format!("{name}: builds natively now; take it off NOT_YET_NATIVE"));
            }
            continue;
        }
        if checked.built.is_none() {
            failures.push(format!("{name}: native build failed: {:?}", checked.backend_errors));
            continue;
        }
        ran += 1;
        let out = Command::new(&exe).output().expect("runs");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stdout.lines().collect();
        let want = header(&text, "expect-stdout:");
        if !want.is_empty() && lines != want {
            failures.push(format!("{name}: stdout {lines:?}, expected {want:?}"));
        }
        let trap = header(&text, "expect-trap:");
        let default_exit = if category.trim() == "runtime-trap" { 101 } else { 0 };
        let want_exit =
            header(&text, "expect-exit:").first().and_then(|e| e.trim().parse().ok()).unwrap_or(default_exit);
        if out.status.code() != Some(want_exit) {
            failures.push(format!(
                "{name}: exit {:?}, expected {want_exit} (stderr: {})",
                out.status.code(),
                stderr.trim()
            ));
        }
        if let Some(t) = trap.first() {
            // `T01 @ 7:12` → the message ends with `(file:7:12)`.
            let pos = t.split('@').nth(1).map(str::trim).unwrap_or("");
            if !stderr.trim_end().ends_with(&format!(":{pos})")) {
                failures.push(format!("{name}: trap message `{}` doesn't point at {pos}", stderr.trim()));
            }
        }
    }
    let _ = fs::remove_dir_all(&out_dir);
    assert!(ran >= 40, "only {ran} native programs ran");
    assert!(failures.is_empty(), "{} native failures:\n{}", failures.len(), failures.join("\n"));
}

/// The README's welcome program, compiled natively, prints what the README says.
#[test]
fn native_welcome_program() {
    if !clang_available() {
        return;
    }
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../README.md")).expect("README");
    let code = readme.split("```ante\n").nth(1).and_then(|r| r.split("```").next()).expect("an ante block");
    let expected =
        readme.split("```text\n").nth(1).and_then(|r| r.split("```").next()).expect("a text block");
    let dir = std::env::temp_dir().join(format!("antec-native-welcome-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let src = dir.join("welcome.ante");
    fs::write(&src, code).expect("write");
    let exe = dir.join(if cfg!(windows) { "welcome.exe" } else { "welcome" });
    let checked = antec::package::process(&src, Action::Build { output: exe.clone() }).expect("readable");
    assert!(checked.built.is_some(), "build failed: {:?}", checked.backend_errors);
    let out = Command::new(&exe).output().expect("runs");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}
