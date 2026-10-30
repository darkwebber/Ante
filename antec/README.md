# antec

"Ante" programming language ki stage-0 compiler, Rust lo raasindhi. Language ni [`spec/ante_language_spec_1_0.md`](../spec/ante_language_spec_1_0.md) define chesthundhi. Ee compiler aa spec lo unna Appendix E milestones follow avthundhi.

| Milestone | Status |
|---|---|
| M1 Lexer and layout (§4) | ✅ done |
| M2 Parser and AST (§5–6), `antec fmt` | ✅ done |
| M3 Names, modules, imports, visibility (§7–8) | ✅ done |
| M4 Type checker (§9–14) | ✅ done |
| M5 Call binding, pipes (§11–12) | ✅ M4 tho paatu done |
| M6 MIR and interpreter (Part V) | ✅ done |
| M7 LLVM code generation (native binaries) | ✅ done |
| M8 Borrow checker (§15) | ✅ done |
| M9 Generics, traits, closures (§13, §10.7) | ✅ done |
| M10 Standard library (§21) | ✅ done |
| M11 C FFI (§22) | ✅ done |
| M12 Tasks and sharing (§24) | ✅ done |
| M13–M24 | inka start avvaledhu (spec Appendix E) |

## Build and test

Repo root nunchi:

```bash
cargo test -p antec
```

```bash
cargo run -p antec -- check examples/ante_sundharaniki.ante
```

```bash
cargo run -p antec -- run examples/ante_sundharaniki.ante
```

```bash
cargo run -p antec -- emit mir examples/ante_sundharaniki.ante
```

```bash
cargo run -p antec -- build examples/ante_sundharaniki.ante -o sundharaniki
```

Fast ga run avvalante (build konchem slow, binary chaala fast):

```bash
cargo run -p antec -- build examples/ante_sundharaniki.ante -o sundharaniki --release
```

Code ni official style lo pettadaniki (folder isthe anni `.ante` files):

```bash
cargo run -p antec -- fmt examples
```

Program ki arguments ivvalante `--` tharvatha raayandi (`system.args()` lo vasthayi):

```bash
cargo run -p antec -- run examples/ante_sundharaniki.ante -- okati rendu
```

```bash
cargo run -p antec -- emit llvm examples/ante_sundharaniki.ante
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
| `src/fmt.rs` | `antec fmt`: syntax tree nunchi code ni malli raasthundhi, comments tho; output same AST ki parse avthundho, rendosaari format chesthe maaradho check chesi gaani file ni touch cheyyadhu |
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
| `src/typeck/results.rs` | Type checker nirnayalu (prathi call ae function ki, arguments ae order lo, auto borrows) MIR kosam |
| `src/mir/mod.rs` | MIR: basic blocks, places, moves, drops; `antec emit mir` printer |
| `src/mir/build.rs` | Body lowering: scopes, drops, `vellagane`, `?`, pattern matching, closures, calls |
| `src/mir/borrowck.rs` | Borrow checker (§15): moves and initialization (E0601, E0603, E0609), NLL loans (E0604–E0606) |
| `src/mir/elaborate.rs` | Drop elaboration: partial moves ki per-field drop flags, moved place ki assign chesthe double drop ledhu |
| `src/mir/mono.rs` | Monomorphization check: generic instances anni reachable aa ani, types aagakunda perigithe E0805 |
| `src/interp/mod.rs` | MIR interpreter: memory, frames, traps (T01–T10), drop order, constants (E1102) |
| `src/interp/native.rs` | Runtime stdlib: prelude, `padham`, `varusa`, `jodi`, `lekkalu`, `file`, `system`, display, iterators |
| `src/interp/value.rs` | Runtime values, addresses, insertion-order `jodi` |
| `src/codegen/mod.rs` | Native backend: monomorphization, type layouts, module assembly |
| `src/codegen/func.rs` | Okka MIR body → okka LLVM function: places, overflow checks, calls |
| `src/codegen/native.rs` | Stdlib, iterators, adapters native ga |
| `src/codegen/glue.rs` | Prathi type ki display, `==`, order, `xerox`, drop functions |
| `src/codegen/peephole.rs` | Chinna clean-up: okka block lo store tharvatha load ni forward chesthundhi, evaru chadavani stack slots theesesthundhi (IR chinnaga, clang fast ga) |
| `runtime/ante_rt.c` | Chinna C runtime: program start (`modhalu` thread, T08 guard), output, traps, text, sort, files, time |
| `tests/native.rs` | Prathi run-pass/runtime-trap test ni native ga `-O0` lo, `--release` lo build chesi output, exit code, trap code, position compare chesthundhi |
| `tests/fmt.rs` | `antec fmt` golden tests; repo lo prathi valid program fixed point ki format avvali |
| `tests/fuzz.rs` | Mutation fuzzing (deterministic): e input ichhina checker, formatter panic avvakoodadhu; lothaina nesting ki E0216 |
| `tests/cli.rs` | CLI: `--help`, unknown commands, `run -- args`, `-o` source ni overwrite cheyyakoodadhu, `[build]` settings, `fmt --check` |
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

## Programs ela run avthayi (short ga)

- **MIR.** Prathi function oka control-flow graph avthundhi: basic blocks, places (`_3.1`, `(*_2)`), moves, `drop` lu explicit ga. `antec emit mir file.ante` tho choodochu.
- **Scopes and drops.** Block ki oka frame: aa block lo declare chesina locals reverse order lo drop avthayi, mundhu `vellagane` blocks LIFO lo run avthayi. `icchey`, `aapey`, `vadhiley`, `?` anni ade cleanup chesthayi (§19.3–19.4). Move ayyina value malli drop avvadhu.
- **Interpreter.** Prathi local ki oka slot; reference ante slot + path. Overflow, bounds, sunna tho bhaagham anni spec laage trap avthayi (exit 101, `kompa munigindhi! … (file:line:col)`).
- **Constants** kuda interpreter tho ne lekkisthaam (check time lo); trap aithe E1102.
- **Implementation-defined choices** (§20.6): kindha [list](#implementation-defined-choices-206) choodandi.
- **Ippatiki levu:** `torch`/`python` calls, raw pointers, `memory`, `bayata "C"` calls (T05 tho aagipothayi; M11, M21–M22 lo vasthayi).

## Native binaries ela vasthayi (short ga)

- **Textual LLVM IR + clang.** `antec` LLVM IR text raasthundhi, `antec/runtime/ante_rt.c` tho kalipi `clang` ki isthundhi: default `-O0` (fast build), `--release` aithe `-O2`. Rust dependencies emi levu; `clang` (leka `$ANTE_CC`) unte chaalu.
- **Runtime cache.** `ante_rt.c` okkasari compile ayyi cache lo untundhi (`$ANTE_CACHE_DIR`, leka macOS lo `~/Library/Caches/antec`, Linux lo `~/.cache/antec`, Windows lo `%LOCALAPPDATA%\antec`). Runtime, compiler, target maarithe kottha object.
- **Temp files** prathi build ki kottha, guess cheyyaleni peru tho, owner matrame chadavagalige folder lo; build ayyaaka delete.
- **`[build]` settings** (`ante.toml`): `stack = "256MB"` `modhalu` thread stack (minimum 8MB, default 64MB), `target = "aarch64-unknown-linux-gnu"` laanti 64-bit triple clang ki `--target` ga velthundhi.
- **Stack overflow = T08.** `modhalu` thana sontha thread lo run avthundhi, stack kindha 1MB guard region tho. Adhi touch aithe crash kaakunda `kompa munigindhi! stack nindipoyindhi` (exit 101). Position ikkada theliyadhu, anduke message lo `(file:line:col)` undadhu (§20.3).
- **Monomorphization.** `modhalu` nunchi reachable aina prathi function, prathi concrete type combination ki okasari generate avthundhi. Trait calls compile time lone resolve avthayi.
- **Layouts.** `padham`/`varusa` = `{ptr, len, cap}`, `&mukka` = `{ptr, len}`, enums = `{i32 tag, payload}`, `pani` = `{fn, env}`.
- **Traps** interpreter laage: same message, same `file:line:col`, exit 101. Overflow checks LLVM `*.with.overflow` intrinsics tho.
- **Differential testing.** `tests/native.rs` prathi runtime conformance test ni native ga rendu optimization levels lo build chesi, interpreter expect chesina output tho compare chesthundhi.
- **Ippatiki levu (native lo):** `jodi`/`gumpu` (M10 lo hash table tho), `bayata "C"` calls, pointers, `memory` (M11), `torch`/`python`, inka konni standard library functions: `padham.baitlu`/`baitlu_nunchi`/`byte_mukka`, `file.baitlu_chadhuvu`/`baitlu_raayi`/`folderaa`/`list_cheyyi`/`folder_cheyyi`/`theesey`, `file.path_*`, `system.nadupu`. Ivi vaadithe `antec build` clear error isthundhi; `antec run` lo anni pani chesthayi.
- **Memory:** `padham`, `varusa`, `pette` values drop ayyinappudu vaati memory free avthundhi (literals static memory lo untayi, avi free avvavu). `s += t` text ni akkade append chesthundhi. Closure environments reference count tho untayi: last copy poyinappudu captures drop ayyi memory free avthundhi. Ippatiki free avvanidhi: iterator adapter state (user `Okkokkati` loop state kuda), okkokkatiki konni bytes. Move ayyina value ni malli vaadithe borrow checker (E0601) compile time lone aapesthundhi, kaabatti native program freed memory ni eppudu chadavadhu.

## Implementation-defined choices (§20.6)

| Vishayam | `antec` lo |
|---|---|
| Value layout | Native: paina [Layouts](#native-binaries-ela-vasthayi-short-ga). `size_of` ints/floats ki bytes, migathavi ki 8 |
| Stack size | `[build] stack`, default 64MB, minimum 8MB (reserve matrame chesthundhi, vaadinantha memory matrame theesukuntundhi). Native lo stack kindha 1MB guard; interpreter lo 8MB ki 20,000 calls (default 64MB ki 160,000, stack penchithe proportional ga ekkuva) |
| Nesting limits | Brackets, unary operators, blocks, types, patterns kalipi 512 levels; okka expression tree 1,000 levels varaku (`a + b + …` lo prathi operator oka level). Daati vellithe E0216 |
| Transcendental functions (`lekkalu.sin` etc.) | Platform C library (`libm`) / Rust `f64` methods; accuracy vaati documentation laage |
| `lekkalu.motham` / `dot` | Floats ni 8 lanes lo kalipi, chivarlo lanes ni order lo kaluputhayi |
| Collection elements drop order | `varusa`/arrays: modhati element nunchi chivari varaku. Records/tuples: fields reverse order lo (§19.3) |
| Hash seeds | Fixed (deterministic) |
| Constant evaluation step limit | Avasaram ledhu: constants lo loops, function calls raavu (E1101), kaabatti evaluation eppudu aagipothundhi |
| T08/T09 position | Interpreter lo position untundhi; native lo T08/T09 ki position undadhu. T15 (deadlock) ki position eppudu undadhu |
| Lothu data (list, tree) | Drop ki limit ledhu: rendu backends lo okko step ga, stack vaadakunda. `xerox`, `==`, order, display matram recursion: interpreter lo 200,000 levels (≈100,000 list nodes) daatithe T08; native lo stack meedha aadharapadi (default 64MB ki ≈200,000 nodes varaku ok) |
| File error text | Common errors (`aa file ledhu`, `permission ledhu`, `adhi file kaadhu, folder`, …) rendu backends lo same; migathavi OS message |
| Diagnostics text | Tinglish, Unicode characters lo columns |

## Telisina quirks (break cheyyalani try chesi kanukunnavi)

Ivi bugs kaavu, kaani telisi undali:

- **Text ni loop lo penchadam:** `s lo (s + x) pettu` prathisaari kotha `padham` chesthundhi, kaabatti loop lo quadratic. `s += x` vaadandi, adhi alaage penchuthundhi.
- **Verb mundhu index:** `i ni ps[i].pampu()` parse avvadhu (`ps[i]` generic call laaga chadhuvuthundhi). Mundhu `p ante &ps[i]`, tharvatha `i ni p.pampu()`.
- **Rendu calls madhya operator:** `a ni f() + b ni g()` raayalem; `(a ni f()) + (b ni g())` raayandi (E0201 ade cheputhundhi).
- **Chaala tasks okesaari wait chesthe:** anni okate condition variable meedha wait chesthayi, kaabatti prathi wake-up anni tasks ni leputhundhi. Vandala tasks ok; velalo block ayithe slow (2,000 tasks ki ≈15s).
- **Deadlock:** anni tasks time limit lekunda wait chesthunte program hang avvadhu, T15 tho aagipothundhi (§24.4). Okati input kosam leka C code lo block ayithe adhi wait kaadhu, program alaage untundhi.
- **Windows:** task trap ayinappudu values drop avthayi (MSVC `longjmp` unwinding, cleanup funclets tho). MinGW lo inka ledhu. Sort comparator lo trap ayithe runtime sort buffer Windows lo leak avthundhi.
- **`antec emit … | head`:** output madhyalo pipe close ayina panic avvadhu, migatha output vadilesthundhi.

## Gurthu pettukovalsinavi

- **Editions:** keyword table taruvatha edition-aware avvali ([docs/VERSIONING.md](../docs/VERSIONING.md#contributors-ki-compiler-design-lo-idhi-ela-affect-chesthundhi)). New code lo keywords ni global constant ani assume cheyyakandi.
- **exFAT drive:** Cargo "hard linking" warning isthundhi. Adhi harmless. `CARGO_TARGET_DIR` ni internal disk ki set chesthe pothundhi.
