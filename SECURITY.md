# Security policy

## Supported versions

"Ante" ippudu 0.x lo undhi. Security fixes **latest release** ki matrame isthaam.

| Version | Support |
|---|---|
| Latest 0.x release | ✅ |
| Old 0.x releases | ❌ (latest ki upgrade cheyyandi) |

## Vulnerability report cheyyadam

Security problems ni **public issue lo pettakandi.**

Daaniki badhulu GitHub **private vulnerability reporting** use cheyyandi:

1. Repo lo **Security** tab ki vellandi.
2. **Report a vulnerability** click cheyyandi.
3. Em jarigindhi, ela reproduce cheyyalo (chinna `.ante` program unte inka better), impact enti anedhi raayandi.

Report vachaaka:

- 7 days lo reply isthaam.
- Fix plan, timeline meetho share chesthaam.
- Fix release ayyaaka, meeku ok aithe, advisory lo meeku credit isthaam.

## Ee project lo "security bug" ante enti

Ivi security bugs ga treat chesthaam:

- `naa_risk:` lekunda undefined behavior or memory unsafety (spec §1.4 promise break avvadam).
- Malicious source file valla compiler arbitrary code run cheyyadam.
- Release binaries, CI workflows, or supply chain lo problems.

Compiler crash, wrong error message laantivi normal bugs. Vaatiki [bug report](https://github.com/darkwebber/Ante/issues/new?template=bug_report.yml) open cheyyandi.
