# API helpers

Provider-specific examples live in the project **`api/`** folder:

- `api/openai/` — Chat Completions HTTP/SSE
- `api/realtime/` — OpenAI-shaped chat WebSocket (`/v1/chat/ws`)
- `api/claude/` — Anthropic Messages SSE
- `api/gemini/` — Gemini generateContent SSE

Which folders exist depends on `server.api_styles` in `yourai.toml`.
