//! Player commands as the game receives them (1.16.1 dispatch 0x4865d0): right-click
//! (0x4c2370), targeted orders (0x4c2320), stop, hold position, return cargo, train and
//! build. `sel` is the issuing player's selection (a 0x6284e8 row), walked like 0x49a850.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Group move target (0x1c bytes, built by 0x49a8c0 and read by 0x49a500).
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupTarget {
    /// +0x0: the clicked point.
    pub target: (i16, i16),
    /// +0x4: the unit's own destination (0x49a500 output).
    pub result: (i16, i16),
    /// +0x8 / +0xc: base point for air and for ground units (ground may be moved to a
    /// walkable spot).
    pub air: (i16, i16),
    pub ground: (i16, i16),
    /// +0x10: formation offset (base minus group centre).
    pub offset: (i16, i16),
    /// +0x14: summed unit box areas, +0x18: sqrt(2 * area).
    pub area: i32,
    pub radius: u16,
    /// +0x1a: 1/2 keep x/y offsets, 4 target inside the group, 8 has ground units.
    pub flags: u8,
}

/// 0x49ab00 ignores these orders (they arrive through other commands).
fn targeted_ignored(order: u8) -> bool {
    matches!(order, 0 | 0xf | 0x14 | 0x19 | 0x1d..=0x21 | 0x24 | 0x2e | 0x47..=0x49 | 0x50..=0x53 | 0x55..=0x59 | 0x9b)
}

/// 0x47b2e0: unit types with a rally point.
pub fn has_rally(t: u16) -> bool {
    matches!(t, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b)
}

impl State {
    /// 0x49a850: live units of a selection (dying units are skipped).
    pub fn live_selection(&self, sel: &[u32]) -> Vec<u32> {
        sel.iter()
            .copied()
            .take(12)
            .take_while(|&u| u != NIL)
            .filter(|&u| {
                let un = &self.units[u as usize];
                un.sprite != NIL && (un.order != 0 || un.order_state != 1)
            })
            .collect()
    }

    /// 0x4754f0: give `u` an order from a command, queued (shift) when the order allows.
    /// `kind`: 0 no target, 1 unit target, 2 position + unit type, 3 position.
    #[allow(clippy::too_many_arguments)]
    pub fn command_order(&mut self, d: &Data, u: u32, kind: u8, queued: bool, order: u8, target: u32, unit_type: u16, pos: (i16, i16)) {
        let cur = self.units[u as usize].order;
        if queued && d.dat.orders.queueable[order as usize] != 0 && !matches!(cur, 2 | 3 | 0x17 | 0x5d | 0x98 | 0xaf) {
            if self.units[u as usize].order_queue_count >= 8 || self.order_count >= 0x708 {
                // 0x48cf00: "too many queued orders" message.
                return;
            }
            // 0x474b90
            match kind {
                0 => self.order_queue_append(d, u, order, 0xe4, (0, 0), NIL),
                1 => {
                    let p = if target != NIL { self.unit_sprite_pos(target) } else { (0, 0) };
                    self.order_queue_append(d, u, order, 0xe4, p, target)
                }
                2 => self.order_queue_append(d, u, order, unit_type, pos, NIL),
                _ => self.order_queue_append(d, u, order, 0xe4, pos, NIL),
            };
            return;
        }
        // 0x475470
        match kind {
            0 => self.order_now(d, u, order),
            1 => self.order_with_target(d, u, order, target),
            2 => self.order_pos_type(d, u, order, unit_type, pos.0, pos.1),
            _ => self.order_issue_now(d, u, order, pos, NIL, 0xe4),
        }
    }

    /// 0x49c9a0: region at a pixel (split tiles report their first region).
    fn region_rgn1(&self, x: i16, y: i16) -> u16 {
        let v = self.sai.tiles[((y as i32 / 32) * 256 + x as i32 / 32) as usize];
        if v > 0x1fff { self.sai.splits[v as usize - 0x2000].rgn1 } else { v }
    }

    /// 0x49a270: region `r` is `base` or one of its walkable neighbors.
    fn region_near(&self, r: u16, base: u16) -> bool {
        if r == base {
            return true;
        }
        let n = self.sai.regions[base as usize].walkable_neighbors as usize;
        (0..n).any(|i| self.sai.neighbor(base as usize, i) == r)
    }

    /// 0x42d740: a `w` x `h` box centred on `p` fits the map and terrain.
    fn square_fits(&mut self, d: &Data, p: (i16, i16), w: i32, h: i32) -> bool {
        let l = (p.0 as i32 - w / 2) as i16;
        let t = (p.1 as i32 - h / 2) as i16;
        if l < 0 || l as i32 + w >= self.map_px.0 as i32 || t < 0 || t as i32 + h >= self.map_px.1 as i32 {
            return false;
        }
        if !self.point_walkable(d, p.0, p.1) {
            return false;
        }
        let Some(mut c) = self.sai.contours.take() else { return false };
        let (l, u) = (((w - 1) / 2) as i16, ((h - 1) / 2) as i16);
        let (r, dn) = ((w - w / 2 - 1) as i16, (h - h / 2 - 1) as i16);
        c.dims_a = [u, r.wrapping_neg(), dn.wrapping_neg(), l];
        c.dims_b = [u.wrapping_add(1), (-1i16).wrapping_sub(r), (-1i16).wrapping_sub(dn), l.wrapping_add(1)];
        let ok = super::collide::point_clear(&c, p.0, p.1);
        self.sai.contours = Some(c);
        ok
    }

    /// 0x42f650: the region a ground path from `from` toward `to` ends in.
    fn reachable_region(&mut self, from: (i16, i16), to: (i16, i16)) -> u16 {
        let r1 = super::path::region_at(&self.sai, from.0 as i32, from.1 as i32);
        let r2 = super::path::region_at(&self.sai, to.0 as i32, to.1 as i32);
        if r1 == r2 {
            return r2;
        }
        let mut ctx = super::path::PathCtx { unit: NIL, start: from, target: to, goal: to, start_region: r1, target_region: r2, ..Default::default() };
        let mut sai = std::mem::replace(&mut self.sai, super::sai::Sai::empty());
        let ok = super::path::find_region_path(&mut sai, &mut ctx);
        self.sai = sai;
        if !ok {
            return r2;
        }
        ctx.regions[ctx.region_count as usize - 1]
    }

    /// 0x4e4380: move `p` to the nearest open spot for a box of `dims` (left, up, right,
    /// down) in the region reachable from `from`.
    fn nearest_open_spot(&mut self, d: &Data, p: &mut (i16, i16), dims: [i16; 4], from: (i16, i16)) {
        let walkable = self.point_walkable(d, p.0, p.1);
        let [l, u, r, dn] = dims;
        if let Some(c) = self.sai.contours.as_mut() {
            c.dims_a = [u, r.wrapping_neg(), dn.wrapping_neg(), l];
            c.dims_b = [u.wrapping_add(1), (-1i16).wrapping_sub(r), (-1i16).wrapping_sub(dn), l.wrapping_add(1)];
        }
        let mut region = 0;
        let same = walkable && {
            region = self.region_rgn1(p.0, p.1);
            let fr = self.region_rgn1(from.0, from.1);
            self.sai.regions[region as usize].group == self.sai.regions[fr as usize].group
        };
        if !same {
            region = self.reachable_region(from, *p);
        }
        let (mw, mh) = self.map_px;
        *p = super::short_path::closest_tile_in_region(&self.sai, mw, mh, *p, region);
        super::short_path::open_spot(&self.sai, mw, mh, [l.wrapping_neg(), u.wrapping_neg(), r, dn], p);
    }

    /// 0x49a8c0: shared destination data for a group order at `t.target`.
    pub fn group_target(&mut self, d: &Data, sel: &[u32], t: &mut GroupTarget, adjust_ground: bool) {
        t.flags = 0;
        t.area = 0;
        t.radius = 0;
        t.air = t.target;
        t.ground = t.target;
        let units = self.live_selection(sel);
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, 0i32, i32::MAX, 0i32);
        let (mut sx, mut sy, mut n, mut ground) = (0u32, 0u32, 0u32, false);
        for &u in &units {
            if self.units[u as usize].pathing_flags & 1 != 0 {
                ground = true;
            }
            let (x, y) = self.unit_sprite_pos(u);
            let (x, y) = (x as i32, y as i32);
            sx = sx.wrapping_add(x as u32);
            sy = sy.wrapping_add(y as u32);
            n += 1;
            minx = minx.min(x);
            maxx = maxx.max(x);
            miny = miny.min(y);
            maxy = maxy.max(y);
            let v = d.dat.units.dimensions[self.units[u as usize].unit_type as usize];
            let [l, up, r, dn] = [v as u16, (v >> 16) as u16, (v >> 32) as u16, (v >> 48) as u16];
            let h = dn.wrapping_add(up).wrapping_add(1) as i16 as i32;
            let w = r.wrapping_add(l).wrapping_add(1) as i16 as i32;
            t.area = t.area.wrapping_add(h * w);
        }
        if n == 0 {
            return;
        }
        // fild / fadd / fsqrt, then __ftol (truncates).
        let radius = ((t.area as f64) * 2.0).sqrt() as i64 as u16;
        t.radius = radius;
        if ground && adjust_ground {
            let centre = ((sx / n) as i16, (sy / n) as i16);
            if n == 1 {
                // 0x4e4520
                let u = units[0];
                let ut = self.units[u as usize].unit_type;
                if !self.terrain_fits(d, ut, t.ground.0, t.ground.1) {
                    let v = d.dat.units.dimensions[ut as usize];
                    let dims = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
                    let from = self.unit_sprite_pos(u);
                    let mut g = t.ground;
                    self.nearest_open_spot(d, &mut g, dims, from);
                    t.ground = g;
                }
            } else {
                // 0x4e44b0
                let r = radius as i32;
                if !self.square_fits(d, t.ground, r, r) {
                    let (h1, h2) = ((r / 2) as i16, (r - r / 2) as i16);
                    let mut g = t.ground;
                    self.nearest_open_spot(d, &mut g, [h1, h1, h2, h2], centre);
                    t.ground = g;
                }
            }
            // 0x4020f0
            let (mw, mh) = (self.map_px.0 as i16, self.map_px.1 as i16);
            t.ground.0 = t.ground.0.clamp(0, mw - 1);
            t.ground.1 = t.ground.1.clamp(0, mh - 1);
        }
        if n < 2 {
            return;
        }
        let (tx, ty) = (t.target.0 as i32, t.target.1 as i32);
        if tx <= maxx && tx >= minx && ty <= maxy && ty >= miny {
            t.flags = 4;
            t.offset = (0, 0);
            return;
        }
        let limit = if ground {
            t.flags |= 8;
            0xc0
        } else {
            0x100
        };
        if maxx - minx > limit || maxy - miny > limit {
            return;
        }
        t.offset = (t.air.0.wrapping_sub((sx / n) as i16), t.air.1.wrapping_sub((sy / n) as i16));
        t.flags |= 3;
    }

    /// 0x49a500: `u`'s own destination in a group order.
    pub fn unit_group_destination(&mut self, d: &Data, u: u32, t: &GroupTarget) -> (i16, i16) {
        let ground = self.units[u as usize].pathing_flags & 1 != 0;
        let base = if ground { t.ground } else { t.air };
        let up = self.unit_sprite_pos(u);
        let mut res = t.target;
        if t.flags & 4 != 0 {
            let clamp = |p: i16, b: i16| -> i16 {
                let (p, b) = (p as i32, b as i32);
                (if p < b { p.max(b - 0x20) } else { p.min(b + 0x20) }) as i16
            };
            res = (clamp(up.0, base.0), clamp(up.1, base.1));
        } else {
            if t.flags & 1 != 0 {
                res.0 = up.0.wrapping_add(t.offset.0);
            }
            if t.flags & 2 != 0 {
                res.1 = up.1.wrapping_add(t.offset.1);
            }
        }
        let ut = self.units[u as usize].unit_type;
        self.clamp_unit_pos(d, ut, &mut res);
        if !ground {
            return res;
        }
        let base_region = super::path::region_at(&self.sai, base.0 as i32, base.1 as i32);
        let ok = |st: &mut State, p: (i16, i16)| {
            st.terrain_fits(d, ut, p.0, p.1) && {
                let r = super::path::region_at(&st.sai, p.0 as i32, p.1 as i32);
                st.region_near(r, base_region)
            }
        };
        if ok(self, res) {
            return res;
        }
        let mut dist = super::path::approx_distance(res.0 as i32, base.0 as i32, res.1 as i32, base.1 as i32) as i32;
        if dist == 0 {
            dist = 1;
        }
        let mut step = dist / 4;
        if step < 2 {
            step = dist;
        }
        step = step.min(0x10);
        let mut k = step;
        while k <= dist {
            let p = (
                (((base.0 as i32 - res.0 as i32) * k) / dist) as i16 + res.0,
                (((base.1 as i32 - res.1 as i32) * k) / dist) as i16 + res.1,
            );
            if ok(self, p) {
                return p;
            }
            k += step;
        }
        base
    }

    /// 0x4e6e00: transport `tr` has room for `u`.
    fn transport_has_room(&self, d: &Data, tr: u32, u: u32) -> bool {
        let t = &self.units[tr as usize];
        let un = &self.units[u as usize];
        if t.status & 1 == 0 || self.disabled(tr) || un.status & 0x10 != 0 || un.player != t.player {
            return false;
        }
        if d.dat.units.flags[t.unit_type as usize] & 1 != 0 && (self.race_of(d, u) != 1 || d.dat.units.space_required[un.unit_type as usize] > 1) {
            return false;
        }
        todo_port!("0x4e6c40", "units already loaded in the transport");
        let space = d.dat.units.space_provided[t.unit_type as usize] as u32;
        d.dat.units.space_required[un.unit_type as usize] as u32 <= space
    }

    /// 0x47b150: race of `u` (0 zerg, 1 terran, 2 protoss, 4 other).
    fn race_of(&self, d: &Data, u: u32) -> u8 {
        let g = d.dat.units.staredit_group_flags[self.units[u as usize].unit_type as usize];
        if g & 1 != 0 {
            0
        } else if g & 4 != 0 {
            2
        } else if g & 2 != 0 {
            1
        } else {
            4
        }
    }

    /// 0x4557c0: worker right-click on `t` (0xbd: not a harvest target).
    fn harvest_click(&self, d: &Data, u: u32, t: u32) -> u8 {
        let tt = self.units[t as usize].unit_type;
        if d.dat.units.flags[tt as usize] & 0x800 != 0 {
            return 6;
        }
        let rt = self.units[u as usize].resource_type;
        let powerup = rt != 0 && rt & 3 == 0;
        if (0xb0..=0xb2).contains(&tt) {
            return if powerup { 6 } else { 0x55 };
        }
        if self.is_refinery_of(t, u) || tt == 0xbc {
            return if powerup { 6 } else { 0x51 };
        }
        if self.is_drop_off(d, t) && self.units[t as usize].player == self.units[u as usize].player {
            if rt & 2 != 0 {
                return 0x5a;
            }
            if rt & 1 != 0 {
                return 0x54;
            }
        }
        0xbd
    }

    /// 0x455900: right-click of an ordinary unit.
    fn rc_normal(&self, d: &Data, u: u32, t: u32, ty: u16) -> u8 {
        if self.units[u as usize].status & 2 != 0 {
            return 0x17;
        }
        if t == NIL {
            return if ty == 0xe4 { 6 } else { 0x85 };
        }
        let tt = self.units[t as usize].unit_type;
        if (0xc2..=0xc7).contains(&tt) || d.dat.units.flags[tt as usize] & 0x800 != 0 {
            return 6;
        }
        if self.is_enemy(u, t) {
            return 8;
        }
        if self.can_carry(d, t) && self.transport_has_room(d, t, u) {
            return 0x5c;
        }
        if self.units[t as usize].status & 0x10 != 0 { 6 } else { 0x31 }
    }

    /// 0x455e00: right-click of move-only units and buildings with a rally point.
    fn rc_move(&self, d: &Data, u: u32, t: u32, ty: u16) -> u8 {
        let un = &self.units[u as usize];
        if un.status & 2 != 0 {
            return if has_rally(un.unit_type) { if t != NIL { 0x27 } else { 0x28 } } else { 0x17 };
        }
        if t == NIL {
            if (un.unit_type == 0x2d || un.unit_type == 0x31) && ty != 0xe4 {
                return 0x85;
            }
            return 6;
        }
        // 0x474d90 (order 0x5e): an own unit that fits.
        if self.can_carry(d, u) && self.units[t as usize].player == un.player && self.can_load(d, u, t) {
            return 0x5e;
        }
        if self.can_carry(d, t) && self.transport_has_room(d, t, u) {
            return 0x5c;
        }
        let ts = self.units[t as usize].status;
        if ts & 0x10 != 0 {
            return 6;
        }
        if (un.unit_type == 0x2d || un.unit_type == 0x31) && ts & 0x400_0000 == 0 {
            todo_port!("0x491e80", "queen / medic special right-click");
        }
        if un.unit_type == 0x22 { 0xb1 } else { 0x31 }
    }

    /// 0x455890: right-click of units that can't move (attack or idle).
    fn rc_no_move(&self, d: &Data, u: u32, t: u32) -> u8 {
        let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        if t == NIL {
            return idle;
        }
        let tt = self.units[t as usize].unit_type;
        if (0xc2..=0xc7).contains(&tt) || d.dat.units.flags[tt as usize] & 0x800 != 0 {
            return idle;
        }
        if self.is_enemy(u, t) { 8 } else { idle }
    }

    /// 0x455cc0: SCV right-click (harvest, repair, resume construction).
    fn rc_scv(&self, d: &Data, u: u32, t: u32, ty: u16) -> u8 {
        if t == NIL {
            return self.rc_normal(d, u, t, ty);
        }
        let h = self.harvest_click(d, u, t);
        if h != 0xbd {
            return h;
        }
        let tu = &self.units[t as usize];
        let un = &self.units[u as usize];
        if tu.status & 2 == 0 || tu.status & 1 != 0 {
            if self.alliances[un.player as usize][tu.player as usize] != 0 {
                if d.dat.units.flags[tu.unit_type as usize] & 1 == 0 && self.can_carry(d, t) && self.transport_has_room(d, t, u) {
                    return 0x5c;
                }
                // 0x474d90(0x22): mechanical terran unit below full hp.
                let max = d.dat.units.hit_points[tu.unit_type as usize] as i32;
                if self.race_of(d, t) == 1 && d.dat.units.flags[tu.unit_type as usize] & 0x4000_0000 != 0 && tu.status & 1 != 0 && tu.hp < max {
                    return 0x22;
                }
                if self.can_carry(d, t) && self.transport_has_room(d, t, u) {
                    return 0x5c;
                }
            }
        } else if tu.player == un.player
            && self.race_of(d, t) == self.race_of(d, u)
            && d.dat.units.flags[tu.unit_type as usize] & 2 == 0
            && (tu.connected_unit == NIL || self.units[tu.connected_unit as usize].order_target_unit != t)
        {
            return 0x21;
        }
        self.rc_normal(d, u, t, ty)
    }

    /// 0x455760: `u`'s right-click action (units.dat, with the burrowed-lurker,
    /// rally-building and hallucination cases).
    fn right_click_action(&self, d: &Data, u: u32) -> u8 {
        let un = &self.units[u as usize];
        let mut action = if un.unit_type == 0x67 && un.status & 0x10 != 0 {
            3
        } else {
            let a = d.dat.units.right_click_action[un.unit_type as usize];
            if a == 0 && un.status & 2 != 0 && has_rally(un.unit_type) { 2 } else { a }
        };
        if un.status & 0x4000_0000 != 0 && (action == 4 || action == 5) {
            action = 1;
        }
        action
    }

    /// 0x4f6ef0: RightClickAction: go to a right-clicked fog spot; once it is seen,
    /// act on what is really there.
    pub fn order_right_click_action(&mut self, d: &Data, u: u32) {
        let p = self.units[u as usize].order_target_pos;
        match self.units[u as usize].order_state {
            0 => {
                let ok = self.set_move_target_xy(d, u, p);
                let f = &mut self.units[u as usize].fl;
                if f.next_target_waypoint != p {
                    f.next_target_waypoint = p;
                }
                if ok {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                let i = (p.1 as i32 / 32) * self.terrain.width as i32 + p.0 as i32 / 32;
                let player = self.units[u as usize].player;
                if self.terrain.tile_flags[i as usize] & (1 << (player & 0x1f)) == 0 {
                    let ty = self.units[u as usize].order_unit_type;
                    self.right_click_revealed(d, u, p, ty);
                }
            }
            _ => {}
        }
    }

    /// 0x4f6fa0: orders given on a remembered (fogged) unit (Attack2, MoveToInfest,
    /// MoveToRepair, ...): walk to the spot (movable units), and once it is seen give
    /// the real order (orders.dat "obscured order") on what is there.
    pub(super) fn order_obscured(&mut self, d: &Data, u: u32) {
        let p = self.units[u as usize].order_target_pos;
        match self.units[u as usize].order_state {
            0 => {
                if self.units[u as usize].status & 0x2_0000 != 0 {
                    let ok = self.set_move_target_xy(d, u, p);
                    let f = &mut self.units[u as usize].fl;
                    if f.next_target_waypoint != p {
                        f.next_target_waypoint = p;
                    }
                    if !ok {
                        return;
                    }
                }
                self.units[u as usize].order_state = 1;
            }
            1 => {
                let i = (p.1 as i32 / 32) * self.terrain.width as i32 + p.0 as i32 / 32;
                let player = self.units[u as usize].player;
                if self.terrain.tile_flags[i as usize] & (1 << (player & 0x1f)) != 0 {
                    return;
                }
                let ty = self.units[u as usize].order_unit_type;
                let t = self.fog_unit_at(d, p, ty);
                if t != NIL {
                    let ut = self.units[u as usize].unit_type as usize;
                    let mut o = d.dat.orders.obscured_order[self.units[u as usize].order as usize];
                    if o == 8 {
                        o = d.dat.units.ai_attack_unit[ut];
                        let sub = self.units[u as usize].subunit;
                        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
                            let so = d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize];
                            self.queue_front_target(d, sub, so, t);
                            self.order_done_or_next(d, sub);
                        }
                    }
                    self.queue_front_target(d, u, o, t);
                }
                self.order_done_or_next(d, u);
            }
            _ => {}
        }
    }

    /// 0x455f20: right-click again on the unit of type `ty` now seen at `p`.
    fn right_click_revealed(&mut self, d: &Data, u: u32, p: (i16, i16), ty: u16) {
        let action = self.right_click_action(d, u);
        let t = self.fog_unit_at(d, p, ty);
        let mut ord = self.right_click_order(d, u, action, t, 0xe4);
        if ord == 0x17 {
            self.unit_halt(d, u);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != f.move_target {
                f.next_target_waypoint = f.move_target;
            }
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.order_nothing(d, u);
            } else {
                let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
                self.order_now(d, u, o);
            }
            return;
        }
        if ord == 8 {
            ord = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
        }
        let player = self.units[u as usize].player;
        if self.can_issue_order(d, u, ord, player).is_err() {
            return;
        }
        let rally = has_rally(self.units[u as usize].unit_type);
        match ord {
            0x27 => {
                if rally {
                    // 0x466b40
                    let r = if t != NIL { t } else { u };
                    let rp = self.unit_sprite_pos(r);
                    let un = &mut self.units[u as usize];
                    un.rally_unit = r;
                    un.rally_pos = rp;
                }
            }
            0x28 => {
                if rally {
                    // 0x466910
                    let un = &mut self.units[u as usize];
                    un.rally_unit = NIL;
                    un.rally_pos = p;
                }
            }
            _ => {
                if t != NIL {
                    self.queue_front_target(d, u, ord, t);
                } else {
                    self.order_queue_front(d, u, ord, p);
                }
                self.order_done_or_next(d, u);
            }
        }
    }

    /// The right-click handler table 0x5153fc, by units.dat right-click action.
    fn right_click_order(&self, d: &Data, u: u32, action: u8, t: u32, ty: u16) -> u8 {
        match action {
            1 => self.rc_normal(d, u, t, ty),
            2 => self.rc_move(d, u, t, ty),
            3 => self.rc_no_move(d, u, t),
            4 => {
                // 0x4559c0
                if t != NIL {
                    let h = self.harvest_click(d, u, t);
                    if h != 0xbd {
                        return h;
                    }
                }
                self.rc_normal(d, u, t, ty)
            }
            5 => self.rc_scv(d, u, t, ty),
            _ => 0x17,
        }
    }

    /// 0x487fd0: the unit of type `t` whose placement box holds `p` (a right-click on
    /// what the player remembers under the fog), searched within 0x60 pixels.
    fn fog_unit_at(&mut self, d: &Data, p: (i16, i16), t: u16) -> u32 {
        let b = [p.0.wrapping_sub(0x60), p.1.wrapping_sub(0x60), p.0.wrapping_add(0x60), p.1.wrapping_add(0x60)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        // 0x487a90
        let hit = found.into_iter().find(|&c| {
            let un = &self.units[c as usize];
            if un.unit_type != t {
                return false;
            }
            let (w, h) = super::build::placement(d, t);
            let s = &self.sprites[un.sprite as usize];
            ((w / 2 + s.x as i32 - p.0 as i32) as u32) < w as u32 && ((h / 2 + s.y as i32 - p.1 as i32) as u32) < h as u32
        });
        hit.unwrap_or(NIL)
    }

    /// Right-click command (0x4c2370 -> 0x4560d0) at `pos` on unit `target` (or NIL;
    /// `unit_type` != 0xe4 for a remembered fog unit).
    #[allow(clippy::too_many_arguments)]
    pub fn command_right_click(&mut self, d: &Data, player: u8, sel: &[u32], pos: (i16, i16), mut target: u32, mut unit_type: u16, queued: bool) {
        if pos.0 as u16 >= self.map_px.0 || pos.1 as u16 >= self.map_px.1 {
            return;
        }
        let mut gt = GroupTarget { target: pos, ..Default::default() };
        self.group_target(d, sel, &mut gt, true);
        if target != NIL {
            unit_type = 0xe4;
        } else if unit_type != 0xe4 {
            let i = (pos.1 as i32 / 32) * self.terrain.width as i32 + pos.0 as i32 / 32;
            if self.terrain.tile_flags[i as usize] & (1 << (player & 0x1f)) == 0 {
                target = self.fog_unit_at(d, pos, unit_type);
                unit_type = 0xe4;
            }
        }
        for u in self.live_selection(sel) {
            let action = self.right_click_action(d, u);
            let mut ord = self.right_click_order(d, u, action, target, unit_type);
            if ord == 0x17 {
                continue;
            }
            if ord == 0x27 {
                if has_rally(self.units[u as usize].unit_type) {
                    let r = if target != NIL { target } else { u };
                    let p = self.unit_sprite_pos(r);
                    let un = &mut self.units[u as usize];
                    un.rally_unit = r;
                    un.rally_pos = p;
                }
                continue;
            }
            if u == target {
                continue;
            }
            if ord == 0x28 {
                if has_rally(self.units[u as usize].unit_type) {
                    let un = &mut self.units[u as usize];
                    un.rally_unit = NIL;
                    un.rally_pos = pos;
                }
                continue;
            }
            if ord == 8 {
                let sub = self.units[u as usize].subunit;
                if sub != NIL {
                    let so = d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize];
                    self.command_order(d, sub, 1, queued, so, target, 0xe4, (0, 0));
                }
                ord = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
            }
            if self.can_issue_order(d, u, ord, player).is_err() {
                continue;
            }
            if target != NIL {
                self.command_order(d, u, 1, queued, ord, target, 0xe4, (0, 0));
            } else {
                let p = if unit_type == 0xe4 { self.unit_group_destination(d, u, &gt) } else { pos };
                self.command_order(d, u, 2, queued, ord, NIL, unit_type, p);
            }
            self.units[u as usize].order_flags |= 2;
        }
        let _ = &mut target;
    }

    /// 0x49a410: `u` takes targeted orders at all.
    fn takes_targeted(&self, d: &Data, u: u32, order: u8) -> bool {
        let un = &self.units[u as usize];
        let building = un.status & 2 != 0;
        if building && matches!(order, 0x27 | 0x28 | 0x43 | 0x5f) {
            return true;
        }
        if un.unit_type == 0x67 && un.status & 0x10 != 0 {
            return true;
        }
        let a = d.dat.units.right_click_action[un.unit_type as usize];
        a != 0 || (building && has_rally(un.unit_type))
    }

    /// 0x49a480: `u` may target itself with `order`.
    fn may_target_self(&self, d: &Data, u: u32, order: u8) -> bool {
        let un = &self.units[u as usize];
        if (un.status & 2 != 0 && (order == 0x27 || order == 0x28)) || ((un.unit_type == 0x2e || un.unit_type == 0x34) && order == 0x77) {
            return true;
        }
        order == 0x70 && self.can_carry(d, u)
    }

    /// Targeted order command (0x4c2320 -> 0x49ab00).
    #[allow(clippy::too_many_arguments)]
    pub fn command_targeted_order(&mut self, d: &Data, player: u8, sel: &[u32], pos: (i16, i16), target: u32, unit_type: u16, order: u8, queued: bool) {
        if pos.0 as u16 >= self.map_px.0 || pos.1 as u16 >= self.map_px.1 {
            return;
        }
        let mut gt = GroupTarget { target: pos, ..Default::default() };
        self.group_target(d, sel, &mut gt, d.dat.orders.can_be_obscured[order as usize] != 0);
        if targeted_ignored(order) {
            return;
        }
        for u in self.live_selection(sel) {
            let tech = d.dat.orders.tech[order as usize];
            let ok = if tech == 0x2c {
                self.can_issue_order(d, u, order, player).is_ok()
            } else {
                self.can_use_tech(d, u, tech as u16, player) == Ok(1)
            };
            if !ok || (u == target && !self.may_target_self(d, u, order)) || !self.takes_targeted(d, u, order) {
                continue;
            }
            if self.units[u as usize].order == 0x7d && order != 0 {
                continue;
            }
            let ut = self.units[u as usize].unit_type;
            let mut ord = order;
            let mut sub_ord = 0x17;
            let mut rally = true;
            if ut == 0x22 {
                if order == 0xe || order == 8 {
                    ord = 0xb1;
                    rally = false;
                }
            } else if order == 8 {
                let table = if target != NIL { &d.dat.units.ai_attack_unit } else { &d.dat.units.ai_attack_move };
                ord = table[ut as usize];
                if ord == 0x17 {
                    continue;
                }
                let sub = self.units[u as usize].subunit;
                if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
                    sub_ord = table[self.units[sub as usize].unit_type as usize];
                }
            }
            if rally && ord == 0x27 {
                let r = if target != NIL { target } else { u };
                let p = self.unit_sprite_pos(r);
                let un = &mut self.units[u as usize];
                un.rally_unit = r;
                un.rally_pos = p;
            } else if rally && ord == 0x28 {
                let un = &mut self.units[u as usize];
                un.rally_unit = NIL;
                un.rally_pos = pos;
            } else {
                let sub = self.units[u as usize].subunit;
                if target != NIL {
                    self.command_order(d, u, 1, queued, ord, target, 0xe4, (0, 0));
                    if sub_ord != 0x17 {
                        self.command_order(d, sub, 1, queued, sub_ord, target, 0xe4, (0, 0));
                    }
                } else {
                    let p = if unit_type == 0xe4 { self.unit_group_destination(d, u, &gt) } else { pos };
                    self.command_order(d, u, 2, queued, ord, NIL, unit_type, p);
                    if sub_ord != 0x17 {
                        self.command_order(d, sub, 2, queued, sub_ord, NIL, unit_type, p);
                    }
                }
            }
            self.units[u as usize].order_flags |= 2;
        }
    }

    /// 0x4c2f30: Stim Packs: units that may use it (tech 0) and have more than 10 hp
    /// lose 10 hp and are stimmed for 37 (x8) frames.
    pub fn command_stim(&mut self, d: &Data, player: u8, sel: &[u32]) {
        for u in self.live_selection(sel) {
            if self.can_use_tech(d, u, 0, player) != Ok(1) || self.units[u as usize].hp <= 0xa00 {
                continue;
            }
            // An unused synced random number (call site 0x1f), then the stim sound (UI).
            self.rng.rand(0x1f);
            self.apply_damage_pub(d, u, 0xa00, NIL, 0xff, true);
            if self.units[u as usize].stim_timer < 0x25 {
                self.units[u as usize].stim_timer = 0x25;
                self.update_speed(d, u);
            }
        }
    }

    /// Stop command (0x4c2190): the 0x46dc20 checks for order 1 (Stop).
    pub fn command_stop(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            if self.can_issue_order(d, u, 1, player).is_ok() {
                self.command_order(d, u, 1, queued, 1, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// Hold position command (0x4c20c0).
    pub fn command_hold(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            let order = match self.units[u as usize].unit_type {
                0x48 | 0x52 => 0x39,
                0x53 | 0x51 => 0x3e,
                0x2d | 0x31 => 0x6c,
                0x2f | 0x32 => 0x88,
                0x22 => 0xb2,
                _ => 0x6b,
            };
            if self.can_issue_order(d, u, order, player).is_ok() {
                self.command_order(d, u, 1, queued, order, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// Return cargo command (0x4c2040).
    pub fn command_return_cargo(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            let rt = self.units[u as usize].resource_type;
            let order = if rt & 2 != 0 {
                0x5a
            } else if rt & 1 != 0 {
                0x54
            } else {
                continue;
            };
            if self.can_issue_order(d, u, order, player).is_ok() {
                self.command_order(d, u, 1, queued, order, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// Train command (0x4c1c20): only with exactly one unit selected.
    pub fn command_train_sel(&mut self, d: &Data, player: u8, sel: &[u32], t: u16) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        if t < 0x6a && self.can_make_unit(d, u, t, player) == Ok(1) {
            self.command_train(d, u, t);
            // 0x4c36c0: UI refresh.
        }
    }

    /// Build command (0x4c23c0): one selected worker builds `t` at tile (tx, ty).
    #[allow(clippy::too_many_arguments)]
    pub fn command_build_sel(&mut self, d: &Data, player: u8, sel: &[u32], order: u8, tx: i16, ty: i16, t: u16) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        if tx as u16 >= self.terrain.width || ty as u16 >= self.terrain.height {
            return;
        }
        // 0x48dbd0
        let ok = match order {
            0x19 | 0x1e | 0x1f | 0x20 | 0x24 => self.can_make_unit(d, u, t, player) == Ok(1),
            // Landing: the building's own type.
            0x47 => self.units[u as usize].unit_type == t,
            0x9b | 0x2e => {
                todo_port!("0x48dbd0", "nydus exit / addon builds (type in AX)");
                false
            }
            _ => false,
        };
        if ok && self.can_issue_order(d, u, order, player).is_ok() {
            // 0x48e190: addons place by the building's spot.
            if d.dat.units.flags[t as usize] & 2 != 0 {
                self.command_build_addon(d, u, order, tx, ty, t);
            } else {
                self.command_build(d, u, order, tx, ty, t);
            }
        }
    }
}

impl State {
    /// 0x474d90: `target` suits `order` given by `u` (Err holds the stat_txt message).
    fn order_target_valid(&self, d: &Data, u: u32, order: u8, target: u32) -> Result<(), u16> {
        let un = &self.units[u as usize];
        match order {
            0x22 => {
                if target == NIL || self.race_of(d, target) != 1 {
                    return Err(0x374);
                }
                let t = &self.units[target as usize];
                let max = d.dat.units.hit_points[t.unit_type as usize] as i32;
                if d.dat.units.flags[t.unit_type as usize] & 0x4000_0000 == 0 || t.status & 1 == 0 || t.hp >= max {
                    return Err(0x373);
                }
                Ok(())
            }
            0x4f => {
                if target == NIL {
                    return Ok(());
                }
                let t = &self.units[target as usize];
                let err = 0x37d + self.race_of(d, target) as u16;
                if t.unit_type == 0xbc {
                    return Err(err);
                }
                // 0x4688e0: mineral fields and geysers.
                if matches!(t.unit_type, 0xb0..=0xb2 | 0xbc) {
                    return Ok(());
                }
                if d.dat.units.flags[t.unit_type as usize] & 0x2000 == 0 {
                    return Err(0x37c);
                }
                if un.player == t.player { Ok(()) } else { Err(0x380) }
            }
            0x5e => {
                if target == NIL || un.player != self.units[target as usize].player {
                    return Err(0x37a);
                }
                if self.transport_has_room(d, u, target) {
                    return Ok(());
                }
                let need = d.dat.units.space_required[self.units[target as usize].unit_type as usize];
                Err(if need <= d.dat.units.space_provided[un.unit_type as usize] { 0x37b } else { 0x37a })
            }
            0xb1 => {
                if target == NIL {
                    return Ok(());
                }
                if self.can_use_tech(d, u, 0x2c, un.player).is_err() {
                    return Err(0);
                }
                let t = &self.units[target as usize];
                let max = d.dat.units.hit_points[t.unit_type as usize] as i32;
                if t.status & 6 != 0 || d.dat.units.flags[t.unit_type as usize] & 0x1_0000 == 0 || t.hp >= max {
                    return Err(0x52d);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// 0x4746d0: a remembered fog unit of type `ty` suits `order`.
    fn fog_target_valid(&self, d: &Data, u: u32, order: u8, ty: u16) -> Result<(), u16> {
        match order {
            0x23 if !(0x6a..=0x7f).contains(&ty) => Err(0x374),
            0x50 => {
                let err = 0x37d + self.race_of(d, u) as u16;
                if ty == 0xbc {
                    return Err(err);
                }
                if !matches!(ty, 0xb0..=0xb2) {
                    todo_port!("0x474500", "fog gather target check");
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// 0x46f5b0: a map click while targeting. Picks the order for what was clicked
    /// (`target` unit, a fog unit of type `ty`, or ground), checks it client side and
    /// sends the targeted-order command. Err holds the message shown instead.
    #[allow(clippy::too_many_arguments)]
    pub fn target_click(&mut self, d: &Data, player: u8, sel: &[u32], modes: (u8, u8, u8), pos: (i16, i16), target: u32, mut ty: u16, queued: bool) -> Result<(), u16> {
        let order = if target != NIL { modes.1 } else if ty == 0xe4 { modes.0 } else { modes.2 };
        let o = order as usize;
        let weapon = d.dat.orders.weapon[o];
        let weapon_targeting = d.dat.orders.use_weapon_targeting[o] != 0;
        let tech = d.dat.orders.tech[o];
        let energy = if tech < 0x2c { (d.dat.techdata.energy_cost[tech as usize] as u32) << 8 } else { 0 };
        if d.dat.orders.obscured_order[o] == 0xbd {
            ty = 0xe4;
        }
        let mut err: Option<u16> = None;
        let (mut any_ok, mut has_energy, mut castable) = (false, false, false);
        for u in self.live_selection(sel) {
            if self.disabled(u) {
                continue;
            }
            if !weapon_targeting {
                if tech == 0x2c {
                    let r = if ty == 0xe4 { self.order_target_valid(d, u, order, target) } else { self.fog_target_valid(d, u, order, ty) };
                    if let Err(e) = r {
                        err = Some(e);
                    }
                } else {
                    todo_port!("0x492020", "spell target range / validity");
                    castable = true;
                    if self.units[u as usize].energy as u32 >= energy || self.game_options & 0x800 != 0 {
                        has_energy = true;
                    }
                }
            } else if weapon == 0x82 {
                if target == NIL {
                    if self.units[u as usize].unit_type == 0x32 {
                        any_ok = true;
                    } else {
                        todo_port!("0x475c80", "attack a remembered fog unit");
                    }
                } else if self.right_click_kind(d, u) == 3 {
                    todo_port!("0x476430", "immobile attacker: target in range (0x4764d0)");
                    any_ok |= self.can_attack_pub(d, u, target);
                } else {
                    any_ok |= self.can_attack_pub(d, u, target);
                }
            } else {
                todo_port!("0x475ce0", "weapon-targeted spell validity");
                any_ok = true;
                if self.units[u as usize].energy as u32 >= energy || self.game_options & 0x800 != 0 {
                    has_energy = true;
                }
            }
        }
        if !weapon_targeting {
            if energy != 0 {
                if !castable {
                    return Err(err.unwrap_or(0x36c));
                }
                if !has_energy {
                    return Err(0);
                }
            }
        } else {
            if !any_ok || err.is_some() {
                // 0x46efe0 / 0x48ee30: error, but a plain attack is sent anyway.
                if weapon != 0x82 {
                    return Err(err.unwrap_or(0));
                }
                let handle = if target != NIL { self.unit_handle(target) } else { 0 };
                self.command_targeted_packet(d, player, sel, pos, handle, ty, order, queued);
                return Ok(());
            }
            if energy != 0 && !has_energy {
                return Err(0);
            }
        }
        if let Some(e) = err {
            return Err(e);
        }
        let handle = if target != NIL { self.unit_handle(target) } else { 0 };
        self.command_targeted_packet(d, player, sel, pos, handle, ty, order, queued);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn command_targeted_packet(&mut self, d: &Data, player: u8, sel: &[u32], pos: (i16, i16), handle: u32, ty: u16, order: u8, queued: bool) {
        let mut p = vec![0x15];
        p.extend_from_slice(&(pos.0 as u16).to_le_bytes());
        p.extend_from_slice(&(pos.1 as u16).to_le_bytes());
        p.extend_from_slice(&(handle as u16).to_le_bytes());
        p.extend_from_slice(&ty.to_le_bytes());
        p.push(order);
        p.push(queued as u8);
        self.execute_command(d, player, sel, &p);
    }

    /// Run one command packet as the turn loop does (0x487191): with the synced RNG on.
    pub fn execute_command(&mut self, d: &Data, player: u8, sel: &[u32], p: &[u8]) {
        let was = self.rng.enabled;
        self.rng.enabled = true;
        self.receive_command(d, player, sel, p);
        self.rng.enabled = was;
    }

    /// 0x4865d0: run one command packet from `player`, whose selection is `sel`.
    fn receive_command(&mut self, d: &Data, player: u8, sel: &[u32], p: &[u8]) {
        let u16_at = |o: usize| p.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        let byte = |o: usize| p.get(o).copied().unwrap_or(0);
        match byte(0) {
            0x0c => {
                // 0x4c23c0
                let (order, tx, ty, t) = (byte(1), u16_at(2), u16_at(4), u16_at(6));
                self.command_build_sel(d, player, sel, order, tx as i16, ty as i16, t);
            }
            0x14 => {
                // 0x4c2370
                let (x, y, h, ty, q) = (u16_at(1), u16_at(3), u16_at(5), u16_at(7), byte(9));
                if x < self.map_px.0 && y < self.map_px.1 {
                    let t = self.unit_from_handle(h as u32).unwrap_or(NIL);
                    self.command_right_click(d, player, sel, (x as i16, y as i16), t, ty, q != 0);
                }
            }
            0x15 => {
                // 0x4c2320
                let (x, y, h, ty, order, q) = (u16_at(1), u16_at(3), u16_at(5), u16_at(7), byte(9), byte(10));
                if x < self.map_px.0 && y < self.map_px.1 {
                    let t = self.unit_from_handle(h as u32).unwrap_or(NIL);
                    self.command_targeted_order(d, player, sel, (x as i16, y as i16), t, ty, order, q != 0);
                }
            }
            0x09 | 0x0a | 0x0b => {
                // 0x4c2750 / 0x4c2560 / 0x4bfb40
                let n = byte(1) as usize;
                if p.len() < 2 + n * 2 {
                    return;
                }
                let hs: Vec<u16> = (0..n).map(|i| u16_at(2 + i * 2)).collect();
                match byte(0) {
                    0x09 => self.command_select(d, player, &hs),
                    0x0a => self.command_select_add(d, player, &hs),
                    _ => self.command_select_remove(d, player, &hs),
                }
            }
            0x13 => self.command_hotkey(d, player, byte(1), byte(2)),
            0x18 => {
                // 0x4c2ef0: cancel the construction of the one selected unit.
                if let [u] = self.live_selection(sel)[..] {
                    if self.units[u as usize].player == player {
                        self.cancel_unit(d, u);
                    }
                }
            }
            0x19 => {
                // 0x4c2ec0: cancel the morphs of the selected units.
                for u in self.live_selection(sel) {
                    if self.units[u as usize].player == player {
                        self.cancel_unit(d, u);
                    }
                }
            }
            0x20 => {
                // 0x4c0100: cancel training queue slot (0xfe = the last one).
                if let [u] = self.live_selection(sel)[..] {
                    let un = &self.units[u as usize];
                    let t = un.unit_type;
                    if !matches!(t, 0x23 | 0x2b | 0x26 | 0x83 | 0x84 | 0x8d | 0x8f) && un.status & 1 != 0 && un.player == player {
                        match u16_at(1) {
                            0xfe => self.cancel_last_queued(d, u),
                            0xff => {}
                            i => self.cancel_queued(d, u, i as usize),
                        }
                    }
                }
            }
            0x1a => self.command_stop(d, player, sel, byte(1) != 0),
            0x1e => self.command_return_cargo(d, player, sel, byte(1) != 0),
            0x1f => self.command_train_sel(d, player, sel, u16_at(1)),
            0x2b => self.command_hold(d, player, sel, byte(1) != 0),
            0x36 => self.command_stim(d, player, sel),
            0x57 => {
                // 0x4c2e90: only replays act on it (0x4c4d60 -> 0x48a1c0; the message is UI).
                if self.replay {
                    self.leave_game(d, player);
                }
            }
            0x21 => self.command_cloak(d, player, sel),
            0x22 => self.command_decloak(d, player, sel),
            0x23 => self.command_unit_morph(d, player, sel, u16_at(1)),
            0x35 => self.command_building_morph(d, player, sel, u16_at(1)),
            0x25 => self.command_unsiege(d, player, sel, byte(1) != 0),
            0x26 => self.command_siege(d, player, sel, byte(1) != 0),
            0x28 => self.command_unload_all(d, player, sel, byte(1) != 0),
            0x29 => self.command_unload(d, player, u16_at(1)),
            0x2f => self.command_lift(d, player, sel, u16_at(1) as i16, u16_at(3) as i16),
            0x30 => self.command_research(d, player, sel, byte(1)),
            0x31 => self.command_cancel_research(d, player, sel),
            0x32 => self.command_upgrade(d, player, sel, byte(1)),
            0x33 => self.command_cancel_upgrade(d, player, sel),
            _ => todo_port!("0x4865d0", "command receiver not ported yet"),
        }
    }
}
