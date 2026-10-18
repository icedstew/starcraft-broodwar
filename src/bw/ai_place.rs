//! Where a computer player puts a new building: a 64x64 tile grid around a spot is
//! filled with the tiles the building fits on, then narrowed down by region group, AI
//! region safety, path distance, elevation, unbuildable tiles nearby and room for
//! resources, add-ons and traffic; the free tile closest (by path) to a reference
//! point wins (0x4465c0 and the grid filters it runs).

use super::build::placement;
use super::list::NIL;
use super::path::{approx_distance, region_at};
use super::{Data, State};

/// 64x64 tiles centred on the search spot (row major, `grid[row * 64 + col]`).
type Grid = [u8; 0x1000];

fn empty(g: &Grid) -> bool {
    g.iter().all(|&c| c == 0)
}

/// Tile coordinate of a pixel (rounding toward zero, as the game's `/ 32` does).
fn tile(v: i16) -> i32 {
    v as i32 / 32
}

/// 0x51b108: lines 0x443610 clears through the grid, (col, row, step col, step row).
const LINES: [(i32, i32, i32, i32); 13] = [
    (0x20, 0, 0, 1),
    (0, 0x20, 1, 0),
    (0x1f, 0, 0, 1),
    (0, 0x1f, 1, 0),
    (0x21, 0, 0, 1),
    (0, 0x21, 1, 0),
    (0x1e, 0, 0, 1),
    (0x1d, 0, 0, 1),
    (0, 0x1e, 1, 0),
    (0x1c, 0, 0, 1),
    (0, 0x1d, 1, 0),
    (0x1b, 0, 0, 1),
    (0, 0x1c, 1, 0),
];

impl State {
    /// 0x4465c0: a spot for `builder` (a worker, or the building that makes an add-on)
    /// to place unit type `t` near `pos`, within `radius` (path tiles) if possible.
    /// Returns the building's centre in pixels.
    pub(super) fn ai_find_build_spot(&mut self, d: &Data, builder: u32, t: u16, pos: (i16, i16), radius: u32) -> Option<(i16, i16)> {
        let mut pos = pos;
        let (mut cx, mut cy) = (tile(pos.0), tile(pos.1));
        let player = self.units[builder as usize].player;
        if d.dat.units.flags[t as usize] & 2 != 0 {
            // An add-on goes where the builder's add-on slot is.
            let (bw, bh) = placement(d, self.units[builder as usize].unit_type);
            let ap = d.dat.units.addon_position[t as usize];
            let (ax, ay) = (ap as i16 as i32 / 32, (ap >> 16) as i16 as i32 / 32);
            let x = (cx - (bw / 2) / 32 + ax) as i16;
            let y = (cy + (-(bh / 2)) / 32 + ay) as i16;
            if self.placement_check(d, builder, player, x, y, t, 0, false, true, false) == 0 {
                let (bw, bh) = placement(d, self.units[builder as usize].unit_type);
                return Some(((bw / 2 + (x as i32 - ax) * 32) as i16, (bh / 2 + (y as i32 - ay) * 32) as i16));
            }
        }
        // 0x4442e0 picks a region whose centre the search starts from; 0x440420 moves the
        // search to the building to power.
        let mut alt_region = 0x9c4u32;
        let mut keep_lines = true;
        let mut ty = t;
        match t {
            0x6a | 0x83 | 0x9a => keep_lines = false,
            0x6b | 0x6c => ty = 0x6a,
            0x73 => ty = 0x72,
            0x75 | 0x76 => ty = 0x74,
            0x78 => ty = 0x71,
            0x8f | 0x7d => {
                if t == 0x7d || self.ai.players[player as usize].flags & 0x80 == 0 {
                    if let Some(r) = self.ai_defended_region(player) {
                        alt_region = r;
                        keep_lines = false;
                    }
                }
            }
            0x9c => {
                let u = self.ai_building_to_power(player, pos);
                if u != NIL {
                    pos = self.unit_sprite_pos(u);
                    cx = tile(pos.0);
                    cy = tile(pos.1);
                    keep_lines = false;
                }
            }
            _ => {}
        }
        let mut grid: Grid = [0; 0x1000];
        self.builder_override = builder;
        self.ai_place_grid(d, &mut grid, ty, player, pos, radius);
        self.builder_override = NIL;
        if keep_lines {
            Self::grid_lines(&mut grid);
        }
        let mut reference = (cx, cy);
        if alt_region != 0x9c4 {
            let c = self.sai.regions[alt_region as usize].center;
            reference = (((c.0 >> 8) as i16) as i32 / 32, ((c.1 >> 8) as i16) as i32 / 32);
        }
        let mut pass = 0;
        let (mut best, mut best_far) = (99_999_999u32, 99_999_999u32);
        let (mut spot, mut spot_far) = ((0i32, 0i32), (0i32, 0i32));
        loop {
            for row in 0..64 {
                let y = cy - 0x20 + row;
                for col in 0..64 {
                    let x = cx - 0x20 + col;
                    if grid[(row * 64 + col) as usize] == 0 {
                        continue;
                    }
                    let dist = self.ai_path_tiles((reference.0 * 32, reference.1 * 32), (x * 32, y * 32));
                    if dist >= best {
                        continue;
                    }
                    if dist > radius {
                        if dist < best_far {
                            best_far = dist;
                            spot_far = (x, y);
                        }
                    } else {
                        best = dist;
                        spot = (x, y);
                    }
                }
            }
            if best != 99_999_999 {
                break;
            }
            if best_far != 99_999_999 {
                spot = spot_far;
                break;
            }
            if alt_region == 0x9c4 {
                return None;
            }
            pass += 1;
            reference = (cx, cy);
            if pass >= 2 {
                return None;
            }
        }
        let (w, h) = placement(d, ty);
        Some(((w / 2 + spot.0 * 32) as i16, (h / 2 + spot.1 * 32) as i16))
    }

    /// 0x4442e0: of the candidate regions (0x69262c), the one farthest (path tiles) from
    /// `player`'s nearest bunker / creep colony / sunken colony; the only one if single.
    fn ai_defended_region(&mut self, player: u8) -> Option<u32> {
        let cands: Vec<u32> = self.ai.area_candidates.iter().map(|&r| r as u32).collect();
        match cands.len() {
            0 => return None,
            1 => return Some(cands[0]),
            _ => {}
        }
        let pts: Vec<(i32, i32)> = cands
            .iter()
            .map(|&r| {
                let c = self.sai.regions[r as usize].center;
                (((c.0 >> 8) as i16) as i32, ((c.1 >> 8) as i16) as i32)
            })
            .collect();
        let mut dist = vec![99_999_999u32; cands.len()];
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            let next = un.player_next;
            if un.sprite != NIL && un.order != 0 && matches!(un.unit_type, 0x7d | 0x8f | 0x92) {
                let (ux, uy) = self.unit_sprite_pos(u);
                for (i, p) in pts.iter().enumerate() {
                    let dd = self.ai_path_tiles((ux as i32, uy as i32), *p);
                    if dd < dist[i] {
                        dist[i] = dd;
                    }
                }
            }
            u = next;
        }
        let (mut best, mut idx) = (dist[0], 0);
        for i in 1..dist.len() {
            if best < dist[i] {
                best = dist[i];
                idx = i;
            }
        }
        Some(cands[idx])
    }

    /// 0x440420: the nearest building of `player` to `pos` on the whole map (0x4e8320
    /// with callback 0x440190), NIL if none.
    fn ai_building_to_power(&mut self, player: u8, pos: (i16, i16)) -> u32 {
        let ix = super::finder::search(&self.finder.x, self.finder.count, true, pos.0 as i32) as i32;
        let iy = super::finder::search(&self.finder.y, self.finder.count, true, pos.1 as i32) as i32;
        let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
        self.nearest_unit(b, NIL, pos, [ix - 1, iy - 1, ix, iy], &mut |st: &mut State, c: u32| {
            st.units[c as usize].status & 0x400 != 0 && st.units[c as usize].player == player
        })
    }

    /// 0x443ac0: clear the keep-out patterns (17x17 down to 5x5 tiles, starting at
    /// `stage`) around `player`'s turrets / bunkers / cannons (only type `t` for the
    /// first group) and creep / sunken / spore colonies; a smaller pattern if all
    /// would be cleared.
    fn grid_defense_spots(&self, g: &mut Grid, player: u8, pos: (i16, i16), t: u16, mut stage: u32) {
        const PATTERNS: [(usize, &str); 4] = [
            (17, "......xxxxx..........xxxxxxxxx.......xxxxxxxxxxx.....xxxxxxxxxxxxx...xxxxxxxxxxxxxxx..xxxxxxxxxxxxxxx.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.xxxxxxxxxxxxxxx..xxxxxxxxxxxxxxx...xxxxxxxxxxxxx.....xxxxxxxxxxx.......xxxxxxxxx..........xxxxx......"),
            (13, "....xxxxx.......xxxxxxx.....xxxxxxxxx...xxxxxxxxxxx.xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.xxxxxxxxxxx...xxxxxxxxx.....xxxxxxx.......xxxxx...."),
            (9, "...xxx....xxxxxxx..xxxxxxx.xxxxxxxxxxxxxxxxxxxxxxxxxxx.xxxxxxx..xxxxxxx....xxx..."),
            (5, ".xxx.xxxxxxxxxxxxxxx.xxx."),
        ];
        loop {
            let backup = *g;
            let mut u = self.player_units[player as usize];
            while u != NIL {
                let un = &self.units[u as usize];
                if un.sprite != NIL && un.order != 0 {
                    let hit = match un.unit_type {
                        0x7c | 0x7d | 0xa2 => un.unit_type == t,
                        0x8f | 0x90 | 0x92 => true,
                        _ => false,
                    };
                    if hit {
                        let Some(&(n, pat)) = PATTERNS.get(stage as usize) else { return };
                        let pat = pat.as_bytes();
                        let (ux, uy) = self.unit_sprite_pos(u);
                        let mut row = (uy as i32 - pos.1 as i32) / 32 - (n as i32 >> 1) + 0x1f;
                        let col0 = (ux as i32 - pos.0 as i32) / 32 - (n as i32 >> 1) + 0x1f;
                        for r in 0..n {
                            if row > 0x3f {
                                break;
                            }
                            if row >= 0 {
                                for c in 0..n {
                                    let col = col0 + c as i32;
                                    if (0..64).contains(&col) && pat[r * n + c] == b'x' {
                                        g[(row * 64 + col) as usize] = 0;
                                    }
                                }
                            }
                            row += 1;
                        }
                    }
                }
                u = self.units[u as usize].player_next;
            }
            if !empty(g) {
                return;
            }
            *g = backup;
            if stage > 2 {
                return;
            }
            stage += 1;
        }
    }

    /// 0x4448d0: drop the tiles where a cannon would already be powered, unless that
    /// would leave nothing.
    fn grid_pylon_spots(&self, d: &Data, g: &mut Grid, player: u8, pos: (i16, i16)) {
        let backup = *g;
        for row in 0..64 {
            for col in 0..64 {
                let i = row * 64 + col;
                if g[i] != 0 {
                    let (x, y) = Self::cell_px(pos, row, col);
                    if self.psi_powered(d, 0xa2, x, y, player) {
                        g[i] = 0;
                    }
                }
            }
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x4461b0: fill the grid with the tiles `t` fits on and narrow it down.
    fn ai_place_grid(&mut self, d: &Data, g: &mut Grid, t: u16, player: u8, pos: (i16, i16), radius: u32) {
        self.grid_fill(d, g, t, player, pos, radius);
        self.grid_same_group(g, pos);
        if t != 0x9c {
            self.grid_safe_regions(g, t, player, pos);
        }
        self.grid_radius(g, pos, radius);
        if empty(g) {
            return;
        }
        self.grid_same_level(g, pos);
        self.grid_unbuildable(d, g, t, pos);
        let town_hall = matches!(t, 0x6a | 0x83 | 0x9a);
        if !town_hall {
            self.grid_resources(d, g, t, player, pos);
        }
        self.grid_addons(d, g, t, player, pos);
        match t {
            0x7c | 0xa2 => self.grid_defense_spots(g, player, pos, t, 0),
            // Bunkers and creep colonies keep spacing only without a defended region.
            0x7d => {
                if self.ai_defended_region(player).is_none() {
                    self.grid_defense_spots(g, player, pos, t, 0);
                }
            }
            0x8f => {
                if self.ai.players[player as usize].flags & 0x80 != 0 || self.ai_defended_region(player).is_none() {
                    self.grid_defense_spots(g, player, pos, t, 0);
                }
            }
            0x9c => self.grid_pylon_spots(d, g, player, pos),
            _ => {
                if !town_hall {
                    self.grid_spacing(d, g, t, player, pos);
                }
            }
        }
    }

    /// 0x445d60: tiles within `radius` of `pos` where `t` can be placed (with room for
    /// its add-on, for buildings that take one).
    fn grid_fill(&mut self, d: &Data, g: &mut Grid, t: u16, player: u8, pos: (i16, i16), radius: u32) {
        let addon = matches!(t, 0x6a | 0x71 | 0x72 | 0x74);
        let start = 0x20u32.wrapping_sub(radius >> 1);
        let end = radius.wrapping_add(start);
        let (cx, cy) = (tile(pos.0), tile(pos.1));
        for row in 0..64u32 {
            if row >= end {
                return;
            }
            if row < start {
                continue;
            }
            let y = cy - 0x20 + row as i32;
            for col in 0..64u32 {
                if col >= end {
                    break;
                }
                if col < start {
                    continue;
                }
                let x = cx - 0x20 + col as i32;
                if addon && self.placement_check(d, NIL, player, (x + 4) as i16, (y + 1) as i16, 0x7c, 0, false, true, false) != 0 {
                    continue;
                }
                if self.placement_check(d, NIL, player, x as i16, y as i16, t, 0, false, true, false) == 0 {
                    g[(row * 64 + col) as usize] = 1;
                }
            }
        }
    }

    /// Pixel of grid cell (row, col) as the filters sample it: `pos` - 0x400 + 32 per cell.
    fn cell_px(pos: (i16, i16), row: usize, col: usize) -> (i32, i32) {
        (pos.0 as i32 - 0x400 + col as i32 * 32, pos.1 as i32 - 0x400 + row as i32 * 32)
    }

    /// 0x443900: only tiles in `pos`'s region group.
    fn grid_same_group(&self, g: &mut Grid, pos: (i16, i16)) {
        let group = |st: &State, x: i32, y: i32| st.sai.regions[region_at(&st.sai, x, y) as usize].group;
        let want = group(self, pos.0 as i32, pos.1 as i32);
        for row in 0..64 {
            for col in 0..64 {
                let i = row * 64 + col;
                if g[i] != 0 {
                    let (x, y) = Self::cell_px(pos, row, col);
                    if group(self, x, y) != want {
                        g[i] = 0;
                    }
                }
            }
        }
    }

    /// 0x443870: (not for town halls) only tiles whose region's neighbours are all safe
    /// for `player`'s AI (0x436c10).
    fn grid_safe_regions(&self, g: &mut Grid, t: u16, player: u8, pos: (i16, i16)) {
        if matches!(t, 0x6a | 0x83 | 0x9a) {
            return;
        }
        for row in 0..64 {
            for col in 0..64 {
                let i = row * 64 + col;
                if g[i] != 0 {
                    let (x, y) = Self::cell_px(pos, row, col);
                    if !self.ai_neighbors_safe(player, region_at(&self.sai, x, y)) {
                        g[i] = 0;
                    }
                }
            }
        }
    }

    /// 0x436c10: none of region `r`'s neighbours (except the id-0 ones counted at +0x21)
    /// is under attack for `player`'s AI.
    fn ai_neighbors_safe(&self, player: u8, r: u16) -> bool {
        let reg = &self.sai.regions[r as usize];
        let n = reg.neighbor_count as i32 - reg.zero_neighbors as i8 as i32;
        for i in 0..n.max(0) as usize {
            let id = self.sai.neighbor(r as usize, i);
            let ar = &self.ai.regions[player as usize][id as usize];
            if ar.state == 3 || ar.air_target != NIL || ar.ground_target != NIL || ar.flags & 0x20 != 0 {
                return false;
            }
        }
        true
    }

    /// 0x444e20: only tiles within `radius` path tiles of `pos`.
    fn grid_radius(&mut self, g: &mut Grid, pos: (i16, i16), radius: u32) {
        for row in 0..64 {
            for col in 0..64 {
                let i = row * 64 + col;
                if g[i] != 0 {
                    let (x, y) = Self::cell_px(pos, row, col);
                    if self.ai_path_tiles((pos.0 as i32, pos.1 as i32), (x, y)) > radius {
                        g[i] = 0;
                    }
                }
            }
        }
    }

    /// 0x444100: path distance in tiles between two pixels: straight within a region,
    /// through the region centres otherwise, 9999 without a path.
    pub(super) fn ai_path_tiles(&mut self, a: (i32, i32), b: (i32, i32)) -> u32 {
        let ra = region_at(&self.sai, a.0, a.1);
        let rb = region_at(&self.sai, b.0, b.1);
        let straight = |p: (i32, i32), q: (i32, i32)| approx_distance(p.0, q.0, p.1, q.1);
        if ra == rb {
            return (straight(a, b) as i32 / 32) as u32;
        }
        if self.sai.regions[ra as usize].group != self.sai.regions[rb as usize].group {
            return 9999;
        }
        let pa = (a.0 as i16, a.1 as i16);
        let pb = (b.0 as i16, b.1 as i16);
        let mut ctx = super::path::PathCtx { unit: NIL, start: pa, target: pb, goal: pb, ..Default::default() };
        ctx.start_region = ra;
        ctx.target_region = rb;
        let mut sai = std::mem::replace(&mut self.sai, super::sai::Sai::empty());
        let ok = super::path::find_region_path(&mut sai, &mut ctx);
        self.sai = sai;
        if !ok {
            return 9999;
        }
        if ctx.region_count <= 2 {
            return (straight(a, b) as i32 / 32) as u32;
        }
        let center = |st: &State, r: u16| {
            let c = st.sai.regions[r as usize].center;
            (((c.0 >> 8) as i16) as i32, ((c.1 >> 8) as i16) as i32)
        };
        let mut prev = center(self, ctx.regions[1]);
        let mut sum = straight(a, prev);
        let count = ctx.region_count as i32 - 1;
        let mut i = 2;
        while i < count {
            let c = center(self, ctx.regions[i as usize]);
            sum = sum.wrapping_add(straight(prev, c));
            prev = c;
            i += 1;
        }
        sum.wrapping_add(straight(prev, b)) >> 5
    }

    /// 0x4436c0: only tiles at `pos`'s elevation (kept as is if none would be left).
    fn grid_same_level(&self, g: &mut Grid, pos: (i16, i16)) {
        let backup = *g;
        let want = self.los_elevation(pos.0 as i32, pos.1 as i32);
        for row in 0..64 {
            for col in 0..64 {
                let i = row * 64 + col;
                if g[i] != 0 {
                    let (x, y) = Self::cell_px(pos, row, col);
                    if self.los_elevation(x, y) != want {
                        g[i] = 0;
                    }
                }
            }
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x443790: clear the tiles `t` can't use around a `w` x `h` tile obstacle at tile
    /// (tx, ty), `margin` tiles wide.
    #[allow(clippy::too_many_arguments)]
    fn grid_clear_box(d: &Data, g: &mut Grid, margin: i32, tx: i32, ty: i32, w: u32, h: u32, t: u16, pos: (i16, i16)) {
        let (bw, bh) = placement(d, t);
        let (bw, bh) = (bw / 32, bh / 32);
        let cols = (bw + margin * 2 - 1) as u32 + w;
        let rows = (bh + margin * 2 - 1) as u32 + h;
        let col0 = tx - (w >> 1) as i32 - bw - margin + 1 - tile(pos.0) + 0x20;
        let mut row = ty - (h >> 1) as i32 - bh - margin - tile(pos.1) + 0x21;
        for _ in 0..rows {
            let mut col = col0;
            for _ in 0..cols {
                if (0..64).contains(&col) && (0..64).contains(&row) {
                    g[(row * 64 + col) as usize] = 0;
                }
                col += 1;
            }
            row += 1;
        }
    }

    /// 0x443e30: keep a tile of room around unbuildable map tiles (flag 0x10000 clear),
    /// unless that would leave nothing.
    fn grid_unbuildable(&self, d: &Data, g: &mut Grid, t: u16, pos: (i16, i16)) {
        let backup = *g;
        let (mw, mh) = (self.terrain.width as i32, self.terrain.height as i32);
        let mut ty = tile(pos.1) - 0x20;
        for _ in 0..64 {
            if ty >= mh {
                break;
            }
            if ty >= 0 {
                let mut tx = tile(pos.0) - 0x20;
                for _ in 0..64 {
                    if tx >= mw {
                        break;
                    }
                    if tx >= 0 && self.terrain.tile_flags[(ty as i16 as i32 * mw + tx as i16 as i32) as usize] & 0x10000 == 0 {
                        Self::grid_clear_box(d, g, 1, tx, ty, 1, 1, t, pos);
                    }
                    tx += 1;
                }
            }
            ty += 1;
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x443f40: clear around unit `u` (its placement box), `margin` tiles wide.
    fn grid_clear_unit(&self, d: &Data, g: &mut Grid, u: u32, t: u16, pos: (i16, i16), margin: i32) {
        let (ux, uy) = self.unit_sprite_pos(u);
        let (w, h) = placement(d, self.units[u as usize].unit_type);
        Self::grid_clear_box(d, g, margin, tile(ux), tile(uy), (w / 32) as u32, (h / 32) as u32, t, pos);
    }

    /// 0x444ce0: (not for add-ons) keep 2 tiles from `player`'s resource depots and
    /// every resource (except for bunkers), unless that would leave nothing.
    fn grid_resources(&self, d: &Data, g: &mut Grid, t: u16, player: u8, pos: (i16, i16)) {
        if d.dat.units.flags[t as usize] & 2 != 0 {
            return;
        }
        let backup = *g;
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && (self.is_drop_off(d, u) || d.dat.units.flags[un.unit_type as usize] & 0x2000 != 0) {
                self.grid_clear_unit(d, g, u, t, pos, 2);
            }
            u = self.units[u as usize].player_next;
        }
        if t != 0x7d {
            let mut u = self.player_units[11];
            while u != NIL {
                let un = &self.units[u as usize];
                if un.sprite != NIL && un.order != 0 && d.dat.units.flags[un.unit_type as usize] & 0x2000 != 0 {
                    self.grid_clear_unit(d, g, u, t, pos, 2);
                }
                u = self.units[u as usize].player_next;
            }
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x443cc0: keep the add-on spots of `player`'s command centers, factories,
    /// starports and science facilities free.
    fn grid_addons(&self, d: &Data, g: &mut Grid, t: u16, player: u8, pos: (i16, i16)) {
        let backup = *g;
        let bh = (placement(d, t).1 / 32) as u32;
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && matches!(un.unit_type, 0x6a | 0x71 | 0x72 | 0x74) {
                let (ux, uy) = self.unit_sprite_pos(u);
                let mut col = tile(ux) - tile(pos.0) + 0x22;
                let row0 = tile(uy) - tile(pos.1) + 0x21;
                for _ in 0..2 {
                    let mut row = row0;
                    for _ in 0..bh + 1 {
                        if !(0..64).contains(&col) {
                            break;
                        }
                        if (0..64).contains(&row) {
                            g[(row * 64 + col) as usize] = 0;
                        }
                        row -= 1;
                    }
                    col += 1;
                }
            }
            u = self.units[u as usize].player_next;
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x4449a0: keep room around `player`'s production buildings and defenses, and for
    /// production buildings and defenses being placed only tiles whose 8 neighbours were
    /// all free before that; unless that would leave nothing.
    fn grid_spacing(&self, d: &Data, g: &mut Grid, t: u16, player: u8, pos: (i16, i16)) {
        let backup = *g;
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 {
                let margin = match un.unit_type {
                    0x6f | 0x7d | 0x8f => Some(1),
                    0x71 | 0x7c | 0x9b | 0xa0 | 0xa2 => Some(2),
                    _ => None,
                };
                if let Some(m) = margin {
                    self.grid_clear_unit(d, g, u, t, pos, m);
                }
            }
            u = self.units[u as usize].player_next;
        }
        if matches!(t, 0x6f | 0x71 | 0x7c | 0x7d | 0x9b | 0xa0 | 0xa2) {
            // The neighbours are looked up in the grid as it was before the clearing.
            let free = |r: i32, c: i32| (0..64).contains(&r) && (0..64).contains(&c) && backup[(r * 64 + c) as usize] != 0;
            for row in 0..64i32 {
                for col in 0..64i32 {
                    let i = (row * 64 + col) as usize;
                    if g[i] == 0 {
                        continue;
                    }
                    let ok = free(row - 1, col - 1)
                        && free(row - 1, col)
                        && free(row - 1, col + 1)
                        && free(row, col - 1)
                        && free(row, col + 1)
                        && free(row + 1, col - 1)
                        && free(row + 1, col)
                        && free(row + 1, col + 1);
                    if !ok {
                        g[i] = 0;
                    }
                }
            }
        }
        if empty(g) {
            *g = backup;
        }
    }

    /// 0x443610: clear the lines through the middle of the grid (keeping walkways) as
    /// long as something is left; if a line would empty it, nothing is cleared.
    fn grid_lines(g: &mut Grid) {
        let backup = *g;
        for &(c, r, dc, dr) in LINES.iter() {
            let (mut col, mut row) = (c, r);
            for _ in 0..64 {
                g[(row * 64 + col) as usize] = 0;
                col += dc;
                row += dr;
            }
            if empty(g) {
                *g = backup;
                return;
            }
        }
    }
}
