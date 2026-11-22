# CLI

`boggled` is the command agents use. It reads `server.json`, starts `boggled serve` when nothing is listening, and calls the HTTP API. The routes are in [api.md](api.md) and [openapi.yaml](openapi.yaml). The files it writes are in [set-format.md](set-format.md).

`--help` on the binary and on each subcommand lists the flags. This page is the narrative.

## Global flags

| Flag | Effect |
| --- | --- |
| `--json` | Print the server's JSON, pretty-printed. Without it, the CLI prints a short summary. |
| `--set <path>` | Open this set folder and send it as `X-Boggled-Set`. Required for every command that edits one set. |

Both flags are global. `boggled --json --set <path> card list` and `boggled card list --json --set <path>` are the same command.

Errors go to stderr and the process exits 1. The server's `{ "error": "..." }` message is the stderr text.

## Server

`boggled serve` runs the API in the foreground and prints `boggled listening on http://127.0.0.1:<port>/`.

Every other command attaches to the server named in `server.json` when that port accepts a connection and `/api/health` succeeds. Otherwise it starts `boggled serve` as a child and waits until `server.json` is live.

The support directory holds `server.json` and `config.json`:

| OS | Directory |
| --- | --- |
| Windows | `%APPDATA%\boggled` |
| macOS | `~/Library/Application Support/boggled` |
| Linux | `$XDG_DATA_HOME/boggled` or `~/.local/share/boggled` |

`BOGGLED_HOME` replaces that directory. `BOGGLED_PORT` sets the listen port (`0` picks one). `BOGGLED_IDLE_SECS` is the idle exit, 15 minutes by default. `0` keeps the process up. The idle check exits only when no SSE client and no terminal session are open.

`config.json` stores recent set paths (twelve), the terminal command, the Full Magic Pack directory, and whether auto-render is on.

## Set

```
boggled set init --path <dir> [--name <name>] [--code <code>]
boggled --set <dir> set validate
boggled --set <dir> set update [--name <name>] [--code <code>] [--brief <text>]
```

`init` creates a folder that does not exist yet, writes the harness and `set.json`, and opens it. The default name is `Untitled Set` and the default code is `SET`. Init is not an undo step.

`validate` prints the warning list. It does not write.

`update` changes the set name, the set code, or `boggled.designBrief`. Omitted flags stay as they are. This is one undo step labeled `Edit set`.

## Cards

```
boggled --set <dir> card list
boggled --set <dir> card get <uuid>
boggled --set <dir> card create [--name <name>] [--cost <cost>] [--type <type>] ...
boggled --set <dir> card update <uuid> [--name <name>] ...
boggled --set <dir> card delete <uuid>
boggled --set <dir> card set-art <uuid> --path <image>
```

`list` prints collector number, name, and type line, one card per line. `--json` prints the summary objects from `GET /api/cards`, including faces the editor cannot render.

`create` makes a normal creature. Omitted fields use the server defaults (`New Creature`, type line `Creature`, power and toughness `1`, rarity `common`). `--cost` is the mana cost. The other field flags are `--type`, `--text`, `--flavor`, `--power`, `--toughness`, `--rarity`, `--number`, and `--artist`. A non-creature is rejected.

`update` sends only the flags you pass. The server merges them and derives mana value, colors, and the type parts. The undo label uses the name after the save, so a rename to Copper Owl is `Edit Copper Owl`.

`delete` removes any face, including one the editor cannot render.

`set-art` copies a png, jpg, webp, or gif into `art/` and sets `boggled.artFile`. The undo step restores the previous image and the card JSON.

Saving a supported creature renders it when auto-render is on. The PNG is not its own undo step.

## Import and export

```
boggled import <file.mse-set> <dest>
boggled --set <dir> export [--format mtgjson] [--out <file>]
```

`import` converts one `.mse-set` package into a new folder and opens it. The summary prints `Opened <path>`. Details are in [import.md](import.md). The whole import is one undo step labeled `Import set`.

`export` writes one assembled file. `mtgjson` is the only format. The default destination is `export/<code>.json` inside the set. The folder remains the database. Undo restores the previous file, or deletes it when export created it. The summary prints `Exported <path>`.

## Render

```
boggled --set <dir> render
boggled --set <dir> render <uuid>
```

The first paints every face. The second paints one. Supported creatures land in `renders/<uuid>.png`. Other faces are reported in `failed` and the rest still render. Render is not an undo step. After an undo or redo of a card, the server renders the restored creature itself.

## Undo

```
boggled --set <dir> history
boggled --set <dir> undo
boggled --set <dir> redo
```

One stack per set, in the server process, from launch until exit. The UI and the CLI share it. A restart clears it. Reloading the editor does not.

`history` lists undo labels, then redo labels, newest first. `undo` and `redo` print the same shape after the step.

A direct edit of a file that an undo step would restore makes that undo return 409 (`the files changed outside this history`). The entry stays. Redo is cleared. When the bytes match again, undo works. Redo of a step whose files no longer match the recorded before-state returns 409 and leaves the redo entry.

`history`, `undo`, and `redo` still accept `X-Boggled-Set` after the set is closed, when this process has a stack for that path. That is how redo brings an undone import back.

## Pack

```
boggled pack download
```

Downloads the pinned Full Magic Pack commit into the support directory. The runtime includes `data/`, `Magic - Fonts/`, and, on Windows, `magicseteditor.exe`. Run this command again to upgrade an older data-only Windows cache. With the pack absent the editor can still show an offline preview.

## What the CLI does not do

The set folder is still a valid place to edit JSON and images by hand. Those writes show up in the editor through the file stream. They are not undo steps.

There is no `set stats` command and no format-in-place command. Writes are already canonical.
