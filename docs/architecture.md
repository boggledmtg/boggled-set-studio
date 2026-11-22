# Architecture

System shape for Boggled Set Studio. Product behavior is in the root `AGENTS.md`, which also says when to open each of the docs below.

| Doc | What it contracts |
| --- | --- |
| [set-format.md](set-format.md) | Set folder, MTGJSON files, the `boggled` object, set harness |
| [api.md](api.md) | HTTP, the file-change stream, terminal socket, local auth |
| [openapi.yaml](openapi.yaml) | Machine-readable HTTP contract |
| [cli.md](cli.md) | The `boggled` command |
| [renderer.md](renderer.md) | Native Windows renderer, reference fixtures, and frame allowlist |
| [import.md](import.md) | MSE `.mse-set` import |
| [ui.md](ui.md) | Editor layout, pastel styling, and palette |

## Topology

```
┌──────────────────────────────────────────────────────────────┐
│ Frontend (React + TypeScript + Vite)                         │
│  Card list, card form, rendered preview, set info            │
│  Terminal pane (xterm.js), right side                        │
│  Custom titlebar only when the page was opened with ?shell=1│
│  Data: fetch(). File changes: fetch() SSE. Terminal: WS.    │
└─────────────┬───────────────────────────────┬────────────────┘
              │ REST + SSE                    │ WebSocket
              │ 127.0.0.1:<port>              │
┌─────────────▼───────────────────────────────▼────────────────┐
│ Axum server (headless; Tauri is just one client)            │
│  REST for sets, cards, keywords, import, export, render     │
│  In-memory undo stack, one per set, for this process        │
│  SSE for disk changes                                       │
│  WebSocket bridged to a PTY                                 │
│  Serves the built frontend in production                    │
└─────────────┬───────────────────────────────┬────────────────┘
              │ set folders                   │ portable-pty
              │ FMP data dir                  │
┌─────────────▼──────────────┐  ┌─────────────▼────────────────┐
│ Disk                       │  │ User's coding tool           │
│  JSON, art, renders        │  │  cwd = the open set folder   │
│  app config, server token  │  │  edits files or calls CLI    │
└────────────────────────────┘  └──────────────────────────────┘
```

One server process serves every frontend. There is no server-side "current set." Each request names a set root. Two windows can edit two different sets. A browser pointed at the same origin is another frontend.

Tauri's webview loads `http://127.0.0.1:<port>/?shell=1`. A normal browser loads the same origin without that flag and gets no custom titlebar. The terminal is the same component in both.

Development: Vite on its own port, proxying `/api`, `/events`, and `/terminal` to Axum, with hot reload. Production: Axum serves the Vite build and the API on one port.

## Processes and lifetime

The server is separate from the window.

- `boggled serve` runs it headless.
- Opening the Tauri app starts it, or attaches if `server.json` already points at a live process.
- The CLI does the same attach-or-start dance before it makes a request.
- Quitting the window does not kill the server while a terminal session is open or a client still holds an SSE connection.
- When the last client and the last PTY are gone, the server exits after an idle timeout.
- A second start must attach to the existing process. Two servers must not write one set.

Runtime discovery file, rewritten on each start:

| OS | Path |
| --- | --- |
| Windows | `%APPDATA%\boggled\server.json` |
| macOS | `~/Library/Application Support/boggled/server.json` |
| Linux | `$XDG_DATA_HOME/boggled/server.json` or `~/.local/share/boggled/server.json` |

`server.json` holds `pid`, `port`, and `token`. The token is a bearer secret for that process. App preferences live in `config.json` beside it: recent set paths, the terminal command line, the Full Magic Pack directory, and whether auto-render is on. `BOGGLED_HOME` replaces the support directory. `BOGGLED_PORT` chooses the listen port. `BOGGLED_IDLE_SECS` is the idle exit (15 minutes; `0` disables it).

Bind address is `127.0.0.1` only. The rules for that token are in [api.md](api.md).

## Backend modules

Cargo workspace. Each crate owns one reason to change. HTTP handlers stay thin and call these crates.

| Crate | Responsibility |
| --- | --- |
| `domain` | MTGJSON-shaped types, the `boggled` extension, validation, ids, canonical JSON |
| `store` | Set-folder reads and atomic writes, directory scan, path checks |
| `mse` | Windows MSE integration, Full Magic Pack cache, legacy Rust renderer, render allowlist |
| `import` | An MSE `.mse-set` package in, one Boggled set folder out |
| `export` | One assembled file out. `mtgjson` is the format in this version |
| `api` | Axum routes, bearer auth, the undo stack, SSE, static files |
| `terminal` | PTY sessions and the WebSocket bridge |
| `cli` | The `boggled` binary agents call |
| `boggled-desktop` | Tauri shell: window, titlebar permissions, native folder and file dialogs, server spawn |

`domain` does not import Axum, Tauri, or the renderer. `store` does not render. `mse` and `import` do not know about HTTP. `cli` calls `api` over HTTP so a direct disk edit and a CLI edit meet in the same files, and so the CLI cannot drift onto a private mutation path.

## MSE features this architecture covers

| MSE capability | Where it lives |
| --- | --- |
| Set and card editing | Frontend editor, `store`, `domain`. Shape in [set-format.md](set-format.md) |
| Card layouts | `mse` allowlist. This version renders `magic-m15-altered.mse-style` for a normal creature. [renderer.md](renderer.md) |
| Symbol fonts and tagged rules text | `mse` layout. Fonts come from the pack's `Magic - Fonts` folder |
| Keywords and reminder text | `keywords/` and the keyword routes. The card form leaves the card's `keywords` array as stored |
| Text shrink-to-fit | `mse` layout |
| Find | The card list search field |
| Undo | Server memory, one stack per set, shared by the UI and the CLI. [api.md](api.md) |
| Card images | `renders/<uuid>.png` |
| JSON interchange | The set folder, and `POST /api/export` with format `mtgjson` |
| Import an MSE set | [import.md](import.md). `.mse-set` only, for now |
| Booster definitions | Later `packs/` files and a pack module. Not required to render cards |
| Locales | Read from the FMP locale packages when a style asks for them |
| Generic game / other CCGs | Out of scope. The game package is Magic |

## Accounts and cloud

Placeholder only. A future sign-in uses the system browser and a loopback redirect on this Axum server. Tokens go in the OS keychain, never in a set folder.

A later exchange API publishes or downloads a set. The on-disk folder, zipped with relative paths, is the interchange format. The published spec will live in `docs/` when it exists. The `domain` types should not grow account ids, remote URLs, or sync cursors inside card files. A set remains fully usable offline with no account.
