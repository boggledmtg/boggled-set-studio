# Renderer

The card renderer is an interpreter for Full Magic Pack, not a set of hardcoded frames that merely borrow its images. Card files and `boggled.stylesheet` are defined in [set-format.md](set-format.md). The render routes are in [api.md](api.md).

Full Magic Pack is a public collection of Magic Set Editor templates: https://github.com/MagicSetEditorPacks/Full-Magic-Pack. A style such as `magic-m15.mse-style` is a package file plus images. The package file names a game (`magic.mse-game`), shared includes, and symbol fonts, and it computes layout with MSE's expression language (`if … then … else`, field access, `include file`, string and image functions).

At first launch the app asks for a Full Magic Pack `data` directory. The user can point at an existing MSE install or let the app download the `data` tree from a commit this repo pins. The pin is recorded in `config.json`. The download excludes `magicseteditor.exe`, `mse.exe`, and `magicseteditor.com`. Those binaries are GPL Magic Set Editor, and this app replaces them instead of embedding them.

Fonts are not part of that download. Templates name faces such as MPlantin and Beleren. Those files have their own licenses. The app checks for the faces a stylesheet requests and tells the user which files are missing. It does not fetch the pack's `Magic - Fonts` folder automatically.

The pack has no LICENSE file. Being on GitHub does not grant a license to vendor it. Frame art and symbol images in the pack, including the pieces attributed to CardConjurer, are fan reproductions of Magic's card trade dress. Boggled treats them as a local runtime asset, does not commit them, and does not claim Wizards of the Coast's art, symbols, or trademarks. The app's own UI should stay obviously unofficial.

## Pipeline

1. Load and index the pinned `data` directory: game, includes, symbol fonts, locales, styles.
2. Resolve the card's `boggled.stylesheet` and its `depends on` packages.
3. Reject the job if that style is not on the allowlist.
4. Map the card file onto the MSE `card` and `styling` values.
5. Evaluate the style's scripts with the Rust interpreter.
6. Lay out text: wrap, MSE tags, symbol fonts, shrink-to-fit, flavor italics.
7. Composite frame images, masks, the illustration at `artFile`, and the text.
8. Write `renders/<uuid>.png` at the stylesheet's card width, height, and dpi.

Auto-render is on by default for a set this server has open. A saved card, token, or art file is rendered after the watcher debounce, including in headless mode. `boggled render` does the same job when the user wants it immediately. The render queue is per set, so two clients cannot composite the same uuid at once.

Import does not invoke the renderer. An `.mse-set` import copies illustration crops into `art/` and leaves `renders/` empty until something calls render. See [import.md](import.md).

## Allowlist and the script subset

Supporting a frame means all three of these are true:

- The stylesheet and the packages it includes parse.
- Every script function and operator that stylesheet uses is implemented.
- A fixture in this repo renders that frame and compares a perceptual hash of the PNG.

The first allowlist is the common play frames: normal (M15 / mainframe), token, planeswalker, saga, split (including aftermath), flip, transform, modal DFC, adventure, class, and leveler. Battle and other newer frames are added the same way, one at a time.

A Full Magic Pack update that adds a frame, or that uses a script function the interpreter does not have, will not render until this repo changes. That limit is accepted. The interpreter should fail that card with the missing function or package name, and leave every other card renderable.

Text layout and image compositing are the bulk of `mse`, not the HTTP layer. Budget work there before adding editor chrome.

Do not call Magic Set Editor as a subprocess to render. That would hide the gap in the interpreter and tie the app to a GPL binary.

Do not port Magic Set Editor's C++ into this repository. Implement the package format and script behavior from the template files and MSE's public documentation.

## What the editor stores for a frame

The card form is MSE-like: the fields for the selected stylesheet, plus the shared MTGJSON fields. Stylesheet-specific controls write `boggled.styling` and `boggled.mseFields`. Switching stylesheet does not delete those maps; the renderer ignores keys the active style does not read.

## Adding a frame

Implement the missing script functions. Add the stylesheet id to the allowlist. Add a fixture card and a perceptual hash. Do not special-case that frame in the React preview; the preview shows `renders/<uuid>.png`.
