<a href="https://buymeacoffee.com/bdtren" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: 41px !important;width: 174px !important;box-shadow: 0px 3px 2px 0px rgba(190, 190, 190, 0.5) !important;-webkit-box-shadow: 0px 3px 2px 0px rgba(190, 190, 190, 0.5) !important;" ></a>

# yourai

Build and run your own private AI on your computer.

`yourai` checks your PC, Mac, or Linux machine, recommends a model that fits your
hardware, downloads it (verified by checksum), and runs it locally with
[llama.cpp](https://github.com/ggml-org/llama.cpp). You get a chat in the terminal,
a chat page in the browser, and an OpenAI-compatible API on `localhost`.
Your prompts never leave your computer.

Optional capabilities, picked in the wizard:

- **Agent**: the terminal chat can read your files (text, code, PDF) and, if you allow it,
  write them. It can also call the capabilities below.
- **Vision**: models such as Qwen2.5-VL, Gemma 3, and Qwen2.5-Omni see images (and Omni hears audio).
- **Images** with [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp)
  (SD-Turbo, FLUX.1 schnell), and **short videos** with Wan2.1 or MiniMax-H3 FL2VA
  (video + stereo audio on high-memory GPUs).
- **Speech to text** with [whisper.cpp](https://github.com/ggml-org/whisper.cpp), and
  **text to speech** with Qwen3-TTS.

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

Homebrew (Homebrew 7 and later asks you to trust third-party taps once):

```sh
brew tap bdtren/yourai
brew trust --tap bdtren/yourai
brew install yourai
```

npm:

```sh
npm install -g @yourai/cli
```

The script installers put the binary in `~/.yourai/bin` (no `sudo`) and add it to your
`PATH`. On macOS and Linux, `install.sh` also links `yourai` into a writable folder that
is already on your `PATH` (such as `~/.local/bin` or `/opt/homebrew/bin`), so it works in
the same terminal right away; `yourai uninstall` removes that link. They verify the
download's SHA-256 against both the website and the GitHub release before installing.

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
base URL (`http://127.0.0.1:8080/v1`). It only starts after `yourai build` has written a root
`yourai.lock` that matches `[store] project_modules` (default: paths under `yourai_modules/`).
If the port is busy, an interactive start asks whether to use the next free port and writes
`server.port` into `yourai.toml`.

New projects get an editable `web/` chat page. When `server.web_ui = true` and
`server.customize_web_ui = true` and `web/index.html` exists, the gateway serves that folder
(llama.cpp’s WebUI is disabled). Set `customize_web_ui = false` to force the default llama UI
without deleting `web/`. API mounts come from `api/routes.toml` (plus optional handlers);
`.http` files under `api/<style>/` are samples only. Local models cannot fetch live weather
or other web data without a tool.

Non-interactive setup (for scripts and CI):

```sh
yourai init --yes --name my-ai
yourai init --yes --model qwen2.5-7b --quant Q4_K_M --ctx 8192
yourai init --yes --name studio --capabilities agent,image,stt,tts --image-model sd-turbo
cd my-ai && yourai build --accept-license && yourai start
```

### Deploy (remote VPS or expose this PC)

In `yourai init`, choose **Deploy to remote** under network access (or pass `--access remote`).
After the project is created, the deploy wizard asks where yourai should run:

1. **Build and run on a remote server** — SSH into an existing droplet/VPS, install yourai, sync the project, build, and start. You get `http://SERVER_IP:port` plus firewall notes.
2. **Self-host on this PC + remote IP access** — bind `0.0.0.0`, require an API key, print LAN/public URLs, firewall hints, and optional `cloudflared` / Tailscale if you have no public IP.

Cloudflare is only an optional DNS/HTTPS checklist afterward (not a place to run the model).

```sh
yourai deploy           # wizard if needed; else run the saved mode
yourai deploy -e        # re-run the wizard (overwrites .yourai/deploy.toml)
yourai deploy --dry-run # print steps without SSH / start
```

Settings live in `.yourai/deploy.toml`; browser/firewall steps are copied to `.yourai/deploy-README.md`.
With `yourai init --yes --access remote`, a stub is written and you finish with `yourai deploy -e`.

With capabilities turned on:

```sh
yourai image "a red fox in the snow"           # saved in outputs/images/
yourai video "waves on a beach at sunset"      # takes minutes; saved in outputs/videos/
yourai speak "Your build is ready."            # saved in outputs/audio/
yourai transcribe meeting.wav                  # prints the text
yourai chat                                    # "summarize notes.pdf", "draw a logo for…"
```

Image, video, and speech engines start the first time you use them and unload after
`server.idle_unload_minutes` (default 10) without use. Only one of image or video is loaded
at a time.

## Commands

| Command | What it does |
| --- | --- |
| `doctor [--network] [--fix]` | Check CPU, RAM, GPU, disk, ports, and what models fit |
| `init [DIR]` | Guided wizard that creates a project folder |
| `build [-s] [-f] [--accept-license]` | Download models/engines (`yourai_modules/` by default; `-s` also saves to `~/.yourai`, `-f` forces a network download) |
| `start` / `stop` | Start or stop the AI in the background |
| `pause` / `resume` | Unload the model to free memory, then load it again |
| `status` | Running state, port, model, and memory use |
| `logs [-f] [-n N]` | Show server logs |
| `chat` | Chat in the terminal; with the agent on it can use tools (`/image`, `/audio`, `/files`, `/reset`, `/help`, `/exit`) |
| `image <prompt> [--size WxH] [--n N]` | Create images (needs `[image]` enabled) |
| `video <prompt> [--frames N] [--fps N]` | Create a short video (needs `[video]` enabled) |
| `speak <text> [--out FILE]` | Text to speech, saved as WAV (needs `[text_to_speech]`) |
| `transcribe <file> [--out FILE]` | Speech to text (needs `[speech_to_text]`) |
| `config show\|get\|set\|edit\|validate` | Read or change `yourai.toml` safely |
| `deploy [-e] [--dry-run]` | Run yourai on a remote VPS (SSH) or expose this PC over an IP (optional tunnel/DNS) |
| `rebuild [-s] [-f]` | Apply config changes, redoing only what changed |
| `update [--engine] [--model] [-s] [-f]` | Refresh the model catalog and apply updates |
| `self-update [--check]` | Update the CLI itself |
| `uninstall` | Remove the CLI (asks separately about models and engines) |
| `completions <shell>` | Print shell completions (bash, zsh, fish, powershell, elvish) |

Run `yourai <command> --help` for details and examples.

## A project folder

```text
my-ai/
├── yourai.toml          # all settings, with comments
├── yourai.lock          # which models/engines this project uses (commit this)
├── yourai_modules/       # downloaded models and engines (git-ignored, like node_modules)
├── web/                 # editable browser chat (when customize_web_ui = true)
├── api/routes.toml      # live HTTP/WS mounts (edit → yourai rebuild)
├── api/handlers/        # optional custom route scripts
├── api/<style>/         # .http / sample clients for chosen api_styles
├── prompts/system.md    # the system prompt
├── data/docs/           # reserved for document search (later release)
├── outputs/             # images, videos, and audio you create
├── scripts/             # start/stop helpers for sh and PowerShell
├── README.md
└── .yourai/             # runtime state, logs, secrets, deploy.toml (git-ignored)
```

By default models and engines live in `yourai_modules/`. Delete that folder anytime, then run
`yourai build` to restore it. Pass `-s` / `--save` to also keep a copy in `~/.yourai` so other
projects can fast-copy it; pass `-f` / `--force` to download from the network instead of using
that cache (`-f -s` refreshes the saved copy). Set `store.project_modules = false` to skip the
project module folder and install/run from `~/.yourai` only.

## API

`server.api_styles` (set in `yourai init` or `yourai config set`) chooses chat dialects, all under `/v1/`:

| Style | Transport | Routes |
| --- | --- | --- |
| `openai` | HTTP SSE + WebSocket | `/v1/chat/completions`, `/v1/chat/ws` |
| `claude` | HTTP SSE | `/v1/messages` |
| `gemini` | HTTP SSE | `/v1/models/{model}:generateContent`, `:streamGenerateContent` |

`api/routes.toml` is the live route table (loaded on `yourai start` / `rebuild`). Style samples
are scaffolded into `api/openai/`, `api/realtime/` (WS), `api/claude/`, and `api/gemini/` as selected.
Add custom endpoints with `handler = "api/handlers/…"` (JSON stdin/stdout; executable script or `node` for `.js`).

The same port also serves OpenAI-style media endpoints for enabled capabilities:

| Endpoint | Capability |
| --- | --- |
| `POST /v1/images/generations` | image |
| `POST /v1/videos/generations` | video (synchronous; can take minutes) |
| `POST /v1/audio/transcriptions` | speech to text (multipart `file`) |
| `POST /v1/audio/speech` | text to speech (returns WAV) |
| `GET /files/<path>` | files in `outputs/` |
| `GET /yourai/status` | which engines are loaded |

Vision models accept OpenAI `image_url` and `input_audio` content parts in chat messages.

## Privacy and security

- Everything runs on your computer. The CLI only goes online to download the model
  catalog, the engine, the model, and (once a day) to check for a newer CLI version.
  Set `YOURAI_NO_UPDATE_CHECK=1` to turn the update check off.
- The server listens on `127.0.0.1` by default. Choosing "local network" in the wizard
  generates an API key, stored in `.yourai/secrets.toml` (readable only by you), never in
  `yourai.toml`.
- Every download is checked against a SHA-256 in the catalog. Models that need a license
  accepted ask before downloading.
- The chat agent only sees `agent.workspace` (default: the project folder). It cannot leave
  that folder, follow symlinks out of it, or read `.yourai/` / `yourai_modules/`. Writing is off unless
  `agent.allow_write = true`, and every write asks you first with a preview.
- Engines listen on `127.0.0.1` only; everything goes through the one public port, which
  checks the API key in LAN mode.

More detail:

| Doc | Topic |
| --- | --- |
| [SECURITY.md](SECURITY.md) | Vulnerability reporting |
| [docs/SECURITY_ARCHITECTURE.md](docs/SECURITY_ARCHITECTURE.md) | Trust boundaries and components |
| [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) | Threats and mitigations |
| [docs/PRIVACY.md](docs/PRIVACY.md) | What stays local vs network use |
| [docs/PERMISSIONS.md](docs/PERMISSIONS.md) | Agent, filesystem, and network permissions |
| [docs/SECURITY_AUDIT.md](docs/SECURITY_AUDIT.md) | CI audits (`cargo audit` / `cargo deny`) |

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
| `YOURAI_HOME` | Shared store/cache (default `~/.yourai`); used with `yourai build -s`, or as the only install root when `store.project_modules = false` |
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

- `catalog/catalog.json` lists the models and the pinned llama.cpp, stable-diffusion.cpp,
  and whisper.cpp builds. It is bundled into the binary and also published at
  `https://yourai.bdtren.com/cli/catalog.v2.json` (the number is the catalog schema), so the
  model list can be updated without a new CLI release.
- `scripts/` has the installers and the release helpers (`package.sh`, `publish_web.sh`,
  `npm_publish.sh`, `update_formula.sh`), plus `build_engine.sh`, `engines_catalog.py`, and
  `publish_engines.sh` for the macOS engine builds that upstream does not ship.

## TODO

Checklist of product features. Checked items ship in this version (`0.0.9`).
Unchecked items are the next work queue (rough priority top → bottom within each section).

### Core flow
- [x] `doctor` — CPU, RAM, GPU, disk, ports, model fit, optional `--network` / `--fix`
- [x] Guided `init` wizard with Recommended choices and reasons
- [x] Non-interactive `init --yes` (and flag overrides) for scripts and CI
- [x] Init questions for editable web UI and API styles (`--customize-web-ui`, `--api-styles`)
- [x] Project scaffold (`yourai.toml`, `web/`, `api/routes.toml`, prompts, scripts, `.yourai/`)
- [x] `build` — download engine + model with SHA-256 checks and license prompt
- [x] `start` / `stop` / `status` / `logs`
- [x] `pause` / `resume` (unload model to free memory)
- [x] Terminal `chat` (`/reset`, `/exit`, `/help`)
- [x] Browser chat page (project `web/` UI, with llama.cpp WebUI fallback)
- [x] `server.customize_web_ui` — keep `web/` but force default llama UI when false
- [x] `[store] project_modules` — project-local modules (default) or shared `~/.yourai` only
- [x] OpenAI-compatible API on `localhost` (`/v1`) plus optional Claude/Gemini adapters
- [x] OpenAI chat WebSocket (`/v1/chat/ws`) and `api/realtime/` scaffold
- [x] `api/routes.toml` + `api/handlers/` for remappable mounts and custom endpoints
- [x] `config show|get|set|edit|validate` (preserves TOML comments)
- [x] Smart `rebuild` (only redo what changed)
- [x] `update` catalog / `--engine` / `--model`
- [x] `self-update` with install-method detection (script, Homebrew, npm)
- [x] `uninstall` (asks separately about models and engines)
- [x] Shell completions (bash, zsh, fish, powershell, elvish)
- [x] Clear exit codes and `--debug` / `NO_COLOR`
- [ ] Streaming / SSE custom handlers (today custom `handler` routes are buffered JSON only)
- [ ] Move media mounts (`/v1/images|videos|audio…`) into `api/routes.toml`

### Hardware and models
- [x] Hardware report with source and confidence
- [x] Model catalog (bundled + remote refresh)
- [x] Recommender for chat and code goals / quantization / context
- [x] Project-local `yourai_modules/` + `yourai.lock`, optional `~/.yourai` cache with `-s`/`-f`
- [x] Resumable downloads
- [x] llama.cpp runtime (Metal, CUDA, Vulkan, CPU builds)
- [ ] Custom / gated Hugging Face models end-to-end in the wizard
- [ ] Ollama as a managed runtime
- [ ] vLLM as a managed runtime

### Agent and multimodal
- [x] Capabilities multi-select in the wizard, with a model question and recommendation for each
- [x] Agent tools in `chat`: list, read (text, code, PDF), and write files in a sandboxed workspace
- [x] Vision and omni chat models (`/image`, `/audio` attachments; `image_url` / `input_audio` in the API)
- [x] Image generation (`yourai image`, `/v1/images/generations`) with stable-diffusion.cpp
- [x] Video generation (`yourai video`, `/v1/videos/generations`) with Wan2.1 and MiniMax-H3 FL2VA (video + stereo audio)
- [x] Speech to text (`yourai transcribe`, `/v1/audio/transcriptions`) with whisper.cpp
- [x] Text to speech (`yourai speak`, `/v1/audio/speech`) with Qwen3-TTS
- [x] One port for everything: gateway with on-demand engines and idle unload
- [x] Config schema 2 with automatic migration of older `yourai.toml` files
- [ ] Document search / RAG (`data/docs/`, index, retrieve into prompts)
- [ ] Agent tools that run commands or browse the web
- [ ] Image editing and image-to-video
- [ ] Voice cloning from a sample in the wizard

### Privacy, security, and network
- [x] Default bind to `127.0.0.1`
- [x] LAN mode with generated API key (secrets file, not `yourai.toml`)
- [x] `deploy` — remote VPS (SSH) or expose this PC over an IP; optional tunnel/DNS checklist
- [x] Daily CLI update check with `YOURAI_NO_UPDATE_CHECK` opt-out
- [x] Security docs (`SECURITY.md`, threat model, permissions, audit practice)
- [x] CI supply-chain checks (`cargo audit`, `cargo deny`, Dependabot config)
- [x] Turn on macOS code signing and notarization by default in release
- [ ] Auto-start on login

### Distribution
- [x] `install.sh` / `install.ps1` (SHA-256 vs website and GitHub)
- [x] GitHub Releases for six targets (macOS, Linux musl, Windows; arm64 + x64)
- [x] Website channel at `https://yourai.bdtren.com/cli/`
- [x] Homebrew formula (`brew install bdtren/yourai/yourai`)
- [x] npm packages (`@yourai/cli` + platform packages)
- [x] Community docs: contributing guide and code of conduct
- [ ] Submit formula/cask to the official Homebrew catalogues
- [ ] Desktop wrapper app

### Later modules
- [ ] Fine-tuning tools (managed train env → GGUF)
- [ ] Plugins / hooks (RAG injection in the gateway)
- [ ] Community-contributed model catalog

## Contributing

See the [contributing guide](CONTRIBUTING.md) to learn how to contribute to the repository and the development workflow. Please also read the [code of conduct](CODE_OF_CONDUCT.md).

If yourai helps you, you can [buy me a coffee](https://buymeacoffee.com/bdtren).

## License

Apache License 2.0. Models have their own licenses, shown before download.
