# Boggled Set Studio

A desktop editor for custom Magic: The Gathering sets. The set folder is the database. The window is a client of a local server, and the same UI loads in a browser. Agents edit a set with the `boggled` CLI or by writing the files.

This version edits normal M15-frame creatures, imports an MSE `.mse-set`, exports one MTGJSON file, renders the card beside the form, and undoes API edits made since the server started. A terminal in the window runs a shell in the set folder.

## Run the editor

Install Rust and Node.js. From this directory:

```
cargo build -p boggled -p boggled-desktop
cd web
npm install
npm run build
cd ..
cargo run -p boggled-desktop
```

The desktop app starts `boggled serve` when it is not already running, then opens the editor. The server binary has to sit next to `boggled-desktop` (`cargo build -p boggled` puts it there).

To use a browser instead, build the frontend the same way and run `cargo run -p boggled -- serve`. Open the printed `http://127.0.0.1:<port>/` URL and paste the token from the support directory's `server.json`. On Windows that file is `%APPDATA%\boggled\server.json`.

For frontend work, leave `boggled serve` running and start Vite from `web/` with `npm run dev`. Vite listens on `127.0.0.1:5173` and proxies the API. Set `window.__BOGGLED__` in the page, or type the token into the connect field. `BOGGLED_API` overrides the proxy target (default `http://127.0.0.1:4173`).

`boggled pack download` installs the pinned Full Magic Pack in the support directory, including the native MSE engine on Windows. Windows previews and exports use the same engine, and editable fields use the bounds from that render. A missing pack gives an offline preview. See [Windows rendering and fixture verification](docs/renderer.md).

## Commands and API

`boggled --help` lists the commands. The narrative is [docs/cli.md](docs/cli.md). The HTTP contract is [docs/openapi.yaml](docs/openapi.yaml), with the surrounding rules in [docs/api.md](docs/api.md).

Undo and redo are server state. Ctrl+Z in the editor and `boggled undo` move the same stack. A restart clears it.

## Tests

```
cargo test
cd web
npx playwright test
```

Playwright drives the Vite app in Chromium against a real server. It covers opening a set, editing, creating, deleting, illustration, set info, import, search, an unsupported card, export, undo and redo, and the terminal. It does not drive the Tauri window.

## License

Boggled Set Studio is distributed under the GNU General Public License, version 2 or later. See [LICENSE](LICENSE). The Windows renderer targets the MSE build documented in [docs/renderer.md](docs/renderer.md). Third-party template assets and fonts retain their own terms.
