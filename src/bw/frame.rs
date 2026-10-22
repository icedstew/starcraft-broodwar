//! Frame loop: game_step (1.16.1 0x4d94b0) and update_units (0x4ed000).
//!
//! Literal ports; each callee is either ported (bw::*) or an explicit todo_port!.

use super::list::NIL;
use super::{Data, State};

impl State {
    /// 0x4d94b0 game_step.
    pub fn game_step(&mut self, d: &Data) {
        self.rng.set_enabled(true); // 0x4dc540(1)
        self.random_event_tick(d);
        self.ai_update(d);
        if self.counter_100 == 0 {
            self.counter_100 = 100;
        }
        self.counter_100 -= 1;
        self.is_100th_frame = self.counter_100 == 0;
        if self.is_100th_frame {
            self.creep_update_100();
            self.vision_reset();
        }
        self.update_units(d);
        self.update_bullets(d);
        self.update_free_flingies(d);
        self.update_thingies(d);
        self.rng.set_enabled(false); // 0x4dc540(0)
        self.images_update_pos_y(d);
    }

    /// 0x47e410: random event timer (draws from the RNG inside 0x47e130).
    fn random_event_tick(&mut self, _d: &Data) {
        if self.random_event_timer != 0 {
            self.random_event_timer -= 1;
            return;
        }
        // 0x513c24: fewer free recession nodes, more frequent recession steps.
        const DELAY: [u8; 9] = [1, 3, 5, 6, 7, 8, 9, 9, 9];
        self.random_event_timer = DELAY[(self.creep_rec.free() >> 7) as usize] as u32;
        self.recede_one();
    }

    /// 0x4ed000 update_units.
    pub fn update_units(&mut self, d: &Data) {
        self.var_6957d0 = 0;
        // 0x49a2c0; 0x48d9a0 / 0x48e310 / 0x48ca10 end placement and targeting (front end).
        if self.selection_all_disabled() {
            self.selection_emptied = true;
        }
        self.stagger_150 = self.stagger_150.wrapping_sub(1);
        if self.stagger_150 == 0 {
            let mut c = 0u8;
            for u in super::list::iter(&self.units, self.visible_units.head).collect::<Vec<_>>() {
                self.units[u as usize].order_process_timer = c;
                c = (c + 1) % 8;
            }
            self.stagger_150 = 150;
        }
        self.stagger_300 = self.stagger_300.wrapping_sub(1);
        if self.stagger_300 == 0 {
            let mut c = 0u8;
            for u in super::list::iter(&self.units, self.visible_units.head).collect::<Vec<_>>() {
                self.units[u as usize].ai_action_flag = c;
                c = (c + 1) % 30;
            }
            self.stagger_300 = 300;
        }
        self.web_pass(d);
        // Dead units (0x62842c): finish their sprites.
        let mut u = self.dead_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.update_dead_unit(d, u);
            if self.units[u as usize].sprite != NIL {
                if self.is_100th_frame && self.player_type(self.units[u as usize].player) == 2 {
                    let un = &self.units[u as usize];
                    let s = &self.sprites[un.sprite as usize];
                    let (mask, x, y, air) = (self.player_vision[un.player as usize], s.x as i32, s.y as i32, un.status & 4 != 0);
                    self.reveal(mask, 1, x, y, air);
                }
                self.update_unit_visibility(d, u);
            }
            u = next;
        }
        // Visible units (0x628430): movement.
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.unit_movement(d, u);
            u = next;
        }
        // Scanner sweeps (0x6283f4).
        if self.is_100th_frame {
            let mut u = self.scanner_units.head;
            while u != NIL {
                let next = self.units[u as usize].link.next;
                self.iscript_order_unit = Some(u);
                self.iscript_unit = Some(u);
                self.unit_reveal(d, u);
                u = next;
            }
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.update_unit_visibility(d, u);
            let un = &mut self.units[u as usize];
            if un.status & 0x300 != 0 {
                un.is_cloaked = 0;
                if un.ai_action_flag == 0 {
                    self.refresh_detection(d, u);
                    self.units[u as usize].ai_action_flag = 0x1e;
                } else {
                    un.ai_action_flag -= 1;
                }
            }
            u = next;
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u); // 0x6d11f4
            self.iscript_unit = Some(u); // 0x6d11fc
            self.update_unit(d, u);
            u = next;
        }
        let mut u = self.hidden_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            if self.units[u as usize].status & 0x300 != 0 {
                self.units[u as usize].is_cloaked = 0;
            }
            self.iscript_order_unit = Some(u);
            self.iscript_unit = Some(u);
            self.update_hidden_unit(d, u);
            u = next;
        }
        self.update_cloak_list(d);
        self.update_power(d);
        // 0x64dec8: lurker hits, 32 frames of 16 (lurker, target) pairs; advance the ring and clear the new frame.
        self.lurker_hits_frame = (self.lurker_hits_frame + 1) % 0x20;
        self.lurker_hits[self.lurker_hits_frame as usize] = [0; 0x20];
        self.lurker_hits_count = 0;
        let mut u = self.scanner_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            self.iscript_order_unit = Some(u);
            self.iscript_unit = Some(u);
            self.update_unit(d, u);
            u = next;
        }
        self.iscript_order_unit = None;
        self.iscript_unit = None;
    }

    /// 0x4eb5e0 (dead-unit list step).
    fn update_dead_unit(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        if s != NIL && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            self.free_unit_sprite(u);
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
        if self.units[u as usize].status & 0x4000 != 0 {
            if self.dead_removal_counter != 0 {
                self.dead_removal_counter -= 1;
                return;
            }
            self.dead_removal_counter = 3;
            if !self.creep_recede_unit(d, u) {
                return;
            }
        }
        if self.units[u as usize].sprite == NIL {
            self.free_unit(d, u);
        }
    }

    /// 0x4ebbd0 (esi = sprite): count the click flash timer (+0xf) down; the selection circle
    /// (image 0x231, via 0x4d7070, color 0x581d6a[owner] given by the caller) is shown while
    /// bit 3 of the timer is set, or when the sprite is selected (flags 8) and the timer is
    /// out; otherwise removed (0x4975d0). The circle itself is not an image here: flag 1
    /// marks it shown and the view draws it.
    pub fn update_selection_flash(&mut self, s: u32) {
        let sp = &mut self.sprites[s as usize];
        if sp.selection_timer == 0 {
            return;
        }
        sp.selection_timer -= 1;
        if self.local_vision_mask & sp.visibility == 0 {
            sp.selection_timer = 0;
        }
        let t = sp.selection_timer;
        if t & 8 == 0 && (t != 0 || sp.flags & 8 == 0) {
            // 0x4975d0
            sp.flags &= !1;
        } else {
            sp.flags |= 1;
        }
    }

    /// 0x4ecf70 update_unit.
    pub fn update_unit(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        let s = self.units[u as usize].sprite;
        if d.dat.units.flags[ut] & 0x10 == 0 && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 && self.sprites[s as usize].selection_timer != 0 {
            self.update_selection_flash(s);
        }
        self.update_unit_timers(d, u);
        self.execute_main_order(d, u);
        self.execute_secondary_order(d, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[ut] & 0x10 == 0 {
            self.iscript_unit = Some(sub);
            self.update_unit(d, sub);
            self.iscript_unit = Some(u);
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
    }

    /// 0x4ec420: update a unit that is not on the map (hidden sprite).
    pub fn update_hidden_unit(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[ut] & 0x10 == 0 {
            self.iscript_unit = Some(sub);
            self.update_hidden_unit(d, sub);
            self.iscript_unit = Some(u);
        }
        self.movement_step(d, u);
        self.update_unit_timers(d, u);
        self.execute_hidden_order(d, u);
        match self.units[u as usize].secondary_order {
            0x3f => self.secondary_train_fighter(d, u),
            0x6d => self.secondary_cloak(u),
            0x6e => self.set_secondary_order(u, 0x17),
            _ => {}
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
    }

    /// 0x4ebed0: main order of a hidden unit.
    fn execute_hidden_order(&mut self, d: &Data, u: u32) {
        let order = self.units[u as usize].order;
        match order {
            0 => self.order_die(d, u),
            3 | 4 | 0x16 | 0x5c => {
                let next = if self.units[u as usize].status & 0x20 != 0 { 5 } else { 0x17 };
                self.order_now(d, u, next);
            }
            0x11 => self.order_unused_powerup(d, u),
            0x17 | 0x18 | 0xa2 | 0xaf | 0xb0 => {}
            0x1d => self.order_infesting_command_center(d, u),
            0x53 => self.order_harvest_gas(d, u),
            0x61 => self.order_powerup_idle(d, u),
            0x7d => self.order_nuke_launch(d, u),
            0x96 => self.order_reset_collision(d, u),
            0x97 => self.order_reset_harvest_collision(d, u),
            _ => {
                let un = &mut self.units[u as usize];
                let t = un.order_process_timer;
                un.order_process_timer = t.wrapping_sub(1);
                if t != 0 {
                    return;
                }
                un.order_process_timer = 8;
                match order {
                    5 => self.order_bunker_guard(d, u),
                    0x60 => self.pickup4(d, u),
                    0x9c if un.status & 0x20 != 0 => self.order_bunker_guard(d, u),
                    0xa1 => self.order_rescue_passive(d, u),
                    _ => {}
                }
            }
        }
    }

    /// 0x4ec290 update_unit_timers.
    pub fn update_unit_timers(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.main_order_timer = un.main_order_timer.saturating_sub(1);
        un.ground_cooldown = un.ground_cooldown.saturating_sub(1);
        un.air_cooldown = un.air_cooldown.saturating_sub(1);
        un.spell_cooldown = un.spell_cooldown.saturating_sub(1);
        let ut = un.unit_type as usize;
        if d.dat.units.shield_enable[ut] != 0 {
            let max = (d.dat.units.shields[ut] as i32) << 8;
            if un.shields != max {
                un.shields = (un.shields + 7).min(max);
                let s = un.sprite;
                if self.sprites[s as usize].flags & 0x8 != 0 {
                    self.sprite_redraw_bars(s);
                }
            }
        }
        let un = &mut self.units[u as usize];
        // Vultures (0x25) / Jim Raynor's vulture (0x36): re-enable order processing when the weapon is ready.
        if (un.unit_type == 0x25 || un.unit_type == 0x36) && un.ground_cooldown == 0 {
            un.order_process_timer = 0;
        }
        un.is_being_healed = 0;
        let s = un.sprite;
        let completed = un.status & super::unit::status::COMPLETED != 0;
        if completed || self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            let un = &mut self.units[u as usize];
            un.cycle_counter = un.cycle_counter.wrapping_add(1);
            if un.cycle_counter > 7 {
                un.cycle_counter = 0;
                self.status_timers(d, u);
            }
        }
        if self.units[u as usize].status & super::unit::status::COMPLETED == 0 {
            return;
        }
        let ut = self.units[u as usize].unit_type as usize;
        // Regenerating units (units.dat flag 0x80) heal 4/256 hp per frame.
        if d.dat.units.flags[ut] & 0x80 != 0 && self.units[u as usize].hp > 0 && self.units[u as usize].hp != d.dat.units.hit_points[ut] as i32 {
            let hp = self.units[u as usize].hp + 4;
            self.set_unit_hp(d, u, hp);
        }
        self.regenerate_energy(d, u);
        let un = &mut self.units[u as usize];
        un.recent_order_timer = un.recent_order_timer.saturating_sub(1);
        if un.remove_timer != 0 {
            un.remove_timer -= 1;
            if un.remove_timer == 0 {
                self.kill_unit(d, u);
                return;
            }
        }
        let gf = d.dat.units.staredit_group_flags[ut];
        if gf & 5 == 0 && gf & 2 != 0 && (self.units[u as usize].status & 2 != 0 || d.dat.units.flags[ut] & 0x20 != 0) {
            // Under 34% hp, Terran buildings burn 0x14 (fp8) a frame.
            if self.hp_below_third(d, u) {
                let p = self.units[u as usize].last_attacking_player;
                self.apply_damage_pub(d, u, 0x14, NIL, p, true);
            }
        }
    }

    /// 0x492f70: every 8 frames the spell timers count down and end their effects.
    fn status_timers(&mut self, d: &Data, u: u32) {
        let dec = |v: &mut u8| -> bool {
            if *v == 0 {
                return false;
            }
            *v -= 1;
            *v == 0
        };
        if dec(&mut self.units[u as usize].stasis_timer) {
            self.stasis_ends(d, u);
        }
        if dec(&mut self.units[u as usize].stim_timer) {
            self.update_speed(d, u);
        }
        if dec(&mut self.units[u as usize].ensnare_timer) {
            self.remove_overlay_in(u, 0x180, 0x182);
            self.units[u as usize].ensnare_timer = 0;
            self.update_speed(d, u);
        }
        if dec(&mut self.units[u as usize].defense_matrix_timer) {
            let hp = self.units[u as usize].defense_matrix_damage as i32;
            self.defense_matrix_hit(d, u, hp);
        }
        if self.units[u as usize].irradiate_timer != 0 {
            self.units[u as usize].irradiate_timer -= 1;
            self.irradiate_tick(d, u);
            if self.units[u as usize].irradiate_timer == 0 {
                self.remove_overlay_in(u, 0x17c, 0x17e);
                let un = &mut self.units[u as usize];
                un.irradiate_timer = 0;
                un.irradiated_by = NIL;
                un.irradiate_player = 8;
            }
        }
        if dec(&mut self.units[u as usize].lockdown_timer) {
            self.lockdown_ends(d, u);
        }
        if dec(&mut self.units[u as usize].maelstrom_timer) {
            self.maelstrom_ends(d, u);
        }
        if self.units[u as usize].plague_timer != 0 {
            self.units[u as usize].plague_timer -= 1;
            let dmg = ((d.dat.weapons.damage[0x3c] as i32) << 8) / 0x4c;
            if self.units[u as usize].status & 0x400_0000 == 0 && self.units[u as usize].hp > dmg {
                self.apply_damage_pub(d, u, dmg, NIL, 0xff, true);
            }
            if self.units[u as usize].plague_timer == 0 {
                self.remove_overlay_in(u, 0x184, 0x186);
                self.units[u as usize].plague_timer = 0;
            }
        }
        let st = &mut self.units[u as usize].storm_timer;
        *st = st.saturating_sub(1);
        // 0x4f42c0: each acid spore wears off on its own timer.
        let before = self.units[u as usize].acid_spore_count;
        for i in 0..9 {
            let un = &mut self.units[u as usize];
            if un.acid_spore_time[i] != 0 {
                un.acid_spore_time[i] -= 1;
                if un.acid_spore_time[i] == 0 {
                    un.acid_spore_count = un.acid_spore_count.wrapping_sub(1);
                }
            }
        }
        self.acid_overlay_update(d, u, before);
    }

    /// 0x454d20: maelstrom ends on `u`.
    pub(super) fn maelstrom_ends(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.maelstrom_timer = 0;
        if un.status & 0x400 == 0 && un.lockdown_timer == 0 && un.stasis_timer == 0 {
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8 / 0x68c1ec: UI refresh flags.
            let t = un.unit_type;
            self.set_button_set(d, u, t);
        }
        self.spell_overlay_ends(d, u, 0x3d6, 0x3d8);
    }

    /// 0x4f62d0: stasis ends on `u`: the button set comes back (0x4e5d60) unless still
    /// disabled, the unit is no longer invincible (status 0x4000000) unless its type is.
    pub(super) fn stasis_ends(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.stasis_timer = 0;
        if un.status & 0x400 == 0 && un.lockdown_timer == 0 && un.maelstrom_timer == 0 {
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8 / 0x68c1ec: UI refresh flags.
            let t = un.unit_type;
            self.set_button_set(d, u, t);
        }
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000_0000 == 0 {
            self.units[u as usize].status &= !0x400_0000;
        }
        self.spell_overlay_ends(d, u, 0x16d, 0x16f);
    }

    /// 0x454d90: lockdown ends on `u`.
    pub(super) fn lockdown_ends(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.lockdown_timer = 0;
        if un.status & 0x400 == 0 && un.stasis_timer == 0 && un.maelstrom_timer == 0 {
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8 / 0x68c1ec: UI refresh flags.
            let t = un.unit_type;
            self.set_button_set(d, u, t);
        }
        self.spell_overlay_ends(d, u, 0x169, 0x16b);
    }

    /// 0x492370: a disabling spell wears off: its overlay (image ids lo..=hi, on the unit
    /// or its turret) plays its death animation, the turret works again, and a worker
    /// that was waiting at a mineral field resumes (`worker_resume_mining`).
    pub(super) fn spell_overlay_ends(&mut self, d: &Data, u: u32, lo: u16, hi: u16) {
        let find = |st: &State, x: u32| {
            let s = st.units[x as usize].sprite;
            let mut i = st.sprites[s as usize].images.head;
            while i != NIL {
                let id = st.images[i as usize].image_id;
                if (lo..=hi).contains(&id) {
                    return i;
                }
                i = st.images[i as usize].link.next;
            }
            NIL
        };
        let mut img = find(self, u);
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            if img == NIL {
                img = find(self, sub);
            }
            if !self.disabled(u) {
                self.units[sub as usize].status &= !0x400;
                let o = d.dat.units.ai_return_to_idle[self.units[sub as usize].unit_type as usize];
                self.order_now(d, sub, o);
            }
        }
        if img != NIL {
            self.image_run_anim(d, img, 1);
        }
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 8 != 0 {
            self.worker_resume_mining(d, u);
        }
        self.units[u as usize].order_process_timer = 0xf;
    }

    /// 0x468db0 (ESI = `u`): a worker that is not disabled and waits (order 0x56 / 0x52,
    /// state 2) at a mineral field leaves the wait list and takes the field.
    fn worker_resume_mining(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let m = un.ptr_at(0xd0);
        if !self.disabled(u)
            && m != NIL
            && d.dat.units.flags[self.units[m as usize].unit_type as usize] & 0x2000 != 0
            && un.u8_at(0xce) == 0
            && self.units[m as usize].u8_at(0xd3) == 0
            && matches!(un.order, 0x56 | 0x52)
            && un.order_state == 2
        {
            self.leave_wait_list(u, m);
            self.remove_queued(d, u, 0x59);
            self.try_take_mineral(d, u, m);
        }
    }

    /// 0x4ec170 execute_secondary_order.
    pub fn execute_secondary_order(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.secondary_order == 0x95 {
            self.secondary_hallucination(d, u);
            return;
        }
        // Disabled units (doodad-disabled, lockdown, stasis, maelstrom) skip secondary orders.
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            return;
        }
        match un.secondary_order {
            0x25 => self.secondary_build_addon(d, u),
            0x26 => self.secondary_train(d, u),
            0x3f => self.secondary_train_fighter(d, u),
            0x44 => self.secondary_shield_battery(d, u),
            0x4e => self.hatchery_spawn_larva(d, u),
            0x66 => self.secondary_spread_creep(d, u),
            0x6d => self.secondary_cloak(u),
            0x6e => self.secondary_decloak(u),
            0x83 => self.secondary_cloak_nearby(d, u),
            _ => {}
        }
    }

    /// 0x4ea880: secondary order SpreadCreep (0x66); hatcheries also spawn larvae.
    fn secondary_spread_creep(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type;
        if matches!(ut, 0x83..=0x85) {
            self.hatchery_spawn_larva(d, u);
        }
        // Spread creep timer: union +0xcc.
        let t = self.units[u as usize].u8_at(0xcc);
        if t != 0 {
            self.units[u as usize].set_u8_at(0xcc, t - 1);
            return;
        }
        self.units[u as usize].set_u8_at(0xcc, 0xf);
        let (x, y) = self.unit_sprite_pos(u);
        // Every spreader spreads like a hive.
        if self.spread_creep(d, 0x85, x as i32, y as i32) == 0 {
            let next = if matches!(ut, 0x83..=0x85) { 0x4e } else { 0x17 };
            self.set_secondary_order(u, next);
        }
    }

    /// Set the secondary order (clears its state like 0x1401c9840 / inline in 1.16.1).
    pub fn set_secondary_order(&mut self, u: u32, order: u8) {
        let un = &mut self.units[u as usize];
        if un.secondary_order != order {
            un.secondary_order = order;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
    }

    /// 0x497920 update_unit_sprite: run each image's update hook and iscript; free empty sprites.
    pub fn update_unit_sprite(&mut self, d: &Data, s: u32) {
        let mut img = self.sprites[s as usize].images.head;
        while img != NIL {
            let next = self.images[img as usize].link.next;
            self.image_update_hook(d, img);
            super::iscript::run(self, d, img, false, None);
            img = next;
            if self.sprites[s as usize].images.head == NIL {
                self.free_sprite(s);
            }
        }
    }

    /// Image update function pointer (CImage +0x38, table 0x512514 by draw function):
    /// 2 / 5 cloaking (0x4d5d70), 4 / 7 decloaking (0x4d5d10), 0x11 warp fade
    /// (0x4d8590); the rest do nothing (0x4d55f0). +0x30 / +0x31 (low bytes of
    /// `coloring`) are the fade level and its timer.
    fn image_update_hook(&mut self, d: &Data, img: u32) {
        let m = self.images[img as usize].modifier;
        let c = self.images[img as usize].coloring;
        let (lvl, t) = (c as u8, (c >> 8) as u8);
        let set = |st: &mut State, lvl: u8, t: u8| {
            let i = &mut st.images[img as usize];
            i.coloring = (i.coloring & !0xffff) | lvl as u64 | (t as u64) << 8;
        };
        match m {
            2 | 5 => {
                if t != 0 {
                    return set(self, lvl, t - 1);
                }
                let lvl = lvl.wrapping_add(1);
                set(self, lvl, 3);
                if lvl >= 8 {
                    // 0x4d58b0: next draw function (cloaked).
                    self.images[img as usize].modifier = m + 1;
                    if let Some(u) = self.iscript_unit {
                        self.units[u as usize].status |= 0x300;
                    }
                }
                self.images[img as usize].flags |= super::sprite::iflag::REDRAW;
            }
            4 | 7 => {
                if t != 0 {
                    return set(self, lvl, t - 1);
                }
                set(self, lvl.wrapping_sub(1), 3);
                if lvl == 0 {
                    self.images[img as usize].modifier = 0;
                    if let Some(u) = self.iscript_unit {
                        self.units[u as usize].status &= !0x300;
                    }
                }
                self.images[img as usize].flags |= super::sprite::iflag::REDRAW;
            }
            0x11 => {
                // 0x4d8590: warp fade.
                if t != 0 {
                    return set(self, lvl, t - 1);
                }
                set(self, lvl, 2);
                if lvl < 0x3f {
                    set(self, lvl + 1, 2);
                    self.images[img as usize].flags |= super::sprite::iflag::REDRAW;
                    return;
                }
                if self.images[img as usize].iscript.anim != 1 {
                    let header = self.images[img as usize].iscript.header as usize;
                    let pc = d.iscript_u16(header + 10);
                    let st = &mut self.images[img as usize].iscript;
                    st.anim = 1;
                    st.pc = pc;
                    st.wait = 0;
                    st.ret = 0;
                    super::iscript::run(self, d, img, false, None);
                }
                if let Some(u) = self.iscript_order_unit {
                    self.units[u as usize].order_signal |= 1;
                }
                self.images[img as usize].flags |= super::sprite::iflag::REDRAW;
            }
            _ => {}
        }
    }

    /// Unlink an empty sprite from its row list and return it to the free list.
    pub fn free_sprite(&mut self, s: u32) {
        let row = self.sprite_row(s);
        let mut l = self.sprite_rows[row];
        super::list::remove(&mut self.sprites, s, &mut l);
        self.sprite_rows[row] = l;
        super::list::insert_after_head(&mut self.sprites, s, &mut self.free_sprites);
    }

    /// 0x488510: update map sprites (0x654874) and fog sprites (0x654868).
    pub fn update_thingies(&mut self, d: &Data) {
        let mut t = self.active_thingies.head;
        while t != NIL {
            let next = self.thingies[t as usize].link.next;
            self.update_thingy(d, t);
            t = next;
        }
        let mut t = self.active_fog_thingies.head;
        while t != NIL {
            let next = self.fog_thingies[t as usize].link.next;
            self.update_fog_thingy(d, t);
            t = next;
        }
    }

    /// 0x488410: a fogged copy of sprite `src` (a building of `unit_type` the local player
    /// just lost sight of).
    pub(super) fn create_fog_thingy(&mut self, d: &Data, unit_type: u16, src: u32) -> Option<u32> {
        let t = super::sprite::pop(&mut self.fog_thingies, &mut self.free_fog_thingies)?;
        let s = self.copy_sprite(d, src);
        self.fog_thingies[t as usize].hp = 1;
        self.fog_thingies[t as usize].sprite = s.unwrap_or(NIL);
        if s.is_none() {
            super::list::insert_after_head(&mut self.fog_thingies, t, &mut self.free_fog_thingies);
            return None;
        }
        self.fog_thingies[t as usize].hp = unit_type as i32;
        super::list::insert_after_head(&mut self.fog_thingies, t, &mut self.active_fog_thingies);
        Some(t)
    }

    /// 0x488350: a fogged copy goes away once the local player sees any of its area.
    fn update_fog_thingy(&mut self, d: &Data, t: u32) {
        let s = self.fog_thingies[t as usize].sprite;
        if self.sprites[s as usize].selection_timer != 0 {
            self.update_selection_flash(s);
        }
        if self.fog_thingy_area_fogged(d, t) {
            return;
        }
        // 0x4975d0: the copy has no selection circle (UI).
        self.free_sprite_images(s);
        self.fog_thingies[t as usize].sprite = NIL;
        super::list::remove(&mut self.fog_thingies, t, &mut self.active_fog_thingies);
        super::list::insert_after_head(&mut self.fog_thingies, t, &mut self.free_fog_thingies);
    }

    /// 0x487cf0 (eax = 0, 0x402380): every tile under fog thingy `t`'s placement box is
    /// fogged for the local player (in a replay: hidden from every shown player).
    fn fog_thingy_area_fogged(&self, d: &Data, t: u32) -> bool {
        let th = &self.fog_thingies[t as usize];
        let sp = &self.sprites[th.sprite as usize];
        let b = d.dat.units.placement_box[th.hp as usize];
        let (w, h) = ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32);
        let div32 = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
        let (x0, y0) = (div32(sp.x as i32 - w / 2), div32(sp.y as i32 - h / 2));
        let (cols, rows) = (div32(w + 0x1f), div32(h + 0x1f));
        let mw = self.terrain.width as i32;
        for y in (y0..y0 + rows).rev() {
            for x in (x0..x0 + cols).rev() {
                // ponytail: tiles off the map (1.16.1 reads past the flags) count as fogged.
                let f = self.terrain.tile_flags.get((mw * y + x) as usize).copied().unwrap_or(u32::MAX);
                let seen = if self.replay { !(f as u8) & self.local_vision_mask != 0 || self.reveal_all } else { f as u8 & self.local_vision_mask == 0 };
                if seen {
                    return false;
                }
            }
        }
        true
    }

    /// 0x487cf0 (eax = 1, 0x402440) in a replay: some tile under fog thingy `t` is
    /// unexplored by every shown player.
    pub(super) fn fog_thingy_area_unexplored(&self, d: &Data, t: u32) -> bool {
        let th = &self.fog_thingies[t as usize];
        let sp = &self.sprites[th.sprite as usize];
        let b = d.dat.units.placement_box[th.hp as usize];
        let (w, h) = ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32);
        let div32 = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
        let (x0, y0) = (div32(sp.x as i32 - w / 2), div32(sp.y as i32 - h / 2));
        let (cols, rows) = (div32(w + 0x1f), div32(h + 0x1f));
        let mw = self.terrain.width as i32;
        let m = (self.local_vision_mask as u32) << 8;
        (y0..y0 + rows).any(|y| {
            (x0..x0 + cols).any(|x| {
                let f = self.terrain.tile_flags.get((mw * y + x) as usize).copied().unwrap_or(u32::MAX);
                m & !f == 0 && !self.reveal_all
            })
        })
    }

    /// 0x487b00: a fogged mineral field or geyser copy whose placement box, in tiles, is
    /// within `range` tiles of tile (x, y).
    pub(super) fn fog_resource_near(&self, d: &Data, x: i32, y: i32, range: i32) -> Option<u32> {
        let mut t = self.active_fog_thingies.head;
        while t != NIL {
            let sp = &self.sprites[self.fog_thingies[t as usize].sprite as usize];
            let ut = match sp.sprite_id {
                0xb2 | 0xcc | 0x107 | 0x113 => Some(0xbc),
                0x117..=0x119 => Some(sp.sprite_id - 0x67),
                _ => None,
            };
            if let Some(ut) = ut {
                let b = d.dat.units.placement_box[ut as usize];
                let (w, h) = ((b & 0xffff) as i16 as i32, (b >> 16) as i16 as i32);
                let div32 = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
                let x0 = div32(sp.x as i32 - w / 2);
                if ((x - x0 + range) as u32) < (div32(w) + range * 2) as u32 {
                    let y0 = div32(sp.y as i32 - h / 2);
                    if ((y - y0 + range) as u32) < (div32(h) + range * 2) as u32 {
                        return Some(t);
                    }
                }
            }
            t = self.fog_thingies[t as usize].link.next;
        }
        None
    }

    /// 0x487c70: a fog copy of sprite `sprite_id` whose `w` x `h` box centered on it holds
    /// (x, y), skipping the unit types of 0x46ed80.
    pub(super) fn fog_sprite_at(&self, sprite_id: u16, x: i32, y: i32, w: i16, h: i16) -> Option<u32> {
        let mut t = self.active_fog_thingies.head;
        while t != NIL {
            let th = &self.fog_thingies[t as usize];
            let sp = &self.sprites[th.sprite as usize];
            let skip = super::select::never_picked(th.hp as u16);
            if sp.sprite_id == sprite_id
                && !skip
                && ((w as i32 / 2 + sp.x as i32 - x) as u32) < w as i32 as u32
                && ((h as i32 / 2 + sp.y as i32 - y) as u32) < h as i32 as u32
            {
                return Some(t);
            }
            t = th.link.next;
        }
        None
    }

    /// 0x487690 / 0x4877b0: a map sprite's visibility from the fog over its area:
    /// doodad-like sprites (ids < 0x82, 0x182..=0x1e0) show where players have explored
    /// (all of them with the replay's all-players vision), others where players see now.
    fn thingy_fog_visibility(&mut self, s: u32) {
        let sp = &self.sprites[s as usize];
        if sp.flags & super::sprite::sflag::HIDDEN != 0 {
            return;
        }
        let (mw, mh) = (self.terrain.width as i32, self.terrain.height as i32);
        let (w, h) = (sp.width as i32, sp.height as i32);
        let div32 = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
        let mut x0 = div32(sp.x as i32 - w / 2);
        let mut y0 = div32(sp.y as i32 - h / 2);
        let mut cols = (w + 0x1f) >> 5;
        let mut rows = (h + 0x1f) >> 5;
        let id = sp.sprite_id;
        let explored = id < 0x82 || (0x182..0x1e1).contains(&id);
        if explored {
            if x0 < 0 {
                cols += x0;
                x0 = 0;
            } else if cols + x0 > mw {
                cols = mw - x0;
            }
            if y0 < 0 {
                rows += y0;
                y0 = 0;
            } else if rows + y0 > mh {
                rows = mh - y0;
            }
        } else {
            if x0 < 0 {
                cols += x0;
                if cols < 1 {
                    return;
                }
                x0 = 0;
            }
            if y0 < 0 {
                rows += y0;
                y0 = 0;
                if rows < 1 {
                    return;
                }
            }
            if cols + x0 > mw {
                cols = mw - x0;
                if cols < 1 {
                    return;
                }
            }
            if !(rows + y0 <= mh || (mh != y0 && mh - y0 >= 0)) {
                return;
            }
        }
        let all = explored && self.replay && self.reveal_all;
        let mask = if all {
            0xff
        } else {
            (if explored {
                self.tiles_unexplored_mask(x0, y0, cols, rows)
            } else {
                self.tiles_unseen_mask(x0, y0, cols, rows)
            }) as u8
        };
        if self.sprites[s as usize].visibility != mask {
            self.sprite_set_visibility(s, mask);
        }
    }

    /// 0x488020: one map sprite; freed once its images are gone.
    fn update_thingy(&mut self, d: &Data, t: u32) {
        let s = self.thingies[t as usize].sprite;
        if s != NIL {
            self.thingy_fog_visibility(s);
        }
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.thingies[t as usize].sprite = NIL;
            }
        }
        if self.thingies[t as usize].sprite == NIL {
            super::list::remove(&mut self.thingies, t, &mut self.active_thingies);
            super::list::insert_after_head(&mut self.thingies, t, &mut self.free_thingies);
        }
    }

    /// 0x488660: show the right-click marker at map point (x, y): the marker sprite
    /// moves there (one pixel down-right, as the original increments both) and its
    /// images restart animation 2.
    pub fn show_marker(&mut self, d: &Data, x: i16, y: i16) {
        let Some(t) = self.marker_thingy else { return };
        let s = self.thingies[t as usize].sprite;
        self.marker_shown = false;
        self.move_sprite(s, x.wrapping_add(1), y.wrapping_add(1));
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            if self.images[i as usize].flags & super::sprite::iflag::HAS_ISCRIPT_ANIMATIONS != 0 {
                let header = self.images[i as usize].iscript.header as usize;
                let st = &mut self.images[i as usize].iscript;
                st.anim = 2;
                st.pc = d.iscript_u16(header + 0xc);
                st.wait = 0;
                st.ret = 0;
                super::iscript::run(self, d, i, false, None);
            }
            i = next;
        }
        self.marker_shown = true;
    }

    /// 0x488550 tail: the hidden placeholder sprite 0x13e created with the pools.
    pub fn init_thingies(&mut self, d: &Data) {
        let Some(t) = self.create_thingy(d, 0x13e, 0, 0, 0) else { return };
        self.marker_thingy = Some(t);
        let s = self.thingies[t as usize].sprite;
        self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
        self.sprite_set_visibility(s, 0);
    }
}
