//! Creep (MapComn/creep.cpp 0x413xxx-0x414xxx, with the game's tile callbacks
//! 0x47e2d0 / 0x47d230 / 0x47d290 installed by 0x4bd6f0).
//!
//! Creep lives in the tile map itself (0x5993c4, `terrain.mtxm`): a creep tile is cv5
//! group 1 (`tile & 0x7ff0 == 0x10`), and the tile it covered is kept in 0x5993a8
//! (`terrain.creep_saved`). The edge byte map (0x6d0e80) and the 0x413b30 neighbour
//! walk only drive the creep edge graphics, so they are not kept here.

use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x413f20: tile (x, y) has creep.
    pub fn creep_at(&self, x: i32, y: i32) -> bool {
        let m = &self.terrain;
        m.mtxm[(y * m.width as i32 + x) as usize] & 0x7ff0 == 0x10
    }

    /// 0x414290: creep tiles among the 8 neighbours of (x, y).
    fn creep_neighbours(&self, x: u32, y: u32) -> usize {
        let (w, h) = (self.terrain.width as u32, self.terrain.height as u32);
        let mut n = 0;
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            let (cx, cy) = (x.wrapping_add_signed(dx), y.wrapping_add_signed(dy));
            if cx < w && cy < h && self.creep_at(cx as i32, cy as i32) {
                n += 1;
            }
        }
        n
    }

    /// 0x413db0: tile box creep spreads over for a `t` at (x, y) (left, top, right,
    /// bottom; inclusive). True when it is the hatchery-style ellipse, false for the
    /// building's own footprint. `colony` is what creep colonies and hatcheries count as
    /// (0x413870).
    fn creep_box(&self, d: &Data, t: u16, x: i32, y: i32, colony: bool) -> ([i32; 4], bool) {
        let (w, h) = (self.terrain.width as i32, self.terrain.height as i32);
        let big = match t {
            0x83 | 0x8f => colony,
            0x84 | 0x85 | 0x90 | 0x92 => true,
            _ => false,
        };
        let mut b = if big {
            [(x - 0x140) / 32, (y - 200) / 32, (x + 0x140) / 32, (y + 200) / 32]
        } else {
            let v = d.dat.units.placement_box[t as usize];
            let (pw, ph) = ((v as i16 as i32) / 32, ((v >> 16) as i16 as i32) / 32);
            let (l, t) = ((x / 32) as i16 as i32 - pw / 2, (y / 32) as i16 as i32 - ph / 2);
            [l, t, l - 1 + pw, t - 1 + ph]
        };
        // The right / bottom clamps are skipped when the left / top one applied.
        if b[0] < 0 {
            b[0] = 0;
        } else if b[2] >= w {
            b[2] = w - 1;
        }
        if b[1] < 0 {
            b[1] = 0;
        } else if b[3] >= h {
            b[3] = h - 1;
        }
        (b, big)
    }

    /// 0x413fa0 with 0x4140a0: put creep on tile (x, y), remembering the tile it covers.
    fn add_creep_tile(&mut self, x: i32, y: i32) {
        let i = (y * self.terrain.width as i32 + x) as usize;
        if self.terrain.creep_saved[i] == 0 {
            self.terrain.creep_saved[i] = self.terrain.mtxm[i];
        }
        let v = if self.crt_rand() % 100 < 4 { self.crt_rand() % 7 + 6 } else { self.crt_rand() % 6 };
        let tile = 0x10 + v as u16;
        self.terrain.mtxm[i] = tile;
        self.creep_tile_changed(x, y, 0, tile);
    }

    /// MSVC `rand` (0x40c7bd), used for creep tile variations.
    fn crt_rand(&mut self) -> u32 {
        self.crt_seed = self.crt_seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        (self.crt_seed >> 16) & 0x7fff
    }

    /// 0x47d230 inverted: the local player sees tile index `i`.
    fn tile_seen_locally(&self, i: usize) -> bool {
        if self.replay {
            todo_port!("0x47d230", "replay vision for creep display");
        }
        self.terrain.tile_flags[i] & self.local_vision_mask as u32 == 0
    }

    /// 0x47e2d0: tile (x, y) became `tile`; `kind` 0 creep added, 1 removed, 2 edge.
    fn creep_tile_changed(&mut self, x: i32, y: i32, kind: u32, tile: u16) {
        let i = (x + self.terrain.width as i32 * y) as usize;
        if kind == 2 {
            self.terrain.megatiles[i] |= 0x8000;
            // 0x47d920: redraw (UI).
            return;
        }
        if self.tile_seen_locally(i) {
            self.terrain.megatiles[i] = self.terrain.tileset.megatile(tile);
            let f = &mut self.terrain.tile_flags[i];
            *f = if kind == 0 { *f | 0x4000_0000 } else { *f & !0x4000_0000 };
            // 0x47d920: redraw (UI).
        }
        let mega = self.terrain.tileset.megatile(tile) as usize & 0x7fff;
        let new = ((self.terrain.tileset.group_flags(tile) as u32 & 0xffff_fffa) << 16) | self.terrain.megatile_flags.get(mega).copied().unwrap_or(0);
        let f = &mut self.terrain.tile_flags[i];
        *f ^= (new ^ *f) & 0x45_0000;
        if *f & 0x1_0000 != 0 {
            *f &= !0x4_0000;
        }
        if *f & 0x45_0000 == 0 {
            *f |= 0x4_0000;
        }
        self.creep_recede_neighbours(x, y);
    }

    /// 0x47dc10: neighbours of (x, y) that are waiting to lose their creep re-sort
    /// by their creep neighbour count.
    fn creep_recede_neighbours(&mut self, x: i32, y: i32) {
        let (w, h) = (self.terrain.width as u32, self.terrain.height as u32);
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            let (cx, cy) = ((x + dx) as u32, (y + dy) as u32);
            if cx < w && cy < h && self.creep_at(cx as i32, cy as i32) && self.terrain.tile_flags[(cy * w + cx) as usize] & 0x1000_0000 != 0 {
                todo_port!("0x47dc10", "creep recession lists (0x658ae8)");
            }
        }
    }

    /// 0x4148f0: creep under a new `t` at (x, y), then spread it as far as it goes.
    pub fn spread_creep_instantly(&mut self, d: &Data, t: u16, x: i32, y: i32) {
        let v = d.dat.units.placement_box[t as usize];
        let (pw, ph) = ((v as i16 as i32) / 32, ((v >> 16) as i16 as i32) / 32);
        let x0 = (x / 32 - pw / 2) as u32;
        let y0 = (y / 32 - ph / 2) as u32;
        let (w, h) = (self.terrain.width as u32, self.terrain.height);
        for ty in y0..y0.wrapping_add(ph as u32) {
            for tx in x0..x0.wrapping_add(pw as u32) {
                let at = |yy: u32| ((yy as i16 as i32) * w as i32 + tx as i16 as i32) as usize;
                let below = (ty as i16).wrapping_add(1);
                if self.terrain.tile_flags[at(ty)] & 0x2080_0000 == 0
                    && (below as u16 == h || self.terrain.tile_flags[at(below as u32)] & 0x80_0000 == 0)
                {
                    self.add_creep_tile(tx as i32, ty as i32);
                }
            }
        }
        while self.spread_creep(d, t, x, y) == 1 {}
    }

    /// 0x414680: creep one more tile around a `t` at (x, y). 1 = spread, 2 = blocked
    /// only by buildings, 0 = done.
    pub fn spread_creep(&mut self, d: &Data, t: u16, x: i32, y: i32) -> u8 {
        // 0x6d7578: candidates by creep neighbour count (0xf0 slots each in 1.16.1).
        let mut lists: Vec<Vec<(i32, i32)>> = vec![Vec::new(); 8];
        let mut result = 0;
        let ([l, top, r, b], ellipse) = self.creep_box(d, t, x, y, true);
        let (w, h) = (self.terrain.width as i32, self.terrain.height);
        let mut dy = top * 32 - y + 0x10;
        for ty in top..=b {
            let mut dx = l * 32 - x + 0x10;
            for tx in l..=r {
                let f = self.terrain.tile_flags[((ty as i16 as i32) * w + tx as i16 as i32) as usize];
                let below = (ty + 1) as i16;
                if !self.creep_at(tx, ty)
                    && f & 0x2080_0000 == 0
                    && (below as u16 == h || self.terrain.tile_flags[(below as i32 * w + tx as i16 as i32) as usize] & 0x80_0000 == 0)
                {
                    if f & 0x800_0000 != 0 {
                        result = 2;
                    } else {
                        let n = self.creep_neighbours(tx as u32, ty as u32) as i32 - 1;
                        if (0..8).contains(&n) && (!ellipse || dy * dy * 0x100 + dx * dx * 100 <= 0x9c_4000) {
                            lists[n as usize].push((tx, ty));
                        }
                    }
                }
                dx += 32;
            }
            dy += 32;
        }
        let Some(best) = lists.iter().rposition(|l| !l.is_empty()) else { return result };
        let r = self.rng.rand(0x18) >> 4;
        let (tx, ty) = lists[best][r as usize % lists[best].len()];
        self.add_creep_tile(tx, ty);
        1
    }

    /// 0x49c4c0: every 100 frames, bring the shown terrain of tiles the local player
    /// sees (away from the screen's top-left corner) up to date with the creep.
    pub fn creep_update_100(&mut self) {
        let (w, h) = (self.terrain.width as u32, self.terrain.height as u32);
        for i in 0..(w * h) as usize {
            let (x, y) = (i as u32 % w, i as u32 / w);
            if !self.tile_seen_locally(i) {
                continue;
            }
            let near_screen = x.wrapping_sub(self.screen_tile.0 as u32) <= 0x14 && y.wrapping_sub(self.screen_tile.1 as u32) <= 0xd;
            let raw = self.terrain.mtxm[i];
            let mega = self.terrain.tileset.megatile(raw);
            if near_screen || mega == self.terrain.megatiles[i] & 0x7fff {
                continue;
            }
            self.terrain.megatiles[i] = mega;
            let f = &mut self.terrain.tile_flags[i];
            *f = if raw & 0x7ff0 == 0x10 { *f | 0x4000_0000 } else { *f & !0x4000_0000 };
            // 0x413b30 edges and 0x59cb58 redraw: UI.
        }
    }
}
