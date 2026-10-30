# "Ante" for VS Code

"Ante" programming language ki VS Code support. Telugu lo think cheyyi, native speed lo run cheyyi. 🚀

## Em vasthundhi

- **Syntax highlighting:** keywords, case markers (`ni`, `tho`, `ki`…), verbs (`kalupu()`), types, f-strings, escapes, numbers, annotations. Case markers ki separate color untundhi, kaabatti "evaru em chesthunnaro" oka chupulo ardham avthundhi.
- **Markdown lo kuda:** README lo ```` ```ante ```` blocks highlight avthayi.
- **Smart indentation:** `:` tharvatha Enter kotte automatic ga indent; `icchey`, `aapey`, `vadhiley` tharvatha outdent. Tabs vaddhu kaabatti `.ante` files ki spaces automatic ga set avthayi.
- **Snippets:** `sundharaniki` 🎬, `modhalu`, `pani`, `aithe`, `ledhante`, `okko`, `nunchi`, `unnanthasepu`, `batti`, `rakam`, `edho_okati`, `panulu`, `gunam`, `choopinchu`, `naa_risk`.
- **Comment toggle** (`Cmd+/`), bracket matching, indentation tho code folding.

### `antec` unte inka 🧠

Extension `antec lsp` (language server) ni start chesthundhi:

- **Type chesthunnappude errors, warnings** (red/yellow squiggles), package motham, save cheyyani files tho sahaa.
- **Hover:** variable type, function signature, paina raasina `#` comments.
- **Go to definition** (`F12`), **find references** (`Shift+F12`), **rename** (`F2`), package lo anni files lo.
- **Completion:** `p.` tharvatha fields, methods; `naa.`, `lekkalu.`, `Rangu.`; scope lo unna names, prelude, keywords.
- **Format document** (`Shift+Alt+F`), `antec fmt` tho.

`antec` install cheyyadam (repo root nunchi):

```bash
cargo install --path antec
```

`antec` `PATH` lo leka `~/.cargo/bin` lo lekapothe, Settings lo `ante.antecPath` ki path ivvandi. `antec` lekapothe extension highlighting, snippets tho pani chesthune untundhi.

| Setting | Default | Em chesthundhi |
|---|---|---|
| `ante.antecPath` | `antec` | `antec` ekkada undhi (peru, full path, leka workspace nunchi relative path) |
| `ante.lsp.enabled` | `true` | Language server on/off |
| `ante.trace.server` | `off` | `messages` / `verbose`: server tho messages Output panel lo |

Command palette lo **"Ante": Language server restart cheyyi** kuda undhi. Full details, vere editors (Neovim, Helix) setup [docs/TOOLING.md](../../docs/TOOLING.md#language-server-antec-lsp) lo.

## Install (source nunchi)

Marketplace lo inka publish cheyyaledhu. Ippatiki repo nunchi:

```bash
cd editors/vscode
```

```bash
npm install
```

```bash
npm run package
```

```bash
code --install-extension ante-language.vsix
```

Leka GitHub Releases nunchi `ante-language-<version>.vsix` download chesi VS Code lo **Extensions → … → Install from VSIX** cheyyandi.

## Develop

```bash
npm test
```

Ee test VS Code vaade TextMate engine tho "Ante" lines ni tokenize chesi prathi token ki correct scope vachindho check chesthundhi. Inka compiler (`antec/src/token.rs`) lo unna prathi keyword highlight avthondho kuda check chesthundhi. Compiler ki kottha keyword add chesi grammar update cheyyakapothe, test fail avthundhi. 😉

Client tests (`test/server.test.js`) `antec` ni ela vethukuthundho, `antec` lekapothe extension quiet ga highlighting tho start avthundho check chesthayi. `antec` build chesi unte, nijam `antec lsp` tho handshake kuda chesthayi:

```bash
ANTEC=../../target/release/antec npm test
```

## License

MIT OR Apache-2.0, mee ishtam. Details [LICENSE](LICENSE) lo.
