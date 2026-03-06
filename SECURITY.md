# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

Only the latest release receives security patches.
Once HwpForge reaches 1.0, this table will expand to cover the last two minor releases.

## Reporting a Vulnerability

**Do not open a public issue for security vulnerabilities.**

Instead, please report them privately:

1. **GitHub Security Advisory (preferred)**
   — Go to [Security Advisories](https://github.com/ai-screams/HwpForge/security/advisories/new) and create a new draft advisory.
2. **Email**
   — Send details to **<hanyul.ryu@hanyul.xyz>** with the subject line `[HwpForge Security]`.

### What to include

- A description of the vulnerability and its impact.
- Steps to reproduce or a proof of concept.
- Affected crate(s) and version(s).
- Any suggested fix, if you have one.

### What to expect

- **Acknowledgement** within 48 hours of your report.
- **Triage and severity assessment** within 7 days.
- A fix coordinated privately before public disclosure. We aim to release a patch within 30 days of confirming the issue.
- Credit in the release notes (unless you prefer to remain anonymous).

## Scope

HwpForge is a document processing library. The following are in scope:

| Area              | Examples                                                                        |
| ----------------- | ------------------------------------------------------------------------------- |
| Memory safety     | Buffer overflows, use-after-free (note: the crate is `#![forbid(unsafe_code)]`) |
| Input parsing     | ZIP bombs, XML entity expansion, malformed HWPX/HWP5 causing panics or hangs    |
| Path traversal    | Malicious ZIP entries writing outside the target directory                      |
| Denial of service | Crafted inputs causing unbounded memory or CPU consumption                      |
| Dependency issues | Known CVEs in transitive dependencies                                           |

Out of scope: issues in the Hancom 한글 application itself, or issues that require the attacker to already have arbitrary code execution on the host.

## Security Measures

HwpForge employs the following safeguards:

- **`#![forbid(unsafe_code)]`** across all crates — zero unsafe blocks.
- **ZIP bomb defense** — 50 MB per entry, 500 MB total, 10,000 entry limit.
- **`cargo-deny`** — license and advisory audits run in CI (weekly + every PR).
- **Dependabot** — automated dependency update PRs.
- **Nightly canary** — weekly beta/nightly toolchain builds catch regressions early.

## Disclosure Policy

We follow [coordinated disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure).
After a fix is released, the advisory will be published with full details and credit to the reporter.
