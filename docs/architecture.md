# Architecture

System shape for Boggled Set Studio. Product behavior is in the root `AGENTS.md`, which also says when to open each of the docs below.

| Doc | What it contracts |
| --- | --- |
| [set-format.md](set-format.md) | Set folder, MTGJSON files, the `boggled` object, statistics, set harness |
| [api.md](api.md) | HTTP, SSE, terminal socket, CLI, local auth |
| [renderer.md](renderer.md) | Full Magic Pack interpreter and the frame allowlist |
| [import.md](import.md) | MSE `.mse-set` import |

## Topology

```
┌──────────────────────────────────────────────────────────────┐
│ Frontend (React + TypeScript + Vite)                         │
│  Card list, card form, rendered preview, keywords, stats     │
│  Terminal pane (xterm.js), right side                        │
│  Custom titlebar only when the page was opened with ?shell=1│
│  Data: fetch() and EventSource. Terminal: WebSocket.        │
└─────────────┬───────────────────────────────┬────────────────┘
              │ REST + SSE                    │ WebSocket
              │ 127.0.0.1:<port>              │
┌─────────────▼───────────────────────────────▼────────────────┐
│ Axum server (headless; Tauri is just one client)            │
│  REST for sets, cards, keywords, import, render, stats      │
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

`server.json` holds `pid`, `port`, and `token`. The token is a bearer secret for that process. App preferences live in `config.json` beside it: recent set paths, the terminal command line, the Full Magic Pack `data` path, and whether auto-render is on.

Bind address is `127.0.0.1` only. The rules for that token are in [api.md](api.md).

## Backend modules

Cargo workspace. Each crate owns one reason to change. HTTP handlers stay thin and call these crates.

| Crate | Responsibility |
| --- | --- |
| `domain` | MTGJSON-shaped types, the `boggled` extension, validation, ids, canonical JSON |
| `store` | Set-folder reads and atomic writes, directory scan, path checks |
| `mse` | Full Magic Pack index, package parser, script interpreter, text layout, image composite, render allowlist |
| `import` | An MSE `.mse-set` package in, one Boggled set folder out |
| `stats` | Aggregations computed from the card files |
| `api` | Axum routes, bearer auth, SSE, static files |
| `terminal` | PTY sessions and the WebSocket bridge |
| `cli` | The `boggled` binary agents call |
| `desktop` | Tauri shell: window, titlebar permissions, file dialogs, open-in-browser, server spawn |

`domain` does not import Axum, Tauri, or the renderer. `store` does not render. `mse` and `import` do not know about HTTP. `cli` calls `api` over HTTP so a direct disk edit and a CLI edit meet in the same files, and so the CLI cannot drift onto a private mutation path.

## MSE features this architecture covers

| MSE capability | Where it lives |
| --- | --- |
| Set and card editing | Frontend editor, `store`, `domain`. Shape in [set-format.md](set-format.md) |
| Many card layouts | `mse` allowlist. [renderer.md](renderer.md) |
| Symbol fonts and tagged rules text | `mse` layout |
| Keywords and reminder text | `keywords/` plus the editor |
| Text shrink-to-fit | `mse` layout |
| Find, replace, filter, sort | Frontend over the card list |
| Undo | Frontend history of editor edits. Disk has no undo log; git is the history for agent edits |
| Statistics | `stats`. Catalog in [set-format.md](set-format.md) |
| Image export | `renders/` plus a later sheet/PDF export |
| JSON interchange | The set folder, and `export-mtgjson` |
| Import an MSE set | [import.md](import.md). `.mse-set` only, for now |
| Booster definitions | Later `packs/` files and a pack module. Not required to render cards |
| Locales | Read from the FMP locale packages when a style asks for them |
| Generic game / other CCGs | Out of scope. The game package is Magic |

## Accounts and cloud

Placeholder only. A future sign-in uses the system browser and a loopback redirect on this Axum server. Tokens go in the OS keychain, never in a set folder.

A later exchange API publishes or downloads a set. The on-disk folder, zipped with relative paths, is the interchange format. The published spec will live in `docs/` when it exists. The `domain` types should not grow account ids, remote URLs, or sync cursors inside card files. A set remains fully usable offline with no account.
