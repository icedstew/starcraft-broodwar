# Campaign board: image + polygon layout and its editor

Draft 2026-11-01 from a Q&A with the user. Ours, not 1.16.1. Builds on
agent/plans/campaign.md (world file, host, web map).

## What the user wants

A Risk-style board for a campaign: a background image the user supplies, each area drawn
as a polygon tinted with its owner's team colour (instead of a circle), the connections
between areas (borders) and where each battle square sits. A separate HTML + JS editor to
define all of it, with flood fill to turn a painted region into a polygon. The live
campaign map draws the board.

## Decisions (user, 2026-11-01)

| Topic | Decision |
|---|---|
| Generator | Unchanged: `scbw campaign gen` writes only the world text file; no image, circles on a plain background stay the default. |
| Board file | Sidecar JSON next to the world file; world.txt stays the game-state source. No board file = today's circles. |
| Image | Always user-supplied art (a painted Risk-style board), loaded in the editor. |
| Flood fill | Click inside a region; fill stops at pixels that differ beyond a tolerance; outline traced and simplified to a polygon for the selected area. |
| Alpha | One fill alpha + one outline alpha for team colours, in the board file. |
| Live map | campaign.html draws image, tinted polygons, connections and battle squares when the world has a board; circles otherwise. |
| Editor | Separate static HTML + JS app in `web/board/`; opened as a local file, not served by the host. Adds and deletes areas (a board can be built from scratch around a picture). |
| Image format | PNG only. |
| Board link | A `board <path>` line in world.txt (relative to the world file); the host serves the JSON and the PNG. |

## Files

```
world.txt            factions, areas, borders, pool (as today) + `board world.board.json`
world.board.json     the board (below)
world.board.png      the image (any name; the JSON points at it, path relative to the JSON)
```

`world.board.json`, coordinates in image pixels:

```json
{
  "image": "world.board.png",
  "size": [1600, 1000],
  "fill_alpha": 0.45,
  "outline_alpha": 0.9,
  "areas": [
    { "poly": [[x, y], ...] }
  ],
  "borders": [
    { "square": [x, y], "path": [[x, y], ...], "line": false }
  ]
}
```

- `areas[i]` / `borders[i]` match world.txt's areas / borders by index (file order, as the
  world file already numbers them). `poly` optional: no polygon = a circle at the area's x, y.
- The area's name is drawn at its world.txt point (`x, y`, image pixels for a boarded
  world); no separate label spot. A fill moves the point to the polygon's centroid when the
  point is outside the polygon.
- `square` optional: default the middle of the connection line.
- `path` optional: bend points of the connection line (a Risk sea lane); default straight.
- `line: false` hides the connection line (e.g. one across the map's edge).

Index-matching is fragile if someone hand-edits world.txt and reorders: the live page
compares the counts with `/state` and ignores the board (console warning) when they differ.
The host itself only reads `"image"` (no JSON parser in the deps) and checks the PNG
signature.

## Editor — `web/board/index.html` + `board.js` (static, no build, no deps)

Opened from `file://`. Loads files with
`<input type=file>` / drag-drop; saves with a download (and the File System Access API
`showSaveFilePicker` where the browser has it, so re-saves overwrite in place).

Loads: world.txt (parser in JS, same grammar as `World::parse`, keeps the `board` line),
the PNG, an existing board JSON. Saves: world.txt (areas and borders edited) + board JSON.

Modes (toolbar, one active):

1. **Areas**: list on the side (name, owner); select one; drag its point; rename; add /
   delete (delete also drops its borders; indices renumber in both files together).
2. **Fill**: with an area selected, click the image: scanline flood fill on the image
   pixels from the click, colour distance <= tolerance (slider 0..255, RGB max-channel
   distance). Result mask -> outer contour (marching squares) -> Douglas-Peucker simplify
   (epsilon slider, px) -> `poly`. Holes ignored. Preview mask before accepting; Shift-click
   adds to the current mask (regions split by a river or label text).
3. **Polygon**: edit `poly` vertices by hand: drag, click an edge to insert, Delete key to
   remove; draw a fresh polygon by clicking points.
4. **Connect**: click area A then area B: toggles a border. Selected border: map field
   (free text, the world.txt `border a b [map]` map path), drag its battle square, add
   path bend points.
5. **Preview**: as the live map will show it: owner tint at `fill_alpha`, outlines at
   `outline_alpha`, labels, squares. Owner per area editable here (or `-` for dealt).

Board-level: image size read from the image; alpha sliders; zoom/pan (wheel + drag with
Space) since boards are bigger than the screen.

Validation shown live: area without polygon, border with no map and no pool, overlapping
polygons (warning only), disconnected graph (warning: `deal` needs it connected).

Check: a tiny self-test in the page (`?test`) asserting flood fill + contour + simplify on
a synthetic canvas (a drawn rectangle and an L shape give the expected polygon vertex
counts and areas), and world.txt parse -> write round-trip equals input.

## Host + live map

- world.rs: new `board <path>` line (parse/write keep it; `World.board: Option<String>`).
- `scbw campaign host <world>`: if the world has a `board` line, resolves it relative to
  the world file. Loads it with the existing JSON helpers (no serde if the codebase has none;
  check `src/campaign/web.rs` first), validates counts, serves `/board` (the JSON) and
  `/board/image` (the PNG bytes read once at start, `image/png`; refuse non-PNG by its
  signature).
- `/state` gains `"board": true` when one is loaded.
- `campaign.html`: if `st.board`, fetch `/board` once, set the SVG `viewBox` to the image
  size, `<image href="/board/image">` first, then per area a `<polygon>` filled with
  `colour(owner)` at `fill-opacity=fill_alpha`, stroke at `outline_alpha`; connection paths;
  battle squares at `square`; labels at `label`. Area points = polygon centroids for the
  border lines' ends. Without a board: today's code unchanged.
- Captures recolour polygons on the next `/state` poll (nothing new: owner is already in
  state).

Check: `agent/tools/campaign_e2e.sh` world with a hand-made 4-area board; screenshot of the
web map looked at (polygons tinted, squares where placed, image behind); a world with a
mismatched board falls back to circles with a warning in the host log.

## Tasks, in order

1. **Board JSON contract** + `board` line in world.rs (parse/write, test).
2. **Editor shell**: load world.txt + image + board, pan/zoom, Areas and Connect modes,
   save both files. Round-trip test.
3. **Fill mode**: flood fill, contour, simplify, Shift-add, self-test.
4. **Polygon mode** + Preview mode + alpha sliders.
5. **Host**: load/validate the board from the `board` line, `/board`, `/board/image`, `board` flag in `/state`.
6. **Live map**: draw the board in campaign.html; screenshot check.
7. Docs: pointer in campaign.md; usage paragraph at the top of `web/board/index.html`.

## Status (2026-11-01)

Done on branch `campaign-board`: tasks 1-6.
- world.rs `board` line (parse/write, test). Host makes the path absolute (so the copy saved
  in the campaign dir still finds it), serves `/board`, `/board/image`, `"board": true` in
  `/state`; a bad board is logged and the map falls back to circles.
- Editor `web/board/` (index.html + board.js): modes Areas / Fill / Polygon / Connect, Preview,
  undo, pan/zoom, checks panel, `?test` self-test (fill, contour, simplify, grow, world
  round trip). Fill: tolerance, grow (closes outline-line gaps, merges Shift-added
  neighbours), simplify. The outline traced is the one around the area's point, so
  Shift-added pieces that don't touch it drop out instead of replacing the area.
- campaign.html draws image, tinted polygons, connections, squares; sizes in screen px.
- Editor "Auto layout" (Board section): area points to each polygon's most interior spot
  (kept away from other labels, sideways more than up/down), squares onto the middle of the
  shared border (sea lanes: the line's middle), then pushed apart and off label points.
- Example: `~/games/Starcraft/campaign-world/` (34 Risk-style territories, Europe merged to
  British Isles / Western / Eastern / Scandinavia; 58 borders; 4 factions) from the CC0 Commons BlankMap-Equirectangular.svg; scripts and source note there.
  Built by driving the editor headless (fill per territory from lat/lon seeds).
- Testing note: headless Chromium here cannot load http from localhost (sandbox), so the
  live page was checked from file:// with fetch answered from the host's real `/state` and
  `/board`.

Left: a few squares still touch a label's text (Auto layout spaces squares from label
points, not label widths; pushing them clear of the whole label dragged them off their
lines, so that was dropped). Drag them in Connect mode, or a zoomable live map. Docs
pointer in campaign.md (task 7).

## Open questions (defaults in use until answered)

- Very large images (> 4096 px): fill runs on the full-res image; may need a downscaled
  working copy if slow.
- Should the in-game (.exe) next-match list ever show the board? Default: no.
