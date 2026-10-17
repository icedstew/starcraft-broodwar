//! Larvae (CUnitZerg.cpp): hatcheries keep up to three, spawned beside them on creep.
//! A hatchery's union +0xd0 / +0xd2 / +0xd4 / +0xd6 are left / up / right / down
//! "busy" weights (workers returning cargo raise them, they decay every delivery), and
//! +0xca is the larva timer.

use super::list::NIL;
use super::{Data, State};

const LARVA: u16 = 0x23;
/// Hatchery side weights by side: left, up, right, down.
const WEIGHT: [usize; 4] = [0xd0, 0xd2, 0xd4, 0xd6];

impl State {
    fn side_weight(&self, h: u32, side: usize) -> i16 {
        self.units[h as usize].u16_at(WEIGHT[side]) as i16
    }

    /// 0x4e8cb0: a larva fits at `p` (unclamped, on creep) and can wiggle to `p + (dx, dy)`
    /// (unclamped, walkable-ish tile flags 0x410000, no building).
    fn larva_spot_ok(&self, d: &Data, p: (i32, i32), dx: i16, dy: i16) -> bool {
        let mut c = (p.0 as i16, p.1 as i16);
        self.clamp_unit_pos(d, LARVA, &mut c);
        if c.0 as i32 != p.0 || c.1 as i32 != p.1 {
            return false;
        }
        let w = self.terrain.width as i32;
        if self.terrain.tile_flags[((c.1 as i32 / 32) * w + c.0 as i32 / 32) as usize] & 0x40_0000 == 0 {
            return false;
        }
        let mut n = (c.0.wrapping_add(dx), c.1.wrapping_add(dy));
        self.clamp_unit_pos(d, LARVA, &mut n);
        if n.0 as i32 != p.0 + dx as i32 || n.1 as i32 != p.1 + dy as i32 {
            return false;
        }
        self.tile_flags_match(n.0, n.1, 0x41_0000, 0x800_0000)
    }

    /// 0x4e8e10: where hatchery `h` puts its next larva: the middle of its least busy
    /// side (weights under 101), else either end of a side.
    fn larva_spot(&self, d: &Data, h: u32) -> Option<(i32, i32)> {
        let (hx, hy) = self.unit_sprite_pos(h);
        let v = d.dat.units.dimensions[self.units[h as usize].unit_type as usize];
        let [l, u, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let top = hy.wrapping_sub(u) as i32;
        let bottom = dn.wrapping_add(hy) as i32;
        let left = hx.wrapping_sub(l) as i32;
        let right = r.wrapping_add(hx) as i32;
        let (hx, hy) = (hx as i32, hy as i32);
        // Side order: down, left, right, up; (side, spot, wiggle).
        let mid = [(3, (hx, bottom + 10), (0, 0x16)), (0, (left - 10, hy), (-0x16, 0)), (2, (right + 10, hy), (0x16, 0)), (1, (hx, top - 10), (0, -0x16))];
        let mut best = 0x65;
        let mut found = None;
        for (side, p, (dx, dy)) in mid {
            if self.side_weight(h, side) < best && self.larva_spot_ok(d, p, dx, dy) {
                best = self.side_weight(h, side);
                found = Some(p);
                if side == 1 {
                    return found;
                }
            }
        }
        if found.is_some() {
            return found;
        }
        let half_w = (r.wrapping_add(l).wrapping_add(1) as i16) as i32 / 2;
        let half_h = (dn.wrapping_add(u).wrapping_add(1) as i16) as i32 / 2;
        let ends = [
            (3, [(hx - half_w, bottom + 10), (half_w + hx, bottom + 10)], (0, 0x16)),
            (0, [(left - 10, hy - half_h), (left - 10, half_h + hy)], (-0x16, 0)),
            (2, [(right + 10, hy - half_h), (right + 10, half_h + hy)], (0x16, 0)),
            (1, [(hx - half_w, top - 10), (half_w + hx, top - 10)], (0, -0x16)),
        ];
        for (side, spots, (dx, dy)) in ends {
            // The bottom side is tried regardless of its weight.
            if side != 3 && self.side_weight(h, side) >= best {
                continue;
            }
            if let Some(&p) = spots.iter().find(|&&p| self.larva_spot_ok(d, p, dx, dy)) {
                best = self.side_weight(h, side);
                found = Some(p);
                if side == 1 {
                    return found;
                }
            }
        }
        found
    }

    /// 0x49d660: a larva for hatchery `h`, made complete on the spot.
    pub fn spawn_larva(&mut self, d: &Data, h: u32) {
        let Some((x, y)) = self.larva_spot(d, h) else { return };
        let player = self.units[h as usize].player;
        let Some(l) = self.create_unit(d, LARVA, x as i16, y as i16, player) else { return };
        self.finish_unit_pre(d, l);
        self.finish_unit_post(d, l);
        self.units[l as usize].connected_unit = h;
        self.set_larva_side(d, l);
        if self.units[h as usize].ai != NIL {
            self.ai_building_joins_town(d, h, l);
        }
    }

    /// 0x4e8da0: which side of its hatchery a larva is on (order state 0 left, 1 above,
    /// 2 right / below-right, 3 otherwise).
    fn set_larva_side(&mut self, d: &Data, l: u32) {
        let h = self.units[l as usize].connected_unit;
        let (hx, hy) = self.unit_sprite_pos(h);
        let (lx, ly) = self.unit_sprite_pos(l);
        let v = d.dat.units.dimensions[self.units[h as usize].unit_type as usize];
        let [dl, du, dr] = [v as i16, (v >> 16) as i16, (v >> 32) as i16];
        self.units[l as usize].order_state = if lx < hx.wrapping_sub(dl) {
            0
        } else if ly < hy.wrapping_sub(du) {
            1
        } else {
            (lx <= dr.wrapping_add(hx)) as u8 + 2
        };
    }

    /// 0x4ea780: hatchery `h` grows a larva every 38 frames while it has fewer than 3.
    pub fn hatchery_spawn_larva(&mut self, d: &Data, h: u32) {
        let hu = &self.units[h as usize];
        if hu.order_process_timer != 0 || hu.player == 0xb {
            return;
        }
        let (x, y) = self.unit_sprite_pos(h);
        let b = [x.wrapping_sub(0x100), (y as u16).wrapping_sub(0x100) as i16, x.wrapping_add(0x100), (y as u16).wrapping_add(0x100) as i16];
        // 0x4e8280 with 0x4e8c80: count this hatchery's shown larvae, up to 3.
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let mut count = 0;
        for c in found {
            let cu = &self.units[c as usize];
            if cu.unit_type == LARVA && self.sprites[cu.sprite as usize].flags & super::sprite::sflag::HIDDEN == 0 && cu.connected_unit == h {
                count += 1;
                if count >= 3 {
                    break;
                }
            }
        }
        if count >= 3 {
            return;
        }
        let t = self.units[h as usize].u8_at(0xca);
        if t != 0 {
            self.units[h as usize].set_u8_at(0xca, t - 1);
            return;
        }
        self.units[h as usize].set_u8_at(0xca, 0x25);
        let Some((x, y)) = self.larva_spot(d, h) else {
            self.units[h as usize].set_u8_at(0xca, 3);
            return;
        };
        let player = self.units[h as usize].player;
        let Some(l) = self.create_unit(d, LARVA, x as i16, y as i16, player) else { return };
        self.finish_unit_pre(d, l);
        self.finish_unit_post(d, l);
        self.units[l as usize].connected_unit = h;
        self.set_larva_side(d, l);
        if self.units[h as usize].ai != NIL {
            self.ai_building_joins_town(d, h, l);
        }
    }

    /// 0x4e92b0: a worker `w` delivered to hatchery `h`: every side weight decays by
    /// one and the side the worker stands on gets 25 busier (capped near 100).
    pub fn hatchery_note_delivery(&mut self, d: &Data, h: u32, w: u32) {
        for side in [1, 3, 0, 2] {
            let v = self.side_weight(h, side);
            if v != 0 {
                self.units[h as usize].set_u16_at(WEIGHT[side], (v - 1) as u16);
            }
        }
        let (hx, hy) = self.unit_sprite_pos(h);
        let (wx, wy) = self.unit_sprite_pos(w);
        let v = d.dat.units.dimensions[self.units[h as usize].unit_type as usize];
        let [dl, du, _, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let side = if wy < hy.wrapping_sub(du) {
            1
        } else if dn.wrapping_add(hy) < wy {
            3
        } else if wx < hx.wrapping_sub(dl) {
            0
        } else {
            2
        };
        let v = self.side_weight(h, side);
        if v < 100 {
            self.units[h as usize].set_u16_at(WEIGHT[side], (v + 0x19) as u16);
        }
    }

    /// 0x4e8bc0: the tile under pixel (x, y) has creep.
    fn creep_under(&self, x: i16, y: i16) -> bool {
        let w = self.terrain.width as i32;
        self.terrain.tile_flags[((y as i32 / 32) * w + x as i32 / 32) as usize] & 0x40_0000 != 0
    }

    /// 0x4e93e0: (x, y) is not past the hatchery edge on larva `l`'s side.
    fn larva_in_bounds(&self, d: &Data, l: u32, x: i32, y: i32) -> bool {
        let h = self.units[l as usize].connected_unit;
        if h == NIL {
            return true;
        }
        let (hx, hy) = self.unit_sprite_pos(h);
        let v = d.dat.units.dimensions[self.units[h as usize].unit_type as usize];
        let [dl, du, dr, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let (x, y) = (x as u32, y as u32);
        match self.units[l as usize].order_state {
            0 => (hx.wrapping_sub(dl) as u16 as u32) >= x,
            1 => (hy.wrapping_sub(du) as u16 as u32) >= y,
            2 => x >= dr.wrapping_add(hx) as u16 as u32,
            3 => y >= dn.wrapping_add(hy) as u16 as u32,
            _ => panic!("ChkLarvaStray: 1"),
        }
    }

    /// 0x4e94b0: the middle of larva `l`'s hatchery side, 10 px out.
    fn larva_home(&self, d: &Data, l: u32) -> Option<(i32, i32)> {
        let h = self.units[l as usize].connected_unit;
        if h == NIL {
            return None;
        }
        let (hx, hy) = self.unit_sprite_pos(h);
        let v = d.dat.units.dimensions[self.units[h as usize].unit_type as usize];
        let [dl, du, dr, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        Some(match self.units[l as usize].order_state {
            0 => (hx.wrapping_sub(dl) as i32 - 10, hy as i32),
            1 => (hx as i32, hy.wrapping_sub(du) as i32 - 10),
            2 => (dr.wrapping_add(hx) as i32 + 10, hy as i32),
            3 => (hx as i32, dn.wrapping_add(hy) as i32 + 10),
            _ => panic!("ChkLarvaStray: 2"),
        })
    }

    /// 0x4e9f90: Larva: stay near the hatchery, wriggling about on its side; die off
    /// creep.
    pub(super) fn order_larva(&mut self, d: &Data, l: u32) {
        let h = self.units[l as usize].connected_unit;
        if h != NIL && !self.box_distance_within(d, l, 10, h) {
            self.move_to_unit(d, l, h);
            let (x, y) = self.unit_sprite_pos(h);
            let f = &mut self.units[l as usize].fl;
            if f.next_target_waypoint != (x, y) {
                f.next_target_waypoint = (x, y);
            }
        }
        let pos = self.unit_sprite_pos(l);
        let un = &self.units[l as usize];
        let mt = un.fl.move_target;
        if mt != pos {
            let t = un.move_target_unit;
            if t == NIL || mt == self.unit_sprite_pos(t) || self.creep_under(mt.0, mt.1) {
                return;
            }
        }
        let h = self.units[l as usize].connected_unit;
        if (h == NIL || self.units[l as usize].move_target_unit != h) && !self.creep_under(pos.0, pos.1) {
            return self.kill_unit(d, l);
        }
        let r = self.rng.rand(0x12);
        let mut x = pos.0 as i32 + if r & 8 != 0 { 10 } else { -10 };
        let mut y = pos.1 as i32 + if r & 0x80 != 0 { 10 } else { -10 };
        if x as u16 >= self.map_px.0 || y as u16 >= self.map_px.1 || !self.creep_under(x as i16, y as i16) {
            return;
        }
        if !self.larva_in_bounds(d, l, x, y) {
            if self.larva_in_bounds(d, l, pos.0 as i32, pos.1 as i32) {
                return;
            }
            if let Some(p) = self.larva_home(d, l) {
                (x, y) = p;
            }
        }
        self.move_to_facing(d, l, (x as i16, y as i16));
    }
}
