use std::path::Path;
use std::process::ExitCode;

use antec::diag::Diagnostic;
use antec::source::SourceFile;

const USAGE: &str = "\
antec: \"Ante\" compiler (stage 0) 🛠️

usage:
  antec check [path]              file leka package (ante.toml unna folder) ni check cheyyi
                                  (M3: lexing + parsing + names); path lekapothe current folder
  antec emit tokens <file.ante>   token stream choopinchu
  antec emit ast <file.ante>      syntax tree (AST) choopinchu
  antec --version                 version choopinchu";

#[derive(Clone, Copy, PartialEq)]
enum Emit {
    Tokens,
    Ast,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--version" | "-V"] => {
            println!(
                "antec {} (\"Ante\" edition 1.0, stage 0) — Telugu lo think cheyyi, native speed lo run cheyyi 🚀",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        ["check"] => check("."),
        ["check", path] => check(path),
        ["emit", "tokens", path] => emit(path, Emit::Tokens),
        ["emit", "ast", path] => emit(path, Emit::Ast),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
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
            0 => println!("✅ {path}: anthaa ok! Errors emi levu."),
            1 => println!("\n✅ {path}: errors levu, kaani 1 warning undhi 👀"),
            n => println!("\n✅ {path}: errors levu, kaani {n} warnings unnayi 👀"),
        }
    }
    code
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
                println!("{line}:{col}\t{}", tok.kind);
            }
            diags = result.diagnostics;
        } else {
            let (module, parse_diags) = antec::parse_source(&src);
            if let Some(module) = &module {
                print!("{}", antec::pretty::module(module));
            }
            diags = parse_diags;
        }
    }
    report(&src, &diags)
}

fn report(src: &SourceFile, diags: &[Diagnostic]) -> ExitCode {
    for d in diags {
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
