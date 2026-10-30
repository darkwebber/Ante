# Changelog

Ee file ni [release-please](https://github.com/googleapis/release-please) automatic ga update chesthundhi. Format [Keep a Changelog](https://keepachangelog.com) laane untundhi, versions [docs/VERSIONING.md](docs/VERSIONING.md) follow avthayi.

## [0.1.1](https://github.com/darkwebber/Ante/compare/v0.1.0...v0.1.1) (2026-10-30)


### ✨ Kottha features

* `antec fmt`, and hardening before M8 ([#13](https://github.com/darkwebber/Ante/issues/13)) ([b66ad08](https://github.com/darkwebber/Ante/commit/b66ad08bd60395b557e52342e3b0bf12526cbcc9))
* **antec:** beginner readiness pass; spec r16 ([#22](https://github.com/darkwebber/Ante/issues/22)) ([d502b74](https://github.com/darkwebber/Ante/commit/d502b740a076c000848c0aad2f44644c5e691e31))
* **antec:** language server `antec lsp` and VS Code client; spec r18 ([#24](https://github.com/darkwebber/Ante/issues/24)) ([313044d](https://github.com/darkwebber/Ante/commit/313044d2042bbbb598bfe49679970b5dcf4d03ba))
* **antec:** M10 standard library and M11 C FFI in native code; spec r12-r13 ([#18](https://github.com/darkwebber/Ante/issues/18)) ([9b8478f](https://github.com/darkwebber/Ante/commit/9b8478f7f1e589efe45f6717e282fc546411697d))
* **antec:** M12 tasks and sharing; spec r14 ([#19](https://github.com/darkwebber/Ante/issues/19)) ([cfbbbb4](https://github.com/darkwebber/Ante/commit/cfbbbb4d27339b949c54f85412ab0b6725242dbe))
* **antec:** M12 tasks and sharing; spec r14 ([#20](https://github.com/darkwebber/Ante/issues/20)) ([7c7c634](https://github.com/darkwebber/Ante/commit/7c7c634e7cb9cc8570fbd854e57d855c96863683))
* **antec:** M13 data and security; spec r17 ([#23](https://github.com/darkwebber/Ante/issues/23)) ([f731003](https://github.com/darkwebber/Ante/commit/f731003ccd38942b36aec9df73bb4a1414adf39a))
* **antec:** M8 borrow checker, drop elaboration; spec r6-r8 ([#14](https://github.com/darkwebber/Ante/issues/14)) ([830959c](https://github.com/darkwebber/Ante/commit/830959c6ad5950f3a36265bbdff82595876464ff))
* **antec:** M9 generics, traits and closures; spec r9-r10 Python bridge ([#15](https://github.com/darkwebber/Ante/issues/15)) ([6b37942](https://github.com/darkwebber/Ante/commit/6b37942471e29350e78068597eb0b91b2f5cff28))
* MIR and interpreter (M6), `antec run` and `antec emit mir` ([#11](https://github.com/darkwebber/Ante/issues/11)) ([529efdb](https://github.com/darkwebber/Ante/commit/529efdb78b2a2ddcdcf3da7ba3176abefd4acc5b))
* name resolution (M3), lint warnings and VS Code extension ([#7](https://github.com/darkwebber/Ante/issues/7)) ([2e8466e](https://github.com/darkwebber/Ante/commit/2e8466e841b2fe763abeea538a9c7827005d11ab))
* native code generation (M7), `antec build` and `antec emit llvm` ([#12](https://github.com/darkwebber/Ante/issues/12)) ([cb79f49](https://github.com/darkwebber/Ante/commit/cb79f49865ae2e45d6620e95589d15287c8f4813))
* **parser:** parse "Ante" source into an AST (M2) ([#5](https://github.com/darkwebber/Ante/issues/5)) ([5420f2e](https://github.com/darkwebber/Ante/commit/5420f2ed2ac3ca71b7ef48b27359a79df4375efb))
* type checker (M4) with call binding, patterns and traits ([#10](https://github.com/darkwebber/Ante/issues/10)) ([3edf11c](https://github.com/darkwebber/Ante/commit/3edf11c48b3951659be6de4063b25eaec8273352))


### 🐛 Bugs ki tata bye bye

* **antec:** audit after M9: leaks, drop timing, quadratic compile times, diagnostics ([#16](https://github.com/darkwebber/Ante/issues/16)) ([0702e1e](https://github.com/darkwebber/Ante/commit/0702e1e2c072a1af1a8a471cbb2ec8d9a9f8cdfb))
* **antec:** Windows unwinding, build-time blowups, deep data, deadlocks; spec r15 ([#21](https://github.com/darkwebber/Ante/issues/21)) ([3389c33](https://github.com/darkwebber/Ante/commit/3389c3394ebb618bd148803212384cee44223748))

## 0.1.0

First public release! 🎉 "Ante" puttindhi.

### ✨ Kottha features

* **spec:** "Ante" language spec, Edition 1.0 (draft r1): grammar, type system, ownership, standard library, C FFI, PyTorch and Python annexes, self-hosting readiness.
* **lexer:** `antec` milestone M1. Tokens, indentation (INDENT/DEDENT), four line-joining rules, and lexical errors E0101–E0110 with exact line:col.
* **cli:** `antec check <file>` and `antec emit tokens <file>`.
* **examples:** "Ante" Sundharaniki 🎬, the first welcome program (`examples/ante_sundharaniki.ante`).

### 📚 Docs

* Tinglish README, contributing guide, code of conduct, security policy, versioning and release process.
