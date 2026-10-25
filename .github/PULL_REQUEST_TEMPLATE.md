## Em change chesaru?

<!-- Short ga explain cheyyandi. Related issue unte: "Closes #123" -->

## PR title check

<!-- PR title Conventional Commits format lo undali. Squash merge tharvatha adhe commit message.
     ex: "feat(parser): parse else-if chains", "feat(vocab)!: rename vadiley to vadhiley" -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` local ga pass ayyayi
- [ ] Tests add / update chesanu (unit tests or `antec/tests/conformance/` cases)
- [ ] Behavior change aithe, spec (`spec/ante_language_spec_1_0.md`) kuda update chesanu
- [ ] New error code unte, spec Appendix B lo add chesanu
- [ ] Avasaram unte docs (Tinglish) update chesanu

## Breaking change?

- [ ] **Kaadhu**
- [ ] **Avunu.** PR title lo `!` pettanu, commit body lo `BREAKING CHANGE:` footer tho migration raasanu, [VERSIONING.md](https://github.com/darkwebber/Ante/blob/main/docs/VERSIONING.md) rules (deprecation alias, spec revision) follow ayyanu

<!-- Vocabulary change aithe: accept ayina vocabulary proposal issue link ivvandi. -->
