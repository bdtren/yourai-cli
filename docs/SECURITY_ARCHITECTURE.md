# Security architecture

This document describes how the `yourai` CLI is structured so that local AI stays
under the user's control. It complements [THREAT_MODEL.md](THREAT_MODEL.md),
[PRIVACY.md](PRIVACY.md), and [PERMISSIONS.md](PERMISSIONS.md).

## Goals

1. **Local-first** — prompts and generations stay on the machine by default.
2. **Least privilege** — engines bind to loopback; the agent is sandboxed; writes are off until enabled.
3. **Verifiable downloads** — engines and models are checked against catalog SHA-256 digests.
4. **Clear trust boundaries** — the project folder, the gateway, and upstream engines are separate layers.

## High-level layout

```text
┌─────────────────────────────────────────────────────────────┐
│  User / apps (terminal, browser, OpenAI-compatible clients) │
└────────────────────────────┬────────────────────────────────┘
                             │  public port (default 127.0.0.1)
                             ▼
┌─────────────────────────────────────────────────────────────┐
│  yourai gateway                                             │
│  - API key check in LAN / non-loopback mode                 │
│  - routes chat + media                                      │
│  - never exposes engine ports outside loopback              │
└───────┬───────────────┬───────────────┬─────────────────────┘
        │               │               │
        ▼               ▼               ▼
   llama-server    sd-server      whisper / TTS
   (127.0.0.1)     (127.0.0.1)    (127.0.0.1)
```

A **project** (`yourai.toml`, `yourai.lock`, `yourai_modules/`, `prompts/`, `web/`, `api/`)
is the unit of configuration and reproducibility. Engines are slots inside that project,
not separate product types.

## Trust boundaries

| Boundary | Inside | Outside |
| --- | --- | --- |
| Loopback gateway | Local chat UI, `yourai chat`, localhost API clients | LAN / internet peers (only if the user opens the bind address) |
| Agent sandbox | Paths under `agent.workspace` | Parent disks, `.yourai/`, `yourai_modules/`, symlink escapes |
| Catalog / updates | Pinned SHA-256 artifacts from configured base URLs | Arbitrary unsigned URLs |
| Secrets file | `.yourai/secrets.toml` (owner-only when the OS allows) | `yourai.toml` (must not store API keys) |

## Components

### CLI (`yourai`)

- Creates and manages the project.
- Downloads engines/models into `yourai_modules/` (or shared `~/.yourai` when configured).
- Starts the gateway; child engines listen only on `127.0.0.1`.
- Performs optional daily self-update checks (network); disable with `YOURAI_NO_UPDATE_CHECK=1`.

### Gateway

- Single public HTTP(S) entry for chat and media routes.
- Proxies to loopback engines.
- Enforces API keys when `server.require_api_key` is set (LAN / remote expose).
- Serves project `web/` or the default llama.cpp UI when enabled.

### Chat agent

- Optional tools for listing/reading (and optionally writing) files in a workspace.
- Path resolution rejects escapes and sensitive project dirs. See [PERMISSIONS.md](PERMISSIONS.md).

### Engines

- **llama.cpp** `llama-server` — chat / vision / omni.
- **stable-diffusion.cpp** `sd-server` — images and short video (including MiniMax-H3).
- **whisper.cpp** / TTS helpers — speech.

Engines are treated as untrusted network peers relative to the wider LAN: they must not be
bound beyond loopback by yourai.

## Data at rest

| Path | Contents | Sensitivity |
| --- | --- | --- |
| `yourai.toml` | Config (no secrets) | Low–medium |
| `.yourai/secrets.toml` | API key | High |
| `yourai.lock` | Pinned digests and paths | Low |
| `yourai_modules/` | Weights and engine binaries | Medium (large; license-bound) |
| `outputs/` | Generated media | User-dependent |
| `prompts/`, `data/docs/` | User content | User-dependent |

## Network egress

By default the CLI contacts the network only to:

1. Refresh the model/engine catalog
2. Download engines and models
3. Check for CLI updates (daily, optional)

Inference traffic does not leave the machine unless the user points a client elsewhere or
opens a remote deploy/expose path.

## Related docs

- [SECURITY.md](../SECURITY.md) — vulnerability reporting
- [THREAT_MODEL.md](THREAT_MODEL.md)
- [PRIVACY.md](PRIVACY.md)
- [PERMISSIONS.md](PERMISSIONS.md)
- [SECURITY_AUDIT.md](SECURITY_AUDIT.md)
