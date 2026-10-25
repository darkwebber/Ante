# antec

"Ante" programming language ki stage-0 compiler, Rust lo raasindhi. Language ni [`spec/ante_language_spec_1_0.md`](../spec/ante_language_spec_1_0.md) define chesthundhi. Ee compiler aa spec lo unna Appendix E milestones follow avthundhi.

| Milestone | Status |
|---|---|
| M1 Lexer and layout (§4) | ✅ done |
| M2 Parser and AST (§5–6) | ✅ done (`antec fmt` inka raavali) |
| M3 Names, modules, imports, visibility (§7–8) | ✅ done |
| M4 Type checker (§9–14) | ✅ done |
| M5 Call binding, pipes (§11–12) | ✅ M4 tho paatu done |
| M6 MIR and interpreter | ⏳ next |
| M7–M14 | inka start avvaledhu |

## Build and test

Repo root nunchi:

```bash
cargo test -p antec
```

```bash
cargo run -p antec -- check examples/ante_sundharaniki.ante
```

Package (`ante.toml` unna folder) motham check cheyyalante:

```bash
cargo run -p antec -- check antec/tests/conformance/packages/PK006_nested_modules_and_cycles
```

```bash
cargo run -p antec -- emit ast examples/ante_sundharaniki.ante
```

```bash
cargo run -p antec -- emit tokens examples/ante_sundharaniki.ante
```

## Code ekkada undhi

| File | Em chesthundhi |
|---|---|
| `src/source.rs` | Source files, spans, line:col (columns Unicode characters lo count avthayi) |
| `src/diag.rs` | Diagnostics (error code + span + message) |
| `src/token.rs` | Tokens, keyword table |
| `src/lexer.rs` | Lexer (§4.1–4.6), layout: NEWLINE / INDENT / DEDENT, line joining (§4.2) |
| `src/ast.rs` | Syntax tree (§5) |
| `src/parser.rs` | Recursive-descent parser, §6 disambiguation rules (D1–D12) |
| `src/pretty.rs` | `antec emit ast` kosam S-expression printer |
| `src/resolve.rs` | Name resolution (§7–8): namespaces, imports, scopes, privacy, `naa.` pro-drop, constant cycles |
| `src/stdlib.rs` | Prelude, standard library modules lo unna names (stdlib "Ante" source ga vachhe varaku) |
| `src/types.rs` | Types (`Ty`), messages lo types ela kanipisthayo, inference table (snapshot / rollback tho) |
| `src/typeck/mod.rs` | Type checker driver: records, enums, traits, functions, methods, implementations collect chesthundhi |
| `src/typeck/lower.rs` | Source lo raasina types ni `Ty` ga maarchadam |
| `src/typeck/body.rs` | Function bodies: statements, expressions, operators, `?`, `maare` checks, return paths, constants |
| `src/typeck/call.rs` | Calls: case-marker binding (§11.2), auto borrow, methods, pipes |
| `src/typeck/pattern.rs` | Patterns, binding modes, exhaustiveness (E0701), unreachable arms (W0004) |
| `src/typeck/solve.rs` | Unification, coercions (§9.4.2), trait solving |
| `src/typeck/items.rs` | Implementations (orphan, overlap, missing methods), `@automatic`, infinite types, `bayata` signatures |
| `src/typeck/std_lib.rs` | `std/*.ante` signatures load chesthundhi |
| `std/*.ante` | Standard library signatures, "Ante" lone raasinavi (prelude, `lekkalu`, `file`, `system`, `samayam`, `memory`) |
| `src/lint.rs` | Warnings (W0001 unused binding, W0002 unnecessary `maare`, W0003 unreachable code, W0005 unused import); errors lenappudu matrame |
| `src/package.rs` | `ante.toml` + `src/` packages, module discovery, entry point (`modhalu`) check |
| `src/main.rs` | `antec` CLI |
| `tests/lexer.rs` | Lexer unit tests |
| `tests/parser.rs` | Parser golden tests: prathi D-rule, prathi parser error code |
| `tests/resolve.rs` | Resolver tests: scopes, imports, variants, receivers, privacy |
| `tests/lint.rs` | Lint (warning) tests |
| `tests/conformance/types`, `calls`, `patterns`, `traits`, `ownership`, `ffi` | Type checker conformance cases (T, B, P, M, G, O, X series) |
| `tests/conformance.rs` | `tests/conformance/` cases (spec Appendix C format), `examples/`, inka spec/README lo unna prathi `ante` example parse avvali; truncated inputs tho parser panic avvakoodadhu |

## Parser ela pani chesthundhi (short ga)

- **Line first, details later.** Prathi statement ni first full logical line chusi classify chesthaam (D1 items ki, D2 statements ki): last token `pettu` aithe assignment, `icchey` aithe return, `:` aithe block header. Anduke chaala decisions ki backtracking avasaram ledhu.
- **Phrases + verb.** `clause` = phrases (`expr role? label?`, commas tho) + callee `path()`. Okka plain expression unte adhi normal expression (D3).
- **Backtracking rendu chotla matrame:** brackets lopala commas (D4: phrase list callee tho end aithe call, lekapothe list elements) inka `nunchi … lopu` ranges (D5).
- **Errors tharvatha continue:** oka statement lo error vasthe, aa logical line (inka dhaani block) skip chesi next line nunchi parse chesthundhi. Okka run lo chaala errors choopisthundhi.

## Names ela resolve avthayi (short ga)

- **Passes:** (1) prathi module lo declarations collect, (2) imports, (3) `panulu` methods (pro-drop kosam), (4) prathi function body scopes tho, (5) constant cycles.
- **Lookup order** (spec §8.6): local → module item / import → `naa` field or method (method lopala) → prelude → type / module / generic.
- **Result:** prathi peru deniki point chesthundho `Resolution::resolutions` lo save avthundhi (file + offset key tho). M4 type checker idhe vaadukuntundhi.
- **Stdlib:** names `stdlib.rs` lo, signatures `std/*.ante` lo undhi. Rendu sync lo unnayo `typeck::std_lib` test check chesthundhi. `torch`, `python` "open" modules: deentlo e peru aina accept chesthaam.

## Types ela check avthayi (short ga)

- **Collect, then check.** Mundhu prathi `rakam`, `edho_okati`, `gunam`, function signature collect chesthaam. Taruvatha prathi body ni dhaani signature tho check chesthaam (§10.1). Body lopala types unification tho kanukkuntaam; literals mundhu "edho ankam", chivarlo `ankam64` / `amsam64` default.
- **Stdlib signatures "Ante" lone.** `std/prelude.ante` lo `bayata "ante"` blocks (intrinsic functions) inka `gunam <type>_panulu` blocks (built-in types methods) untayi. Mana parser e vaatini chadhuvuthundhi. Kottha stdlib function add cheyyalante, ikkada signature raayandi.
- **Calls:** phrases ni case marker tho formals ki bind chesthaam (§11.2), `&T` formal ki owned value isthe auto borrow, `&maare` ki isthe E0610.
- **Speculation is cheap.** Trait implementations try chese appudu inference table snapshot teesukuntaam; fail aithe rollback. Full copy cheyyamu, kaabatti pedda functions kuda fast.
- **Errors first, warnings later.** Name errors unte types check cheyyamu; type errors unte warnings choopinchamu. Okka mistake ki pedda error list raadhu.

## Gurthu pettukovalsinavi

- **Editions:** keyword table taruvatha edition-aware avvali ([docs/VERSIONING.md](../docs/VERSIONING.md#contributors-ki-compiler-design-lo-idhi-ela-affect-chesthundhi)). New code lo keywords ni global constant ani assume cheyyakandi.
- **exFAT drive:** Cargo "hard linking" warning isthundhi. Adhi harmless. `CARGO_TARGET_DIR` ni internal disk ki set chesthe pothundhi.
