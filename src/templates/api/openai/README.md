# OpenAI-style HTTP API (SSE)

Base URL: `http://127.0.0.1:<port>/v1`

- `POST /v1/chat/completions` — Chat Completions (set `"stream": true` for SSE)
- Media (when enabled): `/v1/images/generations`, `/v1/videos/generations`, `/v1/audio/*`

For WebSocket chat, see [`../realtime/`](../realtime/).

All YourAI chat routes use the `/v1/` prefix so you can add your own `/v2/` later.
