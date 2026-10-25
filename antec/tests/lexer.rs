use antec::lexer::lex;
use antec::source::SourceFile;

/// Token stream as a space-separated string; asserts there were no errors.
fn toks(src: &str) -> String {
    let file = SourceFile::new("t.ante", src);
    let result = lex(&file);
    let errors: Vec<String> = result.diagnostics.iter().map(|d| d.render(&file)).collect();
    assert!(errors.is_empty(), "unexpected diagnostics:\n{}", errors.join(""));
    result.tokens.iter().map(|t| t.kind.to_string()).collect::<Vec<_>>().join(" ")
}

/// Diagnostics as `CODE @ line:col`.
fn errs(src: &str) -> Vec<String> {
    let file = SourceFile::new("t.ante", src);
    lex(&file)
        .diagnostics
        .iter()
        .map(|d| {
            let (line, col) = file.line_col(d.span.start);
            format!("{} @ {line}:{col}", d.code)
        })
        .collect()
}

#[test]
fn call_phrases() {
    assert_eq!(toks("10 ni, 20 tho kalupu()\n"), "int:10 kw:ni , int:20 kw:tho id:kalupu ( ) NEWLINE EOF");
}

#[test]
fn suites_indent_and_dedent() {
    let src = "f() -> khali ante:\n    x aithe:\n        y\n    z\n";
    assert_eq!(
        toks(src),
        "id:f ( ) -> kw:khali kw:ante : NEWLINE \
         INDENT id:x kw:aithe : NEWLINE \
         INDENT id:y NEWLINE \
         DEDENT id:z NEWLINE \
         DEDENT EOF"
    );
}

#[test]
fn dedents_at_end_of_file_without_newline() {
    assert_eq!(
        toks("f() -> khali ante:\n    x"),
        "id:f ( ) -> kw:khali kw:ante : NEWLINE INDENT id:x NEWLINE DEDENT EOF"
    );
}

#[test]
fn joins_inside_brackets() {
    assert_eq!(toks("x ante (1,\n2)\n"), "id:x kw:ante ( int:1 , int:2 ) NEWLINE EOF");
}

#[test]
fn joins_after_trailing_comma() {
    assert_eq!(toks("1 ni,\n2 tho f()\n"), "int:1 kw:ni , int:2 kw:tho id:f ( ) NEWLINE EOF");
}

#[test]
fn joins_before_and_after_pipe() {
    assert_eq!(toks("x\n|> f()\n"), "id:x |> id:f ( ) NEWLINE EOF");
    assert_eq!(toks("x |>\nf()\n"), "id:x |> id:f ( ) NEWLINE EOF");
}

#[test]
fn joins_more_indented_line() {
    assert_eq!(toks("y ni\n    choopinchu()\nz\n"), "id:y kw:ni id:choopinchu ( ) NEWLINE id:z NEWLINE EOF");
}

#[test]
fn joined_lines_keep_the_suite_structure() {
    let src = "\
kalupu_moodu(a: ankam ni,
             b: ankam ni)
    -> ankam ante:
    (a + b) icchey
";
    assert_eq!(
        toks(src),
        "id:kalupu_moodu ( id:a : kw:ankam kw:ni , id:b : kw:ankam kw:ni ) \
         -> kw:ankam kw:ante : NEWLINE \
         INDENT ( id:a + id:b ) kw:icchey NEWLINE DEDENT EOF"
    );
}

#[test]
fn blank_and_comment_lines_are_ignored() {
    assert_eq!(toks("x\n\n   # vyakhya\n  \ny\n"), "id:x NEWLINE id:y NEWLINE EOF");
}

#[test]
fn crlf_and_byte_order_mark() {
    assert_eq!(
        toks("\u{FEFF}x ante 1\r\ny ante 2\r\n"),
        "id:x kw:ante int:1 NEWLINE id:y kw:ante int:2 NEWLINE EOF"
    );
}

#[test]
fn keywords_versus_identifiers() {
    assert_eq!(
        toks("naa_risk naa ledhu ledhu_ayithe _ f sare_thappu @pariksha"),
        "kw:naa_risk kw:naa kw:ledhu id:ledhu_ayithe _ id:f kw:sare_thappu @ id:pariksha NEWLINE EOF"
    );
}

#[test]
fn every_keyword_round_trips() {
    for kw in antec::token::Keyword::ALL {
        assert_eq!(antec::token::Keyword::from_word(kw.as_str()), Some(*kw));
        assert_eq!(toks(kw.as_str()), format!("kw:{} NEWLINE EOF", kw.as_str()));
    }
}

#[test]
fn numbers() {
    assert_eq!(
        toks("1_000 0xFF 0b1011 0o17 1.5 1e-3 1.0e+4 2E5"),
        "int:1000 int:16:FF int:2:1011 int:8:17 float:1.5 float:1e-3 float:1.0e+4 float:2E5 NEWLINE EOF"
    );
}

#[test]
fn tuple_indices_are_not_floats() {
    assert_eq!(toks("t.0.1"), "id:t . int:0 . int:1 NEWLINE EOF");
    assert_eq!(toks("1.enni()"), "int:1 . id:enni ( ) NEWLINE EOF");
}

#[test]
fn malformed_numbers() {
    assert_eq!(
        errs("1__0\n0x_F\n1_\n1e\n123abc\n0b102\n0x\n"),
        [
            "E0106 @ 1:1",
            "E0106 @ 2:1",
            "E0106 @ 3:1",
            "E0106 @ 4:1",
            "E0106 @ 5:1",
            "E0106 @ 6:1",
            "E0106 @ 7:1"
        ]
    );
}

#[test]
fn strings_and_escapes() {
    assert_eq!(toks(r#""a\n\t\"\\\u{C24}" "తెలుగు""#), r#"str:"a\n\t\"\\త" str:"తెలుగు" NEWLINE EOF"#);
}

#[test]
fn string_errors() {
    assert_eq!(
        errs("\"abc\n\"\\q\"\n\"\\u{D800}\"\n\"\\u{110000}\"\n\"\\u{}\"\n"),
        ["E0104 @ 1:1", "E0105 @ 2:2", "E0108 @ 3:2", "E0108 @ 4:2", "E0105 @ 5:2"]
    );
}

#[test]
fn character_literals() {
    assert_eq!(toks(r"'a' '\n' 'త'"), r"char:'a' char:'\n' char:'త' NEWLINE EOF");
    assert_eq!(errs("'ab'\n''\n'a\n"), ["E0109 @ 1:1", "E0109 @ 2:1", "E0104 @ 3:1"]);
}

#[test]
fn fstring_parts() {
    assert_eq!(
        toks(r#"f"Hi {peru}, {{ok}} {a + 1}!""#),
        r#"fstr[text:"Hi " expr@6..10 text:", {ok} " expr@21..26 text:"!"] NEWLINE EOF"#
    );
}

#[test]
fn fstring_errors() {
    assert_eq!(errs("f\"a } b\"\nf\"{}\"\nf\"{x\"\n"), ["E0110 @ 1:5", "E0110 @ 2:3", "E0110 @ 3:3"]);
}

#[test]
fn operators_use_maximal_munch() {
    assert_eq!(
        toks("a |> b || c | d ... .. . -> -= => == != <= >= << >> && @ ~ ^ ? ; += *= /= %"),
        "id:a |> id:b || id:c | id:d ... .. . -> -= => == != <= >= << >> && @ ~ ^ ? ; += *= /= % NEWLINE EOF"
    );
}

#[test]
fn telugu_is_allowed_in_strings_and_comments() {
    assert_eq!(toks("x ante \"తెలుగు\" # తెలుగు\n"), "id:x kw:ante str:\"తెలుగు\" NEWLINE EOF");
}

#[test]
fn telugu_outside_literals_is_one_error() {
    assert_eq!(errs("x ante తెలుగు\n"), ["E0101 @ 1:8"]);
}

#[test]
fn backslash_continuation_is_rejected() {
    assert_eq!(errs("x ante 1 + \\\n    2\n"), ["E0201 @ 1:12"]);
}

#[test]
fn tab_in_indentation() {
    assert_eq!(errs("f() -> khali ante:\n\tx\n"), ["E0102 @ 2:1"]);
}

#[test]
fn dedent_to_unknown_level() {
    assert_eq!(errs("f() -> khali ante:\n    x aithe:\n        y\n      z\n"), ["E0103 @ 4:7"]);
}

#[test]
fn missing_indented_block() {
    assert_eq!(errs("f() -> khali ante:\nx\n"), ["E0201 @ 1:18"]);
    assert_eq!(errs("f() -> khali ante:\n"), ["E0201 @ 1:18"]);
}

#[test]
fn unexpected_indentation_on_first_line() {
    assert_eq!(errs("  x\n"), ["E0201 @ 1:3"]);
}

#[test]
fn bracket_errors() {
    assert_eq!(errs("x ante (1, 2\n"), ["E0201 @ 1:8"]);
    assert_eq!(errs("x ante (1]\n"), ["E0201 @ 1:10"]);
    assert_eq!(errs("x)\n"), ["E0201 @ 1:2"]);
}

#[test]
fn invalid_utf8_is_reported() {
    let (file, diags) = antec::source_from_bytes("t.ante", b"x ante 1\n\xff\n".to_vec());
    let found: Vec<String> = diags
        .iter()
        .map(|d| {
            let (line, col) = file.line_col(d.span.start);
            format!("{} @ {line}:{col}", d.code)
        })
        .collect();
    assert_eq!(found, ["E0101 @ 2:1"]);
}
