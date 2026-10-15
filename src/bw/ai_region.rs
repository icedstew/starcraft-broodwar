//! The AI's regions at work (0x43fc60 and the state handlers): each computer player's
//! regions recount their armies, pick targets for the military units in them, call
//! for help, move units between regions and attack (sai / tai military code).
//!
//! Region states (+5): 0 idle, 1 / 2 gathering (2 at 20+ ground units), 3 lost, 4 / 5
//! held, 6 defending, 7 sending help to +2, 8 / 9 attacking.

use super::ai::AiRegion;
use super::ai_town::{ai_index, ai_kind, ai_ref, AI_MILITARY};
use super::list::NIL;
use super::path::{approx_distance, region_at};
use super::{Data, State};
use crate::todo_port;

/// Unit types that never count as real targets: mines, scarabs, interceptors and the
/// like (0x47b450).
fn is_minor_target(t: u16) -> bool {
    matches!(t, 0x59 | 0x5a | 0x5f | 0x5e | 0x5d | 0x60)
}

impl State {
    fn reg(&self, p: u8, r: u16) -> &AiRegion {
        &self.ai.regions[p as usize][r as usize]
    }

    fn reg_mut(&mut self, p: u8, r: u16) -> &mut AiRegion {
        &mut self.ai.regions[p as usize][r as usize]
    }

    fn campaign_ai(&self, p: u8) -> bool {
        self.ai.players[p as usize].flags & 0x20 != 0
    }

    fn unit_region(&self, u: u32) -> u16 {
        let (x, y) = self.unit_sprite_pos(u);
        region_at(&self.sai, x as i32, y as i32)
    }

    fn region_center(&self, r: u16) -> (i32, i32) {
        let c = self.sai.regions[r as usize].center;
        (c.0 >> 8, c.1 >> 8)
    }

    fn enemy_of(&self, p: u8, u: u32) -> bool {
        let un = &self.units[u as usize];
        let owner = if un.player == 0xb { self.sprites[un.sprite as usize].owner } else { un.player };
        self.alliances[p as usize][owner as usize] == 0
    }

    /// The military AIs of a region, in list order.
    fn region_military(&self, p: u8, r: u16) -> Vec<u32> {
        let mut v = Vec::new();
        let mut m = self.reg(p, r).military;
        while m != NIL {
            v.push(m);
            m = self.ai.military.items[m as usize].link.next;
        }
        v
    }

    /// 0x43fc60: one region's step.
    pub(super) fn ai_region_step(&mut self, d: &Data, p: u8, r: u16) {
        let reg = self.reg_mut(p, r);
        reg.flags &= !4;
        reg.unk_count = reg.unk_count.saturating_sub(1);
        match reg.state {
            0 => {
                // 0x43b050
                self.region_recount(d, p, r);
                self.region_decay_needs(p, r);
            }
            1 | 2 => self.region_gather(d, p, r),
            3 => self.region_lost(d, p, r),
            4 | 5 => {
                // 0x43d550
                self.region_recount(d, p, r);
                self.region_pick_targets(d, p, r);
                if self.region_has_real_target(p, r) {
                    self.set_ai_region_state(d, p, r, 6);
                }
                self.region_decay_needs(p, r);
            }
            6 => self.region_defend(d, p, r),
            7 => self.region_send_help(d, p, r),
            8 | 9 => self.region_attack(d, p, r),
            _ => {}
        }
    }

    /// The two 0x436980 calls of an idle region (unless flag 0x40): its needed air and
    /// ground strength fade.
    fn region_decay_needs(&mut self, p: u8, r: u16) {
        if self.reg(p, r).flags & 0x40 != 0 {
            return;
        }
        let fast = !self.campaign_ai(p);
        let reg = self.reg_mut(p, r);
        reg.needed_air_strength = decay(reg.needed_air_strength, fast);
        reg.needed_ground_strength = decay(reg.needed_ground_strength, fast);
    }

    /// 0x43a390: the region's own and present military strength; what it needs grows to
    /// its enemies' (by half again for a melee AI), capped by +0x21c.
    fn region_recount(&mut self, d: &Data, p: u8, r: u16) {
        let mut local = (0u16, 0u16);
        let mut all = (0u16, 0u16);
        for m in self.region_military(p, r) {
            let u = self.ai.military.items[m as usize].parent;
            let un = &self.units[u as usize];
            if un.status & 0x20 != 0 {
                continue;
            }
            let (air, ground) = (un.air_strength as u16, un.ground_strength as u16);
            all.0 = all.0.wrapping_add(air);
            all.1 = all.1.wrapping_add(ground);
            if self.unit_region(u) == r {
                local.0 = local.0.wrapping_add(air);
                local.1 = local.1.wrapping_add(ground);
            }
        }
        let reg = self.reg_mut(p, r);
        reg.local_military_air_strength = local.0;
        reg.local_military_ground_strength = local.1;
        reg.all_military_air_strength = all.0;
        reg.all_military_ground_strength = all.1;
        if self.campaign_ai(p) {
            // 0x4e8280 with 0x438f60: own units standing in the region count too.
            let (cx, cy) = self.region_center(r);
            let b = [(cx - 0x100) as i16, (cy - 0x100) as i16, (cx + 0x100) as i16, (cy + 0x100) as i16];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, b);
            let found = search.results(start);
            search.pop();
            self.search = search;
            for u in found {
                let un = &self.units[u as usize];
                if un.status & 0x20 == 0 && un.player == p && self.unit_region(u) == r {
                    let (air, ground) = (un.air_strength as u16, un.ground_strength as u16);
                    let reg = self.reg_mut(p, r);
                    reg.all_military_air_strength = reg.all_military_air_strength.wrapping_add(air);
                    reg.local_military_air_strength = reg.local_military_air_strength.wrapping_add(air);
                    reg.all_military_ground_strength = reg.all_military_ground_strength.wrapping_add(ground);
                    reg.local_military_ground_strength = reg.local_military_ground_strength.wrapping_add(ground);
                }
            }
        }
        let melee = !self.campaign_ai(p);
        let cap = self.ai.players[p as usize].max_region_strength_need;
        let reg = self.reg_mut(p, r);
        let mut air = reg.enemy_air_strength as u32;
        if melee {
            air += air >> 1;
        }
        if (reg.needed_air_strength as u32) < air {
            reg.needed_air_strength = air as u16;
        }
        let mut ground = reg.enemy_ground_strength as u32;
        if melee {
            ground += ground >> 1;
        }
        if (reg.needed_ground_strength as u32) < ground {
            reg.needed_ground_strength = ground as u16;
        }
        if cap != 0 {
            if cap < reg.needed_air_strength {
                reg.needed_air_strength = cap;
            }
            if cap < reg.needed_ground_strength {
                reg.needed_ground_strength = cap;
            }
        }
        let _ = d;
    }

    /// 0x437e00: the region's air or ground target is something other than a minor
    /// target.
    fn region_has_real_target(&self, p: u8, r: u16) -> bool {
        let reg = self.reg(p, r);
        let real = |t: u32| t != NIL && !is_minor_target(self.units[t as usize].unit_type);
        real(reg.air_target) || real(reg.ground_target)
    }

    /// 0x43e4d0: state 6, defending: hold until the enemy is gone (state 5, or 0 for a
    /// campaign AI without a base there); with enemies but no base left, give up
    /// (0x43de40).
    fn region_defend(&mut self, d: &Data, p: u8, r: u16) {
        self.region_recount(d, p, r);
        self.region_pick_targets(d, p, r);
        if self.reg(p, r).unk_count != 0 {
            return;
        }
        if !self.campaign_ai(p) {
            let unwalkable = self.sai.regions[r as usize].flags == 0x1ffd;
            let reg = self.reg_mut(p, r);
            if unwalkable {
                reg.needed_ground_strength = reg.needed_ground_strength.max(1000);
            } else {
                reg.needed_air_strength = reg.needed_air_strength.max(1000);
            }
        }
        let reg = self.reg(p, r);
        if reg.air_target == NIL && reg.ground_target == NIL {
            let state = if self.campaign_ai(p) && !self.player_has_base_unit_in(d, r, p) { 0 } else { 5 };
            self.set_ai_region_state(d, p, r, state);
            return;
        }
        if !self.player_has_base_unit_in(d, r, p) {
            self.region_give_up(d, p, r);
        }
    }

    /// 0x43de40: outnumbered (and not just holding with zerglings / firebats), the
    /// region gives up (state 3, or 0 when gathering, attacking or a campaign AI still
    /// has bases near) and its units go to the regions they stand in. True if so.
    fn region_give_up(&mut self, d: &Data, p: u8, r: u16) -> bool {
        let reg = self.reg(p, r);
        if reg.local_military_air_strength >= reg.needed_air_strength && reg.local_military_ground_strength >= reg.needed_ground_strength {
            return false;
        }
        let campaign = self.campaign_ai(p);
        if !campaign && reg.military != NIL {
            let first = self.ai.military.items[reg.military as usize].parent;
            if matches!(self.units[first as usize].unit_type, 0x20 | 0x25) {
                return false;
            }
        }
        let mut state = 3;
        if (campaign && self.base_near_region(d, p, r)) || matches!(self.reg(p, r).state, 8 | 9 | 1 | 2) {
            state = 0;
        }
        self.set_ai_region_state(d, p, r, state);
        let mut m = self.reg(p, r).military;
        while m != NIL && self.reg(p, r).military != NIL {
            let next = self.ai.military.items[m as usize].link.next;
            let u = self.ai.military.items[m as usize].parent;
            let to = self.unit_region(u);
            self.region_remove_military(p, r, m);
            self.units[u as usize].ai = NIL;
            self.region_add_military(d, p, to, u, true);
            m = next;
        }
        true
    }

    /// 0x438ef0: `player` has a base unit in region `r` or a walkable neighbour of it.
    fn base_near_region(&mut self, d: &Data, p: u8, r: u16) -> bool {
        if self.player_has_base_unit_in(d, r, p) {
            return true;
        }
        let n = self.sai.regions[r as usize].walkable_neighbors as usize;
        for i in 0..n {
            let nb = self.sai.neighbor(r as usize, i);
            if self.player_has_base_unit_in(d, nb, p) {
                return true;
            }
        }
        false
    }

    /// 0x4371d0: military AI `m` leaves region `r` (ground count down) and is freed.
    pub(super) fn region_remove_military(&mut self, p: u8, r: u16, m: u32) {
        let u = self.ai.military.items[m as usize].parent;
        let flying = self.units[u as usize].status & 4 != 0;
        let reg = self.reg_mut(p, r);
        if !flying {
            reg.ground_unit_count = reg.ground_unit_count.wrapping_sub(1);
        }
        let mut head = reg.military;
        self.ai.military.release(&mut head, m);
        self.reg_mut(p, r).military = head;
    }

    /// 0x43da20: unit `u` joins region `r` (`p`'s) as a military AI (a lost region
    /// sends it to where it stands unless `keep`) and is sent there (0x43d5d0).
    pub(super) fn region_add_military(&mut self, d: &Data, p: u8, mut r: u16, u: u32, keep: bool) {
        if self.reg(p, r).state == 3 && !keep {
            r = self.unit_region(u);
        }
        let up = self.units[u as usize].player;
        let _ = up;
        let mut head = self.reg(p, r).military;
        let Some(m) = self.ai.military.alloc(&mut head) else { return };
        self.reg_mut(p, r).military = head;
        {
            let mi = &mut self.ai.military.items[m as usize];
            mi.ai_type = 4;
            mi.parent = u;
            mi.region = (p, r);
        }
        if self.units[u as usize].status & 4 == 0 {
            let reg = self.reg_mut(p, r);
            reg.ground_unit_count = reg.ground_unit_count.wrapping_add(1);
        }
        self.units[u as usize].ai = ai_ref(AI_MILITARY, m);
        let order = if self.units[u as usize].unit_type == 0x22 { 0xb1 } else { 0x9d };
        let (cx, cy) = self.region_center(r);
        self.ai_military_move(d, u, order, cx, cy);
        if matches!(self.reg(p, r).state, 8 | 9 | 1 | 2) {
            self.region_pick_slowest(p, r);
        }
    }

    /// 0x436f70: a campaign region's slowest unit (+0x24), ground units first.
    fn region_pick_slowest(&mut self, p: u8, r: u16) {
        if !self.campaign_ai(p) {
            return;
        }
        let cur = self.reg(p, r).slowest_military;
        let (mut ground, mut air) = ((NIL, i32::MAX), (NIL, i32::MAX));
        if cur != NIL {
            let s = self.units[cur as usize].fl.top_speed;
            if self.units[cur as usize].status & 4 == 0 {
                ground = (cur, s);
            } else {
                air = (cur, s);
            }
        }
        for m in self.region_military(p, r) {
            let u = self.ai.military.items[m as usize].parent;
            let s = self.units[u as usize].fl.top_speed;
            if self.units[u as usize].status & 4 == 0 {
                if s < ground.1 {
                    ground = (u, s);
                }
            } else if s < air.1 {
                air = (u, s);
            }
        }
        self.reg_mut(p, r).slowest_military = if ground.0 != NIL { ground.0 } else { air.0 };
    }

    /// 0x43d5d0: military unit `u` goes to (x, y) with `order` (0x9d move, 0x9e attack
    /// move when its region gathers or attacks; 0xb1 / 0xb2 for medics), scattered
    /// around the way from its region's centre when it can get there; transported units
    /// ask out first, sieged tanks fight or unsiege.
    fn ai_military_move(&mut self, d: &Data, u: u32, mut order: u8, mut x: i32, mut y: i32) {
        let r = self.units[u as usize].ai;
        if r == NIL || ai_kind(r) != AI_MILITARY || self.ai.military.items[ai_index(r) as usize].parent != u {
            return;
        }
        let t = self.units[u as usize].unit_type;
        if d.dat.units.flags[t as usize] & 1 != 0 {
            return;
        }
        let un = &self.units[u as usize];
        if un.status & 1 == 0 || un.order == 0x6a || un.order == 0x96 {
            return;
        }
        if un.status & 0x20 != 0 {
            self.unload_unit(d, u);
        } else if un.order == 0x62 || un.order == 6 {
            return;
        }
        let mut unsieged = false;
        if t == 0x1e {
            if self.units[u as usize].order == 0x63 {
                return;
            }
            if self.sieged_tank_has_enemy(d, u) {
                todo_port!("0x43d5d0", "AI sieged tank fights (0x477160, 0x443080, 0x401da0, 0x476fc0)");
                return;
            }
            self.order_now(d, u, 0x63);
            unsieged = true;
        }
        // 0x4381d0
        let r = self.units[u as usize].ai;
        if r == NIL || ai_kind(r) != AI_MILITARY || self.ai.military.items[ai_index(r) as usize].parent != u || d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 != 0 {
            return;
        }
        let (rp, rid) = self.ai.military.items[ai_index(r) as usize].region;
        let rstate = self.reg(rp, rid).state;
        if !is_detector_type(self.units[u as usize].unit_type) && (rstate == 1 || matches!(rstate, 8 | 9)) {
            order = if self.units[u as usize].unit_type == 0x22 { 0xb1 } else { 0x9e };
        }
        let st = self.units[u as usize].status;
        if st & 0x40 != 0 && st & 0x20 == 0 {
            self.ai_attack_order(d, u, 0x9c);
            return;
        }
        let player = self.units[u as usize].player;
        let dest = region_at(&self.sai, x, y);
        let here = self.unit_region(u);
        if self.ai_region_reachable(d, player, here, dest) {
            let (cx, cy) = self.region_center(here);
            let mut dir = crate::game::flingy::direction_to(cx, cy, x, y) as u32;
            dir += if self.rng.rand(0x38) & 8 != 0 { 0x40 } else { 0xc0 };
            if dir >= 0x100 {
                dir -= 0x100;
            }
            let table = crate::game::flingy::direction_table();
            let r1 = (self.rng.rand(0x38) & 0x7f) as i32;
            let px = ((table[dir as usize].0 * r1) >> 8) + x;
            let py = ((table[dir as usize].1 * r1) >> 8) + y;
            let dir2 = crate::game::flingy::direction_to(cx, cy, px, py) as usize;
            let dist = approx_distance(cx, px, cy, py) as i32;
            let r2 = (self.rng.rand(0x38) & 0x7f) as i32;
            let m = (dist + r2 - 0x40).max(0);
            let nx = ((table[dir2].0 * m) >> 8) + cx;
            let ny = ((table[dir2].1 * m) >> 8) + cy;
            if nx >= 0 && nx < self.map_px.0 as i32 && ny >= 0 && ny < self.map_px.1 as i32 {
                let g = |st: &State, a: i32, b: i32| st.sai.regions[region_at(&st.sai, a, b) as usize].group;
                if g(self, nx, ny) == g(self, x, y) {
                    x = nx;
                    y = ny;
                }
            }
        }
        if unsieged {
            if self.units[u as usize].order_queue_head == NIL {
                self.queue_order_at(d, u, order, (x as i16, y as i16), false);
                self.order_done_or_next(d, u);
            }
        } else {
            self.order_at(d, u, order, (x as i16, y as i16));
        }
    }

    /// 0x442460 with 0x4412a0: an enemy within 0x200 that sieged tank `u` sees, can
    /// attack and can hit from where it stands.
    fn sieged_tank_has_enemy(&mut self, d: &Data, u: u32) -> bool {
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x200), y.wrapping_sub(0x200), x.wrapping_add(0x200), y.wrapping_add(0x200)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let p = self.units[u as usize].player;
        found.into_iter().any(|c| {
            if c == u || !self.enemy_of(p, c) {
                return false;
            }
            let cu = &self.units[c as usize];
            if cu.status & 0x300 != 0 && cu.u32_at(0xe4) & (1 << p) == 0 {
                return false;
            }
            self.can_attack(d, u, c, true) && self.ai_can_hit_now(d, u, c)
        })
    }

    /// 0x43c980: military unit `u` fights where it is (`order`, issued unless it has a
    /// target; burrowed units come up first and queue it); an attack-moving unit's AI
    /// remembers the region it fights in (+0xa).
    fn ai_attack_order(&mut self, d: &Data, u: u32, order: u8) {
        let un = &self.units[u as usize];
        if un.status & 0x10 != 0 {
            if un.unit_type == 0x67 && un.order_target_unit != NIL {
                return;
            }
            self.unburrow(d, u);
            if self.units[u as usize].order_queue_head == NIL {
                self.issue_order(d, u, order, false, (0, 0), NIL, 0xe4);
            }
        } else if un.order_target_unit == NIL {
            self.order_now(d, u, order);
        }
        if order == 0x9e {
            let r = self.units[u as usize].ai;
            if r != NIL && ai_kind(r) == AI_MILITARY && self.ai.military.items[ai_index(r) as usize].parent == u && d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 == 0 {
                let here = self.unit_region(u);
                self.ai.military.items[ai_index(r) as usize].unka = here;
            }
        }
    }

    /// 0x43cc40: the region's best air (+0x1c) and ground (+0x20) targets; its idle
    /// military units that can hit one go fight.
    fn region_pick_targets(&mut self, d: &Data, p: u8, r: u16) {
        let units = self.units_in_region(r);
        let air = self.ai_best_target(d, &units, p, NIL, r, 0);
        self.reg_mut(p, r).air_target = air;
        let units = self.units_in_region(r);
        let ground = self.ai_best_target(d, &units, p, NIL, r, 1);
        self.reg_mut(p, r).ground_target = ground;
        if ground == NIL && air == NIL {
            return;
        }
        for m in self.region_military(p, r) {
            let u = self.ai.military.items[m as usize].parent;
            let ai = self.units[u as usize].ai;
            if ai == NIL || ai_kind(ai) != AI_MILITARY || self.ai.military.items[ai_index(ai) as usize].parent != u {
                continue;
            }
            let un = &self.units[u as usize];
            if d.dat.units.flags[un.unit_type as usize] & 1 != 0 || un.status & 0x3000 != 0 {
                continue;
            }
            let reg = self.reg(p, r);
            let (air, ground) = (reg.air_target, reg.ground_target);
            if un.order_target_unit == ground || un.order_target_unit == air || matches!(un.order, 0xe | 0x9d | 0x9e) {
                continue;
            }
            if self.can_carry(d, u) {
                continue;
            }
            let fight = (ground != NIL && self.can_attack(d, u, ground, true)) || (air != NIL && self.can_attack(d, u, air, true));
            if fight {
                self.ai_attack_order(d, u, 0x9c);
            }
        }
    }

    /// 0x431990: the best target among `units` for player `p` (by `attacker`, or for a
    /// region when NIL), scored on strength `kind` (0 air, 1 ground).
    fn ai_best_target(&mut self, d: &Data, units: &[u32], p: u8, attacker: u32, region: u16, kind: usize) -> u32 {
        let (atype, here, ai) = if attacker != NIL {
            (self.units[attacker as usize].unit_type, self.unit_region(attacker), self.units[attacker as usize].ai)
        } else {
            (0xe4, region, NIL)
        };
        let mut best = NIL;
        let mut best_score = -1i32;
        for &c in units {
            if !self.ai_valid_target(d, p, c) {
                continue;
            }
            if attacker != NIL && !self.can_attack(d, attacker, c, true) {
                continue;
            }
            let ct = self.units[c as usize].unit_type;
            if is_minor_target(ct) && self.campaign_ai(p) {
                continue;
            }
            if attacker != NIL {
                let au = &self.units[attacker as usize];
                if self.player_type(au.player) == 1 && au.status & 4 == 0 && !self.ai_can_hit_now(d, attacker, c) && self.units[c as usize].status & 0x80 != 0 {
                    continue;
                }
            }
            if ai != NIL && ai_kind(ai) == super::ai_town::AI_GUARD {
                let home = self.ai.guards.items[ai_index(ai) as usize].home;
                let (hx, hy) = (home.0 as i32, home.1 as i32);
                if self.unit_region(c) != region_at(&self.sai, hx, hy) {
                    let ex = self.units[c as usize].fl.exact;
                    if approx_distance(ex.0, hx << 8, ex.1, hy << 8) & 0xffff_ff00 > 0xc000 {
                        continue;
                    }
                    // 0x401240
                    let ax = self.units[attacker as usize].fl.exact;
                    if approx_distance(ax.0, hx << 8, ax.1, hy << 8) >> 8 > 0xc0 {
                        continue;
                    }
                }
            } else if self.unit_region(c) != here && (attacker == NIL || !self.box_distance_within(d, attacker, 0x120, c)) {
                continue;
            }
            let mut score = self.ai_target_score(d, c, kind) as i32;
            if atype == 8 && self.is_active_detector(d, c) && self.units[c as usize].status & 0x2_0000 != 0 {
                score += 1000;
            }
            if score > best_score {
                best_score = score;
                best = c;
            }
        }
        best
    }

    /// 0x431550: `c` is something player `p` would go for: an enemy (or a minor target),
    /// not invincible, not cloaked unless detected, not a damaged hallucination.
    fn ai_valid_target(&self, d: &Data, p: u8, c: u32) -> bool {
        let cu = &self.units[c as usize];
        if !self.enemy_of(p, c) && !is_minor_target(cu.unit_type) {
            return false;
        }
        if cu.status & 0x400_0000 != 0 {
            return false;
        }
        if cu.status & 0x300 != 0 && cu.u32_at(0xe4) & (1 << p) == 0 {
            return false;
        }
        if cu.status & 0x4000_0000 != 0 && self.life_now(d, c) < self.life_max(d, c) {
            return false;
        }
        true
    }

    /// 0x431730: how much the AI wants target `c` (strength `kind` per hit point, *128;
    /// eggs and the like 0, minor targets 1).
    fn ai_target_score(&self, d: &Data, c: u32, kind: usize) -> u32 {
        let ct = self.units[c as usize].unit_type;
        if matches!(ct, 0x23 | 0x24 | 0x3b | 0x61) {
            return 0;
        }
        let st = self.units[c as usize].status;
        if st & 0x4000_0000 != 0 && self.life_now(d, c) < self.life_max(d, c) {
            return 0;
        }
        if is_minor_target(ct) {
            return 1;
        }
        let base = self.base_strength(d, c, kind);
        // 0x4315e0
        let un = &self.units[c as usize];
        let s = match ct {
            1 if (0x7b..0x82).contains(&un.order) => 1000,
            2 => base.wrapping_add(un.u8_at(0xc0) as u32 * 100),
            0xb | 0x2a | 0x39 | 0x45 => base.wrapping_add(self.loaded_space(d, c).wrapping_mul(100)),
            0x1e => base.wrapping_add(base >> 2),
            0x22 => base.wrapping_add(d.strength[kind][0]),
            0x54 => base.wrapping_add(0x32),
            _ => base,
        };
        let life = self.life_now(d, c).max(0) as u32;
        let life = if life == 0 { 1 } else { life };
        let score = s.wrapping_shl(7) / life;
        if score != 0 {
            score
        } else {
            (st & 0x2_0000 != 0) as u32 + 1
        }
    }

    /// 0x403430: `c` is a working detector (finished, not disabled).
    fn is_active_detector(&self, d: &Data, c: u32) -> bool {
        let cu = &self.units[c as usize];
        d.dat.units.flags[cu.unit_type as usize] & 0x8000 != 0
            && cu.status & 1 != 0
            && cu.status & 0x400 == 0
            && cu.lockdown_timer == 0
            && cu.stasis_timer == 0
            && cu.maelstrom_timer == 0
            && cu.u8_at(0x123) == 0
    }

    /// 0x476870: `u` can hit `c` from where it is (visible, in range and not inside
    /// its weapon's minimum range).
    fn ai_can_hit_now(&self, d: &Data, u: u32, c: u32) -> bool {
        let c = if c != NIL { c } else { self.units[u as usize].order_target_unit };
        if c == NIL {
            return true;
        }
        if !self.target_visible(c, u) {
            return false;
        }
        let w = self.weapon_unit(d, u);
        let weapon = self.weapon_against(d, u, c);
        if weapon >= 0x82 {
            return false;
        }
        let min = d.dat.weapons.min_range[weapon as usize];
        if min != 0 && self.box_distance_within(d, w, min, c) {
            return false;
        }
        let r = self.weapon_range(d, u, weapon);
        self.in_attack_range(d, w, r, c)
    }
}

impl State {
    /// 0x43ee10: states 1 / 2, gathering: with no units left the region goes idle;
    /// 20 ground units make it state 2; else it waits for its slowest unit, holds
    /// against what is there (0x43a790), then moves everyone at the nearest enemy
    /// (0x43a510), whose region takes over the state.
    fn region_gather(&mut self, d: &Data, p: u8, r: u16) {
        self.region_pick_targets(d, p, r);
        let reg = self.reg(p, r);
        if reg.military == NIL {
            self.set_ai_region_state(d, p, r, 0);
            return;
        }
        if reg.state != 2 && reg.ground_unit_count >= 0x14 {
            self.set_ai_region_state(d, p, r, 2);
            return;
        }
        if self.reg(p, r).slowest_military == NIL {
            self.region_pick_slowest(p, r);
        }
        let reg = self.reg(p, r);
        if (reg.air_target != NIL || reg.ground_target != NIL) && !self.region_cannot_fight_there(d, p, r, r) {
            return;
        }
        let t = self.region_find_enemy(d, p, r);
        if t != NIL {
            let to = self.unit_region(t);
            let state = self.reg(p, r).state;
            self.set_ai_region_state(d, p, to, state);
            self.region_move_units(d, p, r, to, false, false, true);
        }
        self.set_ai_region_state(d, p, r, 0);
    }

    /// 0x43d350: state 3, lost: unless a neighbour already sends help, the neighbour
    /// with the second most tiles (else the most) that is free and safely reachable
    /// from the first town is asked to (state 7, +2 = this region).
    fn region_lost(&mut self, d: &Data, p: u8, r: u16) {
        self.region_recount(d, p, r);
        self.region_pick_targets(d, p, r);
        let reg = self.reg(p, r);
        if reg.air_target == NIL && reg.ground_target == NIL {
            self.region_decay_needs(p, r);
        }
        let sreg = &self.sai.regions[r as usize];
        let n = if sreg.flags == 0x1ffd {
            (sreg.neighbor_count as i32 - sreg.zero_neighbors as i8 as i32) as u32
        } else {
            sreg.walkable_neighbors as u32
        };
        let nbs: Vec<u16> = (0..n as usize).map(|i| self.sai.neighbor(r as usize, i)).collect();
        if nbs.iter().any(|&nb| self.reg(p, nb).state == 7 && self.reg(p, nb).target_region_id == r) {
            return;
        }
        let (mut best1, mut best2) = (0u32, 0u32);
        let (mut cand1, mut cand2) = (None, None);
        for &nb in &nbs {
            let nr = self.reg(p, nb);
            if nr.air_target != NIL || nr.ground_target != NIL {
                continue;
            }
            if nr.state == 0 && self.campaign_ai(nr.player) {
                continue;
            }
            if !matches!(nr.state, 5 | 4 | 0) {
                continue;
            }
            let town = self.ai.town_lists[p as usize];
            if town != NIL {
                let (tx, ty) = self.ai.towns.items[town as usize].position;
                let from = region_at(&self.sai, tx as i32, ty as i32);
                if !self.ai_path_safe(from, nb, p) {
                    continue;
                }
            }
            let tiles = self.sai.regions[nb as usize].tile_count as u32;
            if tiles > best1 {
                best1 = tiles;
                cand1 = Some(nb);
            } else if tiles > best2 {
                best2 = tiles;
                cand2 = Some(nb);
            }
        }
        let Some(pick) = cand2.or(cand1) else { return };
        self.reg_mut(p, pick).target_region_id = r;
        self.set_ai_region_state(d, p, pick, 7);
    }

    /// 0x443fb0: the region path from `a`'s centre to `b`'s goes through no region
    /// that is lost, has targets or flag 0x20 for `player` (different groups count as
    /// safe).
    fn ai_path_safe(&mut self, a: u16, b: u16, player: u8) -> bool {
        if a == b || self.sai.regions[a as usize].group != self.sai.regions[b as usize].group {
            return true;
        }
        let ca = self.region_center(a);
        let cb = self.region_center(b);
        let (pa, pb) = ((ca.0 as i16, ca.1 as i16), (cb.0 as i16, cb.1 as i16));
        let mut ctx = super::path::PathCtx { unit: NIL, start: pa, target: pb, goal: pb, ..Default::default() };
        ctx.start_region = a;
        ctx.target_region = b;
        let mut sai = std::mem::replace(&mut self.sai, super::sai::Sai::empty());
        let ok = super::path::find_region_path(&mut sai, &mut ctx);
        self.sai = sai;
        if ok && ctx.region_count > 2 {
            let n = ctx.region_count as i32 - 1;
            for i in 1..n.max(1) as usize {
                let reg = self.reg(player, ctx.regions[i]);
                if reg.state == 3 || reg.air_target != NIL || reg.ground_target != NIL || reg.flags & 0x20 != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// 0x43f020: state 7, sending help to region +2: (campaign regions may give up
    /// first) once there is a real target here it defends (6); after 10 rounds, or
    /// when strong enough for the region it helps, that one defends (6) with all its
    /// units (0x43e580) and this one holds (5, timer 5).
    fn region_send_help(&mut self, d: &Data, p: u8, r: u16) {
        self.region_recount(d, p, r);
        self.region_pick_targets(d, p, r);
        if self.reg(p, r).flags & 0x20 != 0 && self.region_give_up(d, p, r) {
            return;
        }
        let to = self.reg(p, r).target_region_id;
        let reg = self.reg(p, r);
        let treg = self.reg(p, to);
        if reg.defense_priority_add != 0x32 && reg.air_target == NIL && reg.ground_target == NIL && treg.air_target == NIL && treg.ground_target == NIL {
            self.reg_mut(p, r).defense_priority_add = 0x32;
        }
        if self.region_has_real_target(p, r) {
            self.set_ai_region_state(d, p, r, 6);
            return;
        }
        if self.reg(p, r).unk < 10 {
            self.reg_mut(p, r).unk += 1;
            let reg = self.reg(p, r);
            let treg = self.reg(p, to);
            if reg.local_military_air_strength < treg.needed_air_strength || reg.local_military_ground_strength < treg.needed_ground_strength {
                return;
            }
        }
        self.set_ai_region_state(d, p, to, 6);
        self.region_move_units(d, p, r, to, true, true, true);
        self.set_ai_region_state(d, p, r, 5);
        self.reg_mut(p, to).unk_count = 5;
    }

    /// 0x43eec0: states 8 / 9, attacking: (campaign regions may give up first) units
    /// that can't fight here (or ground units on an island) end the attack: the
    /// nearest enemy's region takes it over (9), else the units go home (0x43dd20);
    /// otherwise, once the attack force is complete and nobody is still marching,
    /// the attack moves on to region +2 (state 1).
    fn region_attack(&mut self, d: &Data, p: u8, r: u16) {
        self.region_recount(d, p, r);
        self.region_pick_targets(d, p, r);
        if self.reg(p, r).flags & 0x20 != 0 && self.region_give_up(d, p, r) {
            return;
        }
        let w = self.region_weapons(d, p, r);
        if (w.any_ground && self.sai.regions[r as usize].flags == 0x1ffd) || self.region_cannot_fight_there(d, p, r, r) {
            self.set_ai_region_state(d, p, r, 0);
            let t = self.region_find_enemy(d, p, r);
            if t != NIL {
                let to = self.unit_region(t);
                self.set_ai_region_state(d, p, to, 9);
                self.region_move_units(d, p, r, to, false, false, true);
            } else {
                self.region_disband(d, p, r);
            }
            return;
        }
        if !self.attack_timer_expired(p) {
            let missing = self.attack_force_missing(d, p, r, true);
            if missing.iter().any(|&t| t < 0xe4) {
                return;
            }
            if self.units_still_marching(p) {
                return;
            }
        }
        let to = self.reg(p, r).target_region_id;
        if to != 0 {
            self.set_ai_region_state(d, p, to, 1);
            self.region_move_units(d, p, r, to, false, false, true);
        }
        self.set_ai_region_state(d, p, r, 0);
    }

    /// 0x438fc0: whether the region's units include ground units, and whether none of
    /// them has a ground (or air) weapon.
    fn region_weapons(&self, d: &Data, p: u8, r: u16) -> RegionWeapons {
        let mut w = RegionWeapons { any_ground: false, no_ground_weapon: true, no_air_weapon: true };
        for m in self.region_military(p, r) {
            let u = self.ai.military.items[m as usize].parent;
            let un = &self.units[u as usize];
            let t = un.unit_type as usize;
            if (un.unit_type != 0x67 || un.status & 0x10 != 0) && d.dat.units.ground_weapon[t] != 0x82 {
                w.no_ground_weapon = false;
            }
            let sub = un.subunit;
            if d.dat.units.air_weapon[t] != 0x82 || (sub != NIL && d.dat.units.air_weapon[self.units[sub as usize].unit_type as usize] != 0x82) {
                w.no_air_weapon = false;
            }
            if un.status & 4 == 0 {
                w.any_ground = true;
            }
        }
        w
    }

    /// 0x43a790: the units of region `r` can't take on what is in region `at`: ground
    /// units without transports whose way there is blocked, or (when not armed for
    /// both) an enemy there they can't hit. (Same region: false.)
    fn region_cannot_fight_there(&mut self, d: &Data, p: u8, r: u16, at: u16) -> bool {
        let w = self.region_weapons(d, p, at);
        if w.any_ground && !self.has_transports(p) {
            let head = self.reg(p, r).military;
            if head == NIL {
                return true;
            }
            let u = self.ai.military.items[head as usize].parent;
            if self.units[u as usize].pathing_flags & 1 != 0 {
                let (cx, cy) = self.region_center(at);
                if !self.same_group(u, (cx as i16, cy as i16)) {
                    return true;
                }
            }
        }
        if (!w.no_air_weapon && !w.no_ground_weapon) || r == at {
            return false;
        }
        for c in self.units_in_region(at) {
            if !self.enemy_of(p, c) {
                continue;
            }
            let flying = self.units[c as usize].status & 4 != 0;
            if (!flying && w.no_air_weapon) || (flying && w.no_ground_weapon) {
                return false;
            }
        }
        true
    }

    /// 0x437250: `player` has a shuttle or dropship, or overlords that can carry.
    fn has_transports(&self, p: u8) -> bool {
        let c = &self.players.completed_counts;
        let p = p as usize;
        c[0x45][p] != 0 || c[0xb][p] != 0 || (self.players.upgrade_sc[p][0x18] != 0 && c[0x2a][p] != 0)
    }

    /// 0x43a510: the enemy the region's units should go for: its nearest building,
    /// else nearest unit (from the region centre, ones they can hit, reachable for
    /// ground forces), among enemies standing in regions idle for this AI; else the
    /// first enemy seen in a busy region (if reachable).
    fn region_find_enemy(&mut self, d: &Data, p: u8, r: u16) -> u32 {
        let w = self.region_weapons(d, p, r);
        let head = self.reg(p, r).military;
        if head == NIL {
            return NIL;
        }
        let first = self.ai.military.items[head as usize].parent;
        let (cx, cy) = self.region_center(r);
        let (mut best_b, mut best_u) = ((NIL, 9_999_999u32), (NIL, 9_999_999u32));
        let mut fallback = NIL;
        let mut u = self.visible_units.head;
        let reachable = |st: &State, c: u32| -> bool {
            let cr = st.unit_region(c);
            if st.sai.regions[cr as usize].flags == 0x1ffd {
                return false;
            }
            st.has_transports(p) || st.can_reach_spot(first, st.unit_sprite_pos(c))
        };
        while u != NIL {
            let next = self.units[u as usize].link.next;
            let un = &self.units[u as usize];
            if un.sprite == NIL || un.order == 0 || !self.enemy_of(p, u) {
                u = next;
                continue;
            }
            let ur = self.unit_region(u);
            if self.reg(p, ur).state != 0 {
                fallback = u;
                u = next;
                continue;
            }
            let t = un.unit_type;
            if (0xc2..=0xc7).contains(&t) {
                u = next;
                continue;
            }
            if fallback == NIL {
                fallback = u;
            }
            let ok = if !w.any_ground { !matches!(t, 0x7c | 0xa2 | 0x90 | 0x7d) } else { reachable(self, u) };
            if ok {
                let un = &self.units[u as usize];
                let cant = if un.status & 4 != 0 { w.no_air_weapon } else { w.no_ground_weapon };
                if !cant {
                    let (ux, uy) = self.unit_sprite_pos(u);
                    let dist = approx_distance(cx, ux as i32, cy, uy as i32);
                    if un.status & 2 != 0 {
                        if dist < best_b.1 {
                            best_b = (u, dist);
                        }
                    } else if dist < best_u.1 {
                        best_u = (u, dist);
                    }
                }
            }
            u = next;
        }
        if best_b.0 != NIL {
            return best_b.0;
        }
        if best_u.0 != NIL {
            return best_u.0;
        }
        if !w.any_ground || fallback == NIL {
            return fallback;
        }
        if reachable(self, fallback) { fallback } else { NIL }
    }

    /// 0x43e580: region `from` hands its units to `to` (and its detector if `to` has
    /// none); detector types stay unless `all`; units in transports stay when
    /// `keep_loaded` (melee AI only); with `call_guards` guards near `to` join too.
    #[allow(clippy::too_many_arguments)]
    fn region_move_units(&mut self, d: &Data, p: u8, from: u16, to: u16, call_guards: bool, all: bool, keep_loaded: bool) {
        if self.reg(p, to).detector == NIL {
            let det = self.reg(p, from).detector;
            self.reg_mut(p, to).detector = det;
            self.reg_mut(p, from).detector = NIL;
        }
        let mut m = self.reg(p, from).military;
        while m != NIL && self.reg(p, from).military != NIL {
            let next = self.ai.military.items[m as usize].link.next;
            let u = self.ai.military.items[m as usize].parent;
            let un = &self.units[u as usize];
            let stay = (!all && is_detector_type(un.unit_type)) || (un.status & 0x20 != 0 && keep_loaded && !self.campaign_ai(un.player));
            if !stay {
                self.region_remove_military(p, from, m);
                self.units[u as usize].ai = NIL;
                self.region_add_military(d, p, to, u, true);
            }
            m = next;
        }
        if call_guards {
            // 0x4e8280 with 0x43dde0
            let (cx, cy) = self.region_center(to);
            let b = [(cx - 0x100) as i16, (cy - 0x100) as i16, (cx + 0x100) as i16, (cy + 0x100) as i16];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, b);
            let found = search.results(start);
            search.pop();
            self.search = search;
            for g in found {
                let ai = self.units[g as usize].ai;
                if ai != NIL && ai_kind(ai) == super::ai_town::AI_GUARD && self.units[g as usize].status & 0x20 == 0 && self.units[g as usize].player == p && self.unit_region(g) == to {
                    self.ai_release_unit(d, g, false);
                    self.region_add_military(d, p, to, g, true);
                }
            }
        }
    }

    /// 0x43dd20: every unit of the region goes to the region it stands in.
    fn region_disband(&mut self, d: &Data, p: u8, r: u16) {
        let mut m = self.reg(p, r).military;
        while m != NIL && self.reg(p, r).military != NIL {
            let next = self.ai.military.items[m as usize].link.next;
            let u = self.ai.military.items[m as usize].parent;
            self.region_remove_military(p, r, m);
            self.units[u as usize].ai = NIL;
            let up = self.units[u as usize].player;
            let at = self.unit_region(u);
            self.region_add_military(d, up, at, u, true);
            m = next;
        }
    }

    /// 0x447090: the attack started at +0x228 has run its time (2 minutes, 3 for a
    /// campaign AI); the timer is cleared then.
    fn attack_timer_expired(&mut self, p: u8) -> bool {
        let pa = &self.ai.players[p as usize];
        let t = pa.last_attack_second;
        if t == 0 {
            return false;
        }
        let limit = if pa.flags & 0x20 != 0 { 0xb4 } else { 0x78 };
        if self.trig.elapsed.wrapping_sub(t) >= limit {
            self.ai.players[p as usize].last_attack_second = 0;
            return true;
        }
        false
    }

    /// 0x438050: the attack force (+0x230) less the units region `r` has (and, unless
    /// `finished_only`, the ones being made; eggs count as what they become): 0xe4
    /// marks a slot that is filled.
    fn attack_force_missing(&self, d: &Data, p: u8, r: u16, finished_only: bool) -> Vec<u32> {
        let mut list: Vec<u32> = Vec::new();
        for &e in self.ai.players[p as usize].attack_force.iter() {
            if e == 0 {
                break;
            }
            list.push(e as u32 - 1);
        }
        let mut n = list.len();
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            let next = un.player_next;
            if un.sprite == NIL || un.order == 0 || d.dat.units.flags[un.unit_type as usize] & 0x10 != 0 {
                u = next;
                continue;
            }
            let mut t = if un.unit_type == 0x1e { 5 } else { un.unit_type as u32 };
            let mut k = 1;
            let done = un.status & 1 != 0 && un.order != 0x6a && un.order != 0x96;
            if !done {
                if finished_only {
                    u = next;
                    continue;
                }
                if matches!(t, 0x24 | 0x3b | 0x61) {
                    t = un.build_queue[un.build_queue_slot as usize] as u32;
                    if d.dat.units.flags[t as usize] & 0x400 != 0 {
                        k = 2;
                    }
                }
            }
            if d.dat.units.ai_internal[t as usize] & 1 != 0 {
                u = next;
                continue;
            }
            if done {
                let ai = un.ai;
                if ai == NIL || ai_kind(ai) != AI_MILITARY || self.ai.military.items[ai_index(ai) as usize].region != (p, r) {
                    u = next;
                    continue;
                }
            }
            let mut last = n as isize - 1;
            for _ in 0..k {
                if let Some(i) = list[..n].iter().position(|&e| e == t) {
                    list[i] = 0xe4;
                    if i as isize == last {
                        n -= 1;
                        last -= 1;
                    }
                }
            }
            u = next;
        }
        list.truncate(n);
        list
    }

    /// 0x43a2e0: one of `player`'s attacking units is still on its way (no enemy
    /// target, no auto target, not in its region).
    fn units_still_marching(&self, p: u8) -> bool {
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && un.ai != NIL && ai_kind(un.ai) == AI_MILITARY {
                let (rp, rid) = self.ai.military.items[ai_index(un.ai) as usize].region;
                if matches!(self.reg(rp, rid).state, 8 | 9) {
                    let t = un.order_target_unit;
                    let fighting = t != NIL && self.enemy_of(un.player, t);
                    if !fighting && un.auto_target == NIL && self.unit_region(u) != rid {
                        return true;
                    }
                }
            }
            u = un.player_next;
        }
        false
    }

    /// 0x437e70: `player`'s units can get from region `a` to `b`: neighbours, an air
    /// target at `a` that can be hit from the air, else the 10-frame flood (0x437070)
    /// through regions without enemies reaches `b`. Other groups: no; non-players: yes.
    pub(super) fn ai_region_reachable(&mut self, d: &Data, player: u8, a: u16, b: u16) -> bool {
        if player >= 8 || a == b {
            return true;
        }
        if self.sai.regions[a as usize].group != self.sai.regions[b as usize].group {
            return false;
        }
        // 0x437180
        let ra = &self.sai.regions[a as usize];
        let n = ra.neighbor_count as i32 - ra.zero_neighbors as i8 as i32;
        if (0..n.max(0) as usize).any(|i| self.sai.neighbor(a as usize, i) == b) {
            return true;
        }
        let p = player as usize;
        // The AI regions are all zero until a game sets them up (0x437a70).
        let at = self.ai.regions[p].get(a as usize).map_or(NIL, |r| r.air_target);
        if at != NIL && self.air_weapon_of(d, at) != 0x82 {
            return true;
        }
        if self.trig.elapsed >= self.ai.flood_time[p].wrapping_add(10) || self.ai.flood_start[p] != a as u32 {
            self.ai_region_flood(player, a);
        }
        self.ai.flood[p][b as usize] == 2
    }

    /// 0x475840: air weapon of `u` (or its turret).
    fn air_weapon_of(&self, d: &Data, u: u32) -> u8 {
        let w = d.dat.units.air_weapon[self.units[u as usize].unit_type as usize];
        if w != 0x82 {
            return w;
        }
        let s = self.units[u as usize].subunit;
        if s == NIL {
            return 0x82;
        }
        d.dat.units.air_weapon[self.units[s as usize].unit_type as usize]
    }

    /// 0x437070: from region `start` through walkable neighbours; regions with targets
    /// for `player`'s AI are reached but not passed (2 = reached).
    fn ai_region_flood(&mut self, player: u8, start: u16) {
        let p = player as usize;
        self.ai.flood_time[p] = self.trig.elapsed;
        self.ai.flood_start[p] = start as u32;
        let map = &mut self.ai.flood[p];
        map.iter_mut().for_each(|b| *b = 0);
        map[start as usize] = 1;
        let count = self.sai.region_count as usize;
        if count < 2 {
            return;
        }
        loop {
            let mut grew = false;
            for r in 1..count {
                if self.ai.flood[p][r] != 1 {
                    continue;
                }
                self.ai.flood[p][r] = 2;
                for i in 0..self.sai.regions[r].walkable_neighbors as usize {
                    let nb = self.sai.neighbor(r, i) as usize;
                    if self.ai.flood[p][nb] == 0 {
                        let busy = self.ai.regions[p].get(nb).is_some_and(|ar| ar.air_target != NIL || ar.ground_target != NIL);
                        self.ai.flood[p][nb] = if busy { 2 } else { 1 };
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }
    }
}

/// 0x438fc0's three answers.
struct RegionWeapons {
    any_ground: bool,
    no_ground_weapon: bool,
    no_air_weapon: bool,
}

/// 0x437000: science vessels, overlords, observers and the like stay out of fights.
fn is_detector_type(t: u16) -> bool {
    matches!(t, 9 | 0x2d | 0x2e | 0x43)
}

/// 0x436980: a melee AI's (`fast`) needs drop by 300; a campaign AI's slowly.
fn decay(v: u16, fast: bool) -> u16 {
    if !fast {
        if v > 1000 {
            return v - 0xd;
        }
        if v > 200 {
            return v - 6;
        }
        if v > 2 {
            return v - 2;
        }
    } else if v > 300 {
        return v - 300;
    }
    0
}
