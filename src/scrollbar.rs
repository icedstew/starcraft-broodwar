//! 1.16.1 list box scroll bars (dlg\scrlbar.cpp): the type-7 child 0x414c90 makes for a
//! list box, shown by 0x41b300 while the rows overflow. Kept as data on the list
//! (`ListBox::scroll`); its rect is relative to the dialog like the list's.

use crate::dlg::{flag, Ctl, ListBox, Skin, Surf};
use crate::todo_port;

/// The dialog grp sizes 0x419290 keeps in globals (0x4197b0 entry).
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    /// 0x6d04c4: frame 0x10's width (the bar's width).
    pub w: i32,
    /// 0x6d04f4: frame 0x10's height (an arrow).
    pub a: i32,
    /// 0x6d04c8: frame 0x1d's height (a track piece).
    pub s: i32,
    /// 0x6d04cc / 0x6d04ec: frame 0x1c's width / height (the thumb).
    pub tw: i32,
    pub t: i32,
}

/// 0x501544: the flag each hit area (0..=5) presses; 0x51a198: its value step (areas 4
/// and 5 are -/+ the page, 0x51a1a0 / 0x51a1a2).
const PRESS: [u32; 6] = [0, 0x4000_0000, 0x2000_0000, 0x8000_0000, 0x0800_0000, 0x0400_0000];

/// One scroll bar control (0x56 bytes at 0x414c90).
#[derive(Clone, Debug)]
pub struct ScrollBar {
    /// +0x20: the list's id - 0x8000.
    pub id: i16,
    /// left, top, right, bottom (+4..+0xa), relative to the dialog.
    pub rect: (i32, i32, i32, i32),
    /// +0x18: 0x20004 at creation, 8 shown, 0x40000000 / 0x20000000 arrows pressed,
    /// 0x80000000 thumb dragged.
    pub flags: u32,
    /// +0x3a / +0x3c / +0x3e.
    pub value: i16,
    pub min: i16,
    pub max: i16,
    /// +0x46: the page (the list's rows); +0x49: the step.
    pub page: u8,
    pub step: u8,
    /// +0x40: the value change the held area applies per tick; +0x42: the flag it holds.
    pub delta: i16,
    pub press: u32,
    /// +0x47: ticks to the next repeat; +0x4a: the dragged thumb offset.
    pub tick: u8,
    pub drag: i16,
    pub m: Metrics,
    /// Held by the mouse (the dialog's capture 0x6d5e54 / 0x6d5e4c / 0x6d5e74) and its
    /// last position, dialog-relative.
    pub captured: bool,
    pub mouse: (i32, i32),
    /// Set when `track` re-presses an arrow (0x4bbcf0, the button click sound); the window
    /// loop takes it with `Ctl::list_scroll_take_click`.
    pub click: bool,
}

impl ScrollBar {
    /// 0x414d30 (type 7): the thumb's travel.
    fn travel(&self) -> i32 {
        (self.rect.3 - self.rect.1 - self.m.a * 2 - self.m.t - 5) as i16 as i32
    }

    /// 0x414dd0: the thumb's offset from the top of the track.
    fn thumb_off(&self) -> i32 {
        if self.flags & 0x8000_0000 != 0 {
            return self.drag as i32;
        }
        let range = self.max as i32 - self.min as i32;
        let l = self.travel();
        if range != 0 && self.value != self.min {
            if self.value != self.max {
                return (range / 2 + (self.value as i32 - self.min as i32) * l) / range;
            }
            return l;
        }
        0
    }

    /// 0x414f10 (type 7): the area at (x, y), dialog-relative: 0 outside, 1 up arrow,
    /// 2 down arrow, 3 thumb, 4 above it, 5 below it.
    fn hit(&self, x: i32, y: i32) -> usize {
        let (l, t, r, b) = self.rect;
        if x < l || x > r || y < t || y > b {
            return 0;
        }
        let u = self.m.a + 2;
        if t + u < y {
            if (b - t + 1) - u + t <= y {
                return 2;
            }
            let at = t + self.thumb_off() as i16 as i32 + u;
            if at <= y {
                return if at + self.m.t <= y { 5 } else { 3 };
            }
            return 4;
        }
        1
    }

    /// 0x4152f0 (event 0xb): set the value, clamped; true when it changed (the callback
    /// 0x41a6a0 then moves the list).
    fn set_value(&mut self, v: i16) -> bool {
        let v = if v < self.min {
            self.min
        } else if self.max < v {
            self.max
        } else {
            v
        };
        let changed = self.value != v;
        self.value = v;
        changed
    }

    /// 0x415340: new range (ECX min, EDX max); true when the clamped value changed.
    fn set_range(&mut self, min: i16, max: i16) -> bool {
        if self.min == min && self.max == max {
            return false;
        }
        self.min = min;
        self.max = max;
        self.set_value(self.value)
    }

    /// 0x415140 (type 7): the mouse at (x, y): a dragged thumb follows it (+0x4a, and the
    /// change +0x40 that centers it there), a held arrow / page area stays pressed only
    /// while the mouse is over it.
    fn track(&mut self, x: i32, y: i32) {
        let hit = self.hit(x, y);
        let held = self.press;
        if held == 0x8000_0000 {
            let lim = ((self.rect.3 - (self.m.t >> 1)) - self.m.a - 3) as i16 as i32;
            let y = y.min(lim);
            let mut p = y - self.rect.1 - ((self.m.t >> 1) + self.m.a + 2);
            if p < 0 {
                p = 0;
            }
            self.drag = p as i16;
            let l = self.travel();
            if l != 0 {
                let (min, max) = (self.min as i32, self.max as i32);
                self.delta = (((max - min + 1) * p) / l - self.value as i32 + min) as i16;
            }
        } else if self.flags & held == 0 {
            if PRESS[hit] == held {
                self.flags ^= held;
                if self.flags & held != 0 {
                    // 0x4bbcf0: plays the button click (sfxdata entry 15) if sound is on.
                    self.click = true;
                }
            }
        } else if PRESS[hit] != held {
            self.flags ^= held;
        }
    }

    /// 0x415910: one tick of a held area: every `tick` ticks the mouse is tracked and,
    /// while the area is held, the value moves by +0x40 (event 0xb). Some(new value).
    fn repeat(&mut self) -> Option<i16> {
        self.tick = self.tick.wrapping_sub(1);
        if self.tick != 0 {
            return None;
        }
        self.tick = if self.press & 0x6000_0000 != 0 { 1 } else { 3 };
        let (x, y) = self.mouse;
        self.track(x, y);
        if self.press & self.flags != 0 {
            Some(self.delta.wrapping_add(self.value))
        } else {
            None
        }
    }

    /// 0x414e30 (event 0 / 1, key `vk`): the value a key sets: Page Up / Down, End, Home,
    /// Up / Down arrows (type 7).
    pub fn key_value(&self, vk: u16) -> Option<i16> {
        let v = self.value;
        Some(match vk {
            0x21 => v - self.page as i16,
            0x22 => v + self.page as i16,
            0x23 => self.max,
            0x24 => self.min,
            0x26 => v - 1,
            0x28 => v + 1,
            _ => return None,
        })
    }
}

impl ListBox {
    /// Run `f` on the bar; when it moved the value, callback 0x41a6a0 (ECX = the list)
    /// sets the list's top row to it (not while the layout 0x41b410 has bit 31 set).
    fn scroll_op<R>(&mut self, notify: bool, f: impl FnOnce(&mut ScrollBar) -> (R, bool)) -> Option<R> {
        let b = self.scroll.as_mut()?;
        let (r, changed) = f(b);
        if changed && notify {
            self.top = b.value as u8;
        }
        Some(r)
    }

    /// 0x41a1b0: the width a shown scroll bar takes (bar.right - bar.left).
    pub fn scroll_width(&self) -> i32 {
        match &self.scroll {
            Some(b) if b.flags & flag::VISIBLE != 0 => (b.rect.2 - b.rect.0) as i16 as i32,
            _ => 0,
        }
    }
}

impl Skin {
    /// 0x414c90 (type 7) as 0x41b140 uses it: a list taller than a scroll bar's arrows and
    /// one track piece (0x6d04c8 + 2 * 0x6d04f4) gets a hidden bar, its bottom snapped to
    /// whole track pieces (the list's bottom follows), at the list's right edge, then
    /// event 10 (0x414c00: right = max(left + width, right)).
    pub fn list_scroll_create(&self, c: &mut Ctl) {
        let (w, a) = self.frame_wh(0x10);
        let m = Metrics { w, a, s: self.frame_wh(0x1d).1, tw: self.frame_wh(0x1c).0, t: self.frame_wh(0x1c).1 };
        let h = (c.rect.3 - c.rect.1) as i16 as i32;
        if h <= m.s + m.a * 2 || m.s == 0 {
            return;
        }
        let u = m.a * 2 + 4;
        let bottom = (((m.s >> 1) - u + h) / m.s * m.s + u) as i16 as i32;
        c.rect.3 = (bottom + c.rect.1) as i16 as i32;
        let right = c.rect.2;
        let mut b = ScrollBar {
            id: (c.id as i32 - 0x8000) as i16,
            rect: (right - m.w + 1, c.rect.1, right, c.rect.3),
            flags: 0x20004,
            value: 0,
            min: 0,
            max: 0,
            page: 0,
            step: 0,
            delta: 0,
            press: 0,
            tick: 0,
            drag: 0,
            m,
            captured: false,
            mouse: (0, 0),
            click: false,
        };
        b.rect.2 = b.rect.2.max(b.rect.0 + m.w);
        if let Some(l) = c.list.as_mut() {
            l.scroll = Some(Box::new(b));
        }
    }

    /// 0x415760 (type 7): the bar's arrows (0x10 + enabled + pressed), its track (frame
    /// 0x1d, 0x1e repeated, 0x1f) and the thumb (0x1c, not when disabled). `r` is the
    /// list's screen rect (the bar's rect moves with it).
    pub fn list_scroll_draw(&self, c: &Ctl, r: (i32, i32, i32, i32), s: &mut Surf) {
        let Some(b) = c.list.as_ref().and_then(|l| l.scroll.as_deref()) else { return };
        if b.flags & flag::VISIBLE == 0 {
            return;
        }
        let m = b.m;
        let (dx, dy) = (r.0 - c.rect.0, r.1 - c.rect.1);
        let clip = (b.rect.0 + dx, b.rect.1 + dy, b.rect.2 + dx, b.rect.3 + dy);
        let (x, top) = (clip.0, clip.1);
        if b.flags & 0x200_0000 != 0 {
            // 0x415090: a plain bar: an outline in color 0x6ceb28 (0x6ceb30 if enabled) and,
            // when enabled, a 6 high one at the thumb. (Its focus flag 0x1000 update needs the
            // dialog's focused control and is not kept: the bit is not used here.)
            let col = self.colors.get();
            let color = col[if b.flags & flag::DISABLED != 0 { 8 } else { 0x10 }];
            let w = clip.2 - clip.0;
            let outline = |(l, t, r, bt): (i32, i32, i32, i32), s: &mut Surf| {
                for y in [t, bt] {
                    for x in l..=r {
                        if x >= 0 && y >= 0 && x < s.w && y < s.h {
                            s.px[(y * s.w + x) as usize] = color;
                        }
                    }
                }
                for x in [l, r] {
                    for y in t..=bt {
                        if x >= 0 && y >= 0 && x < s.w && y < s.h {
                            s.px[(y * s.w + x) as usize] = color;
                        }
                    }
                }
            };
            outline((x, top, x + w, clip.3 - clip.1 + top), s);
            if b.flags & flag::DISABLED == 0 {
                let y = b.thumb_off() as i16 as i32 + top;
                outline((x, y, x + w, y + 5), s);
            }
            return;
        }
        if (m.s + m.w) * 2 > (b.rect.3 - b.rect.1) as i16 as i32 {
            return;
        }
        let en = (!(b.flags >> 1) & 1) as u8;
        self.frame(en + if b.flags & flag::PRESSED != 0 { 0x11 } else { 0x10 }, x, top, clip, s);
        let mut y = m.a + 2;
        self.frame(0x1d, x, y + top, clip, s);
        let q = (b.travel() + m.t) / m.s;
        for _ in 1..q.max(1) {
            y += m.s;
            self.frame(0x1e, x, y + top, clip, s);
        }
        self.frame(0x1f, x, y + m.s + top, clip, s);
        self.frame(en + if b.flags & flag::WIDE != 0 { 0x14 } else { 0x13 }, x, y + m.s + 2 + m.s + top, clip, s);
        if b.flags & flag::DISABLED == 0 {
            self.frame(0x1c, ((m.w - m.tw) >> 1) - 1 + x, b.thumb_off() as i16 as i32 + m.a + 2 + top, clip, s);
        }
    }
}

impl Ctl {
    /// 0x41b300 (ECX = this list, EAX = its bar): called by the layout 0x41b410 (bit 31
    /// set, so the callback is silent): the bar is shown (the list's right edge moves in
    /// by the bar's width + 3) while there are more rows than fit and the list is shown,
    /// else hidden; page = rows shown, range 0..=last top row, value = the top row.
    pub fn list_scroll_layout(&mut self) {
        let vis = self.flags & flag::VISIBLE != 0;
        let Some(l) = self.list.as_mut() else { return };
        let (n, rows, max_top, top, sel) = (l.items.len() as u8, l.rows, l.max_top, l.top, l.sel);
        let Some(b) = l.scroll.as_deref_mut() else { return };
        let shift = (b.rect.2 - b.rect.0) as i16 as i32 + 3;
        if rows < n {
            if b.flags & flag::VISIBLE == 0 && vis {
                b.flags |= flag::VISIBLE;
                if b.flags & flag::DISABLED != 0 {
                    // 0x418e00: enable the bar again.
                    b.flags &= !flag::DISABLED;
                    todo_port!("0x418900", "scroll bar enable event");
                }
                self.rect.2 -= shift;
            }
            if b.flags & flag::VISIBLE != 0 && !vis {
                b.flags &= !flag::VISIBLE;
                self.rect.2 += shift;
            }
        } else if b.flags & flag::VISIBLE != 0 {
            // 0x418640: disable the bar (and tell it event 6), then hide it.
            if b.flags & flag::DISABLED == 0 {
                b.flags = b.flags & !0x80 | flag::DISABLED;
            }
            b.flags &= !flag::VISIBLE;
            self.rect.2 += shift;
        }
        b.page = rows;
        b.step = 1;
        b.set_range(0, max_top as i16);
        if sel != 0xff {
            b.set_value(top as i16);
        }
    }

    /// Takes the pending arrow re-press click sound (0x4bbcf0).
    pub fn list_scroll_take_click(&mut self) -> bool {
        self.list.as_mut().and_then(|l| l.scroll.as_deref_mut()).is_some_and(|b| std::mem::take(&mut b.click))
    }

    /// The bar is held by the mouse (a click returned true and the button is still down).
    pub fn list_scroll_active(&self) -> bool {
        self.list.as_ref().and_then(|l| l.scroll.as_deref()).is_some_and(|b| b.captured)
    }

    /// 0x415af0 event 4 (0x415a90 -> 0x415980 -> 0x415910): the left button went down at
    /// (x, y), dialog-relative: true when the bar took it (arrows and page areas move the
    /// list at once, the thumb jumps under the mouse).
    pub fn list_scroll_click(&mut self, x: i32, y: i32) -> bool {
        let Some(l) = self.list.as_mut() else { return false };
        let r = l.scroll_op(true, |b| {
            if b.flags & flag::VISIBLE == 0 || b.flags & flag::DISABLED != 0 {
                return (false, false);
            }
            // 0x51a1a0 / 0x51a1a2: the page steps.
            let hit = b.hit(x, y);
            if hit == 0 {
                return (false, false);
            }
            let delta = [0, -1, 1, 0, -(b.page as i16), b.page as i16][hit];
            b.mouse = (x, y);
            b.captured = true;
            b.delta = delta;
            b.press = PRESS[hit];
            b.flags |= b.press;
            b.tick = 1;
            todo_port!("0x6d5eac", "click sound callback (2)");
            let v = b.repeat();
            let changed = v.is_some_and(|v| b.set_value(v));
            (true, changed)
        });
        r.unwrap_or(false)
    }

    /// 0x415af0 event 3: the mouse moved while the bar is held; a dragged thumb follows.
    pub fn list_scroll_drag(&mut self, x: i32, y: i32) {
        let Some(l) = self.list.as_mut() else { return };
        l.scroll_op(true, |b| {
            if !b.captured {
                return ((), false);
            }
            b.mouse = (x, y);
            b.track(x, y);
            if b.press == 0x8000_0000 {
                b.tick = 1;
                let v = b.repeat();
                return ((), v.is_some_and(|v| b.set_value(v)));
            }
            ((), false)
        });
    }

    /// 0x415af0 event 0xd: a timer tick while the bar is held (arrows repeat every tick,
    /// page areas every 3rd). The caller's tick rate is not ported (0x416090 timers).
    pub fn list_scroll_tick(&mut self) {
        let Some(l) = self.list.as_mut() else { return };
        l.scroll_op(true, |b| {
            if !b.captured {
                return ((), false);
            }
            let v = b.repeat();
            ((), v.is_some_and(|v| b.set_value(v)))
        });
    }

    /// 0x415af0 event 5: the button went up: the held area is let go (flags & 0x13ffffff).
    pub fn list_scroll_release(&mut self, x: i32, y: i32) {
        let Some(l) = self.list.as_mut() else { return };
        l.scroll_op(true, |b| {
            if !b.captured {
                return ((), false);
            }
            b.mouse = (x, y);
            b.track(x, y);
            b.flags &= 0x13ff_ffff;
            b.captured = false;
            ((), false)
        });
    }
}
