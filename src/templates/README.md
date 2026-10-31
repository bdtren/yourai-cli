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
(if `server.web_ui = true`), or point any OpenAI-compatible app at `http://127.0.0.1:<port>/v1`.

## Files

| File | Who owns it | What it is |
|---|---|---|
| `yourai.toml` | you | All settings. Edit it, then run `yourai rebuild`. |
| `prompts/system.md` | you | The instructions the AI follows in `yourai chat`. |
| `data/docs/` | you | Reserved for document search in a future version. |
| `scripts/` | you | Optional `start`/`stop` wrappers for shells and PowerShell. |
| `.yourai/` | yourai | Runtime state, logs, lock file, and secrets. Gitignored; do not share. |

Models and engines are stored once in `~/.yourai` and shared by all projects.

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
