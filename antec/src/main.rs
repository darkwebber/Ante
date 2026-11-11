use std::path::Path;
use std::process::ExitCode;

use antec::diag::Diagnostic;
use antec::package::Action;
use antec::source::SourceFile;

const USAGE: &str = "\
antec: \"Ante\" compiler (stage 0) 🛠️

usage:
  antec new <peru>                kottha project folder (ante.toml, src/modhalu.ante) create cheyyi
  antec repl                      okko line raasi ventane result choodu (`:help` lopala)
  antec run [path] [--interp] [-- args…]
                                  program ni run cheyyi (check chesi, `modhalu()` nunchi start);
                                  clang unte native ga build chesi fast ga run chesthundhi,
                                  `--interp` ayithe interpreter lo; `--` tharvatha unnavi
                                  program ki `system.args()` ga vasthayi
  antec build [path] [-o out] [--release]
                                  native executable build cheyyi (LLVM + clang tho);
                                  `--release` optimize chesthundhi (build konchem slow)
  antec pariksha [path] [peru]    `@pariksha` tests anni run cheyyi (prathi test vere process lo);
                                  `peru` isthe aa peru unna tests matrame (`antec test` kuda ok)
  antec check [path]              file leka package (ante.toml unna folder) ni check cheyyi;
                                  path lekapothe current folder
  antec fmt [path] [--check]      code ni official style lo format cheyyi (folder ayithe anni
                                  .ante files); `--check` emi maarchadhu, format avvakapothe fail
  antec lsp                       editor kosam language server (LSP, stdin/stdout lo);
                                  VS Code extension dheenni thane start chesthundhi
  antec emit tokens <file.ante>   token stream choopinchu
  antec emit ast <file.ante>      syntax tree (AST) choopinchu
  antec emit mir [path]           MIR (control-flow graph) choopinchu
  antec emit llvm [path]          LLVM IR choopinchu
  antec --help                    ee help choopinchu
  antec --version                 version choopinchu";

#[derive(Clone, Copy, PartialEq)]
enum Emit {
    Tokens,
    Ast,
}

/// `print!` that doesn't panic when stdout is closed early
/// (`antec emit llvm | head`); the rest of the output is dropped.
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = write!(std::io::stdout(), $($t)*);
    }};
}

macro_rules! outln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    antec::on_big_stack(move || dispatch(&args))
}

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("error: {msg}\n\n{USAGE}");
    ExitCode::from(2)
}

fn dispatch(args: &[String]) -> ExitCode {
    // Everything after `--` belongs to the program (`antec run`).
    let (args, rest) = match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], args[i + 1..].to_vec()),
        None => (args, Vec::new()),
    };
    let mut output = None;
    let mut release = false;
    let mut check_only = false;
    let mut interp = false;
    let mut words: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" => match it.next() {
                Some(o) => output = Some(o.as_str()),
                None => return usage_error("`-o` tharvatha output file peru kavali"),
            },
            "--release" => release = true,
            "--check" => check_only = true,
            "--interp" => interp = true,
            "--help" | "-h" | "help" if words.is_empty() => {
                outln!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" if words.is_empty() => {
                outln!(
                    "antec {} (\"Ante\" edition 1.0, stage 0) — Telugu lo think cheyyi, native speed lo run cheyyi 🚀",
                    env!("CARGO_PKG_VERSION")
                );
                return ExitCode::SUCCESS;
            }
            flag if flag.starts_with('-') && flag.len() > 1 => {
                return usage_error(&format!("`{flag}` ane option theliyadhu"))
            }
            word => words.push(word),
        }
    }
    let only_build = |used: bool, what: &str| {
        if used {
            Some(usage_error(&format!("`{what}` `antec build` tho matrame pani chesthundhi")))
        } else {
            None
        }
    };
    if !matches!(words.first(), Some(&"build")) {
        if let Some(code) = only_build(output.is_some(), "-o").or_else(|| only_build(release, "--release")) {
            return code;
        }
    }
    if check_only && !matches!(words.first(), Some(&"fmt")) {
        return usage_error("`--check` `antec fmt` tho matrame pani chesthundhi");
    }
    if interp && !matches!(words.first(), Some(&"run")) {
        return usage_error("`--interp` `antec run` tho matrame pani chesthundhi");
    }
    if !rest.is_empty() && !matches!(words.first(), Some(&"run")) {
        return usage_error("`--` tharvatha arguments `antec run` ki matrame");
    }
    match words.as_slice() {
        [] => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        ["fmt"] => fmt(".", check_only),
        ["fmt", path] => fmt(path, check_only),
        ["pariksha" | "test"] => pariksha(".", None),
        ["pariksha" | "test", one] if Path::new(one).exists() => pariksha(one, None),
        ["pariksha" | "test", filter] => pariksha(".", Some(filter)),
        ["pariksha" | "test", path, filter] => pariksha(path, Some(filter)),
        ["repl"] => repl(),
        ["new"] => usage_error("`antec new` tharvatha project peru kavali, ex: `antec new naa_project`"),
        ["new", name] => new_project(name),
        ["lsp"] => ExitCode::from(antec::lsp::run(std::io::stdin(), std::io::stdout()) as u8),
        ["check"] => check("."),
        ["check", path] => check(path),
        ["run"] => run(".", rest, interp),
        ["run", path] => run(path, rest, interp),
        ["emit", "mir"] => emit_mir(".", Action::EmitMir),
        ["emit", "mir", path] => emit_mir(path, Action::EmitMir),
        ["emit", "llvm"] => emit_mir(".", Action::EmitLlvm),
        ["emit", "llvm", path] => emit_mir(path, Action::EmitLlvm),
        ["build"] => build(".", output, release),
        ["build", path] => build(path, output, release),
        ["emit", "tokens", path] => emit(path, Emit::Tokens),
        ["emit", "ast", path] => emit(path, Emit::Ast),
        ["emit", what, ..] => {
            usage_error(&format!("`emit {what}` theliyadhu: tokens, ast, mir, llvm lo okati"))
        }
        [cmd, ..]
            if !["check", "run", "build", "emit", "fmt", "pariksha", "test", "lsp", "new", "repl"]
                .contains(cmd) =>
        {
            usage_error(&format!("`{cmd}` ane command ledhu"))
        }
        [cmd, ..] => usage_error(&format!("`antec {cmd}` ki arguments ekkuva ayyayi")),
    }
}

const REPL_HELP: &str = "\
Okko line raasi Enter kottandi:
  x ante 5                 peru pettu (tharvatha lines lo kuda untundhi)
  x * 2                    expression ayithe value choopisthundhi
  \"hi\" ni choopinchu()     statements run avthayi
  rendu(x: ankam ni) -> ankam ante:
      (x * 2) icchey       `:` tho end ayithe inka lines adugutha; khali line tho mugisthundhi
Commands:
  :program                 ippati varaku raasina program motham choopinchu
  :save <file.ante>        aa program ni file lo save cheyyi
  :reset                   anni marchipoyi kotthaga start cheyyi
  :q                       bayatiki (Ctrl-D kuda ok)";

/// `antec repl` (spec §23.7.1).
fn repl() -> ExitCode {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    let interactive = std::io::stdin().is_terminal();
    let prompt = |p: &str| {
        if interactive {
            out!("{p}");
            let _ = std::io::stdout().flush();
        }
    };
    if interactive {
        outln!(
            "\"Ante\" {} repl 🐣  okko line raasi Enter kottandi. `:help` ki help, `:q` ki bye",
            env!("CARGO_PKG_VERSION")
        );
    }
    let mut session = antec::repl::Session::new();
    let stdin = std::io::stdin();
    loop {
        prompt("ante> ");
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            if interactive {
                outln!("\ntata bye bye 👋");
            }
            return ExitCode::SUCCESS;
        }
        let command = line.trim();
        match command.split_whitespace().next().unwrap_or("") {
            ":q" | ":quit" | ":exit" | "exit" | "quit" | "tata" => return ExitCode::SUCCESS,
            ":help" | ":h" | "help" => {
                outln!("{REPL_HELP}");
                continue;
            }
            ":reset" => {
                session.reset();
                outln!("🧹 kottha session");
                continue;
            }
            ":program" => {
                out!("{}", session.source());
                continue;
            }
            ":save" => {
                let Some(file) = command.split_whitespace().nth(1) else {
                    eprintln!("`:save` tharvatha file peru kavali, ex: `:save naa_program.ante`");
                    continue;
                };
                match std::fs::write(file, session.source()) {
                    Ok(()) => outln!("💾 {file} lo save chesam. `antec run {file}` tho run cheyyochu"),
                    Err(err) => eprintln!("error: `{file}` raayalekapoyanu ({err})"),
                }
                continue;
            }
            c if c.starts_with(':') => {
                eprintln!("`{c}` ane command ledhu. `:help` choodandi");
                continue;
            }
            _ => {}
        }
        // A line ending in `:` opens a block: read until an empty line.
        let mut text = line.trim_end().to_string();
        if text.ends_with(':') {
            loop {
                prompt("....> ");
                let mut more = String::new();
                if stdin.lock().read_line(&mut more).unwrap_or(0) == 0 || more.trim().is_empty() {
                    break;
                }
                text.push('\n');
                text.push_str(more.trim_end());
            }
        }
        let reply = session.entry(&text);
        out!("{}", reply.out);
        let _ = std::io::stdout().flush();
        eprint!("{}", reply.err);
    }
}

/// `antec new`: a package folder with a first program and a test in it.
fn new_project(name: &str) -> ExitCode {
    let root = Path::new(name);
    let peru = root.file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let valid = peru.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && peru.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return usage_error(&format!(
            "`{peru}` project peru ga kudharadhu: English letters, numbers, `_`, `-` matrame, modhalu letter tho"
        ));
    }
    if root.exists() && std::fs::read_dir(root).map_or(true, |mut d| d.next().is_some()) {
        eprintln!("error: `{name}` already undhi, andhulo files kuda unnayi. Vere peru try cheyyandi");
        return ExitCode::from(2);
    }
    let toml =
        format!("[package]\nname = \"{peru}\"\nversion = \"0.1.0\"\nedition = \"1.0\"\nkind = \"bin\"\n");
    let main = "\
# Mee modhati \"Ante\" program 🎉
#
# Run:   antec run
# Test:  antec pariksha

hello_cheppu(evariki: padham ni) -> padham ante:
    f\"Hello {evariki}! 👋\" icchey

modhalu() -> khali ante:
    \"World\" ni hello_cheppu() |> choopinchu()

@pariksha
hello_pariksha() -> khali ante:
    ((\"Ravi\" ni hello_cheppu()) == \"Hello Ravi! 👋\") nijame()
";
    let ignore = format!("/{peru}\n/{peru}.exe\n");
    let written = std::fs::create_dir_all(root.join("src"))
        .and_then(|()| std::fs::write(root.join("ante.toml"), toml))
        .and_then(|()| std::fs::write(root.join("src").join("modhalu.ante"), main))
        .and_then(|()| std::fs::write(root.join(".gitignore"), ignore));
    if let Err(err) = written {
        eprintln!("error: `{name}` create cheyyalekapoyanu ({err})");
        return ExitCode::from(2);
    }
    outln!(
        "✨ `{name}` ready!\n\n    cd {name}\n    antec run\n\nCode `src/modhalu.ante` lo undhi. Enjoy 🚀"
    );
    ExitCode::SUCCESS
}

fn check(path: &str) -> ExitCode {
    let checked = match antec::package::check_path(Path::new(path)) {
        Ok(checked) => checked,
        Err(err) => {
            eprintln!("error: `{path}` chadavalekapoyanu 😕 ({err})");
            return ExitCode::from(2);
        }
    };
    for (file, d) in &checked.diagnostics {
        eprint!("{}", d.render(&checked.sources[*file]));
    }
    let code = summary(checked.error_count());
    if code == ExitCode::SUCCESS {
        match checked.warning_count() {
            0 => outln!("✅ {path}: anthaa ok! Errors emi levu."),
            1 => outln!("\n✅ {path}: errors levu, kaani 1 warning undhi 👀"),
            n => outln!("\n✅ {path}: errors levu, kaani {n} warnings unnayi 👀"),
        }
    }
    code
}

/// `antec fmt`: formats one file, or every `.ante` file under a folder.
fn fmt(path: &str, check_only: bool) -> ExitCode {
    let root = Path::new(path);
    let mut files = Vec::new();
    if root.is_dir() {
        collect_ante_files(root, &mut files);
    } else {
        files.push(root.to_path_buf());
    }
    let (mut changed, mut failed) = (0, 0);
    for file in &files {
        let (src, diags) = match antec::load_source(file) {
            Ok(loaded) => loaded,
            Err(err) => {
                eprintln!("error: `{}` chadavalekapoyanu 😕 ({err})", file.display());
                failed += 1;
                continue;
            }
        };
        if !diags.is_empty() {
            report(&src, &diags);
            failed += 1;
            continue;
        }
        match antec::fmt::format_source(&src) {
            Ok(text) if text == src.text => {}
            Ok(text) => {
                changed += 1;
                if check_only {
                    outln!("✏️  {} format avvaledhu", file.display());
                } else if let Err(err) = std::fs::write(file, text) {
                    eprintln!("error: `{}` raayalekapoyanu ({err})", file.display());
                    failed += 1;
                } else {
                    outln!("✨ {} format chesam", file.display());
                }
            }
            Err(antec::fmt::FmtError::Invalid(diags)) => {
                for d in &diags {
                    eprint!("{}", d.render(&src));
                }
                eprintln!("`{}` lo errors unnayi, fix chesaaka format cheddham", file.display());
                failed += 1;
            }
            Err(antec::fmt::FmtError::Bug(why)) => {
                eprintln!(
                    "error: `{}` ni format cheyyalekapoyanu ({why}). Idhi antec bug, file ni touch cheyyaledhu. Issue open cheyyandi 🙏",
                    file.display()
                );
                failed += 1;
            }
        }
    }
    if failed > 0 {
        return ExitCode::from(1);
    }
    if check_only && changed > 0 {
        outln!("\n{changed} file(s) ki `antec fmt` kavali");
        return ExitCode::from(1);
    }
    if changed == 0 {
        outln!("✅ anni files already neat ga unnayi ({})", files.len());
    }
    ExitCode::SUCCESS
}

fn collect_ante_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<_> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if p.is_dir() {
            collect_ante_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "ante") {
            out.push(p);
        }
    }
}

fn run(path: &str, args: Vec<String>, interp: bool) -> ExitCode {
    let checked =
        match antec::package::process(Path::new(path), Action::Run { stream: true, args, native: !interp }) {
            Ok(checked) => checked,
            Err(err) => {
                eprintln!("error: `{path}` chadavalekapoyanu 😕 ({err})");
                return ExitCode::from(2);
            }
        };
    for (file, d) in checked.diagnostics.iter().filter(|(_, d)| d.is_error()) {
        eprint!("{}", d.render(&checked.sources[*file]));
    }
    for e in &checked.backend_errors {
        eprintln!("error: {e}");
    }
    match checked.run {
        Some(out) => {
            out!("{}", out.stdout);
            eprint!("{}", out.stderr);
            ExitCode::from(out.exit_code.clamp(0, 255) as u8)
        }
        None => summary(checked.error_count().max(1)),
    }
}

/// `antec pariksha`: runs the `@pariksha` tests (§23.3) and reports each. A
/// folder that isn't a package has its files tested one by one.
fn pariksha(path: &str, filter: Option<&str>) -> ExitCode {
    let root = Path::new(path);
    let mut files = Vec::new();
    if root.is_dir() && antec::package::find_package_root(&root.join("x")).is_none() {
        collect_ante_files(root, &mut files);
    } else {
        files.push(root.to_path_buf());
    }
    let (mut passed, mut failed, mut broken) = (0, 0, 0);
    for file in &files {
        let action = Action::Test { filter: filter.map(str::to_string) };
        let checked = match antec::package::process(file, action) {
            Ok(checked) => checked,
            Err(err) => {
                eprintln!("error: `{}` chadavalekapoyanu 😕 ({err})", file.display());
                return ExitCode::from(2);
            }
        };
        for (f, d) in checked.diagnostics.iter().filter(|(_, d)| d.is_error()) {
            eprint!("{}", d.render(&checked.sources[*f]));
        }
        let mut seen = Vec::new();
        for e in &checked.backend_errors {
            if !seen.contains(&e) {
                eprintln!("error: {e}");
                seen.push(e);
            }
        }
        if checked.error_count() > 0 || !checked.backend_errors.is_empty() {
            broken += 1;
            continue;
        }
        if checked.tests.is_empty() {
            continue;
        }
        if files.len() > 1 {
            outln!("📄 {}", file.display());
        }
        for t in &checked.tests {
            if t.passed() {
                passed += 1;
                outln!("  ✅ {}", t.name);
            } else {
                failed += 1;
                outln!("  ❌ {}", t.name);
                for line in t.output.stdout.lines().chain(t.output.stderr.lines()) {
                    outln!("     │ {line}");
                }
            }
        }
    }
    if broken > 0 {
        eprintln!("\n{broken} file(s) compile avvaledhu, paina errors choodandi 💪");
        return ExitCode::from(1);
    }
    let total = passed + failed;
    if total == 0 {
        match filter {
            Some(f) => outln!("🤷 `{f}` peru unna pariksha lu emi levu"),
            None => outln!("🤷 pariksha lu emi levu. Function mundhu `@pariksha` raayandi"),
        }
        return ExitCode::SUCCESS;
    }
    if failed == 0 {
        outln!("\n🎉 anni pass! ({passed}/{total})");
        ExitCode::SUCCESS
    } else {
        outln!("\n😬 {passed} pass, {failed} fail. Fail ayina vaati output paina undhi");
        ExitCode::from(1)
    }
}

fn build(path: &str, out: Option<&str>, release: bool) -> ExitCode {
    let output = match out {
        Some(o) => std::path::PathBuf::from(o),
        None => {
            let p = Path::new(path);
            let stem = if p.is_dir() {
                p.canonicalize().ok().and_then(|c| c.file_name().map(|n| n.to_string_lossy().into_owned()))
            } else {
                p.file_stem().map(|n| n.to_string_lossy().into_owned())
            };
            let name = stem.unwrap_or_else(|| "program".into());
            std::path::PathBuf::from(if cfg!(windows) { format!("{name}.exe") } else { name })
        }
    };
    let checked = match antec::package::process(Path::new(path), Action::Build { output, release }) {
        Ok(checked) => checked,
        Err(err) => {
            eprintln!("error: `{path}` chadavalekapoyanu 😕 ({err})");
            return ExitCode::from(2);
        }
    };
    for (file, d) in checked.diagnostics.iter().filter(|(_, d)| d.is_error()) {
        eprint!("{}", d.render(&checked.sources[*file]));
    }
    for e in &checked.backend_errors {
        eprintln!("error: {e}");
    }
    if checked.error_count() > 0 {
        return summary(checked.error_count());
    }
    match &checked.built {
        Some(p) => {
            outln!("✅ native program ready: {} 🚀", p.display());
            ExitCode::SUCCESS
        }
        None => ExitCode::from(1),
    }
}

fn emit_mir(path: &str, action: Action) -> ExitCode {
    let checked = match antec::package::process(Path::new(path), action) {
        Ok(checked) => checked,
        Err(err) => {
            eprintln!("error: `{path}` chadavalekapoyanu 😕 ({err})");
            return ExitCode::from(2);
        }
    };
    for (file, d) in checked.diagnostics.iter().filter(|(_, d)| d.is_error()) {
        eprint!("{}", d.render(&checked.sources[*file]));
    }
    for e in &checked.backend_errors {
        eprintln!("error: {e}");
    }
    if let Some(mir) = &checked.mir {
        out!("{mir}");
    }
    if !checked.backend_errors.is_empty() {
        return ExitCode::from(1);
    }
    summary(checked.error_count())
}

fn emit(path: &str, what: Emit) -> ExitCode {
    let (src, mut diags) = match antec::load_source(Path::new(path)) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("error: `{path}` file chadavalekapoyanu 😕 ({err})");
            return ExitCode::from(2);
        }
    };
    if diags.is_empty() {
        if what == Emit::Tokens {
            let result = antec::lexer::lex(&src);
            for tok in &result.tokens {
                let (line, col) = src.line_col(tok.span.start);
                outln!("{line}:{col}\t{}", tok.kind);
            }
            diags = result.diagnostics;
        } else {
            let (module, parse_diags) = antec::parse_source(&src);
            if let Some(module) = &module {
                out!("{}", antec::pretty::module(module));
            }
            diags = parse_diags;
        }
    }
    report(&src, &diags)
}

fn report(src: &SourceFile, diags: &[Diagnostic]) -> ExitCode {
    for d in diags {
        let mut d = d.clone();
        antec::hint::add(&mut d, src);
        eprint!("{}", d.render(src));
    }
    summary(diags.iter().filter(|d| d.is_error()).count())
}

fn summary(errors: usize) -> ExitCode {
    match errors {
        0 => ExitCode::SUCCESS,
        1 => {
            eprintln!("\n1 error vachindhi. Fix chesi malli try cheyyandi 💪");
            ExitCode::from(1)
        }
        n => {
            eprintln!("\n{n} errors vachayi. Okkokkati fix cheddham 💪");
            ExitCode::from(1)
        }
    }
}
