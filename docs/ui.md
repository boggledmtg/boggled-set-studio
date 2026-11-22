# UI

Look and layout of the editor. Product behavior is in the root `AGENTS.md`. How the window, server, and terminal are wired is in [architecture.md](architecture.md).

The mockup is [bss.fig](bss.fig). It shows the Cards workspace. The app mark is the bogle:

![Bogle](bogle-icon.png)

## Layout

The editor follows Magic Set Editor's cards workspace. The extra surface is a terminal docked on the right.

The mockup is the visual guide for chrome, density, and palette. This version's window, from top to bottom:

- A title bar with the bogle mark, the open set's name, "Boggled Set Studio," and Minimize, Maximize, and Close. The title bar is shown only when the page was opened with `?shell=1`. A normal browser loads the same editor without it.
- A menu bar: File, Edit, Cards, Help.
- A toolbar with the set name, New card, Delete card, Set illustration, Export, a card search field, and the selection count.
- A tab strip: Cards and Set info.

File holds New set, Open set, Import MSE set, Export MTGJSON, and Close set. Edit holds Undo and Redo. Cards holds New card, Delete card, and Set illustration. Help holds About.

The Cards tab is three columns.

| Column | Contents |
| --- | --- |
| Left | The rendered card, then the field form, then notes. |
| Center | The card list: name, cost, mana value, type, power and toughness, rarity, collector number, and color. The selected row is highlighted. |
| Right | The terminal. It runs the user's coding tool with the open set folder as its working directory. |

Name, mana cost, type line, rules, flavor, power, and toughness are boxes on the card preview. The type box is one field. It suggests supertypes and card types before the dash, and subtypes after it, and the save still writes one MTGJSON type line. Rarity, collector number, artist, and notes stay in the form under the card. Notes edit `boggled.designNotes`. The Set info tab edits the set name, the set code, and the design brief, and the terminal stays on the right.

With the Windows runtime, unfocused fields show the native MSE image. Clicking a field opens a browser input at the bounds supplied with that image. The browser handles text selection, caret movement, and type suggestions.

A field saves when it blurs or after a short pause (about 400 ms). That save is one undo step. Ctrl+Z and Ctrl+Y (and Ctrl+Shift+Z) call the server. While the text in the field still differs from the last save, Ctrl+Z is the browser's own undo. While the terminal pane is focused, Ctrl+Z goes to the shell.

With a set open, path fields are typed. When `?shell=1` is set, those fields also show Browse, which calls the desktop `pick_folder`, `pick_file`, or `save_file` command. The browser build has no Browse button.

An unsupported face stays in the grid. Its preview says "This frame is unsupported" and its Name field is disabled. An empty selection says "No card selected". An empty search says "No cards".

Card frames in the product come from Full Magic Pack. The mockup draws a sample card in this palette so the preview column has a picture. That column is where the real render sits.

## Palette

Colors come from the bogle. The icon is a saturated gold circle, a coral tongue, and deep brown. The UI uses those hues softened into cream, gold, cocoa, and coral.

| Role | Hex | Where it goes |
| --- | --- | --- |
| Gold | `#FBD56A` | Title bar, tab strip, selected row |
| Gold deep | `#E7A83A` | Selection marker, card frame (`#F0C15E`) |
| Cream | `#FFF9F2` | Editor surfaces, menus, card plates |
| Ink | `#3C2615` | Primary text |
| Cocoa | `#5C3A1E` | Secondary text and rules on gold |
| Coral | `#E57B5B` | Small accents, such as the close control and terminal prompts |
| Line | `#EAD7B4` | Hairline borders |
| Terminal | `#3E2718` | The right pane |

Terminal text is cream (`#FFF3DC`). Paths and the cursor are gold. Command prompts are coral (`#F0A888`). The terminal is the dark surface, in the icon's deep brown.

## Styling

Pastel and warm. Corners are slightly rounded. Shadows are soft and brown. Chrome is gold and cream. The card list is a light paper grid with a gold selected row. The bogle is the mark in the title bar.
