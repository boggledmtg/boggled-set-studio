# Set folder and schema

On-disk contract for a set. The folder is the database. Import, the API, and the renderer all write this shape. See [architecture.md](architecture.md) for how the process is split, [api.md](api.md) for the routes that read and write it, and [import.md](import.md) for how foreign files become it.

## Set folder

Initialized by `boggled set init`. The user can put the folder anywhere.

```
<set-root>/
  AGENTS.md            harness for the coding agent working on this set
  README.md
  .gitignore
  .gitattributes       Git LFS for art/ and renders/
  docs/
    design-brief.md    the user's prompt and ongoing direction
    schema.md          card and set fields, copied from the app docs
  set.json
  cards/<uuid>.json    one MTGJSON card face per file
  tokens/<uuid>.json   token faces, same schema
  keywords/<id>.json
  art/<uuid>.<ext>     illustration crops
  renders/<uuid>.png   rendered card, including frame and text
  symbols/             set symbol and watermark art, when custom
```

`set.json` does not embed the card list. A consumer that wants a stock MTGJSON file uses the export, which inlines `cards` and `tokens` and can strip `boggled`.

`.gitignore` ignores OS junk and `.boggled/` (local lock and scratch). It does not ignore `art/` or `renders/`. Those are part of the set the user asked to commit. `.gitattributes` marks both trees for Git LFS so a few hundred print-resolution PNGs do not dominate normal diffs. Export sheets and PDFs go to `export/`, which is gitignored.

Filenames for cards and tokens are the MTGJSON `uuid`. Collector order is the `number` field, not the filename. Cross-face links use `otherFaceIds`. A transform, modal DFC, or adventure is two or more face files.

Atomicity stops at a single file. A multi-file change can be observed mid-write. Agents should write a face completely and rename into place, which the CLI already does. The UI tolerates a torn read by keeping the last good document and waiting for the next event.

Canonical JSON, used by UI saves, the CLI, and `boggled format`: UTF-8, two-space indent, trailing newline, keys in schema order, and `boggled` last. Reads accept any equivalent JSON. The server does not rewrite a file just because it was read.

Writes are create-temp-in-the-same-directory, fsync, then rename.

## Schema

Card files are [MTGJSON Card (Set)](https://mtgjson.com/data-models/card/card-set/) objects. Token files use that same face schema, because a custom token still has rules text, a frame, and usually a power and toughness. The MTGJSON [Card (Token)](https://mtgjson.com/data-models/card/card-token/) model is a narrower export shape. `boggled set export-mtgjson` projects `tokens/` into that model. Set metadata is an [MTGJSON Set](https://mtgjson.com/data-models/set/) object without `cards`, `tokens`, or `booster`.

Required on a face the editor created: `uuid`, `name`, `layout`, `setCode`, `number`, `rarity`, `types`, `subtypes`, `supertypes`, `type`, `manaCost`, `manaValue`, `colors`, `colorIdentity`, `text`, `side`. Combat and planeswalker faces also carry `power`, `toughness`, or `loyalty` when those boxes exist. `otherFaceIds` links the rest of a multi-face card. `artist`, `flavorText`, `keywords`, and `identifiers` are present and may be empty.

Fields this app does not understand yet are still stored. Validation warns on a missing required field and on a `setCode` that disagrees with `set.json`. It does not fail a file for having extra MTGJSON keys.

Studio data sits in `boggled` so a strict export can drop one object and leave an MTGJSON document.

```json
{
  "uuid": "…",
  "name": "Brass Owl",
  "layout": "normal",
  "setCode": "STM",
  "number": "12",
  "manaCost": "{2}{U}",
  "manaValue": 3,
  "types": ["Artifact", "Creature"],
  "subtypes": ["Bird"],
  "supertypes": [],
  "type": "Artifact Creature — Bird",
  "text": "Flying",
  "power": "1",
  "toughness": "3",
  "rarity": "common",
  "artist": "",
  "flavorText": "",
  "colorIdentity": ["U"],
  "colors": ["U"],
  "side": "a",
  "keywords": ["Flying"],
  "boggled": {
    "schemaVersion": 1,
    "lore": "",
    "artDirection": "",
    "artPrompt": "",
    "designNotes": "",
    "artFile": "art/<uuid>.png",
    "renderFile": "renders/<uuid>.png",
    "stylesheet": "magic-m15-mainframe.mse-style",
    "styling": {},
    "mseFields": {}
  }
}
```

`artFile` and `renderFile` are relative to the set root. `stylesheet` names a package in the installed Full Magic Pack. `styling` is the MSE styling map for options the template exposes (frame toggles, text-box variants). `mseFields` holds template inputs that have no MTGJSON field. The renderer builds the MSE `card` value from the MTGJSON fields first, then overlays `mseFields`. How that render runs is in [renderer.md](renderer.md).

`set.json` uses the same pattern. MTGJSON set fields in use: `name`, `code`, `releaseDate`, `type`, `baseSetSize`, `totalSetSize`, `keyruneCode`, `languages`, `translations`, and `block` when the user sets one. Studio fields:

```json
"boggled": {
  "schemaVersion": 1,
  "designBrief": "",
  "lore": "",
  "artDirection": "",
  "themes": [],
  "mechanicalThemes": [],
  "notes": ""
}
```

`themes` and `mechanicalThemes` are how a one-line prompt becomes something the agent and the stats view can check against ("steampunk", "artifacts matter", "tribal").

Keyword files are not an MTGJSON type. MSE-style keyword definitions live here so reminder text can be generated and so the agent can invent a mechanic once.

```json
{
  "id": "steambind",
  "name": "Steambind",
  "reminder": "…",
  "rules": "…",
  "boggled": { "schemaVersion": 1, "lore": "", "designNotes": "" }
}
```

`schemaVersion` lives inside `boggled`. A migration walks every file in the set and bumps that number. It does not rewrite unrelated keys.

An imported face may also carry `boggled.import`. That object is defined in [import.md](import.md).

## Statistics

Stats are derived. Nothing under the set folder is a stats cache.

The v1 catalog:

- Mana curve, overall and split by type line
- Color and color-identity counts
- Type, supertype, and subtype counts
- Rarity counts against the set's target distribution, when `set.json` declares one in `boggled`
- Keyword frequency, joined to `keywords/`
- Layout counts
- Theme coverage: how many cards' `boggled.designNotes`, type line, or text mention each entry in `mechanicalThemes`

As-fan and booster simulation land with the pack module. MSE's scripted statistics dimensions are a later consumer of the same interpreter, not a second stats engine.

## Set harness

`boggled set init` writes a separate `AGENTS.md` into the set. That file is for the coding agent designing cards. The root `AGENTS.md` in this repository is for agents changing the app. They are not the same document.

The set harness tells the agent to:

1. Read `docs/design-brief.md`, `set.json`, and `docs/schema.md`.
2. Record lore, art direction, and themes on the set before creating cards.
3. Write one face per file, preserving unknown keys and the `boggled` object.
4. Put illustrations in `art/` and point `boggled.artFile` at them, preferably with `boggled card set-art`.
5. Generate those illustrations with the user's tools. ComfyUI, cloud image APIs, and Ollama are all outside this app. The app only requires that a file lands on disk.
6. Run `boggled render` after a batch of writes when the editor is closed. When the server is running, saving the file is enough.
7. Run `boggled set validate` before considering a batch done.
8. Keep rendered PNGs in `renders/` and let Git LFS track them.

The one-prompt goal is this harness plus the user's coding agent. There is no backend job that expands a prompt into a set.

## Adding a card field

If MTGJSON already has the field, use that key. If it is studio data, add it under `boggled` and to `docs/schema.md` in the set scaffold. Bump `schemaVersion` only when old files must be rewritten.
