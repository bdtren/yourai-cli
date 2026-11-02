# Claude (Anthropic) Messages API — SSE

Base URL: `http://127.0.0.1:<port>/v1`

- `POST /v1/messages` — Anthropic Messages shape; `"stream": true` returns Anthropic SSE events

Auth: `Authorization: Bearer <key>` or `x-api-key: <key>` when required.

Text chat only in this version. WebSocket is not offered for Claude — use SSE.
All routes stay under `/v1/` so you can add `/v2/` yourself later.
