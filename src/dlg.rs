//! 1.16.1 dialogs (dlg.cpp, modeless.cpp, opt_dlgs.cpp): `.bin` dialog resources and
//! the default drawing of their controls (table 0x501504, by control type).

use crate::grp::{Blend, Grp};
use crate::smk::Smk;
use crate::storage::Storage;
use crate::todo_port;
use crate::ui::{Font, TextDst, TextOpts};
use std::cell::RefCell;
use std::rc::Rc;

/// Control flags (+0x18).
pub mod flag {
    pub const DISABLED: u32 = 0x2;
    pub const VISIBLE: u32 = 0x8;
    /// Text starts with a hotkey byte (0x300).
    pub const HOTKEY: u32 = 0x300;
    pub const FONT_SMALL: u32 = 0x400;
    pub const FONT_LARGE: u32 = 0x800;
    /// Drawn with the focus outline (0x418440).
    pub const FOCUS: u32 = 0x1000;
    pub const FONT_LARGEST: u32 = 0x4000;
    /// No space between glyphs.
    pub const NO_SPACING: u32 = 0x10000;
    pub const NO_OUTLINE: u32 = 0x20000;
    /// The control's box sits at the right of its rect (check / option boxes).
    pub const BOX_RIGHT: u32 = 0x10000000;
    /// Scroll bars: the option dialogs' tick slider (0x4153b0).
    pub const TICK_SLIDER: u32 = 0x2000000;
    /// Buttons: three-piece wide button (0x4efeb0).
    pub const WIDE: u32 = 0x20000000;
    pub const PRESSED: u32 = 0x40000000;
}

/// One control (0x56 bytes in a `.bin`).
#[derive(Clone, Debug)]
pub struct Ctl {
    pub id: i16,
    /// left, top, right, bottom (inclusive), relative to the dialog.
    pub rect: (i32, i32, i32, i32),
    pub ty: u16,
    pub flags: u32,
    /// +0x14, as stored (color codes and the hotkey byte included).
    pub text: Vec<u8>,
    /// +0x24: base frame in the dialog grp.
    pub graphic: u16,
    /// +0x36: check box / option state.
    pub checked: bool,
    /// Scroll bars: value, min, max (+0x3a, +0x3c, +0x3e).
    pub value: (i16, i16, i16),
    /// Tick sliders: thumb frame (+0x48, 100 from 0x414c00) and the value step per
    /// tick (+0x49).
    pub thumb: u8,
    pub step: u8,
    /// +0x2e when a dialog's init replaced the type's default draw.
    pub update: Update,
    /// Image controls (type 5): +0xc..+0x14, the pcx named by the text (0x4d27a0).
    pub image: Option<Rc<Image>>,
    /// Light-up buttons (type 14): the animation list at +0x42.
    pub anims: Vec<Anim>,
    /// Light-up buttons: +0x36..+0x3c, the rect the mouse lights the button in (relative
    /// to the control until 0x4e14a0 adds its position).
    pub hot: (i32, i32, i32, i32),
    /// Light-up buttons: +0x3e, the light level 0..=100 (0x4e0e40 moves it 15 per tick).
    pub hover: i16,
    /// Light-up buttons: +0x46..+0x4c, the text rect (made absolute by 0x4e14a0).
    pub trect: (i32, i32, i32, i32),
    /// Light-up buttons: +0x4e, the text flags (the control flags >> 0x14).
    pub align: u8,
    /// List boxes (type 12) once 0x41b140 ran (event 10).
    pub list: Option<ListBox>,
    /// Edit controls (type 8) once 0x416450 ran (event 10); the text is `text` (+0x14).
    pub edit: Option<Edit>,
    /// The create game screen's list and folder label: the frames of glue\create\iCreate.grp
    /// (0x59ba60) their draws (`Update::MapList` / `DirLabel`) use.
    pub icons: Vec<Icon>,
    /// The score screen's custom draws (`Update::Score`, glue_score.rs).
    pub score: Option<ScoreCtl>,
    /// The ready room's own picture (`Update::BriefText` / `Update::Portrait`, glue_ready.rs).
    pub pic: Option<Pic>,
}

/// An off-screen picture a control's own draw shows: the briefing message (0x66ff68) or a
/// portrait's video buffer (0x44ef00).
#[derive(Clone, Debug, Default)]
pub struct Pic {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
    /// The first row shown (the message's scroll, [control + 0x24]).
    pub scroll: i32,
    /// Where it sits in the control (a portrait's bytes +0x11 / +0x12).
    pub dx: i32,
    pub dy: i32,
}

/// What a gluScore.bin control draws (its +0x2e draw function and the row data it reads).
#[derive(Clone, Debug)]
pub enum ScoreCtl {
    /// 0x4b3820: a tab button (the selected one, flag bit 31, shows only its text).
    Tab,
    /// 0x4b3b80: the total score's box (its picture) and the text.
    Box,
    /// 0x4b38a0: a row's name label: the race icon framed in the player's color
    /// (`color`), the name, and `aux` (the force / team name) beside the icon.
    Name { gfx: Rc<Vec<Icon>>, race: u8, color: u8, aux: Vec<u8> },
    /// 0x4b3a40: a row's bar: `value` out of `max` over the control's width, in the frames
    /// of the player's color slot `color`; then the number.
    Bar { gfx: Rc<Vec<Icon>>, value: i32, max: i32, color: u8 },
}

/// A frame of a grp: its offset in the box, size and pixels (0 transparent).
#[derive(Clone, Debug)]
pub struct Icon {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

/// 0x41b140 +0x47: the most rows a list box holds.
pub const LIST_MAX: usize = 0xfe;

/// A list box's rows and scroll state (dlg\lbox.cpp).
#[derive(Clone, Debug, Default)]
pub struct ListBox {
    /// +0x3a / +0x3e / +0x42 (+0x46 rows): per row the text, its flags (low nibble: the
    /// text color, 0 = 3) and data.
    pub items: Vec<(Vec<u8>, u8, u32)>,
    /// +0x48: the selected row (0xff none).
    pub sel: u8,
    /// +0x49: the row height.
    pub row_h: u8,
    /// +0x4a: the rows shown.
    pub rows: u8,
    /// +0x4b: the last top row.
    pub max_top: u8,
    /// +0x4d: the top row.
    pub top: u8,
    /// +0x4e: the highlighted row (follows the mouse while the button is down).
    pub hover: u8,
    /// +0x18 bit 31: the layout (0x41b410) waits until the rows are added.
    pub batch: bool,
    /// The left button went down in the list (the dialog's capture 0x6d5e54 / 0x6d5e4c).
    pub pressed: bool,
    /// Combo boxes (type 13) +0x4c: bit 0 the drop-down opens upward (0x41b410: it does not
    /// fit above the dialog's bottom).
    pub up: u8,
    /// Combo boxes +0x50: the closed top (0x41a110), the open rect grows from it.
    pub saved_top: i32,
    /// The dialog's rect bottom (+0xa of the parent, 0x41b410 compares with it); 479 until the
    /// caller sets it.
    pub parent_bottom: i32,
    /// +0x36: the scroll bar child (0x414c90, scrollbar.rs).
    pub scroll: Option<Box<crate::scrollbar::ScrollBar>>,
}

/// An edit control's state (dlg\edit.cpp, 0x416450).
#[derive(Clone, Debug, Default)]
pub struct Edit {
    /// +0x36: 0x10 insert mode, 1 the shown part needs 0x416240, high nibble the text
    /// color (0 = 2).
    pub mode: u8,
    /// +0x37: the first character shown.
    pub first: u8,
    /// +0x38 / +0x39: the text's x margin and y offset.
    pub margin: u8,
    pub dy: u8,
    /// +0x3a: the width the text may use.
    pub width: i16,
    /// +0x3e: the caret.
    pub caret: u8,
    /// +0x26 (gluPEdit 0x4b6e50): the buffer size; the text keeps one byte for the end.
    pub max: u32,
    /// +0x24 (gluPEdit): the text is the preset one; the first character replaces it.
    pub preset: bool,
    /// The caret is shown (+0x18 0x40000000) and the ms to its next toggle (the 0x416600
    /// timer: 500 on, 250 off); None when the control has no focus.
    pub blink: Option<u32>,
}

/// An 8-bit image (0x4d27a0's out record: width, height, pixels).
#[derive(Debug)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

/// One entry of a light-up button's list (0x4e0be0 / 0x4e1260): a Smacker video drawn
/// at an offset in the control.
#[derive(Clone)]
pub struct Anim {
    /// +4: 1 fades with the light, 2 dims when unlit, 4 loops, 8 only while lit, 0x10
    /// transparent.
    pub flags: u32,
    /// +8: the fade level 0..=100.
    pub alpha: i16,
    /// +0xa: the .smk path.
    pub path: Vec<u8>,
    /// +0x12 / +0x14: offset in the control.
    pub x: i32,
    pub y: i32,
    /// +0xe / +0x1a: the open video and its milliseconds into the frame; None when
    /// finished (0x4e0f50 frees it).
    pub video: Option<Rc<RefCell<Smk>>>,
    pub elapsed: u32,
}

impl std::fmt::Debug for Anim {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Anim({:?}, flags {:#x})", String::from_utf8_lossy(&self.path), self.flags)
    }
}

/// Draw functions dialogs install over the defaults (+0x2e).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Update {
    Default,
    /// 0x4f4fb0: the console's MENU button (set by 0x4f5180).
    MenuButton,
    /// 0x47e590: a tip: color 2, left aligned, no line gap.
    Tip,
    /// 0x4ad6b0: a row of the create game screen's map list (folder or map icon first).
    MapList,
    /// 0x4ad980: the create game screen's folder label (the folder icon, then the text).
    DirLabel,
    /// The score screen's controls (`Ctl::score`).
    Score,
    /// 0x4d8930: a credits text area: the label with these text flags (0x6ce110) and flag
    /// 0x80000000.
    Credits(u8),
    /// 0x46c6e0: the ready room's message buffer from its scroll row, over the control.
    BriefText,
    /// 0x44e9e0: a ready room portrait's video buffer, copied into the control.
    Portrait,
}

#[derive(Clone, Debug)]
pub struct Dialog {
    /// The dialog's own entry (control 0): rect is absolute.
    pub this: Ctl,
    pub controls: Vec<Ctl>,
    /// +0x3e: the active (hovered / focused) control, an index into `controls`.
    pub active: Option<usize>,
    /// +0xc / +0x10: the background bitmap (0x4c9cc0) and its size.
    pub bg: Option<(i32, i32, Vec<u8>)>,
    /// 0x6cddc4 / 0x6cddc8: the mouse (screen), for the light-up buttons.
    pub mouse: (i32, i32),
    /// Milliseconds since the 30 ms timer 0x4e0f50 last ran.
    timer: u32,
}

impl Ctl {
    /// 0x41b410 (list boxes, combo boxes): the rows shown, the bottom snapped to whole
    /// rows, the selection and the top row kept in range. A combo box is laid out with its
    /// drop-down rows added (0x41b410 grows the rect down, or up when it does not fit) and
    /// shrunk back after; 0x418e00 enables it.
    pub fn list_layout(&mut self) {
        let combo = self.ty == 13;
        let Some(l) = self.list.as_mut() else { return };
        if l.batch {
            return;
        }
        let rh = l.row_h as i32;
        let mut grow = 0;
        if combo {
            grow = rh * l.items.len() as i32;
            if l.parent_bottom < self.rect.3 + grow {
                self.rect.1 -= grow;
                l.up |= 1;
            } else {
                self.rect.3 += grow;
                l.up = 0;
            }
            // 0x418e00
            if self.flags & flag::DISABLED != 0 {
                self.flags &= !flag::DISABLED;
                todo_port!("0x418900", "enabled control: tooltip / focus bookkeeping");
            }
        }
        let t = self.rect.1;
        let b = self.rect.3;
        let rows = (b - t + 1) as i16 as i32 / rh.max(1);
        l.rows = rows as u8;
        let h = (l.row_h as i32 * rows) as i16 as i32;
        self.rect.3 = (t + h.max(rh)) as i16 as i32;
        l.max_top = 0;
        let n = l.items.len() as u8;
        if n == 0 {
            l.sel = 0xff;
        } else if n <= l.sel && l.sel != 0xff {
            l.sel = n - 1;
        }
        l.hover = l.sel;
        let mt = (n as i32 - l.rows as i32).max(0) as u8;
        l.max_top = mt;
        if mt < l.top {
            l.top = mt;
        }
        let (scroll, up) = (l.scroll.is_some(), l.up);
        if scroll {
            self.list_scroll_layout();
        }
        if combo {
            if up & 1 == 0 {
                self.rect.3 -= grow;
            } else {
                self.rect.1 += grow;
            }
        }
    }

    /// 0x41b760: a row appended; its index (0xff when the list is full).
    pub fn list_add(&mut self, text: &[u8], flags: u8) -> u8 {
        let Some(l) = self.list.as_mut() else { return 0xff };
        if l.items.len() >= LIST_MAX {
            return 0xff;
        }
        l.items.push((text.to_vec(), flags, 0));
        let i = l.items.len() as u8 - 1;
        self.list_layout();
        i
    }

    /// 0x41b620: a row inserted at `idx` (appended when past the end).
    pub fn list_insert(&mut self, idx: u8, text: &[u8], flags: u8) {
        let Some(l) = self.list.as_mut() else { return };
        if l.items.len() >= LIST_MAX {
            return;
        }
        let i = (idx as usize).min(l.items.len());
        l.items.insert(i, (text.to_vec(), flags, 0));
        self.list_layout();
    }

    /// 0x41b550: row `idx` removed (the event proc hears event 0xc first).
    pub fn list_remove(&mut self, idx: u8) {
        let Some(l) = self.list.as_mut() else { return };
        if (idx as usize) < l.items.len() {
            l.items.remove(idx as usize);
        }
        self.list_layout();
    }

    /// 0x41b890: every row removed, then one layout.
    pub fn list_clear(&mut self) {
        let Some(l) = self.list.as_mut() else { return };
        let was = l.batch;
        l.batch = true;
        l.items.clear();
        if !was {
            l.batch = false;
            self.list_layout();
        }
    }

    /// 0x41baa0 event 0xe code 0xb: select `row`, unless the mouse holds another row.
    pub fn list_select(&mut self, row: u8) {
        let Some(l) = self.list.as_mut() else { return };
        if l.hover != l.sel && l.hover != row {
            return;
        }
        l.sel = row;
        l.hover = row;
    }

    /// 0x41a600 (BL = `row`): scroll so that `row` shows.
    pub fn list_scroll_to(&mut self, row: u8) {
        let Some(l) = self.list.as_mut() else { return };
        if row < l.top {
            l.top = row;
        } else if l.top as u32 + l.rows as u32 <= row as u32 {
            l.top = row.wrapping_sub(l.rows).wrapping_add(1);
        }
    }

    /// 0x41a1b0: (x, y) relative to the dialog is over the rows (a shown scroll bar is
    /// not counted). A combo box's rows exclude its button row.
    pub fn list_inside(&self, x: i32, y: i32) -> bool {
        let (l, mut t, r, mut b) = self.rect;
        let r = r - self.list.as_ref().map_or(0, |lb| lb.scroll_width());
        if let Some(lb) = self.list.as_ref().filter(|_| self.ty == 13) {
            if lb.up & 1 == 0 {
                t += lb.row_h as i32;
            } else {
                b -= lb.row_h as i32;
            }
        }
        (l..=r).contains(&x) && (t..=b).contains(&y)
    }

    /// 0x41a2c0: the row at (x, y) relative to the dialog; above or below the list it is
    /// the row next to the highlighted one (the drag scrolls). A combo box that opens
    /// downward starts below its button row.
    pub fn list_row_at(&self, _x: i32, y: i32) -> u8 {
        let Some(l) = self.list.as_ref() else { return 0xff };
        let n = l.items.len() as u8;
        let y0 = self.rect.1 + if self.ty == 13 && l.up & 1 == 0 { l.row_h as i32 } else { 0 };
        let r = if y < y0 {
            if l.top < l.hover {
                l.top
            } else if l.hover == 0 {
                0
            } else {
                l.hover - 1
            }
        } else if self.rect.3 < y {
            if (l.hover as i32) < l.rows as i32 - 1 + l.top as i32 {
                l.rows.wrapping_add(l.top).wrapping_sub(1)
            } else if (l.hover as i32) < n as i32 - 1 {
                l.hover + 1
            } else {
                l.hover
            }
        } else {
            l.top.wrapping_add(((y - y0) as i16 as i32 / l.row_h.max(1) as i32) as u8)
        };
        if n <= r {
            n.wrapping_sub(1)
        } else {
            r
        }
    }

    /// 0x41a140 (events 0xe code 6; 0x41a4e0 for event 5 also redraws): the open drop-down
    /// closes, the rect back to the button (the saved top, one row high).
    pub fn combo_close(&mut self) {
        let Some(l) = self.list.as_ref() else { return };
        if self.flags & 0x100000 != 0 {
            self.flags &= !0x100000;
            self.rect.1 = l.saved_top;
            // DAT_006d04b0 = 0: the caller's "open combo" is gone.
            self.rect.3 = l.row_h as i32 + l.saved_top;
        }
    }

    /// 0x41a9d0: a key (virtual key `vk`) in a list box: None when it is not a list key,
    /// else the row to select (None: none).
    pub fn list_key(&self, vk: u16) -> Option<Option<u8>> {
        let l = self.list.as_ref()?;
        let (s, n, rows) = (l.sel, l.items.len() as i32, l.rows);
        let last = n - 1;
        Some(match vk {
            // Page up.
            0x21 if s != 0 && s as i32 <= last => Some(if s > rows { s - rows + 1 } else { 0 }),
            // Page down.
            0x22 if (s as i32) < last => Some(if (rows as i32 + s as i32) < last { (rows + s).wrapping_sub(1) } else { last as u8 }),
            // End, Home.
            0x23 if s as i32 != last => Some(last as u8),
            0x24 if s != 0 => Some(0),
            // Up, down.
            0x26 if s != 0 && s as i32 <= last => Some(s - 1),
            0x28 if (s as i32) < last => Some(s + 1),
            0x21..=0x24 | 0x26 | 0x28 => None,
            _ => return None,
        })
    }

    /// 0x416240 (font set 1, the glue's): the edit's first shown character, so that the
    /// caret shows.
    pub fn edit_rescroll(&mut self, font: &Font) {
        let (l, r) = (self.rect.0, self.rect.2);
        let Some(e) = self.edit.as_mut() else { return };
        let mut w = ((r - e.margin as i32 * 2) - l - font.advance(0xa6, 1) + 1) as i16;
        e.width = w;
        if e.caret < e.first {
            e.first = e.caret;
            return;
        }
        let t = &self.text;
        let shown: i32 = (e.first as usize..e.caret as usize).map(|i| font.advance(t.get(i).copied().unwrap_or(0), 1)).sum();
        if (shown as i16) < w {
            return;
        }
        e.first = e.caret;
        let mut p = e.caret as usize;
        while p > 0 {
            let a = font.advance(t.get(p - 1).copied().unwrap_or(0), 1) as i16;
            p -= 1;
            if w <= a {
                return;
            }
            w -= a;
            e.first -= 1;
        }
    }

    /// 0x416710: `c` typed at the caret (inserted, or over the next character when not
    /// in insert mode); the buffer holds 0xff characters.
    fn edit_insert(&mut self, c: u8) {
        let Some(e) = self.edit.as_mut() else { return };
        if self.text.len() + 1 >= 0x100 {
            return;
        }
        let at = e.caret as usize;
        if at == self.text.len() {
            self.text.push(c);
        } else if e.mode & 0x10 != 0 {
            self.text.insert(at, c);
        } else {
            self.text[at] = c;
        }
        e.caret += 1;
        e.mode |= 1;
    }

    /// 0x416690: the character before the caret (`after`: at the caret) deleted.
    fn edit_delete(&mut self, after: bool) {
        let Some(e) = self.edit.as_mut() else { return };
        if self.text.is_empty() {
            return;
        }
        let at = if after { e.caret as usize } else {
            e.caret -= 1;
            e.caret as usize
        };
        if at < self.text.len() {
            self.text.remove(at);
        }
        if e.first != 0 {
            e.mode |= 1;
        }
    }

    /// 0x416980 events 0 / 1 (key down): Insert toggles insert mode, End / Home / Left /
    /// Right move the caret, Delete deletes at it. True when handled.
    pub fn edit_key(&mut self, vk: u16, mods: u8) -> bool {
        let len = self.text.len() as u8;
        let Some(e) = self.edit.as_mut() else { return false };
        match vk {
            0x2d if mods & 0xf == 0 => e.mode ^= 0x10,
            0x23 => e.caret = len,
            0x24 => e.caret = 0,
            0x25 => e.caret = e.caret.saturating_sub(1),
            0x27 => e.caret = (e.caret + 1).min(len),
            0x2e => {
                if e.caret < len {
                    self.edit_delete(true);
                }
            }
            _ => return false,
        }
        true
    }

    /// 0x416980 event 0xf (a character): backspace deletes before the caret, a character
    /// the font has is typed. True when handled (0x41fcc0 says the font has it).
    pub fn edit_char(&mut self, c: u8, font: &Font) -> bool {
        let Some(e) = self.edit.as_mut() else { return false };
        // 0x416790 (AL = 1): the caret shows, its 500 ms timer restarts.
        e.blink = Some(500);
        self.flags |= 0x4000_0000;
        if c == 8 && e.caret != 0 {
            self.edit_delete(false);
            return true;
        }
        if !font.has_glyph(c) {
            return false;
        }
        self.edit_insert(c);
        true
    }

    /// 0x4164d0: the caret moves to the mouse at dialog x `x` (in proportion to the text
    /// width).
    pub fn edit_click(&mut self, x: i32, font: &Font) {
        let w = font.width_bytes(&self.text);
        let len = self.text.len() as i32;
        let l = self.rect.0;
        let Some(e) = self.edit.as_mut() else { return };
        let dx = x - l;
        e.caret = if dx < 1 {
            0
        } else if dx < w {
            (len * dx / w) as u8
        } else {
            len as u8
        };
        e.blink = Some(500);
        self.flags |= 0x4000_0000;
    }
}

impl Dialog {
    pub fn parse(d: &[u8]) -> Dialog {
        let i16_at = |o: usize| d.get(o..o + 2).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]));
        let u32_at = |o: usize| d.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let cstr = |t: usize| if t != 0 && t < d.len() { d[t..].split(|&c| c == 0).next().unwrap_or(&[]).to_vec() } else { Vec::new() };
        let anims = |mut p: usize| {
            let mut v = Vec::new();
            while p != 0 && p + 0x1e <= d.len() && v.len() < 16 {
                v.push(Anim { flags: u32_at(p + 4), alpha: i16_at(p + 8), path: cstr(u32_at(p + 10) as usize), x: i16_at(p + 0x12) as i32, y: i16_at(p + 0x14) as i32, video: None, elapsed: 0 });
                p = u32_at(p) as usize;
            }
            v
        };
        let ctl = |o: usize| {
            let t = u32_at(o + 0x14) as usize;
            let text = if t != 0 && t < d.len() { d[t..].split(|&c| c == 0).next().unwrap_or(&[]).to_vec() } else { Vec::new() };
            Ctl {
                id: i16_at(o + 0x20),
                rect: (i16_at(o + 4) as i32, i16_at(o + 6) as i32, i16_at(o + 8) as i32, i16_at(o + 10) as i32),
                ty: i16_at(o + 0x22) as u16,
                flags: u32_at(o + 0x18),
                text,
                graphic: i16_at(o + 0x24) as u16,
                checked: false,
                value: (0, 0, 0),
                thumb: 0,
                step: 0,
                update: Update::Default,
                image: None,
                anims: if i16_at(o + 0x22) == 14 { anims(u32_at(o + 0x42) as usize) } else { Vec::new() },
                hot: (i16_at(o + 0x36) as i32, i16_at(o + 0x38) as i32, i16_at(o + 0x3a) as i32, i16_at(o + 0x3c) as i32),
                hover: i16_at(o + 0x3e),
                trect: (i16_at(o + 0x46) as i32, i16_at(o + 0x48) as i32, i16_at(o + 0x4a) as i32, i16_at(o + 0x4c) as i32),
                align: 0,
                list: None,
                edit: None,
                icons: Vec::new(),
                score: None,
                pic: None,
            }
        };
        let mut this = ctl(0);
        // 0x419d20: the dialog's right / bottom are stored as width - 1 / height - 1.
        this.rect.2 += this.rect.0;
        this.rect.3 += this.rect.1;
        let mut controls = Vec::new();
        let mut o = 0x56;
        while o + 0x56 <= d.len() {
            controls.push(ctl(o));
            let next = u32_at(o) as usize;
            if next == 0 || next <= o {
                break;
            }
            o = next;
        }
        Dialog { this, controls, active: None, bg: None, mouse: (0, 0), timer: 0 }
    }

    /// The dialog's screen origin.
    pub fn origin(&self) -> (i32, i32) {
        (self.this.rect.0, self.this.rect.1)
    }

    pub fn get(&self, id: i16) -> Option<&Ctl> {
        self.controls.iter().find(|c| c.id == id)
    }

    pub fn get_mut(&mut self, id: i16) -> Option<&mut Ctl> {
        self.controls.iter_mut().find(|c| c.id == id)
    }

    /// Screen rect of control `id`.
    pub fn screen_rect(&self, id: i16) -> Option<(i32, i32, i32, i32)> {
        self.get(id).map(|c| self.abs(c))
    }

    pub fn abs(&self, c: &Ctl) -> (i32, i32, i32, i32) {
        let (x, y) = self.origin();
        (x + c.rect.0, y + c.rect.1, x + c.rect.2, y + c.rect.3)
    }

    /// The topmost visible, enabled control under screen point (x, y). Light-up buttons
    /// answer for their hot rect (0x4e0b30: visible, enabled, flag 0x10).
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let (ox, oy) = self.origin();
        self.controls.iter().rposition(|c| {
            if c.ty == 14 {
                let (l, t, r, b) = c.hot;
                return c.flags & flag::VISIBLE != 0
                    && c.flags & flag::DISABLED == 0
                    && c.flags & 0x10 != 0
                    && (l..=r).contains(&(x - ox))
                    && (t..=b).contains(&(y - oy));
            }
            let (l, t, r, b) = self.abs(c);
            // A picture (type 5) takes the mouse only with flag 0x10 (the ready room's pictures lie over its buttons).
            c.flags & flag::VISIBLE != 0 && c.ty != 9 && c.ty != 10 && c.ty != 11 && (c.ty != 5 || c.flags & 0x10 != 0) && (l..=r).contains(&x) && (t..=b).contains(&y)
        })
    }

    /// 0x4194e0 (after the controls are placed): an image control loads the pcx its text
    /// names through 0x4d27a0; a light-up button's text rect moves to its position.
    pub fn bind(&mut self, files: &Storage) {
        for c in &mut self.controls {
            if c.ty == 14 {
                c.trect = (c.trect.0 + c.rect.0, c.trect.1 + c.rect.1, c.trect.2 + c.rect.0, c.trect.3 + c.rect.1);
            }
            if c.ty == 5 && !c.text.is_empty() {
                let path = String::from_utf8_lossy(&c.text).into_owned();
                let Ok(d) = files.read(&path) else { crate::bw::fatal_error(&path) };
                let (w, h, px) = crate::view::pcx(&d);
                c.image = Some(Rc::new(Image { w: w as i32, h: h as i32, px }));
            }
        }
    }

    /// Event 10 to the light-up buttons (0x4e14a0): the hot rect moves to the dialog,
    /// each list entry opens its video (0x4e1260) and shows its first frame.
    pub fn init_lightups(&mut self, files: &Storage) {
        for c in &mut self.controls {
            if c.ty != 14 {
                continue;
            }
            let (l, t) = (c.rect.0, c.rect.1);
            c.hot = (c.hot.0 + l, c.hot.1 + t, c.hot.2 + l, c.hot.3 + t);
            let mut extra = c.flags >> 9 & 0x10;
            for (i, a) in c.anims.iter_mut().enumerate() {
                if i > 0 && a.flags & 2 != 0 {
                    a.alpha = 0x1e;
                }
                a.flags |= extra;
                extra = 0x10;
                if a.path.is_empty() {
                    continue;
                }
                let path = String::from_utf8_lossy(&a.path).into_owned();
                let Some(mut smk) = files.read(&path).ok().and_then(Smk::parse) else { crate::bw::fatal_error(&path) };
                smk.next_frame();
                a.video = Some(Rc::new(RefCell::new(smk)));
            }
            c.align = (c.flags >> 0x14) as u8;
        }
    }

    /// The 30 ms timer 0x4e0f50 of the light-up buttons, for `ms` elapsed: plays their
    /// videos (a finished one is dropped), then 0x4e0e40 moves the light levels.
    pub fn tick(&mut self, ms: u32) {
        // 0x416600: a focused edit's caret toggles, 500 ms shown and 250 ms hidden.
        for c in &mut self.controls {
            let Some(left) = c.edit.as_mut().and_then(|e| e.blink.as_mut()) else { continue };
            let mut ms = ms;
            while ms >= *left {
                ms -= *left;
                c.flags ^= 0x4000_0000;
                *left = if c.flags & 0x4000_0000 != 0 { 500 } else { 250 };
            }
            *left -= ms;
        }
        self.timer += ms;
        let (ox, oy) = self.origin();
        let (mx, my) = (self.mouse.0 - ox, self.mouse.1 - oy);
        while self.timer >= 30 {
            self.timer -= 30;
            for c in self.controls.iter_mut().filter(|c| c.ty == 14 && c.flags & flag::VISIBLE != 0) {
                c.anims.retain_mut(|a| {
                    let Some(v) = a.video.as_ref() else { return false };
                    let mut v = v.borrow_mut();
                    a.elapsed += 30;
                    while a.elapsed >= v.ms.max(1) {
                        a.elapsed -= v.ms.max(1);
                        if !v.next_frame() {
                            if a.flags & 4 == 0 {
                                return false;
                            }
                            v.rewind();
                            v.next_frame();
                        }
                    }
                    true
                });
                // 0x4e0e40.
                let (hl, ht, hr, hb) = c.hot;
                let lit = c.flags & 0x80 != 0 && (hl..=hr).contains(&mx) && (ht..=hb).contains(&my);
                if lit {
                    c.hover = (c.hover + 15).min(100);
                } else if c.hover > 0 {
                    c.hover = (c.hover - 15).max(0);
                }
                for a in c.anims.iter_mut().filter(|a| a.flags & 1 != 0) {
                    let target = if lit {
                        100
                    } else if a.flags & 2 != 0 {
                        30
                    } else {
                        100
                    };
                    let al = a.alpha;
                    if lit {
                        if al < 100 {
                            a.alpha = (al + 15).min(100);
                        }
                    } else if al < target {
                        a.alpha = if al + 15 < target { al + 15 } else { target };
                    } else if al > target {
                        a.alpha = if target < al - 15 { al - 15 } else { target };
                    }
                }
            }
        }
    }
}

/// The graphics dialogs draw with (0x4197b0 list entry, made current by 0x419290).
pub struct Skin {
    /// dlgs\<race>.grp.
    pub grp: Grp,
    /// dlgs\tile.grp: in-game menu backgrounds (0x4c9cc0).
    pub tile: Grp,
    /// 0x6ceb20: the 26 interface colors as palette indices (they depend on the
    /// tileset's palette; set before drawing).
    pub colors: std::cell::Cell<[u8; 26]>,
    /// font8, font10, font16, font16x (0x6ce0f4..0x6ce100).
    pub fonts: [Font; 4],
    /// tfontgam.pcx rows.
    pub text_rows: Vec<u8>,
    /// The translucency tables of the glue palette (0x4178b0); None in game.
    pub fx: Option<Effects>,
}

/// 0x417c20: the light level (0..=7) of a percentage (0x6d09b8).
pub fn level(percent: i32) -> u8 {
    (percent.clamp(0, 100) * 7 / 100) as u8
}

/// The three blend tables 0x6d0590 (0x4178b0 with no excluded colors) and the shade
/// color 0x51a18c the glue's translucent images darken toward.
pub struct Effects {
    /// 0x6d0590[p - 1]: `t[i * 256 + j]` is the palette color nearest to
    /// (j * p + i * (8 - p)) / 8 for p = 1..=3.
    pub tables: [Vec<u8>; 3],
    pub shade: u8,
    /// 0x6d5e24: the light percentage translucent image controls (type 5) use.
    pub image_pct: i32,
}

/// 0x51b2f0: the 26 neighbor cells 0x4174a0 searches.
const NEIGHBORS: [[i32; 3]; 26] = [
    [-1, 0, 0], [0, -1, 0], [0, 0, -1], [1, 0, 0], [0, 1, 0], [0, 0, 1], [-1, 1, 0], [-1, 0, 1], [1, -1, 0], [0, -1, 1], [1, 0, -1],
    [0, 1, -1], [-1, -1, 0], [-1, 0, -1], [0, -1, -1], [1, 1, 0], [1, 0, 1], [0, 1, 1], [-1, -1, 1], [-1, 1, -1], [1, -1, -1], [1, 1, -1],
    [1, -1, 1], [-1, 1, 1], [-1, -1, -1], [1, 1, 1],
];

impl Effects {
    /// 0x4178b0 (no excluded colors): the three tables by 0x4174a0 for p = 1..=3.
    pub fn new(pal: &[[u8; 3]; 256], shade: u8, image_pct: i32) -> Effects {
        Effects { tables: [1, 2, 3].map(|p| Self::table(pal, p)), shade, image_pct }
    }

    /// 0x4174a0: the nearest-color table for the blend fraction p / 8. The palette is
    /// bucketed in 18 x 18 x 18 cells of 16 levels (the lists newest first); a color is
    /// the nearest in its cell, or in the first neighbor cell with a match within 0x400.
    fn table(pal: &[[u8; 3]; 256], p: u32) -> Vec<u8> {
        let cell = |c: [u32; 3]| ((c[0] >> 4) + 1, (c[1] >> 4) + 1, (c[2] >> 4) + 1);
        let key = |x: i32, y: i32, z: i32| ((x * 18 + y) * 18 + z) as usize;
        let mut grid: Vec<Vec<u8>> = vec![Vec::new(); 18 * 18 * 18];
        for i in 0..256usize {
            if i == 0 || pal[i] != pal[i - 1] {
                let (x, y, z) = cell(pal[i].map(|c| c as u32));
                grid[key(x as i32, y as i32, z as i32)].push(i as u8);
            }
        }
        let mut t = vec![0u8; 0x10000];
        for j in 0..256 {
            t[j] = j as u8;
            t[j * 256] = j as u8;
        }
        for i in 1..256usize {
            for j in 1..256usize {
                let b: [u32; 3] = std::array::from_fn(|k| (pal[j][k] as u32 * p + pal[i][k] as u32 * (8 - p)) >> 3);
                let dist = |c: u8| -> u32 { (0..3).map(|k| (pal[c as usize][k] as i32 - b[k] as i32).pow(2) as u32).sum() };
                let (cx, cy, cz) = cell(b);
                let (cx, cy, cz) = (cx as i32, cy as i32, cz as i32);
                let mut best = u32::MAX;
                let mut out = i as u8;
                let head = &grid[key(cx, cy, cz)];
                if head.len() == 1 {
                    out = head[0];
                } else if !head.is_empty() {
                    for &c in head.iter().rev() {
                        let d = dist(c);
                        if d < best {
                            best = d;
                            out = c;
                        }
                    }
                } else {
                    for n in NEIGHBORS {
                        if best < 0x401 {
                            break;
                        }
                        for &c in grid[key(cx + n[0], cy + n[1], cz + n[2])].iter().rev() {
                            let d = dist(c);
                            if d < best {
                                best = d;
                                out = c;
                            }
                        }
                    }
                }
                t[i * 256 + j] = out;
            }
        }
        t
    }

    /// 0x417a10's per-color step: `base` lit toward `target` at light `level`.
    pub fn mix(&self, level: u8, base: u8, target: u8) -> u8 {
        match level {
            0 => base,
            7.. => target,
            1 | 2 => self.tables[level as usize - 1][base as usize * 256 + target as usize],
            _ => self.tables[if level < 4 { level as usize - 1 } else { 6 - level as usize }][target as usize * 256 + base as usize],
        }
    }

    /// 0x416f30: the remap a translucent blit at `level` applies to the pixels it covers
    /// (the shade color blended over each).
    pub fn shade_remap(&self, level: u8) -> [u8; 256] {
        let sh = self.shade as usize;
        std::array::from_fn(|k| match level {
            0 => 0,
            1 | 2 => self.tables[level as usize - 1][k * 256 + sh],
            7.. => k as u8,
            _ => self.tables[if level < 4 { level as usize - 1 } else { 6 - level as usize }][sh * 256 + k],
        })
    }
}

/// An 8-bit surface.
pub struct Surf<'a> {
    pub px: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

impl Skin {
    pub(crate) fn frame_wh(&self, f: u16) -> (i32, i32) {
        self.grp.frame(f as usize).map_or((0, 0), |f| (f.w, f.h))
    }

    /// The font a control's flags pick (0x41fb30 with 0x6ce0f4 / f8 / fc / 100).
    pub fn font_of(&self, c: &Ctl) -> &Font {
        &self.fonts[match c.flags & 0x4c00 {
            0x400 => 0,
            0x800 => 2,
            0x4000 => 3,
            _ => 1,
        }]
    }

    /// 0x41b140 (a list or combo box's event 10; a combo then 0x41a110): no rows, the row
    /// height (the font's, at least the height of frame 0x35: 0x6d0500, then flag 0x20000000), the layout.
    pub fn list_init(&self, c: &mut Ctl) {
        let fh = self.font_of(c).height;
        let min = self.frame_wh(0x35).1;
        let row_h = if fh < min {
            c.flags |= 0x2000_0000;
            min
        } else {
            c.flags &= !0x2000_0000;
            fh
        };
        c.list = Some(ListBox { row_h: row_h as u8, parent_bottom: 479, ..Default::default() });
        c.list_layout();
        // 0x6d04c8 + 2 * 0x6d04f4: lists taller than a scroll bar's arrows and thumb get
        // one (0x414c90, id - 0x8000), shown by 0x41b300 once the rows overflow.
        self.list_scroll_create(c);
        if c.ty == 13 {
            // 0x41bc40 -> 0x41a110: the closed rect is one row high, below the saved top.
            let l = c.list.as_mut().unwrap();
            l.saved_top = c.rect.1;
            c.rect.3 = c.rect.1 + row_h;
            c.flags |= 0x20000;
        }
    }

    /// 0x41a580 (a combo box's event 4, the left button down): the rect grows by the rows
    /// (down, or up when 0x41b410 found no room) and a bit more for the frame, and the
    /// combo is open (flag 0x100000; the native keeps it in 0x6d04b0 for 0x41a530 /
    /// 0x44fd30 / 0x471860 to close). Not drawn last natively: drawn in control order.
    pub fn combo_open(&self, c: &mut Ctl) {
        let Some(l) = c.list.as_ref() else { return };
        let (a, b) = if c.flags & 0x2000_0000 != 0 {
            (self.frame_wh(0x2f).1, self.frame_wh(0x23).1)
        } else {
            (self.frame_wh(0x47).1, self.frame_wh(0x3b).1)
        };
        let ext = l.row_h as i32 * l.items.len() as i32 + (a - b);
        if l.up & 1 == 0 {
            c.rect.3 += ext;
        } else {
            c.rect.1 -= ext;
        }
        c.flags = (c.flags | 1) & !0x4000_0000 | 0x100000;
    }

    /// 0x41bc40 (a combo box's event proc), the parts before it hands the event to the
    /// list box proc 0x41baa0: event 4 opens, event 5 closes (0x41a4e0), event 0xe code 6
    /// closes (0x41a140), code 0 with no rows disables (0x418640), code 10 initializes.
    /// Mouse up then selects: 0x41a490 sends event 0xe code 0xb with the highlighted row
    /// when it is a row other than the selected one (`Ctl::list_select`).
    pub fn combo_proc(&self, c: &mut Ctl, event: u16, code: u32) {
        match (event, code) {
            (4, _) => self.combo_open(c),
            (5, _) | (0xe, 6) => c.combo_close(),
            (0xe, 0) if c.list.as_ref().is_some_and(|l| l.items.is_empty()) => {
                // 0x418640
                if c.flags & flag::DISABLED == 0 {
                    c.flags = c.flags & !0x80 | flag::DISABLED;
                }
            }
            (0xe, 10) => self.list_init(c),
            _ => {}
        }
    }

    /// 0x416450 (an edit's event 10): empty, caret 0, insert mode.
    pub fn edit_init(&self, c: &mut Ctl) {
        c.text.clear();
        c.edit = Some(Edit { mode: 0x10, ..Default::default() });
    }

    /// 0x41d160 sizes the 0x4197b0 entry keeps: max(w, h) of frame 10 (option box).
    fn option_size(&self) -> i32 {
        let (w, h) = self.frame_wh(10);
        w.max(h)
    }

    /// max(w, h) of frame 15 (check box), 0x6d0528.
    fn check_size(&self) -> i32 {
        let (w, h) = self.frame_wh(15);
        w.max(h)
    }

    /// 0x4ef600: dialog grp frame `f` at (x, y) (plus the frame's offsets), clipped to
    /// `clip` (left, top, right, bottom inclusive).
    pub(crate) fn frame(&self, f: u8, x: i32, y: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(fr) = self.grp.frame(f as usize) else { return };
        let (fx, fy) = (x + fr.x, y + fr.y);
        let (l, t) = (fx.max(clip.0), fy.max(clip.1));
        let (r, b) = ((fx + fr.w).min(clip.2 + 1), (fy + fr.h).min(clip.3 + 1));
        if l >= r || t >= b {
            return;
        }
        let (gw, gh) = (self.grp.w, self.grp.h);
        let mut tmp = vec![0u8; (gw * gh) as usize];
        self.grp.draw(f as usize, false, 0, 0, &mut tmp, gw, gh, Blend::Normal);
        for py in t..b {
            for px in l..r {
                let c = tmp[((py - y) * gw + px - x) as usize];
                if c != 0 && px >= 0 && py >= 0 && px < s.w && py < s.h {
                    s.px[(py * s.w + px) as usize] = c;
                }
            }
        }
    }

    /// 0x4e1c70: a one-pixel rectangle outline.
    fn outline(&self, (l, t, r, b): (i32, i32, i32, i32), color: u8, s: &mut Surf) {
        let mut put = |x: i32, y: i32| {
            if x >= 0 && y >= 0 && x < s.w && y < s.h {
                s.px[(y * s.w + x) as usize] = color;
            }
        };
        for x in l..=r {
            put(x, t);
            put(x, b);
        }
        for y in t..=b {
            put(l, y);
            put(r, y);
        }
    }

    /// 0x4ef870: a control's text in `rect` at offset (dx, dy) with text flags `align`
    /// (0x6ce110). Font by the flags, color by state and control type.
    #[allow(clippy::too_many_arguments)]
    pub fn ctl_text(&self, c: &Ctl, text: &[u8], rect: (i32, i32, i32, i32), dx: i32, dy: i32, align: u8, s: &mut Surf) {
        let font = &self.fonts[match c.flags & 0x4c00 {
            0x400 => 0,
            0x800 => 2,
            0x4000 => 3,
            _ => 1,
        }];
        let f = c.flags;
        let color = if f & flag::DISABLED != 0 {
            5
        } else if f & 0x80 != 0 {
            4
        } else {
            match c.ty {
                1 | 2 | 3 | 13 => 3,
                9..=11 if f & 0x8000_0000 != 0 => 4,
                9..=11 if f & 0x4000_0000 != 0 => 6,
                9..=11 if f & 0x2000_0000 != 0 => 3,
                _ => 2,
            }
        };
        let text = if f & flag::HOTKEY != 0 && !text.is_empty() { &text[1..] } else { text };
        let o = TextOpts { flags: align, spacing: if f & flag::NO_SPACING != 0 { 0 } else { 1 }, line_gap: 3, clip: rect };
        // Bytes as stored: 0x80.. are the font's own glyphs (the caret 0xa6), not UTF-8.
        font.draw_bytes(text, rect.0 + dx, rect.1 + dy, color, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
    }

    /// 0x418440: the focus flag: set on the active control, or on a default button
    /// (type 1) when nothing is active and the dialog is not 0x20000000.
    fn focus(dlg: &Dialog, i: usize, c: &Ctl) -> bool {
        c.flags & flag::DISABLED == 0
            && c.flags & flag::VISIBLE != 0
            && (dlg.active == Some(i) || (c.ty == 1 && dlg.active.is_none() && dlg.this.flags & flag::WIDE == 0))
    }

    /// 0x418cd0-style redraw of a whole dialog: its background, then each visible
    /// control's default draw (table 0x501504).
    pub fn draw(&self, dlg: &Dialog, s: &mut Surf) {
        if dlg.this.flags & flag::VISIBLE == 0 {
            return;
        }
        let (ox, oy) = dlg.origin();
        if let Some((bw, bh, bg)) = &dlg.bg {
            // 0x4ef590 -> 0x4ef440: the dialog bitmap; index 0 is transparent. Flag
            // 0x40000 with the glue's effects: mode 4 (0x416f30), the shade color blends.
            if let Some(fx) = self.fx.as_ref().filter(|_| dlg.this.flags & 0x40000 != 0) {
                let remap = fx.shade_remap(level(fx.image_pct));
                let r = (ox, oy, ox + bw - 1, oy + bh - 1);
                self.blit_shade(bg, *bw, *bh, ox, oy, r, fx.shade, &remap, s);
            } else {
            for y in 0..*bh {
                for x in 0..*bw {
                    let c = bg[(y * bw + x) as usize];
                    let (px, py) = (ox + x, oy + y);
                    if c != 0 && px >= 0 && py >= 0 && px < s.w && py < s.h {
                        s.px[(py * s.w + px) as usize] = c;
                    }
                }
            }
            }
        }
        // An open combo (0x100000, 0x6d04b0) shows over the controls after it: the
        // native screens have its rows on top (its redraw comes after theirs); it is
        // drawn last here.
        let open = |c: &Ctl| c.ty == 13 && c.flags & 0x100000 != 0;
        let order = dlg.controls.iter().enumerate().filter(|(_, c)| !open(c)).chain(dlg.controls.iter().enumerate().filter(|(_, c)| open(c)));
        for (i, c) in order {
            if c.flags & flag::VISIBLE == 0 {
                continue;
            }
            if c.ty == 14 {
                self.lightup(dlg, c, dlg.abs(c), s);
                continue;
            }
            let mut c = c.clone();
            if Self::focus(dlg, i, &c) {
                c.flags |= flag::FOCUS;
            } else {
                c.flags &= !flag::FOCUS;
            }
            let r = dlg.abs(&c);
            self.draw_ctl(&c, r, s);
            // A list's scroll bar child (0x414c90) is its own control, drawn after it.
            if c.list.as_ref().is_some_and(|l| l.scroll.is_some()) {
                self.list_scroll_draw(&c, r, s);
            }
        }
    }

    /// One control's draw (+0x2e, by default 0x501504[type]) at screen rect `r`.
    pub fn draw_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let f = c.flags;
        let (l, t, rr, b) = r;
        if c.update == Update::Tip {
            let font = &self.fonts[match c.flags & 0x4c00 {
                0x400 => 0,
                0x800 => 2,
                0x4000 => 3,
                _ => 1,
            }];
            let o = TextOpts { flags: 0x11, spacing: 1, line_gap: 0, clip: r };
            font.draw_bytes(&c.text, l, t, 2, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
            return;
        }
        if c.update == Update::MenuButton {
            // 0x4f4fb0
            let fr = c.graphic as u8 + if f & flag::DISABLED != 0 { 0 } else if f & flag::PRESSED != 0 { 2 } else { 1 };
            self.frame(fr, l, t, r, s);
            if !c.text.is_empty() {
                let d = if f & flag::PRESSED != 0 { 0 } else { 3 };
                self.ctl_text(c, &c.text, r, d, d, 0x22, s);
            }
            return;
        }
        match (c.update, c.ty) {
            (Update::MapList, 12) => return self.map_list_ctl(c, r, s),
            (Update::DirLabel, _) => return self.dir_label_ctl(c, r, s),
            (Update::Score, _) => return self.score_ctl(c, r, s),
            (Update::BriefText, _) => {
                if let Some(p) = c.pic.as_ref() {
                    let rows = (p.h - p.scroll).min(b - t + 1).max(0);
                    let from = (p.scroll.max(0) * p.w) as usize;
                    self.blit(p.px.get(from..).unwrap_or(&[]), p.w, rows, l, t, r, None, true, s);
                }
                return;
            }
            (Update::Portrait, _) => {
                if let Some(p) = c.pic.as_ref() {
                    self.blit(&p.px, p.w, p.h, l + p.dx, t + p.dy, r, None, false, s);
                }
                return;
            }
            (Update::Credits(align), _) => {
                let mut c = c.clone();
                c.flags |= 0x8000_0000;
                return self.ctl_text(&c, &c.text, r, 0, 0, align, s);
            }
            _ => {}
        }
        match c.ty {
            1 | 2 if f & flag::WIDE != 0 => {
                // 0x4efeb0
                let base = c.graphic as u8 + if f & flag::DISABLED != 0 { 0 } else if f & flag::PRESSED != 0 { 6 } else { 3 };
                self.wide(base, l, t, rr - l + 1, r, s);
                if f & flag::FOCUS != 0 {
                    self.outline(r, self.colors.get()[0x10], s);
                }
                if !c.text.is_empty() {
                    self.ctl_text(c, &c.text, r, 0, 0, 0x22, s);
                }
            }
            1 | 2 => {
                // 0x4efd90 (no 0x20000 = draw the frame)
                if f & flag::NO_OUTLINE == 0 {
                    let fr = c.graphic as u8 + if f & flag::DISABLED != 0 { 0 } else if f & flag::PRESSED != 0 { 2 } else { 1 };
                    self.frame(fr, l, t, r, s);
                }
                if c.ty == 1 {
                    self.outline(r, self.colors.get()[5], s);
                }
                if f & flag::FOCUS != 0 {
                    self.outline(r, self.colors.get()[0x10], s);
                }
                if !c.text.is_empty() {
                    let d = (f >> 0x1d & 2) as i32;
                    self.ctl_text(c, &c.text, r, d, d, 0x22, s);
                }
            }
            3 => {
                // 0x4efc30: option button
                let size = self.option_size();
                let x = if f & flag::BOX_RIGHT != 0 { rr - size } else { l };
                let fr = if f & flag::DISABLED != 0 { 6 } else if f & flag::PRESSED != 0 { (c.checked as u8 + 4) * 2 } else { c.checked as u8 * 2 + 7 };
                self.frame(fr, x, t + (b - size - t) / 2, r, s);
                self.box_text(c, r, size, s);
            }
            4 => {
                // 0x4efa80: check box
                let size = self.check_size();
                let x = if f & flag::BOX_RIGHT != 0 { rr - size } else { l };
                let y = t + (b - size - t) / 2;
                let g = c.graphic as u8;
                let fr = if f & flag::DISABLED != 0 {
                    if c.checked { 0x8b } else { g }
                } else if f & flag::PRESSED != 0 {
                    (c.checked as u8 + 1) * 2 + g
                } else {
                    c.checked as u8 * 2 + g + 1
                };
                self.frame(fr, x, y, r, s);
                if f & flag::FOCUS != 0 {
                    self.outline((x, y, x + size - 1, y + size - 1), self.colors.get()[0x10], s);
                }
                self.box_text(c, r, size, s);
            }
            5 => self.image_ctl(c, r, s),
            8 => self.edit_ctl(c, r, s),
            12 => self.list_ctl(c, r, s),
            13 => self.combo_ctl(c, r, s),
            6 => self.hscroll(c, r, s),
            9..=11 if !c.text.is_empty() => {
                self.ctl_text(c, &c.text, r, 0, 0, [0x11, 0x12, 0x14][c.ty as usize - 9], s);
            }
            _ => {}
        }
    }

    /// An icon (frame of iCreate.grp, 0x4ef440 mode 1: its own box at (x, y), the frame's
    /// offsets are not used) clipped.
    fn icon(&self, i: &Icon, x: i32, y: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        self.blit(&i.px, i.w, i.h, x, y, clip, None, true, s);
    }

    /// 0x4ad6b0 for each row (the list box's draw): the folder (frames 0 / 1) or map icon
    /// (3 / 4) of its node (the data's top byte is the node's flags, 8 a folder), lit when the
    /// row is the highlighted one, centered in the row; the text after it (color 3, 4
    /// lit), cut to what fits.
    fn map_list_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(l) = c.list.as_ref() else { return };
        let font = self.font_of(c);
        let rh = l.row_h as i32;
        let mut top = r.1;
        let mut row = l.top;
        for _ in 0..l.rows.min(l.items.len() as u8) {
            if let Some(item) = l.items.get(row as usize) {
                let lit = row == l.hover;
                let frame = if item.2 >> 24 == 8 { lit as usize } else { 3 + lit as usize };
                // 0x41af10 gives 0x4ad6b0 the row's left + 2 and its own rect.
                let mut dx = 2;
                if let Some(i) = c.icons.get(frame) {
                    let y = top + (rh + 1) / 2 - i.h / 2;
                    self.icon(i, r.0 + dx, y, r, s);
                    dx += i.w + 2;
                }
                let x0 = r.0 + dx;
                let mut t: Vec<u8> = Vec::new();
                let avail = r.2 - x0 + 1;
                for &ch in item.0.iter().take(0xff) {
                    if font.width_bytes(&[t.as_slice(), &[ch]].concat()) > avail {
                        break;
                    }
                    t.push(ch);
                }
                let o = TextOpts { flags: 0x11, spacing: 1, line_gap: 3, clip: (x0, top, r.2, top + rh) };
                font.draw_bytes(&t, x0, top, if lit { 4 } else { 3 }, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
            }
            top += rh;
            row = row.wrapping_add(1);
        }
    }

    /// 0x4ad980: the folder label: the folder icon (frame 2) centered in the label, the text
    /// after it.
    fn dir_label_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let mut dx = 0;
        if let Some(i) = c.icons.get(2) {
            let y = r.1 + (r.3 - r.1 + 1) / 2 - i.h / 2;
            self.icon(i, r.0, y, r, s);
            dx = i.w + 2;
        }
        if !c.text.is_empty() {
            self.ctl_text(c, &c.text, r, dx, 0, 0x11, s);
        }
    }

    /// The score screen's controls (glue_score.rs). 0x4b3820 (a tab: its picture unless it
    /// is the selected one, which is lit instead, then the centered text), 0x4b3b80 (the
    /// total's box and its text 2 px down), 0x4b38a0 (the race icon, 0x4b3600: frame
    /// table byte + 3 of iScore.grp drawn 8 px in and 0x14 down with 0x4ef440 mode 1 (0x2000),
    /// framed in the player's color, the name, and the team name beside the icon in
    /// font 0 color 6) and 0x4b3a40 (the bar and its number).
    fn score_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(k) = c.score.as_ref() else { return };
        match k {
            ScoreCtl::Tab => {
                let mut t = c.clone();
                t.flags &= !0x80;
                if t.flags & 0x8000_0000 != 0 {
                    t.flags |= 0x80;
                } else if let Some(img) = c.image.as_ref() {
                    self.blit(&img.px, img.w, img.h, r.0, r.1, r, None, true, s);
                }
                if !c.text.is_empty() {
                    self.ctl_text(&t, &t.text, r, 0, 0, 0x22, s);
                }
            }
            ScoreCtl::Box => {
                if let Some(img) = c.image.as_ref() {
                    self.blit(&img.px, img.w, img.h, r.0, r.1, r, None, true, s);
                }
                if !c.text.is_empty() {
                    self.ctl_text(c, &c.text, r, 0, 2, 0x12, s);
                }
            }
            ScoreCtl::Name { gfx, race, color, aux } => {
                let Some(icon) = gfx.get(*race as usize + 3) else { return };
                self.icon(icon, r.0 + 8, r.1 + 0x14, r, s);
                self.outline((r.0 + 8, r.1 + 0x14, r.0 + 8 + icon.w - 1, r.1 + 0x14 + icon.h - 1), *color, s);
                // 0x51a0e8 = 0: no word wrap; the rect's right edge is 0xb8.
                if !c.text.is_empty() {
                    self.ctl_text(c, &c.text, (r.0, r.1, 0xb8, r.3), 0, 0, 0x11, s);
                }
                if !aux.is_empty() {
                    let o = TextOpts { flags: 0x11, spacing: 1, line_gap: 3, clip: r };
                    self.fonts[0].draw_bytes(aux, icon.w + r.0 + 0x28, r.1 + 0x14, 6, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
                }
            }
            ScoreCtl::Bar { gfx, value, max, color } => {
                // The width the bar fills: width * value / max, at most the width, at least 1
                // for a value.
                let width = (r.2 - r.0 + 1) as u32;
                let fill = if *max == 0 { 0 } else { (width * *value as u32 / *max as u32).min(width) };
                let fill = if *value != 0 && fill == 0 { 1 } else { fill } as i32;
                let (back, front) = (*color as usize * 6 + 9, *color as usize * 6 + 6);
                let pw = self.frame_wh(0x85).0;
                if width != 0 {
                    self.score_wide(gfx, back, pw, r.0, r.1, width as i32, r, s);
                }
                if fill != 0 {
                    self.score_wide(gfx, front, pw, r.0, r.1, fill, r, s);
                }
                if !c.text.is_empty() {
                    self.ctl_text(c, &c.text, r, 0, 2, 0x12, s);
                }
            }
        }
    }

    /// 0x4ef710 with iScore.grp's frames (`gfx`) and the piece width `pw`: `wide_pw`.
    #[allow(clippy::too_many_arguments)]
    fn score_wide(&self, gfx: &[Icon], base: usize, pw: i32, x: i32, y: i32, width: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        let pw = pw.max(1);
        let right = x + width - 1;
        let mut f = |k: usize, fx: i32, c: (i32, i32, i32, i32)| {
            if let Some(i) = gfx.get(k) {
                self.icon(i, fx + i.x, y + i.y, c, s);
            }
        };
        let mid = (clip.0, clip.1, clip.2.min(right - pw), clip.3);
        f(base, x, mid);
        for k in 1..=(width - pw - 1) / pw {
            f(base + 1, x + k * pw, mid);
        }
        if pw <= width {
            f(base + 2, x - pw + width, (clip.0.max(x + pw), clip.1, clip.2, clip.3));
        }
    }

    /// 0x41b950 -> 0x41af10 per row: a list box's rows from the top row, in their color
    /// (the highlighted row 4), cut with "..." (0x51b2cc) when wider than the row.
    fn list_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(l) = c.list.as_ref() else { return };
        if c.flags & 0x20000 == 0 {
            self.list_frame(c, r, s);
        }
        let rh = l.row_h as i32;
        let mut top = r.1;
        let mut row = l.top;
        for _ in 0..l.rows.min(l.items.len() as u8) {
            self.list_row(c, row, (r.0, top, r.2, top + rh), 2, 0, s);
            top += rh;
            row = row.wrapping_add(1);
        }
    }

    /// 0x4f02c0: the frame of a list box: an inner outline (0x6ceb30 with the focus flag
    /// 0x1000, else 0x6ceb2f) and the outer one (0x6ceb2c).
    fn list_frame(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let col = self.colors.get();
        let inner = col[if c.flags & 0x1000 != 0 { 0x10 } else { 0xf }];
        self.outline((r.0 + 1, r.1 + 1, r.2 - 1, r.3 - 1), inner, s);
        self.outline(r, col[0xc], s);
    }

    /// 0x41af10 (the row callback at +0x52; `dx`, `dy` the text offset): row `row` in
    /// `rect`: its background (frames 0x4d.., or 0x35.. with flag 0x20000000) unless the
    /// control has flag 0x20000, then its text, cut with "..." when wider than the rect.
    #[allow(clippy::too_many_arguments)]
    fn list_row(&self, c: &Ctl, row: u8, rect: (i32, i32, i32, i32), dx: i32, dy: i32, s: &mut Surf) {
        let Some(l) = c.list.as_ref() else { return };
        if l.items.is_empty() {
            return;
        }
        let font = self.font_of(c);
        let color = if row == l.hover && c.flags & 0x1000_0000 == 0 {
            4
        } else {
            match l.items.get(row as usize).map_or(0, |i| i.1 & 0xf) {
                0 => 3,
                x => x,
            }
        };
        if c.flags & 0x20000 == 0 {
            let (base, pw) = if c.flags & 0x2000_0000 != 0 { (0x35, self.frame_wh(0x35).0) } else { (0x4d, self.frame_wh(0x4d).0) };
            self.wide_pw(base, pw, rect.0, rect.1, rect.2 - rect.0 + 1, rect, s);
        }
        if let Some(item) = l.items.get(row as usize) {
            let x0 = rect.0 + dx;
            let y0 = rect.1 + dy;
            let mut t: Vec<u8> = item.0.iter().copied().take(0xff).collect();
            let avail = rect.2 - x0;
            if avail < font.width_bytes(&t) {
                let lim = avail - font.width_bytes(b"...");
                while !t.is_empty() && lim < font.width_bytes(&t) {
                    t.pop();
                }
                t.extend_from_slice(b"...");
            }
            let o = TextOpts { flags: 0x11, spacing: 1, line_gap: 3, clip: (x0, y0, rect.2, rect.3) };
            font.draw_bytes(&t, x0, y0, color, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
        }
    }

    /// 0x41b110 (a combo box's draw): the closed combo (0x41ac50), then the drop-down
    /// rows (0x41a6d0) while it is open (flag 0x100000).
    fn combo_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        self.combo_body(c, r, s);
        if c.flags & 0x100000 != 0 {
            self.combo_rows(c, r, s);
        }
    }

    /// 0x41ac50: the combo box's body at its saved top (one 0x2f high): the three-piece
    /// background (frames 0x4d.. closed, 0x50.. lit by flag 0x80, 0x47../0x44.. open down
    /// / up; with flag 0x20000000 0x35.., 0x38.., 0x2f.., 0x2c..), the arrow button (0x4a,
    /// 0x4b lit, 0x4c disabled; 0x32, 0x33, 0x34) and the selected row's text.
    fn combo_body(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(l) = c.list.as_ref() else { return };
        let f = c.flags;
        let (open, up, lit, dis) = (f & 0x100000 != 0, l.up & 1 != 0, f & 0x80 != 0, f & 2 != 0);
        let (btn, base, pw, bw) = if f & 0x2000_0000 == 0 {
            let (mut btn, mut base) = (0x4a, 0x4d);
            if open {
                base = if up { 0x44 } else { 0x47 };
            } else if lit {
                (btn, base) = (0x4b, 0x50);
            } else if dis {
                btn = 0x4c;
            }
            (btn, base, self.frame_wh(0x3b).0, self.frame_wh(0x4a).0)
        } else {
            let (mut btn, mut base) = (0x32, 0x35);
            if open {
                base = if up { 0x2c } else { 0x2f };
            } else if lit {
                (btn, base) = (0x33, 0x38);
            } else if dis {
                btn = 0x34;
            }
            (btn, base, self.frame_wh(0x23).0, self.frame_wh(0x32).0)
        };
        let bw = bw + 4;
        // The rect is the saved top (+0x50) and the frame's height (0x6d04bc, frame 0x2f).
        let top = l.saved_top + (r.1 - c.rect.1);
        let bottom = top + self.frame_wh(0x2f).1;
        self.wide_pw(base, pw, r.0, top, r.2 - r.0 + 1, r, s);
        self.frame(btn, r.2 - bw, top + 4, r, s);
        if l.items.is_empty() || l.sel == 0xff {
            return;
        }
        // The text is drawn with the control's text pointer on the selected row and flag
        // 2 clear, align 0x11 (0x4ef870, offset 4, 0).
        let mut t = c.clone();
        t.flags &= !2;
        let text = l.items[l.sel as usize].0.clone();
        self.ctl_text(&t, &text, (r.0, top, r.2 - bw, bottom), 4, 0, 0x11, s);
    }

    /// 0x41a6d0: the open combo box's drop-down: from the button's side a background piece
    /// (edge frame 0x41 / 0x3b, then 0x3e; with flag 0x20000000 0x29 / 0x23, then 0x26)
    /// and the row's text per row, last row at the bottom when it opens down, row 0 at the
    /// top when it opens up.
    fn combo_rows(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(l) = c.list.as_ref() else { return };
        let (first_down, first_up, mid, pw) = if c.flags & 0x2000_0000 != 0 {
            (0x29, 0x23, 0x26, self.frame_wh(0x23).0)
        } else {
            (0x41, 0x3b, 0x3e, self.frame_wh(0x3b).0)
        };
        let rh = l.row_h as i32;
        let n = l.items.len() as i32;
        let (mut top, step, mut row, first) = if l.up & 1 == 0 { (r.3 - rh, -rh, n - 1, first_down) } else { (r.1, rh, 0, first_up) };
        let rstep = step.signum();
        for k in 0..n.max(1) {
            let rect = (r.0, top, r.2, top + rh);
            self.wide_pw(if k == 0 { first } else { mid }, pw, r.0, top, r.2 - r.0 + 1, r, s);
            self.list_row(c, row as u8, rect, 4, 0, s);
            row += rstep;
            top += step;
        }
    }

    /// 0x4efa00 -> 0x4167f0: an edit's text from its first shown character, the caret
    /// (code 4, glyph 0xa6, code 1: 0x50612c) at its place while it blinks on; the focus
    /// outline (0x6ceb20) unless flag 0x20000, and 0x6ceb30 with flag 0x80000000.
    fn edit_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(e) = c.edit.as_ref() else { return };
        let font = self.font_of(c);
        let focus = c.flags & flag::FOCUS != 0;
        if c.flags & 0x20000 == 0 && focus {
            self.outline(r, self.colors.get()[0], s);
        }
        let first = (e.first as usize).min(c.text.len());
        let mut t = c.text[first..].to_vec();
        if e.blink.is_some() && c.flags & 0x4000_0000 != 0 {
            let at = (e.caret as usize).saturating_sub(first).min(t.len());
            t.splice(at..at, [4, 0xa6, 1]);
        }
        let color = match e.mode >> 4 {
            0 => 2,
            x => x,
        };
        // 0x51a0e8 = 0 (no word wrap): one line.
        let o = TextOpts { flags: 0x11, spacing: 1, line_gap: 3, clip: r };
        font.draw_bytes(&t, r.0 + e.margin as i32, r.1 + e.dy as i32, color, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
        if c.flags & 0x8000_0000 != 0 && focus {
            self.outline(r, self.colors.get()[0x10], s);
        }
    }

    /// 0x4ef560 -> 0x4ef440: an image control's picture at the control's corner, clipped
    /// to the control: flag 0x40000 darkens what it covers (mode 4, the shade color
    /// blended over the pixels under the picture), else flag 0x2000 skips color 0.
    fn image_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(img) = c.image.as_ref() else { return };
        if c.flags & 0x40000 == 0 {
            return self.blit(&img.px, img.w, img.h, r.0, r.1, r, None, c.flags & 0x2000 != 0, s);
        }
        if c.flags & 0x2000000 != 0 {
            todo_port!("0x416f30", "mode 5: the second shade 0x51a194 / level 0x6d0c50");
        }
        let Some(fx) = self.fx.as_ref() else { return };
        // 0x416f30 through table 0x6d05a8 (0x417270: row 0 keeps the screen, row s
        // writes s): only the shade color's row is the blend.
        let remap = fx.shade_remap(level(fx.image_pct));
        self.blit_shade(&img.px, img.w, img.h, r.0, r.1, r, fx.shade, &remap, s);
    }

    /// 0x4172f0 mode 4 (0x416f30): source 0 keeps the screen, the shade color `shade`
    /// darkens it through `remap`, any other color is drawn.
    #[allow(clippy::too_many_arguments)]
    fn blit_shade(&self, px: &[u8], w: i32, h: i32, x: i32, y: i32, clip: (i32, i32, i32, i32), shade: u8, remap: &[u8; 256], s: &mut Surf) {
        for sy in 0..h {
            let dy = y + sy;
            if dy < clip.1 || dy > clip.3 || dy < 0 || dy >= s.h {
                continue;
            }
            for sx in 0..w {
                let dx = x + sx;
                if dx < clip.0 || dx > clip.2 || dx < 0 || dx >= s.w {
                    continue;
                }
                let Some(&c) = px.get((sy * w + sx) as usize) else { continue };
                let d = &mut s.px[(dy * s.w + dx) as usize];
                if c == shade {
                    *d = remap[*d as usize];
                } else if c != 0 {
                    *d = c;
                }
            }
        }
    }

    /// 0x4172f0 (modes 0, 1 and 4): `px` (w x h) at (x, y) clipped to `clip` (inclusive).
    /// With `remap` the covered pixels (source not 0) are replaced by their remap; else
    /// the source is copied, skipping color 0 when `keyed`.
    #[allow(clippy::too_many_arguments)]
    fn blit(&self, px: &[u8], w: i32, h: i32, x: i32, y: i32, clip: (i32, i32, i32, i32), remap: Option<&[u8; 256]>, keyed: bool, s: &mut Surf) {
        for sy in 0..h {
            let dy = y + sy;
            if dy < clip.1 || dy > clip.3 || dy < 0 || dy >= s.h {
                continue;
            }
            for sx in 0..w {
                let dx = x + sx;
                if dx < clip.0 || dx > clip.2 || dx < 0 || dx >= s.w {
                    continue;
                }
                let Some(&c) = px.get((sy * w + sx) as usize) else { continue };
                let d = &mut s.px[(dy * s.w + dx) as usize];
                match remap {
                    Some(m) if c != 0 => *d = m[*d as usize],
                    Some(_) => {}
                    None if c != 0 || !keyed => *d = c,
                    None => {}
                }
            }
        }
    }

    /// 0x4e1180: a light-up button: its videos (the ones flagged 8 only while lit), then
    /// its text (0x4e1020), lit toward the highlight color by the light level (0x417a10).
    fn lightup(&self, dlg: &Dialog, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let (ox, oy) = dlg.origin();
        let (mx, my) = (dlg.mouse.0 - ox, dlg.mouse.1 - oy);
        let (hl, ht, hr, hb) = c.hot;
        let lit = c.flags & 0x80 != 0 && (hl..=hr).contains(&mx) && (ht..=hb).contains(&my);
        for a in &c.anims {
            if !lit && a.flags & 8 != 0 {
                continue;
            }
            let Some(v) = a.video.as_ref() else { continue };
            if a.flags & 3 != 0 {
                todo_port!("0x417130", "translucent video blit by the fade level");
                continue;
            }
            let v = v.borrow();
            self.blit(&v.pixels, v.w as i32, v.h as i32, r.0 + a.x, r.1 + a.y, r, None, a.flags & 0x10 != 0, s);
        }
        if c.text.is_empty() {
            return;
        }
        let f = c.flags;
        let font = &self.fonts[match f & 0x4c00 {
            0x400 => 0,
            0x800 => 2,
            0x4000 => 3,
            _ => 1,
        }];
        let text: &[u8] = if f & flag::HOTKEY != 0 { &c.text[1..] } else { &c.text };
        let t = (c.trect.0 + ox, c.trect.1 + oy, c.trect.2 + ox, c.trect.3 + oy);
        let o = TextOpts { flags: c.align, spacing: if f & flag::NO_SPACING != 0 { 0 } else { 1 }, line_gap: 3, clip: t };
        let mut rows = self.text_rows.clone();
        let color = if f & flag::DISABLED != 0 {
            5
        } else {
            if let (Some(fx), true) = (self.fx.as_ref(), rows.len() >= 24) {
                let l = level(c.hover as i32);
                for k in 0..8 {
                    rows[8 + k] = fx.mix(l, self.text_rows[8 + k], self.text_rows[16 + k]);
                }
            }
            3
        };
        font.draw_bytes(text, t.0, t.1, color, &o, TextDst { rows: &rows, out: s.px, w: s.w, h: s.h });
    }

    /// The text beside a check / option box (0x21 left, or 0x24 right of a right box).
    fn box_text(&self, c: &Ctl, r: (i32, i32, i32, i32), size: i32, s: &mut Surf) {
        if c.text.is_empty() {
            return;
        }
        if c.flags & flag::BOX_RIGHT == 0 {
            self.ctl_text(c, &c.text, r, size + 4, 0, 0x21, s);
        } else {
            self.ctl_text(c, &c.text, r, 0, 0, 0x24, s);
        }
    }

    /// 0x4ef710 with the piece width 0x6d04e8 (frame 103's width), as the buttons use it.
    fn wide(&self, base: u8, x: i32, y: i32, width: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        self.wide_pw(base, self.frame_wh(0x67).0, x, y, width, clip, s);
    }

    /// 0x4ef710: a three-piece row from frames `base`, `base + 1` (repeated), `base + 2`,
    /// each `pw` (EDI) wide, over `width` pixels from (x, y).
    #[allow(clippy::too_many_arguments)]
    fn wide_pw(&self, base: u8, pw: i32, x: i32, y: i32, width: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        let pw = pw.max(1);
        let right = x + width - 1;
        self.frame(base, x, y, (clip.0, clip.1, clip.2.min(right - pw), clip.3), s);
        let n = (width - pw - 1) / pw;
        for k in 1..=n {
            self.frame(base + 1, x + k * pw, y, (clip.0, clip.1, clip.2.min(right - pw), clip.3), s);
        }
        if pw <= width {
            self.frame(base + 2, x - pw + width, y, (clip.0.max(x + pw), clip.1, clip.2, clip.3), s);
        }
    }

    /// 0x414dd0: the thumb's offset for the value over `range` pixels.
    fn thumb_pos(c: &Ctl, range: i32) -> i32 {
        let (v, lo, hi) = (c.value.0 as i32, c.value.1 as i32, c.value.2 as i32);
        if hi == lo || v == lo {
            0
        } else if v == hi {
            range
        } else {
            ((hi - lo) / 2 + (v - lo) * range) / (hi - lo)
        }
    }

    /// 0x414d30: a tick slider's thumb travel: width - frame 99's width + 1.
    pub fn slider_range(&self, r: (i32, i32, i32, i32)) -> i32 {
        r.2 - r.0 - self.frame_wh(0x63).0 + 1
    }

    /// 0x4153b0: the option dialogs' slider: a three-piece track (frames 0x5e.., 0x5b..
    /// disabled), tick marks (0x62, 0x61 disabled) and the thumb (+0x48, 99 disabled).
    fn tick_slider(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let (l, t, rr, _) = r;
        let f = c.flags;
        let dis = f & flag::DISABLED != 0;
        let pw = self.frame_wh(0x5c).0.max(1);
        let track = if dis { 0x5b } else { 0x5e };
        // 0x4ef710 with piece width 0x6d04dc.
        let right = l + rr - l;
        let n = (rr - l + 1 - pw - 1) / pw;
        self.frame(track, l, t, (r.0, r.1, r.2.min(right - pw), r.3), s);
        for k in 1..=n {
            self.frame(track + 1, l + k * pw, t, (r.0, r.1, r.2.min(right - pw), r.3), s);
        }
        if pw <= rr - l + 1 {
            self.frame(track + 2, l - pw + rr - l + 1, t, (r.0.max(l + pw), r.1, r.2, r.3), s);
        }
        let tick = if dis { 0x61 } else { 0x62 };
        let tw = self.frame_wh(0x63).0;
        let half = (self.frame_wh(0x61).0 + 1) / 2;
        self.frame(tick, (tw >> 1) + l, t, r, s);
        let steps = ((c.value.2 as i32 - c.value.1 as i32) / (c.step.max(1) as i32)).min(7);
        for i in 1..steps {
            self.frame(tick, (tw >> 1) + ((rr - l - tw + 1) * i) / steps - half + l, t, r, s);
        }
        self.frame(tick, (rr - half) - (tw >> 1), t, r, s);
        let thumb = if dis { 99 } else { c.thumb };
        self.frame(thumb, Self::thumb_pos(c, self.slider_range(r)) + l, t, r, s);
        if f & flag::FOCUS != 0 {
            self.outline(r, self.colors.get()[0x10], s);
        }
    }

    /// 0x4155a0: a horizontal scroll bar.
    fn hscroll(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let (l, t, rr, _) = r;
        let f = c.flags;
        if f & flag::TICK_SLIDER != 0 {
            return self.tick_slider(c, r, s);
        }
        let arrow = self.frame_wh(0x16).0;
        let piece = self.frame_wh(0x20).0.max(1);
        if (piece + arrow) * 2 > rr - l {
            return;
        }
        let en = (!(f >> 1) & 1) as u8;
        let left = if f & flag::PRESSED != 0 { en + 0x17 } else { en + 0x16 };
        self.frame(left, l, t, r, s);
        let mut x = arrow;
        self.frame(0x20, l + x, t, r, s);
        for _ in 0..((rr - arrow * 2 - l) / piece - 2) {
            x += piece;
            self.frame(0x21, l + x, t, r, s);
        }
        self.frame(0x22, l + x + piece, t, r, s);
        let right = if f & flag::WIDE != 0 { en + 0x1a } else { en + 0x19 };
        self.frame(right, l + x + piece + arrow, t, r, s);
        if f & flag::DISABLED == 0 {
            let (_, ah) = self.frame_wh(0x16);
            let (tw, th) = self.frame_wh(0x1c);
            let y = (ah - th) / 2 + t;
            // 0x414d30
            let range = rr - arrow * 2 - l - tw - 1;
            self.frame(0x1c, Self::thumb_pos(c, range) + arrow + l, y, r, s);
        }
        if f & flag::NO_OUTLINE == 0 && f & flag::FOCUS != 0 {
            self.outline(r, self.colors.get()[0x10], s);
        }
    }

    /// 0x4c9cc0: an in-game menu's background bitmap: dlgs\tile.grp's 3x3 frames (rows
    /// 0 / 3 / 6, columns +0 / +1 / +2) tiled over the dialog, its size rounded up to
    /// whole tiles.
    pub fn menu_background(&self, dlg: &mut Dialog) {
        let (tw, th) = (self.tile.w.max(1), self.tile.h.max(1));
        let (mut w, mut h) = (dlg.this.rect.2 - dlg.this.rect.0 + 1, dlg.this.rect.3 - dlg.this.rect.1 + 1);
        if w % tw != 0 {
            w += tw - w % tw;
        }
        if h % th != 0 {
            h += th - h % th;
        }
        let mut bm = vec![0u8; (w * h) as usize];
        // 0x4c8f10: one row of tiles; 0x40abbe blits frames without their offsets.
        let row = |bm: &mut Vec<u8>, base: usize, y: i32| {
            let put = |f: usize, x: i32, bm: &mut Vec<u8>| {
                let Some(fr) = self.tile.frame(f) else { return };
                self.tile.draw(f, false, x - fr.x, y - fr.y, bm, w, h, Blend::Normal);
            };
            put(base, 0, bm);
            let mut x = tw;
            while x < w - tw {
                put(base + 1, x, bm);
                x += tw;
            }
            put(base + 2, x, bm);
        };
        row(&mut bm, 0, 0);
        let mut y = th;
        while y < h - th {
            row(&mut bm, 3, y);
            y += th;
        }
        row(&mut bm, 6, y);
        dlg.bg = Some((w, h, bm));
    }
}
