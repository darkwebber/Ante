//! `antec repl` (spec §23.7.1): try "Ante" one line at a time.
//!
//! A session is one program: its imports and declarations, then a
//! `modhalu` whose body is every statement entered so far. Each entry is
//! checked as part of that program and run on the MIR interpreter, and only
//! the output the new entry made is shown. Earlier entries run again each
//! time; the lines they read from the keyboard are replayed
//! (`interp::input`), but a random number or the clock can differ on the
//! re-run. An entry with a compile error or a trap is dropped, and the
//! session goes on as it was.

use crate::diag::Diagnostic;
use crate::source::SourceFile;

/// The name an expression's value gets while the session shows it.
const VALUE: &str = "ante_repl_viluva";

#[derive(Default)]
pub struct Session {
    /// Imports and declarations, each with the key that replaces it.
    items: Vec<(String, String)>,
    /// The statements of `modhalu`, one entry each.
    stmts: Vec<String>,
    /// Lines the accepted entries read from the keyboard.
    inputs: Vec<Option<String>>,
    /// How much of the program's output is already shown.
    shown: usize,
}

/// What one entry did.
pub struct Reply {
    /// Output to show: the program's own, then the value of an expression.
    pub out: String,
    /// Errors, traps and hints, for standard error.
    pub err: String,
    pub ok: bool,
}

enum Kind {
    Item(String),
    Stmt,
}

impl Session {
    pub fn new() -> Self {
        Session::default()
    }

    /// The session as one program, with `extra` as the newest statement.
    fn program(&self, extra_item: Option<(&str, &str)>, extra_stmt: Option<&str>) -> String {
        let mut text = String::new();
        for (key, item) in &self.items {
            if extra_item.is_some_and(|(k, _)| k == key) {
                continue;
            }
            text.push_str(item);
            text.push('\n');
        }
        if let Some((_, item)) = extra_item {
            text.push_str(item);
            text.push('\n');
        }
        text.push_str("modhalu() -> khali ante:\n");
        let mut any = false;
        for s in self.stmts.iter().map(String::as_str).chain(extra_stmt) {
            for line in s.lines() {
                text.push_str("    ");
                text.push_str(line);
                text.push('\n');
            }
            any = true;
        }
        if !any {
            text.push_str("    khali_ga_undu ante ()\n");
        }
        text
    }

    /// The whole session so far, as a file you could save (`:choopinchu`).
    pub fn source(&self) -> String {
        self.program(None, None)
    }

    pub fn reset(&mut self) {
        *self = Session::default();
    }

    /// Checks and runs one entry: one or more complete lines.
    pub fn entry(&mut self, text: &str) -> Reply {
        let text = text.trim_end();
        if text.trim().is_empty() {
            return Reply { out: String::new(), err: String::new(), ok: true };
        }
        match classify(text) {
            Kind::Item(key) => {
                let program = self.program(Some((&key, text)), None);
                let checked = crate::package::check_single(&SourceFile::new("repl", program.clone()));
                let errors: Vec<Diagnostic> = checked.into_iter().filter(Diagnostic::is_error).collect();
                if !errors.is_empty() {
                    return Reply { out: String::new(), err: render(&program, &errors), ok: false };
                }
                match self.items.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = text.to_string(),
                    None => self.items.push((key, text.to_string())),
                }
                Reply { out: String::new(), err: String::new(), ok: true }
            }
            Kind::Stmt => self.statement(text),
        }
    }

    fn statement(&mut self, text: &str) -> Reply {
        // A one-line expression that isn't `khali` shows its value.
        let mut run_text = text.to_string();
        let mut ty_note = None;
        let value = format!("{VALUE}{}", self.stmts.len());
        if !text.contains('\n') {
            let probe = self.program(None, Some(&format!("{value}: khali ante ({text})")));
            let diags = crate::package::check_single(&SourceFile::new("repl", probe));
            let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.is_error()).collect();
            if let [only] = errors.as_slice() {
                if let Some(ty) = only.message.strip_prefix("type match kaaledhu: `khali` kavali, kaani `") {
                    let ty = ty.split('`').next().unwrap_or("").to_string();
                    let shown = format!("{value} ante ({text})\n{value} ni choopinchu()");
                    let program = self.program(None, Some(&shown));
                    let ok = crate::package::check_single(&SourceFile::new("repl", program))
                        .iter()
                        .all(|d| !d.is_error());
                    if ok {
                        run_text = shown;
                    } else {
                        ty_note = Some(ty);
                    }
                }
            }
        }
        let program = self.program(None, Some(&run_text));
        crate::interp::input::start(self.inputs.clone());
        let checked = crate::package::run_source(&SourceFile::new("repl", program.clone()));
        let inputs = crate::interp::input::finish();
        let errors: Vec<Diagnostic> =
            checked.diagnostics.iter().map(|(_, d)| d.clone()).filter(Diagnostic::is_error).collect();
        if !errors.is_empty() {
            return Reply { out: String::new(), err: render(&program, &errors), ok: false };
        }
        let Some(run) = checked.run else {
            let err = checked.backend_errors.iter().map(|e| format!("error: {e}\n")).collect();
            return Reply { out: String::new(), err, ok: false };
        };
        let new_out = run.stdout.get(self.shown..).unwrap_or("").to_string();
        if run.exit_code != 0 || run.trap.is_some() {
            let err = if run.stderr.is_empty() {
                format!("program exit code {}\n", run.exit_code)
            } else {
                without_positions(&run.stderr)
            };
            return Reply { out: new_out, err, ok: false };
        }
        self.shown = run.stdout.len();
        self.inputs = inputs;
        self.stmts.push(run_text);
        let mut out = new_out;
        if let Some(ty) = ty_note {
            out.push_str(&format!(": {ty}\n"));
        }
        Reply { out, err: run.stderr, ok: true }
    }
}

/// A trap message names a place in the session's program, which the
/// learner never sees: `… (repl:16:5)` loses the place.
fn without_positions(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = match line.rfind(" (repl:") {
            Some(i) if line.ends_with(')') => &line[..i],
            _ => line,
        };
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Spec D1, item kinds, for the first line of an entry.
fn classify(text: &str) -> Kind {
    let first = text.lines().next().unwrap_or("").trim();
    let line = first.strip_prefix("andhariki ").unwrap_or(first);
    let word = |i: usize| line.split_whitespace().nth(i).unwrap_or("").to_string();
    if first.starts_with('@') {
        let decl = text.lines().find(|l| !l.trim_start().starts_with('@')).unwrap_or("");
        return Kind::Item(decl.trim().to_string());
    }
    if line.ends_with("thechuko") || line.contains("panulu ante:") || line.contains("gunam undhi") {
        return Kind::Item(line.to_string());
    }
    if matches!(word(0).as_str(), "rakam" | "edho_okati" | "gunam" | "bayata") {
        return Kind::Item(format!("{} {}", word(0), word(1)));
    }
    let name_end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
    if name_end > 0
        && line[name_end..].starts_with(['(', '['])
        && line.ends_with("ante:")
        && line.contains("->")
    {
        return Kind::Item(line[..name_end].to_string());
    }
    Kind::Stmt
}

/// Errors in the session's program, shown against the entry's own lines.
fn render(program: &str, errors: &[Diagnostic]) -> String {
    let src = SourceFile::new("repl", program.to_string());
    let mut out = String::new();
    for d in errors {
        let mut d = d.clone();
        crate::hint::add(&mut d, &src);
        let (line, col) = src.line_col(d.span.start);
        let text = src.line_text(line);
        let indent = text.len() - text.trim_start().len();
        let shown = text.trim_start();
        out.push_str(&format!("error[{}]: {}\n", d.code, d.message));
        out.push_str(&format!("  | {shown}\n"));
        out.push_str(&format!("  | {}^\n", " ".repeat(col.saturating_sub(1 + indent))));
        if let Some(h) = &d.help {
            out.push_str(&format!("  = 💡 {h}\n"));
        }
    }
    out
}
