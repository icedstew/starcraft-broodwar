//! AI spellcasting: the per-unit-type spell choice 0x4a13c0 with its target search
//! wrappers (0x440bb0 .. 0x443220) and their predicates (0x4407e0 .. 0x4428d0).
//!
//! The wrappers search a box around the caster with 0x4e8740 (or the whole map);
//! `flag` is the wrapper's EAX/EBX argument (0 = look far and take what is worth it).

use super::list::NIL;
use super::{Data, State};

/// Search box of radius `r` around (x, y) as 0x4e82e0 / the wrappers build it.
fn around(x: i16, y: i16, r: i32) -> [i16; 4] {
    [(x as i32 - r) as i16, (y as i32 - r) as i16, (x as i32 + r) as i16, (y as i32 + r) as i16]
}

impl State {
    fn ai_flag20(&self, player: u8) -> bool {
        self.ai.players[player as usize].flags & 0x20 != 0
    }

    /// 0x6284a0: the whole map as a search box.
    fn map_box(&self) -> [i16; 4] {
        [0, 0, self.map_px.0 as i16, self.map_px.1 as i16]
    }

    /// 0x4e87e0 (box part): the box the wrappers build around `u`.
    fn caster_box(&self, u: u32, r: i32) -> [i16; 4] {
        let (x, y) = self.unit_sprite_pos(u);
        around(x, y, r)
    }

    /// Wrapper radius: `flag` set = 0x120, else `base` (doubled for an AI player with
    /// flag 0x20).
    fn spell_radius(&self, u: u32, flag: u32, base: i32) -> i32 {
        if flag != 0 {
            0x120
        } else if self.ai_flag20(self.units[u as usize].player) {
            base * 2
        } else {
            base
        }
    }

    /// 0x4e8280 over the box of radius `r` around `c` (what 0x4e82e0 does): the units in it.
    fn spell_units_near(&mut self, c: u32, r: i32) -> Vec<u32> {
        let b = self.caster_box(c, r);
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
    }

    /// 0x446e50: `player`'s nuke timer (minutes) has run out.
    fn nuke_ready(&self, player: u8) -> bool {
        let p = &self.ai.players[player as usize];
        p.nuke_rate_minutes as u32 * 0x3c + p.previous_nuke_timer <= self.trig.elapsed
    }

    /// 0x447810: `u` is its player's free medic.
    fn is_free_medic(&self, u: u32) -> bool {
        u == self.ai.players[self.units[u as usize].player as usize].free_medic
    }

    /// 0x463360: the first of `u`'s player's silos holding a ready nuke that no other
    /// ghost has claimed (0x463360 with EDI = 1).
    fn find_free_silo(&self, u: u32) -> u32 {
        let mut s = self.player_units[self.units[u as usize].player as usize];
        while s != NIL {
            let su = &self.units[s as usize];
            if su.unit_type == 0x6c && su.u32_at(0xd4) != 0 {
                let nuke = su.ptr_at(0xd0);
                let owner = self.units[nuke as usize].connected_unit;
                if owner == NIL || owner == u || self.units[owner as usize].unit_type == 0x6c {
                    return s;
                }
            }
            s = su.player_next;
        }
        NIL
    }

    /// 0x49e1c0: energy (fp8) of the tech `order` casts.
    pub(super) fn order_energy_cost(&self, d: &Data, order: u8) -> u32 {
        let t = d.dat.orders.tech[order as usize];
        if t < 0x2c { (d.dat.techdata.energy_cost[t as usize] as u32) << 8 } else { 0 }
    }

    /// 0x4a11e0: `u` has the energy for `order` (any with the energy cheat) and may use
    /// tech `tech`.
    fn spell_ready(&self, d: &Data, u: u32, order: u8, tech: u16) -> bool {
        let cost = self.order_energy_cost(d, order) as u16;
        if self.game_options & 0x800 == 0 && self.units[u as usize].energy < cost {
            return false;
        }
        self.can_use_tech(d, u, tech, self.units[u as usize].player) == Ok(1)
    }

    /// 0x4a1290: `u` casts `order` on `t` unless `t` is none or already marked with `mask`
    /// in its +0x92 flags; the mark is set when the order is given.
    fn cast_on(&mut self, d: &Data, u: u32, order: u8, mask: u8, t: u32) -> bool {
        if t != NIL && self.units[t as usize].user_action_flags & mask == 0 {
            if self.units[u as usize].order != order {
                self.order_with_target(d, u, order, t);
                self.units[t as usize].user_action_flags |= mask;
            }
            return true;
        }
        false
    }

    /// `u` already casts `order` on `t` (the check before 0x4a1290).
    fn casting(&self, u: u32, order: u8, t: u32) -> bool {
        self.units[u as usize].order == order && self.units[u as usize].order_target_unit == t
    }

    /// 0x4a13c0: an AI unit casts a spell it can use: per unit type the spells in priority
    /// order, each on the target its wrapper picks. `flag` 0 also stops (false) while the
    /// player's spell cooldown (+0x115) is running. True when an order was given.
    pub(super) fn ai_cast_spell(&mut self, d: &Data, u: u32, flag: u32) -> bool {
        let p = self.units[u as usize].player;
        if flag == 0 && self.ai.players[p as usize].spell_cooldown != 0 {
            return false;
        }
        match self.units[u as usize].unit_type {
            1 => {
                if self.spell_ready(d, u, 0x73, 1) {
                    let t = self.ai_lockdown_target(d, u, flag);
                    if self.casting(u, 0x73, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x73, 1, t) {
                        return true;
                    }
                }
                if self.nuke_ready(p) {
                    let silo = self.find_free_silo(u);
                    if silo != NIL {
                        let t = self.ai_nuke_target(d, u);
                        if self.cast_on(d, u, 0x7e, 1, t) {
                            let nuke = self.units[silo as usize].ptr_at(0xd0);
                            self.units[nuke as usize].connected_unit = u;
                            self.ai.players[p as usize].previous_nuke_timer = self.trig.elapsed;
                            return true;
                        }
                    }
                }
            }
            9 => {
                if self.spell_ready(d, u, 0x7a, 2) {
                    let t = self.ai_emp_target(d, u, flag);
                    if self.casting(u, 0x7a, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x7a, 1, t) {
                        return true;
                    }
                }
                if self.spell_ready(d, u, 0x8d, 6) {
                    let t = self.ai_matrix_target(d, u, flag);
                    if self.casting(u, 0x8d, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x8d, 1, t) {
                        return true;
                    }
                }
                if self.spell_ready(d, u, 0x8f, 7) {
                    let t = self.ai_irradiate_target(d, u, flag);
                    if self.casting(u, 0x8f, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x8f, 1, t) {
                        return true;
                    }
                }
            }
            0xc => {
                if self.spell_ready(d, u, 0x71, 8) {
                    let t = self.ai_yamato_target(d, u, flag);
                    if self.casting(u, 0x71, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x71, 1, t) {
                        return true;
                    }
                }
            }
            0x22 => {
                if self.spell_ready(d, u, 0xb4, 0x18) {
                    let t = self.ai_restoration_target(d, u);
                    if self.casting(u, 0xb4, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0xb4, 1, t) {
                        return true;
                    }
                    if self.is_free_medic(u) && self.units[u as usize].order != 0xb4 {
                        let t = self.ai_restoration_own_target(d, u, flag);
                        if self.cast_on(d, u, 0xb4, 1, t) {
                            return true;
                        }
                    }
                }
                if self.units[u as usize].energy >> 8 == (self.max_energy(d, u) as i32 >> 8) as u16
                    && self.spell_ready(d, u, 0xb9, 0x1e)
                {
                    let t = self.ai_flare_target(d, u, flag);
                    if self.casting(u, 0xb9, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0xb9, 1, t);
                }
            }
            0x2d => {
                let f20 = self.ai_flag20(p);
                let full = self.units[u as usize].energy >> 8 == (self.max_energy(d, u) as i32 >> 8) as u16;
                let to_parasite = if !f20 { full } else { self.rng.rand(0x2a) as u8 == 1 };
                if to_parasite {
                    if self.spell_ready(d, u, 0x78, 0x12) {
                        let t = self.ai_parasite_target(d, u, flag);
                        if self.casting(u, 0x78, t) {
                            return false;
                        }
                        return self.cast_on(d, u, 0x78, 4, t);
                    }
                    if f20 {
                        return false;
                    }
                }
                if self.spell_ready(d, u, 0x79, 0xd) {
                    let t = self.ai_plague_target(d, u, flag);
                    if self.casting(u, 0x79, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x79, 1, t) {
                        let pos = self.unit_sprite_pos(u);
                        self.queue_order_at(d, u, 6, pos, false);
                        return true;
                    }
                }
                if !f20 && !full {
                    return false;
                }
                if self.spell_ready(d, u, 0x92, 0x11) {
                    let t = self.ai_ensnare_target(d, u, flag);
                    if self.casting(u, 0x92, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0x92, 1, t);
                }
            }
            0x2e => {
                if self.spell_ready(d, u, 0x90, 0xf) {
                    let t = self.ai_consume_target(d, u, flag);
                    if self.casting(u, 0x90, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x90, 1, t) {
                        return true;
                    }
                }
                if self.spell_ready(d, u, 0x77, 0xe) {
                    let t = self.ai_swarm_target(d, u, flag);
                    if self.casting(u, 0x77, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0x77, 1, t);
                }
            }
            0x3c => {
                if self.spell_ready(d, u, 0xb5, 0x19) {
                    let t = self.ai_web_target(d, u, flag);
                    if self.casting(u, 0xb5, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0xb5, 1, t);
                }
            }
            0x3f => {
                if self.spell_ready(d, u, 0xb8, 0x1d) {
                    let t = self.ai_maelstrom_target(d, u, flag);
                    if self.casting(u, 0xb8, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0xb8, 1, t) {
                        return true;
                    }
                }
                if self.spell_ready(d, u, 0xb6, 0x1b) {
                    let t = self.ai_mind_control_target(d, u);
                    if self.casting(u, 0xb6, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0xb6, 1, t) {
                        return true;
                    }
                }
                let sum = (self.order_energy_cost(d, 0xba) as u16 as i32) + (self.order_energy_cost(d, 0xb6) as u16 as i32);
                let max = self.max_energy(d, u) as i32;
                let cost = sum.min(max);
                if self.energy_short(u, cost as u16) || self.can_use_tech(d, u, 0x1f, p) != Ok(1) {
                    return false;
                }
                let t = self.ai_feedback_target(d, u, flag);
                if self.casting(u, 0xba, t) {
                    return false;
                }
                return self.cast_on(d, u, 0xba, 1, t);
            }
            0x43 => {
                if self.spell_ready(d, u, 0x8e, 0x13) {
                    let t = self.ai_storm_target(d, u, flag);
                    if self.casting(u, 0x8e, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x8e, 1, t) {
                        return true;
                    }
                }
                if self.units[u as usize].energy >> 8 == (self.max_energy(d, u) as i32 >> 8) as u16
                    && self.spell_ready(d, u, 0x94, 0x14)
                {
                    let t = self.ai_hallucination_target(d, u, flag);
                    if self.casting(u, 0x94, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0x94, 1, t);
                }
            }
            0x47 => {
                if self.spell_ready(d, u, 0x93, 0x16) {
                    let t = self.ai_stasis_target(d, u, flag);
                    if self.casting(u, 0x93, t) {
                        return false;
                    }
                    if self.cast_on(d, u, 0x93, 1, t) {
                        return true;
                    }
                }
                if self.spell_ready(d, u, 0x89, 0x15) {
                    let t = self.ai_recall_target(u);
                    if self.casting(u, 0x89, t) {
                        return false;
                    }
                    return self.cast_on(d, u, 0x89, 1, t);
                }
            }
            _ => {}
        }
        false
    }

    /// 0x4a12c0: a non-flag-0x20 AI tank standing idle on its spot sieges up.
    pub(super) fn ai_siege_up(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if !self.ai_flag20(un.player)
            && (un.order == 0x9c || un.order == 2)
            && un.fl.move_target == self.unit_sprite_pos(u)
            && self.can_use_tech(d, u, 5, un.player) == Ok(1)
        {
            self.start_order_now(d, u, 0x62);
        }
    }

    /// 0x4a1340: the same for a burrow-capable unit (not a lurker egg 0x29) that is not
    /// burrowed.
    pub(super) fn ai_burrow_down(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x10 == 0
            && un.unit_type != 0x29
            && (un.order == 0x9c || un.order == 2)
            && un.fl.move_target == self.unit_sprite_pos(u)
            && self.can_use_tech(d, u, 0xb, un.player) == Ok(1)
        {
            self.start_order_now(d, u, 0x74);
        }
    }

    // ---- target search wrappers ----

    /// 0x4e87e0 (search part) / 0x4e8740 from `u` over `b` with `ok(candidate)`.
    fn spell_search(&mut self, u: u32, b: [i16; 4], ok: &mut dyn FnMut(&mut State, u32) -> bool) -> u32 {
        self.nearest_unit_to(u, b, ok)
    }

    /// 0x440bb0: Recall: a friendly carrier/reaver/... of `u` in the region it stands in
    /// (not while that region is being attacked or targeted).
    fn ai_recall_target(&mut self, u: u32) -> u32 {
        let (x, y) = self.unit_sprite_pos(u);
        let p = self.units[u as usize].player as usize;
        let r = super::path::region_at(&self.sai, x as i32, y as i32) as usize;
        let rg = &self.ai.regions[p][r];
        if rg.state != 3 && rg.air_target == NIL && rg.ground_target == NIL && rg.flags & 0x20 == 0 {
            let b = self.map_box();
            return self.spell_search(u, b, &mut |st, c| st.pred_recall(c, u));
        }
        NIL
    }

    /// 0x440c10: restoration on an own unit that is parasited or under maelstrom.
    fn ai_restoration_own_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        if flag != 0 {
            return NIL;
        }
        let b = self.map_box();
        self.spell_search(u, b, &mut |st, c| st.pred_restore_own(d, c, u))
    }

    /// 0x440c30: Restoration within 0x400.
    fn ai_restoration_target(&mut self, d: &Data, u: u32) -> u32 {
        let b = self.caster_box(u, 0x400);
        self.spell_search(u, b, &mut |st, c| st.pred_restore(d, c, u))
    }

    /// 0x442530: EMP Shockwave.
    fn ai_emp_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x800 } else { 0x120 };
        let b = self.caster_box(u, r);
        let t = self.spell_search(u, b, &mut |st, c| st.pred_emp_shields(d, c, u));
        if t == NIL && flag == 0 {
            // 0x4e87e0 with the same radius.
            return self.spell_search(u, b, &mut |st, c| st.pred_emp_energy(d, c, u));
        }
        t
    }

    /// 0x442660: Irradiate.
    fn ai_irradiate_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x800 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.ai_spell_flag = flag;
        self.spell_search(u, b, &mut |st, c| st.pred_irradiate(d, c, u))
    }

    /// 0x4426c0: Lockdown.
    fn ai_lockdown_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_lockdown(d, c, u))
    }

    /// 0x442740: the nuke target (any unit of an enemy that is worth it, whole map).
    fn ai_nuke_target(&mut self, d: &Data, u: u32) -> u32 {
        let b = self.map_box();
        self.spell_search(u, b, &mut |st, c| st.pred_nuke(d, c, u))
    }

    /// 0x442760: Ensnare.
    fn ai_ensnare_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_ensnare(d, c, u))
    }

    /// 0x4427e0: Consume.
    fn ai_consume_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x1000 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_consume(d, c, u))
    }

    /// 0x442840: Dark Swarm: the ally in a melee fight (its attacker's target if the
    /// found unit is in the air).
    fn ai_swarm_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        let t = self.spell_search(u, b, &mut |st, c| st.pred_swarm(d, c, u));
        if t == NIL {
            return NIL;
        }
        if self.units[t as usize].status & 4 != 0 {
            return self.units[t as usize].order_target_unit;
        }
        t
    }

    /// 0x442930: Psionic Storm.
    fn ai_storm_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.ai_spell_flag = flag;
        self.spell_search(u, b, &mut |st, c| st.pred_storm(d, c, u))
    }

    /// 0x4429b0: Stasis Field.
    fn ai_stasis_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_stasis(d, c, u))
    }

    /// 0x442a30: Optical Flare.
    fn ai_flare_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x400 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_flare(d, c, u))
    }

    /// 0x442a90: Parasite.
    fn ai_parasite_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x800 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_parasite(d, c, u))
    }

    /// 0x442af0: Plague.
    fn ai_plague_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x800 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.ai_spell_flag = flag;
        self.spell_search(u, b, &mut |st, c| st.pred_plague(d, c, u))
    }

    /// 0x442b50: Mind Control within 0x400.
    fn ai_mind_control_target(&mut self, d: &Data, u: u32) -> u32 {
        let b = self.caster_box(u, 0x400);
        self.spell_search(u, b, &mut |st, c| st.pred_mind_control(d, c, u))
    }

    /// 0x442bb0: Maelstrom.
    fn ai_maelstrom_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_maelstrom(d, c, u))
    }

    /// 0x442c30: Feedback.
    fn ai_feedback_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x400);
        let b = self.caster_box(u, r);
        self.ai_spell_flag = flag;
        self.spell_search(u, b, &mut |st, c| st.pred_feedback(d, c, u))
    }

    /// 0x442cb0: Disruption Web.
    fn ai_web_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = self.spell_radius(u, flag, 0x800);
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_web(d, c, u))
    }

    /// 0x442d30: Yamato Gun.
    fn ai_yamato_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let b = if flag != 0 { self.caster_box(u, 0x200) } else { self.map_box() };
        self.spell_search(u, b, &mut |st, c| st.pred_yamato(d, c, u))
    }

    /// 0x4431c0: Defensive Matrix.
    fn ai_matrix_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        let r = if flag == 0 { 0x800 } else { 0x120 };
        let b = self.caster_box(u, r);
        self.spell_search(u, b, &mut |st, c| st.pred_matrix(d, c, u))
    }

    /// 0x443220: Hallucination (never in the flag case).
    fn ai_hallucination_target(&mut self, d: &Data, u: u32, flag: u32) -> u32 {
        if flag != 0 {
            return NIL;
        }
        let b = self.caster_box(u, 0x400);
        self.spell_search(u, b, &mut |st, c| st.pred_hallucination(d, c, u))
    }

    // ---- predicates: `c` the candidate, `s` the caster ----

    /// 0x440e30: `c` is a visible enemy of `s`'s player worth considering (not an
    /// interceptor, scarab, nuke, or an order 0x51..0x53 unit).
    fn spell_target_ok(&self, c: u32, s: u32) -> bool {
        let su = &self.units[s as usize];
        let cu = &self.units[c as usize];
        let bit = 1u32 << (su.player & 0x1f);
        if self.sprites[cu.sprite as usize].visibility as u32 & bit == 0 {
            return false;
        }
        if cu.status & 0x300 != 0 && cu.visibility & bit == 0 {
            return false;
        }
        self.is_enemy(s, c) && !matches!(cu.unit_type, 0x49 | 0xd | 0x55) && !matches!(cu.order, 0x51..=0x53)
    }

    /// 0x440c90 (with EDX = 0): `c`'s order target is one of `s`'s allies.
    fn target_is_ally(&self, c: u32, s: u32) -> bool {
        let t = self.units[c as usize].order_target_unit;
        t != NIL && {
            let tp = self.units[t as usize].player;
            tp < 8 && self.alliances[self.units[s as usize].player as usize][tp as usize] != 0
        }
    }

    /// 0x440a60: the shields of an enemy of `s` (EMP's value).
    fn enemy_shields(&self, d: &Data, f: u32, s: u32) -> i32 {
        let fu = &self.units[f as usize];
        if fu.status & 0x400_0000 == 0 && self.is_enemy(s, f) && d.dat.units.shield_enable[fu.unit_type as usize] != 0 {
            fu.shields >> 8
        } else {
            0
        }
    }

    /// 0x440ac0: the energy of an enemy spellcaster of `s`.
    fn enemy_energy(&self, d: &Data, f: u32, s: u32) -> u32 {
        let fu = &self.units[f as usize];
        if fu.status & 0x400_0000 == 0
            && self.is_enemy(s, f)
            && d.dat.units.flags[fu.unit_type as usize] & 0x20_0000 != 0
            && fu.status & 0x4000_0000 == 0
        {
            (fu.energy >> 8) as u32 & 0xff
        } else {
            0
        }
    }

    /// 0x440b20: the nuke target's running value `sum` after enemy unit `f` (a detector,
    /// bunker-like building or lurker makes it worth the full 0x320).
    fn nuke_value(&self, d: &Data, f: u32, s: u32, mut sum: u32) -> u32 {
        let fu = &self.units[f as usize];
        if fu.status & 0x400_0000 != 0 || !self.is_enemy(s, f) {
            return sum;
        }
        let flags = d.dat.units.flags[fu.unit_type as usize];
        if flags & 8 != 0 || fu.status & 0x2_0000 == 0 {
            sum += self.life_now(d, f) as u32;
        }
        if flags & 1 != 0 && (self.is_active_detector(d, f) || fu.unit_type == 0x92 || fu.unit_type == 0x67) {
            sum = 0x320;
        }
        sum
    }

    /// 0x440dc0: what an enemy of `s` hit by weapon `w` adds to the target's value (at
    /// weapon 0x3c only the hp).
    fn spell_value_enemy(&self, d: &Data, f: u32, s: u32, w: u8) -> u32 {
        let fu = &self.units[f as usize];
        if f == s || fu.status & 0x400_0000 != 0 || !self.weapon_can_target(d, w, f, s) || !self.is_enemy(s, f) {
            return 0;
        }
        if w == 0x3c {
            return ((fu.hp + 0xff) >> 8) as u32;
        }
        self.life_now(d, f) as u32
    }

    /// 0x440cf0: the value of an enemy organic unit that is not under maelstrom.
    fn spell_value_organic(&self, d: &Data, f: u32, s: u32, w: u8) -> u32 {
        let fu = &self.units[f as usize];
        if f == s || fu.status & 0x400_0000 != 0 || !self.weapon_can_target(d, w, f, s) || !self.is_enemy(s, f) {
            return 0;
        }
        if d.dat.units.flags[fu.unit_type as usize] & 0x1_0000 != 0 && fu.maelstrom_timer == 0 {
            return self.life_now(d, f) as u32;
        }
        0
    }

    /// 0x440d60: the value of an ally of `s` that weapon `w` could hit.
    fn spell_value_ally(&self, d: &Data, f: u32, s: u32, w: u8) -> u32 {
        let fu = &self.units[f as usize];
        if fu.status & 0x400_0000 != 0 || !self.weapon_can_target(d, w, f, s) || self.is_enemy(s, f) {
            return 0;
        }
        self.life_now(d, f) as u32
    }

    /// The area test of 0x4416c0 / 0x4417a0 / 0x441950 / 0x441a50 / 0x441da0: within 0x60
    /// of `c` the enemies' value (0x440dc0, or 0x440cf0 with `organic`) is at least 0xfa
    /// (or any with the flag at 0x6957a0) and more than twice the allies' (0x440d60).
    fn area_worth(&mut self, d: &Data, c: u32, s: u32, w: u8, organic: bool) -> bool {
        let near = self.spell_units_near(c, 0x60);
        let enemy: u32 = near
            .iter()
            .map(|&f| if organic { self.spell_value_organic(d, f, s, w) } else { self.spell_value_enemy(d, f, s, w) })
            .sum();
        if self.ai_spell_flag == 0 && enemy < 0xfa {
            return false;
        }
        let ally: u32 = near.iter().map(|&f| self.spell_value_ally(d, f, s, w)).sum();
        ally * 2 < enemy
    }

    /// 0x440790: an own carrier / reaver / scout... with a target and at most 10 shields
    /// worth of damage.
    fn pred_recall(&self, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        cu.player == self.units[s as usize].player
            && cu.order_target_unit != NIL
            && matches!(cu.unit_type, 0x48 | 0x52 | 0x53 | 0x51)
            && (cu.shields & !0xff) <= 0xa00
    }

    /// 0x440870: an own unit under parasite or maelstrom that weapon 0x66 can hit.
    fn pred_restore_own(&self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        cu.player == self.units[s as usize].player
            && (cu.parasite_flags != 0 || cu.maelstrom_timer != 0)
            && self.weapon_can_target(d, 0x66, c, s)
    }

    /// 0x4407e0: an own unit with more than 0x3c life under a harmful spell.
    fn pred_restore(&self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        cu.player == self.units[s as usize].player
            && self.weapon_can_target(d, 0x66, c, s)
            && self.life_now(d, c) > 0x3c
            && (cu.ensnare_timer != 0
                || cu.plague_timer != 0
                || cu.irradiate_timer != 0
                || cu.lockdown_timer != 0
                || cu.maelstrom_timer != 0
                || cu.acid_spore_count != 0)
    }

    /// 0x4413d0: EMP: enough enemy shields (weapon 0x21) around.
    fn pred_emp_shields(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s)
            || d.dat.units.shield_enable[self.units[c as usize].unit_type as usize] == 0
            || !self.weapon_can_target(d, 0x21, c, s)
        {
            return false;
        }
        let sum: i32 = self.spell_units_near(c, 0xa0).iter().map(|&f| self.enemy_shields(d, f, s)).sum();
        sum as u32 > 199
    }

    /// 0x441450: EMP: enough enemy energy (weapon 0x21) around.
    fn pred_emp_energy(&mut self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        if !self.spell_target_ok(c, s)
            || d.dat.units.flags[cu.unit_type as usize] & 0x20_0000 == 0
            || cu.status & 0x4000_0000 != 0
            || !self.weapon_can_target(d, 0x21, c, s)
        {
            return false;
        }
        let sum: u32 = self.spell_units_near(c, 0xa0).iter().map(|&f| self.enemy_energy(d, f, s)).sum();
        sum > 199
    }

    /// 0x454c90: `c` is a living (not larva, egg, lurker egg) organic unit.
    fn is_living_organic(&self, d: &Data, c: u32) -> bool {
        let t = self.units[c as usize].unit_type;
        let f = d.dat.units.flags[t as usize];
        f & 0x1_0000 != 0 && f & 1 == 0 && !matches!(t, 0x23 | 0x24 | 0x61)
    }

    /// 0x4414e0: Irradiate.
    fn pred_irradiate(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s)
            || self.units[c as usize].irradiate_timer != 0
            || !self.weapon_can_target(d, 0x22, c, s)
            || !self.is_living_organic(d, c)
        {
            return false;
        }
        let t = self.units[c as usize].unit_type;
        self.ai_spell_flag != 0
            || !self.ai_flag20(self.units[s as usize].player)
            || d.dat.units.flags[t as usize] & 8 != 0
            || t == 0x2a
            || t == 0x22
    }

    /// 0x441560: Lockdown.
    fn pred_lockdown(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x20, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        if cu.status & 0x400 != 0 || cu.lockdown_timer != 0 || cu.stasis_timer != 0 || cu.maelstrom_timer != 0 {
            return false;
        }
        let t = cu.unit_type;
        if (t == 0x45 || t == 0xb) && cu.fl.flags & 2 != 0 && self.has_loaded_units(c) {
            return true;
        }
        if t != 0x48 && t != 0x53 {
            return self.target_is_ally(c, s);
        }
        true
    }

    /// 0x441600: the nuke target: an enemy position with at least 0x320 worth of units
    /// around it (0x440b20).
    fn pred_nuke(&mut self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        let bit = 1u32 << (self.units[s as usize].player & 0x1f);
        if cu.status & 0x300 != 0 && cu.visibility & bit == 0 {
            return false;
        }
        if !self.is_enemy(s, c) {
            return false;
        }
        let sum = self.spell_units_near(c, 0xc0).iter().fold(0u32, |sum, &f| self.nuke_value(d, f, s, sum));
        sum > 0x31f
    }

    /// 0x4416c0: Ensnare.
    fn pred_ensnare(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x3a, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        cu.ensnare_timer == 0 && cu.status & 0x2_0000 != 0 && self.target_is_ally(c, s) && self.area_worth(d, c, s, 0x3a, false)
    }

    /// 0x4417a0: Consume's host: the candidate's own life in the area (weapon 0x3c).
    fn pred_consume(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x3c, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        cu.plague_timer == 0
            && d.dat.units.flags[cu.unit_type as usize] & 0x40 == 0
            && self.area_worth(d, c, s, 0x3c, false)
    }

    /// 0x441870: Dark Swarm: an ally `c` fights (melee) a ground ally of `s` that is a
    /// melee unit whose weapon the swarm would help.
    fn pred_swarm(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let o = self.units[c as usize].order_target_unit;
        if o == NIL {
            return false;
        }
        let ou = &self.units[o as usize];
        if ou.player >= 8
            || self.alliances[self.units[s as usize].player as usize][ou.player as usize] == 0
            || ou.subunit != NIL
            || ou.status & 4 != 0
        {
            return false;
        }
        let w = self.ground_weapon(d, o);
        if w == super::attack::NO_WEAPON || d.dat.weapons.behavior[w as usize] != 5 {
            return false;
        }
        let sub = self.units[c as usize].subunit;
        let a = if self.is_turret(d, sub) { sub } else { c };
        let w2 = self.ground_weapon(d, a);
        if w2 == super::attack::NO_WEAPON {
            return false;
        }
        match d.dat.weapons.behavior[w2 as usize] {
            0 | 1 | 2 | 4 | 7 => self.miss_chance(d, o, a) == 0,
            _ => false,
        }
    }

    /// 0x441950: Storm (weapon 0x54).
    fn pred_storm(&mut self, d: &Data, c: u32, s: u32) -> bool {
        self.spell_target_ok(c, s) && self.weapon_can_target(d, 0x54, c, s) && self.area_worth(d, c, s, 0x54, false)
    }

    /// 0x441a50: Stasis (weapon 0x53): a non-building enemy attacking an ally.
    fn pred_stasis(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x53, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        if d.dat.units.flags[cu.unit_type as usize] & 1 != 0 {
            return false;
        }
        let o = cu.order_target_unit;
        if o == NIL {
            return false;
        }
        let op = self.units[o as usize].player;
        if op >= 8 || self.alliances[self.units[s as usize].player as usize][op as usize] == 0 {
            return false;
        }
        self.area_worth(d, c, s, 0x53, false)
    }

    /// 0x441b50: Optical Flare: a detector or a unit with more than 0x50 life.
    fn pred_flare(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        if d.dat.units.flags[cu.unit_type as usize] & 1 != 0 || cu.is_blind != 0 {
            return false;
        }
        self.is_active_detector(d, c) || self.life_now(d, c) > 0x50
    }

    /// 0x441bb0: Parasite (weapon 0x38).
    fn pred_parasite(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let sp = self.units[s as usize].player;
        let f20 = self.ai_flag20(sp);
        let cu = &self.units[c as usize];
        if !((!f20 || cu.parasite_flags == 0) && cu.parasite_flags & (1 << (sp & 0x1f)) == 0) {
            return false;
        }
        if !f20
            && !self.is_active_detector(d, c)
            && !self.can_carry(d, c)
            && !self.is_spellcaster(d, c)
            && d.dat.units.flags[cu.unit_type as usize] & 8 == 0
            && self.life_now(d, c) < 300
        {
            return false;
        }
        self.weapon_can_target(d, 0x38, c, s)
    }

    /// 0x442050: Plague (weapon 0x39).
    fn pred_plague(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x39, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        let f = d.dat.units.flags[cu.unit_type as usize];
        if f & 0x40 != 0 {
            return false;
        }
        if self.ai_spell_flag == 0
            && self.life_now(d, c) < 100
            && f & 8 == 0
            && !matches!(cu.unit_type, 0x1e | 5 | 0x22)
            && !(0x7b..=0x81).contains(&cu.order)
        {
            return false;
        }
        true
    }

    /// 0x441c50: Mind Control: a worthwhile enemy unit.
    fn pred_mind_control(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let t = self.units[c as usize].unit_type;
        if d.dat.units.flags[t as usize] & 0x40 != 0 {
            return false;
        }
        if (t == 0x45 || t == 0xb) && self.has_loaded_units(c) {
            return true;
        }
        if matches!(
            t,
            5 | 9 | 0xc | 0x1e | 0x22 | 0x27 | 0x2a | 0x2d | 0x2e | 0x3e | 0x3f | 0x43 | 0x47 | 0x48 | 0x53 | 0x67
        ) {
            return true;
        }
        // Costs at least a Dark Templar's in minerals and gas.
        let u = &d.dat.units;
        u.mineral_cost[t as usize] >= u.mineral_cost[0x3d] && u.gas_cost[t as usize] >= u.gas_cost[0x3d]
    }

    /// 0x441d40: Maelstrom: an organic spellcaster... whose energy is at least its life.
    fn pred_maelstrom(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        let f = d.dat.units.flags[cu.unit_type as usize];
        if f & 0x20_0000 == 0 || cu.status & 0x4000_0000 != 0 || cu.status & 2 != 0 || f & 0x40 != 0 {
            return false;
        }
        self.life_now(d, c) <= (cu.energy >> 8 & 0xff) as i32
    }

    /// 0x441da0: Feedback (weapon 0x6c).
    fn pred_feedback(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) || !self.weapon_can_target(d, 0x6c, c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        let t = cu.unit_type;
        let f = d.dat.units.flags[t as usize];
        if f & 0x1_0000 == 0 || cu.maelstrom_timer != 0 {
            return false;
        }
        if t == 0x2a && cu.fl.flags & 2 != 0 && self.has_loaded_units(c) {
            return true;
        }
        if f & 1 == 0 && self.target_is_ally(c, s) {
            return match t {
                0x23 | 0x24 | 0x3b | 0x61 => false,
                _ => self.area_worth(d, c, s, 0x6c, true),
            };
        }
        false
    }

    /// 0x441f20: Disruption Web.
    fn pred_web(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let sp = self.units[s as usize].player as usize;
        // The unit (`c`, or a loaded bunker's first passenger) attacks an ally in the air.
        let attacks_ally_air = |st: &State, o: u32| -> bool {
            let t = st.units[o as usize].order_target_unit;
            t != NIL && {
                let tu = &st.units[t as usize];
                tu.player <= 7 && tu.status & 4 != 0 && st.alliances[sp][tu.player as usize] != 0
            }
        };
        if self.units[c as usize].unit_type == 0x7d && self.has_loaded_units(c) {
            if let Some(p) = self.first_passenger(d, c) {
                if !attacks_ally_air(self, p) {
                    return false;
                }
            }
        } else if !attacks_ally_air(self, c) {
            return false;
        }
        let cu = &self.units[c as usize];
        if cu.status & 0x8000 == 0 {
            if cu.status & 4 == 0 && self.box_distance_within(d, s, 0x100, c) {
                return true;
            }
            return matches!(cu.unit_type, 0x7c | 0x7d | 0x90 | 0xa2);
        }
        false
    }

    /// 0x4420e0: Yamato Gun.
    fn pred_yamato(&mut self, d: &Data, c: u32, s: u32) -> bool {
        if !self.spell_target_ok(c, s) {
            return false;
        }
        let cu = &self.units[c as usize];
        if cu.status & 2 != 0 && self.is_armed_pub(d, c) {
            return true;
        }
        let t = cu.unit_type;
        if t == 0x7d {
            if self.has_loaded_units(c) {
                return true;
            }
        } else if t == 0x24 {
            return false;
        }
        let life = self.life_now(d, c);
        life > 199 && life < 0x1c3 && d.dat.units.flags[t as usize] & 0x40 == 0
    }

    /// 0x4425a0: Defensive Matrix: an ally without one, with a target, hurt-able (100+
    /// max life, at most 0x4b00 hp) and no spell error.
    fn pred_matrix(&mut self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        if cu.defense_matrix_damage != 0 || self.is_enemy(s, c) {
            return false;
        }
        if cu.order_target_unit != NIL
            && cu.status & 0x100 == 0
            && self.max_hp_points(d, c) >= 100
            && ((cu.hp + 0xff) & !0xff) < 0x4b01
        {
            if cu.status & 0x400_0000 == 0 {
                return self.tech_target_error(d, c, 6, self.units[s as usize].player) == 0;
            }
            return false;
        }
        false
    }

    /// 0x4428d0: Hallucination: an own carrier, scout, arbiter or archon.
    fn pred_hallucination(&mut self, d: &Data, c: u32, s: u32) -> bool {
        let cu = &self.units[c as usize];
        cu.player == self.units[s as usize].player
            && cu.status & 0x400_0000 == 0
            && self.tech_target_error(d, c, 0x14, self.units[s as usize].player) == 0
            && matches!(cu.unit_type, 0x48 | 0x46 | 0x47 | 0x44)
    }
}
