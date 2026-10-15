//! 1.16.1 dialogs (dlg.cpp, modeless.cpp, opt_dlgs.cpp): `.bin` dialog resources and
//! the default drawing of their controls (table 0x501504, by control type).

use crate::grp::{Blend, Grp};
use crate::ui::{Font, TextDst, TextOpts};

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
}

/// Draw functions dialogs install over the defaults (+0x2e).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Update {
    Default,
    /// 0x4f4fb0: the console's MENU button (set by 0x4f5180).
    MenuButton,
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
}

impl Dialog {
    pub fn parse(d: &[u8]) -> Dialog {
        let i16_at = |o: usize| d.get(o..o + 2).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]));
        let u32_at = |o: usize| d.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
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
        Dialog { this, controls, active: None, bg: None }
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

    /// The topmost visible, enabled control under screen point (x, y).
    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.controls.iter().rposition(|c| {
            let (l, t, r, b) = self.abs(c);
            c.flags & flag::VISIBLE != 0 && c.ty != 9 && c.ty != 10 && c.ty != 11 && (l..=r).contains(&x) && (t..=b).contains(&y)
        })
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
}

/// An 8-bit surface.
pub struct Surf<'a> {
    pub px: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

impl Skin {
    fn frame_wh(&self, f: u16) -> (i32, i32) {
        self.grp.frame(f as usize).map_or((0, 0), |f| (f.w, f.h))
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
    fn frame(&self, f: u8, x: i32, y: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
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
        let text = String::from_utf8_lossy(text);
        font.draw_text(&text, rect.0 + dx, rect.1 + dy, color, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
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
            // 0x4ef590 -> 0x4ef440: the dialog bitmap; index 0 is transparent.
            // ponytail: 0x4c90c0's translucent mode (flag 0x40000, blend table 0x6d0590 on
            // the key color) is drawn opaque.
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
        for (i, c) in dlg.controls.iter().enumerate() {
            if c.flags & flag::VISIBLE == 0 {
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
        }
    }

    /// One control's draw (+0x2e, by default 0x501504[type]) at screen rect `r`.
    pub fn draw_ctl(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let f = c.flags;
        let (l, t, rr, b) = r;
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
            6 => self.hscroll(c, r, s),
            9..=11 if !c.text.is_empty() => {
                self.ctl_text(c, &c.text, r, 0, 0, [0x11, 0x12, 0x14][c.ty as usize - 9], s);
            }
            _ => {}
        }
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

    /// 0x4ef710: a three-piece row from frames `base`, `base + 1` (repeated), `base + 2`,
    /// each 0x6d04e8 (frame 103's width) wide, over `width` pixels from (x, y).
    fn wide(&self, base: u8, x: i32, y: i32, width: i32, clip: (i32, i32, i32, i32), s: &mut Surf) {
        let pw = self.frame_wh(0x67).0.max(1);
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
