# "Ante" ki contribute cheyyadam

Welcome! 🙏 "Ante" ni manam andaram kalisi build chesthunnam. Chinna typo fix aina, new word suggestion aina, compiler lo big feature aina, every bit helps. Mee first PR ki advance lo thanks, inka oka virtual Irani chai ☕.

Ee guide lo:

1. [Meeru ela help cheyyochu](#meeru-ela-help-cheyyochu)
2. [Setup](#setup)
3. [Workflow](#workflow)
4. [Commit / PR title format](#commit--pr-title-format)
5. [Tests](#tests)
6. [Spec changes](#spec-changes)
7. [New vocabulary propose cheyyadam](#new-vocabulary-propose-cheyyadam)
8. [RFCs (big changes)](#rfcs-big-changes)
9. [Code style](#code-style)
10. [Review and merge](#review-and-merge)
11. [License](#license)

## Meeru ela help cheyyochu

| Em cheyyochu | Ekkada |
|---|---|
| Bug report | [Bug report issue](https://github.com/darkwebber/Ante/issues/new?template=bug_report.yml) |
| New word / word change | [Vocabulary proposal](https://github.com/darkwebber/Ante/issues/new?template=vocabulary_proposal.yml) |
| New feature idea | [Feature request](https://github.com/darkwebber/Ante/issues/new?template=feature_request.yml) |
| Compiler code | `good first issue` label unna issues tho start cheyyandi |
| Tests | `antec/tests/conformance/` lo new `.ante` cases |
| Docs | README, ee guide, spec examples. Tinglish lo raayandi |

Big work start chese mundhu, oka issue open chesi cheppandi. Lekapothe iddharu same pani chesi, chivarlo "nenu already chesesaa" moment vasthundhi. 😅

## Setup

Kavalsindhi: [Rust](https://rustup.rs) 1.85 or latest, inka git.

```bash
git clone https://github.com/darkwebber/Ante.git
```

```bash
cd Ante
```

```bash
cargo test
```

PR pampe mundhu ee moodu pass avvali. CI kuda ivve check chesthundhi, kaabatti ikkade pass chesthe CI tho godava undadhu:

```bash
cargo fmt --all --check
```

Idhi marchipothe CI lo rustfmt fail avthundhi (manaki kuda okasari ayyindhi 😅). Commit mundhu automatic ga run avvalante oka git hook pettukondi:

```bash
printf '#!/bin/sh\nexec cargo fmt --all --check\n' > .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
```

"Ante" files (`examples/`, tests) maaristhe vaatini kuda format cheyyandi:

```bash
cargo run -p antec -- fmt examples
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

> **External drive (exFAT) meedha unte:** Cargo "hard linking" warning isthundhi. Adhi harmless. Vaddhu anukunte `CARGO_TARGET_DIR` ni internal disk lo oka folder ki set cheyyandi.

## Workflow

1. Repo ni fork cheyyandi.
2. `main` nunchi oka branch create cheyyandi: `feat/parser-ranges`, `fix/lexer-crlf`, `vocab/vadhiley-alias`.
3. Chinna, focused commits cheyyandi. Oka PR lo oka topic matrame.
4. Tests, docs update cheyyandi.
5. PR open cheyyandi. Template lo checklist fill cheyyandi.
6. CI green ayyaaka, review ayyaaka, maintainer **squash merge** chesthaaru. Anduke PR title chaala important. Adhe final commit message avthundhi, CHANGELOG lo kuda adhe kanipisthundhi.

## Commit / PR title format

Manam [Conventional Commits](https://www.conventionalcommits.org/) use chesthaam. Dheeni valla release-please automatic ga version bump chesi CHANGELOG raasthundhi.

```text
<type>(<scope>): <short summary>
```

| Type | Eppudu use cheyyali | 0.x lo version meedha effect |
|---|---|---|
| `feat` | new feature | patch |
| `fix` | bug fix | patch |
| `perf` | speed improvement | patch |
| `docs` | docs matrame | release undadhu |
| `refactor` | behavior same, code clean up | release undadhu |
| `test` | tests matrame | release undadhu |
| `ci`, `build`, `chore` | tooling | release undadhu |

Scopes: `lexer`, `parser`, `types`, `borrowck`, `codegen`, `stdlib`, `spec`, `vocab`, `cli`, `torch`, `python`.

**Breaking change** aithe type tharvatha `!` pettandi. Body lo `BREAKING CHANGE:` footer tho migration ela cheyyalo raayandi:

```text
feat(vocab)!: rename `vadiley` to `vadhiley`

BREAKING CHANGE: `vadiley` is now `vadhiley`. Run `antec migrate`
or replace the word by hand.
```

Examples:

```text
feat(parser): parse `ledhante … aithe:` else-if chains
fix(lexer): report E0103 at the first token of the line
docs(spec): clarify the D4 comma rule
```

## Tests

Behavior change chesthe test must. Rendu types:

1. **Rust unit tests:** `antec/tests/*.rs`
2. **Conformance tests:** `antec/tests/conformance/<area>/<ID>_<name>.ante`. Format spec [Appendix C](spec/ante_language_spec_1_0.md#appendix-c--conformance-suite) lo undhi:

```ante
# category: compile-fail
# expect-error: E0106 @ 5:12
# level: core
modhalu() -> khali ante:
    x ante 1__0
```

Programs run chese tests kuda raayochu (`run-pass`, `runtime-trap`):

```ante
# category: run-pass
# expect-stdout: 30
# level: core
kalupu(a: ankam ni, b: ankam tho) -> ankam ante:
    (a + b) icchey

modhalu() -> khali ante:
    10 ni, 20 tho kalupu() |> choopinchu()
```

Prathi `# expect-stdout:` oka output line. Ee tests interpreter tho run avthayi, `clang` unte native binary ga kuda (`tests/native.rs`, `-O0` lo `--release` lo rendu sarlu); anni chotla output same undali. `runtime-trap` ki `# expect-trap: T01 @ 7:12` (trap code + position; native lo T08/T09 ki position undadhu), `# expect-exit:` default ga run-pass ki 0, runtime-trap ki 101.

ID prefixes: `L` lexer, `S` syntax, `N` names, `PK` packages, `B` binding, `P` pipes, `T` types/arithmetic, `O` ownership, `M` match, `G` generics, `D` data, `W` warnings, `X` interop.

Error position eppudu aa mistake unna chotu ni point cheyyali: tappu value, tappu phrase, leka function peru (E0410 laantivi). Test raase mundhu `cargo run -p antec -- check <file>` tho message chadavandi, adhi friendly ga undha ani kuda choodandi. 😊

**Stdlib function add cheyyadam:** peru `antec/src/stdlib.rs` lo, signature `antec/std/<module>.ante` lo (built-in type methods `gunam <type>_panulu` block lo). Rendu sync lo lekapothe `cargo test` fail avthundhi.

**Package tests** (rendu or ekkuva modules kavalsinappudu): `antec/tests/conformance/packages/PK<number>_<name>/` lo `ante.toml` + `src/` pettandi. Expectations `ante.toml` modhatlo comments ga, file peru tho:

```toml
# category: compile-fail
# expect-error: E0303 @ src/modhalu.ante:1:14
# level: core
[package]
name = "pk002"
kind = "bin"
``` New ID theeskune mundhu, aa folder lo already unna numbers check cheyyandi.

> **Note:** compiler inka complete avvaledhu. Conformance runner ippatiki ready ayina stages matrame run chesthundhi. `compile-fail` cases lo expected error code, line:col match avvali, inka compiler ichhe **prathi** error oka `# expect-error:` line lo undali (extra error vasthe adhi kuda bug ae, cascade avvakoodadhu). Migatha categories lo, ippudu unna stages lo errors raakudadhu.

### Editor grammar

Kottha keyword add chesthe, [`editors/vscode/syntaxes/ante.tmLanguage.json`](editors/vscode/syntaxes/ante.tmLanguage.json) lo kuda add cheyyandi. Test:

```bash
cd editors/vscode && npm ci && npm test
```

Ee test compiler keyword table chadivi, prathi keyword highlight avthondho check chesthundhi. Grammar update marchipothe CI gurthu chesthundhi. 😉

## Spec changes

[`spec/ante_language_spec_1_0.md`](spec/ante_language_spec_1_0.md) ee project ki **source of truth**.

- Compiler behavior change chese PR, same PR lo spec ni kuda update cheyyali.
- Spec lo new error code add chesthe, Appendix B table lo kuda add cheyyali.
- Spec English lo untundhi, endhukante adhi formal, precise ga undali. Migatha docs anni Tinglish lo.

## New vocabulary propose cheyyadam

"Ante" ki main strength dhaani words. Anduke vocabulary changes ki oka proper process undhi. Idhi mana language lo assembly elections laantidhi, kaani godavalu lekunda. 🗳️

### Manchi word ki rules

Spec Appendix A lo unna rules:

| Rule | Meaning | Example |
|---|---|---|
| R1 | Intlo maatlade language, newspaper language kaadhu | `lekkalu` ✅, `ganitam` ❌ |
| R2 | Technical words English lo, grammar Telugu lo | `thread`, `file` ✅, `daaralu` ❌ |
| R3 | English word + Telugu suffix ok | `HashCheyochu`, `sort_cheyyi` |
| R4 | Aa place ki correct grammar form | `maare motham` (participle), `naa.peru` (possessive) |
| R5 | Pun ok, kaani meaning ardham ayyelaa undali | `aasajeevi` = unsigned (optimist eppudu negative avvadu) |
| R6 | Oke spelling, romanization rule A.2 prakaram | `thappu`, `ledhu` (dental త/ద ki `th`/`dh`) |

### Process

1. **Proposal:** [Vocabulary proposal](https://github.com/darkwebber/Ante/issues/new?template=vocabulary_proposal.yml) issue open cheyyandi. Ippudu unna word, mee new word, meaning, enduku better, example code raayandi.
2. **Discussion:** kanisam **7 days** community comments kosam open lo untundhi. 👍 / 👎 reactions kuda count chesthaam.
3. **Decision:** maintainers accept or reject chesi reason raasthaaru.
4. **Breaking aithe:** word already use lo unte (rename or remove), adhi breaking change. [docs/VERSIONING.md](docs/VERSIONING.md) lo unna deprecation rules follow avvali: old word oka release varaku warning tho pani chesthundhi, migration steps CHANGELOG lo untayi.
5. **Implementation:** spec (Appendix A + aa word use ayina anni places), lexer keyword table, tests, docs. Anni oke PR lo.

Kottha word add chesthe (old words emi change avvakunda) adhi normally breaking kaadhu. Kaani evaraina aa word ni already variable name ga use chesi unte, keyword ayyaaka vaalla code break avthundhi. Anduke new keywords ni kuda breaking ga treat chesthaam.

## RFCs (big changes)

Syntax, semantics, type system, ownership, or chaala words okesari change chese vaatiki RFC kavali:

1. [`rfcs/0000-template.md`](rfcs/0000-template.md) ni `rfcs/0000-mee-topic.md` ga copy cheyyandi.
2. Fill chesi PR open cheyyandi (title: `docs(rfc): <topic>`).
3. Kanisam 14 days discussion.
4. Accept aithe maintainer number isthaaru (`rfcs/0007-…`), merge chesthaaru. Implementation separate PRs lo.

## Code style

- `cargo fmt` format, `cargo clippy -D warnings` clean.
- Chuttu unna code style lone raayandi: names, comments, idioms.
- Comments lo spec section reference ivvandi, ex: `// spec §4.2.3`.
- Diagnostics: error code spec Appendix B nunchi theeskondi. Message fun ga, Tinglish lo raayandi ("verb ekkada?", "`a` ni already icchesav kada!"), kaani joke kosam info miss avvakoodadhu: actual type, expected type, fix ela cheyyalo cheppali. Developer ki error chusi navvu raavali, confusion kaadhu. 😄
- New dependency add chese mundhu issue lo discuss cheyyandi. Compiler ni light ga unchali.

## Review and merge

- CI lo anni jobs green avvali: fmt, clippy, tests (Linux, macOS, Windows), MSRV.
- Kanisam oka maintainer approval.
- PR title Conventional Commits format lo undali (bot check chesthundhi).
- Squash merge matrame.

Releases ela jaruguthaayo [docs/RELEASING.md](docs/RELEASING.md) lo undhi.

## License

Meeru separate ga cheppakapothe, meeru submit chese prathi contribution MIT OR Apache-2.0 kindha dual licensed avthundhi. Extra terms or conditions emi undavu. Details [README](README.md#license) lo.

Doubts unte issue open cheyyandi. Bhayapadakandi, ikkada evaru "silly question" ani anaru. 😊
