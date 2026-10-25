use antec::source::SourceFile;

/// All diagnostics as `CODE @ line:col`, errors and warnings alike.
fn diags(src: &str) -> Vec<String> {
    let file = SourceFile::new("t.ante", src);
    antec::check_source(&file)
        .iter()
        .map(|d| {
            let (line, col) = file.line_col(d.span.start);
            format!("{} @ {line}:{col}", d.code)
        })
        .collect()
}

/// Diagnostics for statements inside `modhalu()` (line 1 is the header).
fn body(stmts: &str) -> Vec<String> {
    let indented: String = stmts.lines().map(|l| format!("    {l}\n")).collect();
    diags(&format!("modhalu() -> khali ante:\n{indented}"))
}

const NONE: [&str; 0] = [];

#[test]
fn unused_bindings() {
    assert_eq!(body("x ante 5"), ["W0001 @ 2:5"]);
    assert_eq!(body("_x ante 5"), NONE);
    assert_eq!(body("x ante 5\nx ni choopinchu()"), NONE);
    assert_eq!(body("(a, b) ante (1, 2)\na ni choopinchu()"), ["W0001 @ 2:9"]);
    assert_eq!(body("o ante undhi(1)\no batti:\n    undhi(v) aithe:\n        icchey\n    ledhu aithe:\n        icchey"), ["W0001 @ 4:15"]);
    assert_eq!(body("f ante (x: ankam ni) => 1\n2 ni f()"), ["W0001 @ 2:13"]);
}

#[test]
fn function_parameters_are_not_linted() {
    assert_eq!(diags("f(x: ankam ni) -> khali ante:\n    icchey\n"), NONE);
}

#[test]
fn unnecessary_maare() {
    assert_eq!(body("maare x ante 1\nx ni choopinchu()"), ["W0002 @ 2:11"]);
    assert_eq!(body("maare x ante 1\nx += 1"), NONE);
    assert_eq!(body("maare x ante 1\nx lo 2 pettu"), NONE);
    assert_eq!(body("maare xs ante [1]\nxs[0] lo 5 pettu"), NONE);
    assert_eq!(body("maare xs ante [1]\n5 ni xs.cherchu()"), NONE);
    assert_eq!(body("maare xs ante [1]\nr ante &maare xs\nr ni choopinchu()"), NONE);
}

#[test]
fn unreachable_code() {
    assert_eq!(body("icchey\n\"never\" ni choopinchu()\n\"also never\" ni choopinchu()"), ["W0003 @ 3:5"]);
    assert_eq!(body("kompa_munigindhi()\n\"never\" ni choopinchu()"), ["W0003 @ 3:5"]);
    assert_eq!(body("0 nunchi 3 lopu okko i ki:\n    aapey\n    i ni choopinchu()"), ["W0003 @ 4:9"]);
    assert_eq!(body("avunu aithe:\n    icchey\n\"reachable\" ni choopinchu()"), NONE);
}

#[test]
fn unused_imports() {
    assert_eq!(diags("lekkalu thechuko\nmodhalu() -> khali ante:\n    icchey\n"), ["W0005 @ 1:1"]);
    assert_eq!(
        diags("lekkalu thechuko\nmodhalu() -> khali ante:\n    x ante 2.0 ni lekkalu.sqrt()\n    x ni choopinchu()\n"),
        NONE
    );
    assert_eq!(
        diags(
            "file nunchi chadhuvu, FileThappu thechuko\nmodhalu() -> khali ante:\n    \"a\" ni chadhuvu()\n"
        ),
        ["W0005 @ 1:23"]
    );
}

#[test]
fn no_lints_while_there_are_errors() {
    assert_eq!(body("x ante y"), ["E0301 @ 2:12"]);
}
