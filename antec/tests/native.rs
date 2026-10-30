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
const NOT_YET_NATIVE: &[&str] = &[];

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
            // The runtime writes the trap's code here (`CODE line col file`).
            let report = exe.with_extension("trap");
            let _ = fs::remove_file(&report);
            let out = Command::new(&exe).env("ANTE_TRAP_REPORT", &report).output().expect("runs");
            let reported = fs::read_to_string(&report).ok();
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
                let got = reported
                    .as_deref()
                    .and_then(|r| r.split_whitespace().next())
                    .unwrap_or_else(|| trap_code(msg.lines().last().unwrap_or("")));
                if got != code {
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

/// Files, folders and child programs (§21.3) do the same thing natively as
/// in the interpreter. Each backend works in its own temp folder, given as
/// the program's first argument.
#[test]
fn native_files_and_processes_match_the_interpreter() {
    if !clang_available() {
        return;
    }
    let program = r#"file thechuko
system thechuko

modhalu() -> khali ante:
    base ante system.args()[0].xerox()
    sub ante &base ni, "a/b" tho file.path_kalupu()
    &sub ni file.folder_cheyyi() |> choopinchu()
    &sub ni file.folder_cheyyi() |> choopinchu()
    &sub ni file.folderaa() |> choopinchu()
    t ante &sub ni, "x.txt" tho file.path_kalupu()
    &t ki, "namaste\n" ni file.raayi() |> choopinchu()
    &t ni file.chadhuvu() |> choopinchu()
    &t ni file.undhaa() |> choopinchu()
    raw: varusa[aasajeevi ankam8] ante [0, 255, 10]
    r ante &sub ni, "raw.bin" tho file.path_kalupu()
    &r ki, raw[0 nunchi 3 lopu] ni file.baitlu_raayi() |> choopinchu()
    &r ni file.baitlu_chadhuvu() |> choopinchu()
    &r ni file.chadhuvu() |> choopinchu()
    &sub ni file.list_cheyyi() |> choopinchu()
    inside ante &t ni, "y" tho file.path_kalupu()
    &inside ni file.folder_cheyyi() |> choopinchu()
    &t ni file.theesey() |> choopinchu()
    &t ni file.theesey() |> choopinchu()
    &t ni file.undhaa() |> choopinchu()
    &sub ni file.list_cheyyi() |> choopinchu()
    "ee_program_ledhu_antec_test" ni, [] tho system.nadupu() |> choopinchu()
    maare shell ante "sh"
    maare script ante ["-c", "echo out; echo err 1>&2; exit 3"]
    system.os() == "windows" aithe:
        shell lo "cmd" pettu
        script lo ["/c", "echo out& echo err 1>&2& exit 3"] pettu
    &shell ni, script[0 nunchi 2 lopu] tho system.nadupu() batti:
        sare(ph) aithe:
            f"{ph.status} [{ph.stdout.kathirinchu()}] [{ph.stderr.kathirinchu()}]" ni choopinchu()
        thappu(e) aithe:
            e ni choopinchu()
"#;
    let dir = std::env::temp_dir().join(format!("antec-native-files-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    let src = dir.join("files.ante");
    fs::write(&src, program).expect("write");
    let run_dir = dir.join("run");
    let native_dir = dir.join("native");
    let interp = Command::new(env!("CARGO_BIN_EXE_antec"))
        .args(["run", src.to_str().unwrap(), "--", run_dir.to_str().unwrap()])
        .output()
        .expect("antec runs");
    let exe = dir.join(if cfg!(windows) { "files.exe" } else { "files" });
    let action = Action::Build { output: exe.clone(), release: false };
    let checked = antec::on_big_stack(|| antec::package::process(&src, action)).expect("readable");
    assert!(checked.built.is_some(), "build failed: {:?}", checked.backend_errors);
    let native = Command::new(&exe).arg(&native_dir).output().expect("runs");
    let _ = fs::remove_dir_all(&dir);
    let (a, b) = (String::from_utf8_lossy(&interp.stdout), String::from_utf8_lossy(&native.stdout));
    assert!(interp.status.success(), "interpreter failed: {}", String::from_utf8_lossy(&interp.stderr));
    assert_eq!(a, b, "native stderr: {}", String::from_utf8_lossy(&native.stderr));
    assert!(a.contains("3 [out] [err]"), "unexpected output:\n{a}");
}

/// The complete programs of spec Appendix D (except D.7, which needs Python,
/// Annex T) print the same and exit the same natively as in the interpreter.
#[test]
fn appendix_d_programs_match_the_interpreter() {
    if !clang_available() {
        return;
    }
    let spec =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/ante_language_spec_1_0.md"))
            .expect("spec");
    let appendix = spec.split("# Appendix D").nth(1).and_then(|r| r.split("# Appendix E").next()).expect("D");
    let dir = std::env::temp_dir().join(format!("antec-native-appendix-d-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp dir");
    let mut checked = 0;
    for (i, block) in appendix.split("```ante\n").skip(1).enumerate() {
        let code = block.split("```").next().unwrap_or("");
        if !code.contains("modhalu()") || code.contains("torch") {
            continue;
        }
        let src = dir.join(format!("d{i}.ante"));
        fs::write(&src, code).expect("write");
        let interp = Command::new(env!("CARGO_BIN_EXE_antec"))
            .current_dir(&dir)
            .args(["run", src.to_str().unwrap()])
            .output()
            .expect("antec runs");
        let exe = dir.join(if cfg!(windows) { format!("d{i}.exe") } else { format!("d{i}") });
        let action = Action::Build { output: exe.clone(), release: true };
        let checked_build = antec::on_big_stack(|| antec::package::process(&src, action)).expect("readable");
        assert!(
            checked_build.built.is_some(),
            "D program {i}: build failed: {:?}",
            checked_build.backend_errors
        );
        let native = Command::new(&exe).current_dir(&dir).output().expect("runs");
        assert_eq!(
            String::from_utf8_lossy(&interp.stdout),
            String::from_utf8_lossy(&native.stdout),
            "D program {i}: stdout differs"
        );
        assert_eq!(interp.status.code(), native.status.code(), "D program {i}: exit differs");
        checked += 1;
    }
    let _ = fs::remove_dir_all(&dir);
    assert!(checked >= 5, "expected D.1–D.5, ran {checked}");
}
