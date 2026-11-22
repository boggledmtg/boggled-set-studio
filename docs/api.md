# API, events, and CLI

How the frontend and the `boggled` CLI talk to the server. The machine-readable contract is [openapi.yaml](openapi.yaml). Command narrative is [cli.md](cli.md). The files they read and write are specified in [set-format.md](set-format.md). Import's request body is summarized here and specified in [import.md](import.md). Process lifetime and the `server.json` path are in [architecture.md](architecture.md).

## HTTP boundary

Prefix routes with `/api`. Send `Authorization: Bearer <token>` on every `/api` and `/events` request. `GET /terminal` upgrades without that header; the first WebSocket text frame carries the token. CORS allows any origin and exposes `ETag` and `X-Revision`. Static files from the built editor do not require the token.

Every set-scoped route also sends the absolute set root in `X-Boggled-Set`. The server accepts that path when it is a set root this process has opened. Opening a set is `POST /api/sets/open` with the path the user picked. That registration is process state. The card data is not. History routes also accept a path that is closed, when this process still has a stack for it.

Intended route groups:

| Group | Role |
| --- | --- |
| `GET /api/health` | Liveness. Requires the bearer token |
| `POST /api/sets/init`, `POST /api/sets/open`, `POST /api/sets/close`, `GET /api/sets` | Create, register, drop, or list set roots for this process |
| `GET` and `PUT /api/set` | `set.json` |
| `GET /api/cards`, `POST /api/cards`, `GET`, `PUT`, `DELETE /api/cards/{uuid}` | One file per card face |
| `POST /api/cards/{uuid}/art`, `GET /api/cards/{uuid}/picture` | Copy an illustration, or return the rendered PNG |
| Same pattern for `/api/keywords` | Keyword definitions |
| `POST /api/render` and `POST /api/render/{uuid}` | Render one face or the whole set. Behavior in [renderer.md](renderer.md) |
| `POST /api/validate` | Schema and cross-reference check, no write |
| `POST /api/import` | Convert an `.mse-set` package into a new set folder. Body in [import.md](import.md) |
| `POST /api/export` and `GET /api/export/formats` | Write one assembled file. `mtgjson` is the only format |
| `GET /api/history`, `POST /api/history/undo`, `POST /api/history/redo` | In-memory undo stack for this process |
| `GET /api/pack` and `POST /api/pack/download` | The pinned Full Magic Pack |
| `GET /events` | File-change stream, read with `fetch` |
| `GET /terminal` | WebSocket upgrade |

`PUT` requires `If-Match` with the revision the client last read. The revision is the SHA-256 of the file bytes. The response sends it as a quoted `ETag` and as a raw `X-Revision`. A missing `If-Match` is 428. A mismatch returns 409 and does not write. Direct disk edits do not send `If-Match`; they just replace the file. The UI then reloads from the file-change event.

`POST /api/cards` accepts only a normal creature after derive. `PUT` of a face that would stop being one is rejected. `DELETE` removes any face. Art may be set on any face. The picture route returns 400 `unsupported frame` for the rest.

Create ignores a client-supplied uuid. Writes merge onto the existing document, merge `boggled` one level, and keep unknown keys. The editor's card save sends name, mana cost, type line, rules, flavor, power, toughness, rarity, collector number, artist, and `boggled.designNotes`. The set save sends name, code, and `boggled.designBrief`.

With the Windows runtime installed, the picture response includes `X-Boggled-Layout` and `X-Revision`. The layout header is JSON with `name`, `casting_cost`, `type`, `text`, and `pt` arrays of [left, top, width, height] in 375 × 523 logical units. `flavorTop` is relative to the text box. The server obtains the bounds from the render that produced the PNG in the response.

## Undo

One stack per set, in process memory, from launch until exit. Closing the editor window does not clear it. Restarting the server does. Two windows on the same set share the stack. History is keyed by the normalized path, so it survives close, and an undone import can be redone after the folder is gone.

Every successful API mutation from the UI or the CLI is one step: create, update, or delete a card; set an illustration; edit set info; export; import; create, edit, or delete a keyword. `POST /api/sets/init` is not a step. Render is not a step. A direct disk edit is not a step.

The history body is `{ "undo": string[], "redo": string[] }`, newest first. Labels are `Create {name}`, `Edit {name}` (the name after the save), `Delete {name}`, `Set illustration`, `Edit set`, `Export {format}`, `Import set`, `Create keyword {name}`, `Edit keyword {name}`, and `Delete keyword {id}`.

Undo writes the previous bytes only when every touched file still matches the recorded result. Otherwise the response is 409 `the files changed outside this history`, the entry stays, and redo is cleared. Redo checks the current bytes against the before-snapshot. A failed redo leaves that entry in place. An empty stack is 400 `nothing to undo` or `nothing to redo`.

Export restores the previous output bytes, or deletes the file when export created it. Import deletes the created folder only when the tree still matches the manifest and no extra file is present. Illustration undo restores the image bytes and the card JSON.

Render PNGs are not in the snapshot. The card write sets `boggled.renderFile` before the after-snapshot, then writes the PNG. After undo or redo, the server renders the restored creature again. When the card was deleted, it deletes `renders/<uuid>.png` as cleanup.

The watcher still emits events for these writes. Those events are not new undo steps.

Unknown JSON keys are preserved on write. The server validates the fields it knows and round-trips the rest. An agent can add an official MTGJSON field this repo does not model yet without having it deleted on the next UI save.

Canonical JSON and atomic writes are defined in [set-format.md](set-format.md).

## SSE

`GET /events` is a stream for one open set. The browser cannot set `Authorization` on `EventSource`, so the client reads it with `fetch`. The server watches that set tree. After a short debounce (about 200 ms) it sends:

| Event | When |
| --- | --- |
| `set` | `set.json` changed |
| `card` | a file under `cards/` or `tokens/` changed or was removed |
| `keyword` | a file under `keywords/` changed or was removed |
| `art` | a file under `art/` changed |
| `render` | a file under `renders/` changed |
| `rescan` | the watcher overflowed or the change is ambiguous |

The payload includes relative paths. The UI refetches those resources. It does not apply a patch from the event. Echoing the UI's own saves is fine; the refetch is idempotent.

This stream is independent of the terminal socket. Terminal output is not a file-change bus.

## Terminal

`terminal` uses `portable-pty`. A session has an id, a set root as its cwd, a size, and the configured command from `config.json`. The default command is the platform shell (`cmd.exe` on Windows, with no arguments). The user sets it to their coding tool.

The first text frame is `{ "type": "auth", "token", "set", "cols", "rows", "session"? }`. The server answers `{ "type": "started", "session" }` or `{ "type": "error", "error" }`. Output is binary. Input from the editor is binary. Resize is `{ "type": "resize", "cols", "rows" }` and close is `{ "type": "close" }`. A disconnected pane can reattach by session id while the PTY is still alive. The server replays up to 64 KB of buffered output. More than one session may exist. The UI shows one pane.

The coding tool is an ordinary process. The app does not inject tools into it. The set harness tells the agent to use `boggled` or to edit the files.

## CLI

`boggled` is the supported command for agents. The full narrative, including undo, is in [cli.md](cli.md). `--help` lists every flag.

| Command | Effect |
| --- | --- |
| `boggled serve` | Run the server in the foreground |
| `boggled set init --path <dir>` | Scaffold a set folder that does not exist yet |
| `boggled set validate` | `POST /api/validate` |
| `boggled set update` | Change the set name, code, or design brief |
| `boggled import <file> <dest>` | Convert an `.mse-set` package into a new set folder |
| `boggled export` | Write the assembled Set document (`--format mtgjson`, optional `--out`) |
| `boggled card list\|get\|create\|update\|delete` | Card routes |
| `boggled card set-art <uuid> --path <image>` | Copy the image into `art/` and set `boggled.artFile` |
| `boggled render [uuid]` | Render one face, or the whole set when the uuid is omitted |
| `boggled undo`, `redo`, `history` | The in-memory stack for `--set` |
| `boggled pack download` | Download the pinned Full Magic Pack |

Passing `--set <path>` selects the set. The CLI prints machine-readable JSON on stdout when `--json` is set.

Direct file edits remain valid. The set harness describes both. `set-art` and `render` exist so the agent does not have to guess paths or invoke the renderer by hand. A direct edit is not an undo step.

## Security

The API can read and write set folders, so it is a local file-write surface.

- Listen on `127.0.0.1` only.
- Require the per-process bearer token.
- Refuse set roots that were not opened in this process. Import destinations are the exception: the user names a new folder in the import request, and that call creates it. See [import.md](import.md).
- Resolve paths and reject anything that escapes the set root or the Full Magic Pack data directory.
- Do not put the token in a set folder, a log line, the query string, or the frontend's persisted storage. The Tauri shell injects `window.__BOGGLED__` before the page scripts run. The CLI reads `server.json`. A browser opened by hand prompts for the token.

## Adding an endpoint

Add the behavior to `domain` or `store` first. Then add the route and the CLI command. Update the set harness if the agent is supposed to call it.

## Adding a frontend view

Build it against the existing routes. Subscribe to SSE for the resources it shows. Gate the titlebar on `shell=1`.
