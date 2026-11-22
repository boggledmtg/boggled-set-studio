# Boggled Set Studio

Boggled Set Studio is a desktop app for designing custom Magic: The Gathering sets. It is an AI-first Magic Set Editor: a visual set editor, a Full Magic Pack card renderer, and a terminal where the user runs their own coding agent inside the set folder.

The app targets Windows, macOS, and Linux. It does not target mobile.

## How a session works

The user keeps each set in its own folder anywhere on disk. That folder is the source of truth and is meant to become a git repo. The visual editor reads and writes the files there. A terminal pane on the right starts the user's coding tool (Codex, Claude, Grok, or anything else they configure) with its working directory set to that folder.

The agent builds a set from a prompt such as "steampunk commander set, artifacts matter, tribal." The set folder's own harness tells the agent how to fill lore and art direction, write cards, generate images with whatever tool the user has, and place those images where the renderer can find them. The app does not call an LLM and does not generate images itself.

When a file in the open set is saved, the UI updates. The backend also runs headless, with no window, so an agent can keep working while the editor is closed.

## What the user sees

The editor follows Magic Set Editor's cards workspace: a card list, a card form, a rendered card, set info, and undo. Keyword definitions live in the set folder. The extra surface is the terminal pane. Layout, pastel styling, and the palette are in [docs/ui.md](docs/ui.md).

The same React frontend loads in the Tauri window and in a normal browser. The custom titlebar is the only intentional visual difference. It is shown only when Tauri opened the page.

Card frames come from Full Magic Pack templates. The user supplies the illustration, or the agent writes an image into the set after generating it externally (ComfyUI, a cloud image tool, Ollama, or any other tool the agent can drive).

## Where the code lives

Rust backend, React frontend, Tauri shell.

- The frontend talks to the backend with HTTP and one Server-Sent Events stream. The terminal is the only WebSocket.
- Tauri is the window. It does not own set data. Window drag, minimize, maximize, and close are the Tauri calls. File dialogs and "open this URL in the system browser" are Tauri calls. Everything about a set goes through the HTTP API or the files on disk.
- The backend is a set of modules behind an Axum server. The CLI and the UI share that server. If the server is not running, the CLI starts it.
- Agents may also edit the JSON and images directly. Those writes are valid. The API is not a gate.

## Docs

Read this file for how the product behaves. Open a doc below when the task touches that area. Do not load the others "just in case."

| Doc | Read it when |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | You need the map of the running system: Tauri versus the Axum server, the crate split, process lifetime, or which MSE features live where. |
| [docs/set-format.md](docs/set-format.md) | You are changing card or set JSON, the set folder layout, validation, statistics, or the harness that `boggled set init` writes. |
| [docs/api.md](docs/api.md) | You are adding or changing an HTTP route, the file-change stream, the terminal socket, undo, or the local bearer token. |
| [docs/openapi.yaml](docs/openapi.yaml) | You are changing the HTTP contract. |
| [docs/cli.md](docs/cli.md) | You are changing the `boggled` command. |
| [docs/renderer.md](docs/renderer.md) | You are changing card rendering, Full Magic Pack loading, the script interpreter, or the frame allowlist. |
| [docs/import.md](docs/import.md) | You are changing MSE `.mse-set` import, or adding another import format later. |
| [docs/ui.md](docs/ui.md) | You are changing the editor layout, color, or visual styling. |

## Rules that keep the design intact

- Treat the set folder as the database. Do not add a second source of truth.
- Put studio-only data under the `boggled` object. Leave MTGJSON fields recognizable.
- Keep one card face per file. Do not inline every card into `set.json`.
- On Windows, use the native MSE engine with the pinned Full Magic Pack. Keep pack assets and fonts in the local runtime installation.
- Add newly supported frames to the renderer allowlist on purpose. A template that appears in a future Full Magic Pack download is unsupported until this repo grows a test for it.
- Build editor geometry from the same native layout used for the card image. Distribute BSS under GPL-2.0-or-later and supply corresponding source for distributed native worker builds.
- Keep the server bound to `127.0.0.1` and require its bearer token on HTTP, SSE, and the terminal socket.
- Leave account and cloud upload as placeholders. Set files stay usable with no account.

## Product scope

In scope: MSE-style set editing, keyword files, the M15 creature frame the allowlist names, card renders, an agent CLI, server-side undo, a headless server, a per-set harness, and import from an MSE `.mse-set` package. That import writes a normal set folder and then stops. The `.mse-set` is not kept as a second database.

Out of scope for the architecture: mobile, an in-app chat model, an in-app image-generation provider, a generic multi-game engine, and compatibility with every future Full Magic Pack template without a code change.
