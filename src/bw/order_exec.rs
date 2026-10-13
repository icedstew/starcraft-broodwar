//! execute_main_order (1.16.1 0x4ec4d0): order dispatch.
//!
//! Generated from the decompiled switches (re: dispatch table), then hand-ported per handler.
//! Unported handlers are `todo_port!` stubs that leave the unit's state unchanged.

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
            0x71 | 0x73 | 0x77 | 0x78 | 0x79 | 0x7a | 0x8e | 0x8f | 0x90 | 0x91 | 0x92 | 0x93 | 0xb4 | 0xb5 | 0xb9 | 0xba => self.order_fire_yamato_gun(d, u),
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
            todo_port!("0x4e8280", "disabled unit: clear targets of units targeting it");
        }
    }

    /// 0x43ff90: acid spore / interceptor timer work every order-process tick.
    fn order_timer_tick(&mut self, _d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.acid_spore_count > 3 {
            todo_port!("0x43fe50", "acid spore timers");
            return;
        }
        if self.var_6957d0 == 0 && un.unit_type == 0x49 && un.u16_at(0xc0) != 0 {
            todo_port!("0x43fe50", "acid spore timers (interceptor)");
            self.var_6957d0 = 1;
        }
    }

    /// 0x464d10: NukeTrack.
    fn order_nuke_track(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464d10", "NukeTrack");
    }

    /// 0x4777f0: TurretGuard.
    fn order_turret_guard(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4777f0", "TurretGuard");
    }

    /// 0x477980: TurretAttack.
    fn order_turret_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x477980", "TurretAttack");
    }


    /// 0x4673d0: Repair.
    fn order_repair(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4673d0", "Repair");
    }




    /// 0x465e00: ScarabAttack.
    fn order_scarab_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x465e00", "ScarabAttack");
    }

    /// 0x493dd0: RechargeShieldsUnit.
    fn order_recharge_shields_unit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x493dd0", "RechargeShieldsUnit");
    }

    /// 0x464360: BuildingLand.
    fn order_building_land(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464360", "BuildingLand");
    }

    /// 0x4649b0: BuildingLiftOff.
    fn order_building_lift_off(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4649b0", "BuildingLiftOff");
    }

    /// 0x4548b0: ResearchTech.
    fn order_research_tech(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4548b0", "ResearchTech");
    }

    /// 0x4546a0: Upgrade.
    fn order_upgrade(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4546a0", "Upgrade");
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

    /// 0x493920: Interrupted.
    fn order_interrupted(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x493920", "Interrupted");
    }

    /// 0x464bd0: Sieging.
    fn order_sieging(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464bd0", "Sieging");
    }

    /// 0x464ae0: Unsieging.
    fn order_unsieging(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464ae0", "Unsieging");
    }

    /// 0x4948b0: ArchonWarp.
    fn order_archon_warp(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4948b0", "ArchonWarp");
    }

    /// 0x493b10: CompletingArchonSummon.
    fn order_completing_archon_summon(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x493b10", "CompletingArchonSummon");
    }

    /// 0x4e6700: NukeTrain.
    fn order_nuke_train(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e6700", "NukeTrain");
    }

    /// 0x493a80: InitializeArbiter.
    fn order_initialize_arbiter(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x493a80", "InitializeArbiter");
    }

    /// 0x4671b0: ResetCollision.
    fn order_reset_collision(&mut self, d: &Data, u: u32) {
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
    fn order_reset_harvest_collision(&mut self, d: &Data, u: u32) {
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

    /// 0x464180: MedicHeal.
    fn order_medic_heal(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464180", "MedicHeal");
    }

    /// 0x4637b0: HealMove.
    fn order_heal_move(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4637b0", "HealMove");
    }

    /// 0x464050: MedicHoldPosition.
    fn order_medic_hold_position(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464050", "MedicHoldPosition");
    }

    /// 0x463740: MedicHealToIdle.
    fn order_medic_heal_to_idle(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463740", "MedicHealToIdle");
    }

    /// 0x494690: DarkArchonMeld.
    fn order_dark_archon_meld(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x494690", "DarkArchonMeld");
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
        todo_port!("0x476000", "PlayerGuard: return-to-guard-position check (0x443080, 0x476fc0)");
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
                    todo_port!("0x4eb980", "move to target unit (0x4eb720)");
                    false
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

    /// 0x4654b0: ReaverStop.
    fn order_reaver_stop(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4654b0", "ReaverStop");
    }

    /// 0x4f6fa0: Attack2, MoveToInfest, MoveToRepair, CarrierMoveToAttack, ReaverMoveToAttack, Harvest2, MoveToFireYamatoGun.
    fn order_attack2(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f6fa0", "Attack2, MoveToInfest, MoveToRepair, CarrierMoveToAttack, ReaverMoveToAttack, Harvest2, MoveToFireYamatoGun");
    }

    /// 0x477d00: AttackFixedRange.
    fn order_attack_fixed_range(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x477d00", "AttackFixedRange");
    }

    /// 0x479040: AttackMove.
    fn order_attack_move(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x479040", "AttackMove");
    }

    /// inline: InfestedCommandCenter.
    fn order_infested_command_center(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "InfestedCommandCenter");
    }

    /// 0x4f3ea0: UnusedPowerup.
    fn order_unused_powerup(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f3ea0", "UnusedPowerup");
    }

    /// 0x476f50: TowerGuard.
    fn order_tower_guard(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x476f50", "TowerGuard");
    }

    /// 0x479150: TowerAttack.
    fn order_tower_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x479150", "TowerAttack");
    }

    /// 0x463df0: VultureMine.
    fn order_vulture_mine(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463df0", "VultureMine");
    }

    /// 0x47c4f0: StayInRange.
    fn order_stay_in_range(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47c4f0", "StayInRange");
    }


    /// inline: CastInfestation.
    fn order_cast_infestation(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "CastInfestation");
    }

    /// inline: InfestingCommandCenter.
    fn order_infesting_command_center(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "InfestingCommandCenter");
    }

    /// 0x4e6880: PlaceAddon.
    fn order_place_addon(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e6880", "PlaceAddon");
    }

    /// 0x45d0d0: ZergBuildingMorph.
    fn order_zerg_building_morph(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x45d0d0", "ZergBuildingMorph");
    }

    /// 0x45dc20: BuildNydusExit.
    fn order_build_nydus_exit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x45dc20", "BuildNydusExit");
    }

    /// inline: EnterNydusCanal.
    fn order_enter_nydus_canal(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "EnterNydusCanal");
    }

    /// 0x47c7b0: Follow.
    fn order_follow(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47c7b0", "Follow");
    }

    /// 0x4666a0: Carrier, CarrierFight, CarrierHoldPosition.
    fn order_carrier(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4666a0", "Carrier, CarrierFight, CarrierHoldPosition");
    }

    /// 0x465910: CarrierStop.
    fn order_carrier_stop(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x465910", "CarrierStop");
    }

    /// 0x465950: CarrierAttack.
    fn order_carrier_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x465950", "CarrierAttack");
    }

    /// 0x466720: CarrierIgnore2.
    fn order_carrier_ignore2(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x466720", "CarrierIgnore2");
    }

    /// 0x4665d0: Reaver, ReaverFight, ReaverHoldPosition.
    fn order_reaver(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4665d0", "Reaver, ReaverFight, ReaverHoldPosition");
    }

    /// 0x465690: ReaverAttack.
    fn order_reaver_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x465690", "ReaverAttack");
    }

    /// 0x465f60: InterceptorAttack.
    fn order_interceptor_attack(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x465f60", "InterceptorAttack");
    }

    /// 0x493990: RechargeShieldsBattery.
    fn order_recharge_shields_battery(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x493990", "RechargeShieldsBattery");
    }

    /// 0x466350: InterceptorReturn.
    fn order_interceptor_return(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x466350", "InterceptorReturn");
    }


    /// 0x463ac0: LiftingOff.
    fn order_lifting_off(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463ac0", "LiftingOff");
    }









    /// 0x4e7cf0: EnterTransport.
    fn order_enter_transport(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e7cf0", "EnterTransport");
    }

    /// 0x4e7300: PickupIdle.
    fn order_pickup_idle(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e7300", "PickupIdle");
    }

    /// 0x4e75d0: PickupTransport.
    fn order_pickup_transport(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e75d0", "PickupTransport");
    }

    /// 0x4e73b0: PickupBunker.
    fn order_pickup_bunker(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e73b0", "PickupBunker");
    }

    /// 0x4e7b70: Pickup4.
    fn order_pickup4(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e7b70", "Pickup4");
    }

    /// 0x4f3e10: PowerupIdle.
    fn order_powerup_idle(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f3e10", "PowerupIdle");
    }

    /// 0x47bab0: WatchTarget.
    fn order_watch_target(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47bab0", "WatchTarget");
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
    fn order_hold_position(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478d10", "HoldPosition");
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
    fn order_nothing(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head != super::list::NIL {
            self.prepare_next_order(d, u);
        }
    }

    /// 0x4e80d0: Unload.
    fn order_unload(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e80d0", "Unload");
    }

    /// 0x4e7700: MoveUnload.
    fn order_move_unload(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e7700", "MoveUnload");
    }

    /// 0x492850: FireYamatoGun, CastLockdown, CastDarkSwarm, CastParasite, CastSpawnBroodlings, CastEMPShockwave, CastPsionicStorm, CastIrradiate, CastPlague, CastConsume, CastEnsnare, CastStasisField, CastRestoration, CastDisruptionWeb, CastOpticalFlare, CastMaelstrom.
    fn order_fire_yamato_gun(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x492850", "FireYamatoGun, CastLockdown, CastDarkSwarm, CastParasite, CastSpawnBroodlings, CastEMPShockwave, CastPsionicStorm, CastIrradiate, CastPlague, CastConsume, CastEnsnare, CastStasisField, CastRestoration, CastDisruptionWeb, CastOpticalFlare, CastMaelstrom");
    }

    /// inline: Burrowing.
    fn order_burrowing(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "Burrowing");
    }

    /// inline: Burrowed.
    fn order_burrowed(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "Burrowed");
    }

    /// inline: Unburrowing.
    fn order_unburrowing(&mut self, _d: &Data, _u: u32) {
        todo_port!("inline", "Unburrowing");
    }

    /// 0x464730: NukeLaunch.
    fn order_nuke_launch(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464730", "NukeLaunch");
    }

    /// 0x463610: NukePaint.
    fn order_nuke_paint(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463610", "NukePaint");
    }

    /// 0x479410: NukeUnit.
    fn order_nuke_unit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x479410", "NukeUnit");
    }

    /// 0x479200: CastNuclearStrike.
    fn order_cast_nuclear_strike(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x479200", "CastNuclearStrike");
    }

    /// 0x464fd0: PlaceMine.
    fn order_place_mine(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464fd0", "PlaceMine");
    }

    /// 0x4f6ef0: RightClickAction.
    fn order_right_click_action(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f6ef0", "RightClickAction");
    }

    /// 0x4788e0: SuicideUnit.
    fn order_suicide_unit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4788e0", "SuicideUnit");
    }

    /// 0x478a40: SuicideLocation.
    fn order_suicide_location(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478a40", "SuicideLocation");
    }

    /// 0x494470: CastRecall.
    fn order_cast_recall(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x494470", "CastRecall");
    }

    /// 0x464e40: CastScannerSweep.
    fn order_cast_scanner_sweep(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x464e40", "CastScannerSweep");
    }

    /// 0x463d30: Scanner.
    fn order_scanner(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463d30", "Scanner");
    }

    /// 0x4550a0: CastDefensiveMatrix.
    fn order_cast_defensive_matrix(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4550a0", "CastDefensiveMatrix");
    }

    /// 0x4f6c40: CastHallucination.
    fn order_cast_hallucination(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f6c40", "CastHallucination");
    }

    /// 0x4780f0: Patrol.
    fn order_patrol(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4780f0", "Patrol");
    }

    /// 0x4e4210: CTFCOPInit.
    fn order_ctfcopinit(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e4210", "CTFCOPInit");
    }

    /// 0x4e41a0: CTFCOP1.
    fn order_ctfcop1(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4e41a0", "CTFCOP1");
    }

    /// 0x4a28b0: ComputerAI.
    fn order_computer_ai(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4a28b0", "ComputerAI");
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

    /// 0x4a1ef0: RescuePassive.
    fn order_rescue_passive(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4a1ef0", "RescuePassive");
    }

    /// 0x4a1c20: Neutral.
    fn order_neutral(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4a1c20", "Neutral");
    }

    /// 0x478490: ComputerReturn.
    fn order_computer_return(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x478490", "ComputerReturn");
    }

    /// 0x47bf80: HiddenGun.
    fn order_hidden_gun(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47bf80", "HiddenGun");
    }

    /// 0x47bcd0: OpenDoor.
    fn order_open_door(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47bcd0", "OpenDoor");
    }

    /// 0x47bc50: CloseDoor.
    fn order_close_door(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47bc50", "CloseDoor");
    }

    /// 0x47c0a0: HideTrap.
    fn order_hide_trap(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47c0a0", "HideTrap");
    }

    /// 0x47c1b0: RevealTrap.
    fn order_reveal_trap(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47c1b0", "RevealTrap");
    }

    /// 0x47be80: EnableDoodad.
    fn order_enable_doodad(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47be80", "EnableDoodad");
    }

    /// 0x47bd60: DisableDoodad.
    fn order_disable_doodad(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47bd60", "DisableDoodad");
    }

    /// 0x463900: Medic.
    fn order_medic(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x463900", "Medic");
    }

    /// 0x4f6950: CastMindControl.
    fn order_cast_mind_control(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f6950", "CastMindControl");
    }

    /// 0x4f6d40: CastFeedback.
    fn order_cast_feedback(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x4f6d40", "CastFeedback");
    }

    /// 0x47c210: JunkYardDog.
    fn order_junk_yard_dog(&mut self, _d: &Data, _u: u32) {
        todo_port!("0x47c210", "JunkYardDog");
    }

}
