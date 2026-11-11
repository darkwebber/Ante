# learn/: "Ante" lo DSA nerchukundham 🧠

Ikkada prathi lesson oka chinna `.ante` file: paina Tinglish lo explanation, madhyalo algorithm, chivarlo `@pariksha` tests. Chadhivi, run chesi, maarchi, tests malli run cheyyandi. Algorithm ni break chesthe test ventane cheptundhi! 😄

Spec lo [learning track](../spec/ante_language_spec_1_0.md#appendix-e--implementation-roadmap-for-antec) plan undhi: L0 (computer fundamentals) nunchi L9 (GenAI apps) varaku. Ippudu L1 start ayyindhi. Roadmap lo lessons ki pedda place undhi (phase P3: L0–L3, sumaru 60 lessons): lesson raasthunnappudu language lo edhaina ibbandi anipisthe, adhi bug laage count avthundhi, issue open cheyyandi.

## Ela vaadali

Oka lesson run cheyyandi (output choodadaniki):

```bash
antec run learn/L1/01_gcd.ante
```

Aa lesson tests:

```bash
antec pariksha learn/L1/01_gcd.ante
```

Anni lessons tests okesaari:

```bash
antec pariksha learn
```

Peru lo `heap` unna tests matrame:

```bash
antec pariksha learn heap
```

(`antec` lekapothe [install](../README.md#install) cheyyandi. Repo nunchi direct ga ayithe `antec` badhulu `cargo run -p antec --` raayandi.)

## L1: numbers, searching, sorting, basic data structures

| Lesson | Em nerchukuntam |
|---|---|
| [01 GCD](L1/01_gcd.ante) | Euclid algorithm, LCM, O(log n) ela vasthundhi |
| [02 Power](L1/02_power.ante) | Binary exponentiation, modulo, overflow trap |
| [03 Sieve](L1/03_sieve.ante) | Sieve of Eratosthenes, primes anni okesaari |
| [04 Modular inverse](L1/04_mod_inverse.ante) | Extended Euclid, recursion, `undochu` |
| [05 Binary search](L1/05_binary_search.ante) | Sagam sagam vethakadam, `modhati_kanisam` |
| [06 Sorting](L1/06_sorting.ante) | Insertion sort (generic, `marpidi`), merge sort |
| [07 Prefix sums](L1/07_prefix_sums.ante) | Range sums O(1) lo, `jodi` tho subarray counting |
| [08 Two pointers](L1/08_two_pointers.ante) | Pair sum, duplicates theeseyadam |
| [09 Stack and queue](L1/09_stack_queue.ante) | Brackets check, `line` tho Josephus game |
| [10 Heap](L1/10_heap.ante) | `kuppa`, k chinnavi, running median |

## Data: JSON, secrets

| Lesson | Em nerchukuntam |
|---|---|
| [01 JSON](data/01_json.ante) | `@automatic Raayochu, Chadhavochu`, `json.raayi`/`json.chadhuvu`, thappu `daari`, extra keys enduku thappu |
| [02 Passwords](data/02_passwords.ante) | Argon2id `password_hash`/`password_check`, salt, `Rahasyam` (`***`) |

## Web: server, database

| Lesson | Em nerchukuntam |
|---|---|
| [01 HTTP server](web/01_http_server.ante) | `http.Router`, `{peru}` paths, JSON response, 404/405, socket lekunda handler tests |
| [02 SQLite](web/02_sqlite.ante) | `sql.open`, `sql"…"` (injection raadhu), `okati`/`anni`, transaction rollback |

## Kotha lesson raayalante

- Okka file, okka idea. Paina explanation colloquial Tinglish lo: friend ki explain chesinattu.
- "Enduku" mundhu cheppandi (brute force enduku slow), tharvatha trick.
- Kanisam 3 `@pariksha` tests: normal case, edge cases (khaali, okkate element), brute force tho polika.
- `antec fmt learn` and `antec pariksha learn` pass avvali. CI kuda ivi check chesthundhi.
