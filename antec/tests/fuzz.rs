//! Robustness: no input makes `antec` panic or overflow its stack (spec M2:
//! "fuzzing finds no panics"). Deterministic mutations of every conformance
//! program and example go through the checker, code generation and the
//! formatter; deeply nested inputs must be rejected with E0216, not crash.

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use antec::source::SourceFile;

/// A small deterministic generator (xorshift), so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("._") {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "ante") {
                out.push(p);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    walk(&root.join("tests/conformance"), &mut out);
    walk(&root.join("../examples"), &mut out);
    out
}

const PIECES: &[&str] = &[
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    ":",
    "ni",
    "tho",
    "ante",
    "icchey",
    "aithe",
    "|>",
    "_",
    "&",
    "&maare ",
    "*",
    "?",
    "\"",
    "'",
    "f\"{",
    "}",
    "\n    ",
    "\n",
    "\t",
    "0x",
    "1e999",
    "99999999999999999999999999",
    "\u{0}",
    "\u{feff}",
    "\r\n",
    "nunchi",
    "lopu",
    "okko",
    "ki",
    "batti",
    "vellagane",
    "maare",
    "->",
    "=",
    ",",
    "#",
    "\\",
    "(x) => x",
    "theesuko ",
    "ledhante:",
    "P{a: 1}",
    "undhi(1)",
    "a ni f()",
    "...",
    "@",
    "@peru \"x\"\n    ",
    "sql\"",
    "sql\"{",
    "@automatic web.Daari\n",
    "-",
    "+",
];

/// Checks and formats `text`; returns a description of any panic.
fn exercise(name: &str, text: &str) -> Option<String> {
    let src = SourceFile::new(name, text);
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Checking builds MIR; valid programs go on to LLVM IR too.
        let _ = antec::package::emit_llvm_source(&src);
        if let Err(antec::fmt::FmtError::Bug(why)) = antec::fmt::format_source(&src) {
            panic!("fmt bug: {why}");
        }
    }));
    result.err().map(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".into())
    })
}

#[test]
fn mutated_programs_never_panic() {
    let files = sources();
    assert!(!files.is_empty());
    let originals: Vec<String> = files.iter().map(|p| fs::read_to_string(p).unwrap_or_default()).collect();
    // Longer runs: `ANTE_FUZZ_ROUNDS=100000 ANTE_FUZZ_SEED=7 cargo test --test fuzz`.
    let env = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (rounds, seed) = (env("ANTE_FUZZ_ROUNDS", 600), env("ANTE_FUZZ_SEED", 0x5eed_a17e).max(1));
    let failures = antec::on_big_stack(|| {
        let mut rng = Rng(seed);
        let mut failures = Vec::new();
        for round in 0..rounds {
            let k = rng.below(originals.len());
            let mut chars: Vec<char> = originals[k].chars().collect();
            for _ in 0..1 + rng.below(5) {
                let at = rng.below(chars.len() + 1);
                match rng.below(3) {
                    0 => {
                        let piece = PIECES[rng.below(PIECES.len())];
                        chars.splice(at..at, piece.chars());
                    }
                    1 if !chars.is_empty() => {
                        let end = (at + 1 + rng.below(8)).min(chars.len());
                        chars.drain(at.min(end)..end);
                    }
                    _ => {
                        let from = rng.below(chars.len() + 1);
                        let end = (from + 1 + rng.below(30)).min(chars.len());
                        let copy: Vec<char> = chars[from.min(end)..end].to_vec();
                        chars.splice(at..at, copy);
                    }
                }
            }
            let text: String = chars.into_iter().collect();
            // A crash that aborts (stack overflow) can't be caught: keep the
            // input on disk so it can be found afterwards.
            if let Ok(path) = std::env::var("ANTE_FUZZ_DUMP") {
                let _ = fs::write(path, &text);
            }
            if let Some(why) = exercise("fuzz.ante", &text) {
                failures.push(format!("round {round} (from {}): {why}\n{text}", files[k].display()));
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{} panics:\n{}", failures.len(), failures.join("\n---\n"));
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let cases = [
        format!("modhalu() -> khali ante:\n    x ante {}1{}\n", "(".repeat(20_000), ")".repeat(20_000)),
        format!("modhalu() -> khali ante:\n    x ante {}1\n", "-".repeat(50_000)),
        format!("modhalu() -> khali ante:\n    x ante 1{}\n", " + 1".repeat(100_000)),
        format!("modhalu() -> khali ante:\n    x ante {}1{}\n", "[".repeat(5_000), "]".repeat(5_000)),
        format!(
            "f(x: {}ankam{}) -> khali ante:\n    x ni choopinchu()\n",
            "varusa[".repeat(5_000),
            "]".repeat(5_000)
        ),
        {
            let mut s = String::from("modhalu() -> khali ante:\n");
            for i in 0..2_000 {
                s.push_str(&" ".repeat(4 * (i + 1)));
                s.push_str("avunu aithe:\n");
            }
            s.push_str(&" ".repeat(4 * 2_001));
            s.push_str("1 ni choopinchu()\n");
            s
        },
    ];
    let results = antec::on_big_stack(|| {
        cases
            .iter()
            .map(|text| {
                let src = SourceFile::new("deep.ante", text.as_str());
                antec::check_source(&src).iter().map(|d| d.code).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    });
    for (i, codes) in results.iter().enumerate() {
        assert!(codes.contains(&"E0216"), "case {i}: expected E0216, got {codes:?}");
    }
}

/// Long programs compile in about linear time. Each of these used to be
/// quadratic somewhere: a long list literal in the parser, a long
/// `ledhante` chain in the MIR builder and borrow checker, many locals in
/// drop flags. The budget is generous; quadratic behaviour blows far past it.
#[test]
fn long_programs_compile_in_linear_time() {
    let list = format!(
        "modhalu() -> khali ante:\n    v ante [{}]\n    v.enni() ni choopinchu()\n",
        (0..40_000).map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
    );
    let chain = {
        let mut s = String::from("f(n: ankam ni) -> ankam ante:\n    n == 0 aithe:\n        0 icchey\n");
        for i in 1..4_000 {
            s.push_str(&format!("    ledhante n == {i} aithe:\n        {i} icchey\n"));
        }
        s.push_str("    -1 icchey\n\nmodhalu() -> khali ante:\n    5 ni f() |> choopinchu()\n");
        s
    };
    let locals = {
        let mut s = String::from("modhalu() -> khali ante:\n    maare t ante 0\n");
        for i in 0..2_000 {
            s.push_str(&format!("    s{i} ante \"a\" + \"{i}\"\n    t += s{i}.byte_len()\n"));
        }
        s.push_str("    t ni choopinchu()\n");
        s
    };
    for (name, text) in [("list", list), ("chain", chain), ("locals", locals)] {
        let start = std::time::Instant::now();
        let src = SourceFile::new("long.ante", text.as_str());
        let out = antec::on_big_stack(|| antec::package::emit_llvm_source(&src));
        assert!(out.error_count() == 0 && out.mir.is_some(), "{name}: didn't compile");
        let took = start.elapsed();
        assert!(took.as_secs() < 20, "{name}: took {took:?}");
    }
}
