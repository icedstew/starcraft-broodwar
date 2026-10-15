//! execute_main_order (1.16.1 0x4ec4d0): order dispatch.
//!
//! Generated from the decompiled switches (re: dispatch table), then hand-ported per handler.
//! Unported handlers are `todo_port!` stubs that leave the unit's state unchanged.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x4ec4d0.
    pub fn execute_main_order(&mut self, d: &Data, u: u32) {
        let order = self.units[u as usize].order;
        match order {
            0x0 => return self.order_die(d, u),
            0x30 => return self.order_incomplete_warping(d, u),
            0x81 => return self.order_nuke_track(d, u),
            0xae => return self.order_warp_in(d, u),
            _ => {}
        }
        let un = &self.units[u as usize];
        let st = un.status;
        if st & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            self.order_disabled_tick(d, u);
            return;
        }
        if st & 0x20000 == 0 && st & 0x8000 != 0 {
            self.order_disabled_tick(d, u);
        }
        match order {
            0x4 => self.order_turret_guard(d, u),
            0x16 => self.order_turret_attack(d, u),
            0x1a => self.order_drone_build(d, u),
            0x1e => self.order_place_building(d, u),
            0x1f => self.order_place_protoss_building(d, u),
            0x21 => self.order_constructing_building(d, u),
            0x22 => self.order_repair(d, u),
            0x29 => self.order_zerg_birth(d, u),
            0x2a => self.order_zerg_unit_morph(d, u),
            0x2c => self.order_incomplete_building(d, u),
            0x2d => self.order_incomplete_morphing(d, u),
            0x41 => self.order_scarab_attack(d, u),
            0x42 => self.order_recharge_shields_unit(d, u),
            0x47 => self.order_building_land(d, u),
            0x48 => self.order_building_lift_off(d, u),
            0x4b => self.order_research_tech(d, u),
            0x4c => self.order_upgrade(d, u),
            0x58 => self.order_harvest3(d, u),
            0x59 => self.order_harvest4(d, u),
            0x5b => self.order_interrupted(d, u),
            0x62 => self.order_sieging(d, u),
            0x63 => self.order_unsieging(d, u),
            0x69 => self.order_archon_warp(d, u),
            0x6a => self.order_completing_archon_summon(d, u),
            0x7c => self.order_nuke_train(d, u),
            0x82 => self.order_initialize_arbiter(d, u),
            0x96 => self.order_reset_collision(d, u),
            0x97 => self.order_reset_harvest_collision(d, u),
            0x9b => self.order_ctfcop2(d, u),
            0xa5 => self.order_self_destructing(d, u),
            0xa6 => self.order_critter(d, u),
            0xb0 => self.order_medic_heal(d, u),
            0xb1 => self.order_heal_move(d, u),
            0xb2 => self.order_medic_hold_position(d, u),
            0xb3 => self.order_medic_heal_to_idle(d, u),
            0xb7 => self.order_dark_archon_meld(d, u),
            _ => {}
        }
        let un = &mut self.units[u as usize];
        let t = un.order_process_timer;
        un.order_process_timer = t.wrapping_sub(1);
        if t != 0 {
            return;
        }
        un.order_process_timer = 8;
        self.order_timer_tick(d, u);
        self.units[u as usize].user_action_flags = 0;
        // The first switch may have changed the order (0x4ec4d0 re-reads +0x4d).
        match self.units[u as usize].order {
            0x1 => self.order_stop(d, u),
            0x2 => self.order_guard(d, u),
            0x3 => self.order_player_guard(d, u),
            0x6 | 0x33 => self.order_move(d, u),
            0x7 => self.order_reaver_stop(d, u),
            0x9 | 0x1c | 0x23 | 0x36 | 0x3c | 0x50 | 0x72 => self.order_attack2(d, u),
            0xa => self.order_attack_unit(d, u),
            0xb => self.order_attack_fixed_range(d, u),
            0xe => self.order_attack_move(d, u),
            0xf => self.order_infested_command_center(d, u),
            0x11 => self.order_unused_powerup(d, u),
            0x12 => self.order_tower_guard(d, u),
            0x13 => self.order_tower_attack(d, u),
            0x14 => self.order_vulture_mine(d, u),
            0x15 => self.order_stay_in_range(d, u),
            0x19 => self.order_drone_start_build(d, u),
            0x1b => self.order_cast_infestation(d, u),
            0x1d => self.order_infesting_command_center(d, u),
            0x20 => self.order_create_protoss_building(d, u),
            0x24 => self.order_place_addon(d, u),
            0x2b => self.order_zerg_building_morph(d, u),
            0x2e => self.order_build_nydus_exit(d, u),
            0x2f => self.order_enter_nydus_canal(d, u),
            0x31 => self.order_follow(d, u),
            0x32 | 0x38 | 0x39 => self.order_carrier(d, u),
            0x34 => self.order_carrier_stop(d, u),
            0x35 => self.order_carrier_attack(d, u),
            0x37 => self.order_carrier_ignore2(d, u),
            0x3a | 0x3d | 0x3e => self.order_reaver(d, u),
            0x3b => self.order_reaver_attack(d, u),
            0x40 => self.order_interceptor_attack(d, u),
            0x43 => self.order_recharge_shields_battery(d, u),
            0x45 => self.order_interceptor_return(d, u),
            0x46 => self.order_drone_land(d, u),
            0x4a => self.order_lifting_off(d, u),
            0x4d => self.order_larva(d, u),
            0x4f | 0x51 => self.order_harvest1(d, u),
            0x52 => self.order_wait_for_gas(d, u),
            0x53 => self.order_harvest_gas(d, u),
            0x54 | 0x5a => self.order_return_resources(d, u),
            0x55 => self.order_move_to_minerals(d, u),
            0x56 => self.order_wait_for_minerals(d, u),
            0x57 => self.order_mining_minerals(d, u),
            0x5c => self.order_enter_transport(d, u),
            0x5d => self.order_pickup_idle(d, u),
            0x5e => self.order_pickup_transport(d, u),
            0x5f => self.order_pickup_bunker(d, u),
            0x60 => self.order_pickup4(d, u),
            0x61 => self.order_powerup_idle(d, u),
            0x64 => self.order_watch_target(d, u),
            0x65 => self.order_init_creep_growth(d, u),
            0x67 => self.order_stopping_creep_growth(d, u),
            0x6b => self.order_hold_position(d, u),
            0x6c | 0x88 => self.order_queen_hold_position(d, u),
            0x17 => self.order_nothing(d, u),
            0x6f => self.order_unload(d, u),
            0x70 => self.order_move_unload(d, u),
            0x71 | 0x73 | 0x77 | 0x78 | 0x79 | 0x7a | 0x8e | 0x8f | 0x90 | 0x91 | 0x92 | 0x93 | 0xb4 | 0xb5 | 0xb9 | 0xba => self.order_cast_spell(d, u),
            0x74 => self.order_burrowing(d, u),
            0x75 => self.order_burrowed(d, u),
            0x76 => self.order_unburrowing(d, u),
            0x7d => self.order_nuke_launch(d, u),
            0x7e => self.order_nuke_paint(d, u),
            0x7f => self.order_nuke_unit(d, u),
            0x80 => self.order_cast_nuclear_strike(d, u),
            0x84 => self.order_place_mine(d, u),
            0x85 => self.order_right_click_action(d, u),
            0x86 => self.order_suicide_unit(d, u),
            0x87 => self.order_suicide_location(d, u),
            0x89 => self.order_cast_recall(d, u),
            0x8b => self.order_cast_scanner_sweep(d, u),
            0x8c => self.order_scanner(d, u),
            0x8d => self.order_cast_defensive_matrix(d, u),
            0x94 => self.order_cast_hallucination(d, u),
            0x98 => self.order_patrol(d, u),
            0x99 => self.order_ctfcopinit(d, u),
            0x9a => self.order_ctfcop1(d, u),
            0x9c => self.order_computer_ai(d, u),
            0x9d => self.order_atk_move_ep(d, u),
            0x9e => self.order_harass_move(d, u),
            0x9f => self.order_aipatrol(d, u),
            0xa0 => self.order_guard_post(d, u),
            0xa1 => self.order_rescue_passive(d, u),
            0xa2 => self.order_neutral(d, u),
            0xa3 => self.order_computer_return(d, u),
            0xa4 => self.order_initialize_psi_provider(d, u),
            0xa7 => self.order_hidden_gun(d, u),
            0xa8 => self.order_open_door(d, u),
            0xa9 => self.order_close_door(d, u),
            0xaa => self.order_hide_trap(d, u),
            0xab => self.order_reveal_trap(d, u),
            0xac => self.order_enable_doodad(d, u),
            0xad => self.order_disable_doodad(d, u),
            0xaf => self.order_medic(d, u),
            0xb6 => self.order_cast_mind_control(d, u),
            0xb8 => self.order_cast_feedback(d, u),
            0xbb => self.order_junk_yard_dog(d, u),
            _ => {}
        }
    }

    /// 0x4eb5b0: halt, and stop turning (next target waypoint := move target).
    pub fn unit_halt_facing(&mut self, d: &Data, u: u32) {
        self.unit_halt(d, u);
        let f = &mut self.units[u as usize].fl;
        if f.move_target != f.next_target_waypoint {
            f.next_target_waypoint = f.move_target;
        }
    }

    /// 0x43ff00: disabled units only tick the (15-frame) main order timer.
    fn order_disabled_tick(&mut self, _d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        if un.main_order_timer != 0 {
            return;
        }
        un.main_order_timer = 0xf;
        if un.lockdown_timer != 0 || un.maelstrom_timer != 0 || un.status & 0x8000 != 0 {
            self.ai_alert_around(_d, u, 0x280, false);
        }
    }

    /// 0x4e8280 over (x +- r, y +- r) with callback 0x43fcf0: computer units nearby
    /// that could attack `u` react to it (0x6957ac = `wide`).
    fn ai_alert_around(&mut self, d: &Data, u: u32, r: i16, wide: bool) {
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(r), y.wrapping_sub(r), x.wrapping_add(r), y.wrapping_add(r)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        for c in found {
            let cu = &self.units[c as usize];
            if cu.ai == NIL {
                continue;
            }
            if cu.status & 4 == 0 && (wide || self.units[u as usize].status & 0x8000 != 0) {
                continue;
            }
            if self.is_enemy(c, u)
                && self.units[c as usize].auto_target == NIL
                && self.can_attack(d, c, u, true)
                && (self.units[c as usize].status & 0x2_0000 != 0 || self.can_hit_now(d, c, u))
            {
                todo_port!("0x43f320", "computer unit reacts to a nearby target");
            }
        }
    }

    /// 0x43ff90: acid spore / interceptor timer work every order-process tick.
    fn order_timer_tick(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.acid_spore_count > 3 {
            self.ai_alert_wide(d, u);
            return;
        }
        if self.var_6957d0 == 0 && un.unit_type == 0x49 && un.ptr_at(0xc0) != NIL {
            self.ai_alert_wide(d, u);
            self.var_6957d0 = 1;
        }
    }

    /// 0x43fe50: computer units within 0x280, then (wide) within 0x500, react to `u`.
    fn ai_alert_wide(&mut self, d: &Data, u: u32) {
        self.ai_alert_around(d, u, 0x280, false);
        self.ai_alert_around(d, u, 0x500, true);
    }

    /// 0x4777f0: TurretGuard.
    fn order_turret_guard(&mut self, d: &Data, u: u32) {
        let p = self.units[self.units[u as usize].subunit as usize].fl.next_target_waypoint;
        if self.units[u as usize].fl.next_target_waypoint != p {
            self.units[u as usize].fl.next_target_waypoint = p;
        }
        self.order_player_guard(d, u);
    }

    /// 0x477980: TurretAttack: aim at and fire on the order target while the base
    /// allows it; back to TurretGuard (4) when the target is gone or out of reach.
    fn order_turret_attack(&mut self, d: &Data, u: u32) {
        self.pick_better_target(d, u);
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        }
        if !self.can_attack(d, u, t, true) || !self.target_visible(t, u) {
            self.units[u as usize].order_flags |= 1;
            // 0x474810 inlined: drop interruptible orders and TurretGuards, queue one.
            self.issue_order(d, u, 4, true, (0, 0), NIL, 0xe4);
            self.order_nothing(d, u);
            return;
        }
        let base = self.units[u as usize].subunit;
        if self.units[u as usize].unit_type == 6 && self.units[base as usize].ai != NIL {
            todo_port!("0x477b0e", "AI siege tank sieges up for its turret's target");
        }
        if !self.target_in_reach(d, u, t) {
            let bt = self.units[base as usize].unit_type as usize;
            if self.units[base as usize].order != d.dat.units.ai_attack_unit[bt] || self.right_click_kind(d, base) == 3 {
                self.start_order_now(d, u, 4);
                return;
            }
        }
        let p = self.units[u as usize].order_target_pos;
        if self.units[u as usize].fl.next_target_waypoint != p {
            self.units[u as usize].fl.next_target_waypoint = p;
        }
        if self.units[base as usize].fl.flags & 2 != 0 {
            return;
        }
        self.fire(d, u, 5);
    }


    /// 0x4673d0: Repair: SCVs walk to a damaged mechanical Terran unit and pay for each
    /// repair tick (+0xcc counts the frames until the next payment).
    fn order_repair(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        let stop = t == NIL || {
            let tu = &self.units[t as usize];
            let tt = tu.unit_type as usize;
            tu.hp >= d.dat.units.hit_points[tt] as i32
                || tu.stasis_timer != 0
                || (self.units[u as usize].ai != NIL && tu.status & 0x2_0000 != 0 && tu.order_target_unit != u)
                || tu.status & 0x40 != 0
                || d.dat.units.flags[tt] & 0x4000_0000 == 0
                || tu.status & 1 == 0
        };
        if stop {
            self.repair_finish(d, u, t);
            return;
        }
        let g = d.dat.units.staredit_group_flags[self.units[t as usize].unit_type as usize];
        if g & 5 != 0 || g & 2 == 0 {
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xc);
            self.order_done_or_next(d, u);
            return;
        }
        let s = self.units[u as usize].sprite;
        let tp = self.unit_sprite_pos(t);
        let face = |st: &mut State| {
            let f = &mut st.units[u as usize].fl;
            if f.next_target_waypoint != tp {
                f.next_target_waypoint = tp;
            }
            // 0x479fe0
            st.units[u as usize].order_target_unit = t;
            st.units[u as usize].order_target_pos = tp;
        };
        match self.units[u as usize].order_state {
            0 | 1 => {
                if self.units[u as usize].order_state == 0 {
                    // 0x466d10 / 0x466ee0: can the first tick be paid (error otherwise).
                    let (m, g, _) = self.repair_tick_cost(d, t);
                    let p = self.units[u as usize].player;
                    let race = self.player_races[self.local_player as usize] as u16;
                    if self.players.minerals[p as usize] < m {
                        self.local_error(p, 0x93 + race, 0x352);
                    } else if self.players.gas[p as usize] < g {
                        self.local_error(p, 0x96 + race, 0x353);
                    }
                    self.units[u as usize].set_u16_at(0xcc, 0);
                    self.units[u as usize].order_state = 1;
                }
                if self.units[t as usize].status & 1 != 0 {
                    if !self.move_to_unit_aim(d, u, t) {
                        return;
                    }
                    self.units[u as usize].order_target_unit = t;
                    self.units[u as usize].order_target_pos = tp;
                    self.units[u as usize].order_state = 6;
                } else {
                    self.order_with_target(d, u, 0x21, t);
                }
                return;
            }
            6 => {
                if self.unit_at_move_target(u) == 0 {
                    self.follow_move(d, u, t);
                    return;
                }
                if !self.box_distance_within(d, u, 5, t) {
                    self.units[u as usize].order_state = 1;
                    return;
                }
                face(self);
                self.units[u as usize].order_state = 7;
            }
            7 => {
                if !self.box_distance_within(d, u, 5, t) {
                    self.units[u as usize].order_state = 1;
                    return;
                }
                face(self);
                let f = &self.units[u as usize].fl;
                let facing = f.next_target_waypoint == f.pos || {
                    let sp = (self.sprites[s as usize].x, self.sprites[s as usize].y);
                    let dir = crate::game::flingy::direction_to(sp.0 as i32, sp.1 as i32, f.next_target_waypoint.0 as i32, f.next_target_waypoint.1 as i32);
                    let mut diff = f.heading as i32 - dir as i32;
                    if diff < 0 {
                        diff += 0x100;
                    }
                    if diff > 0x80 {
                        diff = 0x100 - diff;
                    }
                    let w = self.ground_weapon(d, u);
                    diff <= d.dat.weapons.attack_angle[w as usize] as i32
                };
                if facing {
                    self.sprite_images_run_anim(d, s, 0xf);
                    self.units[u as usize].order_state = 8;
                }
            }
            8 => {
                face(self);
                if !self.box_distance_within(d, u, 5, t) || self.units[t as usize].order == 0x4a {
                    self.sprite_images_run_anim(d, s, 8);
                    self.units[u as usize].order_state = 1;
                }
            }
            _ => return,
        }
        let timer = self.units[u as usize].u16_at(0xcc);
        self.units[u as usize].set_u16_at(0xcc, timer.wrapping_sub(1));
        if timer == 0 && !self.repair_pay(d, u, t) {
            self.sprite_images_run_anim(d, s, 0xc);
            self.order_done_or_next(d, u);
            return;
        }
        let mut gain = self.units[t as usize].hp_gain as i32;
        if self.game_options & 2 != 0 {
            gain = (gain << 4) & 0xffff;
        }
        let hp = self.units[t as usize].hp + gain;
        self.set_unit_hp(d, t, hp);
        if self.units[t as usize].hp >= d.dat.units.hit_points[self.units[t as usize].unit_type as usize] as i32 {
            self.repair_finish(d, u, t);
        }
    }

    /// The end of Repair (0x4673d0): stop the repair animation, next order or idle, and
    /// the target forgets its repairer (+0x80).
    fn repair_finish(&mut self, d: &Data, u: u32, t: u32) {
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0xc);
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.order_nothing(d, u);
        } else {
            let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
            self.start_order_now(d, u, o);
        }
        if t != NIL && self.units[t as usize].status & 0x40 == 0 {
            self.units[t as usize].connected_unit = NIL;
        }
    }

    /// 0x466d10: one repair tick of `t`: (minerals, gas, frames until the next tick).
    fn repair_tick_cost(&self, d: &Data, t: u32) -> (i32, i32, i16) {
        let tt = self.units[t as usize].unit_type as usize;
        let m = d.dat.units.mineral_cost[tt] as u32;
        let g = d.dat.units.gas_cost[tt] as u32;
        let small = if m == 0 || g == 0 { m.max(g) } else { m.min(g) };
        if small == 0 {
            return (0, 0, -1);
        }
        let max = d.dat.units.hit_points[tt] as i32;
        let cheat = self.game_options & 2 != 0;
        let gain = |v: u32| if cheat { (v << 4) & 0xffff } else { v };
        let hg = self.units[t as usize].hp_gain as u32;
        let time = ((max * 3) / (gain(hg) * small) as i32) as i16;
        if time == 0 {
            let per = |c: u32| if c == 0 { 0 } else { ((gain(hg) * c) as i32 / (max * 3)).max(1) };
            (per(m), per(g), 1)
        } else if small == m {
            ((1), (g / m) as i32, time)
        } else {
            ((m / g) as i32, 1, time)
        }
    }

    /// 0x466fb0: pay the next repair tick, or false when the player can't.
    fn repair_pay(&mut self, d: &Data, u: u32, t: u32) -> bool {
        let (m, g, time) = self.repair_tick_cost(d, t);
        let p = self.units[u as usize].player as usize;
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[p] < m {
            self.local_error(p as u8, 0x93 + race, 0x352);
            return false;
        }
        if self.players.gas[p] < g {
            self.local_error(p as u8, 0x96 + race, 0x353);
            return false;
        }
        self.units[u as usize].set_u16_at(0xcc, time as u16);
        self.players.minerals[p] -= m;
        self.players.gas[p] -= g;
        true
    }






    /// 0x468e80: Harvest3.
    fn order_harvest3(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].u8_at(0xce) != 0 {
            let g = self.units[u as usize].ptr_at(0xd0);
            self.release_mineral_pub(d, u, g);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x468ed0: Harvest4.
    fn order_harvest4(&mut self, d: &Data, u: u32) {
        let r = self.units[u as usize].ptr_at(0xd0);
        if r != super::list::NIL {
            self.leave_wait_list(u, r);
        }
        self.order_next_or_idle(d, u);
    }



    /// 0x4671b0: ResetCollision.
    pub(super) fn order_reset_collision(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        let ut = self.units[u as usize].unit_type as usize;
        self.sprites[s as usize].elevation = d.dat.units.elevation[ut];
        let st = &mut self.units[u as usize].status;
        *st = (*st & 0xff5f_ffff) | 0x10_0000;
        self.update_unit_overlaps(d, u);
        self.path_delete(u);
        self.units[u as usize].movement_state = 0;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let pf = &mut self.units[u as usize].pathing_flags;
        *pf = (*pf & !1) | low;
        if self.units[u as usize].order_queue_head == super::list::NIL {
            let idle = d.dat.units.ai_return_to_idle[ut];
            self.issue_order(d, u, idle, false, (0, 0), super::list::NIL, 0xe4);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x42e320: ResetHarvestCollision.
    pub(super) fn order_reset_harvest_collision(&mut self, d: &Data, u: u32) {
        let head = self.units[u as usize].order_queue_head;
        if head != super::list::NIL && matches!(self.orders[head as usize].order, 0x4f | 0x51 | 0x52 | 0x53 | 0x55 | 0x56 | 0x57 | 0x59 | 0x58) {
            // Still harvesting: keep this order queued behind the harvest orders.
            if self.units[u as usize].order != 0 {
                loop {
                    let t = self.units[u as usize].order_queue_tail;
                    if t == super::list::NIL || self.orders[t as usize].order != 0x97 {
                        break;
                    }
                    self.order_remove(d, u, t);
                }
                self.order_queue_append(d, u, 0x97, 0xe4, (0, 0), super::list::NIL);
            }
            self.order_next_or_idle(d, u);
            return;
        }
        self.units[u as usize].status &= 0xff5f_ffff;
        self.update_unit_overlaps(d, u);
        if self.units[u as usize].order_queue_head == super::list::NIL {
            let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, idle, false, (0, 0), super::list::NIL, 0xe4);
        }
        self.path_delete(u);
        self.units[u as usize].movement_state = 0;
        let s = self.units[u as usize].sprite;
        let low = (self.sprites[s as usize].elevation < 0xc) as u8;
        let pf = &mut self.units[u as usize].pathing_flags;
        *pf = (*pf & !1) | low;
        self.order_next_or_idle(d, u);
    }

    /// 0x475420: clear 0x1000 and start the next queued order, else idle.
    pub fn order_next_or_idle(&mut self, d: &Data, u: u32) {
        self.units[u as usize].status &= !0x1000;
        if self.units[u as usize].order_queue_head != super::list::NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
            return;
        }
        let o = if self.units[u as usize].ai != super::list::NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
        self.order_now(d, u, o);
    }

    /// 0x4e3fb0: CTFCOP2.
    fn order_ctfcop2(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e3fb0", "CTFCOP2");
    }

    /// 0x475710: SelfDestructing.
    fn order_self_destructing(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x475710", "SelfDestructing");
    }

    /// 0x47c3c0: Critter.
    fn order_critter(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let dir = self.rng.rand_between(0x30, 0, 0xff) as usize;
                let ut = self.units[u as usize].unit_type;
                let dist = if ut == 0x5e { self.rng.rand_between(0x30, 0xa0, 0x280) } else { 0x20 };
                let s = self.units[u as usize].sprite as usize;
                let (tx, ty) = crate::game::flingy::direction_table()[dir];
                let mut p = (
                    ((tx.wrapping_mul(dist) >> 8) as i16).wrapping_add(self.sprites[s].x),
                    ((ty.wrapping_mul(dist) >> 8) as i16).wrapping_add(self.sprites[s].y),
                );
                self.clamp_unit_pos(d, ut, &mut p);
                self.units[u as usize].order_target_pos = p;
                self.set_move_target_xy(d, u, p);
                self.units[u as usize].order_state = 1;
            }
            1 => {
                let un = &self.units[u as usize];
                let s = &self.sprites[un.sprite as usize];
                if un.fl.move_target != (s.x, s.y) {
                    return;
                }
                if un.unit_type == 0x5e {
                    self.units[u as usize].order_state = 0;
                    return;
                }
                if self.unit_at_move_target(u) == 2 {
                    let un = &mut self.units[u as usize];
                    un.main_order_timer = 0x4b;
                    un.order_state = 2;
                    return;
                }
                let v = self.rng.rand_between(0x2f, 0, 0x4b);
                let un = &mut self.units[u as usize];
                un.order_state = 2;
                un.main_order_timer = if v <= 0xf { 0 } else { v as u8 };
            }
            2 => {
                if self.units[u as usize].main_order_timer == 0 {
                    self.units[u as usize].order_state = 0;
                }
            }
            _ => {}
        }
    }


    /// 0x47bba0: Stop.
    fn order_stop(&mut self, d: &Data, u: u32) {
        self.unit_halt_facing(d, u);
        self.order_start(d, u);
        self.order_done_or_next(d, u);
    }

    /// 0x475b90: Guard.
    fn order_guard(&mut self, d: &Data, u: u32) {
        let t = self.rng.rand_between(0x1d, 0, 0xf) as u8;
        self.units[u as usize].main_order_timer = t;
        let un = &self.units[u as usize];
        if self.player_type(un.player) == 1 {
            if un.ai == super::list::NIL {
                if d.dat.units.flags[un.unit_type as usize] & 0x40 == 0 {
                    self.units[u as usize].order = 0x9c;
                    return;
                }
            } else {
                self.units[u as usize].order = 0x9c;
                todo_port!("0x475b90", "AI unit type 1 -> GuardPost (0xa0) (needs AI structs)");
                return;
            }
        }
        self.units[u as usize].order = 3;
    }

    /// 0x4774a0: PlayerGuard.
    fn order_player_guard(&mut self, d: &Data, u: u32) {
        let found = self.attack_auto_target(d, u);
        if found || self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let t = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[t] & 0x10 != 0 {
            let p = self.units[u as usize].subunit;
            let w = self.units[p as usize].fl.next_target_waypoint;
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != w {
                f.next_target_waypoint = w;
            }
        }
        if self.acquisition_range(u) as u8 != 0 {
            let t = self.find_auto_target(d, u);
            if t != super::list::NIL {
                self.order_attack(d, u, t, true);
            }
        }
    }

    /// 0x47c950: Move, ReaverCarrierMove.
    fn order_move(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        match un.order_state {
            0 => {
                let ok = if un.order_target_unit == super::list::NIL {
                    let p = un.order_target_pos;
                    let ok = self.set_move_target_xy(d, u, p);
                    let f = &mut self.units[u as usize].fl;
                    if f.next_target_waypoint != p {
                        f.next_target_waypoint = p;
                    }
                    ok
                } else {
                    let t = un.order_target_unit;
                    self.move_to_unit_aim(d, u, t)
                };
                if ok {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                if self.unit_at_move_target(u) != 0 {
                    self.order_done_or_next(d, u);
                    if self.units[u as usize].ai != super::list::NIL {
                        self.units[u as usize].auto_target = super::list::NIL;
                    }
                }
            }
            _ => {}
        }
    }

    /// 0x4f6fa0: Attack2, MoveToInfest, MoveToRepair, CarrierMoveToAttack, ReaverMoveToAttack, Harvest2, MoveToFireYamatoGun.
    fn order_attack2(&mut self, d: &Data, u: u32) {
        self.order_obscured(d, u);
    }

    /// 0x477d00: AttackFixedRange.
    fn order_attack_fixed_range(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ai == NIL {
            self.pick_better_target(d, u);
        } else {
            let t = self.find_auto_target(d, u);
            self.units[u as usize].order_target_unit = t;
        }
        let t = self.units[u as usize].order_target_unit;
        if t != NIL {
            self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        }
        if t != NIL && self.can_attack(d, u, t, true) {
            if self.can_hit_now(d, u, t) {
                let un = &mut self.units[u as usize];
                if un.fl.next_target_waypoint != un.order_target_pos {
                    un.fl.next_target_waypoint = un.order_target_pos;
                }
                self.fire(d, u, 5);
            } else if self.units[u as usize].ai != NIL {
                // 0x475260: AI units attack-move there instead.
                let p = self.units[u as usize].order_target_pos;
                self.order_issue_now(d, u, 0xe, p, NIL, 0xe4);
            } else {
                self.order_done_or_next(d, u);
            }
            return;
        }
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_now(d, u, o);
        }
    }

    /// 0x479040: AttackMove.
    fn order_attack_move(&mut self, d: &Data, u: u32) {
        let order = self.units[u as usize].order;
        if !self.attack_move_step(d, u, order) || self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        // (The status test that follows is always true in 1.16.1: setne + inc.)
        if self.units[u as usize].fl.move_target == self.unit_sprite_pos(u) {
            self.order_done_or_next(d, u);
            return;
        }
        self.attack_move_acquire(d, u, order);
    }

    /// 0x478370: move to the order position (state 0 -> 1), and stop to attack the
    /// auto target when it is a visible enemy. `order` is pushed back to resume after.
    /// False when the unit stopped to attack or has not started moving.
    fn attack_move_step(&mut self, d: &Data, u: u32, order: u8) -> bool {
        if self.units[u as usize].order_state == 0 {
            let p = self.units[u as usize].order_target_pos;
            let r = self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
            if !r {
                return false;
            }
            self.units[u as usize].order_state = 1;
        }
        let t = self.units[u as usize].auto_target;
        if t == NIL
            || !self.is_enemy(u, t)
            || self.sprites[self.units[t as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0
            || self.units[t as usize].status & 0x400_0000 != 0
            || self.cloaked_from(t, u)
        {
            return true;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x477160", "AI attack-move target choice");
            return true;
        }
        self.attack_move_switch(d, u, order, t, true);
        false
    }

    /// 0x477820: while attack-moving, stop for the best enemy in acquisition range.
    fn attack_move_acquire(&mut self, d: &Data, u: u32, order: u8) {
        if self.acquisition_range(u) as u8 == 0 {
            return;
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x477160", "AI attack-move target choice");
            return;
        }
        let t = self.find_auto_target(d, u);
        if t != NIL {
            self.attack_move_switch(d, u, order, t, false);
        }
    }

    /// The common tail of 0x478370 / 0x477820: halt, queue `order` (to resume) and the
    /// unit's attack order on `t` in front (0x474af0, 0x474540), start it (0x475420) and
    /// mark the unit (and subunit) as auto-targeting (0x4026a0). 0x478370 also forgets
    /// the auto target first.
    fn attack_move_switch(&mut self, d: &Data, u: u32, order: u8, t: u32, clear_auto: bool) {
        self.unit_halt(d, u);
        let (pos, target) = (self.units[u as usize].order_target_pos, self.units[u as usize].order_target_unit);
        self.queue_front(d, u, order, pos, target);
        let attack = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
        let head = self.units[u as usize].order_queue_head;
        let tp = self.unit_sprite_pos(t);
        self.order_insert_before(d, u, attack, 0xe4, tp, t, head);
        if clear_auto {
            self.units[u as usize].auto_target = NIL;
        }
        self.order_next_or_idle(d, u);
        self.units[u as usize].status |= 0x800_0000;
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            self.units[sub as usize].status |= 0x800_0000;
        }
    }

    /// 0x4ea4c0: InfestedCommandCenter: the command center (order target: the queen
    /// inside it) turns into its infested type after 3 frames and joins the queen's owner.
    fn order_infested_command_center(&mut self, d: &Data, u: u32) {
        let q = self.units[u as usize].order_target_unit;
        if q != NIL && self.damaged_6a(d, u) {
            if self.units[u as usize].order_state == 0 {
                self.units[u as usize].main_order_timer = 3;
                self.units[u as usize].order_state = 2;
                return;
            }
            let t = self.units[u as usize].main_order_timer;
            self.units[u as usize].main_order_timer = t.wrapping_sub(1);
            if t != 0 {
                return;
            }
            if self.units[u as usize].ptr_at(0xc0) != NIL {
                self.detach_addon(d, u);
            }
            if super::command::has_rally(self.units[u as usize].unit_type) {
                self.units[u as usize].rally_unit = u;
            }
            let new_type = d.dat.units.infestation[self.units[u as usize].unit_type as usize];
            let qp = self.units[q as usize].player;
            self.count_death(d, u, qp);
            self.modify_unit_counters(d, u, -1);
            if self.units[u as usize].status & 1 != 0 {
                self.modify_completed_counters(d, u, -1, false);
            }
            self.units[u as usize].unit_type = new_type;
            self.modify_unit_counters(d, u, 1);
            if self.units[u as usize].status & 1 != 0 {
                self.modify_completed_counters(d, u, 1, false);
            }
            let owner = self.units[q as usize].player;
            self.change_owner(d, u, owner, true);
            self.set_unit_colors(d, u, owner);
            let un = &mut self.units[u as usize];
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
            un.secondary_order = 0x17;
            self.clear_build_queue(d, u);
            while self.units[u as usize].order_queue_head != NIL {
                let n = self.units[u as usize].order_queue_head;
                self.order_remove(d, u, n);
            }
            if self.units[u as usize].status & 2 != 0 {
                self.unit_anim(d, u, 0x14);
            }
            let c = d.dat.units.construction_image[new_type as usize] as u16;
            if c < 999 {
                let s = self.units[u as usize].sprite;
                // 0x498e00: before the main image.
                let main = self.sprites[s as usize].main_image;
                self.create_image(d, s, main, c, 0, 0, 0);
            }
            let hp = d.dat.units.hit_points[new_type as usize] as i32;
            self.set_unit_hp(d, u, hp);
            self.set_button_set(d, u, new_type);
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8 / 0x68c1ec: UI refresh flags.
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x476f50: TowerGuard.
    fn order_tower_guard(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 || un.main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let t = self.find_target_in_range(d, u);
        self.units[u as usize].order_target_unit = t;
        if t != NIL {
            self.order_with_target(d, u, 0x13, t);
            self.units[u as usize].status |= 0x800_0000;
            let sub = self.units[u as usize].subunit;
            if sub != NIL {
                self.units[sub as usize].status |= 0x800_0000;
            }
        }
    }

    /// 0x479150: TowerAttack.
    fn order_tower_attack(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let free = un.status & 0x400 == 0 && un.lockdown_timer == 0 && un.stasis_timer == 0 && un.maelstrom_timer == 0;
        if free && self.attack_in_place(d, u) {
            let p = self.unit_sprite_pos(u);
            self.set_move_target_xy(d, u, p);
            let un = &mut self.units[u as usize];
            if un.fl.next_target_waypoint != un.order_target_pos {
                un.fl.next_target_waypoint = un.order_target_pos;
            }
            return;
        }
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            self.prepare_next_order(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_now(d, u, idle);
        }
    }

    /// 0x47c4f0: StayInRange.
    fn order_stay_in_range(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            self.unit_halt_facing(d, u);
            self.order_done_or_next(d, u);
            return;
        }
        self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
        if !self.can_attack(d, u, t, true) {
            let un = &self.units[u as usize];
            if un.status & 0x800_0000 == 0 && un.ai == NIL {
                let p = un.order_target_pos;
                self.order_queue_front(d, u, 6, p);
            }
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].order_state != 0 {
            if self.target_in_reach(d, u, t) {
                self.unit_halt_facing(d, u);
            } else if !self.chase(d, u, t) {
                return;
            }
        } else {
            if !self.target_in_reach(d, u, t) && !self.move_to_unit(d, u, t) {
                return;
            }
            self.units[u as usize].order_state = 1;
        }
        // Goliaths and tanks turn the body toward a target more than 0x1f off.
        let un = &self.units[u as usize];
        if matches!(un.unit_type, 3 | 0x11) && un.fl.flags & 2 == 0 {
            let s = &self.sprites[un.sprite as usize];
            let tp = un.order_target_pos;
            let dir = crate::game::flingy::direction_to(s.x as i32, s.y as i32, tp.0 as i32, tp.1 as i32);
            let mut diff = dir.wrapping_sub(un.fl.heading) as i8;
            if diff < -0x7f {
                diff = -0x80;
            }
            if (diff as i32).abs() >= 0x20 {
                self.set_target_waypoint(u, tp);
            }
        }
    }


    /// 0x4e98e0: CastInfestation: the queen walks to a damaged command center and then
    /// enters it (InfestingCommandCenter).
    fn order_cast_infestation(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        match self.units[u as usize].order_state {
            0 => {
                if t != NIL && self.damaged_6a(d, t) {
                    if self.move_to_unit_aim(d, u, t) {
                        self.units[u as usize].order_state = 1;
                    }
                    return;
                }
                // 0x48ef30: error 0x365 for the local player (UI).
                self.order_done_or_next(d, u);
            }
            1 => {
                let un = &self.units[u as usize];
                let s = &self.sprites[un.sprite as usize];
                if un.fl.move_target != (s.x, s.y) {
                    if t != NIL {
                        self.follow_move(d, u, t);
                    }
                    return;
                }
                let stopped_by_collision = un.status & 0x80000 != 0;
                if t != NIL && self.damaged_6a(d, t) && !stopped_by_collision {
                    self.order_on(d, u, 0x1d, t);
                } else {
                    self.order_done_or_next(d, u);
                }
            }
            _ => {}
        }
    }

    /// 0x4ea290: InfestingCommandCenter: the queen hides inside the command center for
    /// 0x3c frames, then comes back out next to it.
    pub(super) fn order_infesting_command_center(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        match self.units[u as usize].order_state {
            0 => {
                if t != NIL && self.damaged_6a(d, t) && self.units[t as usize].order != 0xf {
                    self.hide_unit(d, u);
                    self.order_on(d, t, 0xf, u);
                    self.units[u as usize].order_state = 3;
                    self.units[u as usize].main_order_timer = 0x3c;
                    return;
                }
                self.unit_halt(d, u);
                let f = &mut self.units[u as usize].fl;
                if f.move_target != f.next_target_waypoint {
                    f.next_target_waypoint = f.move_target;
                }
                if self.units[u as usize].order_queue_head == NIL {
                    let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                    self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
                }
            }
            3 => {
                let tm = self.units[u as usize].main_order_timer;
                if tm != 0 {
                    self.units[u as usize].main_order_timer = tm - 1;
                    return;
                }
                let s = self.units[u as usize].sprite;
                let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
                let Some(p) = self.find_unit_placement(d, u, p, None, false) else {
                    return;
                };
                self.set_unit_position(d, u, p.0, p.1);
                self.unit_halt_facing(d, u);
                self.show_unit(d, u);
                if self.units[u as usize].order_queue_head == NIL {
                    let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                    self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
                }
            }
            _ => return,
        }
        self.order_next_or_idle(d, u);
    }


    /// 0x45d0d0: ZergBuildingMorph: pay for the queued building, show its construction
    /// image and continue with IncompleteMorphing.
    fn order_zerg_building_morph(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let t = un.build_queue[un.build_queue_slot as usize % 5] as usize;
        let p = un.player as usize;
        // 0x6ca51c / 0x6ca4ec: the cost being checked.
        let (m, g) = (d.dat.units.mineral_cost[t] as i32, d.dat.units.gas_cost[t] as i32);
        if self.supply_ok_show(d, p, t as u16) {
            let race = self.player_races[self.local_player as usize] as u16;
            if self.players.minerals[p] < m {
                // 0x48ee30: not enough minerals.
                self.local_error(p as u8, 0x93 + race, 0x352);
            } else if self.players.gas[p] < g {
                self.local_error(p as u8, 0x96 + race, 0x353);
            } else {
                self.players.minerals[p] -= m;
                self.players.gas[p] -= g;
                let un = &mut self.units[u as usize];
                un.status &= !1;
                let free = un.status & 0x400 == 0 && un.lockdown_timer == 0 && un.stasis_timer == 0 && un.maelstrom_timer == 0;
                if free || d.dat.units.flags[un.unit_type as usize] & 1 != 0 {
                    un.button_set = 0xec;
                }
                self.reset_unit_image(d, u, true);
                let un = &mut self.units[u as usize];
                un.remaining_build_time = d.dat.units.build_time[t];
                un.order_flags |= 1;
                if un.order != 0 {
                    loop {
                        let h = self.units[u as usize].order_queue_head;
                        if h == NIL {
                            break;
                        }
                        let o = self.orders[h as usize].order;
                        if d.dat.orders.interruptible[o as usize] == 0 && o != 0x2d {
                            break;
                        }
                        self.order_remove(d, u, h);
                    }
                    self.order_queue_append(d, u, 0x2d, 0xe4, (0, 0), NIL);
                }
                self.prepare_next_order(d, u);
                let s = self.units[u as usize].sprite;
                let mut i = self.sprites[s as usize].images.head;
                while i != NIL {
                    let next = self.images[i as usize].link.next;
                    self.image_run_anim(d, i, 0xd);
                    i = next;
                }
                // 0x68c1e8 etc.: button refresh (UI).
                return;
            }
        }
        let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
    }

    /// 0x45dc20: BuildNydusExit.
    fn order_build_nydus_exit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x45dc20", "BuildNydusExit");
    }

    /// inline: EnterNydusCanal.
    fn order_enter_nydus_canal(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "EnterNydusCanal");
    }

    /// 0x47c7b0: Follow: keep close to the order target; enemies become a Guard order
    /// (attack), a nydus canal / shield battery target hands over to their orders.
    fn order_follow(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.order_nothing(d, u);
                return;
            }
            let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
            self.start_order_now(d, u, o);
            return;
        }
        if self.units[u as usize].ai != NIL {
            // AI units stop following when at full hit points.
            let un = &self.units[u as usize];
            let mut max = d.dat.units.hit_points[un.unit_type as usize] as i32 >> 8;
            if max == 0 {
                max = (un.hp + 0xff) >> 8;
                if max == 0 {
                    max = 1;
                }
            }
            if (un.hp + 0xff) >> 8 == max {
                self.order_done_or_next(d, u);
                return;
            }
        }
        if self.units[u as usize].order_state == 0 {
            if !self.move_to_unit_aim(d, u, t) {
                return;
            }
            match self.units[t as usize].unit_type {
                0x86 => todo_port!("0x4e8c20", "following onto a nydus canal: EnterNyduscanal (0x2f)"),
                0xac => {
                    if self.follow_onto_battery(d, u, t) {
                        return;
                    }
                }
                _ => {}
            }
            self.units[u as usize].order_state = 1;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 7;
        // 0x47b910: not allied.
        if self.is_enemy(u, t) && self.can_attack(d, u, t, true) {
            self.start_order_now(d, u, 2);
            return;
        }
        let air = self.units[u as usize].status & 4 != 0;
        // 0x49cbb0: both in the same region group.
        let same_group = || {
            let (a, b) = (self.unit_sprite_pos(u), self.unit_sprite_pos(t));
            let ra = super::path::region_at(&self.sai, a.0 as i32, a.1 as i32);
            let rb = super::path::region_at(&self.sai, b.0 as i32, b.1 as i32);
            self.sai.regions[ra as usize].group == self.sai.regions[rb as usize].group
        };
        if air || same_group() {
            // 0x47b490: the target's owner is a rescuable or neutral (type 3 / 4) player.
            let tt = self.player_type(self.units[t as usize].player);
            if (tt == 3 || tt == 4) && !self.box_distance_within(d, u, 1, t) {
                self.move_to_unit_aim(d, u, t);
                return;
            }
            if !self.box_distance_within(d, u, 0x60, t) {
                self.follow_move(d, u, t);
                return;
            }
            if self.units[t as usize].status & 2 != 0 || self.units[u as usize].order_queue_head != NIL {
                self.order_done_or_next(d, u);
                return;
            }
        }
        self.unit_halt_facing(d, u);
    }

    /// 0x475310 with order `o`: issue it now (no target) and start it.
    pub(super) fn start_order_now(&mut self, d: &Data, u: u32, o: u8) {
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
        self.order_nothing(d, u);
    }

    /// 0x4eb900: follow `t` while it is in sight (within 0x1c0 when not seen),
    /// unless the unit just got an order (+0xe3).
    pub(super) fn follow_move(&mut self, d: &Data, u: u32, t: u32) -> bool {
        if self.units[u as usize].recent_order_timer != 0 {
            return true;
        }
        let bit = 1u8 << (self.units[u as usize].player & 0x1f);
        let seen = self.sprites[self.units[t as usize].sprite as usize].visibility & bit != 0;
        if !seen && !self.box_distance_within(d, u, 0x1c0, t) {
            if self.unit_at_move_target(u) != 1 {
                return true;
            }
            self.order_done_or_next(d, u);
            return false;
        }
        self.move_to_unit(d, u, t)
    }















    /// 0x4e7b70: Pickup4.
    fn order_pickup4(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e7b70", "Pickup4");
    }

    /// 0x47bab0: WatchTarget.
    fn order_watch_target(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].order_target_unit;
        if t != NIL && self.can_attack(d, u, t, true) {
            if self.target_in_reach(d, u, t) || self.right_click_kind(d, u) != 3 {
                return;
            }
        }
        // 0x475350
        if self.units[u as usize].order_queue_head != NIL {
            self.order_nothing(d, u);
        } else if self.units[u as usize].ai != NIL {
            self.order_now(d, u, 0x9c);
        } else {
            let p = self.units[u as usize].order_target_pos;
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.order_at(d, u, o, p);
        }
    }

    /// inline: InitCreepGrowth.
    fn order_init_creep_growth(&mut self, d: &Data, u: u32) {
        self.creep_growth_order(d, u, 0x66);
    }

    /// Inline in 0x4ec4d0 (orders 0x65 / 0x67): go idle and set the creep
    /// secondary order.
    fn creep_growth_order(&mut self, d: &Data, u: u32, secondary: u8) {
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].ai == super::list::NIL {
            let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, idle, true, (0, 0), super::list::NIL, 0xe4);
        } else if self.units[u as usize].order != 0 {
            loop {
                let t = self.units[u as usize].order_queue_tail;
                if t == super::list::NIL {
                    break;
                }
                let o = self.orders[t as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != 0x9c {
                    break;
                }
                self.order_remove(d, u, t);
            }
            self.order_queue_append(d, u, 0x9c, 0xe4, (0, 0), super::list::NIL);
        }
        self.prepare_next_order(d, u);
        self.set_secondary_order(u, secondary);
    }

    /// inline: StoppingCreepGrowth.
    fn order_stopping_creep_growth(&mut self, d: &Data, u: u32) {
        self.creep_growth_order(d, u, 0x17);
    }

    /// 0x478d10: HoldPosition.
    fn order_hold_position(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            self.unit_halt(d, u);
            let un = &mut self.units[u as usize];
            if un.fl.next_target_waypoint != un.fl.move_target {
                un.fl.next_target_waypoint = un.fl.move_target;
            }
            un.status |= 0x800_0000;
            let sub = un.subunit;
            if sub != NIL {
                self.units[sub as usize].status |= 0x800_0000;
            }
            self.units[u as usize].order_state = 1;
        }
        if !self.attack_in_place(d, u) {
            let un = &mut self.units[u as usize];
            if un.main_order_timer == 0 {
                un.main_order_timer = 0xf;
                let t = self.find_target_in_range(d, u);
                let un = &mut self.units[u as usize];
                un.order_target_unit = t;
                if t != NIL {
                    un.order_process_timer = 0;
                }
            }
            return;
        }
        let un = &self.units[u as usize];
        let sub = un.subunit;
        let turret = sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0;
        if !turret || un.unit_type == 3 || un.unit_type == 0x11 {
            let un = &mut self.units[u as usize];
            if un.fl.next_target_waypoint != un.order_target_pos {
                un.fl.next_target_waypoint = un.order_target_pos;
            }
        }
    }

    /// 0x4eb5b0: QueenHoldPosition, SuicideHoldPosition.
    fn order_queen_hold_position(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            self.unit_halt_facing(d, u);
            self.units[u as usize].order_state = 1;
        }
        self.order_nothing(d, u);
    }

    /// 0x475000: Nothing.
    pub(super) fn order_nothing(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head != super::list::NIL {
            self.prepare_next_order(d, u);
        }
    }




    /// 0x4788e0: SuicideUnit.
    fn order_suicide_unit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4788e0", "SuicideUnit");
    }

    /// 0x478a40: SuicideLocation.
    fn order_suicide_location(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478a40", "SuicideLocation");
    }

    /// 0x464e40: CastScannerSweep: pay the energy and put a scanner sweep (0x21) at the
    /// order position.
    fn order_cast_scanner_sweep(&mut self, d: &Data, u: u32) {
        let cost = d.dat.techdata.energy_cost[4] as u8 as u16;
        let cheat = self.game_options & 0x800 != 0;
        if !cheat && self.units[u as usize].energy < cost << 8 {
            // 0x48ccb0: "not enough energy" for the local player (UI).
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.order_nothing(d, u);
            } else {
                let o = if self.units[u as usize].ai != NIL { 0x9c } else { d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize] };
                self.order_now(d, u, o);
            }
            return;
        }
        let mut p = self.units[u as usize].order_target_pos;
        self.clamp_unit_pos(d, 0x21, &mut p);
        let player = self.units[u as usize].player;
        let Some(s) = self.create_unit(d, 0x21, p.0, p.1, player) else {
            self.show_last_error(player);
            self.order_done_or_next(d, u);
            return;
        };
        self.finish_unit_pre(d, s);
        self.finish_unit_post(d, s);
        // 0x48ed50: the sweep sound (UI).
        if !cheat {
            let e = &mut self.units[u as usize].energy;
            *e = e.wrapping_sub(cost << 8);
        }
        self.order_done_or_next(d, u);
    }

    /// 0x463d30: Scanner: the sweep goes away when its iscript signals (bit 4).
    fn order_scanner(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_signal & 4 != 0 {
            self.units[u as usize].order_signal &= !4;
            self.kill_unit(d, u);
        }
    }

    /// 0x4780f0: Patrol.
    fn order_patrol(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            let head = self.units[u as usize].order_queue_head;
            if head == NIL || self.orders[head as usize].order != 0x98 {
                // First call: this leg queued as a Patrol order, then the way back.
                let (pos, target) = (self.units[u as usize].order_target_pos, self.units[u as usize].order_target_unit);
                self.units[u as usize].order_flags |= 1;
                self.issue_order(d, u, 0x98, true, pos, target, 0xe4);
                self.order_nothing(d, u);
                let back = self.unit_sprite_pos(u);
                self.order_queue_append(d, u, 0x98, 0xe4, back, NIL);
                return;
            }
            let p = self.units[u as usize].order_target_pos;
            let r = self.set_move_target_xy(d, u, p);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != p {
                f.next_target_waypoint = p;
            }
            if !r {
                return;
            }
            self.units[u as usize].order_state = 1;
        }
        let at = self.units[u as usize].auto_target;
        if at != NIL
            && self.is_enemy(u, at)
            && self.sprites[self.units[at as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
            && self.units[at as usize].status & 0x400_0000 == 0
            && !self.cloaked_from(at, u)
        {
            self.patrol_attack(d, u, at, true);
            return;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        if self.units[u as usize].unit_type == 0x22 && self.units[u as usize].order_target_unit == NIL {
            todo_port!("0x4422a0", "patrolling medics look for units to heal");
        }
        if self.units[u as usize].fl.move_target == self.unit_sprite_pos(u) {
            // 0x4748e0: queue the way back, then the next leg.
            let back = if self.unit_at_move_target(u) == 2 { self.units[u as usize].order_target_pos } else { self.unit_sprite_pos(u) };
            self.order_queue_append(d, u, 0x98, 0xe4, back, NIL);
            self.order_next_or_idle(d, u);
            return;
        }
        if self.acquisition_range(u) as u8 == 0 {
            return;
        }
        let t = self.find_auto_target(d, u);
        if t != NIL {
            self.patrol_attack(d, u, t, false);
        }
    }

    /// The common end of Patrol's attacks: this leg queued again and the unit's attack
    /// order on `t` before it (0x474540, 0x4747e0), then the attack starts (0x475420) and
    /// the unit is marked auto-targeting (0x4026a0).
    fn patrol_attack(&mut self, d: &Data, u: u32, t: u32, clear_auto: bool) {
        let (pos, target) = (self.units[u as usize].order_target_pos, self.units[u as usize].order_target_unit);
        let head = self.units[u as usize].order_queue_head;
        self.order_insert_before(d, u, 0x98, 0xe4, pos, target, head);
        let attack = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
        let head = self.units[u as usize].order_queue_head;
        let tp = self.unit_sprite_pos(t);
        self.order_insert_before(d, u, attack, 0xe4, tp, t, head);
        if clear_auto {
            self.units[u as usize].auto_target = NIL;
        }
        self.order_next_or_idle(d, u);
        self.units[u as usize].status |= 0x800_0000;
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            self.units[sub as usize].status |= 0x800_0000;
        }
    }

    /// 0x4e4210: CTFCOPInit.
    fn order_ctfcopinit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e4210", "CTFCOPInit");
    }

    /// 0x4e41a0: CTFCOP1.
    fn order_ctfcop1(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e41a0", "CTFCOP1");
    }

    /// 0x4a28b0: ComputerAI: every 15 frames the unit's AI (worker, building or
    /// military) decides what it does; a unit without AI goes to its human idle order.
    fn order_computer_ai(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let t = self.units[u as usize].unit_type;
        if t == 0x23 {
            todo_port!("0x4e9f90", "AI larva");
            let un = &self.units[u as usize];
            if un.sprite == NIL || un.order == 0 {
                return;
            }
        } else if t == 0x22 {
            todo_port!("0x436ae0", "AI medic");
        }
        let ai = self.units[u as usize].ai;
        if ai == NIL {
            let idle = d.dat.units.ai_human_idle[t as usize];
            self.order_now(d, u, idle);
            return;
        }
        if self.ai_special_unit_order(d, u) {
            return;
        }
        match super::ai_town::ai_kind(ai) {
            super::ai_town::AI_WORKER => self.worker_ai(d, u),
            super::ai_town::AI_BUILDING => {
                todo_port!("0x4361a0", "AI building");
                self.units[u as usize].main_order_timer = 3;
            }
            super::ai_town::AI_MILITARY => {
                todo_port!("0x43d910", "AI military");
                self.units[u as usize].main_order_timer = 3;
            }
            _ => {
                let idle = d.dat.units.ai_human_idle[t as usize];
                self.order_now(d, u, idle);
            }
        }
        let flags = d.dat.units.flags[t as usize];
        let st = self.units[u as usize].status;
        if self.race_of(d, u) == 1 && flags & 0x4000_0000 != 0 {
            todo_port!("0x436e70", "AI: mechanical terran units go for repairs (0x440770)");
            let _ = st;
        } else if t == 5 {
            todo_port!("0x4a12c0", "AI siege tank mode");
        } else if flags & 0x10_0000 != 0 {
            todo_port!("0x4a1340", "AI burrowing");
        }
    }

    /// 0x478de0: AtkMoveEP.
    fn order_atk_move_ep(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478de0", "AtkMoveEP");
    }

    /// 0x478ec0: HarassMove.
    fn order_harass_move(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478ec0", "HarassMove");
    }

    /// 0x4a1d80: AIPatrol.
    fn order_aipatrol(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4a1d80", "AIPatrol");
    }

    /// 0x4778e0: GuardPost.
    fn order_guard_post(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4778e0", "GuardPost");
    }

    /// 0x478490: ComputerReturn.
    fn order_computer_return(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478490", "ComputerReturn");
    }

}
