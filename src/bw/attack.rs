//! Attacking: the AttackUnit order (0x479bd0), weapon ranges and firing.

use super::ai_town::{ai_index, ai_kind, AI_MILITARY};
use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Weapon id meaning "none".
pub const NO_WEAPON: u8 = 0x82;

/// 0x401470: carrier or Gantrithor.
pub(super) fn is_carrier(t: u16) -> bool {
    matches!(t, 0x48 | 0x52)
}

/// 0x401490: reaver or Warbringer.
pub(super) fn is_reaver_type(t: u16) -> bool {
    matches!(t, 0x53 | 0x51)
}

/// 0x402750: queen or Matriarch.
pub(super) fn is_queen(t: u16) -> bool {
    matches!(t, 0x2d | 0x31)
}

impl State {
    /// 0x401d40: `u` is a turret (subunit).
    pub(super) fn is_turret(&self, d: &Data, u: u32) -> bool {
        u != NIL && d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0
    }

    /// The unit that holds the weapons: the turret if `u` has one.
    pub(super) fn weapon_unit(&self, d: &Data, u: u32) -> u32 {
        let sub = self.units[u as usize].subunit;
        if self.is_turret(d, sub) { sub } else { u }
    }

    /// 0x475ad0: ground weapon (an unburrowed lurker has none).
    pub fn ground_weapon(&self, d: &Data, u: u32) -> u8 {
        let un = &self.units[u as usize];
        if un.unit_type == 0x67 && un.status & 0x10 == 0 {
            return NO_WEAPON;
        }
        d.dat.units.ground_weapon[un.unit_type as usize]
    }

    /// Weapon `u` would use against `target` (air weapon for air targets).
    pub(super) fn weapon_against(&self, d: &Data, u: u32, target: u32) -> u8 {
        let w = self.weapon_unit(d, u);
        if self.units[target as usize].status & 4 != 0 {
            d.dat.units.air_weapon[self.units[w as usize].unit_type as usize]
        } else {
            self.ground_weapon(d, w)
        }
    }

    /// 0x401d60: `target` is cloaked and not detected by `u`'s owner.
    pub(super) fn cloaked_from(&self, target: u32, u: u32) -> bool {
        let t = &self.units[target as usize];
        t.status & 0x300 != 0 && t.visibility & (1 << (self.units[u as usize].player & 0x1f)) == 0
    }

    /// 0x4e5e30: `target` is visible to `u`'s owner.
    pub(super) fn target_visible(&self, target: u32, u: u32) -> bool {
        let bit = 1u32 << (self.units[u as usize].player & 0x1f);
        let t = &self.units[target as usize];
        if t.status & 0x300 != 0 && t.visibility & bit == 0 {
            return false;
        }
        self.sprites[t.sprite as usize].visibility as u32 & bit != 0
    }

    /// 0x402210: `target` is a damaged (< 50% hp) completed unit of type 0x6a.
    pub(super) fn damaged_6a(&self, d: &Data, target: u32) -> bool {
        let t = &self.units[target as usize];
        if t.status & 1 == 0 || t.unit_type != 0x6a {
            return false;
        }
        let mut max = d.dat.units.hit_points[0x6a] as i32 >> 8;
        if max == 0 {
            max = (t.hp + 0xff) >> 8;
            if max == 0 {
                max = 1;
            }
        }
        ((t.hp + 0xff) >> 8) * 100 / max < 0x32
    }

    /// 0x476730 / the inline copy in 0x478540: `u` can attack `target` at all.
    pub(super) fn can_attack(&self, d: &Data, u: u32, target: u32, check_cloak: bool) -> bool {
        let un = &self.units[u as usize];
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 || target == NIL {
            return false;
        }
        let ts = self.units[target as usize].status;
        if ts & 0x400_0000 != 0 || self.sprites[self.units[target as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return false;
        }
        if check_cloak && self.cloaked_from(target, u) {
            return false;
        }
        let w = self.weapon_unit(d, u);
        let t = un.unit_type;
        if is_carrier(t) {
            return true;
        }
        if is_reaver_type(t) {
            // Reavers: ground targets in the same region group.
            return ts & 4 == 0 && self.units_same_group(u, target);
        }
        if is_queen(t) {
            return self.damaged_6a(d, target);
        }
        if t == 0x47 && un.ai != NIL {
            return false;
        }
        if ts & 4 != 0 {
            d.dat.units.air_weapon[self.units[w as usize].unit_type as usize] != NO_WEAPON
        } else {
            self.ground_weapon(d, w) != NO_WEAPON
        }
    }

    /// 0x475870: weapon range in pixels with range upgrades and the bunker bonus.
    pub(super) fn weapon_range(&self, d: &Data, u: u32, weapon: u8) -> u32 {
        let un = &self.units[u as usize];
        let base = d.dat.weapons.max_range[weapon as usize];
        let mut bonus = if un.status & 0x20 != 0 { 0x40 } else { 0 };
        let up = &self.players.upgrade_sc[un.player as usize];
        match un.unit_type {
            0 if up[0x10] != 0 => return base + bonus + 0x20,
            3 | 4 if weapon == 8 => {
                if self.players.upgrade_bw[un.player as usize][8] != 0 {
                    bonus += 0x60;
                }
            }
            0x11 | 0x12 if weapon == 10 => {
                if self.is_expansion {
                    bonus += 0x60;
                }
            }
            0x26 if up[0x1e] != 0 => return base + bonus + 0x20,
            0x42 if up[0x21] != 0 => return base + bonus + 0x40,
            0x4e if self.is_expansion => return base + bonus + 0x40,
            _ => {}
        }
        base + bonus
    }

    /// 0x4763d0: `target` is within `range` of `u`, plus `u`'s stopping distance when
    /// it moves and the target is not moving the same way.
    pub(super) fn in_attack_range(&self, d: &Data, u: u32, mut range: u32, target: u32) -> bool {
        let f = &self.units[u as usize].fl;
        if f.flags & 2 != 0 {
            let tf = &self.units[target as usize].fl;
            let mut diff = tf.velocity_dir_prev.wrapping_sub(f.velocity_dir_prev) as u32;
            if diff > 0x80 {
                diff = 0x100 - diff;
            }
            if tf.flags & 2 == 0 || diff >= 0x21 {
                range = range.wrapping_add((super::flingy::halt_distance(f, d) >> 8) as u32);
            }
        }
        self.box_distance_within(d, u, range, target)
    }

    /// 0x476430: `u` cannot hit `target` from here (not visible or out of range).
    pub(super) fn cannot_hit_now(&self, d: &Data, u: u32, target: u32) -> bool {
        if !self.target_visible(target, u) {
            return true;
        }
        let w = self.weapon_against(d, u, target);
        if w == NO_WEAPON {
            return false;
        }
        let r = self.weapon_range(d, u, w);
        !self.in_attack_range(d, u, r, target)
    }

    /// 0x475a10: `u` has a ground weapon that fires a projectile (not 0x2f).
    fn has_ranged_ground_weapon(&self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].unit_type as usize;
        if t == 0x2f {
            return false;
        }
        let w = d.dat.units.ground_weapon[t];
        w == NO_WEAPON || d.dat.weapons.flingy[w as usize] != 0
    }

    /// 0x4eb720: move toward unit `target` (move target unit +0x14).
    pub(super) fn move_to_unit(&mut self, d: &Data, u: u32, target: u32) -> bool {
        let un = &self.units[u as usize];
        if un.move_target_unit == target {
            let ts = &self.sprites[self.units[target as usize].sprite as usize];
            if un.fl.move_target == (ts.x, ts.y) || self.at_goal_unit_edge(d, u, target, un.fl.move_target) {
                self.units[u as usize].status &= !0x8_0000;
                return true;
            }
        }
        if self.units[u as usize].ai != NIL && self.units[u as usize].order != 0x60 {
            let tp = self.unit_sprite_pos(target);
            if self.ai_ride_instead_of_move(d, u, tp) {
                return false;
            }
        }
        let p = self.units[u as usize].path;
        if p != NIL {
            self.paths[p as usize].flags |= 1;
        }
        let ts = &self.sprites[self.units[target as usize].sprite as usize];
        let tp = (ts.x, ts.y);
        let un = &mut self.units[u as usize];
        if tp != un.fl.move_target || un.move_target_unit != target {
            un.fl.move_target = tp;
            un.move_target_unit = target;
            un.fl.flags = (un.fl.flags & 0xfb) | 1;
            un.fl.next_waypoint = tp;
        }
        un.recent_order_timer = 0xf;
        un.status &= !0x8_0000;
        let q = un.order_queue_head;
        if q != NIL && d.dat.orders.stop_moving[self.orders[q as usize].order as usize] == 0 {
            self.units[u as usize].fl.flags |= 0x20;
        } else {
            self.units[u as usize].fl.flags &= 0xdf;
        }
        true
    }

    /// 0x42da90: `p` lies on the edge of the box where mover `u` touches `goal`. As in
    /// 1.16.1 the right edge also adds the mover's top extent and the bottom edge
    /// leaves the mover out.
    fn at_goal_unit_edge(&self, d: &Data, u: u32, goal: u32, p: (i16, i16)) -> bool {
        let dims = |t: u16| {
            let v = d.dat.units.dimensions[t as usize];
            (v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16)
        };
        let gs = &self.sprites[self.units[goal as usize].sprite as usize];
        let (gl, gu, gr, gd) = dims(self.units[goal as usize].unit_type);
        let (ml, mu, mr, md) = dims(self.units[u as usize].unit_type);
        let left = gs.x.wrapping_sub(gl).wrapping_add((-1i16).wrapping_sub(mr));
        let top = gs.y.wrapping_sub(gu).wrapping_add((-1i16).wrapping_sub(md));
        let bottom = gd.wrapping_add(gs.y);
        let right = gr.wrapping_add(gs.x).wrapping_add(ml + 1).wrapping_add(mu + 1);
        let (x, y) = p;
        if x == left || x == right {
            (top..=bottom).contains(&y)
        } else {
            (y == top || y == bottom) && (left..=right).contains(&x)
        }
    }

    /// 0x4eb900: chase `target` unless recently ordered; give up when it is out of
    /// sight and far.
    pub(super) fn chase(&mut self, d: &Data, u: u32, target: u32) -> bool {
        if self.units[u as usize].recent_order_timer != 0 {
            return true;
        }
        let bit = 1u8 << (self.units[u as usize].player & 0x1f);
        if self.sprites[self.units[target as usize].sprite as usize].visibility & bit == 0 && !self.box_distance_within(d, u, 0x1c0, target) {
            if self.unit_at_move_target(u) != 1 {
                return true;
            }
            self.order_done_or_next(d, u);
            return false;
        }
        self.move_to_unit(d, u, target)
    }

    /// 0x477f10: approach `target`'s position (melee units turning to it), or chase it.
    fn approach(&mut self, d: &Data, u: u32, target: u32) {
        if !(self.target_visible(target, u) && self.units[target as usize].fl.flags & 2 != 0) {
            self.chase(d, u, target);
            return;
        }
        let tp = self.unit_sprite_pos(target);
        let h = super::flingy::halt_distance(&self.units[u as usize].fl, d) * 3;
        let (dx, dy) = crate::game::flingy::direction_table()[self.units[target as usize].fl.velocity_dir_prev as usize];
        let x = (dx.wrapping_mul(h) >> 16) + tp.0 as i32;
        let y = (dy.wrapping_mul(h) >> 16) + tp.1 as i32;
        let p = if (x as u32) < self.map_px.0 as u32 && (y as u32) < self.map_px.1 as u32 { (x as i16, y as i16) } else { tp };
        if self.units[u as usize].recent_order_timer == 0 {
            self.set_move_target_xy(d, u, p);
        }
    }

    /// 0x478540: AttackUnit core. True when in range and ready to fire.
    fn attack_unit_step(&mut self, d: &Data, u: u32) -> bool {
        let target = self.units[u as usize].order_target_unit;
        if !self.can_attack(d, u, target, true) {
            let un = &self.units[u as usize];
            if un.order_target_unit != NIL && un.status & 0x800_0000 == 0 {
                if un.ai == NIL {
                    let ts = &self.sprites[self.units[target as usize].sprite as usize];
                    let p = (ts.x, ts.y);
                    self.order_queue_front(d, u, 6, p);
                }
                self.order_done_or_next(d, u);
                return false;
            }
            self.unit_halt(d, u);
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.prepare_next_order(d, u);
                return false;
            }
            let idle = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
            self.order_now(d, u, idle);
            return false;
        }
        let ts = &self.sprites[self.units[target as usize].sprite as usize];
        let tp = (ts.x, ts.y);
        let un = &mut self.units[u as usize];
        un.order_target_pos = tp;
        if un.fl.next_target_waypoint != tp {
            un.fl.next_target_waypoint = tp;
        }
        if un.order_state == 0 {
            if !self.target_in_reach(d, u, target) && !self.move_to_unit(d, u, self.units[u as usize].order_target_unit) {
                return false;
            }
            self.units[u as usize].order_state = 1;
            return true;
        }
        if !self.cannot_hit_now(d, u, target) {
            if self.units[u as usize].unit_type == 0x2f {
                self.approach(d, u, target);
            } else {
                self.unit_halt(d, u);
            }
            return true;
        }
        if self.units[u as usize].ai != NIL {
            let ot = self.units[u as usize].order_target_unit;
            if self.ai_cant_reach_unit(d, u, ot) || self.unit_at_move_target(u) == 2 {
                if !self.ai_choose_target(d, u, true, false) {
                    self.unit_halt_facing(d, u);
                    self.order_done_or_next(d, u);
                }
                return false;
            }
        } else {
            let un = &self.units[u as usize];
            let st = un.status;
            if (st & 0x800_0000 != 0 && st & 0xc == 0 && self.units[target as usize].status & 4 != 0) || self.unit_at_move_target(u) == 2 {
                self.unit_halt_facing(d, u);
                self.order_done_or_next(d, u);
                return false;
            }
        }
        if self.has_ranged_ground_weapon(d, u) {
            self.chase(d, u, target);
        } else {
            self.approach(d, u, target);
        }
        false
    }

    /// Inline in 0x478540, 0x477980 and 0x47c4f0: `target` is seen and `u` can hit it
    /// from where it stands (outside the weapon's minimum range, inside its range).
    pub(super) fn target_in_reach(&self, d: &Data, u: u32, target: u32) -> bool {
        if !self.target_visible(target, u) {
            return false;
        }
        let w = self.weapon_against(d, u, target);
        if w >= NO_WEAPON {
            return false;
        }
        let min = d.dat.weapons.min_range[w as usize];
        if min != 0 && self.box_distance_within(d, self.weapon_unit(d, u), min, target) {
            return false;
        }
        let r = self.weapon_range(d, u, w);
        self.in_attack_range(d, u, r, target)
    }

    /// 0x442160: how much `u` wants to attack `t` (0 best .. 5 never).
    fn target_priority(&self, d: &Data, u: u32, t: u32) -> i32 {
        let tt = self.units[t as usize].unit_type;
        let mut c = t;
        if tt == 0x7d {
            // 0x4e6c90: the bunker's first live passenger stands in for it.
            if let Some(p) = self.first_passenger(d, t) {
                c = p;
            }
        } else if matches!(tt, 0x23 | 0x24 | 0x3b | 0x61) {
            return 5;
        }
        let cu = &self.units[c as usize];
        let mut p = if d.dat.units.flags[cu.unit_type as usize] & 8 != 0 {
            2
        } else if self.can_attack(d, c, u, true) {
            0
        } else if self.is_armed(d, c) {
            2
        } else if cu.status & 0x2_0000 != 0 {
            3
        } else {
            4
        };
        if cu.status & 1 == 0 || c != t {
            p += 1;
        }
        if p == 0 && cu.status & 0x8000 != 0 {
            return 1;
        }
        p
    }

    /// 0x4e6c90: the first live unit in `tr`'s passenger slots.
    pub(super) fn first_passenger(&self, d: &Data, tr: u32) -> Option<u32> {
        let n = d.dat.units.space_provided[self.units[tr as usize].unit_type as usize] as usize;
        (0..n.min(8)).find_map(|i| {
            let h = self.units[tr as usize].loaded_units[i] as u32;
            if h == 0 {
                return None;
            }
            let c = (h & 0x7ff) - 1;
            let cu = &self.units[c as usize];
            (cu.sprite != NIL && (cu.order != 0 || cu.order_state != 1) && cu.unique_id as u32 == h >> 11).then_some(c)
        })
    }

    /// 0x476000: target acquisition range in tiles, with range upgrades.
    pub(super) fn acquisition_range(&self, u: u32) -> u32 {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let p = un.player as usize;
        if super::unit::is_ghost(t) && un.status & 0x300 != 0 && un.order != 0x6b {
            return 0;
        }
        let base = self.acquisition[t as usize] as u32;
        let up = &self.players;
        match t {
            0 if up.upgrade_sc[p][0x10] != 0 => base + 1,
            3 | 4 if up.upgrade_bw[p][8] != 0 => base + 3,
            0x11 | 0x12 if self.is_expansion => base + 3,
            0x26 if up.upgrade_sc[p][0x1e] != 0 => base + 1,
            0x42 if up.upgrade_sc[p][0x21] != 0 => base + 2,
            0x4e if self.is_expansion => base + 2,
            _ => base,
        }
    }

    /// 0x440520: the smaller weapon minimum range of `u` (or its turret).
    fn min_attack_range(&self, d: &Data, u: u32) -> u32 {
        let un = &self.units[u as usize];
        let ut = &d.dat.units;
        let sub = un.subunit;
        let mut g = if un.unit_type == 0x67 && un.status & 0x10 == 0 { NO_WEAPON } else { ut.ground_weapon[un.unit_type as usize] };
        if g == NO_WEAPON && un.unit_type != 0x67 && sub != NIL {
            g = ut.ground_weapon[self.units[sub as usize].unit_type as usize];
        }
        let mut a = ut.air_weapon[un.unit_type as usize];
        if a == NO_WEAPON && sub != NIL {
            a = ut.air_weapon[self.units[sub as usize].unit_type as usize];
        }
        let min = |w: u8| d.dat.weapons.min_range[w as usize];
        match (g == NO_WEAPON, a == NO_WEAPON) {
            (true, true) => 0,
            (true, false) => min(a),
            (false, true) => min(g),
            (false, false) => min(a).min(min(g)),
        }
    }

    /// 0x4a1140: computer-owned ground `u` ignores a fleeing target it can't hit.
    fn ignores_target(&self, d: &Data, u: u32, t: u32) -> bool {
        let un = &self.units[u as usize];
        self.player_type(un.player) == 1 && un.status & 4 == 0 && !self.can_hit_now(d, u, t) && self.units[t as usize].status & 0x80 != 0
    }

    /// 0x443080: best enemy for `u` within its acquisition range (priority buckets,
    /// nearest within the best bucket).
    pub fn find_auto_target(&mut self, d: &Data, u: u32) -> u32 {
        let mut range = self.acquisition_range(u);
        let un = &self.units[u as usize];
        if un.status & 0x20 != 0 {
            range += 2;
        } else if un.ai != NIL {
            range = range.max(d.dat.units.sight_range[un.unit_type as usize] as u32);
        }
        let range = range * 32;
        let min = self.min_attack_range(d, u);
        let buckets = self.target_buckets(d, u, range, min);
        let Some(list) = buckets.iter().find(|b| !b.is_empty()) else { return NIL };
        self.pick_target(d, u, list)
    }

    /// 0x4405e0: the target `u` takes from a bucket `list`: for an AI scourge the one with
    /// the most hp + shields (0x4026d0, first on ties), else the nearest (first on ties).
    fn pick_target(&self, d: &Data, u: u32, list: &[u32]) -> u32 {
        if list.len() == 1 {
            return list[0];
        }
        let mut best = 0;
        if self.units[u as usize].ai != NIL && self.units[u as usize].unit_type == 0x2f {
            let mut life = self.life_now(d, list[0]) as u32;
            for (i, &c) in list.iter().enumerate().skip(1) {
                let l = self.life_now(d, c) as u32;
                if l > life {
                    life = l;
                    best = i;
                }
            }
            return list[best];
        }
        let up = self.unit_sprite_pos(u);
        let dist = |st: &State, c: u32| {
            let p = st.unit_sprite_pos(c);
            super::path::approx_distance(up.0 as i32, p.0 as i32, up.1 as i32, p.1 as i32)
        };
        let mut bd = dist(self, list[0]);
        for (i, &c) in list.iter().enumerate().skip(1) {
            let dd = dist(self, c);
            if dd < bd {
                bd = dd;
                best = i;
            }
        }
        list[best]
    }

    /// 0x442fc0: an enemy `u` can hit without moving: acquisition range only (+0x40 in
    /// a bunker), a random one of the best priority bucket (0x4401b0).
    pub fn find_target_in_range(&mut self, d: &Data, u: u32) -> u32 {
        let mut range = self.acquisition_range(u) as u8 as u32 * 32;
        if self.units[u as usize].status & 0x20 != 0 {
            range += 0x40;
        }
        let buckets = self.target_buckets(d, u, range, 0);
        let Some(list) = buckets.iter().find(|b| !b.is_empty()) else { return NIL };
        if list.len() == 1 || !self.rng.enabled {
            return list[0];
        }
        list[self.rng.rand(0x13) as usize % list.len()]
    }

    /// 0x4e8280 over the finder box `range` + 0x40 around `u` with filter 0x442da0
    /// (`min` = 0x6955dc, `range` = 0x6957a4): candidates by target priority (0x440160).
    fn target_buckets(&mut self, d: &Data, u: u32, range: u32, min: u32) -> [Vec<u32>; 6] {
        let (x, y) = self.unit_sprite_pos(u);
        let e = range as i16 + 0x40;
        let b = [x.wrapping_sub(e), y.wrapping_sub(e), x.wrapping_add(e), y.wrapping_add(e)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let mut buckets: [Vec<u32>; 6] = Default::default();
        let player = self.units[u as usize].player;
        for c in found {
            // 0x442da0
            if c == u || self.sprites[self.units[c as usize].sprite as usize].visibility & (1 << (player & 7)) == 0 {
                continue;
            }
            if !self.can_attack(d, u, c, true) || !self.is_enemy(u, c) {
                continue;
            }
            if (min != 0 && self.box_distance_within(d, u, min, c)) || !self.box_distance_within(d, u, range, c) {
                continue;
            }
            let sub = self.units[u as usize].subunit;
            let a = if self.is_turret(d, sub) { sub } else { u };
            if self.units[a as usize].status & 0x1_0000 == 0 {
                let w = d.dat.units.ground_weapon[self.units[a as usize].unit_type as usize];
                let p = self.unit_sprite_pos(c);
                if !self.facing(d, a, p, w) {
                    continue;
                }
            }
            if self.ignores_target(d, u, c) {
                continue;
            }
            let pr = self.target_priority(d, u, c) as usize;
            // 0x440160: 16 per bucket.
            if pr < 6 && buckets[pr].len() < 0x10 {
                buckets[pr].push(c);
            }
        }
        buckets
    }

    /// 0x4e5db0: `u`'s owner sees its order target (or, without one, the order
    /// position's tile).
    pub(super) fn order_target_visible(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        let bit = 1u32 << (un.player & 0x1f);
        let t = un.order_target_unit;
        if t == NIL {
            let (x, y) = un.order_target_pos;
            let i = (y as i32 / 32) * self.terrain.width as i32 + x as i32 / 32;
            let f = self.terrain.tile_flags.get(i as usize).copied().unwrap_or(0);
            return !f & 0xff & bit != 0;
        }
        let tu = &self.units[t as usize];
        if tu.status & 0x300 != 0 && tu.visibility & bit == 0 {
            return false;
        }
        self.sprites[tu.sprite as usize].visibility as u32 & bit != 0
    }

    /// 0x478b40: attack the order target from where `u` stands (hold position). True
    /// while there is something to do here (cooling down, turning, fired); false when
    /// there is no hittable target in range.
    pub(super) fn attack_in_place(&mut self, d: &Data, u: u32) -> bool {
        if self.units[u as usize].order_target_unit == NIL {
            return false;
        }
        self.pick_better_target(d, u);
        if !self.order_target_visible(u) {
            return false;
        }
        let t = self.units[u as usize].order_target_unit;
        if !self.can_attack(d, u, t, true) {
            return false;
        }
        let tp = self.unit_sprite_pos(t);
        self.units[u as usize].order_target_pos = tp;
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            return true;
        }
        let (w, slot) = if self.units[t as usize].status & 4 != 0 {
            (d.dat.units.air_weapon[self.units[u as usize].unit_type as usize], 2)
        } else {
            (self.ground_weapon(d, u), 1)
        };
        let un = &mut self.units[u as usize];
        let cd = if slot == 1 { un.ground_cooldown } else { un.air_cooldown };
        if cd != 0 {
            if un.order_process_timer as i32 > cd as i32 - 1 {
                un.order_process_timer = cd - 1;
            }
            return true;
        }
        if un.fl.flags & 8 != 0 {
            return true;
        }
        let dist = self.unit_box_gap(d, t, u);
        let min = d.dat.weapons.min_range[w as usize];
        if (min != 0 && dist < min) || dist > self.weapon_range(d, u, w) {
            return false;
        }
        let sp = self.unit_sprite_pos(u);
        let tp = self.units[u as usize].order_target_pos;
        let dir = crate::game::flingy::direction_to(sp.0 as i32, sp.1 as i32, tp.0 as i32, tp.1 as i32);
        let mut diff = self.units[u as usize].fl.heading as i32 - dir as i32;
        if diff < 0 {
            diff += 0x100;
        }
        if diff > 0x80 {
            diff = 0x100 - diff;
        }
        if diff > d.dat.weapons.attack_angle[w as usize] as i32 {
            if self.units[u as usize].status & 0x1_0000 != 0 {
                self.units[u as usize].order_process_timer = 0;
                return true;
            }
            return false;
        }
        let st = self.units[u as usize].status;
        if st & 0x8000 != 0 {
            return false;
        }
        if st & 0x20 != 0 {
            self.bunker_muzzle_flash(d, u);
        }
        self.units[u as usize].fl.flags |= 8;
        self.bullets.spin_source = NIL;
        let base = self.weapon_cooldown(d, u, w);
        let r = (self.rng.rand(0xd) & 3) as u8;
        let cd = base.wrapping_add(r.wrapping_sub(1));
        let un = &mut self.units[u as usize];
        un.ground_cooldown = cd;
        un.air_cooldown = cd;
        let s = un.sprite;
        self.sprite_images_run_anim(d, s, slot + 4);
        true
    }

    /// 0x477510: an auto-targeting unit (status 0x8000000) switches to a better target.
    pub(super) fn pick_better_target(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x800_0000 == 0 {
            return;
        }
        let mut best = self.units[u as usize].order_target_unit;
        let mut prio;
        if best == NIL {
            prio = i32::MAX;
        } else {
            if self.units[u as usize].ai != NIL && !self.is_enemy(u, best) {
                return;
            }
            if !self.can_attack(d, u, best, true) {
                best = NIL;
                prio = i32::MAX;
            } else if self.ignores_target(d, u, best) {
                best = NIL;
                prio = i32::MAX;
            } else {
                prio = self.target_priority(d, u, best);
                if prio == 0 {
                    if self.can_hit_now(d, u, best) {
                        return;
                    }
                    prio = 1;
                }
            }
        }
        let at = self.units[u as usize].auto_target;
        if at != NIL {
            if !self.can_attack(d, u, at, true) || !self.is_enemy(u, at) || self.ignores_target(d, u, at) {
                self.units[u as usize].auto_target = NIL;
            } else {
                let mut p = self.target_priority(d, u, at);
                if p == 0 && !self.can_hit_now(d, u, at) {
                    p = 1;
                }
                if p < prio {
                    best = at;
                    prio = p;
                }
            }
        }
        if best == NIL || prio != 0 {
            if self.units[u as usize].ai != NIL && self.ai_choose_target(d, u, true, false) {
                return;
            }
            let c = self.find_auto_target(d, u);
            if c != NIL && self.target_priority(d, u, c) < prio {
                best = c;
            }
        }
        if best != NIL && !self.can_attack(d, u, best, true) {
            return;
        }
        self.units[u as usize].order_target_unit = best;
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            let so = self.units[sub as usize].order;
            if so == d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize] || so == 0x6b {
                self.units[sub as usize].order_target_unit = best;
            }
        }
    }

    /// 0x479bd0: AttackUnit.
    pub fn order_attack_unit(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if self.player_type(un.player) == 1 {
            let t = un.order_target_unit;
            if (t == NIL || d.dat.units.flags[self.units[t as usize].unit_type as usize] & 1 == 0) && self.ai_guard_return(d, u, 0) {
                return;
            }
        }
        self.pick_better_target(d, u);
        if !self.attack_unit_step(d, u) {
            return;
        }
        if self.units[u as usize].ai != NIL {
            self.ai_stim_check(d, u);
        }
        self.fire(d, u, 5);
    }

    /// 0x43ffd0: a computer marine or firebat (not stimmed, or net speed bonus 0 or less)
    /// at least half healthy whose target could hit it, is in reach of it and has 100 or
    /// more hp and shields uses Stim Packs.
    fn ai_stim_check(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let t = un.order_target_unit;
        if !matches!(un.unit_type, 0 | 0x20) || t == NIL {
            return;
        }
        let speed = (un.stim_timer != 0) as i32 + (un.status & 0x1000_0000 != 0) as i32 - (un.ensnare_timer != 0) as i32;
        if speed >= 1 {
            return;
        }
        if ((un.hp + 0xff) >> 8) < self.max_hp_points(d, u) / 2 {
            return;
        }
        // 0x476730: the target can attack this unit.
        if !self.can_attack(d, t, u, true) || !self.target_visible(t, u) {
            return;
        }
        let w = self.weapon_against(d, u, t);
        if w >= NO_WEAPON {
            return;
        }
        let wu = self.weapon_unit(d, u);
        let min = d.dat.weapons.min_range[w as usize];
        if min != 0 && self.box_distance_within(d, wu, min, t) {
            return;
        }
        let r = self.weapon_range(d, u, w);
        if !self.in_attack_range(d, wu, r, t) {
            return;
        }
        let player = self.units[u as usize].player;
        if self.can_use_tech(d, u, 0, player) == Ok(1) && self.life_now(d, t) >= 100 {
            self.ai_stim(d, u);
        }
    }

    /// 0x4554a0: Stim Packs (as the command 0x4c2f30 does): above 10 hp, the stim
    /// sound, 10 hp lost, stimmed for 37 frames.
    fn ai_stim(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].hp <= 0xa00 {
            return;
        }
        let id = self.rng.rand_between(0x1f, 0x116, 0x117);
        self.unit_sound(id as u16, u, 0);
        self.apply_damage_pub(d, u, 0xa00, NIL, 0xff, true);
        if self.units[u as usize].stim_timer < 0x25 {
            self.units[u as usize].stim_timer = 0x25;
            self.update_speed(d, u);
        }
    }

    /// 0x476ed0: fire at the order target with the right weapon, starting `anim`
    /// (`anim + 1` for air targets).
    pub fn fire(&mut self, d: &Data, u: u32, anim: u8) {
        self.bullets.spin_source = NIL;
        let st = self.units[u as usize].status;
        if st & 0x8000 != 0 {
            return;
        }
        let target = self.units[u as usize].order_target_unit;
        let t = self.units[u as usize].unit_type;
        if target != NIL && self.units[target as usize].status & 4 != 0 {
            let w = d.dat.units.air_weapon[t as usize];
            self.fire_weapon(d, u, w, 2, anim + 1);
        } else if t == 0x67 && st & 0x10 == 0 {
            self.fire_weapon(d, u, NO_WEAPON, 1, anim);
        } else {
            let w = d.dat.units.ground_weapon[t as usize];
            self.fire_weapon(d, u, w, 1, anim);
        }
    }

    /// 0x476c90: start the attack animation when the cooldown of `slot` (1 ground,
    /// 2 air) is over and the target can be hit.
    fn fire_weapon(&mut self, d: &Data, u: u32, w: u8, slot: u8, anim: u8) -> bool {
        if w == NO_WEAPON {
            return false;
        }
        let cd = if slot == 1 { self.units[u as usize].ground_cooldown } else { self.units[u as usize].air_cooldown };
        if cd != 0 {
            let un = &mut self.units[u as usize];
            if (cd as i32 - 1) < un.order_process_timer as i32 {
                un.order_process_timer = cd - 1;
            }
            return false;
        }
        if !self.weapon_ready(d, u, w) {
            return false;
        }
        self.units[u as usize].fl.flags |= 8;
        let base = self.weapon_cooldown(d, u, w);
        let r = (self.rng.rand(0xc) & 3) as u8;
        let cd = base.wrapping_add(r.wrapping_sub(1));
        let un = &mut self.units[u as usize];
        un.ground_cooldown = cd;
        un.air_cooldown = cd;
        let s = un.sprite;
        self.sprite_images_run_anim(d, s, anim);
        true
    }

    /// 0x474a20 (0x474b40 and 0x4749d0 with p = (0, 0)): queue `order` at `p` in front of the queue.
    pub(super) fn order_queue_front(&mut self, d: &Data, u: u32, order: u8, p: (i16, i16)) {
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            self.order_insert_before(d, u, order, 0xe4, p, NIL, head);
        } else {
            self.order_queue_append(d, u, order, 0xe4, p, NIL);
        }
    }

    /// 0x475dc0: weapon cooldown with acid spores, stim/adrenal and ensnare.
    pub(super) fn weapon_cooldown(&self, d: &Data, u: u32, w: u8) -> u8 {
        let un = &self.units[u as usize];
        let base = d.dat.weapons.cooldown[w as usize] as u32;
        let mut c = base;
        if un.acid_spore_count != 0 {
            c += (base >> 3).max(3) * un.acid_spore_count as u32;
        }
        let m = (un.stim_timer != 0) as i32 + (un.status & 0x2000_0000 != 0) as i32 - (un.ensnare_timer != 0) as i32;
        if m < 0 {
            c += c >> 2;
        } else if m > 0 {
            c >>= 1;
        }
        c.clamp(5, 0xfa) as u8
    }

    /// 0x476640: the order target (or position) is within weapon `w`'s range and arc.
    pub(super) fn weapon_ready(&mut self, d: &Data, u: u32, w: u8) -> bool {
        if self.units[u as usize].fl.flags & 8 != 0 {
            return false;
        }
        let target = self.units[u as usize].order_target_unit;
        let dist = if target != NIL { self.unit_box_gap(d, target, u) } else { self.point_gap(d, u, self.units[u as usize].order_target_pos) };
        let min = d.dat.weapons.min_range[w as usize];
        if (min != 0 && dist < min) || dist > self.weapon_range(d, u, w) || self.units[u as usize].pathing_flags & 2 != 0 {
            return false;
        }
        let un = &self.units[u as usize];
        if un.fl.next_target_waypoint != un.fl.pos && !self.facing(d, u, un.fl.next_target_waypoint, w) {
            self.units[u as usize].order_process_timer = 0;
            return false;
        }
        true
    }

    /// 0x477fd0: a unit firing from a bunker shows a muzzle flash (0x179, firebats and
    /// Gui Montag 0x17a) at the bunker's attack overlay point for its facing.
    fn bunker_muzzle_flash(&mut self, d: &Data, u: u32) {
        let b = self.units[u as usize].connected_unit;
        let main = self.sprites[self.units[b as usize].sprite as usize].main_image;
        let im = &self.images[main as usize];
        let h = self.units[u as usize].fl.heading as i32;
        let idx = ((h + 0x10) >> 5) & 7;
        let (mut ox, oy) = d.lo_offset(0, im.image_id, im.frame, idx as i8).unwrap_or((0, 0));
        if im.flags & super::sprite::iflag::FLIPPED != 0 {
            ox = ox.wrapping_neg();
        }
        let (sprite_id, dir) = if matches!(self.units[u as usize].unit_type, 0x20 | 0xa) {
            (0x17a, ((h + 8) >> 4) & 0xf)
        } else {
            (0x179, idx * 2)
        };
        let (x, y) = self.unit_sprite_pos(u);
        let Some(th) = self.create_thingy(d, sprite_id, x.wrapping_add(ox as i16), y.wrapping_add(oy as i16), 0) else {
            return;
        };
        let ts = self.thingies[th as usize].sprite;
        let us = self.units[u as usize].sprite;
        self.sprites[ts as usize].elevation = self.sprites[us as usize].elevation.wrapping_add(1);
        self.sprite_images_set_direction(d, ts, (dir << 4) as u8);
        self.sprite_fog_visibility(ts);
    }

    /// 0x475be0: `u` faces (x, y) within weapon `w`'s attack angle (lurkers just turn).
    fn facing(&mut self, d: &Data, u: u32, p: (i16, i16), w: u8) -> bool {
        let s = self.unit_sprite_pos(u);
        let dir = crate::game::flingy::direction_to(s.0 as i32, s.1 as i32, p.0 as i32, p.1 as i32);
        if self.units[u as usize].unit_type == 0x67 {
            self.units[u as usize].fl.heading = dir;
            return true;
        }
        let mut diff = self.units[u as usize].fl.heading as i32 - dir as i32;
        if diff < 0 {
            diff += 0x100;
        }
        if diff > 0x80 {
            diff = 0x100 - diff;
        }
        diff <= d.dat.weapons.attack_angle[w as usize] as i32
    }

    /// Box of `u` (its parent's box for a turret).
    fn body_box(&self, d: &Data, u: u32) -> [i16; 4] {
        let b = if self.is_turret(d, u) { self.units[u as usize].subunit } else { u };
        self.unit_box(d, b)
    }

    /// 0x430d30: approximate gap between the boxes of `a` and `b` (`b`'s parent for a
    /// turret).
    fn unit_box_gap(&self, d: &Data, a: u32, b: u32) -> u32 {
        let ba = self.unit_box(d, a);
        let bb = self.body_box(d, b);
        let gap = |lo: i16, hi1: i16, lo2: i16, hi2: i16| {
            let g = lo as i32 - hi1.wrapping_add(1) as i32;
            if g >= 0 {
                return g;
            }
            let g = lo2 as i32 - hi2.wrapping_add(1) as i32;
            if g >= 0 { g } else { 0 }
        };
        let dx = gap(bb[0], ba[2], ba[0], bb[2]);
        let dy = gap(bb[1], ba[3], ba[1], bb[3]);
        super::path::approx_distance(0, dx, 0, dy)
    }

    /// 0x430e50: approximate gap between `u`'s box and a point.
    fn point_gap(&self, d: &Data, u: u32, p: (i16, i16)) -> u32 {
        let b = self.body_box(d, u);
        let gap = |lo: i16, hi: i16, v: i16| {
            let g = lo as i32 - v as i32;
            if g >= 0 {
                return g;
            }
            let g = v as i32 - hi.wrapping_add(1) as i32;
            if g >= 0 { g } else { 0 }
        };
        super::path::approx_distance(0, gap(b[0], b[2], p.0), 0, gap(b[1], b[3], p.1))
    }

    /// 0x479d60: fire the ground (1) or air (2) weapon at the order target.
    pub fn attack_with(&mut self, d: &Data, u: u32, slot: u8) {
        if self.units[u as usize].order_target_unit == NIL {
            return;
        }
        let w = if slot == 1 { self.ground_weapon(d, u) } else { d.dat.units.air_weapon[self.units[u as usize].unit_type as usize] };
        self.fire_projectile(d, u, w);
        if d.dat.weapons.damage_factor[w as usize] == 2 {
            self.fire_projectile(d, u, w);
        }
    }

    /// 0x479c90: create weapon `w`'s bullet at its launch point.
    pub fn fire_projectile(&mut self, d: &Data, u: u32, w: u8) {
        let wi = w as usize;
        let un = &self.units[u as usize];
        let (x, y) = match d.dat.weapons.behavior[wi] {
            2 => {
                if un.order_target_unit == NIL {
                    return;
                }
                self.impact_point(d, u)
            }
            4 => un.order_target_pos,
            _ => {
                let (tx, ty) = crate::game::flingy::direction_table()[un.fl.heading as usize];
                let fwd = self.forward_offset(d, w) as i32;
                let p = self.unit_sprite_pos(u);
                (
                    (((tx * fwd) >> 8) + p.0 as i32) as i16,
                    ((((ty * fwd) >> 8) - d.dat.weapons.upward_offset[wi] as i32) + p.1 as i32) as i16,
                )
            }
        };
        if d.dat.weapons.flingy[wi] != 0 {
            let (player, dir) = (self.units[u as usize].player, self.units[u as usize].fl.heading);
            self.create_bullet(d, u, w, x, y, player, dir);
        }
    }

    /// 0x4762c0: where a shot from `u` meets the inner half of its target's box.
    fn impact_point(&self, d: &Data, u: u32) -> (i16, i16) {
        let t = self.units[u as usize].order_target_unit;
        let [mut l, mut tp, mut r, mut b] = self.unit_box(d, t);
        let dx = ((r as i32 - l as i32) >> 2) as i16;
        l = l.wrapping_add(dx);
        r = r.wrapping_sub(dx);
        let dy = ((b as i32 - tp as i32) >> 2) as i16;
        tp = tp.wrapping_add(dy);
        b = b.wrapping_sub(dy);
        let c = (((r as i32 - l as i32) / 2) as i16).wrapping_add(l);
        let cy = (((b as i32 - tp as i32) / 2) as i16).wrapping_add(tp);
        let me = self.unit_sprite_pos(u);
        match super::collide::clip_segment((c, cy), me, [l, tp, r, b]) {
            Some((_, p)) => p,
            None => (c, cy),
        }
    }

    /// 0x657910: weapons.dat forward offset, which `attkshiftproj` overwrites.
    fn forward_offset(&self, d: &Data, w: u8) -> u8 {
        self.forward_offsets.get(w as usize).copied().unwrap_or(d.dat.weapons.forward_offset[w as usize])
    }

    pub fn set_forward_offset(&mut self, d: &Data, w: u8, v: u8) {
        if self.forward_offsets.is_empty() {
            self.forward_offsets = d.dat.weapons.forward_offset.to_vec();
        }
        if let Some(o) = self.forward_offsets.get_mut(w as usize) {
            *o = v;
        }
    }

    /// 0x4753e0: nobrkcodeend: orders may interrupt again; start a queued one.
    pub fn nobrk_end(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.status &= !super::unit::status::NO_BRK_CODE_START;
        let s = un.sprite;
        self.sprites[s as usize].flags &= 0x7f;
        let un = &self.units[u as usize];
        if un.order_queue_head != NIL && un.order_flags & 1 != 0 {
            self.order_start(d, u);
            self.prepare_next_order(d, u);
        }
    }

    /// Owner used for alliance checks (neutral units use their sprite's owner).
    fn alliance_owner(&self, u: u32) -> usize {
        let un = &self.units[u as usize];
        if un.player == 0xb { self.sprites[un.sprite as usize].owner as usize } else { un.player as usize }
    }

    pub fn alliance_owner_pub(&self, u: u32) -> usize {
        self.alliance_owner(u)
    }

    /// 0x47b910: `t` is an enemy of `u`.
    pub(super) fn is_enemy(&self, u: u32, t: u32) -> bool {
        self.alliances[self.units[u as usize].player as usize][self.alliance_owner(t)] == 0
    }

    /// 0x476870: `u` can hit `target` (default: its order target) from where it is.
    pub(super) fn can_hit_now(&self, d: &Data, u: u32, target: u32) -> bool {
        let target = if target != NIL { target } else { self.units[u as usize].order_target_unit };
        if target == NIL {
            return true;
        }
        if !self.target_visible(target, u) {
            return false;
        }
        let w = self.weapon_against(d, u, target);
        if w >= NO_WEAPON {
            return false;
        }
        let min = d.dat.weapons.min_range[w as usize];
        if min != 0 && self.box_distance_within(d, u, min, target) {
            return false;
        }
        let r = self.weapon_range(d, u, w);
        self.in_attack_range(d, u, r, target)
    }

    /// 0x4769d0: `u`, now targeting `current`, should switch to `attacker`.
    fn should_retarget(&self, d: &Data, u: u32, current: u32, attacker: u32) -> bool {
        if attacker == current {
            return false;
        }
        if self.alliances[self.units[u as usize].player as usize][self.alliance_owner(attacker)] != 0 {
            return false;
        }
        if current == NIL || !self.can_attack(d, u, current, true) || !self.is_enemy(u, current) {
            return true;
        }
        self.can_hit_now(d, u, attacker) && !self.can_hit_now(d, u, current)
    }

    pub(super) fn can_attack_pub(&self, d: &Data, u: u32, target: u32) -> bool {
        self.can_attack(d, u, target, true)
    }

    pub(super) fn is_armed_pub(&self, d: &Data, u: u32) -> bool {
        self.is_armed(d, u)
    }

    /// 0x476180: `u` has a weapon (including interceptors / scarabs and turrets).
    fn is_armed(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let ti = t as usize;
        let no_ground = (t == 0x67 && un.status & 0x10 == 0) || d.dat.units.ground_weapon[ti] == NO_WEAPON;
        if !(no_ground && d.dat.units.air_weapon[ti] == NO_WEAPON) {
            return true;
        }
        if matches!(t, 0x48 | 0x52) && un.u8_at(0xc9).wrapping_add(un.u8_at(0xc8)) != 0 {
            return true;
        }
        if matches!(t, 0x53 | 0x51) && un.u8_at(0xc8) != 0 {
            return true;
        }
        let sub = un.subunit;
        if sub == NIL {
            return false;
        }
        self.ground_weapon(d, sub) != NO_WEAPON || d.dat.units.air_weapon[self.units[sub as usize].unit_type as usize] != NO_WEAPON
    }

    /// Set the auto target (+0x7c) of `u` and its turret.
    fn set_auto_target(&mut self, d: &Data, u: u32, t: u32) {
        self.units[u as usize].auto_target = t;
        let sub = self.units[u as usize].subunit;
        if self.is_turret(d, sub) {
            let lap = self.units[u as usize].last_attacking_player;
            self.units[sub as usize].last_attacking_player = lap;
            self.units[sub as usize].auto_target = t;
        }
    }

    /// 0x4795d0: `u` was hit by `attacker`: remember it, reveal it, fight back, and
    /// alert nearby units of the same owner.
    pub fn attacked_by(&mut self, d: &Data, u: u32, mut attacker: u32, reveal: bool) {
        if attacker == NIL || self.units[attacker as usize].player == self.units[u as usize].player {
            return;
        }
        self.units[u as usize].last_attacking_player = self.units[attacker as usize].player;
        if self.units[u as usize].ai != NIL && self.units[u as usize].auto_target == NIL {
            self.set_auto_target(d, u, attacker);
        }
        if self.player_type(self.units[u as usize].player) == 1 {
            self.ai_react_to_target(d, u, attacker, true, false);
        }
        let p = self.units[u as usize].player;
        if reveal && !self.cloaked_from(attacker, u) && p < 8 {
            let a = &self.units[attacker as usize];
            let s = &self.sprites[a.sprite as usize];
            let (x, y, air) = (s.x as i32, s.y as i32, a.status & 4 != 0);
            self.reveal(1 << p, 1, x, y, air);
        }
        if self.units[attacker as usize].status & 0x20 != 0 {
            attacker = self.units[attacker as usize].connected_unit;
        }
        let current = self.units[u as usize].auto_target;
        if self.should_retarget(d, u, current, attacker) {
            self.fight_back(d, u, attacker);
        }
        if self.units[u as usize].ai == NIL {
            let (x, y) = self.unit_sprite_pos(u);
            let b = [(x as u16).wrapping_sub(0x60) as i16, y.wrapping_sub(0x60), (x as u16).wrapping_add(0x60) as i16, y.wrapping_add(0x60)];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, b);
            let near = search.results(start);
            search.pop();
            self.search = search;
            for o in near {
                if o != u && self.units[o as usize].player == p {
                    self.help_ally(d, o, attacker);
                }
            }
        }
    }

    /// 0x4776c0: answer an attack: target the attacker, or run if `u` can't fight it.
    fn fight_back(&mut self, d: &Data, u: u32, attacker: u32) {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        if t == 0x23 {
            return;
        }
        if un.status & 0x10 != 0 && un.irradiate_timer == 0 && t != 0x67 {
            if d.dat.units.flags[t as usize] & 0x10_0000 != 0 {
                self.unburrow(d, u);
            }
            return;
        }
        let flags = d.dat.units.flags[t as usize];
        if flags & 1 == 0 && (!self.can_attack(d, u, attacker, true) || flags & 8 != 0) {
            // Run from it, if the current order lets the unit flee (orders.dat field 11).
            if t == 0x67 && un.status & 0x10 != 0 && (un.ai == NIL || un.irradiate_timer != 0) {
                return;
            }
            let mut m = u;
            if flags & 0x10 != 0 {
                m = un.subunit;
            }
            let mu = &self.units[m as usize];
            if d.dat.orders.unknown11[mu.order as usize] == 0 || mu.status & 0x2_0000 == 0 {
                return;
            }
            if !self.ai_may_flee(m) {
                return;
            }
            if self.cover_chance_pub(d, m) == 0xff {
                return;
            }
            let p = self.flee_point(d, attacker, m);
            self.order_target(d, m, 6, p, NIL);
            return;
        }
        self.set_auto_target(d, u, attacker);
    }

    /// 0x476a50: a point 192, 128, 64, 32 or 16 pixels from `u`, away from `from`,
    /// that a ground unit can reach (its own position if none).
    pub(super) fn flee_point(&mut self, d: &Data, from: u32, u: u32) -> (i16, i16) {
        let fp = self.unit_sprite_pos(from);
        let up = self.unit_sprite_pos(u);
        let dir = crate::game::flingy::direction_to(fp.0 as i32, fp.1 as i32, up.0 as i32, up.1 as i32);
        let (vx, vy) = crate::game::flingy::direction_table()[dir as usize];
        for dist in [192, 128, 64, 32, 16] {
            let x = ((vx * dist) >> 8) + up.0 as i32;
            let y = ((vy * dist) >> 8) + up.1 as i32;
            let x = x.clamp(0, self.map_px.0 as i32 - 1);
            let y = y.clamp(0, self.map_px.1 as i32 - 1);
            let p = (x as i16, y as i16);
            if self.units[u as usize].pathing_flags & 1 == 0 {
                return p;
            }
            let r1 = super::path::region_at(&self.sai, x, y);
            let r2 = super::path::region_at(&self.sai, up.0 as i32, up.1 as i32);
            let (g1, g2) = (self.sai.regions[r1 as usize].group, self.sai.regions[r2 as usize].group);
            if g1 != g2 {
                continue;
            }
            let player = self.units[u as usize].player;
            if self.ai_region_reachable(d, player, r2, r1) {
                return p;
            }
        }
        up
    }

    /// 0x476e60: nearby armed unit of the same owner joins in.
    fn help_ally(&mut self, d: &Data, o: u32, attacker: u32) {
        let un = &self.units[o as usize];
        let t = un.unit_type;
        if t == 0x47 || t == 0x56 || d.dat.units.flags[t as usize] & 8 != 0 || un.status & 0x10 != 0 {
            return;
        }
        if !self.is_armed(d, o) {
            return;
        }
        let current = self.units[o as usize].auto_target;
        if !self.should_retarget(d, o, current, attacker) {
            return;
        }
        self.set_auto_target(d, o, attacker);
    }

    /// 0x4770e0: attack the auto target if it is still a valid enemy.
    pub fn attack_auto_target(&mut self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].auto_target;
        if t == NIL || self.units[u as usize].ai != NIL {
            return false;
        }
        if self.is_enemy(u, t) && self.can_attack(d, u, t, true) {
            self.order_attack(d, u, t, true);
            self.units[u as usize].status |= 8;
            return true;
        }
        self.units[u as usize].auto_target = NIL;
        false
    }

    /// 0x476fc0: order `u` to attack `t` with its attack order.
    pub(super) fn order_attack(&mut self, d: &Data, u: u32, mut t: u32, mark: bool) {
        if self.units[t as usize].unit_type == 0x49 && self.units[t as usize].ptr_at(0xc0) != NIL {
            t = self.units[t as usize].ptr_at(0xc0);
        }
        let order = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
        self.order_with_target(d, u, order, t);
        if mark {
            self.units[u as usize].status |= 0x800_0000;
            let sub = self.units[u as usize].subunit;
            if sub != NIL {
                self.units[sub as usize].status |= 0x800_0000;
            }
        }
        let sub = self.units[u as usize].subunit;
        if self.is_turret(d, sub) {
            let order = d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize];
            self.order_with_target(d, sub, order, t);
            if mark {
                self.units[sub as usize].status |= 0x800_0000;
                let ss = self.units[sub as usize].subunit;
                if ss != NIL {
                    self.units[ss as usize].status |= 0x800_0000;
                }
            }
        }
        if self.units[u as usize].ai != NIL && self.units[u as usize].unit_type == 0x1e && self.units[u as usize].order != 0x63 {
            todo_port!("0x476fc0", "AI siege tank attack handling");
        }
    }

    /// 0x4752b0: issue `order` with `t` as target and start it.
    pub(super) fn order_with_target(&mut self, d: &Data, u: u32, order: u8, t: u32) {
        self.units[u as usize].order_flags |= 1;
        let pos = if t != NIL { self.unit_sprite_pos(t) } else { (0, 0) };
        self.issue_order(d, u, order, true, pos, t, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// 0x475ec0: the damage bonus of weapon `w` for the owner's upgrade level.
    pub(super) fn weapon_upgrade_bonus(&self, d: &Data, u: u32, w: u8) -> u32 {
        let p = self.units[u as usize].player as usize;
        let up = d.dat.weapons.damage_upgrade[w as usize] as usize;
        let level = if up < 0x2e { self.players.upgrade_sc[p][up] } else { self.players.upgrade_bw[p][up - 0x2e] } as u32;
        d.dat.weapons.damage_bonus[w as usize] as u32 * level
    }

    /// 0x479b40: melee hit on the order target with the ground weapon.
    pub fn attack_melee(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            return;
        }
        let w = self.ground_weapon(d, u);
        if self.units[u as usize].status & 0x4000_0000 != 0 {
            let dir = self.units[u as usize].fl.heading;
            self.hallucination_hit(d, t, u, w, dir);
            return;
        }
        let (player, dir) = (self.units[u as usize].player, self.units[u as usize].fl.heading);
        let bonus = self.weapon_upgrade_bonus(d, u, w);
        let dmg = (bonus + d.dat.weapons.damage[w as usize] as u32) << 8;
        self.damage_unit(d, t, dmg, w, 1, dir, u, player);
    }

    /// 0x479c30: iscript `useweapon`: weapon `w` at the order target position.
    pub fn use_weapon(&mut self, d: &Data, u: u32, w: u8) {
        if d.dat.weapons.flingy[w as usize] == 0 {
            return;
        }
        let un = &self.units[u as usize];
        let (p, player, dir) = (un.order_target_pos, un.player, un.fl.heading);
        self.create_bullet(d, u, w, p.0, p.1, player, dir);
        if d.dat.weapons.damage_factor[w as usize] == 2 {
            self.create_bullet(d, u, w, p.0, p.1, player, dir);
        }
    }
}

/// AI target choice (0x477160 and its helpers).
impl State {
    /// 0x431de0: the best target of `u`'s player (by `u`) among the units in `u`'s
    /// region grown by 0x120, scored on strength `kind` (0 air, 1 ground).
    fn ai_region_search_target(&mut self, d: &Data, u: u32, kind: usize) -> u32 {
        let (x, y) = self.unit_sprite_pos(u);
        let r = super::path::region_at(&self.sai, x as i32, y as i32);
        let [l, t, rt, b] = self.sai.regions[r as usize].rect;
        let bx = [l.wrapping_sub(0x120), t.wrapping_sub(0x120), rt.wrapping_add(0x120), b.wrapping_add(0x120)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, bx);
        let units = search.results(start);
        search.pop();
        self.search = search;
        let p = self.units[u as usize].player;
        self.ai_best_target(d, &units, p, u, r, kind)
    }

    /// 0x431e90 (`kind` 1, ground) / 0x431ee0 (`kind` 0, air): the target `u`'s AI has for
    /// its region (a military unit takes its region's, else a search around `u`).
    fn ai_unit_target(&mut self, d: &Data, u: u32, kind: usize) -> u32 {
        let ai = self.units[u as usize].ai;
        if ai_kind(ai) != AI_MILITARY {
            return self.ai_region_search_target(d, u, kind);
        }
        let m = &self.ai.military.items[ai_index(ai) as usize];
        if m.parent == u && d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 == 0 {
            let rg = self.reg(m.region.0, m.region.1);
            return if kind == 1 { rg.ground_target } else { rg.air_target };
        }
        NIL
    }

    /// 0x476c50 (EAX = `t`) as the 0x477160 callers use it.
    fn ai_cant_reach_unit(&mut self, d: &Data, u: u32, t: u32) -> bool {
        let p = self.unit_sprite_pos(t);
        self.cant_reach_ground(d, u, p)
    }

    /// The test 0x477160 repeats on every candidate `c`: unusable when missing, out of
    /// reach (`p4`), not attackable, or a mine / scarab / interceptor (unless `p3`).
    fn ai_target_bad(&mut self, d: &Data, u: u32, c: u32, p3: bool, p4: bool) -> bool {
        c == NIL
            || (p4 && self.ai_cant_reach_unit(d, u, c))
            || !self.can_attack(d, u, c, true)
            || (!p3 && super::ai_region::is_minor_target(self.units[c as usize].unit_type))
    }

    /// 0x476930 (EAX = `c`): `c` is an enemy of `u` that can hit it from where it is
    /// (a damaged hallucination never counts; a loaded bunker always does).
    pub(super) fn ai_threatens(&self, d: &Data, c: u32, u: u32) -> bool {
        let cu = &self.units[c as usize];
        if self.alliances[self.units[u as usize].player as usize][self.alliance_owner(c)] != 0 {
            return false;
        }
        if cu.status & 0x4000_0000 != 0 && self.life_max(d, c) != self.life_now(d, c) {
            return false;
        }
        if self.can_attack(d, c, u, true) {
            return !self.cannot_hit_now(d, c, u);
        }
        cu.unit_type == 0x7d && self.has_loaded_units(c)
    }

    /// 0x43b790 (EAX = `a`, ECX = `u`, stack `b`): the better of two candidate targets for
    /// `u` (interceptors count as their carrier; NIL when neither is usable).
    fn ai_better_target(&self, d: &Data, u: u32, a: u32, b: u32) -> u32 {
        let carrier = |c: u32| {
            if c == NIL {
                return NIL;
            }
            let cu = &self.units[c as usize];
            let k = cu.ptr_at(0xc0);
            if cu.unit_type == 0x49 && k != NIL && (self.can_hit_now(d, u, k) || self.units[u as usize].status & 0x20000 != 0) { k } else { c }
        };
        let (mut a, mut b) = (carrier(a), carrier(b));
        let ok = |c: u32| c != NIL && !self.ignores_target(d, u, c) && self.can_attack(d, u, c, true);
        if !ok(a) {
            a = NIL;
        }
        if !ok(b) {
            b = NIL;
        }
        if a == NIL {
            return b;
        }
        if b == NIL {
            return a;
        }
        let bunker = |c: u32| self.units[c as usize].unit_type == 0x7d && self.has_loaded_units(c);
        if !bunker(b) && !self.is_armed(d, b) && self.is_armed(d, a) {
            return a;
        }
        if !(bunker(a) || self.is_armed(d, a) || !self.is_armed(d, b)) {
            return b;
        }
        if !(self.ai_threatens(d, a, u) || !self.ai_threatens(d, b, u)) {
            return b;
        }
        if self.can_hit_now(d, u, a) {
            return a;
        }
        if self.can_hit_now(d, u, b) {
            return b;
        }
        if self.unit_box_gap(d, u, a) <= self.unit_box_gap(d, u, b) { a } else { b }
    }

    /// 0x439050: `u`'s AI does not forbid looking for a new target (a military unit in
    /// the region it is assigned to does, while that region is in state 7).
    fn ai_may_look_for_target(&self, u: u32) -> bool {
        let ai = self.units[u as usize].ai;
        if ai_kind(ai) != AI_MILITARY {
            return true;
        }
        let (p, r) = self.ai.military.items[ai_index(ai) as usize].region;
        let rg = self.reg(p, r);
        let (x, y) = self.unit_sprite_pos(u);
        super::path::region_at(&self.sai, x as i32, y as i32) != rg.id || rg.state != 7
    }

    /// 0x477160: an AI unit looks for its order / auto target (the region's target, the
    /// current ones, else the nearest enemy) and attacks the best. `p3` allows mines,
    /// scarabs and interceptors; `p4` requires the target to be reachable. True if it
    /// has a target. (Its stack argument 1 only matters to the siege tank part of 0x476fc0.)
    pub(super) fn ai_choose_target(&mut self, d: &Data, u: u32, p3: bool, mut p4: bool) -> bool {
        let un = &self.units[u as usize];
        if un.ai == NIL || !self.is_armed(d, u) {
            return false;
        }
        let head = un.order_queue_head;
        let head_order = if head != NIL { Some(self.orders[head as usize].order) } else { None };
        if head_order == Some(0x98) {
            return false;
        }
        if un.order == 0x60 {
            p4 = true;
            if un.auto_target == NIL && head_order == Some(0xa) {
                return false;
            }
        }
        let (ot, mt) = (un.order_target_unit, un.move_target_unit);
        if ot != NIL && un.status & 0x20000 != 0 && mt != NIL && self.is_enemy(u, mt) && self.unit_at_move_target(u) == 2 {
            let sp = self.unit_sprite_pos(u);
            let un = &mut self.units[u as usize];
            un.fl.move_target = sp;
            un.status &= !0x8_0000;
            self.units[ot as usize].status |= 0x80;
            if ot == self.units[u as usize].auto_target {
                self.units[u as usize].auto_target = NIL;
            }
            self.clear_order_target(d, u);
        }
        // The region's target (the unit's own kind first), else the other kind.
        let kind = if self.units[u as usize].status & 4 == 0 { 1 } else { 0 };
        let mut region = self.ai_unit_target(d, u, kind);
        if self.ai_target_bad(d, u, region, p3, p4) {
            region = self.ai_unit_target(d, u, 1 - kind);
            if self.ai_target_bad(d, u, region, p3, p4) {
                region = NIL;
            }
        }
        // The order target, if the order uses weapon targeting.
        let mut order_t = if d.dat.orders.use_weapon_targeting[self.units[u as usize].order as usize] != 0 { self.units[u as usize].order_target_unit } else { NIL };
        if self.ai_target_bad(d, u, order_t, p3, p4) {
            order_t = NIL;
        }
        if p4 {
            let a = self.units[u as usize].auto_target;
            if a != NIL && self.ai_cant_reach_unit(d, u, a) {
                self.units[u as usize].auto_target = NIL;
            }
        }
        let auto = self.units[u as usize].auto_target;
        if auto != NIL && !self.disabled(auto) {
            let au = &self.units[auto as usize];
            let (atype, aot) = (au.unit_type, au.order_target_unit);
            if (atype != 0x7d && (aot == NIL || self.units[aot as usize].player != self.units[u as usize].player)) || (p4 && self.ai_cant_reach_unit(d, u, auto)) {
                self.units[u as usize].auto_target = NIL;
            }
            let auto = self.units[u as usize].auto_target;
            if auto == NIL || !self.can_attack(d, u, auto, true) || (!p3 && super::ai_region::is_minor_target(self.units[auto as usize].unit_type)) {
                self.units[u as usize].auto_target = NIL;
            }
        }
        let auto = self.units[u as usize].auto_target;
        let first = self.ai_better_target(d, u, order_t, region);
        let mut t = self.ai_better_target(d, u, first, auto);
        if t == NIL {
            if !self.ai_may_look_for_target(u) {
                return false;
            }
            let found = if self.ai_guard_return(d, u, 1) { NIL } else { self.find_auto_target(d, u) };
            t = self.ai_better_target(d, u, NIL, found);
            if self.ai_target_bad(d, u, t, p3, p4) {
                return false;
            }
        }
        if self.units[u as usize].order_target_unit != t {
            if self.units[u as usize].status & 0x20 == 0 {
                self.order_attack(d, u, t, true);
            } else {
                self.units[u as usize].order_target_unit = t;
            }
            let ty = self.units[u as usize].unit_type;
            if (is_carrier(ty) || is_reaver_type(ty)) && !self.can_hit_now(d, u, t) {
                self.units[u as usize].status |= 8;
            }
        }
        true
    }
}
