# Privacy

`yourai` is designed so that **inference stays on your computer**. This page explains
what data is processed locally, what may leave the machine, and what we do not collect.

## Local by default

When you chat, generate images/video/speech, or call the local API:

- Prompts and completions are handled by engines on `127.0.0.1`
- Files the agent reads stay within the configured workspace on disk
- Generated media is written under the project's `outputs/` folder

YourAI (BDTren) does **not** receive your prompts, documents, or generations as part of
normal local use.

## When the CLI uses the network

| Purpose | What is sent | How to avoid / limit |
| --- | --- | --- |
| Catalog refresh | HTTP GET to the catalog URL | Offline use after a prior successful build; pin local catalog |
| Download engines / models | HTTP GET to release / Hugging Face URLs | Pre-seed `yourai_modules/` / `~/.yourai`; use `-s` cache |
| CLI update check (daily) | Version check against the release channel | `YOURAI_NO_UPDATE_CHECK=1` |
| Optional remote deploy | SSH / provider steps you initiate | Do not run `deploy` if you want zero remote ops |

Downloads are integrity-checked with SHA-256 from the catalog or release metadata.

## What is stored on disk

- Project config: `yourai.toml`, `prompts/`, `api/`, `web/`
- Runtime state and secrets: `.yourai/` (gitignored)
- Models and engines: `yourai_modules/` (gitignored) or shared `~/.yourai`
- Outputs: `outputs/` (gitignored)

Do not commit `.yourai/secrets.toml`. Prefer environment overrides (`YOURAI_API_KEY`)
in automation when appropriate.

## Logs

Server and engine logs under `.yourai/` may include operational messages. Avoid pasting
secrets into prompts if you share log files. Default product behavior aims not to log
API keys or Authorization headers.

## Third-party models and licenses

Model weights come from upstream publishers (for example Hugging Face repos listed in the
catalog). Those publishers' licenses and privacy policies apply to the weights themselves.
`yourai` shows license acceptance prompts when configured for a model.

## Website and accounts

The product website ([yourai.bdtren.com](https://yourai.bdtren.com)) and optional mobile
apps may have separate privacy policies for accounts, store purchases, or community
features. This document covers the **open-source CLI** and local project runtime.

## Questions

Privacy or data-handling questions: [support@bdtren.com](mailto:support@bdtren.com).
