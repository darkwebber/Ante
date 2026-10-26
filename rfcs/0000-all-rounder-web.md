# RFC: "Ante" all-rounder: Services and Web

- **Start date:** 2026-10-26
- **Author(s):** @darkwebber
- **RFC PR:** (number ikkada)
- **Tracking issue:** (accept ayyaaka)
- **Edition:** 1.0 (draft)
- **Breaking?** kaadhu (`panchukunna` reserved word ippudu type ayyindhi; adhi mundhe keyword, kaabatti evari code break avvadhu)

## Summary

"Ante" ni systems language tho paatu all-rounder cheddham: servers, inka **web apps "Ante" lone**, HTML/CSS/JS okka line kuda raayakunda. Spec r6 lo rendu kotha conformance levels vasthayi:
- **Services 1.0** (spec Part IX): tasks, sharing, JSON, time, crypto, HTTP, SQL, logs, config.
- **Web 1.0** (spec Annex W): pages, styles, routes, server functions, sessions, security, accessibility, deploy.

Normative text anthaa spec lone undhi. Ee RFC lo "enduku ila decide chesam" undhi.

## Motivation

Telugu speakers ki "Ante" lo nerchukuni, adhe language tho real product (website, app) build chesi deploy cheyyagalagali. Ippudu web app ki minimum moodu languages (HTML, CSS, JS) plus oka server language kavali. Beginners ki adhi chaala peddha goda. Inka web lo chaala bugs (XSS, CSRF, SQL injection, access control marchipovadam, accessibility) "default ga unsafe" APIs valla vasthayi. Manam mundhe design chesthe, safe way ne easy way avthundhi.

Inko reason: official "Ante" website ni "Ante" lone raayali (dogfooding). Adhi language ki best test.

## Ela untundhi (guide-level explanation)

Page ante moodu functions: `modhalu` (start state), `maarpu` (message vasthe state maarchu), `roopam` (state nunchi UI). Elm architecture laaga.

```ante
lekka_maarpu(s: &maare Lekka ni, m: Sandesham tho) -> web.Aagna[Sandesham] ante:
    m batti:
        Sandesham.Penchu aithe:
            s.viluva += 1
        Sandesham.Thagginchu aithe:
            s.viluva -= 1
    web.emi_ledhu() icchey

lekka_roopam(s: &Lekka ni) -> web.Node[Sandesham] ante:
    [
        f"Lekka: {s.viluva}" ni web.text(),
        "Penchu" ni, Sandesham.Penchu tho web.button(),
    ] ni web.row() |> web.theme.space.m tho web.gap() icchey
```

Database nunchi data kavali ante, `@server` function raasi page nunchi `adugu` ("ask") command tho pilustham. Full example spec Annex W.2 lo undhi.

## Exact ga em change avthundhi (reference-level explanation)

Spec r6 (Appendix G lo list undhi):
- §1.2: Services, Web levels.
- §4.4.2: `panchukunna` Types group loki.
- §4.5: `sql"…"` literal (f-string laage lex avthundhi; interpolations bound parameters).
- §5.1: fields, variants mundhu annotations; annotation arguments lo `module.Name` paths.
- §20.3: T11 (poisoned lock), T12 (lock re-entry); first task kaakapothe trap aa task ni matrame aaputhundhi (§24.5).
- Part IX (§24–29), Annex W (W.1–W.27).
- Appendix B: E1202 (spec lo undhi, compiler lo inka ledhu), E13xx, E14xx, W14xx.
- Appendix C: KK, DT, NW, DB, WB seed cases.
- Appendix E: milestones M12–M23, learning track L1–L5.

`antec` ippude kotha syntax ni parse chesthundhi, kaani semantics implement ayye varaku E1202 isthundhi.

## Key decisions

| Decision | Choice | Enduku |
|---|---|---|
| Concurrency model | Structured tasks (`task.kalisi` + `pampu`), M:N scheduler, `async`/`await` ledhu | Function "colors" levu, beginners ki simple. Structured ga untundhi kaabatti tasks parent scope nunchi borrow cheyyochu (Rust `thread::scope` laaga), borrow checker data races ni aapesthundhi. |
| Task lo trap | Aa task matrame aagipothundhi (unwind, drops run avthayi), lock poison avthundhi | Server lo okka request crash ayithe motham server padipokoodadhu (OWASP A10). Main task lo trap ithe ippatilaage process exit 101. |
| UI model | Elm architecture: state, message, `maarpu`, pure `roopam` | Predictable (principle 5), browser lekunda test cheyyochu, `async` avasaram ledhu, native UI ki portable. |
| Rendering | Oke page code moodu modes lo: `Mundhe` (build time), `Browser` (SSR + hydrate), `Live` (server state, WebSocket) | Docs site ki zero JS; normal apps ki fast + offline; secrets server lo unde apps ki Live. Leptos/Dioxus (SSR + hydration), Phoenix LiveView (live) lo prove ayina patterns. |
| Server lo data load | SSR "settling": `modhalu` lo `adugu` commands server lone run chesi, data tho HTML pampadam | Separate "loader" concept avasaram ledhu; SEO ki first HTML lo data untundhi. |
| Styling | Typed `web.style` values + theme tokens → generated atomic CSS | CSS cascade godavalu levu; dark mode, reduced motion, focus ring automatic. |
| Routes | Enum + `@daari "/path/{id}"`; links route values ni theesukuntayi | Broken internal links compile error. |
| Access control | `@server` ki guard compulsory (`evarikaina` explicit ga public) | OWASP A01: marchipothe compile error. |
| CSRF | Custom header + `Sec-Fetch-Site`/`Origin` check | Page code lo tokens handle cheyyakkarledhu. |
| SQL | `sql"…"` literal matrame; string nunchi query safe code lo cheyyalem | SQL injection impossible by construction. |
| Secrets | `Rahasyam[T]`: print `***`, serialize avvadhu | Logs, JSON, page state loki leak avvadhu. |
| Accessibility | Labels, alt text signatures lo required; WCAG 2.2 AA output; contrast lint | Accessible way ne default way. |
| Browser code | Mana compiler `wasm32` ki compile chesthundhi; WasmGC/JSPI avasaram ledhu | Anni browsers lo ippude pani chesthundhi (JSPI Safari/Firefox lo inka settle avvaledhu). |
| Deploy | `static`, `server` (single binary), `container` (OCI), `wasi` (WASI 0.3 component) | Vendor lock-in ledhu; `antec` lo hosting company code undadhu. |

## Vocabulary (words unte)

| New word | Meaning | Rule |
|---|---|---|
| `kalisi` | together (task team scope) | R1 |
| `pampu`, `Pampey`, `Andhuko` | send, sender, receive | R1 |
| `Token`, `aagu` | task handle, wait: temple/hospital lo token theesukuni, taruvatha vachi collect chesinattu | R2, R5 |
| `Kaluva` | canal (channel) | R1, pun: values ala flow avthayi |
| `Thaalam`, `Key`, `theruvu` | lock, key, open ("thaalam vesaaru") | R1, R2 |
| `nidrapo`, `edhi_mundhu` | go to sleep ("Nidrapo mawa"), whichever first | R1 |
| `Raayochu`, `Chadhavochu`, `Raasevaadu`, `Chadhivevaadu` | can be written / read; the one who writes / reads | R1 + `-ochu`, `-vaadu` |
| `Kotthadhi`, `@lekapothe_kotthadhi` | default value ("a new one"), "if missing, a new one" | R1 |
| `Gadiyaram`, `Date`, `Time`, `Zone` | clock reading; the rest people say in English | R1, R2 |
| `bhadram`, `Rahasyam` | "bhadram ga undu" (crypto module), secret | R1 |
| `Aagna`, `Vinu` | command (with the playful "tamari aagna" royal vibe), listen (subscription) | R1, R5 |
| `modhalu`, `maarpu`, `roopam` | start, change, look (page functions) | R1 |
| `Kaburu` (examples) | page messages: "em kaburu?" | R1 |
| `adugu`, `kaapala`, `evarikaina` | ask (call server fn), guard ("kaapala kaayi"), anyone | R1 |
| `daari`, `Raaka`, `Gurthu`, `Kolatha`, `chitram` | route, arrival ("mee raaka"), mark (id), measure, picture | R1 |
| `Color`, `Shape`, `percent`, `Medium`, `save_cheyyi`, `design_kosam` | words people type in English anyway | R2, R3 |
| `Mundhe` | beforehand (pre-rendered) | R1 |
| `cancel_cheyyi`, `copy_cheyyi`, `scroll_cheyyi` | English stem + Telugu suffix | R3 |
| `task`, `json`, `http`, `sql`, `web`, `log`, `config` | English technical nouns | R2 |

Words spec r7 lo maarchinavi (Reddit lo Telugu comments lo evvaru vaadani newspaper words): `Pravesham` → `Raaka`, `raksha` → `bhadram`, `Rasheedu` → `Token`, `Chevi` (ante "ear" kooda) → `Key`, `Velu` (adhi "finger"!) → `Time`, `Tedhi`/`Pradesham` → `Date`/`Zone`, `Aakaaram` → `Shape`, `shaatham` → `percent`, `Madhyama` → `Medium`, `Rangu` → `Color`, `nilva` → `save_cheyyi`, `alankaram` → `design_kosam`, `Anumathi_Ledhu` → `PermissionLedhu`, `kaavali` ("want" ani artham) → `kaapala` (guard), `sthanam` → `ekkada`.

Kotha keywords levu. Prathi word ni keyword table tho check chesam (`rakam`, `lo`, `pettu` laantivi field/parameter perlu ga vaadaledhu).

## Breaking changes and migration

Emi break avvadhu. `panchukunna` mundhe reserved keyword, ippudu type keyword. `sql"…"` mundhu syntax error (identifier tharvatha string), ippudu literal.

## Drawbacks

- Spec chaala peddhadhi avthundhi; implement cheyyadaniki chaala time (M12–M18).
- Core 1.0 lo ledhu kaabatti, Services/Web lekunda Core implementations kuda conforming. Web kavalante Web level implementation kavali.
- Elm architecture ki boilerplate ekkuva (message enums). Trade-off: predictable, testable.
- Live mode ki server memory per tab kavali.

## Alternatives

- **async/await:** popular, kaani function colors, `Pin` laantivi beginners ki kashtam. Green tasks simple.
- **React-style components with hooks:** hidden state, rules of hooks, re-render bugs. Elm style lo state anthaa okka chota.
- **Fine-grained signals (Leptos, Solid):** fast, kaani ownership tho kalipithe complex; mana borrow checker tho closures lo shared mutable state kashtam.
- **Only server-driven (LiveView only):** offline ledhu, prathi click ki network round trip.
- **HTML templates:** user HTML nerchukovali, XSS risk; mana goal ki opposite.
- **JS ki compile cheyyadam:** wasm tho mana semantics (checked arithmetic, traps) exact ga untayi.

## Prior art

Elm (architecture), Phoenix LiveView (live mode), Leptos/Dioxus (SSR + hydration, server functions), SwiftUI/Jetpack Compose (modifiers), Rust `thread::scope` (structured tasks), Go (green threads, `net/http`), serde (data tree), Tailwind (atomic CSS from tokens), Testing Library (role-based queries), OWASP Top 10:2025, WCAG 2.2.

## Learning track

cp-algorithms (CC BY-SA 4.0, ~160 articles) lo unna algorithms anni "Ante" lo, Tinglish explanations tho `learn/` lo. Explanations cp-algorithms nunchi adapt chesthe CC BY-SA 4.0 + attribution, separate license file tho; compiler code MIT/Apache-2.0 ne. Implementations independent ga raasthaam. Stages L1–L5 Appendix E lo.

## Unresolved questions

1. ~~`pani` values `CopyKottochu`, kaabatti closure environments ni free cheyyalem.~~ Spec r10 lo decide chesam: `pani` copies environment ni panchukuntayi (reference count), last copy poyinappudu captures drop avthayi (§10.7.4).
2. Live mode reconnect: state session lo automatic ga save cheyyala, leka ippatilaage restart?
3. Annex N (native desktop/mobile) ki e back ends: platform toolkits leka mana own renderer?
4. Package registry, signing (supply chain) ki separate RFC kavali.
