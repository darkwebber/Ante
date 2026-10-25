# antec

"Ante" programming language ki stage-0 compiler, Rust lo raasindhi. Language ni [`spec/ante_language_spec_1_0.md`](../spec/ante_language_spec_1_0.md) define chesthundhi. Ee compiler aa spec lo unna Appendix E milestones follow avthundhi.

| Milestone | Status |
|---|---|
| M1 Lexer and layout (§4) | ✅ done |
| M2 Parser and AST (§5–6) | ⏳ next |
| M3–M14 | inka start avvaledhu |

## Build and test

Repo root nunchi:

```bash
cargo test -p antec
```

```bash
cargo run -p antec -- emit tokens antec/tests/conformance/lex/L003_line_joining.ante
```

## Code ekkada undhi

| File | Em chesthundhi |
|---|---|
| `src/source.rs` | Source files, spans, line:col (columns Unicode characters lo count avthayi) |
| `src/diag.rs` | Diagnostics (error code + span + message) |
| `src/token.rs` | Tokens, keyword table |
| `src/lexer.rs` | Lexer (§4.1–4.6), layout: NEWLINE / INDENT / DEDENT, line joining (§4.2) |
| `src/main.rs` | `antec` CLI |
| `tests/lexer.rs` | Lexer unit tests |
| `tests/conformance.rs` | `tests/conformance/` lo unna `.ante` cases ni run chesthundhi (spec Appendix C format) |

## Gurthu pettukovalsinavi

- **Editions:** keyword table taruvatha edition-aware avvali ([docs/VERSIONING.md](../docs/VERSIONING.md#contributors-ki-compiler-design-lo-idhi-ela-affect-chesthundhi)). New code lo keywords ni global constant ani assume cheyyakandi.
- **exFAT drive:** Cargo "hard linking" warning isthundhi. Adhi harmless. `CARGO_TARGET_DIR` ni internal disk ki set chesthe pothundhi.
