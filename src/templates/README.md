# {{name}}

A local AI project created by [YourAI](https://yourai.bdtren.com). Everything runs on this computer.

## Everyday commands

```bash
yourai build     # download the engine and model (first time, or after model changes)
yourai start     # run the AI in the background
yourai status    # is it running? which port? how much memory?
yourai chat      # talk to it in the terminal
yourai pause     # unload the model to free memory
yourai resume    # load it again
yourai stop      # stop it
yourai logs -f   # follow the server log
```

When it is running, open the URL printed by `yourai start` in a browser for the chat page
(if `server.web_ui = true`). With `server.customize_web_ui = true`, edit `web/` to customize
that page; set it to `false` to use the default llama.cpp UI without deleting `web/`.
API mounts live in `api/routes.toml` (plus optional `api/handlers/`). Or point any
OpenAI-compatible app at `http://127.0.0.1:<port>/v1`.

If you turned on capabilities in `yourai.toml`:

```bash
yourai image "a red fox in the snow"     # [image]
yourai video "waves on a beach"          # [video], takes minutes
yourai speak "Hello there"               # [text_to_speech]
yourai transcribe recording.wav          # [speech_to_text]
```

With `[agent] enabled = true`, `yourai chat` can read files in `agent.workspace` (and write them
after asking, if `allow_write = true`) and use the capabilities above. Type `/help` in the chat.

## Files

| File | Who owns it | What it is |
|---|---|---|
| `yourai.toml` | you | All settings. Edit it, then run `yourai rebuild`. |
| `prompts/system.md` | you | The instructions the AI follows in `yourai chat`. |
| `data/docs/` | you | Reserved for document search in a future version. |
| `scripts/` | you | Optional `start`/`stop` wrappers for shells and PowerShell. |
| `web/` | you | Browser chat page when `customize_web_ui = true`. Edit freely. |
| `api/routes.toml` | you | Live HTTP/WS mounts (edit, then `yourai rebuild`). |
| `api/handlers/` | you | Optional custom route scripts (JSON stdin/stdout). |
| `api/<style>/` | you | `.http` / sample clients for the API styles you chose. |
| `outputs/` | yourai | Images, videos, and audio you create. Gitignored. |
| `yourai.lock` | yourai | Which models and engines this project uses. Safe to commit. |
| `yourai_modules/` | yourai | Downloaded models and engines for this project (when `store.project_modules = true`). Gitignored; delete anytime and run `yourai build` again. |
| `.yourai/` | yourai | Runtime state, logs, and secrets. Gitignored; do not share. |

By default `yourai build` installs into `yourai_modules/`. Use `yourai build -s` to also keep a
copy in `~/.yourai` for other projects, and `yourai build -f` to download from the network instead
of using that cache (`-f -s` updates the saved copy). Set `store.project_modules = false` in
`yourai.toml` to skip `yourai_modules/` and install/run from `~/.yourai` only.

## Changing settings

```bash
yourai config show
yourai config set generation.temperature 0.5
yourai config set runtime.context_length 8192
yourai config validate
yourai rebuild
```

`yourai rebuild` only redoes what changed: a restart for generation, server, or prompt changes;
a download for model or engine changes.

## Privacy

Prompts and answers never leave this computer. `yourai` only uses the network to download the
catalog, the engine, the model, and CLI updates. The server listens on `127.0.0.1` unless you
change `server.host`, and then an API key is required.
