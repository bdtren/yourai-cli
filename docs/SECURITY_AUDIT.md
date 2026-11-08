# Security audit practice

How we look for security issues in `yourai-cli`, and what automation runs in CI.

## Continuous checks (CI)

Workflow: [`.github/workflows/security.yml`](../.github/workflows/security.yml)

| Check | Tool | Purpose |
| --- | --- | --- |
| RustSec advisories | `cargo audit` | Fail on known vulnerable crates in `Cargo.lock` |
| Policy / licenses / sources | `cargo deny` | Advisories, banned crates, allowed registries, license allowlist |
| Dependency updates | Dependabot | PRs for Cargo and GitHub Actions bumps |

The security workflow runs on:

- Push to `main`
- Pull requests
- Weekly schedule (catch newly published advisories)
- Manual `workflow_dispatch`

Ordinary CI (fmt, clippy, tests) remains in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml).

## Manual review checklist

Before a release that touches networking, the agent, downloads, or auth:

- [ ] Default bind remains loopback unless docs/wizard clearly opt in
- [ ] Non-loopback paths require API keys
- [ ] Agent path checks still block escapes, `.yourai/`, and `yourai_modules/`
- [ ] Writes still gated by `allow_write` + confirmation
- [ ] New downloads have pinned SHA-256 in the catalog
- [ ] Secrets are not logged or written into `yourai.toml`
- [ ] `cargo audit` and `cargo deny check` are clean (or documented exceptions)
- [ ] Threat model / permissions docs updated if the surface changed

## Local commands

```sh
cargo install cargo-audit cargo-deny --locked   # once
cargo audit
cargo deny check
```

Policy lives in [`deny.toml`](../deny.toml) at the repo root.

## Third-party engines

llama.cpp, stable-diffusion.cpp, and whisper.cpp are external binaries. yourai verifies
digests at download time but does not re-audit upstream C/C++ codebases on every release.
Engine upgrades should note upstream security advisories when relevant.

## Reporting

See [SECURITY.md](../SECURITY.md) for private vulnerability reporting
(`[support@bdtren.com](mailto:support@bdtren.com)`).

## Audit history

| Date | Notes |
| --- | --- |
| 2026-11-08 | Added security docs, `deny.toml`, Dependabot, and CI `cargo audit` / `cargo deny` |
