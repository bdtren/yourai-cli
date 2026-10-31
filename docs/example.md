# Example: Linh builds a private study buddy

A complete walkthrough of `yourai`, from a fresh install to sharing the AI with a
phone. The person and project are made up; the commands and output come from a real
run of yourai v0.0.1 (some numbers are rounded).

**Who:** Linh, a university student.
**Computer:** MacBook Pro, Apple M5 Pro, 32 GB memory.
**Goal:** a private AI that explains lecture topics and quizzes her before exams,
works offline, and never sends her notes to anyone.

---

## 1. Install

```console
$ curl -fsSL https://yourai.bdtren.com/cli/install.sh | bash
yourai: installing yourai v0.0.1 for aarch64-apple-darwin
######################################################################## 100.0%
yourai: checksum verified against https://yourai.bdtren.com/cli and GitHub
yourai: added /Users/linh/.yourai/bin to PATH in /Users/linh/.zshrc (open a new terminal, or run: export PATH="/Users/linh/.yourai/bin:$PATH")
yourai: installed to /Users/linh/.yourai/bin/yourai
yourai: next: run  yourai doctor  then  yourai init
```

She opens a new terminal tab so the `PATH` change takes effect.

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
Network was not tested. Run `yourai doctor --network` to check downloads.

Next: yourai init
```

Her Mac can run a 14B model. She decides to start tiny to try things out, and upgrade
later (step 9).

## 3. Create the project: `yourai init`

The interactive wizard asks one question at a time and marks the best choice for her
hardware as **Recommended**. To pick the smallest model in one line instead, she runs:

```console
$ yourai init --yes --name study-buddy --model qwen2.5-0.5b

Review
┌────────────────┬───────────────────────────────────────┐
│ Setting        ┆ Your choice                           │
╞════════════════╪═══════════════════════════════════════╡
│ Project folder ┆ study-buddy                           │
│ Goal           ┆ Chat assistant                        │
│ Model          ┆ Qwen2.5 0.5B Instruct (Small, 0.4 GB) │
│ Quantization   ┆ Q5_K_M                                │
│ Runtime        ┆ llama.cpp                             │
│ Context length ┆ 4096 tokens                           │
│ Interface      ┆ API + browser chat page               │
│ Extra features ┆ No extras for now                     │
│ Network access ┆ Only this computer (127.0.0.1)        │
│ Port           ┆ 8080                                  │
└────────────────┴───────────────────────────────────────┘
┌──────────────────────┬────────────────────────────────────────────────────┐
│ Estimate             ┆ Amount                                             │
╞══════════════════════╪════════════════════════════════════════════════════╡
│ Download             ┆ 420 MB model + 16 MB engine                        │
│ Memory while running ┆ about 869 MB (budget 20.8 GB)                      │
│ Disk use             ┆ about 461 MB in ~/.yourai (shared by all projects) │
└──────────────────────┴────────────────────────────────────────────────────┘
Nothing is downloaded yet. `yourai build` downloads after you confirm the model license.
✔ Created study-buddy

Next: cd study-buddy && yourai build && yourai start
```

The new folder:

```text
study-buddy/
├── yourai.toml          # all settings, with comments
├── prompts/system.md    # the instructions the AI follows
├── data/docs/           # reserved for document search (later release)
├── scripts/             # start/stop helpers
├── README.md
└── .gitignore
```

She makes it a study tutor by editing `prompts/system.md`:

```markdown
You are Linh's study buddy. Explain topics simply, with one short example.
When she says "quiz me", ask one question at a time and wait for her answer.
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
│ Project      ┆ study-buddy (/Users/linh/study-buddy)                  │
│ Model        ┆ qwen2.5-0.5b Q5_K_M                                    │
│ Built        ┆ yes, engine llama.cpp b12015 (macos-aarch64-metal)     │
│ Address      ┆ http://127.0.0.1:8080  (API: http://127.0.0.1:8080/v1) │
│ Process      ┆ pid 14631                                              │
│ Memory (RAM) ┆ 615 MB                                                 │
│ Uptime       ┆ 0h 0m 4s                                               │
│ Health       ┆ healthy                                                │
└──────────────┴────────────────────────────────────────────────────────┘
```

It runs in the background, so she can close the terminal. She also bookmarks
<http://127.0.0.1:8080> for the browser chat page.

## 6. Talk to it: `yourai chat`

```console
$ yourai chat
Chatting with study-buddy (qwen2.5-0.5b). Type /exit to quit, /reset to start over.
you › Explain photosynthesis in one sentence.
ai › Photosynthesis is the process by which plants, algae, and some bacteria turn
light energy into chemical energy stored in glucose, using carbon dioxide and water.

you › /reset
✔ conversation cleared
you › /exit
```

## 7. Use it from her own code (OpenAI-compatible API)

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

Answers feel too random for quizzes, so she lowers the temperature:

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

To paste in whole lecture summaries she needs a bigger context:

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
her editor, and `yourai config validate` checks it.

## 9. Upgrade the model

The 0.5B model is fast but sometimes gets facts wrong. `doctor` said her Mac can run
much bigger models, so she switches to the 7B one:

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

The old model stays in `~/.yourai/models`, so switching back is instant.

## 10. Free memory for a game: `yourai pause` and `yourai resume`

```console
$ yourai pause
Unloading the model…
✔ paused: the model is unloaded and its memory is free

Next: yourai resume

$ yourai resume
Loading the model (first start can take a minute)…
✔ study-buddy is running (pid 15307, loaded in 1s)
```

## 11. Something odd? `yourai logs`

```console
$ yourai logs -n 4
0.00.234.824 I cmn          init: llama threadpool init, n_threads = 8
0.00.262.097 I srv    load_model: initializing, n_slots = 4, n_ctx_slot = 8192
0.00.264.492 I srv  server_run_i: model loaded
0.00.264.496 I srv  server_on_li: listening on http://127.0.0.1:8080
```

`yourai logs -f` keeps following new lines until Ctrl+C.

## 12. Use it from her phone (local network)

She creates a second project that other devices on her Wi-Fi can reach:

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

## 13. Keep things fresh: `yourai update` and `yourai self-update`

```console
$ yourai update
Refreshing the model catalog…

Updates for this project
┌───────────────────────────┬───────────────┬───────────────┬────────────┐
│ Component                 ┆ Installed     ┆ Latest        ┆            │
╞═══════════════════════════╪═══════════════╪═══════════════╪════════════╡
│ Engine (llama.cpp)        ┆ b12015        ┆ b12015        ┆ up to date │
│ Model qwen2.5-7b Q4_K_M   ┆ bb46a2c1a0a4… ┆ bb46a2c1a0a4… ┆ up to date │
└───────────────────────────┴───────────────┴───────────────┴────────────┘

$ yourai self-update --check
Checking for a new version…
✔ yourai v0.0.1 is the latest version
```

## 14. Done for the semester: `yourai stop` and `yourai uninstall`

```console
$ yourai stop
Stopping…
✔ stopped

$ yourai uninstall
This will remove
• the yourai CLI: /Users/linh/.yourai/bin/yourai
• the PATH line added to /Users/linh/.zshrc
• Your projects are never touched.
? Remove the yourai CLI? Yes
? Also delete downloaded models and engines in /Users/linh/.yourai (5.1 GB)? They are slow to download again. No
✔ removed yourai. Open a new terminal to refresh PATH.
```

She keeps the models, so reinstalling next semester needs no downloads.

---

## Command summary

| Step | Command | What it did for Linh |
| --- | --- | --- |
| 2 | `yourai doctor` | Showed her Mac can run up to a 14B model |
| 3 | `yourai init` | Created `study-buddy/` with a reviewed plan |
| 4 | `yourai build` | Downloaded and verified the engine and model |
| 5 | `yourai start`, `status` | Ran it in the background on port 8080 |
| 6 | `yourai chat` | Chatted in the terminal |
| 7 | API at `/v1` | Used it from curl and Python |
| 8 | `yourai config`, `rebuild` | Lowered temperature, raised context |
| 9 | `config set model.id` + `rebuild` | Upgraded to the 7B model |
| 10 | `yourai pause`, `resume` | Freed memory, then reloaded |
| 11 | `yourai logs` | Looked at the server log |
| 12 | `init --access lan` | Reached it from her phone with an API key |
| 13 | `yourai update`, `self-update` | Checked for newer engine, model, and CLI |
| 14 | `yourai stop`, `uninstall` | Stopped it and removed the CLI, kept models |
