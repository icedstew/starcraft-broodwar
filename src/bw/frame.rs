//! Frame loop: game_step (1.16.1 0x4d94b0) and update_units (0x4ed000).
//!
//! Literal ports; each callee is either ported (bw::*) or an explicit todo_port!.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x4d94b0 game_step.
    pub fn game_step(&mut self, d: &Data) {
        self.rng.enabled = true; // 0x4dc540(1)
        self.random_event_tick(d);
        todo_port!("0x4a2b00", "ai_update");
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
        todo_port!("0x496330", "update flingies (0x63ff34)");
        self.update_thingies(d);
        self.rng.enabled = false; // 0x4dc540(0)
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
        // 0x49a2c0 + 0x48d9a0/0x48e310/0x48ca10 refresh the selection UI only.
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

    /// 0x4ecf70 update_unit.
    pub fn update_unit(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type as usize;
        let s = self.units[u as usize].sprite;
        if d.dat.units.flags[ut] & 0x10 == 0 && self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
            todo_port!("0x4ebbd0", "sprite visibility for owner");
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
            0x11 => todo_port!("0x4f3ea0", "hidden order 0x11"),
            0x17 | 0x18 | 0xa2 | 0xaf | 0xb0 => {}
            0x1d => todo_port!("0x4ea290", "hidden order 0x1d"),
            0x53 => self.order_harvest_gas(d, u),
            0x61 => todo_port!("0x4f3e10", "hidden order 0x61"),
            0x7d => self.order_nuke_launch(d, u),
            0x96 => todo_port!("0x4671b0", "hidden order 0x96"),
            0x97 => todo_port!("0x42e320", "hidden order 0x97"),
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
                    0x60 => todo_port!("0x4e7b70", "hidden order 0x60"),
                    0x9c if un.status & 0x20 != 0 => self.order_bunker_guard(d, u),
                    0xa1 => todo_port!("0x4a1ef0", "hidden order 0xa1"),
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
                    todo_port!("0x497430", "selection hp/shield bar redraw");
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
            // 0x4022c0: under 34% hp, Terran buildings burn 0x14 (fp8) a frame.
            let un = &self.units[u as usize];
            let mut max = d.dat.units.hit_points[ut] as i32 >> 8;
            if max == 0 {
                max = (un.hp + 0xff) >> 8;
                if max == 0 {
                    max = 1;
                }
            }
            if ((((un.hp + 0xff) >> 8) * 100) / max) < 0x22 {
                let p = un.last_attacking_player;
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
            // 0x4f62d0 (0x4e5d60: the button refresh is UI)
            if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 0x2000_0000 == 0 {
                self.units[u as usize].status &= !0x400_0000;
            }
            self.spell_overlay_ends(d, u, 0x16d, 0x16f);
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
            // 0x454d90
            self.spell_overlay_ends(d, u, 0x169, 0x16b);
        }
        if dec(&mut self.units[u as usize].maelstrom_timer) {
            // 0x454d20
            self.spell_overlay_ends(d, u, 0x3d6, 0x3d8);
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

    /// 0x492370: a disabling spell wears off: its overlay (image ids lo..=hi, on the unit
    /// or its turret) plays its death animation, the turret works again, and a worker
    /// that was waiting at a mineral field resumes (0x468db0).
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
            // 0x468db0
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
        self.units[u as usize].order_process_timer = 0xf;
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
        todo_port!("0x488350", "fog sprites (0x654868)");
    }

    /// 0x488020: one map sprite; freed once its images are gone.
    fn update_thingy(&mut self, d: &Data, t: u32) {
        todo_port!("0x487690/0x4877b0", "map sprite visibility from fog");
        let s = self.thingies[t as usize].sprite;
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
