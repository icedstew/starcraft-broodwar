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

## Verification

- `scbw repshot` on a copy of ~/games/.../shots/tl1412_full with chosen meta.txt lines:
  diff_full/ shows HUD vs native (replay HUD text / buttons pixel-exact).
- `SCBW_MENU=game|options|speed|sound|video|tips|objectives [SCBW_MENU_REPLAY=1]
  [SCBW_MENU_INPUT="k o;c x y;k esc"] scbw shot <map> out.png x y` vs captures from
  `re/native_menu.sh <rep> <dir>` (replay menus) and `re/native_game.sh <dir>` (custom
  game as Terran on Astral Balance). The five option dialogs match pixel for pixel
  except their transparent corners.
- `SCBW_PANEL_CLICK=x,y[,shift|ctrl]`, `SCBW_CURSOR=x,y` (tooltips) on `scbw shot`.

## Left

- Dialog translucency: flag 0x40000 -> 0x416f30 blends the key color (0x4c8e80) through
  table 0x6d0590[2] (0x4174a0, level 0x6d0aa8 = 4 from 60%). Native in-game menus show
  the map through.
- Listbox (0x41b950 / 0x41af10, scrollbar 0x415760) for keystroke help (help.bin).
- Save / load, network options, scroll speed tables, gamma, minimap button tooltips.
- Selection circles / health bars as images in their own pools (0x57d768 / 0x5244b8,
  0x4d6930; 0x4d7070, 0x4d4fa0, 0x4975d0, 0x497590): needs per-player selections
  (0x6284e8) and the local selection (0x6284b8) in State.
- Status refresh flags 0x4c36c0, placement box (placebox.cpp 0x48e310), glue menus,
  replay UI 0x4deb90, chat 0x48d1c0, panels 0x427a80 / 0x4273e0 / 0x425ee0 / 0x426ee0.
