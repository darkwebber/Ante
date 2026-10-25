use antec::source::SourceFile;

/// Error codes with positions for a lone file, as `CODE @ line:col`.
fn errs(src: &str) -> Vec<String> {
    let file = SourceFile::new("t.ante", src);
    antec::check_source(&file)
        .iter()
        .filter(|d| d.is_error())
        .map(|d| {
            let (line, col) = file.line_col(d.span.start);
            format!("{} @ {line}:{col}", d.code)
        })
        .collect()
}

/// Errors for statements inside `modhalu()` (line 1 is the header).
fn body(stmts: &str) -> Vec<String> {
    let indented: String = stmts.lines().map(|l| format!("    {l}\n")).collect();
    errs(&format!("modhalu() -> khali ante:\n{indented}"))
}

fn ok(src: &str) {
    assert_eq!(errs(src), Vec::<String>::new(), "expected no errors for:\n{src}");
}

// ── locals and scopes (§8) ─────────────────────────────────────────────────

#[test]
fn undefined_name() {
    assert_eq!(body("x ni choopinchu()"), ["E0301 @ 2:5"]);
    assert_eq!(body("choopnchu()"), ["E0301 @ 2:5"]);
}

#[test]
fn use_before_definition() {
    assert_eq!(body("x ni choopinchu()\nx ante 5"), ["E0306 @ 2:5"]);
    assert_eq!(body("x ante x + 1"), ["E0306 @ 2:12"]);
}

#[test]
fn shadowing_only_in_nested_scopes() {
    assert!(body("x ante 1\navunu aithe:\n    x ante 2\n    x ni choopinchu()").is_empty());
    assert_eq!(body("x ante 1\nx ante 2"), ["E0308 @ 3:5"]);
    assert_eq!(body("(a, a) ante (1, 2)"), ["E0308 @ 2:9"]);
    assert_eq!(errs("f(a: ankam, a: ankam) -> khali ante:\n    icchey\n"), ["E0308 @ 1:13"]);
}

#[test]
fn loop_closure_and_arm_scopes() {
    assert_eq!(body("0 nunchi 3 lopu okko i ki:\n    i ni choopinchu()\ni ni choopinchu()"), ["E0301 @ 4:5"]);
    assert!(body("f ante (x: ankam ni) => x * 2\n3 ni f()").is_empty());
    assert_eq!(body("f ante (x: ankam ni) => x * 2\nx ni choopinchu()"), ["E0301 @ 3:5"]);
    assert_eq!(
        body("o ante undhi(1)\no batti:\n    undhi(v) aithe:\n        v ni choopinchu()\n    ledhu aithe:\n        v ni choopinchu()"),
        ["E0301 @ 7:13"]
    );
}

#[test]
fn pattern_bindings() {
    assert_eq!(body("p ante (1, 1)\np batti:\n    (x, x) aithe:\n        icchey"), ["E0308 @ 4:13"]);
    assert_eq!(body("p ante (1, 1)\np batti:\n    (x, 0) | (0, y) aithe:\n        icchey"), ["E0706 @ 4:18"]);
    assert!(body("p ante (1, 1)\np batti:\n    (x, 0) | (0, x) aithe:\n        x ni choopinchu()").is_empty());
}

// ── module items (§7) ──────────────────────────────────────────────────────

#[test]
fn duplicate_items() {
    assert_eq!(errs("f() -> khali ante:\n    icchey\nf() -> khali ante:\n    icchey\n"), ["E0302 @ 3:1"]);
    assert_eq!(errs("rakam P ante:\n    x: ankam\n    x: ankam\n"), ["E0302 @ 3:5"]);
    assert_eq!(errs("edho_okati R ante:\n    A\n    A\n"), ["E0302 @ 3:5"]);
    assert_eq!(
        errs("modhalu() -> khali ante:\n    icchey\nmodhalu() -> khali ante:\n    icchey\n"),
        ["E0904 @ 3:1"]
    );
}

#[test]
fn items_are_order_independent() {
    ok("modhalu() -> khali ante:\n    p ante later()\n\nlater() -> Point ante:\n    Point{x: 1} icchey\n\nrakam Point ante:\n    x: ankam\n");
}

#[test]
fn constant_cycles() {
    assert_eq!(errs("A: ankam ante B + 1\nB: ankam ante A\n"), ["E0902 @ 1:1"]);
    ok("A: ankam ante B + 1\nB: ankam ante 2\n");
}

// ── imports (§7.3) ─────────────────────────────────────────────────────────

#[test]
fn imports() {
    ok("file nunchi chadhuvu, FileThappu thechuko\nlekkalu ni l ga thechuko\nmodhalu() -> sare_thappu[khali, FileThappu] ante:\n    t ante \"a\" ni chadhuvu()?\n    x ante 2.0 ni l.sqrt()\n");
    assert_eq!(errs("lekka thechuko\n"), ["E0901 @ 1:1"]);
    assert_eq!(errs("file nunchi chadhuv thechuko\n"), ["E0304 @ 1:13"]);
}

#[test]
fn modules_need_importing_and_members_must_exist() {
    assert_eq!(body("x ante 2.0 ni lekkalu.sqrt()"), ["E0301 @ 2:19"]);
    assert_eq!(
        errs("lekkalu thechuko\nmodhalu() -> khali ante:\n    x ante 2.0 ni lekkalu.sqroot()\n"),
        ["E0301 @ 3:27"]
    );
}

#[test]
fn import_clash() {
    assert_eq!(
        errs("file nunchi chadhuvu thechuko\nchadhuvu() -> khali ante:\n    icchey\n"),
        ["E0302 @ 1:13"]
    );
}

#[test]
fn generated_bindings_accept_any_member() {
    ok("torch thechuko\npython nunchi transformers thechuko\nmodhalu() -> khali ante:\n    l ante 784 ni, 128 ni torch.nn.Linear()\n    t: torch.Tensor ante 3 ni torch.randn()\n    k ante transformers.AutoTokenizer\n");
}

// ── types, variants, receivers ─────────────────────────────────────────────

#[test]
fn types_and_generics() {
    ok("modatidhi[T](items: &mukka[T] ni) -> undochu[&T] ante:\n    ledhu icchey\n");
    assert_eq!(errs("f(x: Manishi) -> khali ante:\n    icchey\n"), ["E0301 @ 1:6"]);
    assert_eq!(errs("f(x: idhi) -> khali ante:\n    icchey\n"), ["E0301 @ 1:6"]);
}

#[test]
fn enum_variants() {
    let decl = "edho_okati Rangu ante:\n    Erupu\n    Neelam\n";
    ok(&format!("{decl}modhalu() -> khali ante:\n    r ante Rangu.Erupu\n    k ante Kramam.Thakkuva\n"));
    assert_eq!(errs(&format!("{decl}modhalu() -> khali ante:\n    r ante Rangu.Pink\n")), ["E0301 @ 5:18"]);
}

#[test]
fn receivers_and_pro_drop() {
    let account = "rakam Account ante:\n    balance: ankam\n\n";
    ok(&format!(
        "{account}Account panulu ante:\n    choopu(naa: &idhi) -> khali ante:\n        balance ni choopinchu()\n        naa.balance ni choopinchu()\n        perugu()\n    perugu(naa: &idhi) -> khali ante:\n        icchey\n"
    ));
    assert_eq!(
        errs(&format!("{account}Account panulu ante:\n    choopu(naa: &idhi) -> khali ante:\n        jeetham ni choopinchu()\n")),
        ["E0301 @ 6:9"]
    );
    assert_eq!(body("naa ni choopinchu()"), ["E0301 @ 2:5"]);
    assert_eq!(errs("f(naa: &ankam) -> khali ante:\n    icchey\n"), ["E0301 @ 1:3"]);
}

#[test]
fn traits_and_annotations() {
    let point = "rakam Point ante:\n    x: ankam\n";
    ok(&format!("@automatic Polchochu, Choopinchochu\n{point}Point ki CopyKottochu gunam undhi\n"));
    assert_eq!(errs(&format!("@automatic Point\n{point}")), ["E0301 @ 1:12"]);
    assert_eq!(errs(&format!("@vegaam\n{point}")), ["E0301 @ 1:2"]);
    assert_eq!(errs(&format!("{point}Point ki Point gunam undhi\n")), ["E0301 @ 3:10"]);
}

#[test]
fn private_types_in_public_signatures() {
    assert_eq!(
        errs("rakam Secret ante:\n    x: ankam\nandhariki f(s: Secret) -> khali ante:\n    icchey\n"),
        ["E0303 @ 3:16"]
    );
    ok("andhariki rakam Open ante:\n    x: ankam\nandhariki f(s: Open) -> khali ante:\n    icchey\n");
}
