# Interface (src/ui.rs, dlg.rs, menu.rs, view.rs draw_hud, statdata.rs, smk.rs, portrait.rs)

Read when working on the HUD, dialogs, menus or glue screens.

## Architecture

- The HUD is drawn 8-bit in the game palette (view.rs `draw_hud`), converted to RGB last.
- Text engine: ui.rs `Font::draw_text` = 0x4202b0 / 0x4200d0 / 0x41f920 / 0x41fd70 /
  0x41f7c0 (advance w + xoff + spacing 1, word wrap, flags 0x6ce110: 1 left, 2 center,
  4 right, 0x20 vcenter, 0x10 no vertical align; color codes 0x41f610; extents
  0x6ce0d8..0x6ce0de). Fonts: 0x6ce0f4 font8 (flag 0x400), f8 font10, fc font16 (0x800),
  100 font16x (0x4000).
- Dialogs (dlg.rs): .bin = 0x56-byte controls; dialog rect right/bottom are w-1/h-1.
  Default draws by type: table 0x501504 (interact 0x5014ac). Types: 0 dialog, 1 default
  button, 2 button, 3 option, 4 check, 5 image, 6 hscroll, 7 vscroll, 8 edit, 9/10/11
  label left/center/right, 12 listbox, 13 combo, 14 lightup. Custom draws go in
  `Update`. Dialog grps (dlgs\<race>.grp) are raw bitmaps; frame sizes 0x4197b0.
- Interface colors 0x6ceb20 = nearest palette index of the 26 RGBs at 0x51a0f0
  (minimap.colors). 0x10 = focus outline blue, 0xf black, 0x12 drag box green.
- In-game menus (menu.rs): 0x4f57a0 opens with event proc 0x4ca240 (or the dialog's own);
  activated id 0x6d051c goes to callback 0x6d1234; depth 0x6d1224. Settings 0x6cdfd4.
- Portraits: portrait.rs + smk.rs (Smacker SMK2), statport.bin, arr\portdata.

- Glue font set 1 (0x4daec0): 0x6ce0f4 font10, f8 font14, fc font16, 100 font16x (the
  game's set 2 has font8 first). List boxes: dlg.rs `ListBox` (0x41b140 rows, 0x41b410
  layout, 0x41af10 rows with "..."); edits: `Edit` (0x416450, caret glyph 0xa6 after
  code 4, blink 500 / 250 ms). Event 0xf (WM_CHAR) 0x418ab0: Enter = id -2 or flag
  0x80000, Esc = id -3 or flag 0x40, hotkeys need flag 0x200 and no edit in the dialog.
- Shade blits (0x416f30, table 0x6d05a8 from 0x417270): source 0 keeps the screen, the
  shade color (tEffect.pcx) blends, other colors are drawn.

## Verification

- Glue screens: `SCBW_GLUE_INPUT="c x y;k vk(hex);t text;w ms" SCBW_CHARACTERS=<dir>
  scbw glue out.png`; native with a wine + Xvfb + xdotool script like re/native_game.sh
  (login flow: Single Player 217,120; Expansion 367,275; the New ID popup opens itself
  with no characters).

- `scbw repshot` on a copy of ~/games/.../shots/tl1412_full with chosen meta.txt lines:
  diff_full/ shows HUD vs native (replay HUD text / buttons pixel-exact).
- `SCBW_MENU=game|options|speed|sound|video|tips|objectives [SCBW_MENU_REPLAY=1]
  [SCBW_MENU_INPUT="k o;c x y;k esc"] scbw shot <map> out.png x y` vs captures from
  `re/native_menu.sh <rep> <dir>` (replay menus) and `re/native_game.sh <dir>` (custom
  game as Terran on Astral Balance). The five option dialogs match pixel for pixel
  except their transparent corners.
- `SCBW_PANEL_CLICK=x,y[,shift|ctrl]`, `SCBW_CURSOR=x,y` (tooltips) on `scbw shot`.

## Keys (1.16.1)

- In-game keys are WM_COMMAND accelerators from Local.dll table 101 (102 multiplayer),
  handled by 0x4846e0; digits alone come by WM_CHAR (0x4846b0 -> 0x496b40). Ctrl+digit
  set, Shift+digit add (exe table 113), Alt+digit center + select (0x4967e0), digit
  twice within 500 ms centers (0x4c07b0); F2..F4 recall / Shift+F2..F4 save the screen
  (0x57f270); Ctrl/Alt+C center on the status unit (0x4e6020); +/= -/_ speed; F1 /
  Alt+H help, Alt+O options, Ctrl/Alt+Q quit, Ctrl/Alt+X exit, Alt+M messages
  (0x4ca500, not ported), Alt+S / Alt+L save / load (not ported), Ctrl+M music,
  Ctrl+S sound (read from Local.dll's RT_ACCELERATOR resources).

- Button strings in stat_txt.tbl are hotkey, kind byte (0 plain, 1 unit, 2 upgrade,
  3 tech), text; kind 0 ends a C string, so read them with `dat::tbl_button`, not
  `parse_tbl` (0x4593a0 takes the text at +2).
- Minimap buttons (rez\minimap.bin, proc 0x4a5570: event 0xe user 2 = 0x4a54f0):
  button 2 terrain on / off (0x6d5bbf), tooltip stat_txt 0x32b / 0x32a (1-based).
  The tooltip says Tab, but no Tab was found in Local.dll accelerators (table at file
  0x5aa0) or the minimap procs: not wired yet.

## Left

- Dialog translucency: flag 0x40000 -> 0x416f30 blends the key color (0x4c8e80) through
  table 0x6d0590[2] (0x4174a0, level 0x6d0aa8 = 4 from 60%). Native in-game menus show
  the map through.
- Listbox (0x41b950 / 0x41af10, scrollbar 0x415760) for keystroke help (help.bin).
- Save / load, network options, scroll speed tables (edge and arrow share one rate), gamma, minimap button tooltips.
- Selection circles / health bars as images in their own pools (0x57d768 / 0x5244b8,
  0x4d6930; 0x4d7070, 0x4d4fa0, 0x4975d0, 0x497590): needs per-player selections
  (0x6284e8) and the local selection (0x6284b8) in State.
- Status refresh flags 0x4c36c0, placement box (placebox.cpp 0x48e310), glue menus (login
  done),
  replay UI 0x4deb90, chat 0x48d1c0, panels 0x427a80 / 0x4273e0 / 0x425ee0 / 0x426ee0.
- Shown terrain (megatiles 0x628494) lags MTXM: creep changes reach it only for tiles
  the local player sees (0x47e2d0), every 100 frames off-screen (0x49c4c0), and every
  draw on-screen (0x49c780 from the map callback 0x4bd580; `refresh_screen_tiles`, run
  at the end of `Scene::step`). Without the last, start creep looked small (user report
  2026-10-17). Native check: tl1354 frames 10..130 in the real game show full creep.
