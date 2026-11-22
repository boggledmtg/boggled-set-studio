# Windows MSE renderer

BSS targets the Windows MSE 2.6.3 (Unofficial) executable distributed with [Full Magic Pack revision cec04598](https://github.com/MagicSetEditorPacks/Full-Magic-Pack/tree/cec045982f40870712c0d8f3f450d30a036d87a8). The first supported frame is a normal creature in `magic-m15-altered.mse-style`.

The reference install in the sibling `bss-mse-reference` directory contains the original eleven-card `fixtures.mse-set` and its Windows exports in `reference-pngs/`. The full card is 750 × 1046 pixels at 200% zoom. Collector numbering can depend on other cards in the set.

On Windows, `boggled pack download` installs the pinned engine beside its data and fonts. Repeat the download for a cache created by an older BSS version. The bridge verifies the executable's SHA-256 before launching it. A cache miss starts a hidden MSE process on a temporary set assembled from the current JSON and art. MSE renders the requested face and returns its evaluated name, cost, type, combined text, and power/toughness rectangles through its raw command interface. The server caches the last image and layout together. An edit to set data, a face, or its illustration invalidates that result.

React positions editable controls in MSE's 375 × 523 logical coordinates. Rules and flavor share MSE's text field. The first flavor paragraph supplies the split when present. Power and toughness share the two halves of the native PT field. Browser controls handle keyboard input, selection, caret placement, and type suggestions. The clean preview and exported PNG both come from MSE.

To verify a Windows installation, run `cargo test -p import --test windows_reference -- --ignored`. Set `BSS_MSE_REFERENCE` if the reference installation is elsewhere. The test imports the original set, renders all eleven faces from the resulting JSON, and compares every decoded RGBA pixel to its reference. With the same environment variable set, `npm --prefix web run test:e2e` also checks field placement and editing against the native HTTP response.

BSS is licensed under GPL-2.0-or-later. Distributing a modified MSE worker requires preserving the upstream notices and providing the corresponding source and build scripts under the applicable GPL terms. Full Magic Pack templates, fonts, frame artwork, and symbols have their own provenance and rights. Obtain those separately for local use. See [LICENSE](../LICENSE).

## Legacy Rust renderer

The following describes the existing Rust path and its older perceptual fixtures.

Full Magic Pack is a public collection of Magic Set Editor templates: https://github.com/MagicSetEditorPacks/Full-Magic-Pack. The allowlisted style is `magic-m15-altered.mse-style`. The package file names a game (`magic.mse-game`), shared includes, and symbol fonts, and it computes layout with MSE's expression language (`if … then … else`, field access, `include file`, string and image functions).

The Rust painter reads the pinned pack's data and font files directly on other platforms. With no runtime installed, the editor has a low-resolution offline preview.

When the pack or a face is missing, text falls back to a built-in bitmap font and the card is drawn at 375×523 from the default creature geometry. The editor and the tests work without the pack. A missing pack reports `Run boggled pack download`.

The pack has no LICENSE file. Being on GitHub does not grant a license to vendor it. Frame art and symbol images in the pack, including the pieces attributed to CardConjurer, are fan reproductions of Magic's card trade dress. Boggled treats them as a local runtime asset, does not commit them, and does not claim Wizards of the Coast's art, symbols, or trademarks. The app's own UI should stay obviously unofficial.

## What a normal creature paints

For `magic-m15-altered.mse-style` and a normal creature, the painter:

1. Reads the style's else-branch boxes. The art window is 29,59 316×231. The text box runs from `top_of_textbox()` at 327 to `bottom_of_textbox()` at 481.
2. Scales the frame JPEG `magic-modules.mse-include/cards/375 m15 simple/{letter}card.jpg`. The letter is w, u, b, r, g, c, a, or m.
3. Draws the illustration at `boggled.artFile` into that window. With no illustration, it stretches the pack placeholder from `magic-default-image.mse-include`. Artifact uses `artifact.jpg`. Multicolor uses `multicolor.jpg`.
4. Paints the power/toughness badge `{letter}pt.png` at 285,464, 70×39, then the power/toughness text.
5. Draws the name and type line in Beleren Bold and the rules in MPlantin. A parenthetical reminder and the flavor text use MPlantin italic. Generic and colored mana symbols, including an inline `{1}`, come from `magic-mana-large`.
6. Writes `renders/<uuid>.png` at 375×523.

The painter does not draw the rarity set symbol, the collector line, or the rare stamp, and it does not run `card_background()`. Hybrid and Phyrexian symbols fall back to a colored square. With no pack on disk, the same geometry is a flat frame, a beige art rect, and the bitmap font.

Auto-render is on by default for a set this server has open. A saved card, token, or art file is rendered after the watcher debounce, including in headless mode. `boggled render` does the same job when the user wants it immediately. The render queue is per set, so two clients cannot composite the same uuid at once.

Import does not invoke the renderer. An `.mse-set` import copies illustration crops into `art/` and leaves `renders/` empty until something calls render. See [import.md](import.md).

## Creature fixtures

`crates/mse/fixtures/creatures/` holds eleven normal-creature PNGs: one each of white, blue, black, red, green, colorless, artifact, and multicolor, plus a short-rules card, a long-rules card with an inline generic symbol, and a card with flavor text. Each card has no illustration, so both sides use the pack placeholder.

Those PNGs were exported once with Magic Set Editor 2.5.8 from `Full-Magic-Pack-main`. That binary cannot render the pinned 2.6.0 style: an export against the pinned data is a starfield with no M15 frame. The frame JPEGs, placeholder JPEGs, mana PNGs, power/toughness badges, and Beleren and MPlantin fonts in that pack match the pinned pack byte for byte. The test paints with the pinned pack at `%APPDATA%\boggled\packs\cec045982f40870712c0d8f3f450d30a036d87a8`.

`creature_renders_stay_near_the_mse_fixtures` compares `average_hash`: grayscale, shrink to 8×8, one bit per pixel brighter than the average. The allowed Hamming distance is 6. The measured pairs were 0 or 1 bit apart. A white card against the black fixture is 29 bits and must fail. The test does not launch Magic Set Editor and does not download the pack. A missing pinned pack fails the test.

The 8×8 hash does not see a wrong word when the gray shape of the text block is the same. A spot check of the pairs showed the frame, art window, mana symbols, type line, rules, flavor, and power/toughness badge agree. Line breaks in a long text box can differ. The set symbol and collector line are absent on the Boggled side.

## Allowlist and the script subset

Supporting a frame means all three of these are true:

- The stylesheet and the packages it includes parse.
- Every script function and operator that stylesheet uses for its boxes is implemented.
- A fixture in this repo renders that frame and compares an average hash of the PNG.

The allowlist in this version is one stylesheet: `magic-m15-altered.mse-style`, for layout `normal` whose types include Creature. That covers white, blue, black, red, green, multicolor, colorless, and artifact frames. A legendary creature uses the same frame. Mana costs in the fixture use generic and colored symbols.

Any other stylesheet fails with `missing package: <name>`. A script function the interpreter does not have fails with `missing function: <name>`. A face that is not a normal creature fails with `unsupported frame`. Import still stores those faces, the grid still lists them, and render continues with the faces it can paint. The preview says the frame is unsupported.

A Full Magic Pack update that adds a frame, or that uses a script function the interpreter does not have, will not render until this repo changes. That limit is accepted.





## What the editor stores for a frame

The card form is MSE-like: the fields for the selected stylesheet, plus the shared MTGJSON fields. Stylesheet-specific controls write `boggled.styling` and `boggled.mseFields`. Switching stylesheet does not delete those maps; the renderer ignores keys the active style does not read.

## Adding a frame

For Windows, add the stylesheet to the allowlist, map its editable fields to the evaluated native layout, and add an exact-pixel reference test. For the legacy painter, implement the script functions the style requires and add a perceptual-hash comparison to its fixture suite.
