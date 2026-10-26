# Developer tools

Manchi language ki manchi tools kavali. Code raase time lo editor sahayam cheyyali, mistakes ni compiler twaraga, friendly ga cheppali. Ee doc lo ippudu emi ready ga undho, taruvatha emi vasthundho undhi. 🛠️

## Ippudu ready ✅

### Syntax highlighting (VS Code)

[`editors/vscode/`](../editors/vscode) lo VS Code extension undhi:

- **Highlighting:** keywords, case markers (`ni`, `tho`, `ki`…), verbs (`kalupu()`), types, f-strings, escapes, numbers, annotations. Case markers ki separate color, kaabatti phrase lo "evaru em chesthunnaro" oka chupulo kanipisthundhi.
- **Markdown lo kuda:** ```` ```ante ```` blocks highlight avthayi (README, spec).
- **Smart indentation:** `:` tharvatha auto indent, `icchey` / `aapey` / `vadhiley` tharvatha outdent, `.ante` files ki spaces (tabs E0102 kada 😉).
- **Snippets:** `sundharaniki` 🎬, `modhalu`, `pani`, `aithe`, `okko`, `batti`, `rakam`, `panulu`, `gunam`…

Install cheyyadam:

- **Release nunchi:** [GitHub Releases](https://github.com/darkwebber/Ante/releases) lo `ante-language-<version>.vsix` download chesi, VS Code lo **Extensions → … → Install from VSIX**.
- **Source nunchi:** [editors/vscode/README.md](../editors/vscode/README.md) choodandi.

Grammar TextMate format lo undhi ([`ante.tmLanguage.json`](../editors/vscode/syntaxes/ante.tmLanguage.json)), kaabatti **Sublime Text, JetBrains IDEs (TextMate bundles), Zed, Nova** laanti editors lo kuda adhe file vaadochu.

> **Grammar, compiler sync lo untayi:** `npm test` compiler keyword table (`antec/src/token.rs`) chadivi, prathi keyword highlight avthondho check chesthundhi. Kottha keyword add chesi grammar update marchipothe, CI fail avthundhi.

### Linter (`antec check` lo built-in)

`antec check` errors tho paatu **warnings** kuda chupisthundhi. Warnings build ni aapavu, kaani code ni clean ga unchuthayi:

| Code | Em chupisthundhi | Example |
|---|---|---|
| W0001 | Define chesi vaadani binding | `x ante 5` (x ekkada vaadaledhu) |
| W0002 | Eppudu maarani variable ki `maare` | `maare x ante 1` (x ki kottha value raaledhu) |
| W0003 | Eppudu run avvani code | `icchey` tharvatha statements |
| W0004 | Eppudu raani `batti` arm | `undhi(_)` tharvatha `undhi(2)` |
| W0005 | Vaadani import | `lekkalu thechuko` (lekkalu vaadaledhu) |

```text
warning[W0001]: `x` define chesaru kaani vaadaledhu. Avasaram lekapothe theeseyandi, leka `_x` ani peru pettandi
 --> src/modhalu.ante:5:5
```

Warning vaddhu anukunte, peru `_` tho start cheyyandi (`_x ante 5`).

Lints errors lenappudu matrame run avthayi, kaabatti first errors fix cheyyandi, taruvatha warnings.

## `antec fmt` ✨

Code ni official style lo pedathundhi: 4 spaces indentation, operators chuttu spaces, commas tharvatha okka space, D4 kosam avasaramaina brackets (spec §6), `(a + b) icchey` laanti chadavadaniki easy brackets, okka blank line kante ekkuva undavu, top-level items madhyalo okka blank line. 100 characters kante pedda function signatures lo prathi parameter oka line lo. Literals (`0xFF`, `1_000`, escapes) ela raasaro ala ne untayi. Comments ekkada unnayo akkade untayi.

```bash
cargo run -p antec -- fmt examples
```

CI lo leka commit mundhu check cheyyalante (emi maarchadhu, format avvakapothe exit 1):

```bash
cargo run -p antec -- fmt --check examples
```

**Safe ga untundhi:** format chesina code malli parse chesi, syntax tree (and evaluation order) same ga undho, rendosaari format chesthe emi maaradho check chesthundhi. Edhi fail aina file ni touch cheyyadhu, "antec bug" ani cheptundhi. Errors unna file ni format cheyyadhu.

## Roadmap 🗺️

| Tool | Em chesthundhi | Eppudu | Depends on |
|---|---|---|---|
| **Type-aware lints** | Vaadani results, naming style, `xerox()` avasaram lenichota | Next | Type checker ✅ |
| **Language server (`antec lsp`)** | Type chesthunnappude errors, go to definition, find references, rename, hover lo types, completion | Next | Resolver ✅, type checker ✅ (prathi expression type record chesthundhi) |
| **tree-sitter grammar** | Neovim, Helix, Zed, Emacs lo fast incremental highlighting; GitHub code navigation | Fmt tharvatha | Parser grammar (spec §5) |
| **GitHub highlighting** | github.com lo `.ante` files ki colors | Language popular ayyaaka | [Linguist](https://github.com/github-linguist/linguist) ki PR; daaniki kanisam 200 public repos `.ante` vaadali |
| **VS Code Marketplace** | `ext install` tho direct install | Publisher account set ayyaaka | Extension ✅ |
| **Playground** | Browser lo "Ante" try cheyyadam (antec ni WebAssembly ki compile chesi) | Codegen tharvatha | M7 |

Kottha tool idea unte, [feature request](https://github.com/darkwebber/Ante/issues/new?template=feature_request.yml) open cheyyandi! 🙌
