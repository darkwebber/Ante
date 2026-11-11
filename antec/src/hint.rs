//! Hints for the commonest beginner mistakes (roadmap P1). A hint is one
//! extra line under a diagnostic that shows the fix written with the
//! program's own names: `x = 6` gets `x ante 6`, `if x > 3:` gets
//! `x > 3 aithe:`. Hints are worked out from the diagnostic's code and the
//! source line it points at, after checking, so every stage gets them
//! without knowing about them. Like the message, a hint's text is not
//! normative; the conformance cases in `tests/conformance/hints` check that
//! each one still appears.

use crate::diag::Diagnostic;
use crate::source::SourceFile;

/// The hint for `d`, if it matches a mistake we know.
pub fn hint(d: &Diagnostic, src: &SourceFile) -> Option<String> {
    let (line_no, _) = src.line_col(d.span.start);
    let code = strip_comment(src.line_text(line_no)).trim();
    let marked = src.text.get(d.span.start..d.span.end).unwrap_or("");
    if matches!(d.code, "E0201" | "E0203" | "E0209" | "E0301") {
        if let Some(h) = from_other_language(code) {
            return Some(h);
        }
    }
    let hint = match d.code {
        "E0201" => header(code).or_else(|| assignment(code, src, line_no)),
        "E0202" => missing_verb(code),
        "E0203" => missing_parens(code),
        "E0204" => call_form(code, marked),
        "E0301" => english_word(marked).or_else(|| did_you_mean(marked, src)),
        "E0102" => Some("editor settings lo \"Insert spaces\" on cheyyandi; prathi level ki 4 spaces".into()),
        "E0103" => Some("okate block lo unna lines anni okate indent lo undali (prathi level ki 4 spaces)".into()),
        "E0109" => marked
            .strip_prefix('\'')
            .and_then(|m| m.strip_suffix('\''))
            .filter(|m| m.chars().count() > 1)
            .map(|m| format!("text ayithe double quotes: `\"{m}\"`")),
        "E0401" => mismatch(&d.message, code),
        "E0403" => condition(code),
        "E0417" => chained(code),
        "E0601" => moved(&d.message, src),
        "E0602" => not_maare(&d.message, src, line_no),
        "E0308" => Some(format!(
            "malli `ante` raasthe kottha variable avthundhi. Value maarchaalante modhati saari `maare {name} ante …` ani raasi, tharvatha `{name} lo <kottha value> pettu`",
            name = first_quoted(&d.message).unwrap_or("x")
        )),
        _ => None,
    };
    hint.filter(|h| !h.is_empty())
}

/// Fills in `d.help` when it has none.
pub fn add(d: &mut Diagnostic, src: &SourceFile) {
    if d.help.is_none() {
        d.help = hint(d, src);
    }
}

/// The line without a trailing comment.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut prev = ' ';
    for (i, c) in line.char_indices() {
        match c {
            '"' if prev != '\\' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
        prev = c;
    }
    line
}

/// Python, JavaScript, C or Rust habits, rewritten.
fn from_other_language(line: &str) -> Option<String> {
    fn colon_body(rest: &str) -> Option<&str> {
        rest.strip_suffix(':').map(str::trim)
    }
    if let Some(cond) = line.strip_prefix("if ").and_then(colon_body) {
        return Some(format!(
            "\"Ante\" lo condition mundhu, `aithe` tharvatha: `{} aithe:`",
            cond_text(cond)
        ));
    }
    if let Some(cond) =
        line.strip_prefix("elif ").or_else(|| line.strip_prefix("else if ")).and_then(colon_body)
    {
        return Some(format!("`ledhante {} aithe:` ani raayandi", cond_text(cond)));
    }
    if line == "else:" || line == "else" {
        return Some("else ki `ledhante:`".into());
    }
    if let Some(cond) = line.strip_prefix("while ").and_then(colon_body) {
        return Some(format!("while loop: `{} unnanthasepu:`", cond_text(cond)));
    }
    if let Some(rest) = line.strip_prefix("for ").and_then(colon_body) {
        if let Some((var, what)) = rest.split_once(" in ") {
            let (var, what) = (var.trim(), what.trim());
            return Some(format!("for loop: `{}`", for_loop(var, what)));
        }
    }
    if let Some(value) = line.strip_prefix("return ") {
        return Some(format!("value ichhi bayatiki raavalante: `{} icchey`", value.trim()));
    }
    match line {
        "return" => return Some("`icchey` ani raayandi".into()),
        "break" => return Some("loop aapaalante `aapey`".into()),
        "continue" => return Some("next round ki vellaalante `vadhiley`".into()),
        "pass" => return Some("emi cheyyakkarledhu ante aa line theeseyandi".into()),
        _ => {}
    }
    for (kw, maare) in
        [("let mut ", true), ("let ", false), ("const ", false), ("var ", true), ("val ", false)]
    {
        if let Some((name, value)) = line.strip_prefix(kw).and_then(|r| r.split_once('=')) {
            let name = name.split(':').next().unwrap_or(name).trim();
            let value = value.trim().trim_end_matches(';');
            let m = if maare { "maare " } else { "" };
            return Some(format!(
                "\"Ante\" lo `{kw}` avasaram ledhu: `{m}{name} ante {value}`{}",
                if maare { "" } else { " (tharvatha maarchaalante `maare` pettandi)" }
            ));
        }
    }
    for kw in ["def ", "fn ", "func ", "function "] {
        if let Some(rest) = line.strip_prefix(kw) {
            let name = rest.split(['(', ':', ' ']).next().unwrap_or("").trim();
            let name = if name == "main" { "modhalu" } else { name };
            if name == "modhalu" {
                return Some("program `modhalu() -> khali ante:` nunchi start avthundhi, lopala 4 spaces indent chesi raayandi".into());
            }
            if is_name(name) {
                return Some(format!(
                    "function ila raayandi: `{name}(x: ankam ni) -> khali ante:` (parameter ki type, chivarlo return type)"
                ));
            }
        }
    }
    if let Some(what) = line.strip_prefix("import ").or_else(|| line.strip_prefix("use ")) {
        return Some(format!("`{} thechuko`", what.trim().trim_end_matches(';')));
    }
    None
}

/// `x > 3 and y` written the "Ante" way.
fn cond_text(cond: &str) -> String {
    let cond = cond.trim().trim_start_matches('(').trim_end_matches(')').trim();
    let mut out = Vec::new();
    for word in cond.split(' ') {
        out.push(match word {
            "and" => "&&",
            "or" => "||",
            "not" => "!",
            "True" | "true" => "avunu",
            "False" | "false" => "kaadhu",
            "=" => "==",
            w => w,
        });
    }
    out.join(" ").replace("! ", "!")
}

fn for_loop(var: &str, what: &str) -> String {
    let range_args = what.strip_prefix("range(").and_then(|r| r.strip_suffix(')'));
    if let Some(args) = range_args {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        return match parts.as_slice() {
            [end] => format!("0 nunchi {end} lopu okko {var} ki:"),
            [start, end] => format!("{start} nunchi {end} lopu okko {var} ki:"),
            _ => format!("{what} lo okko {var} ki:"),
        };
    }
    if let Some((a, b)) = what.split_once("..=") {
        return format!("{} nunchi {} varaku okko {var} ki:", a.trim(), b.trim());
    }
    if let Some((a, b)) = what.split_once("..") {
        return format!("{} nunchi {} lopu okko {var} ki:", a.trim(), b.trim());
    }
    format!("{what} lo okko {var} ki:")
}

/// `x = 6`: a new name is `ante`, a change is `lo … pettu`.
fn assignment(line: &str, src: &SourceFile, line_no: usize) -> Option<String> {
    let (name, value) = line.split_once('=')?;
    let (name, value) = (name.trim(), value.trim());
    if value.starts_with('=') || name.ends_with(['!', '<', '>', '+', '-', '*', '/', '%']) {
        return None;
    }
    if !is_name(name) {
        return None;
    }
    if declared_maare(src, name, line_no).is_some() {
        return Some(format!("`maare` value maarchaalante: `{name} lo {value} pettu`"));
    }
    Some(format!(
        "kottha peru ayithe `{name} ante {value}`. Already unna `maare` value ni maarchaalante `{name} lo {value} pettu`"
    ))
}

/// A header line that forgot its `:`, or used `=` to compare.
fn header(line: &str) -> Option<String> {
    for kw in ["aithe", "unnanthasepu", "ledhante", "ante"] {
        if line.ends_with(kw) && !line.ends_with(&format!("{kw}:")) {
            return Some(format!("chivarlo `:` marchipoyaru: `{line}:`"));
        }
    }
    if line.ends_with(" ki") && line.contains(" okko ") {
        return Some(format!("chivarlo `:` marchipoyaru: `{line}:`"));
    }
    if line.ends_with("aithe:") || line.ends_with("unnanthasepu:") {
        if let Some(i) = single_eq(line) {
            return Some(format!(
                "polchadaniki `==` vaadandi (`=` kaadhu): `{}=={}`",
                &line[..i],
                &line[i + 1..]
            ));
        }
    }
    None
}

/// The byte offset of a lone `=` (not part of `==`, `!=`, `<=`, `>=`).
fn single_eq(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    (0..b.len()).find(|&i| {
        b[i] == b'='
            && b.get(i + 1) != Some(&b'=')
            && (i == 0 || !matches!(b[i - 1], b'=' | b'!' | b'<' | b'>'))
    })
}

fn missing_verb(line: &str) -> Option<String> {
    let ends_with_marker = ["ni", "tho", "ki", "lo"].iter().any(|m| line.ends_with(&format!(" {m}")));
    ends_with_marker.then(|| format!("chivarlo em cheyyalo cheppandi, ex: `{line} choopinchu()`"))
}

/// `"hello" ni choopinchu` without the `()`.
fn missing_parens(line: &str) -> Option<String> {
    let last = line.rsplit(' ').next()?;
    (is_name(last) && line.contains(' ')).then(|| format!("verb tharvatha `()` marchipoyaru: `{line}()`"))
}

/// `print("hello")` in "Ante" order, with the "Ante" verb when we know it.
fn call_form(line: &str, marked: &str) -> Option<String> {
    let at = line.find(marked.trim_end_matches('(')).unwrap_or(0);
    let call = &line[at..];
    let open = call.find('(')?;
    let name = call[..open].trim();
    let close = matching_paren(call, open)?;
    let args = call[open + 1..close].trim();
    let english = match name {
        "print" | "println" | "printf" | "puts" | "echo" | "console.log" | "System.out.println"
        | "fmt.Println" => Some("choopinchu"),
        "len" | "length" | "size" | "count" => {
            return Some(format!("length kosam `{}.enni()`", if args.is_empty() { "x" } else { args }));
        }
        "input" | "raw_input" | "readline" | "readLine" | "scanf" | "gets" => {
            return Some(
                "keyboard nunchi oka line chadhavaalante `system.line_chadhuvu()` (`system thechuko` file paina)".into(),
            );
        }
        "str" | "String" | "toString" => {
            return Some(format!(
                "text ga maarchaalante `{}.padham_ga()`",
                if args.is_empty() { "x" } else { args }
            ))
        }
        "int" | "parseInt" | "Number" => {
            return Some(format!(
                "text ni number ga maarchaalante `{}.ankam_ga()` (idhi `sare_thappu` isthundhi)",
                if args.is_empty() { "x" } else { args }
            ))
        }
        _ => None,
    };
    let verb = english.unwrap_or(name);
    if !is_name(verb) {
        return None;
    }
    let rewrite = if args.is_empty() { format!("{verb}()") } else { format!("{args} ni {verb}()") };
    Some(match english {
        Some(_) => format!("\"Ante\" lo idhi `{rewrite}`: value mundhu, verb chivarlo"),
        None => format!("ila raayandi: `{rewrite}`"),
    })
}

fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_str = false;
    for (i, c) in s[open..].char_indices() {
        match c {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Words from other languages that have an "Ante" name.
fn english_word(word: &str) -> Option<String> {
    let ante = match word {
        "true" | "True" | "TRUE" => "avunu",
        "false" | "False" | "FALSE" => "kaadhu",
        "null" | "None" | "nil" | "NULL" | "undefined" | "nullptr" => "ledhu",
        "print" | "println" | "printf" | "puts" | "echo" | "console" => "choopinchu",
        "input" | "readline" | "scanf" => "system.line_chadhuvu",
        "break" => "aapey",
        "continue" => "vadhiley",
        "return" => "icchey",
        "self" | "this" => "naa",
        "int" | "i32" | "i64" | "integer" | "long" | "Int" | "Integer" | "usize" => "ankam",
        "float" | "double" | "f64" | "f32" | "Float" | "Double" => "amsam",
        "str" | "string" | "String" | "text" => "padham",
        "bool" | "boolean" | "Bool" | "Boolean" => "ato_ito",
        "char" | "Char" | "character" => "aksharam",
        "void" | "None_type" | "unit" => "khali",
        "list" | "array" | "vec" | "Vec" | "List" | "ArrayList" => "varusa",
        "dict" | "map" | "HashMap" | "Map" | "Dict" | "dictionary" | "object" => "jodi",
        "set" | "Set" | "HashSet" => "gumpu",
        "main" => "modhalu",
        _ => return None,
    };
    Some(format!("\"Ante\" lo `{word}` ki `{ante}`"))
}

/// Names a few letters away from `word`, from this file or the prelude.
fn did_you_mean(word: &str, src: &SourceFile) -> Option<String> {
    if !is_name(word) {
        return None;
    }
    const COMMON: &[&str] = &[
        "choopinchu",
        "modhalu",
        "icchey",
        "pettu",
        "aithe",
        "ledhante",
        "unnanthasepu",
        "okko",
        "maare",
        "ankam",
        "amsam",
        "padham",
        "ato_ito",
        "aksharam",
        "varusa",
        "jodi",
        "gumpu",
        "khali",
        "avunu",
        "kaadhu",
        "undhi",
        "ledhu",
        "sare",
        "thappu",
        "nijame",
        "aapey",
        "vadhiley",
        "padham_ga",
        "enni",
        "kompa_munigindhi",
        "system",
        "lekkalu",
        "thechuko",
    ];
    let limit = if word.chars().count() <= 4 { 1 } else { 2 };
    let mut best: Option<(usize, String)> = None;
    let words = src
        .text
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty() && w.chars().next().is_some_and(|c| !c.is_ascii_digit()));
    for cand in words.chain(COMMON.iter().copied()) {
        if cand == word {
            continue;
        }
        let dist = edit_distance(word, cand);
        if dist <= limit && best.as_ref().is_none_or(|(d, _)| dist < *d) {
            best = Some((dist, cand.to_string()));
        }
    }
    best.map(|(_, name)| format!("`{name}` ani anukunnara?"))
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 2 {
        return 3;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut row = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            row.push((prev[j] + cost).min(prev[j + 1] + 1).min(row[j] + 1));
        }
        prev = row;
    }
    prev[b.len()]
}

/// Text plus a number, or a whole number with a fraction.
fn mismatch(message: &str, line: &str) -> Option<String> {
    let text_and_number = |a: &str, b: &str| {
        (message.contains(&format!("`{a}` kavali")) && message.contains(&format!("`{b}` vachindhi")))
            || (message.contains(&format!("`{b}` kavali")) && message.contains(&format!("`{a}` vachindhi")))
    };
    if text_and_number("padham", "ankam") || text_and_number("padham", "amsam") {
        if let Some((left, right)) = plus_operands(line) {
            let piece = |s: &str| match s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(text) => text.to_string(),
                None if s.starts_with(|c: char| c.is_ascii_digit()) => s.to_string(),
                None => format!("{{{s}}}"),
            };
            return Some(format!(
                "text, number ni `+` tho kalapalem. Text kavalante f-string: `f\"{}{}\"`. Number kavalante text ni mundhu `.ankam_ga()` tho maarchandi",
                piece(left),
                piece(right)
            ));
        }
        return Some("text, number kalapalem. Text kavalante `f\"… {x}\"` leka `x.padham_ga()`; number kavalante `x.ankam_ga()`".into());
    }
    if text_and_number("ankam", "amsam") {
        return Some(
            "`ankam` (2), `amsam` (2.0) kalapalem. `x ni kolpothu_maarchu[amsam]()` tho maarchandi, leka number ni `2.0` laaga raayandi"
                .into(),
        );
    }
    None
}

/// The operands either side of the first `+` outside quotes.
fn plus_operands(line: &str) -> Option<(&str, &str)> {
    let mut in_str = false;
    let mut at = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '+' if !in_str && !line[i + 1..].starts_with('=') => {
                at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let at = at?;
    let left = operand_end(line[..at].trim_end())?;
    let right = operand_start(line[at + 1..].trim_start())?;
    Some((left, right))
}

fn operand_end(s: &str) -> Option<&str> {
    if let Some(inner) = s.strip_suffix('"') {
        let start = inner.rfind('"')?;
        return Some(&s[start..]);
    }
    let start = s.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).map_or(0, |i| i + 1);
    let w = &s[start..];
    (!w.is_empty()).then_some(w)
}

fn operand_start(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(&s[..end + 2]);
    }
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.')).unwrap_or(s.len());
    let w = &s[..end];
    (!w.is_empty()).then_some(w)
}

fn condition(line: &str) -> Option<String> {
    let cond = line.strip_suffix("aithe:").or_else(|| line.strip_suffix("unnanthasepu:"))?.trim();
    let kw = if line.ends_with("aithe:") { "aithe" } else { "unnanthasepu" };
    if let Some(i) = single_eq(cond) {
        return Some(format!("polchadaniki `==`: `{}=={} {kw}:`", &cond[..i], &cond[i + 1..]));
    }
    Some(format!("condition avunu/kaadhu avvali, ex: `{cond} != 0 {kw}:` leka `{cond} > 0 {kw}:`"))
}

/// `1 < x < 5` → `1 < x && x < 5`.
fn chained(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let ops = ["<", "<=", ">", ">=", "==", "!="];
    let i = words.iter().position(|w| ops.contains(w))?;
    let j = i + 2;
    if i == 0 || j + 1 >= words.len() || !ops.contains(&words[j]) {
        return None;
    }
    Some(format!(
        "rendu comparisons ni `&&` tho kalapandi: `{} {} {} && {} {} {}`",
        words[i - 1],
        words[i],
        words[i + 1],
        words[i + 1],
        words[j],
        words[j + 1]
    ))
}

/// A value used after it moved: usually it should have been borrowed.
fn moved(message: &str, src: &SourceFile) -> Option<String> {
    let name = first_quoted(message)?;
    let line_no: usize = message.split("(line ").nth(1)?.split(' ').next()?.parse().ok()?;
    if line_no == 0 || line_no > src.text.lines().count() {
        return None;
    }
    let at = strip_comment(src.line_text(line_no)).trim();
    if let Some((target, value)) = at.split_once(" ante ") {
        if value.trim() == name {
            return Some(format!(
                "copy avasaram lekapothe borrow cheyyandi: `{} ante &{name}`. Nijamga rendu copies kavalante `{} ante {name}.xerox()`",
                target.trim(),
                target.trim()
            ));
        }
    }
    Some(format!(
        "line {line_no} lo `{name}` function ki vellipoyindhi. Aa function parameter type ki `&` pettandi (ex: `x: &padham ni`): appudu adhi borrow chesukuntundhi, `{name}` meeke migiluthundhi. Nijamga copy kavalante `{name}.xerox()`"
    ))
}

/// Changing a name that wasn't declared `maare`.
fn not_maare(message: &str, src: &SourceFile, before: usize) -> Option<String> {
    let name = first_quoted(message)?;
    for n in (1..before).rev() {
        let line = strip_comment(src.line_text(n)).trim();
        if let Some(value) = line.strip_prefix(&format!("{name} ante ")) {
            return Some(format!(
                "maarchaalante line {n} lo `maare` pettandi: `maare {name} ante {}`",
                value.trim()
            ));
        }
    }
    Some(format!("maarchaalante declare chesetappudu `maare` pettandi: `maare {name} ante …`"))
}

fn declared_maare(src: &SourceFile, name: &str, before: usize) -> Option<usize> {
    (1..before).rev().find(|&n| strip_comment(src.line_text(n)).trim().starts_with(&format!("maare {name} ")))
}

fn first_quoted(message: &str) -> Option<&str> {
    message.split('`').nth(1)
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
