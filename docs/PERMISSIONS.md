# Permissions

This page describes what `yourai` is allowed to do on the filesystem and network, and
how you tighten or widen that surface.

## Summary

| Capability | Default | User control |
| --- | --- | --- |
| Bind address | `127.0.0.1` | `server.host` / wizard network access |
| API authentication | Off on loopback | Required when exposing beyond loopback |
| Agent file read | Off until agent enabled | `agent` capability + `agent.workspace` |
| Agent file write | Off | `agent.allow_write = true` + confirm each write |
| Shell / arbitrary command tools | Not shipped | N/A (future tools would be opt-in) |
| Model / engine download | On `build` | Checksums; license prompts |

## Filesystem

### Project layout

The project root owns config the user edits (`yourai.toml`, `prompts/`, `web/`, `api/`).
Runtime and bulky artifacts are isolated:

- `.yourai/` — state, logs, secrets (gitignored)
- `yourai_modules/` — engines and weights (gitignored)
- `outputs/` — generated media (gitignored)

### Agent sandbox

When the agent is enabled in chat:

1. The workspace defaults to the project folder (`agent.workspace = "."`).
2. Resolved paths must stay inside that workspace after canonicalization.
3. Symlinks that escape the workspace are rejected.
4. Access to `.yourai/` and `yourai_modules/` is blocked even if they sit under the workspace root.
5. Writing requires `agent.allow_write = true` and an interactive confirmation with a preview.

Configure:

```toml
[agent]
enabled = true
workspace = "."
allow_write = false
```

### Process permissions

- Secret files are written with owner-only mode (`0600`) on Unix when supported.
- Engine binaries are installed executable (`0755`) as needed.
- The CLI does not request OS admin/root for normal local use.

## Network

### Listeners

| Listener | Bind | Notes |
| --- | --- | --- |
| Gateway (public API / UI) | `server.host`:`server.port` | Default loopback |
| llama-server / sd-server / whisper / TTS | `127.0.0.1` ephemeral ports | Not published to LAN |

Only the gateway should be reachable from other devices, and only if you change the bind
address and open your firewall.

### API keys

When `server.require_api_key` is true (LAN / remote expose flows):

- Clients must send `Authorization: Bearer <key>` or `x-api-key: <key>`
- The key lives in `.yourai/secrets.toml` or `YOURAI_API_KEY`

### Egress

See [PRIVACY.md](PRIVACY.md) for catalog, download, and update checks.

## Custom API handlers

Projects may add `api/handlers/` scripts wired through `api/routes.toml`. Treat custom
handlers as **trusted local code** you maintain: they run with the privileges of the
yourai process. Do not install untrusted handler scripts.

## Hardening checklist

- Keep `server.host = "127.0.0.1"` unless you need LAN or remote access
- Prefer API keys + firewall rules when exposing a port
- Leave `agent.allow_write = false` unless you need edits
- Narrow `agent.workspace` to a subdirectory of safe documents when possible
- Do not commit `.yourai/` or secrets
- Run `yourai` as a normal OS user, not root
