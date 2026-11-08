# Security Policy

## Supported versions

Security fixes are applied to the latest released `yourai` CLI on the `main` branch
and published as a new semver release when needed. Older patch lines are not
generally backported while the project is in early `0.0.x`.

| Version | Supported |
| --- | --- |
| Latest GitHub Release / `main` | Yes |
| Older `0.0.x` releases | Best-effort only |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Email **[support@bdtren.com](mailto:support@bdtren.com)** with:

- A clear description of the issue and impact
- Steps to reproduce (or a proof of concept)
- Affected version / commit / platform if known
- Whether you plan to disclose publicly and on what timeline

We aim to acknowledge reports within a few business days and to keep you updated
while we investigate and ship a fix.

## Scope

In scope examples:

- Path traversal or sandbox escape from the chat agent
- Authentication bypass on non-loopback / LAN mode
- Secret leakage (API keys, prompts) via logs, files, or the public API
- Supply-chain issues in download verification or update channels
- Remote code execution via crafted project config, API input, or catalogs

Out of scope examples:

- Issues that require physical access to an unlocked machine
- Abuse of intentionally enabled agent write tools after the user confirmed them
- Vulnerabilities only in third-party model weights or upstream engines
  (llama.cpp, stable-diffusion.cpp, whisper.cpp) — report those upstream when possible
- Denial of service from running large local models on underpowered hardware

## Security documentation

| Document | Contents |
| --- | --- |
| [docs/SECURITY_ARCHITECTURE.md](docs/SECURITY_ARCHITECTURE.md) | How the stack is built and trust boundaries |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Assets, attackers, and mitigations |
| [docs/PRIVACY.md](docs/PRIVACY.md) | What leaves the machine and what stays local |
| [docs/PERMISSIONS.md](docs/PERMISSIONS.md) | Agent, filesystem, and network permissions |
| [docs/SECURITY_AUDIT.md](docs/SECURITY_AUDIT.md) | How we audit and what CI checks |

## Preferred coordinated disclosure

1. Private report to `support@bdtren.com`
2. We confirm and triage
3. Fix lands on `main` and a release is published when appropriate
4. Public disclosure after a fix is available (or by mutual agreement)

Thank you for helping keep yourai users safe.
