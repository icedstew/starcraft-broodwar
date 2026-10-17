//! AI military groups moving together (the helpers of HarassMove 0x478ec0): a unit of a
//! region's army waits for or follows the region's slowest military unit.

use super::ai_town::{ai_index, ai_kind, AI_BUILDING, AI_GUARD, AI_MILITARY};
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
