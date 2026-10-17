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
            c.flags & flag::VISIBLE != 0 && c.ty != 9 && c.ty != 10 && c.ty != 11 && (l..=r).contains(&x) && (t..=b).contains(&y)
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
            font.draw_text(&String::from_utf8_lossy(&c.text), l, t, 2, &o, TextDst { rows: &self.text_rows, out: s.px, w: s.w, h: s.h });
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
            6 => self.hscroll(c, r, s),
            9..=11 if !c.text.is_empty() => {
                self.ctl_text(c, &c.text, r, 0, 0, [0x11, 0x12, 0x14][c.ty as usize - 9], s);
            }
            _ => {}
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
        let remap = fx.shade_remap(level(fx.image_pct));
        self.blit(&img.px, img.w, img.h, r.0, r.1, r, Some(&remap), true, s);
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
        let text = String::from_utf8_lossy(if f & flag::HOTKEY != 0 { &c.text[1..] } else { &c.text }).into_owned();
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
        font.draw_text(&text, t.0, t.1, color, &o, TextDst { rows: &rows, out: s.px, w: s.w, h: s.h });
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
