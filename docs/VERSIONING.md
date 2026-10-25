# Versioning and breaking changes

"Ante" ippudu chaala early stage lo undhi. Community nunchi new words vasthayi, konni old words change avthayi. Anduke breaking changes expected. Kaani avi **planned ga, documented ga, migration tho** jaragali. Aa rules anni ee doc lo unnayi.

## Rendu version numbers

| Version | Example | Idhi dheeni gurinchi |
|---|---|---|
| **Toolchain version** (`antec`) | `v0.3.1` | Compiler, CLI, stdlib release. [SemVer](https://semver.org) follow avthundhi. Git tags, GitHub Releases, CHANGELOG anni idhe. |
| **Language edition** | `edition = "1.0"` (`ante.toml` lo) | "Ante" language yokka vocabulary, syntax, semantics. Spec dheenine describe chesthundhi. |

Rust laane, oke compiler chaala editions ni support chesthundhi. Edition maarina kuda, old code compile avthune untundhi.

## Phase 1: ippudu (`antec` 0.x, Edition 1.0 draft)

Edition 1.0 inka **draft**. Vocabulary inka settle avthondhi.

### 0.x lo version numbers ela maaruthayi

Cargo 0.x rules follow avthaam:

| Change | Bump | Example |
|---|---|---|
| Breaking change (word rename or remove, syntax change, behavior change) | **MINOR** | `0.3.4 → 0.4.0` |
| New feature (non-breaking), bug fix, perf | **PATCH** | `0.3.4 → 0.3.5` |

Simple ga cheppalante, `0.3.x` lo unna releases anni okadaanitho okati compatible. `0.4.0` ki upgrade chesthe, CHANGELOG lo "⚠ BREAKING CHANGES" section chadavandi.

Ee version bumps release-please automatic ga chesthundhi ([RELEASING.md](RELEASING.md)).

### Breaking change ki rules

Prathi breaking change ki ivanni kavali:

1. **Accepted proposal.** Vocabulary change aithe accept ayina [vocabulary proposal](https://github.com/darkwebber/Ante/issues/new?template=vocabulary_proposal.yml) issue. Syntax or semantics change aithe accept ayina [RFC](../rfcs/).
2. **PR title lo `!`**, inka **`BREAKING CHANGE:` footer** lo migration steps:
   ```text
   feat(vocab)!: rename `vadiley` to `vadhiley`

   BREAKING CHANGE: `vadiley` is now `vadhiley`. Run `antec migrate`
   or replace the word by hand.
   ```
3. **Deprecation period** (word rename aithe):
   - Rename ayina release lo old word kuda pani chesthundhi, kaani warning **W0100** ("ee word old di, `new_word` use cheyyandi") isthundhi.
   - **Next MINOR release** lo old word remove avthundhi (appudu adhi normal identifier ayipothundhi).
   - So, users ki kanisam oka MINOR release time untundhi.
4. **Migration:** `antec migrate` ready ayyaaka, prathi rename ki automatic rule add cheyyali. Appati varaku CHANGELOG lo manual steps raayali.
5. **Spec update:** Appendix A, aa word use ayina examples, Appendix G laanti change log. Spec header lo draft revision number (`r1`, `r2`, …) penchali.
6. **Tests:** conformance tests ni new word tho update cheyyali. Deprecated alias ki warning test add cheyyali.

### New keywords kuda breaking

New keyword add chesthe, aa word ni already variable name ga use chesina code break avthundhi. Anduke new keywords kuda `!` tho, MINOR bump tho vasthayi.

## Phase 2: `antec` 1.0.0, Edition 1 freeze

Ee conditions anni complete ayyaaka `antec` 1.0.0 release chesthaam:

- [ ] Spec lo Core conformance level (Appendix C) motham pass avvali (milestones M1–M11).
- [ ] **Vocabulary freeze period:** 60 days paatu new breaking vocabulary proposals accept cheyyam. Mundhe "last call" announce chesthaam.
- [ ] Docs, examples, `antec migrate` ready ga undali.

1.0.0 tharvatha:

- **Edition 1.0 frozen.** Edition 1 lo valid ayina prathi program, `antec` 1.x, 2.x, … prathi version lo compile avthundhi. Edition 1 lo inka **breaking changes undavu**.
- Toolchain SemVer normal ga: breaking CLI/stdlib changes ki MAJOR, features ki MINOR, fixes ki PATCH.

## Phase 3: Edition 2.0

1.0 tharvatha vachhe new vocabulary, breaking ideas anni **Edition 2** loki velthayi:

- GitHub lo `edition-2` label tho track chesthaam.
- Edition 2 ready ayyaaka, adhi oka **toolchain MINOR release** lo vasthundhi (ex: `antec` 1.6.0 lo Edition 2.0 support). Toolchain MAJOR bump avasaram ledhu, endhukante old code inka compile avthundhi.
- Package `ante.toml` lo `edition = "2.0"` pedithe new words use cheyyochu. Pettakapothe Edition 1 laane untundhi.
- `antec migrate --edition 2.0` automatic ga code ni update chesthundhi.
- Different editions lo unna packages okadaanitho okati link avthayi.

### Contributors ki: compiler design lo idhi ela affect chesthundhi

Edition support ni ippati nunchi gurthu pettukovali, lekapothe taruvatha big refactor avasaram avthundhi:

- **Vocabulary edition-aware ga undali.** Keyword table (`antec/src/token.rs`) edition batti select avvali. Ippudu oke table undhi, kaani new code adhi global constant ani assume cheyyakoodadhu.
- **Editions surface syntax ni matrame change chesthayi.** Parser edition batti tokens chadivi, **oke AST/HIR** isthundhi. Type checking, borrow checking, codegen ki edition gurinchi theliyakoodadhu.
- **Edition 1 grammar ni delete cheyyam.** Edition 2 kosam branch cheyyandi, replace kaadhu.

## Spec versioning

- Spec file: `spec/ante_language_spec_1_0.md` (Edition 1.0).
- Header lo `Revision: draft rN`. Prathi breaking change ki N penchali. Behavior maarani clarifications ki avasaram ledhu.
- Edition 2 vachhinappudu new file `spec/ante_language_spec_2_0.md`. Edition 1 file freeze avthundhi.

## Quick summary

| Nenu em chesthunna | Breaking? | PR title |
|---|---|---|
| New stdlib function | Kaadhu | `feat(stdlib): …` |
| Bug fix | Kaadhu | `fix(…): …` |
| Word rename | **Avunu** | `feat(vocab)!: …` + deprecation alias |
| New keyword | **Avunu** | `feat(vocab)!: …` |
| Error code change | **Avunu** (tools break avthayi) | `fix(…)!: …` |
| Spec clarification, behavior same | Kaadhu | `docs(spec): …` |
