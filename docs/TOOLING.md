# Developer tools

Manchi language ki manchi tools kavali. Code raase time lo editor sahayam cheyyali, mistakes ni compiler twaraga, friendly ga cheppali. Ee doc lo ippudu emi ready ga undho, taruvatha emi vasthundho undhi. 🛠️

## Ippudu ready ✅

### Syntax highlighting (VS Code)

[`editors/vscode/`](../editors/vscode) lo VS Code extension undhi:

- **Highlighting:** keywords, case markers (`ni`, `tho`, `ki`…), verbs (`kalupu()`), types, f-strings, escapes, numbers, annotations. Case markers ki separate color, kaabatti phrase lo "evaru em chesthunnaro" oka chupulo kanipisthundhi.
- **Markdown lo kuda:** ```` ```ante ```` blocks highlight avthayi (README, spec).
- **Smart indentation:** `:` tharvatha auto indent, `icchey` / `aapey` / `vadhiley` tharvatha outdent, `.ante` files ki spaces (tabs E0102 kada 😉).
- **Snippets:** `sundharaniki` 🎬, `modhalu`, `pani`, `aithe`, `okko`, `batti`, `rakam`, `panulu`, `gunam`, `pariksha` 🧪…

Install cheyyadam:

- **Release nunchi:** [GitHub Releases](https://github.com/darkwebber/Ante/releases) lo `ante-language-<version>.vsix` download chesi, VS Code lo **Extensions → … → Install from VSIX**.
- **Source nunchi:** [editors/vscode/README.md](../editors/vscode/README.md) choodandi.

Grammar TextMate format lo undhi ([`ante.tmLanguage.json`](../editors/vscode/syntaxes/ante.tmLanguage.json)), kaabatti **Sublime Text, JetBrains IDEs (TextMate bundles), Zed, Nova** laanti editors lo kuda adhe file vaadochu.

> **Grammar, compiler sync lo untayi:** `npm test` compiler keyword table (`antec/src/token.rs`) chadivi, prathi keyword highlight avthondho check chesthundhi. Kottha keyword add chesi grammar update marchipothe, CI fail avthundhi.

### Language server: `antec lsp`

`antec lsp` editor tho [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) lo maatladuthundhi (stdin/stdout). VS Code extension dheenni thane start chesthundhi; LSP vachina e editor aina vaadochu.

**Em pani chesthundhi:**

- **Type chesthunnappude errors:** type cheyyadam aapina ~0.2 second lo file ni malli check chesthundhi (open, save appudu ventane). File `ante.toml` unna package `src/` lo unte, package motham check avthundhi, save cheyyani files tho sahaa. Oka file lo maarpu vere file lo error teesthe, adhi kuda kanipisthundhi. Warnings (W0001…) kuda vasthayi.
- **Hover:** variable meedha mouse pedithe dhaani type (`maare h: kuppa[ankam64]`), function meedha pedithe signature, inka paina raasina `#` comments. Fields, variants, records, constants anniki. Vere chota pedithe aa expression type.
- **Go to definition** (`F12`): function, local, field (`naa.x` kuda), record, enum variant, imported name: package lo vere file lo unna kuda.
- **Find references** (`Shift+F12`) inka **rename** (`F2`): locals, functions, methods, records, fields, variants, constants; package lo anni files lo okesaari. Keyword peru ivvalem, standard library peru maarchalem.
- **Completion:** `p.` tharvatha fields, methods; `naa.` tharvatha mee type vi; `lekkalu.` / `Rangu.` tharvatha module items, variants; lekapothe scope lo unna locals, file items, prelude (`choopinchu`, `kuppa`, `line`, `sardina_jodi`…), keywords. Inka type chesthu file half ga unna (parse avvakapoyina) pani chesthundhi.
- **Format document** (`Shift+Alt+F`): `antec fmt` laane. File lo errors unte emi maarchadhu.
- Telugu text, emoji unna lines lo kuda positions correct (LSP UTF-16 lekka, compiler bytes lekka; madhyalo convert chesthundhi).

**Setup (VS Code):**

1. `antec` install cheyyandi (repo root nunchi):

   ```bash
   cargo install --path antec
   ```

2. Extension install cheyyandi (paina choodandi). Antha ayipoyindhi! `.ante` file open chesthe server start avthundhi.
3. `antec` `PATH` lo leka `~/.cargo/bin` lo lekapothe, Settings lo **`ante.antecPath`** ki full path ivvandi (leka workspace nunchi relative, ex: `target/release/antec`).

`antec` lekapothe extension highlighting, snippets tho pani chesthune untundhi, okka saari chinna message isthundhi. `ante.lsp.enabled: false` tho server off cheyyochu. Problem unte **"Ante": Language server restart cheyyi** command, inka `ante.trace.server: "verbose"` tho Output panel lo messages choodochu.

**Vere editors:** command `antec lsp`, language id `ante`, files `*.ante`. Neovim (0.11+) lo:

```lua
vim.filetype.add({ extension = { ante = "ante" } })
vim.lsp.config("antec", { cmd = { "antec", "lsp" }, filetypes = { "ante" }, root_markers = { "ante.toml" } })
vim.lsp.enable("antec")
```

Helix `languages.toml` lo:

```toml
[language-server.antec]
command = "antec"
args = ["lsp"]

[[language]]
name = "ante"
scope = "source.ante"
file-types = ["ante"]
roots = ["ante.toml"]
language-servers = ["antec"]
```

**Inka raanivi (ippatiki):**

- Standard library ki go to definition (prelude signatures `antec` lopale untayi, file ga kaadhu); hover lo maatram vaati signature vasthundhi.
- Trait methods, modules rename; file lo parse error unnappudu hover, definition, rename (aa time lo completion matrame, chivari manchi check nunchi).
- Signature help, quick fixes (code actions), semantic highlighting, workspace symbol search, inlay type hints.

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

## `antec pariksha` 🧪

`@pariksha` mundhu pettina functions anni tests. Prathi test vere process lo run avthundhi, kaabatti okati crash ayina migathavi aagavu. `nijame()` assertion; `thappu` return chesina, leka trap ayina test fail.

```ante
@pariksha
gcd_sariga() -> khali ante:
    ((12, 18 ni gcd()) == 6) nijame()
```

```bash
cargo run -p antec -- pariksha learn
```

```text
📄 learn/L1/01_gcd.ante
  ✅ gcd_chinna_lekkalu
  ❌ lcm_lekkalu
     │ kompa munigindhi! nijame() fail ayyindhi (learn/L1/01_gcd.ante:59:5)

😬 1 pass, 1 fail. Fail ayina vaati output paina undhi
```

- File isthe aa file tests, package isthe package tests, `ante.toml` leni folder isthe prathi `.ante` file vidi vidi ga.
- Chivarlo peru isthe aa peru unna tests matrame: `antec pariksha learn heap`.
- Fail ayina test output (`choopinchu` lu, trap message) `│` tho kindha vasthundhi; pass ayinavi quiet ga untayi.
- Tests clang tho native ga build avthayi, CPU cores anni vaadi okesaari run avthayi; clang lekapothe interpreter lo.
- Test ki parameters unna, leka return type `khali`/`sare_thappu[khali, E]` kaakapothe E1307.

## `antec run` 🏃

`antec run` program ni check chesi, clang dorikithe native ga build chesi run chesthundhi: interpreter kante 10–50x fast. Clang lekapothe (leka `--interp` isthe) MIR interpreter lo run avthundhi. Output, traps rendu lo okate.

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
| **`antec new`** | Kotha package (`ante.toml`, `src/modhalu.ante`, oka test) okka command tho | Next | `antec pariksha` ✅ |
| **LSP extras** | Signature help, quick fixes, semantic highlighting, inlay type hints, stdlib ki go to definition | `antec lsp` tharvatha | `antec lsp` ✅ |
| **tree-sitter grammar** | Neovim, Helix, Zed, Emacs lo fast incremental highlighting; GitHub code navigation | Fmt tharvatha | Parser grammar (spec §5) |
| **GitHub highlighting** | github.com lo `.ante` files ki colors | Language popular ayyaaka | [Linguist](https://github.com/github-linguist/linguist) ki PR; daaniki kanisam 200 public repos `.ante` vaadali |
| **VS Code Marketplace** | `ext install` tho direct install | Publisher account set ayyaaka | Extension ✅ |
| **Playground** | Browser lo "Ante" try cheyyadam (antec ni WebAssembly ki compile chesi) | Codegen tharvatha | M7 |

Kottha tool idea unte, [feature request](https://github.com/darkwebber/Ante/issues/new?template=feature_request.yml) open cheyyandi! 🙌
