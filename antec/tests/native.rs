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

/// Which trap a runtime message reports (§20.3), from its wording.
fn trap_code(msg: &str) -> &'static str {
    let reason = msg.strip_prefix("kompa munigindhi! ").unwrap_or(msg);
    let table: &[(&str, &str)] = &[
        ("pattadhu", "T01"),
        ("maarchalem", "T01"),
        ("sunna (0) tho bhaagham", "T02"),
        ("bayatiki vellindhi", "T03"),
        ("shift count", "T04"),
        ("kachithanga()", "T06"),
        ("nijame()", "T07"),
        ("stack nindipoyindhi", "T08"),
        ("memory ayipoyindhi", "T09"),
        ("mukka haddulu", "T10"),
    ];
    table.iter().find(|(w, _)| reason.contains(w)).map(|(_, c)| *c).unwrap_or("T05")
}

#[test]
fn native_matches_expectations() {
    if !clang_available() {
        assert!(
            std::env::var_os("ANTE_REQUIRE_NATIVE").is_none(),
            "ANTE_REQUIRE_NATIVE set, kaani clang ledhu"
        );
        eprintln!("note: clang dorakaledhu, native tests skip chesthunnam");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let out_dir = std::env::temp_dir().join(format!("antec-native-tests-{}", std::process::id()));
    fs::create_dir_all(&out_dir).expect("temp dir");
    let mut files = Vec::new();
    collect(&root, &mut files);
    // Programs build and run in parallel; each worker takes the next file.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new((0usize, Vec::new()));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(path) = files.get(i) else { break };
                let (ran, failures) = check_native(path, &out_dir);
                let mut r = results.lock().expect("results");
                r.0 += ran;
                r.1.extend(failures);
            });
        }
    });
    let (ran, mut failures) = results.into_inner().expect("results");
    failures.sort();
    let _ = fs::remove_dir_all(&out_dir);
    assert!(ran >= 90, "only {ran} native programs ran");
    assert!(failures.is_empty(), "{} native failures:\n{}", failures.len(), failures.join("\n"));
}

/// Builds one conformance program natively (optimized and not), runs it and
/// compares with its expectations: the number of runs, and any failures.
fn check_native(path: &Path, out_dir: &Path) -> (usize, Vec<String>) {
    let mut failures = Vec::new();
    let mut ran = 0;
    {
        let text = fs::read_to_string(path).expect("readable test");
        let category = header(&text, "category:").first().cloned().unwrap_or_default();
        if !matches!(category.trim(), "run-pass" | "runtime-trap") {
            return (0, failures);
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // Every program is built twice: optimized (`--release`) and not.
        for release in [true, false] {
            let level = if release { "O2" } else { "O0" };
            let base = format!("{name}-{level}");
            let exe = out_dir.join(if cfg!(windows) { format!("{base}.exe") } else { base });
            let action = Action::Build { output: exe.clone(), release };
            let checked = antec::on_big_stack(|| antec::package::process(path, action)).expect("readable");
            if NOT_YET_NATIVE.contains(&name.as_str()) {
                if checked.built.is_some() {
                    failures.push(format!("{name}: builds natively now; take it off NOT_YET_NATIVE"));
                }
                continue;
            }
            if checked.built.is_none() {
                failures.push(format!("{name} ({level}): native build failed: {:?}", checked.backend_errors));
                continue;
            }
            ran += 1;
            let out = Command::new(&exe).output().expect("runs");
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lines: Vec<&str> = stdout.lines().collect();
            let want = header(&text, "expect-stdout:");
            if !want.is_empty() && lines != want {
                failures.push(format!("{name} ({level}): stdout {lines:?}, expected {want:?}"));
            }
            let trap = header(&text, "expect-trap:");
            let default_exit = if category.trim() == "runtime-trap" { 101 } else { 0 };
            let want_exit = header(&text, "expect-exit:")
                .first()
                .and_then(|e| e.trim().parse().ok())
                .unwrap_or(default_exit);
            if out.status.code() != Some(want_exit) {
                failures.push(format!(
                    "{name} ({level}): exit {:?}, expected {want_exit} (stderr: {})",
                    out.status.code(),
                    stderr.trim()
                ));
            }
            if let Some(t) = trap.first() {
                // `T01 @ 7:12` → the right kind of trap, and the message ends
                // with `(file:7:12)`. T08 and T09 have no position natively.
                let (code, pos) =
                    t.split_once('@').map(|(c, p)| (c.trim(), p.trim())).unwrap_or((t.trim(), ""));
                let msg = stderr.trim_end();
                if trap_code(msg) != code {
                    failures.push(format!("{name} ({level}): trap `{msg}` isn't {code}"));
                }
                if !matches!(code, "T08" | "T09") && !msg.ends_with(&format!(":{pos})")) {
                    failures.push(format!("{name} ({level}): trap message `{msg}` doesn't point at {pos}"));
                }
            }
        }
    }
    (ran, failures)
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
    // A debug (`-O0`) build, so both optimization levels are covered.
    let action = Action::Build { output: exe.clone(), release: false };
    let checked = antec::on_big_stack(|| antec::package::process(&src, action)).expect("readable");
    assert!(checked.built.is_some(), "build failed: {:?}", checked.backend_errors);
    let out = Command::new(&exe).output().expect("runs");
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&out.stdout), expected);
}
