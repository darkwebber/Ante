use std::path::Path;
use std::process::ExitCode;

use antec::diag::Diagnostic;
use antec::source::SourceFile;

const USAGE: &str = "\
antec: \"Ante\" compiler (stage 0) 🛠️

usage:
  antec check <file.ante>         ippati varaku ready ayina stages tho check cheyyi (M1: lexing)
  antec emit tokens <file.ante>   token stream choopinchu
  antec --version                 version choopinchu";

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
        ["check", path] => run(path, false),
        ["emit", "tokens", path] => run(path, true),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(path: &str, print_tokens: bool) -> ExitCode {
    let (src, mut diags) = match antec::load_source(Path::new(path)) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("error: `{path}` file chadavalekapoyanu 😕 ({err})");
            return ExitCode::from(2);
        }
    };
    if diags.is_empty() {
        let result = antec::lexer::lex(&src);
        if print_tokens {
            for tok in &result.tokens {
                let (line, col) = src.line_col(tok.span.start);
                println!("{line}:{col}\t{}", tok.kind);
            }
        }
        diags = result.diagnostics;
    }
    let code = report(&src, &diags);
    if !print_tokens && code == ExitCode::SUCCESS {
        println!("✅ {}: anthaa ok! Errors emi levu.", src.name);
    }
    code
}

fn report(src: &SourceFile, diags: &[Diagnostic]) -> ExitCode {
    for d in diags {
        eprint!("{}", d.render(src));
    }
    match diags.iter().filter(|d| d.is_error()).count() {
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
