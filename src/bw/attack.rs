//! Attacking: the AttackUnit order (0x479bd0), weapon ranges and firing.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Weapon id meaning "none".
pub const NO_WEAPON: u8 = 0x82;

impl State {
    fn is_turret(&self, d: &Data, u: u32) -> bool {
        u != NIL && d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x10 != 0
    }

    /// The unit that holds the weapons: the turret if `u` has one.
    fn weapon_unit(&self, d: &Data, u: u32) -> u32 {
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
    fn weapon_against(&self, d: &Data, u: u32, target: u32) -> u8 {
        let w = self.weapon_unit(d, u);
        if self.units[target as usize].status & 4 != 0 {
            d.dat.units.air_weapon[self.units[w as usize].unit_type as usize]
        } else {
            self.ground_weapon(d, w)
        }
    }

    /// 0x401d60: `target` is cloaked and not detected by `u`'s owner.
    fn cloaked_from(&self, target: u32, u: u32) -> bool {
        let t = &self.units[target as usize];
        t.status & 0x300 != 0 && t.visibility & (1 << (self.units[u as usize].player & 0x1f)) == 0
    }

    /// 0x4e5e30: `target` is visible to `u`'s owner.
    fn target_visible(&self, target: u32, u: u32) -> bool {
        let bit = 1u32 << (self.units[u as usize].player & 0x1f);
        let t = &self.units[target as usize];
        if t.status & 0x300 != 0 && t.visibility & bit == 0 {
            return false;
        }
        self.sprites[t.sprite as usize].visibility as u32 & bit != 0
    }

    /// 0x402210: `target` is a damaged (< 50% hp) completed unit of type 0x6a.
    fn damaged_6a(&self, d: &Data, target: u32) -> bool {
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
    fn can_attack(&self, d: &Data, u: u32, target: u32, check_cloak: bool) -> bool {
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
        if matches!(t, 0x48 | 0x52) {
            // Carriers (0x401470).
            return true;
        }
        if matches!(t, 0x53 | 0x51) {
            // Reavers (0x401490): ground targets in the same region group (0x49cbb0).
            return ts & 4 == 0 && self.same_group(target, self.unit_sprite_pos(u));
        }
        if matches!(t, 0x2d | 0x31) {
            // 0x402750
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
    fn weapon_range(&self, d: &Data, u: u32, weapon: u8) -> u32 {
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
    fn in_attack_range(&self, d: &Data, u: u32, mut range: u32, target: u32) -> bool {
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
    fn cannot_hit_now(&self, d: &Data, u: u32, target: u32) -> bool {
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
            todo_port!("0x4e7420", "AI move-to-unit check");
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
    fn chase(&mut self, d: &Data, u: u32, target: u32) -> bool {
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
            let w = self.weapon_against(d, u, target);
            let wu = self.weapon_unit(d, u);
            let in_range = self.target_visible(target, u)
                && w < NO_WEAPON
                && {
                    let min = d.dat.weapons.min_range[w as usize];
                    (min == 0 || !self.box_distance_within(d, wu, min, target)) && {
                        let r = self.weapon_range(d, u, w);
                        self.in_attack_range(d, u, r, target)
                    }
                };
            if !in_range && !self.move_to_unit(d, u, self.units[u as usize].order_target_unit) {
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
            todo_port!("0x478804", "AI attack: 0x476c50 / 0x477160");
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

    /// 0x442160: how much `u` wants to attack `t` (0 best .. 5 never).
    fn target_priority(&self, d: &Data, u: u32, t: u32) -> i32 {
        let tt = self.units[t as usize].unit_type;
        let mut c = t;
        if tt == 0x7d {
            todo_port!("0x4e6c90", "bunker: first loaded unit");
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
        let _ = &mut c;
        p
    }

    /// 0x476000: target acquisition range in tiles, with range upgrades.
    pub(super) fn acquisition_range(&self, u: u32) -> u32 {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let p = un.player as usize;
        if matches!(t, 1 | 0x10 | 100 | 99 | 0x68) && un.status & 0x300 != 0 && un.order != 0x6b {
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
        let Some(list) = buckets.iter().find(|b| !b.is_empty()) else { return NIL };
        if list.len() == 1 {
            return list[0];
        }
        if self.units[u as usize].ai != NIL && self.units[u as usize].unit_type == 0x2f {
            todo_port!("0x4405e0", "AI scourge: prefer the toughest target (0x4026d0)");
        }
        // 0x4405e0: nearest (first on ties).
        let up = self.unit_sprite_pos(u);
        let dist = |st: &State, c: u32| {
            let p = st.unit_sprite_pos(c);
            super::path::approx_distance(up.0 as i32, p.0 as i32, up.1 as i32, p.1 as i32)
        };
        let mut best = 0;
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

    /// 0x477510: an auto-targeting unit (status 0x8000000) switches to a better target.
    fn pick_better_target(&mut self, d: &Data, u: u32) {
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
            if self.units[u as usize].ai != NIL {
                todo_port!("0x477160", "AI target choice");
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
            todo_port!("0x479bd0", "computer players: 0x462ea0");
        }
        self.pick_better_target(d, u);
        if !self.attack_unit_step(d, u) {
            return;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x43ffd0", "AI attack notification");
        }
        self.fire(d, u, 5);
    }

    /// 0x476ed0: fire at the order target with the right weapon, starting `anim`
    /// (`anim + 1` for air targets).
    pub fn fire(&mut self, d: &Data, u: u32, anim: u8) {
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

    /// 0x474a20 (0x474b40 with p = (0, 0)): queue `order` at `p` in front of the queue.
    pub(super) fn order_queue_front(&mut self, d: &Data, u: u32, order: u8, p: (i16, i16)) {
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            self.order_insert_before(d, u, order, 0xe4, p, NIL, head);
        } else {
            self.order_queue_append(d, u, order, 0xe4, p, NIL);
        }
    }

    /// 0x475dc0: weapon cooldown with acid spores, stim/adrenal and ensnare.
    fn weapon_cooldown(&self, d: &Data, u: u32, w: u8) -> u8 {
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
    fn weapon_ready(&mut self, d: &Data, u: u32, w: u8) -> bool {
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
    fn can_hit_now(&self, d: &Data, u: u32, target: u32) -> bool {
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
            todo_port!("0x43f320", "computer player reacts to the attack");
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
                todo_port!("0x4e97c0", "burrowed unit unburrows when attacked");
            }
            return;
        }
        let flags = d.dat.units.flags[t as usize];
        if flags & 1 == 0 && (!self.can_attack(d, u, attacker, true) || flags & 8 != 0) {
            todo_port!("0x4776c0", "flee from an attacker it can't fight (0x462930, 0x476a50)");
            return;
        }
        self.set_auto_target(d, u, attacker);
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
        if self.units[t as usize].unit_type == 0x49 && self.units[t as usize].connected_unit != NIL {
            t = self.units[t as usize].connected_unit;
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

    /// 0x479b40: melee hit on the order target with the ground weapon.
    pub fn attack_melee(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            return;
        }
        let w = self.ground_weapon(d, u);
        if self.units[u as usize].status & 0x4000_0000 != 0 {
            todo_port!("0x479730", "hallucination melee hit");
            return;
        }
        let (player, dir) = (self.units[u as usize].player, self.units[u as usize].fl.heading);
        // 0x475ec0: upgrade bonus.
        let up = d.dat.weapons.damage_upgrade[w as usize] as usize;
        let level = if up < 0x2e { self.players.upgrade_sc[player as usize][up] } else { self.players.upgrade_bw[player as usize][up - 0x2e] } as u32;
        let bonus = (d.dat.weapons.damage_bonus[w as usize] as u32 * level) & 0xffff;
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
