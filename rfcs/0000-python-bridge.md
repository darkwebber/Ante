# RFC: Python bridge (numpy, pandas, scikit-learn, Hugging Face, OpenAI)

- **Start date:** 2026-10-26
- **Author(s):** @darkwebber
- **RFC PR:** (number ikkada)
- **Tracking issue:** (accept ayyaaka)
- **Edition:** 1.0 (draft)
- **Breaking?** kaadhu (`edhaina` mundhe reserved word, ippudu type keyword ayyindhi)

## Summary

"Ante" nunchi Python libraries anni direct ga vaadadaniki spec Annex P ni detail ga raasam: numpy, pandas, scipy, scikit-learn, matplotlib, OpenCV, OpenAI, Hugging Face, Python vaipu PyTorch. Normative text anthaa spec r9 lo (Annex P.1–P.22, §23.7) undhi. Ee RFC lo "enduku ila decide chesam" undhi.

## Motivation

"Ante" goal: Telugu lo programming nerchukune beginners ki best language avvadam. Computer fundamentals, DSA, full-stack web apps, ML/DL, GenAI apps, anni okate language lo. ML lo, GenAI lo nijamaina pani anthaa Python libraries lone jaruguthundhi. Manam vaatini malli raayalem, raayakoodadhu kuda. Kaabatti tutorials lo unna library calls ni "Ante" lo ade peru tho, ade arguments tho pilavagalagali. Theda okkate: call ela raasthaamo.

## Ela untundhi

```ante
python.numpy ni np ga thechuko
python.matplotlib.pyplot ni plt ga thechuko

modhalu() -> sare_thappu[khali, python.PythonThappu] ante:
    x ante 0.0 ni, 6.28 ni, 100 ni np.linspace()?
    y ante x ni np.sin()?
    x ni, y ni plt.plot()?
    "sine.png" ni plt.savefig()?
    sare(()) icchey
```

`x`, `y` rendu `edhaina` ("edhaina parledhu" laaga, ante "anything"): Python objects ki handles. `?` Python exception ni `thappu` ga paiki pampisthundhi.

## Key decisions

| Decision | Choice | Enduku |
|---|---|---|
| Python values | Okate opaque type `edhaina`; arrays numpy arrays gaane untayi | Wrapper layer pettithe library tho paatu update avvaali, eppudu venakabadi untundhi. Ila aithe library docs lo unnadhi anthaa pani chesthundhi. |
| Static checking | Module-level names, arguments, labels stubs nunchi (`.pyi`, `py.typed`, `*-stubs`, typeshed) | Typo (`np.zeross`) compile time lone dorikesthundhi. Objects meedha methods runtime lo, endukante Python duck typing; stubs thappu aithe data thappu avvakoodadhu (`TypeError` vasthundhi). |
| Errors | Calls `sare_thappu[_, PythonThappu]`; operators, indexing, attributes trap T13 | Calls lo expected failures (network, files) untayi, so values. Operators lo failure ante sadharanam ga programmer mistake (shape mismatch), Annex T laage trap. Prathi trap ki `python.choosi` lo checked version undhi. |
| Comparisons | `a > b` on `edhaina` returns `edhaina` | numpy/pandas masks (`df[df["vayasu"] > 30]`) ML lo basic. `aithe` lo vaadaalante `.nijamaa()` raayali, numpy "ambiguous truth value" godava undadhu. |
| Inclusive ranges | Python index lo `varaku` E1508 | Python negative indices tho `-1 varaku` artham maaripothundhi. |
| Arrays | Default copy; `python.array_ga` tho `varusa` ni numpy ki move (zero-copy); borrowed views `naa_risk` lone | Python code ni borrow checker choodaledhu, kaabatti shared memory safe ga undaali ante ownership move matrame. |
| Callbacks | Owning callbacks `python.function_ga`; borrow chese callbacks `python.ikkade` lopala matrame | scipy optimizer laantivi call madhyalone pilusthayi; `ikkade` ayipoyaaka callback pilisthe Python lo `RuntimeError`, dangling reference impossible. |
| Async | `async` keyword ledhu; `e.aagu()` bridge lo unna okka asyncio loop meedha wait chesthundhi, task matrame aaguthundhi | `AsyncOpenAI` laanti clients tasks tho real concurrency isthayi, function colors levu. |
| GIL | Prathi bridge operation attach/detach; tasks Python vaadithe okate OS thread meedha pin | Python thread-locals (`torch.no_grad`) correct ga untayi; "Ante" code eppudu GIL pattukoni undadhu. Free-threaded CPython (3.15t, `abi3t`) kuda support. |
| ABI | CPython Stable ABI (3.11+) | Okka build anni Python versions tho pani chesthundhi. |
| Subinterpreters | Vaadam | numpy support cheyyadhu. |
| Child processes | `multiprocessing` `spawn`, `sys.executable` = env lo Python | PyTorch `DataLoader(num_workers=…)`, `datasets.map(num_proc=…)` embedded program lo kuda pani cheyyali. |
| Learners | Common Python errors ki Tinglish hints (P.12.4); `antec repl` + Jupyter kernel (§23.7) | ML nerchukune vaallu notebooks lo try chesthaaru; error vasthe em cheyyalo cheppali. |
| Security | Python source string run cheyyalem; `Rahasyam` automatic ga Python ki velladhu; `ante.lock` lo hashes | Supply chain, secrets leak avvakunda. |

## Learning tracks

Appendix E lo kotha stages: L0 (computer fundamentals), L6 (numpy/pandas/plots/OpenCV notebooks lo), L7 (scikit-learn, prathi model "Ante" lo scratch nunchi kuda), L8 (deep learning: "Ante" lo chinna autograd, taruvatha Annex T, Hugging Face), L9 (GenAI apps: APIs, streaming, retrieval, Annex W tho full-stack chat app).

## Drawbacks

- Prathi Python call ki `?` raayali. Explicit, kaani konchem ekkuva typing. Beginners ki error ekkada vachindho kanipisthundhi, adhi plus.
- Objects meedha static checking ledhu (instance types `edhaina`). Typed instances Python 1.0 lo levu (P.22).
- Build time ki Python environment kavali (stubs kosam).

## Alternatives

- **Annex T laaga prathi library ki native binding:** numpy, pandas, HF lo vela functions; maintain cheyyalem.
- **Python ni separate process ga, RPC tho:** objects (models, tensors) ni process boundary daatinchadam chaala slow, zero-copy kudaradhu.
- **Python code ni "Ante" lo string ga raayadam (`python.exec`):** security risk, editor support undadhu; `python/` folder lo `.py` files better.
- **Calls kuda trap avvadam (sare_thappu lekunda):** OpenAI rate limits, network errors normal; vaatini handle cheyyalekapothe real apps raayalem.

## Prior art

PyO3 (`attach`/`detach`, Stable ABI), PythonCall.jl, Mojo Python interop, typeshed and PEP 561, PEP 3118 buffers, DLPack, PEP 703/779/803 (free-threading), pandas 3.0 (copy-on-write), Jupyter display protocol.

## Unresolved questions

1. Typed instance types (`numpy.ndarray` ni nominal type ga) Python 2.0 lo kavala?
2. Python classes ni "Ante" lo subclass cheyyadam (HF `TrainerCallback`) ki syntax ela untundhi?
3. Arrow zero-copy tho oka "Ante" data-frame type kavala?
