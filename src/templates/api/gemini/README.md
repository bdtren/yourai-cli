# Gemini generateContent — SSE

Base URL: `http://127.0.0.1:<port>/v1`

Live mount: [`../routes.toml`](../routes.toml) (`upstream = "gemini_generate"`).
`.http` files here are samples only.

- `POST /v1/models/{model}:generateContent`
- `POST /v1/models/{model}:streamGenerateContent` (SSE)

Google’s public API uses `/v1beta/`; YourAI maps the same shapes onto **`/v1/`** so every
dialect shares one version root (you can add `/v2/` later).

Text chat only in this version. No WebSocket for Gemini — use SSE.
