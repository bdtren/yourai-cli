# Threat model

This is a practical threat model for the `yourai` CLI and a local project it manages.
It is not a claim of perfect security.

## Assets

| Asset | Why it matters |
| --- | --- |
| User prompts and chat history | May contain private or proprietary text |
| Local documents in the agent workspace | May be confidential |
| Generated images / video / audio | May be sensitive or licensed content |
| API keys in `.yourai/secrets.toml` | Gate access when the server is reachable beyond loopback |
| Model weights and engines | Integrity and license compliance; large attack surface if replaced |
| Project config and prompts | Can change behavior or widen permissions |

## Actors

| Actor | Capabilities |
| --- | --- |
| Local user | Full control of the machine and project |
| Malicious local process | May read world-readable files; may try to call localhost APIs |
| LAN peer | Can reach the gateway only if bind address / firewall allows it |
| Remote attacker on the internet | Relevant only if the user exposes the port (deploy / tunnel / `0.0.0.0`) |
| Supply-chain attacker | Compromised dependency, catalog host, or release artifact |
| Malicious model / prompt | Tries to coerce the agent into reading or writing sensitive files |

## Trust assumptions

- The OS user account is not fully compromised.
- Upstream engine binaries behave as documented once checksum-verified.
- The user reviews confirmations before enabling write tools or LAN exposure.

## Key threats and mitigations

### T1 — Unauthorized API use on the local network

**Threat:** Someone on the LAN calls `/v1` and uses the user's GPU/models.

**Mitigations:**

- Default bind `127.0.0.1`
- LAN / remote modes require an API key (`Authorization: Bearer` or `x-api-key`)
- Key stored in `.yourai/secrets.toml` with restrictive permissions when the OS supports it

**Residual risk:** Loopback clients on the same OS user can usually call the API without a key
in the default local setup.

### T2 — Agent reads or writes outside the intended folder

**Threat:** Prompt injection or a buggy path joins escape the workspace.

**Mitigations:**

- Workspace rooted under the project; canonicalization checks
- Block access to `.yourai/` and `yourai_modules/`
- Reject symlink escapes
- `allow_write` defaults to false; writes require interactive confirmation

**Residual risk:** Anything the user places inside the workspace is readable by the agent
when the agent capability is enabled.

### T3 — Tampered engine or model download

**Threat:** MITM or compromised mirror serves malware instead of llama.cpp / weights.

**Mitigations:**

- Catalog pins `sha256` per file
- Build refuses mismatched digests
- Installers verify CLI archives against website and GitHub checksums

**Residual risk:** A fully compromised signing/catalog authority could still ship bad files;
users should prefer official channels.

### T4 — Secret leakage via logs or config

**Threat:** API keys or prompts appear in committed files or shared logs.

**Mitigations:**

- Secrets kept out of `yourai.toml`
- Avoid logging Authorization headers and secret file contents
- `.gitignore` excludes `.yourai/`, `yourai_modules/`, `outputs/`

**Residual risk:** Users can still paste secrets into prompts or commit files manually.

### T5 — Remote exposure misconfiguration

**Threat:** `deploy` / bind `0.0.0.0` without a key or firewall.

**Mitigations:**

- Wizard and docs require API keys for non-loopback
- Deploy notes call out firewall / security-group steps

**Residual risk:** Operator error remains the largest risk for internet exposure.

### T6 — Vulnerable Rust dependencies

**Threat:** Known RUSTSEC issues in crates used by the CLI.

**Mitigations:**

- CI `cargo audit` and `cargo deny` (see [SECURITY_AUDIT.md](SECURITY_AUDIT.md))
- Dependabot for Cargo and GitHub Actions

## Out of scope (for now)

- Protecting data from an attacker with root / admin on the same machine
- Formal verification of upstream C++ engines
- Guaranteeing model outputs are safe or non-toxic
- Multi-tenant hardening on a shared server (yourai is single-user / single-project oriented)

## Review cadence

Revisit this model when adding:

- New agent tools (shell, browser, network)
- New bind / auth modes
- Plugin or third-party handler execution
- Cloud sync or telemetry
