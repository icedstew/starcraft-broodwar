//! Fog of war (los.cpp): sight shapes, revealing tiles, sprite visibility.
//!
//! Tile flags (0x6d1260) keep one "not visible" bit per player in bits 0..7 and one
//! "not explored" bit per player in bits 8..15; bits 0x1000000 / 0x2000000 /
//! 0x4000000 are the tile's elevation as seen by line of sight.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// One cell of a sight shape (0x14 bytes in 1.16.1).
#[derive(Clone, Copy, Debug, Default)]
pub struct SightEntry {
    /// +0 / +4: cells closer to the center that must be visible for this one to be.
    pub parents: [u16; 2],
    /// +0x12: number of parents (0 for the center).
    pub kind: u8,
    /// +8: tile offset (dy * map width + dx).
    pub offset: i32,
    /// +0x10 / +0x11
    pub dx: i8,
    pub dy: i8,
}

/// Sight shape for one sight range (0x513a18, 0x1c bytes each).
#[derive(Clone, Debug, Default)]
pub struct Sight {
    /// +0 / +4: bounding box in tiles.
    pub w: i32,
    pub h: i32,
    /// +0x10: cells always revealed (the inner box); +0x14: the rest.
    pub inner: usize,
    pub outer: usize,
    pub entries: Vec<SightEntry>,
    /// +0xc of each entry: the tile flags seen by the last reveal.
    pub cache: Vec<u32>,
}

/// 0x5056f0 / 0x5056ec: ring walk directions (right, down, left, up).
const DX: [i32; 4] = [1, 0, -1, 0];
const DY: [i32; 4] = [0, 1, 0, -1];

/// 0x42ad60: build the 12 sight shapes for a map of width `map_w`.
pub fn build_sights(map_w: i32) -> Vec<Sight> {
    (0..12)
        .map(|r| {
            // 0x513a18: (w, h, inner w, inner h).
            let (w, h, iw, ih) = (2 * r + 3, 2 * r + 3, 3, 3);
            let mut grid = vec![(false, u16::MAX); (w * h) as usize];
            // 0x42acb0: mark the inner box and the circle, count the cells.
            let mut i = (((h - ih) * w) / 2 + (w - iw) / 2) as usize;
            for _ in 0..ih {
                for _ in 0..iw {
                    grid[i].0 = true;
                    i += 1;
                }
                i += (w - iw) as usize;
            }
            mark_circle(w, h, &mut grid);
            let n = grid.iter().filter(|c| c.0).count();
            let inner = (iw * ih) as usize;
            let mut s = Sight { w, h, inner, outer: n - inner, entries: vec![SightEntry::default(); n], cache: vec![0; n] };
            fill_entries(&mut s, &mut grid, map_w);
            s
        })
        .collect()
}

/// 0x42ab50: mark the cells inside the sight circle.
fn mark_circle(w: i32, h: i32, grid: &mut [(bool, u16)]) {
    let mut r3 = w / 2;
    let mut a = (r3 * r3) as u32;
    let mut c = ((r3 - 1) * r3) as u32;
    let mut l20 = r3 * 2;
    let mut lo = 0;
    let oy = h / 2 - r3;
    let mut i14 = 0;
    let mut i18 = 0;
    let mut hi = r3 * 2;
    let mut r7 = r3;
    let mut l10 = r3;
    let mut row = |y: i32, x0: i32, x1: i32| {
        for x in x0..=x1 {
            grid[(y * w + x) as usize].0 = true;
        }
    };
    loop {
        if lo <= hi {
            row(oy + r3, lo, hi);
            row(oy + r7, lo, hi);
        }
        if r3 <= r7 {
            row(oy + lo, r3, r7);
            row(oy + hi, r3, r7);
        }
        i14 += 1;
        i18 += 2;
        a = a.wrapping_add((1 - i18) as u32);
        r3 -= 1;
        r7 += 1;
        if a <= c {
            l10 -= 1;
            hi -= 1;
            l20 -= 2;
            lo += 1;
            c = c.wrapping_sub(l20 as u32);
        }
        if i14 > l10 {
            break;
        }
    }
}

/// 0x42a8d0: number the marked cells ring by ring from the center and link each to
/// the one or two cells between it and the center.
fn fill_entries(s: &mut Sight, grid: &mut [(bool, u16)], map_w: i32) {
    let (w, h) = (s.w, s.h);
    let total = s.inner + s.outer;
    let cell = |x: i32, y: i32| ((h / 2 + y) * w + w / 2 + x) as usize;
    grid[cell(0, 0)].1 = 0;
    let lim = [h, w, h, w, h];
    let (mut x, mut y) = (-1i32, -1i32);
    let mut count = 1usize;
    let mut ring = 2i32;
    if total <= 1 {
        return;
    }
    loop {
        for k in 0..4 {
            let n = if ring > lim[k] {
                x += DX[k] * ring;
                y += DY[k] * ring;
                1
            } else {
                x += DX[k];
                y += DY[k];
                let m = lim[k + 1] - 1;
                if ring < m { ring } else { m }
            };
            for _ in 0..n {
                if grid[cell(x, y)].0 {
                    let e = &mut s.entries[count];
                    if x != 0 || y != 0 {
                        let (mut px, ax) = if x == 0 { (0, 0) } else if x < 0 { (x + 1, -(x + 1)) } else { (x - 1, x - 1) };
                        let (mut py, ay) = if y == 0 { (0, 0) } else if y < 0 { (y + 1, -(y + 1)) } else { (y - 1, y - 1) };
                        if ax == ay || (x == 0 && DX[k] != 0) || (y == 0 && DY[k] != 0) {
                            let p = grid[cell(px, py)].1;
                            e.parents = [p, p];
                            e.kind = 1;
                        } else {
                            let p1 = grid[cell(px, py)].1;
                            if ax > ay {
                                py += if y < 0 { -1 } else { 1 };
                            } else {
                                px += if x < 0 { -1 } else { 1 };
                            }
                            e.parents = [p1, grid[cell(px, py)].1];
                            e.kind = 2;
                        }
                    }
                    e.offset = y * map_w + x;
                    e.dx = x as i8;
                    e.dy = y as i8;
                    grid[cell(x, y)].1 = count as u16;
                    count += 1;
                }
                x += DX[k];
                y += DY[k];
            }
            x -= DX[k];
            y -= DY[k];
        }
        if ring < w - 1 {
            x -= 1;
        }
        if ring < h - 1 {
            y -= 1;
        }
        ring += 2;
        if count >= total {
            break;
        }
    }
}

impl State {
    /// 0x480960 (vision part): everything unseen and unexplored, sight shapes built.
    pub fn init_vision(&mut self) {
        for f in self.terrain.tile_flags.iter_mut() {
            *f |= 0xffff;
        }
        self.sights = build_sights(self.terrain.width as i32);
    }

    /// 0x4bd0f0: line-of-sight elevation at a pixel (0 low, 1 mid, 2 high).
    pub(super) fn los_elevation(&self, x: i32, y: i32) -> u8 {
        let m = &self.terrain;
        let i = (y / 32) as usize * m.width as usize + (x / 32) as usize;
        todo_port!("0x4bd0f0", "creep tile overrides (0x5993a8)");
        let mega = (m.megatiles[i] & 0x7fff) as usize;
        let f = m.vf4.get(mega * 16 + (((y >> 3) & 3) * 4 + ((x >> 3) & 3)) as usize).copied().unwrap_or(0);
        if f & 4 != 0 { 2 } else { ((f >> 1) & 1) as u8 }
    }

    /// 0x4806f0: reveal sight shape `range` around (x, y) for the players in `mask`.
    pub fn reveal(&mut self, mask: u32, range: u8, x: i32, y: i32, air: bool) {
        if mask > 0xff {
            return;
        }
        let clear = !(mask << 8) & !mask;
        let mut see = mask;
        if !air {
            see |= match self.los_elevation(x, y) {
                2 => 0x100_0000,
                1 => 0x500_0000,
                _ => 0x700_0000,
            };
        }
        let (tx, ty) = (x / 32, y / 32);
        let (mw, mh) = (self.terrain.width as i32, self.terrain.height as i32);
        let s = &mut self.sights[range as usize];
        let (hw, hh) = (s.w / 2, s.h / 2);
        let checked = tx - hw < 0 || tx + hw >= mw || ty - hh < 0 || ty + hh >= mh;
        let base = (ty * mw + tx) as isize;
        let flags = &mut self.terrain.tile_flags;
        let on_map = |e: &SightEntry| ((e.dx as i32 + tx) as u32) < mw as u32 && ((e.dy as i32 + ty) as u32) < mh as u32;
        if air {
            // 0x480310 / 0x4802a0: the first `outer` cells, no line of sight.
            for e in &s.entries[..s.outer] {
                if !checked || on_map(e) {
                    flags[(base + e.offset as isize) as usize] &= clear;
                }
            }
            return;
        }
        // 0x480200 / 0x480110
        for i in 0..s.inner {
            let e = s.entries[i];
            s.cache[i] = 0xff;
            if !checked || on_map(&e) {
                let t = (base + e.offset as isize) as usize;
                flags[t] &= clear;
                s.cache[i] = flags[t];
            }
        }
        for i in s.inner..s.inner + s.outer {
            let e = s.entries[i];
            s.cache[i] = 0xff;
            if checked && !on_map(&e) {
                continue;
            }
            let open = (0..e.kind as usize).rev().any(|k| s.cache[e.parents[k] as usize] & see == 0);
            if open {
                let t = (base + e.offset as isize) as usize;
                flags[t] &= clear;
                s.cache[i] = flags[t];
            }
        }
    }

    /// 0x480880 (every 100 frames): hide everything again before units re-reveal.
    pub fn vision_reset(&mut self) {
        todo_port!("0x480880", "game option flags 0x6d5a6c (0x4001 = no fog) / UI refresh 0x4bd350");
        let (w, h) = (self.terrain.width as usize, self.terrain.height as usize);
        for f in &mut self.terrain.tile_flags[..w * h] {
            *f |= 0xff;
        }
    }

    /// 0x4e5b40: sight range of a unit.
    pub fn unit_sight_range(&self, d: &Data, u: u32, ignore_blind: bool) -> u8 {
        let un = &self.units[u as usize];
        if un.status & 2 != 0 && un.status & 1 == 0 && !self.morphing_zerg_building(u) {
            return 4;
        }
        if !ignore_blind && un.is_blind != 0 {
            return 2;
        }
        let up = &self.players.upgrade_sc[un.player as usize];
        let boosted = match un.unit_type {
            1 => up[0x14] != 0,
            0x2a => up[0x19] != 0,
            0x46 => up[0x29] != 0,
            0x54 => up[0x26] != 0,
            _ => false,
        };
        if boosted { 11 } else { d.dat.units.sight_range[un.unit_type as usize] }
    }

    /// 0x45cd00: incomplete building morphing into a zerg building.
    fn morphing_zerg_building(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.build_queue[un.build_queue_slot as usize % 5];
        un.status & 1 == 0 && matches!(t, 0x85 | 0x84 | 0x89 | 0x90 | 0x92)
    }

    /// 0x4f3a80: the unit is seen by everyone (carrying, or transporting a carrier of,
    /// a 0xd7 powerup).
    fn reveals_to_all(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type as usize;
        if d.dat.units.flags[t] & 8 != 0 {
            todo_port!("0x4f3a80", "worker carrying powerup 0xd7 (+0xc0)");
            return false;
        }
        if un.status & 0x4000_0000 == 0
            && (t != 0x2a || self.players.upgrade_sc[un.player as usize][0x18] != 0)
            && d.dat.units.space_provided[t] != 0
        {
            todo_port!("0x4e7080", "loaded worker carrying powerup 0xd7");
        }
        false
    }

    /// 0x4e5f30: reveal around a unit for its owner and everyone sharing its vision.
    pub fn unit_reveal(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let p = un.player;
        if (p >= 8 && un.parasite_flags == 0) || un.unit_type == 0xe {
            return;
        }
        let mask = if self.reveals_to_all(d, u) || (un.unit_type == 0xd7 && un.order == 0x11) {
            0xff
        } else {
            let par = un.parasite_flags as u32;
            let mut m = par;
            if p < 8 {
                m |= self.player_vision[p as usize];
            }
            for q in 0..8 {
                if par & (1 << q) != 0 {
                    m |= self.player_vision[q];
                }
            }
            m
        };
        let range = self.unit_sight_range(d, u, false);
        let s = &self.sprites[self.units[u as usize].sprite as usize];
        let (x, y) = (s.x as i32, s.y as i32);
        let air = self.units[u as usize].status & 4 != 0;
        self.reveal(mask, range, x, y, air);
    }

    /// 0x4877b0 with a unit's placement box: sprite visibility from the fog under it.
    /// True when the local player just lost sight of it.
    fn refresh_unit_sprite_visibility(&mut self, d: &Data, u: u32) -> bool {
        let s = self.units[u as usize].sprite;
        if s == NIL || self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return false;
        }
        let b = d.dat.units.placement_box[self.units[u as usize].unit_type as usize];
        let (w, h) = ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32);
        let sp = &self.sprites[s as usize];
        let mut tx = (sp.x as i32 - w / 2) / 32;
        let mut ty = (sp.y as i32 - h / 2) / 32;
        let mut tw = ((w + 0x1f) as u32 >> 5) as i32;
        let mut th = ((h + 0x1f) as u32 >> 5) as i32;
        let vis = sp.visibility;
        let (mw, mh) = (self.terrain.width as i32, self.terrain.height as i32);
        if tx < 0 {
            tw += tx;
            if tw <= 0 {
                return vis != 0;
            }
            tx = 0;
        }
        if ty < 0 {
            th += ty;
            ty = 0;
            if th <= 0 {
                return vis != 0;
            }
        }
        if tw + tx > mw {
            tw = mw - tx;
            if tw <= 0 {
                return vis != 0;
            }
        }
        if th + ty > mh {
            th = mh - ty;
            if th <= 0 {
                return vis != 0;
            }
        }
        // 0x402500
        let mut v = 0u32;
        for yy in ty..ty + th {
            for xx in tx..tx + tw {
                v |= !(self.terrain.tile_flags[(yy * mw + xx) as usize] & 0xff);
            }
        }
        let v = v as u8;
        if vis == v {
            return false;
        }
        let lm = self.local_vision_mask;
        let was = vis & lm;
        self.sprite_set_visibility(s, v);
        was != 0 && self.sprites[s as usize].visibility & lm == 0
    }

    /// 0x4ebe10: per-frame visibility of a unit's sprite (and its subunit's).
    pub fn update_unit_visibility(&mut self, d: &Data, u: u32) {
        let lost = self.refresh_unit_sprite_visibility(d, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            let ss = self.units[sub as usize].sprite;
            if self.sprites[ss as usize].flags & super::sprite::sflag::HIDDEN == 0 {
                let v = self.sprites[self.units[u as usize].sprite as usize].visibility;
                self.sprite_set_visibility(ss, v);
            }
        }
        if lost {
            todo_port!("0x4ebe10", "local player lost sight: deselect (0x4c3b40), drop selection images, fog sprite copy (0x488410)");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sight_shapes() {
        let s = super::build_sights(128);
        // Range 0 is the 3x3 box; every shape starts at its center.
        assert_eq!((s[0].inner, s[0].outer), (9, 0));
        for sh in &s {
            assert_eq!(sh.entries[0].offset, 0);
            assert!(sh.entries[1..].iter().all(|e| e.kind == 1 || e.kind == 2));
        }
    }
}
