//! `antec fmt` (spec §7.4): canonical output, comments kept, and a fixed
//! point on every valid program in the repository.

use std::fs;
use std::path::{Path, PathBuf};

use antec::fmt::{format_source, FmtError};
use antec::source::SourceFile;

fn format(text: &str) -> String {
    let src = SourceFile::new("t.ante", text);
    match antec::on_big_stack(|| format_source(&src)) {
        Ok(out) => out,
        Err(FmtError::Invalid(d)) => {
            panic!("invalid input: {:?}", d.iter().map(|d| d.code).collect::<Vec<_>>())
        }
        Err(FmtError::Bug(why)) => panic!("formatter bug: {why}"),
    }
}

#[test]
fn messy_code_comes_out_canonical() {
    let messy = "\
# top
lekkalu   thechuko
@automatic Choopinchochu,Polchochu
rakam Point ante:
  x:ankam   # x coord
  y: ankam


kalupu(a: ankam ni,b: ankam tho)->ankam ante:   # adds
  # inside
  ( a+b )icchey
modhalu() -> khali ante:
  v ante [1,2 ,3]
  maare s ante 0
  v lo okko i ki:
      s += i*2+1


      # end of loop
  s>3 aithe:
     \"big\" ni choopinchu()
  ledhante:
     \"small\" ni choopinchu()
  t ante (1 ,)
  handlers ante [modhalu]
  ( handlers[0] )()
  10 ni, 20 tho kalupu() |> choopinchu()
# bye
";
    let want = "\
# top
lekkalu thechuko

@automatic Choopinchochu, Polchochu
rakam Point ante:
    x: ankam  # x coord
    y: ankam

kalupu(a: ankam ni, b: ankam tho) -> ankam ante:  # adds
    # inside
    (a + b) icchey

modhalu() -> khali ante:
    v ante [1, 2, 3]
    maare s ante 0
    v lo okko i ki:
        s += i * 2 + 1

        # end of loop
    s > 3 aithe:
        \"big\" ni choopinchu()
    ledhante:
        \"small\" ni choopinchu()
    t ante (1,)
    handlers ante [modhalu]
    (handlers[0])()
    10 ni, 20 tho kalupu() |> choopinchu()
# bye
";
    assert_eq!(format(messy), want);
    assert_eq!(format(want), want, "formatting is a fixed point");
}

#[test]
fn literals_keep_their_spelling() {
    let text = "modhalu() -> khali ante:\n    x ante 0xFF + 1_000\n    s ante \"tab\\there \\u{41}\"\n    c ante '\\n'\n";
    assert_eq!(format(text), text);
}

#[test]
fn long_signatures_wrap() {
    let text = "f(aaaaaaaaaaaa: ankam ni, bbbbbbbbbbbbbbbb: ankam tho, cccccccccccccccc: varusa[padham] lo, dddd: ankam kosam) -> ankam ante:\n    aaaaaaaaaaaa icchey\n";
    let want = "f(\n    aaaaaaaaaaaa: ankam ni,\n    bbbbbbbbbbbbbbbb: ankam tho,\n    cccccccccccccccc: varusa[padham] lo,\n    dddd: ankam kosam,\n) -> ankam ante:\n    aaaaaaaaaaaa icchey\n";
    assert_eq!(format(text), want);
}

#[test]
fn d4_parentheses_are_inserted() {
    // A call clause after the first list element needs parentheses (D4).
    let text = "modhalu() -> khali ante:\n    v ante [1, (2 ni f())]\n";
    assert_eq!(format(text), text);
}

#[test]
fn files_with_errors_are_left_alone() {
    let src = SourceFile::new("bad.ante", "modhalu() -> khali ante:\n    x ante (1\n");
    assert!(matches!(format_source(&src), Err(FmtError::Invalid(_))));
}

/// Every valid program in the repository formats to a fixed point.
#[test]
fn repository_programs_format_cleanly() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for p in entries.filter_map(|e| e.ok().map(|e| e.path())) {
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
    let mut files = Vec::new();
    walk(&root.join("tests/conformance"), &mut files);
    walk(&root.join("../examples"), &mut files);
    let mut formatted = 0;
    let failures = antec::on_big_stack(|| {
        let mut failures = Vec::new();
        for f in &files {
            let text = fs::read_to_string(f).unwrap_or_default();
            let src = SourceFile::new(f.display().to_string(), text);
            match format_source(&src) {
                Ok(out) => {
                    formatted += 1;
                    let again = format_source(&SourceFile::new("again.ante", out.clone()));
                    if !matches!(&again, Ok(o) if *o == out) {
                        failures.push(format!("{}: not a fixed point", f.display()));
                    }
                }
                Err(FmtError::Invalid(_)) => {} // compile-fail syntax cases
                Err(FmtError::Bug(why)) => failures.push(format!("{}: {why}", f.display())),
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(formatted > 100, "only {formatted} files formatted");
}

/// The examples in the repository are already formatted.
#[test]
fn examples_are_formatted() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    for e in fs::read_dir(dir).expect("examples").filter_map(|e| e.ok()) {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("._") || p.extension().is_none_or(|x| x != "ante") {
            continue;
        }
        let text = fs::read_to_string(&p).expect("readable");
        assert_eq!(format(&text), text, "{} needs `antec fmt`", p.display());
    }
}
