# OpenAI-style HTTP API (SSE)

Base URL: `http://127.0.0.1:<port>/v1`

Live mounts are defined in [`../routes.toml`](../routes.toml) (`upstream = "openai_chat"`).
This folder’s `.http` files are **request samples only** — editing them does not change the gateway.

- `POST /v1/chat/completions` — Chat Completions (set `"stream": true` for SSE)
- Media (when enabled): `/v1/images/generations`, `/v1/videos/generations`, `/v1/audio/*`

For WebSocket chat, see [`../realtime/`](../realtime/).

Custom routes: add a `[[route]]` with `handler = "api/handlers/…"`, then `yourai rebuild`.
