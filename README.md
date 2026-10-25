# "Ante"

[![CI](https://github.com/darkwebber/Ante/actions/workflows/ci.yml/badge.svg)](https://github.com/darkwebber/Ante/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Telugu lo think cheyyi. Native speed lo run cheyyi.** 🚀

"Ante" oka statically typed, natively compiled programming language. Syntax motham Telugu grammar style lo untundhi: verb last lo vasthundhi, prathi argument ki `ni`, `tho`, `ki` laanti case marker untundhi. Main focus: machine learning, general software, inka PyTorch laanti libraries tho direct ga work cheyyadam.

Simple ga cheppalante: amma ki explain chesinattu code raayochu, kaani adhi C laaga fast ga run avthundhi. 😎

> **Status:** Ippudu chaala early stage lo unnam. Spec ready, kaani compiler (`antec`) lo ippatiki lexer (milestone M1) matrame ready. Simple ga cheppalante: code ni tokens ga mukkalu cheyyagaladhu, inka run cheyyaledhu. 🐣 Vocabulary inka change avvochu, breaking changes ela handle chestamo [versioning policy](docs/VERSIONING.md) lo undhi.

## "Ante" Sundharaniki! 🎬

"Ante" ki first welcome program. Idhi oka sentence laage chaduvuthundhi: *"Ante, Sundharaniki Telugu lo code cheyyadam vachindhanta!"* (Nani fans ki ardham ayyindhi kada 😉)

```ante
sundharaniki() -> khali ante:
    "Telugu lo code cheyyadam vachindhanta! 🎉" ni choopinchu()

vachindhanta(evariki: padham, emi: padham ni) -> khali ante:
    f"{evariki} {emi} vachindhanta! 🎉" ni choopinchu()

modhalu() -> khali ante:
    sundharaniki()
    "Ammaki", "WhatsApp lo voice notes pampadam" ni vachindhanta()
    "Meeku", "\"Ante\" lo code cheyyadam" ni vachindhanta()
```

```text
Telugu lo code cheyyadam vachindhanta! 🎉
Ammaki WhatsApp lo voice notes pampadam vachindhanta! 🎉
Meeku "Ante" lo code cheyyadam vachindhanta! 🎉
```

Em jaruguthondhi:

- `sundharaniki() -> khali ante:` ante "sundharaniki **ante**: …". Function definition kuda sentence laage untundhi. `khali` ante emi return cheyyadhu.
- `"…" ni choopinchu()`: value mundhu, verb (`choopinchu`) last lo. Manam maatlade laane.
- `"Ammaki", "…" ni vachindhanta()`: rendu arguments, rendo dhaaniki `ni` marker. Function peru eppudu chivarlo.
- `modhalu()` ante program start ayye chota.

Full file: [examples/ante_sundharaniki.ante](examples/ante_sundharaniki.ante).

## "Ante" ante enti?

Telugu lo "ante" ante "means". "Ante" language lo kuda adhe meaning:

```ante
peru ante "Ravi"        # peru ante "Ravi", idhi inka change avvadhu
maare motham ante 0     # maare = idhi change avvochu
motham += 10
```

Avunu, language peru kuda oka keyword. "Ante" ante `ante`. Ardham ayyindha? Ayithe meeru ready. 😄

## Konchem code choodandi

```ante
kalupu(a: ankam ni, b: ankam tho) -> ankam ante:
    (a + b) icchey

modhalu() -> khali ante:
    10 ni, 20 tho kalupu() |> choopinchu()    # 30
    20 tho, 10 ni kalupu() |> choopinchu()    # order maarina problem ledhu: 30
```

Manam maatlade laane: "10 ni 20 tho kalupu". Arguments mundhu, verb last lo. Case markers unnayi kaabatti, order maarchina compiler confuse avvadhu. Mana amma laage. 😌

Konni favourite words:

| "Ante" lo | Meaning | Programming lo |
|---|---|---|
| `aithe` / `ledhante` | ayithe / lekapothe | if / else |
| `unnanthasepu` | unnantha sepu | while |
| `icchey` | ichchey | return |
| `aasajeevi ankam` | aasha jeevi, eppudu negative avvadu 😄 | unsigned integer |
| `kompa_munigindhi()` | kompa munigindhi! 🏚️ | panic |
| `naa_risk:` | naa risk, nenu chusukunta | unsafe block |
| `CopyKottochu` / `.xerox()` | exam lo copy kottinattu (free, silent) / shop lo xerox (paisalu kharchu) | Copy / Clone |
| `trupthiga_kalupu` | full ayyaaka aapesthundhi, ika chaalu | saturating add |
| `jalleda_pattu` | jalleda lo vadakattu | filter |

Full list [spec Appendix A](spec/ante_language_spec_1_0.md#appendix-a--vocabulary-and-romanization) lo undhi.

## Enduku "Ante"?

- **Telugu vaallaki natural ga untundhi.** Words dictionary nunchi theesukoledhu, manam roju WhatsApp lo maatlade Tinglish nunchi theesukunnam.
- **Safe and fast.** Ownership, borrow checking, bounds checks, checked arithmetic, inka LLVM tho native code. Segfaults ki tata bye bye. 👋
- **ML ready (planned).** PyTorch ki native LibTorch binding vasthundhi, dhaaniki Python install avasaram ledhu. Migatha Python libraries kosam oka bridge untundhi.
- **Self-hosting goal.** First compiler ni Rust lo raasthunnam. Taruvatha "Ante" compiler ni "Ante" lone raasthaam, C laaga.
- **Errors kuda friendly ga.** Compiler mee meeda arustundhi kaani, Tinglish lo, preminchi. 💛
  ```text
  error[E0601]: `a` ni already `b` ki icchesav kada! Malli vaadalevu.
  ```

## Roadmap

| Milestone | Em untundhi | Status |
|---|---|---|
| M1 | Lexer and layout | ✅ done |
| M2 | Parser and AST | ⏳ next |
| M3–M11 | Names, types, borrow checker, LLVM codegen, stdlib, FFI | 🔜 |
| M12 | `@vegam` performance | 🔜 |
| M13 | PyTorch binding | 🔜 |
| M14 | Python bridge | 🔜 |
| Later | Self-hosting ("Ante" lo raasina `antec`) | 🔭 |

Full details kosam spec lo [Appendix E](spec/ante_language_spec_1_0.md#appendix-e--implementation-roadmap-for-antec) inka [Appendix I](spec/ante_language_spec_1_0.md#appendix-i--self-hosting-readiness) choodandi.

## Build cheyyadam

Kavalsindhi: Rust 1.85 or latest ([rustup.rs](https://rustup.rs)). Inka oka cup chai ☕ (optional, kaani recommended).

```bash
git clone https://github.com/darkwebber/Ante.git
```

```bash
cd Ante
```

```bash
cargo test
```

Oka file ni tokens ga choodalante:

```bash
cargo run -p antec -- emit tokens antec/tests/conformance/lex/L003_line_joining.ante
```

## Repo lo em undhi

```text
spec/          Language spec (Edition 1.0)
examples/      Example "Ante" programs ("Ante" Sundharaniki tho start cheyyandi 🎬)
antec/         Rust lo raasina stage-0 compiler
docs/          Versioning, release process
rfcs/          Big changes kosam RFCs
.github/       CI/CD workflows, issue and PR templates
```

## Doubts? (FAQ)

**Telugu raakapothe "Ante" use cheyyochaa?**
Cheyyochu! Keywords chinna, repeat ayye words. Oka week lo `aithe`, `ledhante` mee notlo untayi. Bonus ga konchem Telugu kuda vasthundhi. 😉

**`kompa_munigindhi()` serious ga panic ki peru aa?**
Avunu. Program crash ayye time lo idhi kanna correct word inkokati ledhu.

**Ippude production lo vaadochaa?**
Inka ledhu boss. Ippatiki compiler code ni chadavagaladhu (lexer), run cheyyaledhu. Star ⭐ kotti watch cheyyandi, or inka better, contribute cheyyandi!

**Maa ooru lo ee word vere laaga antaaru. Em cheyyali?**
Super! [Vocabulary proposal](https://github.com/darkwebber/Ante/issues/new?template=vocabulary_proposal.yml) open cheyyandi. "Ante" words community kalisi decide chesthundhi.

## Contribute cheyyandi 🙌

New words suggest cheyyadam nunchi compiler code raayadam varaku, anni welcome. First [CONTRIBUTING.md](CONTRIBUTING.md) chadavandi. Andaru [Code of Conduct](CODE_OF_CONDUCT.md) follow avvali.

## Name gurinchi

Ee project ki [jfecher/ante](https://github.com/jfecher/ante) (`.an` files use chese inkoka "Ante" language) tho em sambandham ledhu. Confusion avvakunda, mana files `.ante`, mana compiler `antec`.

## License

"Ante" ni ee rendu licenses lo meeku nachinadhi choose cheskovachu:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

### Contribution

Meeru separate ga cheppakapothe, "Ante" lo add cheyyadaniki meeru submit chese prathi contribution (Apache-2.0 lo define chesinattu) ee rendu licenses kindha dual licensed avthundhi. Extra terms or conditions emi undavu.

---

Telugu vaallu, Telugu nerchukune vaallu, compiler nerds, andari kosam prema tho build chesthunnam. 💛
