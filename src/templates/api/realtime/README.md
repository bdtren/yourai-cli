# OpenAI-shaped chat WebSocket

Endpoint: `ws://127.0.0.1:<port>/v1/chat/ws`

(Only when `server.api_styles` includes `openai`.)

## Protocol

1. Connect (send `Authorization: Bearer <key>` if `require_api_key` is on).
2. Send **one text frame**: a Chat Completions JSON body (same as HTTP).
3. Streaming (default, or `"stream": true`): receive text frames that are
   `chat.completion.chunk` JSON objects, then a final `"[DONE]"` frame.
4. Non-streaming (`"stream": false`): one frame with a full `chat.completion` object.
5. Errors: one JSON frame `{"error":{"message","type"}}`, then the socket may close.

This is **not** OpenAI Realtime audio — it is chat Completions over WebSocket so
browser and native clients can avoid SSE. HTTP SSE remains at `/v1/chat/completions`.
