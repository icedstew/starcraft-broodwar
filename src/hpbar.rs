//! Health bars of selected units (1.16.1 0x47a820 and helpers): hit points in three
//! color bands, shields above them, energy (or a hallucination's / broodling's time)
//! below, all in 3-pixel boxes with thpbar.pcx colors.

use crate::bw::{Data, State};

/// 0x5140c4: hit point colors for a 5-row bar by percentage (threshold, rows 0..5).
const HP5: [(i32, [u8; 5]); 3] = [(0x42, [0x12, 0, 1, 2, 0x12]), (0x21, [0x12, 3, 4, 5, 0x12]), (0, [0x12, 6, 7, 8, 0x12])];
/// 0x5140e8: the same for the 7-row bar of units with shields.
const HP7: [(i32, [u8; 7]); 3] = [
    (0x42, [0x12, 0, 0, 1, 1, 2, 0x12]),
    (0x21, [0x12, 3, 3, 4, 4, 5, 0x12]),
    (0, [0x12, 6, 6, 7, 7, 8, 0x12]),
];
/// 0x5140d8: energy.
const ENERGY: [u8; 5] = [0x12, 0xc, 0xd, 0xe, 0x12];
/// 0x5140e0 / 0x514100: the empty part.
const EMPTY5: [u8; 5] = [0x12, 0xf, 0x10, 0x11, 0x12];
const EMPTY7: [u8; 7] = [0x12, 0xf, 0xf, 0x10, 0x10, 0x11, 0x12];
/// 0x514108 / 0x51410c: shields, full and empty.
const SHIELD: [u8; 4] = [0x12, 0xa, 0xb, 0x12];
const SHIELD_EMPTY: [u8; 4] = [0x12, 0x10, 0x11, 0x12];
/// thpbar.pcx index of the box separators (0x66fbf6).
const SEP: usize = 0x12;

/// Screen surface being drawn into (0x6cf4a8).
pub struct Surface<'a> {
    pub px: &'a mut [u8],
    pub w: i32,
    pub h: i32,
}

impl Surface<'_> {
    fn hline(&mut self, x: i32, y: i32, n: i32, c: u8) {
        for i in 0..n {
            self.put(x + i, y, c);
        }
    }

    fn put(&mut self, x: i32, y: i32, c: u8) {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) {
            self.px[(y * self.w + x) as usize] = c;
        }
    }
}

/// Clip rectangle as the image system passes it: offset into the bar and size.
#[derive(Clone, Copy)]
pub struct Clip {
    pub left: i32,
    pub top: i32,
    pub w: i32,
    pub h: i32,
}

/// 0x47a750 (`less_one`) / 0x47a6e0 / 0x47a670: fill width in 3-pixel steps; returns
/// (percent, fill).
fn fill(cur: i32, max: i32, width: i32, less_one: bool) -> (i32, i32) {
    let pct = if max == 0 { 100 } else { cur * 100 / max };
    let mut f = pct * (width - less_one as i32) / 100;
    if f < 3 {
        f = 3;
    }
    match f % 3 {
        0 => {}
        1 => f -= 1,
        r => f += 3 - r,
    }
    (pct, f)
}

/// The vertical separators every 3 pixels, aligned to the bar's own start.
fn separators(s: &mut Surface, pal: &[u8; 19], x: i32, y: i32, clip: Clip, rows: i32, skip_first: bool) {
    let mut c = x;
    if clip.left != 0 && clip.left % 3 != 0 {
        c = x - clip.left % 3 + 3;
    }
    if skip_first {
        c += 3;
    }
    while c < x + clip.w {
        for r in 0..rows {
            s.put(c, y + r, pal[SEP]);
        }
        c += 3;
    }
}

/// 0x47a4e0 / 0x47a350: a 5- or 7-row bar filled to `fill` pixels.
fn bar(s: &mut Surface, pal: &[u8; 19], x: i32, y: i32, clip: Clip, fill: i32, full: &[u8], empty: &[u8]) {
    if clip.h != full.len() as i32 {
        return;
    }
    let f = (fill - clip.left).min(clip.w).max(0);
    let e = clip.w - f;
    for r in 0..clip.h {
        if f > 0 {
            s.hline(x, y + r, f, pal[full[r as usize] as usize]);
        }
        if e > 0 {
            s.hline(x + f, y + r, e, pal[empty[r as usize] as usize]);
        }
    }
    separators(s, pal, x, y, clip, clip.h, false);
}

/// 0x47a120: the shield bar over the top rows of a 7-row bar, with cut corners.
fn shield_bar(s: &mut Surface, pal: &[u8; 19], x: i32, y: i32, clip: Clip, fill: i32) {
    if clip.h != 7 {
        return;
    }
    let mut f = (fill - clip.left).min(clip.w).max(0);
    let mut e = clip.w - f;
    let (mut xf, mut xe) = (x, x + f);
    for r in 0..4 {
        if f > 0 {
            s.hline(xf, y + r, f, pal[SHIELD[r as usize] as usize]);
        }
        if e > 0 {
            s.hline(xe, y + r, e, pal[SHIELD_EMPTY[r as usize] as usize]);
        }
        if clip.left == 0 {
            let dx = (r != 1) as i32 + 1;
            if f != 0 {
                f -= dx;
                xf += dx;
            } else if e != 0 {
                e -= dx;
                xe += dx;
            }
        }
    }
    separators(s, pal, x, y, clip, clip.h.min(4), true);
    if clip.left == 0 {
        let mut c = x;
        for r in 0..4 {
            s.put(c, y + r, pal[SEP]);
            s.put(c + 1, y + r, pal[SEP]);
            c += (r != 1) as i32 + 1;
        }
    }
}

/// 0x47a820: draw unit `u`'s health bar at screen (x, y), clipped by `clip`.
pub fn draw(s: &mut Surface, pal: &[u8; 19], st: &State, d: &Data, u: u32, x: i32, y: i32, clip: Clip) {
    let dat = d.dat;
    let un = &st.units[u as usize];
    let t = un.unit_type as usize;
    let ut = &dat.units;
    let width = dat.sprites.health_bar[st.sprites[un.sprite as usize].sprite_id as usize] as i32;
    let max_hp = match ut.hit_points[t] as i32 >> 8 {
        0 => ((un.hp + 0xff) >> 8).max(1),
        m => m,
    };
    let cur = (un.hp + 0xff) >> 8;
    let rows;
    if ut.shield_enable[t] == 0 {
        let c = Clip { h: clip.h.min(5), ..clip };
        let (pct, f) = fill(cur, max_hp, width, true);
        let colors = HP5.iter().find(|(th, _)| pct >= *th).map_or(&HP5[2].1, |e| &e.1);
        bar(s, pal, x, y, c, f, colors, &EMPTY5);
        rows = 6;
    } else {
        let c = Clip { h: clip.h.min(7), ..clip };
        let (pct, f) = fill(cur, max_hp, width, true);
        let (_, sf) = fill(un.shields >> 8, ut.shields[t] as i32, width, false);
        let colors = HP7.iter().find(|(th, _)| pct >= *th).map_or(&HP7[2].1, |e| &e.1);
        bar(s, pal, x, y, c, f, colors, &EMPTY7);
        shield_bar(s, pal, x, y, c, sf);
        rows = 8;
    }
    // Energy for the owner: spellcasters, and the time left of hallucinations and broodlings.
    let caster = ut.flags[t] & 0x20_0000 != 0;
    let hall = un.status & 0x4000_0000 != 0;
    if !((caster && !hall) || hall || t == 0x28) || un.player != st.local_player {
        return;
    }
    let max = if caster && !hall {
        (st.max_energy(d, u) >> 8) as i32
    } else if hall {
        0x546
    } else {
        0x708
    };
    let cur = if caster && !hall { (un.energy >> 8) as i32 } else { un.remove_timer as i32 };
    let c = Clip { top: clip.top + rows, h: clip.h - rows, ..clip };
    if c.h > 0 {
        let (_, f) = fill(cur, max, width, false);
        bar(s, pal, x, y + rows, c, f, &ENERGY, &EMPTY5);
    }
}

/// 0x4d6010: size of `u`'s bar and its offset below the sprite (circle half height +
/// selection offset + 8).
pub fn layout(st: &State, d: &Data, u: u32, circle_h: i32) -> (i32, i32, i32) {
    let un = &st.units[u as usize];
    let t = un.unit_type as usize;
    let id = st.sprites[un.sprite as usize].sprite_id as usize;
    let hb = d.dat.sprites.health_bar[id] as i32;
    let w = (hb - (hb - 1) % 3).max(0x13);
    let hall = un.status & 0x4000_0000 != 0;
    let mut h = if (d.dat.units.flags[t] & 0x20_0000 != 0 && !hall) || hall || t == 0x28 { 6 } else { 0 };
    h += if d.dat.units.shield_enable[t] != 0 { 7 } else { 5 };
    let dy = (circle_h >> 1) + d.dat.sprites.selection_offset[id] as i32 + 8;
    (w, h, dy)
}
