# OpenAI-style HTTP API (SSE)

Base URL: `http://127.0.0.1:<port>/v1`

Live mounts are defined in [`../routes.toml`](../routes.toml) (`upstream = "openai_chat"`, plus media upstreams).
This folder’s `.http` files are **request samples only** — editing them does not change the gateway.

- `POST /v1/chat/completions` — Chat Completions (set `"stream": true` for SSE)
- Media: `images_generations` / `videos_generations` / `audio_transcriptions` / `audio_speech` in `routes.toml`

For WebSocket chat, see [`../realtime/`](../realtime/).

Custom routes: add a `[[route]]` with `handler = "api/handlers/…"`, then `yourai rebuild`.
Buffered handlers return JSON on stdout; set `stream = true` to pipe raw stdout (SSE-friendly).
