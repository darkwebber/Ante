# "Ante" for VS Code

"Ante" programming language ki VS Code support. Telugu lo think cheyyi, native speed lo run cheyyi. 🚀

## Em vasthundhi

- **Syntax highlighting:** keywords, case markers (`ni`, `tho`, `ki`…), verbs (`kalupu()`), types, f-strings, escapes, numbers, annotations. Case markers ki separate color untundhi, kaabatti "evaru em chesthunnaro" oka chupulo ardham avthundhi.
- **Markdown lo kuda:** README lo ```` ```ante ```` blocks highlight avthayi.
- **Smart indentation:** `:` tharvatha Enter kotte automatic ga indent; `icchey`, `aapey`, `vadhiley` tharvatha outdent. Tabs vaddhu kaabatti `.ante` files ki spaces automatic ga set avthayi.
- **Snippets:** `sundharaniki` 🎬, `modhalu`, `pani`, `aithe`, `ledhante`, `okko`, `nunchi`, `unnanthasepu`, `batti`, `rakam`, `edho_okati`, `panulu`, `gunam`, `choopinchu`, `naa_risk`.
- **Comment toggle** (`Cmd+/`), bracket matching, indentation tho code folding.

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

## License

MIT OR Apache-2.0, mee ishtam. Details [LICENSE](LICENSE) lo.
