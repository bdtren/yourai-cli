# yourai

Build and run your own private AI on your computer.

`yourai` checks your PC, Mac, or Linux machine, recommends a model that fits your
hardware, downloads it (verified by checksum), and runs it locally with
[llama.cpp](https://github.com/ggml-org/llama.cpp). You get a chat in the terminal,
a chat page in the browser, and an OpenAI-compatible API on `localhost`.
Your prompts never leave your computer.

Website: <https://yourai.bdtren.com>

## Install

macOS and Linux:

```sh
curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash
```

Windows (PowerShell):

```powershell
irm https://yourai.bdtren.com/cli/install.ps1 | iex
```

Homebrew or npm:

```sh
brew install --cask bdtren/yourai/yourai
npm install -g @yourai/cli
```

The script installers put the binary in `~/.yourai/bin` (no `sudo`) and add it to your
`PATH`. They verify the download's SHA-256 against both the website and the GitHub
release before installing.

Installer options: `--version v0.0.1`, `--no-modify-path`, `--dry-run`
(for example `curl -fsSL .../install.sh | bash -s -- --dry-run`). On Windows use the
`YOURAI_VERSION` and `YOURAI_DRY_RUN=1` environment variables.

## Quick start

```sh
yourai doctor          # what is in this computer, and what can it run?
yourai init            # answer a few questions; the best choice is marked "Recommended"
cd my-ai
yourai build           # download the engine and the model
yourai start           # start the AI in the background
yourai chat            # talk to it in the terminal
```

`yourai start` prints the browser address (by default <http://127.0.0.1:8080>) and the API
base URL (`http://127.0.0.1:8080/v1`).

Non-interactive setup (for scripts and CI):

```sh
yourai init --yes --name my-ai
yourai init --yes --model qwen2.5-7b --quant Q4_K_M --ctx 8192
cd my-ai && yourai build --accept-license && yourai start
```

## Commands

| Command | What it does |
| --- | --- |
| `doctor [--network] [--fix]` | Check CPU, RAM, GPU, disk, ports, and what models fit |
| `init [DIR]` | Guided wizard that creates a project folder |
| `build [--accept-license]` | Download and verify the engine and model |
| `start` / `stop` | Start or stop the AI in the background |
| `pause` / `resume` | Unload the model to free memory, then load it again |
| `status` | Running state, port, model, and memory use |
| `logs [-f] [-n N]` | Show server logs |
| `chat` | Chat in the terminal (`/reset` clears history, `/exit` quits) |
| `config show\|get\|set\|edit\|validate` | Read or change `yourai.toml` safely |
| `rebuild` | Apply config changes, redoing only what changed |
| `update [--engine] [--model]` | Refresh the model catalog and apply updates |
| `self-update [--check]` | Update the CLI itself |
| `uninstall` | Remove the CLI (asks separately about models and engines) |
| `completions <shell>` | Print shell completions (bash, zsh, fish, powershell, elvish) |

Run `yourai <command> --help` for details and examples.

## A project folder

```text
my-ai/
├── yourai.toml          # all settings, with comments
├── prompts/system.md    # the system prompt
├── data/docs/           # reserved for document search (later release)
├── scripts/             # start/stop helpers for sh and PowerShell
├── README.md
└── .yourai/             # runtime state, logs, lock file, secrets (git-ignored)
```

Models and engines are shared between projects in `~/.yourai`, so two projects using
the same model download it once.

## Privacy and security

- Everything runs on your computer. The CLI only goes online to download the model
  catalog, the engine, the model, and (once a day) to check for a newer CLI version.
  Set `YOURAI_NO_UPDATE_CHECK=1` to turn the update check off.
- The server listens on `127.0.0.1` by default. Choosing "local network" in the wizard
  generates an API key, stored in `.yourai/secrets.toml` (readable only by you), never in
  `yourai.toml`.
- Every download is checked against a SHA-256 in the catalog. Models that need a license
  accepted ask before downloading.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | General error |
| 2 | Invalid configuration or arguments |
| 3 | Unsupported platform or feature |
| 4 | Not enough memory or disk |
| 5 | Engine or server failed |
| 6 | Security rule blocked the action |
| 7 | Cancelled, or a question needs an answer in a non-interactive run |
| 8 | Download failed or checksum mismatch |

## Environment variables

| Variable | Purpose |
| --- | --- |
| `YOURAI_HOME` | Shared cache folder (default `~/.yourai`) |
| `YOURAI_PROJECT` | Project folder, instead of searching up from the current folder |
| `YOURAI_API_KEY` | API key for LAN mode, instead of `.yourai/secrets.toml` |
| `YOURAI_BASE_URL` | Where releases and the catalog are served (default `https://yourai.bdtren.com/cli`) |
| `YOURAI_CATALOG_URL` | Override only the catalog URL |
| `YOURAI_NO_UPDATE_CHECK` | Turn off the daily CLI update check |
| `HF_TOKEN` | Hugging Face token for gated custom models |
| `NO_COLOR` | Turn off colors |

## Development

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

- `catalog/catalog.json` lists the models and the pinned llama.cpp build. It is bundled
  into the binary and also published at `https://yourai.bdtren.com/cli/catalog.json`, so the
  model list can be updated without a new CLI release.
- `scripts/` has the installers and the release helpers (`package.sh`, `publish_web.sh`,
  `npm_publish.sh`, `update_cask.sh`).

## License

MIT. Models have their own licenses, shown before download.
