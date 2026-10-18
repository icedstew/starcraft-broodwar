//! AI military groups moving together (the helpers of HarassMove 0x478ec0): a unit of a
//! region's army waits for or follows the region's slowest military unit.

use super::ai::{ai_region_ref, REQ_UNIT};
use super::ai_town::{ai_index, ai_kind, ai_ref, AI_BUILDING, AI_GUARD, AI_MILITARY};
use super::list::NIL;
use super::path::{approx_distance, region_at};
use super::{Data, State};

/// 0x436930: orders (the flee / retreat moves and the like) a computer unit's reaction
/// leaves alone when it is not told to act (`p3` clear).
fn is_flee_order(o: u8) -> bool {
    matches!(o, 0x90 | 0x77 | 0x92 | 0x8f | 0x93 | 0x7a | 0x73 | 0x8e | 0x79 | 0x71)
}

/// 0x447370: always 0.
fn ai_447370(_player: u8) -> bool {
    false
}

impl State {
    /// 0x4369f0: region state 8 / 9 (attacking).
    pub(super) fn region_attacking(state: u8) -> bool {
        state == 8 || state == 9
    }

    /// 0x4381d0 (as a predicate): `u` is a non-building military unit of an AI region;
    /// returns that region (player, id). The guard shared by 0x438410, 0x439550, 0x43aa10
    /// and 0x43c170.
    pub(super) fn ai_army_region(&self, d: &Data, u: u32) -> Option<(u8, u16)> {
        let ai = self.units[u as usize].ai;
        if ai == NIL || ai_kind(ai) != AI_MILITARY {
            return None;
        }
        let m = &self.ai.military.items[ai_index(ai) as usize];
        if m.parent != u || d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 != 0 {
            return None;
        }
        Some(m.region)
    }

    /// 0x49ca70: region `r`'s centre in pixels.
    pub(super) fn region_center_px(&self, r: u16) -> (i16, i16) {
        let c = self.sai.regions[r as usize].center;
        ((c.0 >> 8) as i16, (c.1 >> 8) as i16)
    }

    /// 0x49caa0: region `r`'s centre in pixels.
    fn region_center_xy(&self, r: u16) -> (i32, i32) {
        let c = self.sai.regions[r as usize].center;
        (c.0 >> 8, c.1 >> 8)
    }

    /// 0x49cb40: the region `u` stands in.
    fn unit_region_of(&self, u: u32) -> u16 {
        let (x, y) = self.unit_sprite_pos(u);
        region_at(&self.sai, x as i32, y as i32)
    }

    /// 0x438410: `u` is in an army whose region has another member with order 0x60
    /// (and not status 0x40).
    pub(super) fn ai_group_mate_busy(&self, d: &Data, u: u32) -> bool {
        let Some((p, r)) = self.ai_army_region(d, u) else { return false };
        let reg = self.reg(p, r);
        if reg.state == 2 {
            return false;
        }
        let mut m = reg.military;
        while m != NIL {
            let it = &self.ai.military.items[m as usize];
            let o = it.parent;
            if o != u && self.units[o as usize].order == 0x60 && self.units[o as usize].status & 0x40 == 0 {
                return true;
            }
            m = it.link.next;
        }
        false
    }

    /// 0x438e10: `a` can get to `b`'s region from its own: the AI region test (0x437e70)
    /// for `b`'s player.
    fn ai_units_connected(&mut self, d: &Data, a: u32, b: u32) -> bool {
        let rb = self.unit_region_of(b);
        let ra = self.unit_region_of(a);
        let player = self.units[b as usize].player;
        self.ai_region_reachable(d, player, rb, ra)
    }

    /// 0x439550: `u` is fine where it is: not too far behind its region's slowest unit.
    pub(super) fn ai_keeps_up(&mut self, d: &Data, u: u32) -> bool {
        let Some((p, r)) = self.ai_army_region(d, u) else { return true };
        let reg = self.reg(p, r);
        let (id, s, state) = (reg.id, reg.slowest_military, reg.state);
        if s == NIL || self.units[s as usize].status & 0x40 != 0 || s == u || state == 2 {
            return true;
        }
        let rs = self.unit_region_of(s);
        if self.sai.regions[rs as usize].tile_count <= 0x3c || !self.ai_units_connected(d, s, u) {
            return true;
        }
        let (sx, sy) = self.unit_sprite_pos(s);
        let (ux, uy) = self.unit_sprite_pos(u);
        if approx_distance(ux as i32, sx as i32, uy as i32, sy as i32) <= 0x80 {
            return true;
        }
        let (cx, cy) = self.region_center_xy(id);
        approx_distance(ux as i32, cx, uy as i32, cy) <= 0x180
    }

    /// 0x43aa10: `u` is its region's slowest unit and some other member nearby (within
    /// 0x180, reachable, not keeping up) is more than 0x100 away: it waits.
    pub(super) fn ai_slowest_waits(&mut self, d: &Data, u: u32) -> bool {
        let Some((p, r)) = self.ai_army_region(d, u) else { return false };
        let reg = self.reg(p, r);
        if reg.state == 2 {
            return false;
        }
        let mut m = reg.military;
        while m != NIL {
            let it = &self.ai.military.items[m as usize];
            let (o, next) = (it.parent, it.link.next);
            m = next;
            if o == u {
                continue;
            }
            let (ux, uy) = self.unit_sprite_pos(u);
            let (ox, oy) = self.unit_sprite_pos(o);
            let dist = approx_distance(ux as i32, ox as i32, uy as i32, oy as i32);
            if dist > 0x180 || !self.ai_units_connected(d, o, u) || self.ai_keeps_up(d, o) || dist <= 0x100 {
                continue;
            }
            return true;
        }
        false
    }

    /// 0x43c170: `u` standing at its move target (status 0x80000 or not) outside its AI
    /// region's id: attack-move (0xb1 for type 0x22, else 0x9e) to that region's centre.
    pub(super) fn ai_army_return(&mut self, d: &Data, u: u32) {
        let Some((p, r)) = self.ai_army_region(d, u) else { return };
        let id = self.reg(p, r).id;
        let pos = self.unit_sprite_pos(u);
        if self.units[u as usize].fl.move_target != pos {
            return;
        }
        if self.unit_region_of(u) != id {
            let order = if self.units[u as usize].unit_type != 0x22 { 0x9e } else { 0xb1 };
            let c = self.region_center_px(id);
            self.order_at(d, u, order, c);
        }
    }

    /// 0x402310: `u` is locked down or under maelstrom (status 0x1000 / 0x2000).
    fn ai_unit_busy(&self, u: u32) -> bool {
        self.units[u as usize].status & 0x3000 != 0
    }

    /// 0x43ad60 (ESI = `t`, EDI = `u`): an interceptor `t` counts as its carrier when `u`
    /// can hit the carrier now (or `u` is status 0x20000).
    fn ai_attacker_proxy(&self, d: &Data, t: u32, u: u32) -> u32 {
        if t == NIL || u == NIL {
            return NIL;
        }
        if self.units[t as usize].unit_type == 0x49 {
            let k = self.units[t as usize].ptr_at(0xc0);
            if k != NIL && (self.can_hit_now(d, u, k) || self.units[u as usize].status & 0x2_0000 != 0) {
                return k;
            }
        }
        t
    }

    /// 0x431d00 / 0x4318e0: the summed strength (larger of air and ground) of `player`'s
    /// enemies in region `r` (EAX = region id, stack `player`); 0x431d00 (`air_only`)
    /// counts the air units, 0x4318e0 (stack `air`) the air or the ground ones.
    fn ai_region_enemy_strength(&mut self, d: &Data, player: u8, r: u16, air: bool, air_only: bool) -> u16 {
        let mut sum = 0u32;
        for c in self.units_in_region(r) {
            let st = self.units[c as usize].status;
            if air_only {
                if st & 4 == 0 {
                    continue;
                }
            } else if (st & 4 != 0) != air {
                continue;
            }
            let owner = self.alliance_owner_pub(c);
            if self.alliances[player as usize][owner] == 0 {
                let a = self.unit_strength(d, c, 0);
                let g = self.unit_strength(d, c, 1);
                sum = sum.wrapping_add(a.max(g));
            }
        }
        sum as u16
    }

    /// 0x43c230 (EAX = `u`): an unburrowed lurker not already burrowing burrows (order 0x74).
    fn ai_lurker_burrow(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.unit_type == 0x67 && un.status & 0x10 == 0 && un.order != 0x74 {
            self.order_now(d, u, 0x74);
        }
    }

    /// 0x4410c0 (EAX = `u`): the nearest enemy of `u` of unit type `ty` (any when 0xe4) on
    /// the whole map (0x4e8320 with callback 0x440980 and the box 0x6284a0).
    fn ai_nearest_enemy_of_type(&mut self, u: u32, ty: u16) -> u32 {
        let (x, y) = self.unit_sprite_pos(u);
        let player = self.units[u as usize].player as usize;
        let ix = super::finder::search(&self.finder.x, self.finder.count, true, x as i32) as i32;
        let iy = super::finder::search(&self.finder.y, self.finder.count, true, y as i32) as i32;
        let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
        self.nearest_unit(b, NIL, (x, y), [ix - 1, iy - 1, ix, iy], &mut |st: &mut State, c: u32| {
            (st.units[c as usize].unit_type == ty || ty == 0xe4) && st.alliances[player][st.alliance_owner_pub(c)] == 0
        })
    }

    /// 0x43e400 (EAX = `u`, stack `t`): a computer unit that should not stand and fight
    /// (status 0x20000, not order 0x76) falls back: it goes to an own base region (0x43db50)
    /// or runs from `t` (order 6 to 0x476a50's point). True when it did something.
    fn ai_retreat(&mut self, d: &Data, u: u32, t: u32) -> bool {
        let un = &self.units[u as usize];
        if un.status & 0x2_0000 == 0 || un.order == 0x76 {
            return false;
        }
        let ut = un.unit_type;
        if ut == 0x23 || (ut == 0x67 && un.u8_at(0x118) != 0) {
            return false;
        }
        if (self.is_spellcaster(d, u) || ut == 0x2a) && self.ai_ghost_to_enemy_region(d, u) {
            return true;
        }
        let un = &self.units[u as usize];
        if un.unit_type == 0x48 && un.u8_at(0xc9).wrapping_add(un.u8_at(0xc8)) < 5 && self.ai_ghost_to_enemy_region(d, u) {
            return true;
        }
        let p = self.flee_point(d, t, u);
        if p == self.unit_sprite_pos(u) {
            return false;
        }
        self.order_target(d, u, 6, p, NIL);
        true
    }

    /// 0x43ada0 (stack `u`, `t`, `flag`): the other units of `u`'s owner nearby (0x240 /
    /// 0x120 for buildings, 0x120 / 0x60 otherwise; the smaller in campaign AIs) that could
    /// fight `t` react to it too (0x43f320 with p3 0, p4 1); with `flag` (the caller is in
    /// a fighting region) only military units of a gathering / attacking / defending
    /// region; guard units only when `t` is within their weapon range + 0xc0 of home.
    fn ai_rally_nearby(&mut self, d: &Data, u: u32, t: u32, flag: bool) {
        let p = self.units[u as usize].player;
        let campaign = self.ai.players[p as usize].flags & 0x20 != 0;
        let r: i16 = if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 != 0 {
            if campaign { 0x120 } else { 0x240 }
        } else if campaign {
            0x60
        } else {
            0x120
        };
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(r), y.wrapping_sub(r), x.wrapping_add(r), y.wrapping_add(r)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        for c in found {
            let cu = &self.units[c as usize];
            if c == u || cu.player != p {
                continue;
            }
            let ct = cu.unit_type;
            let tt = self.units[t as usize].unit_type;
            if d.dat.units.flags[ct as usize] & 8 != 0 && d.dat.units.flags[tt as usize] & 8 == 0 {
                continue;
            }
            if ct == 0x47 {
                continue;
            }
            if !(self.can_attack(d, c, t, true) || (self.can_attack(d, t, c, true) && self.units[c as usize].status & 0x10 == 0)) {
                continue;
            }
            if flag {
                let ai = self.units[c as usize].ai;
                if ai == NIL || ai_kind(ai) != AI_MILITARY {
                    continue;
                }
                let (rp, rid) = self.ai.military.items[ai_index(ai) as usize].region;
                if !matches!(self.reg(rp, rid).state, 1 | 8 | 9 | 2) {
                    continue;
                }
                self.ai_react_to_target(d, c, t, false, true);
            }
            let ai = self.units[c as usize].ai;
            if ai != NIL && ai_kind(ai) == AI_GUARD {
                let home = self.ai.guards.items[ai_index(ai) as usize].home;
                let (tx, ty) = self.unit_sprite_pos(t);
                let dist = approx_distance(tx as i32, home.0 as i32, ty as i32, home.1 as i32);
                let sub = self.units[c as usize].subunit;
                let wu = if self.is_turret(d, sub) { sub } else { c };
                let w = if self.units[c as usize].status & 4 != 0 {
                    d.dat.units.air_weapon[self.units[wu as usize].unit_type as usize]
                } else {
                    self.ground_weapon(d, wu)
                };
                if w == 0x82 || dist > self.weapon_range(d, wu, w).wrapping_add(0xc0) {
                    continue;
                }
            }
            self.ai_react_to_target(d, c, t, false, true);
        }
    }

    /// 0x43f320 (ECX = `u`, EDX = `target`, stack `p3`, `p4`): a computer player's unit
    /// `u` was attacked by / sees `target`. A building notes the hit for its town and a
    /// weak unfinished one cancels; the region of `u` and of `target` get their strengths
    /// and state; then the unit casts, flees (0x43e400), or attacks `target`. `p3`: called
    /// for a hit (alerts nearby units, may call for help); `p4`: called from 0x43ada0
    /// (no further alerts, no region bookkeeping).
    pub(super) fn ai_react_to_target(&mut self, d: &Data, u: u32, target: u32, p3: bool, p4: bool) {
        let p = self.units[u as usize].player;
        if p >= 8 || self.player_type(p) != 1 {
            return;
        }
        let ut = self.units[u as usize].unit_type;
        let ai = self.units[u as usize].ai;
        if d.dat.units.flags[ut as usize] & 1 != 0 && ai != NIL {
            self.ai.players[p as usize].previous_building_hit_second = self.trig.elapsed;
            if ai_kind(ai) == AI_BUILDING {
                let town = self.ai.buildings.items[ai_index(ai) as usize].town;
                self.ai.towns.items[town as usize].in_battle = 1;
            }
            if p3
                && target != NIL
                && d.dat.units.flags[self.units[target as usize].unit_type as usize] & 8 == 0
                && !self.units[u as usize].is_dead()
                && self.units[u as usize].status & 1 == 0
                && self.life_now(d, u) < self.life_max(d, u) / 4
            {
                self.cancel_unit(d, u);
                return;
            }
        }
        if target == NIL {
            return;
        }
        let mut tgt = self.ai_attacker_proxy(d, target, u);
        if self.units[tgt as usize].status & 0x20 != 0 {
            tgt = self.units[tgt as usize].connected_unit;
            if tgt == NIL {
                return;
            }
        }
        if self.units[tgt as usize].status & 0x800 != 0 && self.units[tgt as usize].status & 0x10 == 0 {
            tgt = self.ai_nearest_enemy_of_type(u, 0x47);
            if tgt == NIL {
                return;
            }
        }
        if p3 {
            self.ai_cloak_before_nuke(d, u);
            self.ai_lurker_burrow(d, u);
        }
        let ai = self.units[u as usize].ai;
        let mut hold = false;
        let mut fighting = false;
        if ai != NIL {
            if ai_kind(ai) == AI_MILITARY {
                let (rp, rid) = self.ai.military.items[ai_index(ai) as usize].region;
                if self.reg(rp, rid).state == 1 && self.ai_army_region(d, u).is_some() {
                    self.set_ai_region_state(d, rp, rid, 2);
                }
                fighting = self.ai_region_gathering(u, 0);
            }
            if p3 && self.can_carry(d, u) && self.units[u as usize].status & 0x2_0000 != 0 {
                if self.has_loaded_units(u) {
                    if self.units[u as usize].order != 0x6f {
                        self.order_now(d, u, 0x6f);
                    }
                    return;
                }
                if !self.ai_guard_return(d, u, 0) && self.ai_ghost_to_enemy_region(d, u) {
                    return;
                }
                if self.ai.players[p as usize].flags & 0x20 != 0 {
                    hold = true;
                }
            }
        }
        let (ux, uy) = self.unit_sprite_pos(u);
        let ru = region_at(&self.sai, ux as i32, uy as i32);
        if !hold {
            if !p4 {
                self.ai_rally_nearby(d, u, tgt, fighting);
            }
            if !fighting {
                let ai = self.units[u as usize].ai;
                fighting = false;
                if ai != NIL {
                    if !self.ai_region_gathering(u, 0) {
                        fighting = true;
                        if ai_kind(ai) == AI_GUARD && self.ai.players[p as usize].flags & 0x20 != 0 {
                            fighting = false;
                        }
                    }
                }
                let rt = self.unit_region_of(tgt);
                if fighting {
                    if matches!(self.reg(p, ru).state, 5 | 4) {
                        self.set_ai_region_state(d, p, ru, 6);
                    }
                    if self.reg(p, ru).state != 0 && self.reg(p, rt).state == 0 && self.trig.elapsed > 0x3c {
                        self.set_ai_region_state(d, p, rt, 5);
                    }
                }
                if !p4 {
                    let id = self.reg(p, rt).id;
                    let air = self.ai_region_enemy_strength(d, p, id, false, true);
                    let ground = self.ai_region_enemy_strength(d, p, id, false, false);
                    let t = &mut self.ai.regions[p as usize][rt as usize];
                    t.enemy_air_strength = air;
                    t.enemy_ground_strength = ground;
                    let (na, ng) = (t.needed_air_strength, t.needed_ground_strength);
                    let r = &mut self.ai.regions[p as usize][ru as usize];
                    if r.needed_air_strength < na {
                        r.needed_air_strength = na;
                    }
                    if r.needed_ground_strength < ng {
                        r.needed_ground_strength = ng;
                    }
                }
            }
        }
        self.ai.regions[p as usize][ru as usize].flags |= 0x20;
        if self.units[u as usize].status & 1 == 0 {
            return;
        }
        let order = self.units[u as usize].order;
        if matches!(order, 0x6a | 0x96 | 0x21) {
            return;
        }
        if !p3 && is_flee_order(order) {
            return;
        }
        if ut == 0 && order == 0x5c {
            return;
        }
        let busy = self.ai_unit_busy(u);
        if !busy && self.ai_cast_spell(d, u, p3 as u32) {
            return;
        }
        if order == 0x42 || order == 6 {
            return;
        }
        if p3 {
            let un = &self.units[u as usize];
            if ut == 0x48 && un.u8_at(0xc9).wrapping_add(un.u8_at(0xc8)) < 3 && self.ai_ghost_to_enemy_region(d, u) {
                return;
            }
            let tstatus = self.units[tgt as usize].status;
            if tstatus & 0x2_0000 == 0 && self.ai.players[p as usize].flags & 0x20 == 0 {
                let mx = self.life_max(d, u);
                if self.life_now(d, u) <= (mx * 3) / 4 && matches!(ut, 0x47 | 8 | 0x2b) && self.ai_ghost_to_enemy_region(d, u) {
                    return;
                }
                let mx = self.life_max(d, u);
                if self.life_now(d, u) <= (mx * 2) / 3 && matches!(ut, 0xc | 0x48 | 0x46) && self.ai_ghost_to_enemy_region(d, u) {
                    return;
                }
            }
            if ut == 0x67 && self.units[u as usize].status & 0x10 == 0 && order != 0x74 && order != 0x76 && self.ai_retreat(d, u, tgt) {
                return;
            }
            if !busy && self.units[u as usize].status & 0x2_0000 != 0 {
                let pos = self.unit_sprite_pos(tgt);
                if !self.can_reach_spot(u, pos) && self.ai_retreat(d, u, tgt) {
                    return;
                }
            }
        }
        if self.units[u as usize].order_target_unit == tgt {
            return;
        }
        let tt = self.units[tgt as usize].unit_type;
        let fl = &d.dat.units.flags;
        if fl[ut as usize] & 8 != 0 && fl[tt as usize] & 8 == 0 && tt != 0x25 && !ai_447370(p) {
            if p3 {
                self.ai_retreat(d, u, tgt);
            }
            return;
        }
        if p3 && self.units[u as usize].ai != NIL && self.cloaked_from(tgt, u) {
            self.ai_call_support(d, u, tgt);
        }
        if busy {
            return;
        }
        if self.can_attack(d, u, tgt, true) {
            let sub = self.units[u as usize].subunit;
            self.units[u as usize].auto_target = tgt;
            if self.is_turret(d, sub) {
                self.units[sub as usize].auto_target = tgt;
            }
            self.ai_choose_target(d, u, true, false);
            return;
        }
        if self.units[u as usize].ai == NIL {
            return;
        }
        if order == 0x9f {
            let p = self.flee_point(d, tgt, u);
            if p != self.unit_sprite_pos(u) {
                self.order_target(d, u, 6, p, NIL);
                return;
            }
        }
        if !p3 || ut == 0x22 {
            return;
        }
        let ot = self.units[u as usize].order_target_unit;
        if ot != NIL && self.can_attack(d, ot, ot, true) {
            return;
        }
        self.ai_retreat(d, u, tgt);
    }
}

impl State {
    /// 0x4484a0: after a request for a morph (`id`), the units it is made from are
    /// requested too (one priority lower, same value).
    fn ai_request_morph_parts(&mut self, d: &Data, player: u8, id: u16, priority: u8, val: u32) {
        let (part, n) = match id {
            0x2c | 0x3e => (0x2b, 1),
            0x44 => (0x43, 2),
            0x3f => (0x3d, 2),
            0x67 => (0x26, 1),
            _ => return,
        };
        for _ in 0..n {
            self.ai_add_need(d, player, REQ_UNIT, part);
            self.ai_push_request(player, REQ_UNIT, part, priority.wrapping_sub(1), val);
        }
    }

    /// 0x43b6d0 (ECX = `ty`, ESI = `u`, stack `in_transport_blocks`): `u` is a finished
    /// unit of type `ty` (a sieged tank counts for 5) with an AI, not busy with order
    /// 0x6a / 0x96 (0x436e40), not threatened by its target (0x476930), and healthy
    /// enough (half its hp + shields, except for the types that always count).
    fn ai_unit_spare(&self, d: &Data, u: u32, ty: u32, in_transport_blocks: bool) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type as u32;
        if un.sprite == NIL || un.order == 0 || d.dat.units.flags[t as usize] & 0x10 != 0 || un.ai == NIL {
            return false;
        }
        if t != ty && !(ty == 5 && t == 0x1e) {
            return false;
        }
        // 0x436e40
        if !(un.status & 1 != 0 && un.order != 0x6a && un.order != 0x96) {
            return false;
        }
        let target = un.order_target_unit;
        if target != NIL && self.ai_threatens(d, target, u) {
            return false;
        }
        if in_transport_blocks && un.status & 0x20 != 0 {
            return false;
        }
        if t == 0 {
            return un.order != 0x5c;
        }
        if matches!(t, 0x20 | 0x25 | 0x26 | 0x41 | 0x42) {
            return true;
        }
        let max = self.life_max(d, u);
        (max - (max >> 31)) >> 1 <= self.life_now(d, u)
    }

    /// 0x43bc40 (stack: region (`p`, `r`), `ty`, centre `cx` / `cy`, target region `tgt`):
    /// the military AI of the nearest spare unit of type `ty` (not a tank) whose region
    /// can do without it; the path from the unit's region to the centre's is worked out
    /// (0x443fb0) when it walks, the result is not used. NIL when none.
    fn ai_find_spare_military(&mut self, d: &Data, p: u8, r: u16, ty: u32, cx: i32, cy: i32, tgt: u16) -> u32 {
        if ty == 5 {
            return NIL;
        }
        let src_state = self.reg(p, r).state;
        let mut best = NIL;
        let mut best_dist = 9999999u32;
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let cur = u;
            u = self.units[cur as usize].player_next;
            if !self.ai_unit_spare(d, cur, ty, true) {
                continue;
            }
            let node = self.units[cur as usize].ai;
            if ai_kind(node) != AI_MILITARY {
                continue;
            }
            let (mp, mid) = self.ai.military.items[ai_index(node) as usize].region;
            let tr = self.reg(mp, mid);
            if mid == r || matches!(tr.state, 7 | 8 | 9) {
                continue;
            }
            if (src_state == 7 || Self::region_attacking(src_state)) && mid == tgt {
                continue;
            }
            let (air, ground) = (self.units[cur as usize].air_strength as i32, self.units[cur as usize].ground_strength as i32);
            if air != 0 && (tr.all_military_air_strength as i32 - air) < tr.needed_air_strength as i32 {
                continue;
            }
            if ground != 0 && (tr.all_military_ground_strength as i32 - ground) <= tr.needed_ground_strength as i32 {
                continue;
            }
            if self.units[cur as usize].status & 4 == 0 {
                let a = region_at(&self.sai, cx, cy);
                let b = self.unit_region_of(cur);
                let player = self.units[cur as usize].player;
                self.ai_path_safe(b, a, player);
            }
            let (ux, uy) = self.unit_sprite_pos(cur);
            let dist = approx_distance(cx, ux as i32, cy, uy as i32);
            if dist < best_dist {
                best = node;
                best_dist = dist;
            }
        }
        best
    }

    /// 0x43bac0 (stack: region (`p`, `r`), `cap`, `ty`, centre `cx` / `cy`, target region
    /// `tgt`): the AI (guard post or military) of the nearest spare unit of type `ty` that
    /// the region may take: a guard post when the region attacks (a campaign AI: unless
    /// it helps), a military unit of a region that is not the same / quiet, with a
    /// defense priority below `cap`. NIL when none.
    fn ai_find_spare_far(&mut self, d: &Data, p: u8, r: u16, cap: u32, ty: u32, cx: i32, cy: i32, tgt: u16) -> u32 {
        let src_state = self.reg(p, r).state;
        let campaign = self.ai.players[p as usize].flags & 0x20 != 0;
        let mut best = NIL;
        let mut best_dist = 9999999u32;
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let cur = u;
            u = self.units[cur as usize].player_next;
            if !self.ai_unit_spare(d, cur, ty, false) {
                continue;
            }
            let (ux, uy) = self.unit_sprite_pos(cur);
            let dist = approx_distance(cx, ux as i32, cy, uy as i32);
            let node = self.units[cur as usize].ai;
            if dist >= best_dist || node == NIL {
                continue;
            }
            if ai_kind(node) == AI_GUARD {
                let take = if campaign { src_state != 7 } else { Self::region_attacking(src_state) };
                if take {
                    best = node;
                    best_dist = dist;
                }
            } else if ai_kind(node) == AI_MILITARY {
                let (mp, mid) = self.ai.military.items[ai_index(node) as usize].region;
                let tr = self.reg(mp, mid);
                if mid == r || (matches!(tr.state, 1 | 2) && !campaign) {
                    continue;
                }
                let un = &self.units[cur as usize];
                if un.status & 0x20 != 0 {
                    if cap == 0x14 {
                        continue;
                    }
                    if self.ai.players[un.player as usize].flags & 0x20 == 0 && !Self::region_attacking(src_state) {
                        continue;
                    }
                }
                if (src_state == 7 || Self::region_attacking(src_state)) && mid == tgt {
                    continue;
                }
                if (tr.defense_priority_add as u32) < cap {
                    best = node;
                    best_dist = dist;
                }
            }
        }
        best
    }

    /// 0x43e2e0 (EAX = region (`p`, `r`), stack `ty`, `cap`): bring a spare unit of type
    /// `ty` into the region's military: from another region's army (0x43bc40), else from
    /// a guard post or an army the region may take from (0x43bac0). The old region's
    /// strengths are counted again (0x43a390). True when a unit was found.
    fn ai_pull_unit(&mut self, d: &Data, p: u8, r: u16, ty: u32, cap: u32) -> bool {
        let c = self.sai.regions[r as usize].center;
        let (cx, cy) = (c.0 >> 8, c.1 >> 8);
        let tgt = self.reg(p, r).target_region_id;
        let mut node = self.ai_find_spare_military(d, p, r, ty, cx, cy, tgt);
        if node == NIL {
            node = self.ai_find_spare_far(d, p, r, cap, ty, cx, cy, tgt);
            if node == NIL {
                return false;
            }
        }
        let idx = ai_index(node) as usize;
        let (rp, rr) = match ai_kind(node) {
            AI_GUARD => {
                let u = self.ai.guards.items[idx].parent;
                self.units[u as usize].ai = NIL;
                self.region_add_military(d, p, r, u, true);
                self.ai.guards.items[idx].parent = NIL;
                (p, self.unit_region_of(u))
            }
            AI_MILITARY => {
                let m = &self.ai.military.items[idx];
                let (u, old) = (m.parent, m.region);
                self.region_remove_military(old.0, old.1, idx as u32);
                self.units[u as usize].ai = NIL;
                self.region_add_military(d, p, r, u, true);
                if self.units[u as usize].status & 0x20 != 0 {
                    self.unload_unit(d, u);
                }
                old
            }
            _ => return false,
        };
        self.region_recount(d, rp, rr);
        true
    }

    /// 0x43e760 (EAX = `list` of unit ids + 1, EBX = region (`p`, `r`)): the strength
    /// (air or ground) of the units of `list` that could be pulled into the region
    /// (with its defense priority as the cap).
    fn ai_pull_list(&mut self, d: &Data, p: u8, r: u16, list: [u16; 0x14], air: bool) -> u32 {
        let cap = self.reg(p, r).defense_priority_add as u32;
        let mut sum = 0u32;
        for e in list {
            if e == 0 {
                break;
            }
            let id = e as u32 - 1;
            if self.ai_pull_unit(d, p, r, id, cap) {
                sum = sum.wrapping_add(d.strength[if air { 0 } else { 1 }][id as usize]);
            }
        }
        sum
    }

    /// 0x43f100 (EAX = region `r`, stack target region `tgt`, `air`, `need`): ask for the
    /// units that bring region `r`'s army to `need` more strength for `tgt` (air or
    /// ground enemies): first the units of the defense lists that can be pulled in
    /// (not for an attack target, state 4, with a real size), else request them. Returns
    /// the strength gained or asked for (999 when there is nothing to ask).
    fn ai_region_defense(&mut self, d: &Data, p: u8, r: u16, tgt: u16, air: bool, need: u32) -> u32 {
        let pi = p as usize;
        let unwalkable = self.sai.regions[r as usize].flags == 0x1ffd || self.sai.regions[tgt as usize].flags == 0x1ffd;
        if self.reg(p, tgt).state == 4 {
            if self.sai.regions[tgt as usize].tile_count < 0x10 {
                return 999;
            }
        } else {
            let k = match (air, unwalkable) {
                (false, false) => 4,
                (false, true) => 6,
                (true, false) => 5,
                (true, true) => 7,
            };
            let list = self.ai.players[pi].defense_units[k];
            let s = self.ai_pull_list(d, p, r, list, air);
            if s != 0 {
                return s;
            }
        }
        let k = match (air, unwalkable) {
            (false, false) => 0,
            (false, true) => 2,
            (true, false) => 1,
            (true, true) => 3,
        };
        let list = self.ai.players[pi].defense_units[k];
        if list[0] == 0 {
            return 999;
        }
        let priority = self.ai.players[pi].defense_priority_base.wrapping_add(self.reg(p, r).defense_priority_add);
        let val = ai_region_ref(p, r);
        let mut total = 0u32;
        for e in list {
            if e == 0 {
                break;
            }
            let id = e - 1;
            // 0x4485b0 (flag 0x200 off: the request is made, flag 1 off, flag 0x40 on)
            self.ai_request_unit(d, p, id, priority, val, false, false);
            total = total.wrapping_add(d.strength[if air { 0 } else { 1 }][id as usize]);
            if self.ai.players[pi].flags & 0x20 != 0 && need <= total {
                break;
            }
        }
        total
    }

    /// 0x43e670 (EAX = region (`p`, `r`) attacking, state 8): pull the units of the attack
    /// force the region misses into it, else request them (priority 0x3c; 0xa0 for a
    /// melee AI that was not hit in the last 30 seconds).
    fn ai_attack_force_requests(&mut self, d: &Data, p: u8, r: u16) {
        let flags = self.ai.players[p as usize].flags;
        let missing = self.attack_force_missing(d, p, r, flags & 0x20 == 0);
        let hit = self.ai.players[p as usize].previous_building_hit_second;
        let cap = if flags & 0x20 == 0 && (hit == 0 || self.trig.elapsed.wrapping_sub(hit) > 0x1d) { 0xa0 } else { 0x3c };
        for id in missing {
            if id < 0xe4 && !self.ai_pull_unit(d, p, r, id, cap) {
                self.ai_request_unit(d, p, id as u16, 0x3c, ai_region_ref(p, r), true, true);
            }
        }
    }

    /// 0x43f990: one computer player's region requests: every region that attacks (8)
    /// or defends (4+) asks for the strength it misses against its target's enemies;
    /// then (not with flag 8) 2 more observers (Protoss) and up to 5 (2 for a campaign
    /// AI) dropships / shuttles for the last region that waits (4 / 5), once the
    /// player has a control tower / observer.
    pub(super) fn ai_region_requests(&mut self, d: &Data, player: u8) {
        let p = player as usize;
        let mut last: Option<u16> = None;
        for r in 1..self.sai.region_count {
            let state = self.reg(player, r).state;
            if state <= 3 {
                continue;
            }
            if state == 8 {
                self.ai_attack_force_requests(d, player, r);
                continue;
            }
            if state == 5 || state == 4 {
                last = Some(r);
            }
            let tgt = if state < 7 { r } else { self.reg(player, r).target_region_id };
            let mut have = self.reg(player, r).all_military_air_strength as u32;
            let mut need = self.reg(player, tgt).needed_air_strength as u32;
            if need != 0 && have < need {
                while have < need {
                    have = have.wrapping_add(self.ai_region_defense(d, player, r, tgt, true, need - have));
                    need = self.reg(player, tgt).needed_air_strength as u32;
                }
            }
            let mut have = self.reg(player, r).all_military_ground_strength as u32;
            let mut need = self.reg(player, tgt).needed_ground_strength as u32;
            if need != 0 && have < need {
                while have < need {
                    let air = self.reg(player, tgt).enemy_air_strength != 0;
                    have = have.wrapping_add(self.ai_region_defense(d, player, r, tgt, air, need - have));
                    need = self.reg(player, tgt).needed_ground_strength as u32;
                }
            }
            let pa = &mut self.ai.players[p];
            if pa.flags & 0x40 != 0 {
                if pa.defense_priority_base != 0 {
                    pa.defense_priority_base -= 1;
                }
                pa.flags &= !0x40;
            }
        }
        if self.ai.players[p].flags & 8 != 0 {
            return;
        }
        let id: u16 = match self.player_races[p] {
            1 => {
                if self.players.completed_counts[0x73][p] == 0 {
                    return;
                }
                0xb
            }
            2 => {
                let count = self.players.unit_counts[0x54][p] as u32;
                if count < 2 {
                    for _ in 0..2 - count {
                        self.ai_add_need(d, player, REQ_UNIT, 0x54);
                        self.ai_push_request(player, 8, 0x54, 0x50, 0);
                    }
                }
                0x45
            }
            _ => return,
        };
        let Some(r) = last else { return };
        let count = self.players.unit_counts[id as usize][p] as u32;
        let limit = if self.ai.players[p].flags & 0x20 != 0 { 2 } else { 5 };
        let priority = if count != 0 { 0x32 } else { 0x50 };
        let val = ai_region_ref(player, r);
        if count < limit {
            for _ in 0..limit - count {
                self.ai.players[p].flags |= 0x40;
                self.ai_add_need(d, player, REQ_UNIT, id);
                self.ai_push_request(player, REQ_UNIT, id, priority, val);
                self.ai_request_morph_parts(d, player, id, priority, val);
            }
        }
    }

    /// 0x43ebb0 (stack `val`, `player`): defense requests for a town (`val` is the
    /// value the requests carry): a bunker (0x50) when a region wants one (flag 2),
    /// and for each finished bunker with room, a firebat (when the list allows them,
    /// none sits in the bunker and there are fewer firebats than bunkers) or a marine
    /// pulled from elsewhere (0x43e2e0) or requested (priority 0x14 / 100) for the
    /// bunker's region.
    pub(super) fn ai_town_defense_requests(&mut self, d: &Data, player: u8, val: u32) {
        let p = player as usize;
        for r in 1..self.sai.region_count {
            if self.reg(player, r).flags & 2 != 0 {
                let f = d.dat.units.flags[0x7d];
                let ty = if f & 8 != 0 { 4 } else { ((f & 2) << 1 | 3) as u8 };
                self.ai_add_need(d, player, ty, 0x7d);
                self.ai_push_request(player, ty, 0x7d, 0x50, val);
                break;
            }
        }
        let mut b = self.player_units[p];
        while b != NIL {
            let cur = b;
            b = self.units[cur as usize].player_next;
            let un = &self.units[cur as usize];
            if un.sprite == NIL
                || un.order == 0
                || un.unit_type != 0x7d
                || un.status & 1 == 0
                || self.loaded_space(d, cur) >= d.dat.units.space_provided[0x7d] as u32
            {
                continue;
            }
            let list = self.ai.players[p].defense_units[4];
            let listed = |id: u16| list.iter().take_while(|&&e| e != 0).any(|&e| e == id);
            let mut ty = None;
            if listed(0x21) {
                let fewer = self.players.completed_counts[0x20][p] < self.players.completed_counts[0x7d][p];
                if !self.any_passenger(d, cur, |st, c| st.units[c as usize].unit_type == 0x20) && fewer {
                    ty = Some(0x20);
                }
            }
            if ty.is_none() {
                if !listed(1) {
                    return;
                }
                ty = Some(0);
            }
            let ty = ty.unwrap_or(0);
            let (x, y) = self.unit_sprite_pos(cur);
            let r = region_at(&self.sai, x as i32, y as i32);
            let cap = if self.ai.players[p].flags & 0x20 != 0 && self.loaded_space(d, cur) != 0 { 0x14 } else { 100 };
            if !self.ai_pull_unit(d, player, r, ty, cap) {
                let val = ai_region_ref(player, r);
                self.ai.players[p].flags |= 0x40;
                self.ai_add_need(d, player, REQ_UNIT, ty as u16);
                self.ai_push_request(player, REQ_UNIT, ty as u16, cap as u8, val);
                self.ai_request_morph_parts(d, player, ty as u16, cap as u8, val);
                if cap == 100 {
                    return;
                }
            }
        }
    }

    /// 0x4379b0 (ECX = `player`, stack `ty`, `x`; EBX = `y`): the nearest finished
    /// unit of type `ty` that belongs to an army (not state 1 / 7 / 8 / 9) which keeps
    /// more ground strength than it needs without that unit.
    fn ai_find_spare_for_guard(&self, player: u8, ty: u16, x: i32, y: i32) -> u32 {
        let mut best = NIL;
        let mut best_dist = 9999999u32;
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            let next = un.player_next;
            if un.sprite != NIL && un.order != 0 && un.unit_type == ty && un.ai != NIL && ai_kind(un.ai) == AI_MILITARY {
                let (mp, mid) = self.ai.military.items[ai_index(un.ai) as usize].region;
                let tr = self.reg(mp, mid);
                if !matches!(tr.state, 1 | 8 | 9 | 7)
                    && tr.all_military_ground_strength as i32 - un.ground_strength as i32 > tr.needed_ground_strength as i32
                {
                    let (ux, uy) = self.unit_sprite_pos(u);
                    let dist = approx_distance(x, ux as i32, y, uy as i32);
                    if dist < best_dist {
                        best = u;
                        best_dist = dist;
                    }
                }
            }
            u = next;
        }
        best
    }

    /// 0x448630 (EAX = guard post `g`, ECX = `player`): request the guard's unit (2, 0x32)
    /// unless a building was hit in the last 30 seconds; its home region is the value.
    fn ai_guard_request(&mut self, d: &Data, player: u8, g: u32) {
        let hit = self.ai.players[player as usize].previous_building_hit_second;
        if hit != 0 && self.trig.elapsed.wrapping_sub(hit) <= 0x1d {
            return;
        }
        let gd = &self.ai.guards.items[g as usize];
        let id = if gd.unit_id == 0x1e { 5 } else { gd.unit_id };
        let home = gd.home;
        self.ai_add_need(d, player, REQ_UNIT, id);
        self.ai_push_request(player, 2, id, 0x32, ai_ref(AI_GUARD, g));
        let r = region_at(&self.sai, home.0 as i32, home.1 as i32);
        self.ai_request_morph_parts(d, player, id, 0x32, ai_region_ref(player, r));
    }

    /// 0x4630c0: one computer player's guard posts: a campaign AI drops posts whose unit
    /// died 3 times, a post whose unit is a hallucination is dropped; a post without a
    /// unit (after its build time passed) gets a spare Zergling / Scourge from an army
    /// (0x4379b0) or requests its unit (0x448630).
    pub(super) fn ai_guard_requests(&mut self, d: &Data, player: u8) {
        let p = player as usize;
        let mut g = self.ai.guard_lists[p];
        while g != NIL {
            let next = self.ai.guards.items[g as usize].link.next;
            let gd = &self.ai.guards.items[g as usize];
            let release = if self.ai.players[p].flags & 0x20 != 0 && gd.times_died >= 3 {
                true
            } else if gd.parent != NIL {
                self.units[gd.parent as usize].status & 0x4000_0000 != 0
            } else {
                self.ai_guard_post(d, player, g);
                false
            };
            if release {
                let mut head = self.ai.guard_lists[p];
                self.ai.guards.release(&mut head, g);
                self.ai.guard_lists[p] = head;
            }
            if self.ai.guard_lists[p] == NIL {
                return;
            }
            g = next;
        }
    }

    /// The part of 0x4630c0 for a guard post `g` without a unit.
    fn ai_guard_post(&mut self, d: &Data, player: u8, g: u32) {
        let gd = &self.ai.guards.items[g as usize];
        let prev = gd.previous_update;
        if prev != 0 {
            let mut t = gd.unit_id as usize;
            if t == 0x1e {
                t = 5;
            }
            let bt = |i: usize| d.dat.units.build_time[i] as u32;
            let mut time = bt(t);
            match t {
                0x2c | 0x3e => time += bt(0x2b),
                0x3f => time += bt(0x3d),
                0x44 => time += bt(0x43),
                0x67 => time += bt(0x26),
                _ => {}
            }
            let limit = if gd.times_died == 0 { time / 15 + 5 + prev } else { prev + 300 };
            if self.trig.elapsed <= limit {
                return;
            }
        }
        let (ox, oy) = (gd.other_home.0, gd.other_home.1);
        let t = gd.unit_id;
        let r = region_at(&self.sai, ox as i32, oy as i32);
        let tr = self.reg(player, r);
        if tr.state == 3 || tr.air_target != NIL || tr.ground_target != NIL || tr.flags & 0x20 != 0 {
            return;
        }
        let spare = if t == 0x25 || t == 0x2f { self.ai_find_spare_for_guard(player, t, ox as i32, oy as i32) } else { NIL };
        if spare != NIL {
            self.ai_release_unit(d, spare, false);
            let gd = &mut self.ai.guards.items[g as usize];
            gd.home = gd.other_home;
            gd.parent = spare;
            self.units[spare as usize].ai = ai_ref(AI_GUARD, g);
            self.order_now(d, spare, 0x9c);
        } else {
            self.ai_guard_request(d, player, g);
            self.ai.guards.items[g as usize].previous_update = self.trig.elapsed;
        }
    }
}
