# Import

The only import format for now is a Magic Set Editor set package (`.mse-set`). The importer reads that file once and writes a normal set folder. The `.mse-set` is not kept as a second store, and the app does not keep it open.

The folder it writes is specified in [set-format.md](set-format.md). The route and the CLI entry point are also listed in [api.md](api.md).

`POST /api/import` takes a JSON body:

| Field | Meaning |
| --- | --- |
| `source` | Absolute path of the `.mse-set` file to read |
| `dest` | Absolute path of the set folder to create |

`dest` must not already exist, must not sit inside the Full Magic Pack data directory, and is created by this call. The response lists counts and warnings. It does not hide a card that imported with a missing field.

`boggled import <file> --dest <dir>` calls that route. After a successful import the user opens the new folder like any other set. Re-rendering with Full Magic Pack is a separate `boggled render`. Import does not invoke the renderer.

## MSE set package

An `.mse-set` file is a zip. The entry named `set` is MSE's text data file for the set. Other entries are images and auxiliary files the set references. Detect a package by the `.mse-set` extension or by a zip that contains a `set` entry. Anything else is an error.

The `set` entry is MSE's indented package language, not JSON. It carries the game id, the default stylesheet, a `set info` block, one `card` block per face, keyword blocks, and per-card `stylesheet` and `styling` data.

The importer maps that onto the set folder:

| MSE set | Set folder |
| --- | --- |
| `set info` name, set code, and other set-info fields that match MTGJSON | `set.json` |
| Each `card` block | One file under `cards/` or `tokens/` |
| Card fields that match the MTGJSON face schema | Those keys on the face file |
| Card `stylesheet` | `boggled.stylesheet` |
| Card `styling` | `boggled.styling` |
| Card fields with no MTGJSON key | `boggled.mseFields`, under the MSE field name |
| Keyword blocks | `keywords/<id>.json` |
| Image entries referenced as illustration crops | `art/<uuid>.<ext>` and `boggled.artFile` |

A card the Magic game marks as a token goes under `tokens/`. Every other face goes under `cards/`. A missing uuid gets a new uuid, and that uuid is the filename. Collector numbers come from the card's number field when it has one, and otherwise from document order starting at 1 after tokens are separated.

Illustration images inside the package are crops, not finished frames. They go to `art/` only. Import does not write `renders/` and does not set `boggled.renderFile`. The next `boggled render` paints the frame.

Every imported face records where it came from:

```json
"boggled": {
  "schemaVersion": 1,
  "import": {
    "format": "mse-set",
    "sourceName": "Brass Owl",
    "warnings": []
  }
}
```

The importer accepts MSE's underscore field names and the set's default stylesheet. The native short name `m15-altered` maps to `magic-m15-altered.mse-style`. Scalar set-info fields outside the standard mapping go under `boggled.mseFields`. Set-wide style options go under `boggled.styling`. For automatically numbered cards the import metadata records the initial BSS number, so the Windows engine can reproduce MSE numbering until the user overrides it.

| Format id | Detect | Notes |
| --- | --- | --- |
| `mse-set` | `.mse-set` zip containing a `set` entry | Stylesheet, styling, keywords, and embedded art |

## Adding a format later

Another editor is a new type that implements detect and convert, plus a fixture checked into the repo and a row in the table above. The converter's only output is the set folder schema in [set-format.md](set-format.md). Do not add a storage format for that editor. No other format is in scope until then.
