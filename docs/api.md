# API, events, and CLI

How the frontend and the `boggled` CLI talk to the server. The files they read and write are specified in [set-format.md](set-format.md). Import's request body is summarized here and specified in [import.md](import.md). Process lifetime and the `server.json` path are in [architecture.md](architecture.md).

## HTTP boundary

Prefix routes with `/api`. Send `Authorization: Bearer <token>` on every `/api`, `/events`, and `/terminal` request. CORS allows the Vite dev origin and the server's own origin.

Every set-scoped route also sends the absolute set root in `X-Boggled-Set`. The server accepts that path only when it is an existing set root (`set.json` is present) that this process has opened. Opening a set is `POST /api/sets/open` with the path the user picked. That registration is process state. The card data is not.

Intended route groups:

| Group | Role |
| --- | --- |
| `POST /api/sets/open` and `POST /api/sets/close` | Register or drop a set root for this process |
| `GET` and `PUT /api/set` | `set.json` |
| `GET /api/cards`, `GET`, `PUT`, `DELETE /api/cards/:uuid` | One file per card face |
| `POST /api/cards` | Create a face, allocate a uuid, write the file |
| Same pattern for `/api/keywords` | Keyword definitions |
| `POST /api/render` and `POST /api/render/:uuid` | Render one face or the whole set. Behavior in [renderer.md](renderer.md) |
| `POST /api/validate` | Schema and cross-reference check, no write |
| `POST /api/format` | Rewrite JSON in canonical form |
| `GET /api/stats` | Computed statistics |
| `POST /api/import` | Convert an `.mse-set` package into a new set folder. Body in [import.md](import.md) |
| `POST /api/export/mtgjson` | Write a single MTGJSON Set document |
| `GET /events` | SSE |
| `GET /terminal` | WebSocket upgrade |

`PUT` requires `If-Match` with the revision the client last read. The revision is the file's content hash. A mismatch returns 409 and does not write. Direct disk edits do not send `If-Match`; they just replace the file. The UI then reloads from the SSE event.

Unknown JSON keys are preserved on write. The server validates the fields it knows and round-trips the rest. An agent can add an official MTGJSON field this repo does not model yet without having it deleted on the next UI save.

Canonical JSON and atomic writes are defined in [set-format.md](set-format.md).

## SSE

`GET /events` is a stream for one open set. The server watches that set tree. After a short debounce it sends:

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

`terminal` uses `portable-pty`. A session has an id, a set root as its cwd, a size, and the configured command from `config.json`. The default command is the platform shell. The user sets it to their coding tool.

The WebSocket carries bytes both ways plus control messages for resize, close, and restart. A disconnected pane can reattach by session id while the PTY is still alive. More than one session may exist. The UI shows one pane.

The coding tool is an ordinary process. The app does not inject tools into it. The set harness tells the agent to use `boggled` or to edit the files.

## CLI

`boggled` is the supported command for agents. It reads `server.json`, starts `boggled serve` when nothing is listening, and calls the routes above.

| Command | Effect |
| --- | --- |
| `boggled serve` | Run the server in the foreground |
| `boggled set init <dir>` | Scaffold a set folder |
| `boggled import <file> --dest <dir>` | Convert an `.mse-set` package into a new set folder |
| `boggled set validate` | `POST /api/validate` |
| `boggled set format` | `POST /api/format` |
| `boggled set stats` | Print `GET /api/stats` |
| `boggled set export-mtgjson` | Write the assembled Set document |
| `boggled card list\|get\|create\|delete` | Card routes |
| `boggled card set-art <uuid> <image>` | Copy the image into `art/` and set `boggled.artFile` |
| `boggled render [uuid\|--all]` | Render |

Passing `--set <path>` selects the set. The CLI prints machine-readable JSON on stdout when `--json` is set.

Direct file edits remain valid. The set harness describes both. `set-art` and `render` exist so the agent does not have to guess paths or invoke the renderer by hand.

## Security

The API can read and write set folders, so it is a local file-write surface.

- Listen on `127.0.0.1` only.
- Require the per-process bearer token.
- Refuse set roots that were not opened in this process. Import destinations are the exception: the user names a new folder in the import request, and that call creates it. See [import.md](import.md).
- Resolve paths and reject anything that escapes the set root or the Full Magic Pack data directory.
- Do not put the token in a set folder, a log line, or the frontend's persisted storage. The Tauri shell and the CLI may read `server.json`. The browser frontend receives the token from the shell at load, or prompts for the local server when a person opened it by hand.

## Adding an endpoint

Add the behavior to `domain` or `store` first. Then add the route and the CLI command. Update the set harness if the agent is supposed to call it.

## Adding a frontend view

Build it against the existing routes. Subscribe to SSE for the resources it shows. Gate the titlebar on `shell=1`.
