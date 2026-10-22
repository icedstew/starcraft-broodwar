//! Creep (MapComn/creep.cpp 0x413xxx-0x414xxx, with the game's tile callbacks
//! 0x47e2d0 / 0x47d230 / 0x47d290 installed by 0x4bd6f0).
//!
//! Creep lives in the tile map itself (0x5993c4, `terrain.mtxm`): a creep tile is cv5
//! group 1 (`tile & 0x7ff0 == 0x10`), and the tile it covered is kept in 0x5993a8
//! (`terrain.creep_saved`). The edge byte map (0x6d0e80, `terrain.creep_edges`), filled
//! by the 0x413b30 neighbour walk, picks the creep edge pieces drawn on bare tiles.

use super::list::NIL;
use super::{Data, State};
use std::sync::OnceLock;

/// 0x5061b8: the 0x413b30 walk order (the 8 neighbours, then the tile itself).
const WALK: [(i32, i32); 9] = [(1, 1), (0, 1), (-1, 1), (1, 0), (-1, 0), (1, -1), (0, -1), (-1, -1), (0, 0)];

/// 0x413c50 (0x6d0c80: neighbour mask -> edge shape) and 0x413c00 (0x6d0e88: edge
/// shape -> piece number, in order of first use).
fn edge_tables() -> &'static ([u8; 0x200], [u8; 0x80]) {
    static T: OnceLock<([u8; 0x200], [u8; 0x80])> = OnceLock::new();
    T.get_or_init(|| {
        let mut shape = [0u8; 0x200];
        for (m, v) in shape.iter_mut().enumerate() {
            if m & 0x100 != 0 {
                *v = 0x7f;
                continue;
            }
            if m & 0x10 != 0 {
                *v |= 9;
            }
            if m & 8 != 0 {
                *v |= 0x24;
            }
            if m & 0x40 != 0 {
                *v |= 2;
            }
            if m & 2 != 0 {
                *v |= 0x10;
            }
            if m & 0xc0 == 0xc0 {
                *v |= 1;
            }
            if m & 0x60 == 0x60 {
                *v |= 4;
            }
            if m & 6 == 6 {
                *v |= 8;
            }
            if m & 3 == 3 {
                *v |= 0x20;
            }
            if *v & 0x21 == 0x21 || *v & 0xc == 0xc {
                *v |= 0x40;
            }
        }
        let mut piece = [0u8; 0x80];
        let mut n = 0u8;
        for (k, p) in piece.iter_mut().enumerate() {
            if shape.iter().any(|&v| v as usize == k) {
                *p = n;
                n += 1;
            }
        }
        (shape, piece)
    })
}

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
        self.creep_edges_around(x, y);
    }

    /// 0x47d290: tile (x, y) shows creep to the local player.
    fn creep_shown(&self, x: i32, y: i32) -> bool {
        self.terrain.tile_flags[(y * self.terrain.width as i32 + x) as usize] & 0x4000_0000 != 0
    }

    /// 0x413b30 with 0x414440: the bare tiles around (x, y) (and (x, y) itself) get the
    /// creep edge piece for their creep neighbours (0x414230 mask, 0x413a70).
    fn creep_edges_around(&mut self, x: i32, y: i32) {
        let (w, h) = (self.terrain.width as i32, self.terrain.height as i32);
        let (shape, piece) = edge_tables();
        for (dx, dy) in WALK {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || nx >= w || ny < 0 || ny >= h || self.creep_shown(nx, ny) {
                continue;
            }
            // The mask is a byte, so the tile's own bit (1 << 8) drops out.
            let mut mask = 0u8;
            for (i, (ex, ey)) in WALK.iter().enumerate() {
                let (mx, my) = (nx + ex, ny + ey);
                if mx >= 0 && mx < w && my >= 0 && my < h && self.creep_shown(mx, my) {
                    mask |= (1u32 << i) as u8;
                }
            }
            let i = (ny * w + nx) as usize;
            self.terrain.creep_edges[i] = piece[shape[mask as usize] as usize];
            let tile = self.terrain.mtxm[i];
            self.creep_tile_changed(nx, ny, 2, tile);
        }
    }

    /// MSVC `rand` (0x40c7bd), used for creep tile variations.
    pub fn crt_rand(&mut self) -> u32 {
        self.crt_seed = self.crt_seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        (self.crt_seed >> 16) & 0x7fff
    }

    /// 0x47d230 inverted: the local player sees tile index `i`. A replay asks whether
    /// any shown player (0x6d0f18, kept in `local_vision_mask`) sees it; 0x6d0f1c (the
    /// replay vision menu's "all players", not ported) is 0.
    fn tile_seen_locally(&self, i: usize) -> bool {
        let fog = self.terrain.tile_flags[i] as u8;
        if self.replay {
            return !fog & self.local_vision_mask != 0;
        }
        fog & self.local_vision_mask == 0
    }

    /// 0x47e2d0: tile (x, y) became `tile`; `kind` 0 creep added, 1 removed, 2 edge.
    fn creep_tile_changed(&mut self, x: i32, y: i32, kind: u32, tile: u16) {
        let i = (x + self.terrain.width as i32 * y) as usize;
        if kind == 2 {
            self.terrain.megatiles[i] |= 0x8000;
            self.minimap_terrain = true; // 0x47d920
            return;
        }
        if self.tile_seen_locally(i) {
            self.terrain.megatiles[i] = self.terrain.tileset.megatile(tile);
            let f = &mut self.terrain.tile_flags[i];
            *f = if kind == 0 { *f | 0x4000_0000 } else { *f & !0x4000_0000 };
            self.minimap_terrain = true; // 0x47d920
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
                let Some(n) = self.creep_rec.find(cx as u8, cy as u8) else { continue };
                let b = self.creep_neighbours(cx, cy) as u8;
                if b != self.creep_rec.nodes[n as usize].bucket {
                    self.creep_rec.unlink(n);
                    self.creep_rec.push(n, b);
                }
            }
        }
    }

    /// 0x47d770: creep tiles around `u` that were waiting to recede get a source again.
    pub(super) fn creep_unmark_receding(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let (x, y) = (un.fl.pos.0 as i32, un.fl.pos.1 as i32);
        let ([l, top, r, b], ellipse) = self.creep_box(d, un.unit_type, x, y, un.status & 1 != 0);
        let w = self.terrain.width as i32;
        for ty in top..=b {
            let dy = ty * 32 - y + 0x10;
            for tx in l..=r {
                let dx = tx * 32 - x + 0x10;
                if self.terrain.tile_flags[(ty * w + tx) as usize] & 0x1000_0000 != 0 && (!ellipse || dy * dy * 0x100 + dx * dx * 100 < 0x9c_4001) {
                    self.unmark_receding(tx, ty);
                }
            }
        }
    }

    /// 0x47dd60: creep tile (x, y) starts waiting to recede (false when no node is free).
    fn mark_receding(&mut self, x: i32, y: i32) -> bool {
        let r = &mut self.creep_rec;
        if r.counts[9] == 0 {
            return false;
        }
        let n = r.heads[9];
        r.unlink(n);
        r.nodes[n as usize].x = x as u8;
        r.nodes[n as usize].y = y as u8;
        let b = self.creep_neighbours(x as u32, y as u32) as u8;
        self.creep_rec.push(n, b);
        self.creep_rec.hash_insert(n);
        let w = self.terrain.width as i32;
        self.terrain.tile_flags[(y * w + x) as usize] |= 0x1000_0000;
        true
    }

    /// 0x47d540: creep tile (x, y) no longer recedes.
    fn unmark_receding(&mut self, x: i32, y: i32) -> bool {
        let r = &mut self.creep_rec;
        let Some(n) = r.find(x as u8, y as u8) else { return false };
        r.unlink(n);
        r.hash_remove(n);
        r.push(n, 9);
        let w = self.terrain.width as i32;
        self.terrain.tile_flags[(y * w + x) as usize] &= !0x1000_0000;
        true
    }

    /// 0x47e130: one creep tile recedes: a random one among those with the fewest creep
    /// neighbours.
    pub(super) fn recede_one(&mut self) {
        let Some(b) = (0..9).find(|&b| self.creep_rec.counts[b] != 0) else { return };
        let rnd = self.rng.rand(0x1b);
        let r = &mut self.creep_rec;
        let mut n = r.heads[b];
        for _ in 0..((r.counts[b] as u32 * rnd) >> 15) {
            n = r.nodes[n as usize].next;
        }
        r.unlink(n);
        r.hash_remove(n);
        let (x, y) = (r.nodes[n as usize].x as i32, r.nodes[n as usize].y as i32);
        let w = self.terrain.width as i32;
        self.terrain.tile_flags[(y * w + x) as usize] &= !0x1000_0000;
        self.remove_creep_tile(x, y);
        self.creep_rec.push(n, 9);
    }

    /// 0x414560 with callback 0x49def0 (no other source counts): the creep in `t`'s
    /// creep box at (x, y) goes at once (a dying extractor).
    pub(super) fn creep_clear_under(&mut self, d: &Data, t: u16, x: i32, y: i32) {
        let ([l, top, r, b], ellipse) = self.creep_box(d, t, x, y, true);
        let mut dy = top * 32 - y + 0x10;
        for ty in top..=b {
            let mut dx = l * 32 - x + 0x10;
            for tx in l..=r {
                if self.creep_at(tx, ty) && (!ellipse || dy * dy * 0x100 + dx * dx * 100 < 0x9c_4001) {
                    self.remove_creep_tile(tx, ty);
                }
                dx += 32;
            }
            dy += 32;
        }
    }

    /// 0x413fa0 with 0x413aa0: the tile under the creep comes back.
    fn remove_creep_tile(&mut self, x: i32, y: i32) {
        let i = (y * self.terrain.width as i32 + x) as usize;
        let saved = self.terrain.creep_saved[i];
        if saved != 0 {
            self.terrain.mtxm[i] = saved;
            self.terrain.creep_saved[i] = 0;
            self.creep_tile_changed(x, y, 1, saved);
        }
        self.creep_edges_around(x, y);
    }

    /// 0x47de40 / 0x47df90: the creep a removed `t` at (x, y) kept alive starts to
    /// recede, tile by tile, unless another source covers it. False when the pool ran
    /// out (the caller tries again later).
    pub(super) fn creep_start_receding(&mut self, d: &Data, t: u16, x: i32, y: i32, colony: bool) -> bool {
        let ([l, top, r, b], ellipse) = self.creep_box(d, t, x, y, colony);
        let w = self.terrain.width as i32;
        let mask = if t != 0x95 { 0x1800_0000 } else { 0x1000_0000 };
        let mut dy = top * 32 - y + 0x10;
        for ty in top..=b {
            let mut dx = l * 32 - x + 0x10;
            for tx in l..=r {
                let f = self.terrain.tile_flags[(ty * w + tx) as usize];
                if self.creep_at(tx, ty)
                    && (!ellipse || dy * dy * 0x100 + dx * dx * 100 < 0x9c_4001)
                    && f & mask == 0
                    && !self.creep_source_covers(d, tx, ty)
                    && !self.mark_receding(tx, ty)
                {
                    return false;
                }
                dx += 32;
            }
            dy += 32;
        }
        self.dead_removal_counter = 0;
        true
    }

    /// 0x47df90: `u`'s creep starts to recede (a completed colony counts as one).
    pub(super) fn creep_recede_unit(&mut self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let (t, p, done) = (un.unit_type, un.fl.pos, un.status & 1 != 0);
        self.creep_start_receding(d, t, p.0 as i32, p.1 as i32, done)
    }

    /// 0x413d10 with 0x47d860: a zerg building (other than the one going away) keeps
    /// creep on tile (tx, ty).
    fn creep_source_covers(&mut self, d: &Data, tx: i32, ty: i32) -> bool {
        // 0x413d10: a 0x500 x 0x320 box around the tile's centre, clamped to the map
        // (the far edge only when the near one was not clamped).
        let (cx, cy) = (tx * 32 + 0x10, ty * 32 + 0x10);
        let mut b = [cx - 0x280, cy - 0x190, cx + 0x280, cy + 0x190];
        if b[0] < 0 {
            b[0] = 0;
        } else if b[2] >= self.terrain.width as i32 * 32 {
            b[2] = self.terrain.width as i32 * 32 - 1;
        }
        if b[1] < 0 {
            b[1] = 0;
        } else if b[3] >= self.terrain.height as i32 * 32 {
            b[3] = self.terrain.height as i32 * 32 - 1;
        }
        // 0x47d860: any grounded building whose creep (0x414180) reaches the tile.
        let start = self.units_in_finder_box_now([b[0] as i16, b[1] as i16, b[2] as i16, b[3] as i16]);
        let found = self.search.results(start);
        self.search.pop();
        found.into_iter().any(|u| {
            let un = &self.units[u as usize];
            if un.status & 2 == 0 {
                return false;
            }
            let t = un.unit_type;
            // 0x414180: a zerg building.
            if d.dat.units.flags[t as usize] & 1 == 0 || d.dat.units.staredit_group_flags[t as usize] & 1 == 0 {
                return false;
            }
            let (x, y) = self.unit_sprite_pos(u);
            let (x, y) = (x as i32, y as i32);
            let ([l, top, r, bot], _) = self.creep_box(d, t, x, y, un.status & 1 != 0);
            if tx < l || tx > r || ty < top || ty > bot {
                return false;
            }
            let (dy, dx) = (ty * 32 - y + 0x10, tx * 32 - x + 0x10);
            // 0x413850
            dy * dy * 0x100 + dx * dx * 100 <= 0x9c_4000
        })
    }

    /// 0x45ce90: creep under a finished Zerg building `t` at (x, y) (creep sources, and
    /// buildings that don't need creep), with a ring around buildings flagged 0x80000000.
    pub(super) fn creep_under_building(&mut self, d: &Data, t: u16, x: i32, y: i32) {
        let colony = matches!(t, 0x83 | 0x8f | 0x84 | 0x85 | 0x90 | 0x92);
        if !colony && d.dat.units.flags[t as usize] & 0x2_0000 != 0 {
            return;
        }
        let v = d.dat.units.placement_box[t as usize];
        let (pw, ph) = (v as i16 as i32, (v as i32) >> 16);
        let (tx, ty, w, h) = ((x - pw / 2) / 32, (y - ph / 2) / 32, pw / 32, ph / 32);
        self.set_tile_flags(tx, ty, w, h, 0x800_0000, false);
        if d.dat.units.flags[t as usize] & 0x8000_0000 != 0 {
            self.creep_fill(tx - 2, ty - 1, w + 4, h + 2, true);
        } else {
            self.creep_fill(tx, ty, w, h, false);
        }
        self.set_tile_flags(tx, ty, w, h, 0x800_0000, true);
    }

    /// 0x472e50 (clear) / 0x472eb0 (set): `mask` on a w x h tile box.
    fn set_tile_flags(&mut self, tx: i32, ty: i32, w: i32, h: i32, mask: u32, set: bool) {
        let mw = self.terrain.width as i32;
        for row in 0..h {
            for col in 0..w {
                let f = &mut self.terrain.tile_flags[((ty + row) * mw + tx + col) as usize];
                *f = if set { *f | mask } else { *f & !mask };
            }
        }
    }

    /// 0x45cd50: creep on the free tiles of a w x h box (without its corners when
    /// `ring`).
    fn creep_fill(&mut self, tx: i32, ty: i32, w: i32, h: i32, ring: bool) {
        let (mw, mh) = (self.terrain.width as u32, self.terrain.height as u32);
        for row in 0..h {
            let y = (ty + row) as u32;
            if y >= mh {
                continue;
            }
            for col in 0..w {
                let x = (tx + col) as u32;
                let corner = (col == 0 || col == w - 1) && (row == 0 || row == h - 1);
                if x >= mw || (ring && corner) {
                    continue;
                }
                let f = self.terrain.tile_flags[(y as i16 as i32 * mw as i32 + x as i16 as i32) as usize];
                let below = (y as i16).wrapping_add(1);
                if f & 0x2080_0000 == 0
                    && (below as u16 as u32 == mh || self.terrain.tile_flags[(below as i32 * mw as i32 + x as i16 as i32) as usize] & 0x80_0000 == 0)
                    && f & 0x800_0000 == 0
                {
                    self.add_creep_tile(x as i32, y as i32);
                    self.minimap_terrain = true;
                }
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
        let r = self.rng.rand(0x1a) >> 4;
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
            if !near_screen {
                self.refresh_shown_tile(i, x, y);
            }
        }
    }

    /// 0x49c780 (each map draw, from the game screen's callback 0x4bd580): the screen's
    /// 0xe rows of up to 0x15 tiles from its top-left tile, the area 0x49c4c0 leaves
    /// out, brought up to date the same way (0x49c620 per row).
    /// 0x49c620: one row of shown tiles refreshed from MTXM (inline here, per row).
    pub fn refresh_screen_tiles(&mut self) {
        let (w, h) = (self.terrain.width as u32, self.terrain.height as u32);
        let (sx, sy) = (self.screen_tile.0 as u32, self.screen_tile.1 as u32);
        let n = w.saturating_sub(sx).min(0x15);
        for y in sy..(sy + 0xe).min(h) {
            for x in sx..sx + n {
                let i = (y * w + x) as usize;
                if self.tile_seen_locally(i) {
                    self.refresh_shown_tile(i, x, y);
                }
            }
        }
    }

    /// The shared step of 0x49c4c0 / 0x49c620: a tile whose shown megatile differs from
    /// MTXM's takes it, with the creep flag and the edges around it (0x413b30).
    fn refresh_shown_tile(&mut self, i: usize, x: u32, y: u32) {
        let raw = self.terrain.mtxm[i];
        let mega = self.terrain.tileset.megatile(raw);
        if mega == self.terrain.megatiles[i] & 0x7fff {
            return;
        }
        self.terrain.megatiles[i] = mega;
        let f = &mut self.terrain.tile_flags[i];
        *f = if raw & 0x7ff0 == 0x10 { *f | 0x4000_0000 } else { *f & !0x4000_0000 };
        self.creep_edges_around(x as i32, y as i32);
        self.minimap_terrain = true; // 0x59cb58
    }
}

/// One creep-recession node (0x659b10, 0x400 of 0x14 bytes).
#[derive(Clone, Copy, Debug)]
pub struct RecedeNode {
    pub(crate) next: u32,
    pub(crate) prev: u32,
    pub(crate) hnext: u32,
    pub(crate) hprev: u32,
    pub(crate) x: u8,
    pub(crate) y: u8,
    /// Neighbour-count bucket 0..=8, 9 = free.
    pub(crate) bucket: u8,
}

/// Creep tiles waiting to recede (0x47d660): buckets by creep-neighbour count
/// (0x658ae8, counts 0x65eb14; bucket 9 is the free list) and a tile hash (0x658b10).
#[derive(Clone, Debug)]
pub struct Recession {
    pub(crate) nodes: Vec<RecedeNode>,
    pub(crate) heads: [u32; 10],
    pub(crate) counts: [u16; 10],
    pub(crate) hash: Vec<u32>,
}

impl Recession {
    /// 0x47d660: every node free, pushed in pool order (the last one heads the list).
    pub fn new() -> Recession {
        let mut r = Recession {
            nodes: vec![RecedeNode { next: NIL, prev: NIL, hnext: NIL, hprev: NIL, x: 0, y: 0, bucket: 9 }; 0x400],
            heads: [NIL; 10],
            counts: [0; 10],
            hash: vec![NIL; 0x400],
        };
        for n in 0..0x400 {
            r.push(n, 9);
        }
        r
    }

    /// 0x65eb26: free nodes.
    pub fn free(&self) -> u16 {
        self.counts[9]
    }

    fn key(x: u8, y: u8) -> usize {
        (y as usize * 0x11 + x as usize) & 0x3ff
    }

    /// Insert node `n` at the head of bucket `b`.
    fn push(&mut self, n: u32, b: u8) {
        let head = self.heads[b as usize];
        let node = &mut self.nodes[n as usize];
        node.bucket = b;
        node.next = head;
        node.prev = NIL;
        if head != NIL {
            self.nodes[head as usize].prev = n;
        }
        self.heads[b as usize] = n;
        self.counts[b as usize] += 1;
    }

    /// Unlink node `n` from its bucket.
    fn unlink(&mut self, n: u32) {
        let RecedeNode { next, prev, bucket, .. } = self.nodes[n as usize];
        self.counts[bucket as usize] -= 1;
        if self.heads[bucket as usize] == n {
            self.heads[bucket as usize] = next;
        }
        if next != NIL {
            self.nodes[next as usize].prev = prev;
        }
        if prev != NIL {
            self.nodes[prev as usize].next = next;
        }
        self.nodes[n as usize].next = NIL;
        self.nodes[n as usize].prev = NIL;
    }

    /// 0x47d5f0: hash node `n` by its tile.
    fn hash_insert(&mut self, n: u32) {
        let k = Self::key(self.nodes[n as usize].x, self.nodes[n as usize].y);
        let head = self.hash[k];
        self.nodes[n as usize].hnext = head;
        self.nodes[n as usize].hprev = NIL;
        if head != NIL {
            self.nodes[head as usize].hprev = n;
        }
        self.hash[k] = n;
    }

    /// 0x47d320: unhash node `n`.
    fn hash_remove(&mut self, n: u32) {
        let RecedeNode { hnext, hprev, x, y, .. } = self.nodes[n as usize];
        let k = Self::key(x, y);
        if self.hash[k] == n {
            self.hash[k] = hnext;
        }
        if hnext != NIL {
            self.nodes[hnext as usize].hprev = hprev;
        }
        if hprev != NIL {
            self.nodes[hprev as usize].hnext = hnext;
        }
        self.nodes[n as usize].hnext = NIL;
        self.nodes[n as usize].hprev = NIL;
    }

    /// 0x47d2c0: the node of tile (x, y).
    fn find(&self, x: u8, y: u8) -> Option<u32> {
        let mut n = self.hash[Self::key(x, y)];
        while n != NIL {
            let node = &self.nodes[n as usize];
            if node.x == x && node.y == y {
                return Some(n);
            }
            n = node.hnext;
        }
        None
    }
}
