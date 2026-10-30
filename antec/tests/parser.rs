use antec::source::SourceFile;

/// Parses a whole module and prints it; asserts there were no diagnostics.
fn module(src: &str) -> String {
    let file = SourceFile::new("t.ante", src);
    let (module, diags) = antec::parse_source(&file);
    let rendered: Vec<String> = diags.iter().map(|d| d.render(&file)).collect();
    assert!(rendered.is_empty(), "unexpected diagnostics:\n{}", rendered.join(""));
    antec::pretty::module(&module.expect("parsed module"))
}

/// Parses statements inside a function body and prints them, one per line.
fn stmts(body: &str) -> String {
    let indented: String = body.lines().map(|l| format!("    {l}\n")).collect();
    let printed = module(&format!("f() -> khali ante:\n{indented}"));
    let mut lines: Vec<&str> = printed.lines().skip(1).collect();
    if let Some(last) = lines.last_mut() {
        *last = last.strip_suffix(')').unwrap_or(last);
    }
    lines.iter().map(|l| l.strip_prefix("  ").unwrap_or(l)).collect::<Vec<_>>().join("\n")
}

/// Error codes with positions, as `CODE @ line:col`.
fn errs(src: &str) -> Vec<String> {
    let file = SourceFile::new("t.ante", src);
    antec::parse_source(&file)
        .1
        .iter()
        .map(|d| {
            let (line, col) = file.line_col(d.span.start);
            format!("{} @ {line}:{col}", d.code)
        })
        .collect()
}

/// Errors for statements wrapped in a function body (line 1 is the header).
fn body_errs(body: &str) -> Vec<String> {
    let indented: String = body.lines().map(|l| format!("    {l}\n")).collect();
    errs(&format!("f() -> khali ante:\n{indented}"))
}

// ── D3: phrases and callees ────────────────────────────────────────────────

#[test]
fn case_marked_call() {
    assert_eq!(stmts("10 ni, 20 tho kalupu()"), "(call kalupu (ni 10) (tho 20))");
    assert_eq!(stmts("20 tho, 10 ni kalupu()"), "(call kalupu (tho 20) (ni 10))");
}

#[test]
fn named_labels() {
    assert_eq!(
        stmts(r#""Ravi" ni, "Vizag" ni ooru ga, 23 ni vayasu ga manishi_chesko()"#),
        r#"(call manishi_chesko (ni "Ravi") (ni "Vizag" :ooru) (ni 23 :vayasu))"#
    );
}

#[test]
fn arithmetic_binds_before_phrase() {
    assert_eq!(stmts("a + b ni f()"), "(call f (ni (+ a b)))");
    assert_eq!(stmts("(a ni f()) + b"), "(+ (call f (ni a)) b)");
}

#[test]
fn unmarked_phrase_and_zero_argument_calls() {
    assert_eq!(stmts("x choopinchu()"), "(call choopinchu x)");
    assert_eq!(stmts("f()"), "(call f)");
    assert_eq!(stmts("f() g()"), "(call g (call f))");
    assert_eq!(stmts("500 ni acc.jama()"), "(call acc.jama (ni 500))");
}

#[test]
fn spread_phrase() {
    assert_eq!(stmts("...numbers ni anni_kalupu()"), "(call anni_kalupu (ni ...numbers))");
}

#[test]
fn closure_argument_needs_parentheses() {
    assert_eq!(
        stmts("xs, ((x) => x * 2) ni prathi_dhaaniki()"),
        "(call prathi_dhaaniki xs (ni (closure (x) (* x 2))))"
    );
    assert_eq!(
        stmts("f ante theesuko (x: ankam32 ni) => x + n"),
        "(let f (theesuko closure (x: ankam32 ni) (+ x n)))"
    );
}

#[test]
fn borrows_in_phrases() {
    assert_eq!(
        stmts("&maare model ni, &maare opt tho, x kosam, y ki train_step()"),
        "(call train_step (ni (&maare model)) (tho (&maare opt)) (kosam x) (ki y))"
    );
}

// ── D5: ranges vs `nunchi` role ────────────────────────────────────────────

#[test]
fn nunchi_is_a_role_unless_a_range_follows() {
    assert_eq!(stmts(r#""f.txt" nunchi chadhuvu()"#), r#"(call chadhuvu (nunchi "f.txt"))"#);
    assert_eq!(stmts("r ante 0 nunchi n lopu"), "(let r (lopu 0 n))");
    assert_eq!(stmts("r ante 1 nunchi 5 varaku"), "(let r (varaku 1 5))");
    assert_eq!(stmts("m ante xs[1 nunchi 3 lopu]"), "(let m (index xs (lopu 1 3)))");
}

// ── D4: commas inside brackets ─────────────────────────────────────────────

#[test]
fn commas_in_brackets() {
    assert_eq!(stmts("t ante (a, b)"), "(let t (tuple a b))");
    assert_eq!(stmts("t ante (a, b f())"), "(let t (call f a b))");
    assert_eq!(stmts("t ante (x,)"), "(let t (tuple x))");
    assert_eq!(stmts("c ante Chettu.Komma(x, 5, (y ni f()))"), "(let c (Chettu.Komma x 5 (call f (ni y))))");
    assert_eq!(
        stmts("m ante Model{l1: 784 ni, 128 ni Linear(), l2: x}"),
        "(let m (record Model (l1 (call Linear (ni 784) (ni 128))) (l2 x)))"
    );
    assert_eq!(stmts(r#"j ante {"Ravi": 23, "Sita": 25}"#), r#"(let j (map ("Ravi" 23) ("Sita" 25)))"#);
    assert_eq!(stmts("xs ante [1, 2, 3]"), "(let xs (list 1 2 3))");
}

// ── D7, D10, D11: generic calls, constructors, `?` ─────────────────────────

#[test]
fn generic_calls_and_indexing() {
    assert_eq!(stmts("y ante v ni maarchu[ankam64]()"), "(let y (call maarchu[ankam64] (ni v)))");
    assert_eq!(stmts("(handlers[0])()"), "(call (index handlers 0))");
    assert_eq!(stmts("v ante varusa.kotthadhi[ankam32]()"), "(let v (call varusa.kotthadhi[ankam32]))");
    assert_eq!(stmts("c ante t[_, -1]"), "(let c (index t _ (neg 1)))");
}

#[test]
fn constructors() {
    assert_eq!(stmts("o ante undhi(5)"), "(let o (undhi 5))");
    assert_eq!(stmts("sare(n * 2) icchey"), "(icchey (sare (* n 2)))");
    assert_eq!(stmts("sare icchey"), "(icchey sare)");
    assert_eq!(stmts("e ante ledhu"), "(let e ledhu)");
}

#[test]
fn try_applies_to_the_whole_call() {
    assert_eq!(stmts("t ante \"a.txt\" ni chadhuvu()?"), "(let t (? (call chadhuvu (ni \"a.txt\"))))");
    assert_eq!(stmts("text |> parse()? |> compute()"), "(|> (|> text (? (call parse))) (call compute))");
}

// ── pipes ──────────────────────────────────────────────────────────────────

#[test]
fn pipes_and_holes() {
    assert_eq!(stmts("10 |> 20 tho kalupu()"), "(|> 10 (call kalupu (tho 20)))");
    assert_eq!(stmts("x |> _ ni a ga, 5 tho kalupu()"), "(|> x (call kalupu (ni _ :a) (tho 5)))");
    assert_eq!(
        stmts("marks |> 3 ni modhati() |> choopinchu()"),
        "(|> (|> marks (call modhati (ni 3))) (call choopinchu))"
    );
}

// ── expressions ────────────────────────────────────────────────────────────

#[test]
fn precedence() {
    assert_eq!(stmts("v ante a + b * c == d && e"), "(let v (&& (== (+ a (* b c)) d) e))");
    assert_eq!(stmts("v ante x @ w + b"), "(let v (+ (@ x w) b))");
    assert_eq!(stmts("v ante 1 << 4 | m & 7"), "(let v (| (<< 1 4) (& m 7)))");
    assert_eq!(stmts("v ante -x.y[0]"), "(let v (neg (index x.y 0)))");
    assert_eq!(stmts("v ante !avunu || kaadhu"), "(let v (|| (! avunu) kaadhu))");
}

#[test]
fn fields_and_tuple_indices() {
    assert_eq!(stmts("v ante t.0.1"), "(let v (. (. t 0) 1))");
    assert_eq!(stmts("v ante (f()).peru"), "(let v (. (call f) peru))");
}

#[test]
fn fstrings() {
    assert_eq!(
        stmts(r#"s ante f"{m.peru} ki {a + 1} ellu""#),
        r#"(let s (fstr m.peru " ki " (+ a 1) " ellu"))"#
    );
}

// ── statements (D2) ────────────────────────────────────────────────────────

#[test]
fn bindings_and_assignment() {
    assert_eq!(stmts(r#"peru ante "Ravi""#), r#"(let peru "Ravi")"#);
    assert_eq!(stmts("maare motham: ankam32 ante 0"), "(let maare motham: ankam32 0)");
    assert_eq!(stmts("(a, b) ante p"), "(let (a b) p)");
    assert_eq!(stmts("x lo y ni f() pettu"), "(pettu x (call f (ni y)))");
    assert_eq!(stmts("*r lo 5 pettu"), "(pettu (* r) 5)");
    assert_eq!(stmts("naa.balance += amount"), "(+= naa.balance amount)");
    assert_eq!(stmts("(a + b) icchey"), "(icchey (+ a b))");
    assert_eq!(stmts("icchey"), "(icchey)");
}

#[test]
fn if_else_chain() {
    let src = "\
score >= 90 aithe:
    \"A\" ni choopinchu()
ledhante score >= 75 aithe:
    \"B\" ni choopinchu()
ledhante:
    \"C\" ni choopinchu()";
    assert_eq!(
        stmts(src),
        "(aithe
  ((>= score 90)
    (call choopinchu (ni \"A\")))
  ((>= score 75)
    (call choopinchu (ni \"B\")))
  (ledhante
    (call choopinchu (ni \"C\"))))"
    );
}

#[test]
fn loops() {
    let src = "\
0 nunchi 5 lopu okko i ki:
    i ni choopinchu()
a, b kalipi() lo okko (x, y) ki:
    total += x * y
    x > 0 aithe:
        vadhiley
    aapey
x > 0 unnanthasepu:
    x -= 1";
    assert_eq!(
        stmts(src),
        "(okko i (lopu 0 5)
  (call choopinchu (ni i)))
(okko (tuple x y) (call kalipi a b)
  (+= total (* x y))
  (aithe
    ((> x 0)
      (vadhiley)))
  (aapey))
(unnanthasepu (> x 0)
  (-= x 1))"
    );
}

#[test]
fn match_with_or_range_and_variant_patterns() {
    let src = "\
c batti:
    'a' nunchi 'z' varaku | '_' aithe:
        avunu icchey
    Chettu.Komma(e, maare v, _) aithe:
        v icchey
    undhi(Account{balance: b, ..}) aithe:
        b icchey
    ledhu aithe:
        kaadhu icchey";
    assert_eq!(
        stmts(src),
        "(batti c
  ((| (varaku 'a' 'z') '_')
    (icchey avunu))
  ((Chettu.Komma e (maare v) _)
    (icchey v))
  ((undhi (record Account (balance b) ..))
    (icchey b))
  (ledhu
    (icchey kaadhu)))"
    );
}

#[test]
fn defer_and_unsafe_blocks() {
    let src = "\
vellagane:
    f.close()
naa_risk:
    p ni free()";
    assert_eq!(
        stmts(src),
        "(vellagane
  (call f.close))
(naa_risk
  (call free (ni p)))"
    );
}

// ── items (D1) ─────────────────────────────────────────────────────────────

#[test]
fn items() {
    let src = "\
file nunchi chadhuvu, FileThappu thechuko
torch thechuko
lekkalu ni l ga thechuko

PI: amsam64 ante 3.14159

@automatic Polchochu, Choopinchochu
andhariki rakam Manishi ante:
    andhariki peru: padham
    vayasu: ankam32

edho_okati Chettu ante:
    Aaku
    Komma(pette[Chettu], ankam, pette[Chettu])

Account panulu ante:
    kotthadhi() -> Account ante:
        Account{balance: 0} icchey

gunam Choopinchochu ante:
    padham_ga(naa: &idhi) -> padham

[T: Choopinchochu] varusa[T] ki Choopinchochu gunam undhi:
    padham_ga(naa: &idhi) -> padham ante:
        \"[]\" icchey

Point ki CopyKottochu gunam undhi

bayata \"C\" ante:
    sqrt(x: amsam64) -> amsam64

@twaraga
dot[T, N: ankam](a: &mukka[T] ni, migathavi xs: ankam ni, peru: padham ni peru_thone = \"x\") -> pani[(ankam ni) -> ankam] ante:
    a icchey
";
    assert_eq!(
        module(src),
        "(import file (chadhuvu FileThappu))
(import torch)
(import lekkalu as l)
(const PI amsam64 3.14159)
@automatic Polchochu, Choopinchochu
(andhariki rakam Manishi
  (andhariki peru padham)
  (vayasu ankam32))
(edho_okati Chettu
  Aaku
  (Komma pette[Chettu] ankam pette[Chettu]))
(panulu Account
  (fn kotthadhi () -> Account
    (icchey (record Account (balance 0)))))
(gunam Choopinchochu
  (sig padham_ga ((naa &idhi)) -> padham))
(impl[T: Choopinchochu] varusa[T] ki Choopinchochu
  (fn padham_ga ((naa &idhi)) -> padham
    (icchey \"[]\")))
(impl Point ki CopyKottochu)
(bayata \"C\"
  (sig sqrt ((x amsam64)) -> amsam64))
@twaraga
(fn dot[T, N: ankam] ((a &mukka[T] ni) (migathavi xs ankam ni) (peru padham ni peru_thone = \"x\")) -> pani[(ankam ni) -> ankam]
  (icchey a))
"
    );
}

#[test]
fn unsigned_array_and_pointer_types() {
    assert_eq!(
        stmts("x: [aasajeevi ankam8; 4] ante y\np: *maare khali ante q"),
        "(let x: [aasajeevi ankam8; 4] y)\n(let p: *maare khali q)"
    );
}

// ── errors ─────────────────────────────────────────────────────────────────

#[test]
fn missing_verb_and_comma() {
    assert_eq!(body_errs("10 ni"), ["E0202 @ 2:10"]);
    assert_eq!(body_errs("10 ni 20 tho kalupu()"), ["E0203 @ 2:11"]);
}

#[test]
fn python_style_call_is_rejected() {
    assert_eq!(body_errs("kalupu(1, 2)"), ["E0204 @ 2:5"]);
    assert_eq!(body_errs("x ni f(y)"), ["E0204 @ 2:10"]);
}

#[test]
fn nested_calls_need_parentheses() {
    assert_eq!(body_errs("x ni f() g()"), ["E0201 @ 2:14"]);
}

#[test]
fn misc_syntax_errors() {
    assert_eq!(body_errs("x ante 1; y ante 2"), ["E0206 @ 2:13"]);
    assert_eq!(body_errs("5 ga f()"), ["E0205 @ 2:7"]);
    assert_eq!(body_errs("ledhante:\n    x ni f()"), ["E0207 @ 2:5"]);
    assert_eq!(body_errs("v ante (y += 1)"), ["E0210 @ 2:15"]);
    assert_eq!(body_errs("g() -> khali ante:\n    x ni f()"), ["E0211 @ 2:5"]);
    assert_eq!(body_errs("x |> a + b"), ["E0212 @ 2:10"]);
    assert_eq!(body_errs("x |> _ ni, _ tho f()"), ["E0213 @ 2:10"]);
    assert_eq!(body_errs("(a + b) lo 5 pettu"), ["E0215 @ 2:5"]);
    assert_eq!(body_errs("v ante a < b < c"), ["E0417 @ 2:18"]);
}

#[test]
fn control_flow_errors() {
    assert_eq!(body_errs("aapey"), ["E0213 @ 2:5"]);
    assert_eq!(body_errs("vellagane:\n    icchey"), ["E0214 @ 3:9"]);
}

#[test]
fn module_level_errors() {
    assert_eq!(errs("x ni choopinchu()\n"), ["E0209 @ 1:1"]);
    assert_eq!(errs("@pariksha\nx thechuko\n"), ["E0208 @ 1:1"]);
    assert_eq!(errs("f(ante: ankam) -> khali ante:\n    icchey\n"), ["E0307 @ 1:3"]);
    // A reserved word is reported once (E1201), not again as E0209.
    assert_eq!(errs("async\n"), ["E1201 @ 1:1"]);
    assert_eq!(errs("@pariksha\nx ante 1\n"), ["E0208 @ 1:1"]);
}

#[test]
fn pattern_errors() {
    assert_eq!(body_errs("x batti:\n    1.5 aithe:\n        icchey"), ["E0705 @ 3:9"]);
    assert_eq!(body_errs("x batti:\n    'z' nunchi 'a' varaku aithe:\n        icchey"), ["E0707 @ 3:9"]);
}

#[test]
fn errors_recover_at_the_next_line() {
    assert_eq!(body_errs("10 ni\nkalupu(1)\n20 ni"), ["E0202 @ 2:10", "E0204 @ 3:5", "E0202 @ 4:10"]);
}

/// Annotations on fields and variants, module paths in annotation arguments
/// and `sql"…"` literals parse (spec r6, §5.1 and §4.5).
#[test]
fn services_and_web_syntax_parses() {
    assert_eq!(
        module("@automatic web.Daari\nedho_okati P ante:\n    @daari \"/u/{id}\"\n    User(ankam)\n"),
        "@automatic web.Daari\n(edho_okati P\n  @daari \"/u/{id}\"\n  (User ankam))\n"
    );
    assert_eq!(
        module("rakam U ante:\n    @peru \"user_id\"\n    andhariki id: ankam\n"),
        "(rakam U\n  @peru \"user_id\"\n  (andhariki id ankam))\n"
    );
    assert_eq!(stmts("q ante sql\"SELECT {x}\""), "(let q (sql \"SELECT \" x))");
}

/// The checker refuses what the spec defines but `antec` doesn't do yet.
#[test]
fn planned_features_are_e1202() {
    let check = |src: &str| -> Vec<&'static str> {
        let file = SourceFile::new("t.ante", src);
        antec::check_source(&file).iter().map(|d| d.code).collect()
    };
    assert_eq!(check("modhalu() -> khali ante:\n    x ante 1\n    q ante sql\"{x}\"\n"), ["E1202"]);
    assert_eq!(check("rakam U ante:\n    @peru \"a\"\n    id: ankam\n"), ["E1202"]);
    assert_eq!(check("rakam U ante:\n    @pariksha\n    id: ankam\n"), ["E1307"]);
    assert_eq!(errs("x: edhaina ante 1\n").first().map(|e| &e[..5]), Some("E1202"));
}

/// Words renamed in spec r7 still work for one release, with W0100.
#[test]
fn renamed_words_warn_w0100() {
    let codes = |src: &str| -> Vec<&'static str> {
        let file = SourceFile::new("t.ante", src);
        antec::check_source(&file).iter().map(|d| d.code).collect()
    };
    let old = "rakam D ante:\n    p: ankam\n\nD ki Sheshamochu gunam undhi:\n    migulu(naa: &idhi, inkokati: &idhi ni) -> idhi ante:\n        D{p: naa.p % inkokati.p} icchey\n\n@vegam\nf(x: ankam ni) -> ankam ante:\n    x icchey\n\nmodhalu() -> khali ante:\n    s ante \"abc\"\n    t ante 0 ni, 1 tho s.byte_bhaagam()\n    (D{p: 7} % D{p: 4}).p ni choopinchu()\n    (1 ni f()) ni choopinchu()\n    t.sareyaa() ni choopinchu()\n";
    assert_eq!(codes(old), ["W0100", "W0100", "W0100"]);
    let new = old
        .replace("Sheshamochu", "Migulochu")
        .replace("@vegam", "@twaraga")
        .replace("byte_bhaagam", "byte_mukka");
    assert!(codes(&new).is_empty(), "{:?}", codes(&new));
}
