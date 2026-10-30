//! The `antec` command line: help and errors, `run -- args`, `build`
//! safeguards, `[build]` settings, `fmt --check`, `pariksha` and the
//! `learn/` lessons.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn antec(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_antec")).args(args).output().expect("antec runs")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("antec-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn text(o: &[u8]) -> String {
    String::from_utf8_lossy(o).into_owned()
}

#[test]
fn help_and_version_succeed() {
    for flag in ["--help", "-h", "help", "--version"] {
        let out = antec(&[flag]);
        assert!(out.status.success(), "`antec {flag}` failed");
        assert!(!out.stdout.is_empty());
    }
}

#[test]
fn unknown_commands_and_flags_are_named() {
    let out = antec(&["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("`frobnicate` ane command ledhu"));
    let out = antec(&["check", "--wat"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("`--wat`"));
    let out = antec(&["check", "-o", "x"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn run_passes_arguments_after_double_dash() {
    let dir = scratch("args");
    let file = dir.join("args.ante");
    fs::write(&file, "system thechuko\n\nmodhalu() -> khali ante:\n    system.args() ni choopinchu()\n")
        .unwrap();
    let out = antec(&["run", file.to_str().unwrap(), "--", "a", "b c"]);
    assert_eq!(text(&out.stdout), "[a, b c]\n", "stderr: {}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_refuses_to_overwrite_source() {
    let dir = scratch("clobber");
    let file = dir.join("p.ante");
    let program = "modhalu() -> khali ante:\n    1 ni choopinchu()\n";
    fs::write(&file, program).unwrap();
    let out = antec(&["build", file.to_str().unwrap(), "-o", file.to_str().unwrap()]);
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), program, "source file was overwritten");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_settings_are_checked() {
    let dir = scratch("settings");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/modhalu.ante"), "modhalu() -> khali ante:\n    1 ni choopinchu()\n").unwrap();
    let manifest = |build: &str| {
        format!("[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"1.0\"\n\n[build]\n{build}\n")
    };
    fs::write(dir.join("helper.txt"), "").unwrap();
    for bad in [
        "stack = \"1KB\"",
        "stack = \"lots\"",
        "target = \"i686-pc-windows-msvc\"",
        "c_sources = [\"../outside.c\"]",
        "c_sources = [\"/etc/passwd.c\"]",
        "c_sources = [\"helper.txt\"]",
        "c_sources = [\"missing.c\"]",
        "c_sources = \"glue.c\"",
        "libs = [\"-fplugin=evil\"]",
        "libs = [\"m; rm\"]",
        "task_stack = \"4KB\"",
        "task_stack = \"many\"",
    ] {
        fs::write(dir.join("ante.toml"), manifest(bad)).unwrap();
        let out = antec(&["check", dir.to_str().unwrap()]);
        assert!(text(&out.stderr).contains("E0905"), "`{bad}` wasn't rejected: {}", text(&out.stderr));
    }
    fs::write(dir.join("ante.toml"), manifest("stack = \"64MB\"")).unwrap();
    let out = antec(&["run", dir.to_str().unwrap()]);
    assert_eq!(text(&out.stdout), "1\n", "stderr: {}", text(&out.stderr));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fmt_check_reports_without_writing() {
    let dir = scratch("fmt");
    let file = dir.join("f.ante");
    let messy = "modhalu() -> khali ante:\n  x ante 1+2\n  x ni choopinchu()\n";
    fs::write(&file, messy).unwrap();
    let out = antec(&["fmt", "--check", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(fs::read_to_string(&file).unwrap(), messy);
    let out = antec(&["fmt", file.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "modhalu() -> khali ante:\n    x ante 1 + 2\n    x ni choopinchu()\n"
    );
    let out = antec(&["fmt", "--check", file.to_str().unwrap()]);
    assert!(out.status.success());
    let _ = fs::remove_dir_all(&dir);
}

const TESTS: &str = "\
@pariksha
kalustundhi() -> khali ante:
    (1 + 1 == 2) nijame()

@pariksha
padipothundhi() -> khali ante:
    \"ikkada\" ni choopinchu()
    (1 + 1 == 3), \"lekka thappu\" ni nijame()

@pariksha
sare_ga() -> sare_thappu[khali, padham] ante:
    sare icchey
";

#[test]
fn pariksha_reports_each_test() {
    let dir = scratch("pariksha");
    let file = dir.join("t.ante");
    fs::write(&file, TESTS).unwrap();
    let path = file.to_str().unwrap();
    let both = |o: &Output| text(&o.stdout) + &text(&o.stderr);
    for interp in [false, true] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_antec"));
        cmd.args(["pariksha", path]);
        if interp {
            // No C compiler: the interpreter runs the tests.
            cmd.env("ANTE_CC", dir.join("no-such-cc"));
        }
        let out = cmd.output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{}", both(&out));
        let shown = text(&out.stdout);
        assert!(shown.contains("✅ kalustundhi") && shown.contains("✅ sare_ga"), "{shown}");
        assert!(shown.contains("❌ padipothundhi"), "{shown}");
        assert!(shown.contains("│ ikkada") && shown.contains("lekka thappu"), "{shown}");
        assert!(shown.contains("2 pass, 1 fail"), "{shown}");
    }
    let out = antec(&["pariksha", path, "sare"]);
    assert!(out.status.success(), "{}", both(&out));
    assert!(text(&out.stdout).contains("(1/1)"));
    let out = antec(&["test", path, "ledhu_asalu"]);
    assert!(out.status.success());
    assert!(text(&out.stdout).contains("pariksha lu emi levu"));
    fs::write(&file, "@pariksha\nx(n: ankam) -> khali ante:\n    ()\n").unwrap();
    let out = antec(&["pariksha", path]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E1307"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_interp_flag() {
    let dir = scratch("interp");
    let file = dir.join("p.ante");
    fs::write(&file, "modhalu() -> khali ante:\n    \"namaste\" ni choopinchu()\n").unwrap();
    for args in [vec!["run", file.to_str().unwrap()], vec!["run", "--interp", file.to_str().unwrap()]] {
        let out = antec(&args);
        assert_eq!(text(&out.stdout), "namaste\n", "{args:?}: {}", text(&out.stderr));
    }
    let out = antec(&["check", "--interp", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let _ = fs::remove_dir_all(&dir);
}

/// Programs that pull in the standard modules written in "Ante" (`json`,
/// `encoding`, the data tree) hear nothing about those files.
#[test]
fn std_ante_files_stay_quiet() {
    let dt = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/dt");
    for entry in fs::read_dir(&dt).unwrap() {
        let path = entry.unwrap().path();
        let out = antec(&["check", path.to_str().unwrap()]);
        let all = text(&out.stdout) + &text(&out.stderr);
        assert!(!all.contains("<ante>/"), "{}:\n{all}", path.display());
    }
}

/// Every lesson in `learn/` passes its tests and is formatted.
#[test]
fn learn_lessons_pass() {
    let learn = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../learn");
    let path = learn.to_str().unwrap();
    let out = antec(&["pariksha", path]);
    assert!(out.status.success(), "{}{}", text(&out.stdout), text(&out.stderr));
    assert!(text(&out.stdout).contains("anni pass"));
    let out = antec(&["fmt", "--check", path]);
    assert!(out.status.success(), "{}", text(&out.stdout));
}
