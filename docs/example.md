# Example: Duy builds a private study buddy

A complete walkthrough of `yourai`, from a fresh install to sharing the AI on a
phone, a VPS, or over an IP. The person and project are made up; the commands and
output come from a real run of yourai v0.0.7 (some numbers are rounded).

**Who:** Duy, a university student.
**Computer:** MacBook Pro, Apple M5 Pro, 32 GB memory.
**Goal:** a private AI that explains lecture topics and quizzes him before exams,
works offline, and never sends his notes to anyone.

---

## 1. Install

```console
$ curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash
yourai: installing yourai v0.0.7 for aarch64-apple-darwin
######################################################################## 100.0%
yourai: checksum verified against https://yourai.bdtren.com/cli and GitHub
yourai: added /Users/duy/.yourai/bin to PATH in /Users/duy/.zshrc
yourai: installed to /Users/duy/.yourai/bin/yourai
yourai: linked /opt/homebrew/bin/yourai, so you can run yourai now
yourai: next: run  yourai doctor  then  yourai init
```

He opens a new terminal tab so the `PATH` change takes effect.

## 2. Check the computer: `yourai doctor`

```console
$ yourai doctor

Your computer
┌──────────────────────────┬───────────────────────────────────────────┬───────────────────────────────────┬────────────┐
│ Item                     ┆ Value                                     ┆ Source                            ┆ Confidence │
╞══════════════════════════╪═══════════════════════════════════════════╪═══════════════════════════════════╪════════════╡
│ Operating system         ┆ macOS 15.6 (aarch64)                      ┆ system                            ┆ high       │
│ CPU                      ┆ Apple M5 Pro · 16 cores / 16 threads      ┆ system                            ┆ high       │
│ RAM                      ┆ 32.0 GB total, 9.5 GB free now            ┆ system                            ┆ high       │
│ GPU                      ┆ Apple Silicon GPU (Metal, unified memory) ┆ drivers / system tools            ┆ high       │
│ GPU memory               ┆ about 20.8 GB usable (estimate)           ┆ estimated share of unified memory ┆ medium     │
│ Free disk (~/.yourai)    ┆ 256.0 GB                                  ┆ system                            ┆ high       │
│ Memory budget for models ┆ 20.8 GB (GPU memory)                      ┆ yourai estimate                   ┆ medium     │
└──────────────────────────┴───────────────────────────────────────────┴───────────────────────────────────┴────────────┘

Checks
┌──────────────┬────────┬────────────────────────────────────────┬────────────┐
│ Check        ┆ Result ┆ Details                                ┆ What to do │
╞══════════════╪════════╪════════════════════════════════════════╪════════════╡
│ RAM          ┆ ok     ┆ 32.0 GB                                ┆            │
│ Disk space   ┆ ok     ┆ 256.0 GB free                          ┆            │
│ Engine build ┆ ok     ┆ llama.cpp b12015 (macos-aarch64-metal) ┆            │
│ Port 8080    ┆ ok     ┆ free                                   ┆            │
│ Ollama       ┆ ok     ┆ not installed (not needed)             ┆            │
└──────────────┴────────┴────────────────────────────────────────┴────────────┘

What you can run
• chat: Qwen2.5 14B Instruct with Q5_K_M (about 10.2 GB of memory)
• code: Qwen2.5 Coder 14B Instruct with Q5_K_M (about 10.2 GB of memory)
• vision: Gemma 3 12B Instruct with Q4_K_M (about 9.4 GB of memory)
• image generation: FLUX.1 schnell (high quality) (about 12.3 GB of memory while working)
• video generation: Wan2.1 T2V 1.3B (text to video) (about 9.3 GB of memory while working)
• speech to text: Whisper small (better) (about 1.2 GB of memory while working)
• text to speech: Qwen3-TTS 1.7B (10 languages) (about 2.4 GB of memory while working)
Network was not tested. Run `yourai doctor --network` to check downloads.

Next: yourai init
```

His Mac can run a 14B model, and also create images, videos, and speech. He decides to
start tiny to try things out, and add more later (steps 9 and 10).

## 3. Create the project: `yourai init`

The interactive wizard asks one question at a time and marks the best choice for his
hardware as **Recommended**. The first two questions are the ones people often stop on.

### Goal (pick one)

```console
$ yourai init
? Project name: study-buddy

What do you want to do?
Your goal decides which models and settings we suggest.

┌───────────────────────────────┬──────────────────────────────────────────────────────────────────────────────────┐
│ Option                        ┆ What it means                                                                    │
╞═══════════════════════════════╪══════════════════════════════════════════════════════════════════════════════════╡
│ Chat assistant  (Recommended) ┆ A general assistant you talk to in the terminal, a browser, or via an API.       │
│                               ┆ Simplest setup and lowest hardware needs.                                        │
│ Coding assistant              ┆ Uses a model tuned for code with a longer context. Better for programming help.  │
└───────────────────────────────┴──────────────────────────────────────────────────────────────────────────────────┘

Recommended for first-time users: it works on almost any computer.
? Choose one:
> Chat assistant (Recommended)
  Coding assistant
```

Duy keeps **Chat assistant** (Enter). Coding would swap in a code-tuned model and a longer
context later; either goal can still turn on the capabilities below.

### Capabilities (pick any)

This is a multi-select. Space toggles a row, Enter confirms. On his M5 Pro (about 20.8 GB of
usable GPU memory) every option is pre-ticked, including short videos.

```console
What else should it be able to do?
Pick any. Each one adds a download; image, video, and speech engines only load while you use
them. Change them later in yourai.toml.

┌──────────────────────────────────────────┬────────────────────────────────────────────────────────────────────────┐
│ Option                                   ┆ What it means                                                          │
╞══════════════════════════════════════════╪════════════════════════════════════════════════════════════════════════╡
│ Read files (agent)  (Recommended)        ┆ `yourai chat` can list and read files in the project (text, code,      │
│                                          ┆ PDF) and use the tools you turn on below. Writing files is off until   │
│                                          ┆ you allow it.                                                          │
│ See images and hear audio  (Recommended) ┆ Picks a chat model that understands pictures (and, with omni models,   │
│                                          ┆ audio). Attach files with /image and /audio in `yourai chat`.          │
│ Create images  (Recommended)             ┆ Make pictures from a text description. Runs only while you use it.     │
│ Speech to text  (Recommended)            ┆ Turn recordings into text with Whisper (many languages).               │
│ Text to speech  (Recommended)            ┆ Read answers aloud and save them as WAV files.                         │
│ Create short videos  (Recommended)       ┆ A few seconds of video from a text description. Slow: minutes per      │
│                                          ┆ clip even on a good GPU.                                               │
└──────────────────────────────────────────┴────────────────────────────────────────────────────────────────────────┘

Recommended for 20.8 GB of GPU memory: everything that runs comfortably on it.
? Choose any (space to toggle, enter to confirm):
> [x] Read files (agent) (Recommended)
  [x] See images and hear audio (Recommended)
  [x] Create images (Recommended)
  [x] Speech to text (Recommended)
  [x] Text to speech (Recommended)
  [x] Create short videos (Recommended)
```

He unticks vision, images, speech, and video with Space (he wants a tiny first download),
leaves **Read files (agent)** on, and presses Enter. Later questions then pick the chat model,
quantization, context length, interface, network access, and port. Each capability he left on
would have gotten its own model question.

### Same project in one line

To skip the questions and start tiny, he runs:

```console
$ yourai init --yes --name study-buddy --model qwen2.5-0.5b

Review
┌────────────────┬───────────────────────────────────────┐
│ Setting        ┆ Your choice                           │
╞════════════════╪═══════════════════════════════════════╡
│ Project folder ┆ study-buddy                           │
│ Goal           ┆ Chat assistant                        │
│ Capabilities   ┆ Read files (agent)                    │
│ Model          ┆ Qwen2.5 0.5B Instruct (Small, 0.4 GB) │
│ Quantization   ┆ Q5_K_M                                │
│ Runtime        ┆ llama.cpp                             │
│ Context length ┆ 4096 tokens                           │
│ Interface      ┆ API + browser chat page               │
│ Network access ┆ Only this computer (127.0.0.1)        │
│ Port           ┆ 8080                                  │
└────────────────┴───────────────────────────────────────┘
┌──────────────────────┬────────────────────────────────────────────────────┐
│ Estimate             ┆ Amount                                             │
╞══════════════════════╪════════════════════════════════════════════════════╡
│ Download             ┆ 420 MB model + 16 MB engine                        │
│ Memory while running ┆ about 869 MB (budget 20.8 GB)                      │
│ Disk use             ┆ about 461 MB in yourai_modules/ (project-local)     │
└──────────────────────┴────────────────────────────────────────────────────┘
Nothing is downloaded yet. `yourai build` downloads after you confirm the model license.
✔ Created study-buddy

Next: cd study-buddy && yourai build && yourai start
```

With `--yes` and no `--capabilities`, only the agent is turned on: `yourai chat` can read
files in the project, and nothing extra is downloaded. Pass
`--capabilities agent,image,stt` (and the matching `--image-model` / `--stt-model` flags) when
you want media in a non-interactive run.

The new folder:

```text
study-buddy/
├── yourai.toml          # all settings, with comments
├── yourai.lock          # written by `yourai build` (commit this)
├── yourai_modules/       # models and engines (gitignored; delete anytime)
├── web/                 # editable browser chat page
├── prompts/system.md    # the instructions the AI follows
├── data/docs/           # reserved for document search (later release)
├── outputs/             # images, videos, and audio he creates (step 10)
├── scripts/             # start/stop helpers
├── README.md
└── .gitignore           # ignores .yourai/, yourai_modules/, and outputs/
```

He makes it a study tutor by editing `prompts/system.md`:

```markdown
You are Duy's study buddy. Explain topics simply, with one short example.
When he says "quiz me", ask one question at a time and wait for his answer.
If you are not sure about a fact, say so.
```

## 4. Download: `yourai build`

```console
$ cd study-buddy
$ yourai build
Model license
Qwen2.5 0.5B Instruct is distributed under: Apache-2.0
  https://www.apache.org/licenses/LICENSE-2.0
? Accept this license and download the model? Yes
Engine
Downloading engine llama-b12015-bin-macos-arm64.tar.gz…
✔ engine llama-b12015-bin-macos-arm64.tar.gz downloaded and verified
✔ engine llama.cpp b12015 (macos-aarch64-metal) installed
Model
Downloading model Qwen2.5 0.5B Instruct…
✔ model Qwen2.5 0.5B Instruct downloaded and verified
✔ study-buddy is built
Next: yourai start
```

Every file is checked against a SHA-256 checksum. If the Wi-Fi drops, running
`yourai build` again resumes where it stopped.

## 5. Run it: `yourai start` and `yourai status`

```console
$ yourai start
Loading the model (first start can take a minute)…
✔ study-buddy is running (pid 14631, loaded in 4s)
• Chat in your browser: http://127.0.0.1:8080
• OpenAI-compatible API: http://127.0.0.1:8080/v1

Next: yourai chat   (or open the URL above)

$ yourai status
┌──────────────┬────────────────────────────────────────────────────────┐
│ State        ┆ running                                                │
│ Project      ┆ study-buddy (/Users/duy/study-buddy)                  │
│ Model        ┆ qwen2.5-0.5b Q5_K_M                                    │
│ Built        ┆ yes, engine llama.cpp b12015 (macos-aarch64-metal)     │
│ Address      ┆ http://127.0.0.1:8080  (API: http://127.0.0.1:8080/v1) │
│ Process      ┆ pid 14631                                              │
│ Memory (RAM) ┆ 615 MB                                                 │
│ Uptime       ┆ 0h 0m 4s                                               │
│ Health       ┆ healthy                                                │
└──────────────┴────────────────────────────────────────────────────────┘
```

It runs in the background, so he can close the terminal. He also bookmarks
<http://127.0.0.1:8080> for the browser chat page.

## 6. Talk to it: `yourai chat`

```console
$ yourai chat
Chatting with study-buddy (qwen2.5-0.5b). Type /help for commands, /exit to quit.
Agent tools: list_files, read_file.
you › Explain photosynthesis in one sentence.
ai › Photosynthesis is the process by which plants, algae, and some bacteria turn
light energy into chemical energy stored in glucose, using carbon dioxide and water.

you › Read notes.txt and tell me the code word.
⚙ read_file({"path":"notes.txt"})
ai › The secret code word is PELICAN.

you › /reset
✔ conversation cleared
you › /exit
```

The agent can only see the project folder (`agent.workspace`), never `.yourai/` or `yourai_modules/` where the
secrets live, and it can also read PDFs. It cannot change files unless he sets
`agent.allow_write = true`, and even then it shows a preview and asks first.

## 7. Use it from his own code (OpenAI-compatible API)

Any tool that speaks the OpenAI API works when pointed at `http://127.0.0.1:8080/v1`.

```console
$ curl -s http://127.0.0.1:8080/v1/chat/completions \
    -H 'Content-Type: application/json' \
    -d '{"messages":[{"role":"user","content":"Quiz me: one question about the water cycle."}]}'
```

In Python with the `openai` package:

```python
from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:8080/v1", api_key="not-needed")
reply = client.chat.completions.create(
    model="study-buddy",
    messages=[{"role": "user", "content": "Quiz me: one question about the water cycle."}],
)
print(reply.choices[0].message.content)
```

## 8. Tune it: `yourai config` and `yourai rebuild`

Answers feel too random for quizzes, so he lowers the temperature:

```console
$ yourai config get generation.temperature
0.7
$ yourai config set generation.temperature 0.3
✔ generation.temperature = 0.3
• Restart needed: run `yourai rebuild` (fast, nothing is downloaded).
$ yourai rebuild
• changed: generation
• restarting to apply changes
✔ study-buddy is running (pid 16399, loaded in 1s)
```

To paste in whole lecture summaries he needs a bigger context:

```console
$ yourai config set runtime.context_length 8192
✔ runtime.context_length = 8192
• Engine or model change: run `yourai rebuild` (may download files).
$ yourai rebuild
• changed: runtime
• model or runtime changed: checking downloads (files already present are reused)
✔ engine llama.cpp b12015 (macos-aarch64-metal) is installed
✔ model Qwen2.5 0.5B Instruct already downloaded and verified
• restarting to apply changes
✔ study-buddy is running (pid 16499, loaded in 1s)
```

`rebuild` only redoes what changed. Bad values are refused before anything is written:

```console
$ yourai config set server.host 0.0.0.0
error: server.host exposes the AI to your network, so server.require_api_key must be true
hint: run `yourai config set server.require_api_key true`
```

`yourai config show` prints the whole `yourai.toml`, `yourai config edit` opens it in
his editor, and `yourai config validate` checks it.

## 9. Upgrade the model

The 0.5B model is fast but sometimes gets facts wrong. `doctor` said his Mac can run
much bigger models, so he switches to the 7B one:

```console
$ yourai config set model.id qwen2.5-7b
$ yourai config set model.quant Q4_K_M
$ yourai rebuild
• changed: model
• model or runtime changed: checking downloads (files already present are reused)
Model
Downloading model Qwen2.5 7B Instruct…
✔ model Qwen2.5 7B Instruct downloaded and verified
• restarting to apply changes
✔ study-buddy is running
```

The old model stays in `yourai_modules/models`, so switching back is instant. With
`yourai build -s`, a copy also stays in `~/.yourai` for other projects.

## 10. Pictures and speech: `yourai image`, `speak`, and `transcribe`

For his biology slides he turns on image creation with the fast SD-Turbo model, and
speech in both directions so he can listen to summaries and transcribe lectures:

```console
$ yourai config set image.enabled true
$ yourai config set image.model sd-turbo
$ yourai config set speech_to_text.enabled true
$ yourai config set text_to_speech.enabled true
$ yourai rebuild
• changed: image, speech_to_text, text_to_speech
• models or engines changed: checking downloads (files already present are reused)
Engines
✔ engine llama.cpp b12015 (macos-aarch64-metal) is installed
✔ engine stable-diffusion.cpp master-1031-c6dfa9d (macos-aarch64-metal) installed
✔ engine whisper.cpp v1.9.5 (macos-aarch64-metal) installed
Models
✔ model SD-Turbo (fast, 512px) downloaded and verified
✔ model Whisper small (better) downloaded and verified
✔ model Qwen3-TTS 1.7B (10 languages) downloaded and verified
✔ study-buddy is running
• Also ready: `yourai image`, `yourai transcribe`, `yourai speak` (engines load on first use, unload after 10 idle minutes)

$ yourai image "a cute cartoon plant cell diagram"
Creating the image (the first one also loads the model)…
✔ saved outputs/images/a-cute-cartoon-plant-cell-diagram-1793519664.png
• took 8s

$ yourai speak "Photosynthesis turns light into chemical energy."
✔ saved outputs/audio/photosynthesis-turns-light-into-chemical-energy-1793519668.wav (4.5s)

$ yourai transcribe lecture-week3.wav --out lecture-week3.txt
✔ wrote lecture-week3.txt (21.3s)
```

Everything still goes through port 8080, so apps can use `/v1/images/generations`,
`/v1/audio/speech`, and `/v1/audio/transcriptions` too. The image and speech engines only
use memory while he uses them; `yourai status` shows which are loaded:

```console
$ yourai status
…
│ image          ┆ sd-turbo — loaded, 2.0 GB RAM, port 39101          │
│ speech to text ┆ whisper-small — ready (loads on first use)         │
│ text to speech ┆ qwen3-tts-1.7b — runs per request                  │
```

With the agent on, `yourai chat` can use these too: "draw a diagram of mitosis" makes the
model call `generate_image`, and the picture is saved in `outputs/images/`.

## 11. Free memory for a game: `yourai pause` and `yourai resume`

```console
$ yourai pause
Unloading the model…
✔ paused: the model is unloaded and its memory is free

Next: yourai resume

$ yourai resume
Loading the model (first start can take a minute)…
✔ study-buddy is running (pid 15307, loaded in 1s)
```

## 12. Something odd? `yourai logs`

```console
$ yourai logs -n 4
0.00.234.824 I cmn          init: llama threadpool init, n_threads = 8
0.00.262.097 I srv    load_model: initializing, n_slots = 4, n_ctx_slot = 8192
0.00.264.492 I srv  server_run_i: model loaded
0.00.264.496 I srv  server_on_li: listening on http://127.0.0.1:8080
```

`yourai logs -f` keeps following new lines until Ctrl+C.

## 13. Use it from his phone (local network)

He creates a second project that other devices on his Wi-Fi can reach:

```console
$ cd ~ && yourai init --yes --name study-buddy-lan --model qwen2.5-7b --access lan
! Other devices on your network will be able to reach this AI. An API key will be generated and required.
✔ Created study-buddy-lan
• API key saved to study-buddy-lan/.yourai/secrets.toml (yai-da47…). Apps must send it as `Authorization: Bearer <key>`.
$ cd study-buddy-lan && yourai build && yourai start
```

The key is never written to `yourai.toml`. Requests without it get `401`; requests
with `Authorization: Bearer <key>` work. The 7B model is already downloaded, so `build`
only takes a second.

## 13b. Deploy to a VPS or expose this PC: `yourai deploy`

Later he wants classmates off-campus to try the tutor. In init he could have picked
**Deploy to remote**; from an existing project he runs:

```console
$ cd ~/study-buddy
$ yourai deploy -e

Where should yourai run?
? Choose one:
> Build and run yourai on a remote server
  Self-host on this PC and grant remote access via an IP
```

**Mode A — remote server:** he already has a DigitalOcean droplet. The wizard asks for
`root@DROPLET_IP`, generates `~/.ssh/id_ed25519_yourai` if needed, prints the public key
and the DigitalOcean Security URL, waits until `ssh` works, then installs yourai on the
droplet, syncs the project, builds, and starts. He opens TCP 8080 on the cloud firewall and
opens `http://DROPLET_IP:8080` with the API key. Optional Cloudflare DNS is only a checklist
after that (grey-cloud A record, then Full SSL with Caddy) — not a third place to run the model.

**Mode B — this PC + IP:** he keeps the model on the Mac. Deploy sets `server.host = 0.0.0.0`,
requires an API key, prints `http://LAN_IP:8080`, firewall tips, and offers `cloudflared` if
there is no usable public IP / port-forward.

```console
$ yourai deploy --dry-run
• [dry-run] would set server.require_api_key=true and server.host=0.0.0.0
✔ Wrote study-buddy/.yourai/deploy-README.md
```

Settings stay in `.yourai/deploy.toml`. Non-interactive init writes a stub:

```console
$ yourai init --yes --name buddy-remote --access remote --model qwen2.5-7b
• Wrote a deploy stub in .yourai/deploy.toml (non-interactive init).
Next: cd buddy-remote && yourai deploy -e
```

## 14. Keep things fresh: `yourai update` and `yourai self-update`

```console
$ cd ~/study-buddy
$ yourai update
Refreshing the model catalog…

Updates for this project
┌─────────────────────────────────────┬─────────────────────┬─────────────────────┬────────────┐
│ Component                           ┆ Installed           ┆ Latest              ┆            │
╞═════════════════════════════════════╪═════════════════════╪═════════════════════╪════════════╡
│ Engine (llama.cpp)                  ┆ b12015              ┆ b12015              ┆ up to date │
│ Engine (stable-diffusion.cpp)       ┆ master-1031-c6dfa9d ┆ master-1031-c6dfa9d ┆ up to date │
│ Engine (whisper.cpp)                ┆ v1.9.5              ┆ v1.9.5              ┆ up to date │
│ Model qwen2.5-7b Q4_K_M             ┆ 65b8fcd92af6…       ┆ 65b8fcd92af6…       ┆ up to date │
│ Image generation model sd-turbo     ┆ d50be7655f0a…       ┆ d50be7655f0a…       ┆ up to date │
│ Speech to text model whisper-small  ┆ 1be3a9b20638…       ┆ 1be3a9b20638…       ┆ up to date │
│ Text to speech model qwen3-tts-1.7b ┆ 8d18c94acb2a…       ┆ 8d18c94acb2a…       ┆ up to date │
└─────────────────────────────────────┴─────────────────────┴─────────────────────┴────────────┘

$ yourai self-update --check
Checking for a new version…
✔ yourai v0.0.7 is the latest version
```

## 15. Done for the semester: `yourai stop` and `yourai uninstall`

```console
$ yourai stop
Stopping…
✔ stopped

$ yourai uninstall
This will remove
• the yourai CLI: /Users/duy/.yourai/bin/yourai
• the PATH line added to /Users/duy/.zshrc
• Your projects are never touched.
? Remove the yourai CLI? Yes
? Also delete downloaded models and engines in /Users/duy/.yourai (5.1 GB)? They are slow to download again. No
✔ removed yourai. Open a new terminal to refresh PATH.
```

He keeps the models, so reinstalling next semester needs no downloads.

---

## Command summary

| Step | Command | What it did for Duy |
| --- | --- | --- |
| 2 | `yourai doctor` | Showed his Mac can run up to a 14B model |
| 3 | `yourai init` | Created `study-buddy/` with a reviewed plan |
| 4 | `yourai build` | Downloaded and verified the engine and model |
| 5 | `yourai start`, `status` | Ran it in the background on port 8080 |
| 6 | `yourai chat` | Chatted in the terminal; the agent read his notes |
| 7 | API at `/v1` | Used it from curl and Python |
| 8 | `yourai config`, `rebuild` | Lowered temperature, raised context |
| 9 | `config set model.id` + `rebuild` | Upgraded to the 7B model |
| 10 | `yourai image`, `speak`, `transcribe` | Made a slide picture, listened to and transcribed lectures |
| 11 | `yourai pause`, `resume` | Freed memory, then reloaded |
| 12 | `yourai logs` | Looked at the server log |
| 13 | `init --access lan` | Reached it from his phone with an API key |
| 13b | `yourai deploy -e` | Remote VPS (SSH) or expose this PC over an IP |
| 14 | `yourai update`, `self-update` | Checked for newer engines, models, and CLI |
| 15 | `yourai stop`, `uninstall` | Stopped it and removed the CLI, kept models |
