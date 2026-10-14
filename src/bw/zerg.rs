//! Drones becoming buildings: DroneStartBuild (0x45cf80), DroneLand (0x4e9aa0),
//! DroneBuild (0x45e090) and the morph itself (0x45d890 / 0x45d790 / 0x45d2e0).

use super::list::NIL;
use super::sprite::iflag;
use super::{Data, State};
use crate::todo_port;

impl State {
    /// 0x498150 on the last image (the shadow): set its y offset.
    fn set_shadow_y(&mut self, u: u32, y: i8) {
        let s = self.units[u as usize].sprite;
        let i = self.sprites[s as usize].images.tail;
        let im = &mut self.images[i as usize];
        if im.modifier == 10 && im.y != y {
            im.flags |= iflag::REDRAW;
            im.y = y;
        }
    }

    /// Freeze the shadow's y offset (the drone's shadow stays put while it lands).
    fn set_shadow_frozen(&mut self, u: u32, frozen: bool) {
        let s = self.units[u as usize].sprite;
        let i = self.sprites[s as usize].images.tail;
        let im = &mut self.images[i as usize];
        if im.modifier == 10 {
            if frozen {
                im.flags |= iflag::FREEZE_Y;
            } else {
                im.flags &= !iflag::FREEZE_Y;
            }
        }
    }

    /// Back off a failed build: shadow down, queue 0x96 first, next order.
    fn drone_build_failed(&mut self, d: &Data, u: u32) {
        self.set_shadow_y(u, 7);
        // 0x4983a0: image screen positions (UI).
        self.order_queue_front(d, u, 0x96, (0, 0));
        self.order_next_or_idle(d, u);
    }

    /// The tile placement of the building being made, from (x, y).
    fn build_tile(d: &Data, t: u16, x: i16, y: i16) -> (i16, i16) {
        let v = d.dat.units.placement_box[t as usize];
        let (pw, ph) = (v as i16 as i32, (v >> 16) as i16 as i32);
        (((x as i32 - pw / 2) / 32) as i16, ((y as i32 - ph / 2) / 32) as i16)
    }

    fn building_queued(&self, u: u32) -> u16 {
        let un = &self.units[u as usize];
        un.build_queue[un.build_queue_slot as usize]
    }

    /// 0x45cf80: DroneStartBuild: land at the spot (0x46), then build (0x1a).
    pub(super) fn order_drone_start_build(&mut self, d: &Data, u: u32) {
        self.units[u as usize].order_flags |= 1;
        let pos = self.units[u as usize].order_target_pos;
        self.issue_order(d, u, 0x46, true, pos, NIL, 0xe4);
        self.prepare_next_order(d, u);
        let pos = self.units[u as usize].order_target_pos;
        self.issue_order(d, u, 0x1a, false, pos, NIL, 0xe4);
    }

    /// 0x4e9aa0: DroneLand.
    pub(super) fn order_drone_land(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ai != NIL {
            self.units[u as usize].status |= 0x80_0000;
        }
        match self.units[u as usize].order_state {
            0 => {
                let p = self.units[u as usize].order_target_pos;
                if self.move_to_facing(d, u, (p.0, p.1 - 7)) {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                let r = self.unit_at_move_target(u);
                if r == 0 {
                    return;
                }
                let t = self.building_queued(u);
                let un = &self.units[u as usize];
                let (player, pos) = (un.player, un.order_target_pos);
                let (tx, ty) = Self::build_tile(d, t, pos.0, pos.1);
                let err = self.placement_check(d, u, player, tx, ty, t, 0, false, true, true);
                if t == 0x95 && r == 1 {
                    if err != 0 {
                        return self.drone_land_failed(d, u);
                    }
                    let g = self.unit_of_type_at(0xbc, pos.0, pos.1);
                    if g != NIL && self.box_distance_within(d, u, 0x20, g) {
                        let un = &mut self.units[u as usize];
                        un.status &= !0x10_0000;
                        let s = un.sprite;
                        self.sprites[s as usize].elevation = d.dat.units.elevation[un.unit_type as usize] + 1;
                        self.issue_order(d, u, 0x96, false, (0, 0), NIL, 0xe4);
                        let idle = self.idle_order_of(d, u);
                        self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
                        if self.move_to_facing(d, u, pos) {
                            self.units[u as usize].order_state = 2;
                            self.units[u as usize].status |= 0x1000;
                        }
                        return;
                    }
                } else if err != 0 || r == 2 {
                    return self.drone_land_failed(d, u);
                }
                if !self.within(u, pos, 0x80) {
                    return self.drone_land_failed(d, u);
                }
                if !self.move_to_facing(d, u, pos) && self.units[u as usize].status & 0x3000 == 0 {
                    return;
                }
                self.set_shadow_frozen(u, true);
                self.units[u as usize].status |= 0x1000;
                self.units[u as usize].order_state = 2;
            }
            2 => {
                let r = self.unit_at_move_target(u);
                if r == 0 {
                    return;
                }
                self.set_shadow_y(u, 0);
                self.set_shadow_frozen(u, false);
                let head = self.units[u as usize].order_queue_head;
                let build_next = head != NIL && self.orders[head as usize].order == 0x1a;
                if r == 2 {
                    // 0x48f5a0: "can't build here" (UI).
                    if build_next {
                        self.order_remove(d, u, head);
                    }
                    self.set_shadow_y(u, 7);
                    self.order_queue_front(d, u, 0x96, (0, 0));
                } else if build_next {
                    let p = self.units[u as usize].order_target_pos;
                    self.set_unit_position(d, u, p.0, p.1);
                } else {
                    self.set_shadow_y(u, 7);
                    self.order_queue_front(d, u, 0x96, (0, 0));
                }
                // 0x68c1e8 etc.: button refresh (UI).
                self.order_next_or_idle(d, u);
            }
            _ => {}
        }
    }

    /// The failure exit of DroneLand state 1.
    fn drone_land_failed(&mut self, d: &Data, u: u32) {
        // 0x48f5a0: "can't build here" (UI).
        let head = self.units[u as usize].order_queue_head;
        if head != NIL && self.orders[head as usize].order == 0x1a {
            self.order_remove(d, u, head);
        }
        let idle = self.idle_order_of(d, u);
        self.order_now(d, u, idle);
    }

    /// 0x45e090: DroneBuild.
    pub(super) fn order_drone_build(&mut self, d: &Data, u: u32) {
        let (x, y) = self.unit_sprite_pos(u);
        if (x, y) != self.units[u as usize].order_target_pos {
            return self.drone_build_failed(d, u);
        }
        let t = self.building_queued(u);
        let player = self.units[u as usize].player;
        let (tx, ty) = Self::build_tile(d, t, x, y);
        let err = self.placement_check(d, u, player, tx, ty, t, 0, false, true, true);
        if err != 0 || self.unit_sprite_pos(u) != self.units[u as usize].order_target_pos {
            // 0x48f6c0(0x358): "can't build here" with the unit race's sound (0xa2 + race).
            // ponytail: the sound cooldown check 0x48ed50 is not ported.
            let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
            self.local_error(player, 0xa2 + race, 0x358);
            return self.drone_build_failed(d, u);
        }
        let p = player as usize;
        let (m, g) = (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32);
        if !self.supply_ok_show(d, p, t) {
            return self.drone_build_failed(d, u);
        }
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[p] < m {
            self.local_error(player, 0x93 + race, 0x352);
            return self.drone_build_failed(d, u);
        }
        if self.players.gas[p] < g {
            self.local_error(player, 0x96 + race, 0x353);
            return self.drone_build_failed(d, u);
        }
        self.players.minerals[p] -= m;
        self.players.gas[p] -= g;
        let powerup = if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 8 != 0 { self.units[u as usize].ptr_at(0xc0) } else { NIL };
        if powerup != NIL {
            todo_port!("0x4f3b70", "drone carrying a powerup drops it");
        }
        if t == 0x95 {
            self.morph_extractor(d, u);
        } else {
            self.morph_drone(d, u, t);
        }
        if powerup != NIL {
            todo_port!("0x4eb9f0", "dropped powerup returns to its spot");
        }
    }

    /// 0x45d790: the drone becomes an extractor on the geyser and goes away.
    fn morph_extractor(&mut self, d: &Data, u: u32) {
        let Some(g) = self.take_over_geyser(d, u, 0x95) else {
            self.set_shadow_y(u, 7);
            self.set_shadow_frozen(u, true);
            let (x, y) = self.unit_sprite_pos(u);
            let f = &mut self.units[u as usize].fl;
            if f.next_target_waypoint != (x, y - 7) {
                f.next_target_waypoint = (x, y - 7);
            }
            self.order_queue_front(d, u, 0x96, (0, 0));
            return self.order_next_or_idle(d, u);
        };
        if self.player_type(self.units[g as usize].player) == 1 {
            todo_port!("0x435770", "AI: new building joins its town");
        }
        self.units[u as usize].order_flags |= 4;
        self.kill_unit(d, u);
        self.start_zerg_building(d, g);
        let s = self.units[g as usize].sprite;
        self.add_image_below_main(d, s, 0x159);
    }

    /// 0x45d890: the drone turns into building `t`.
    fn morph_drone(&mut self, d: &Data, u: u32, t: u16) {
        self.modify_completed_counters(d, u, -1, false);
        self.units[u as usize].status &= !1;
        self.transform_unit(d, u, t);
        if self.player_type(self.units[u as usize].player) == 1 && self.units[u as usize].ai != NIL {
            todo_port!("0x45d890", "AI: drone leaves its town (0x404470 / 0x433dd0)");
        }
        self.start_zerg_building(d, u);
    }

    /// 0x45d2e0: a new Zerg building starts morphing (IncompleteMorphing, 0x2d).
    fn start_zerg_building(&mut self, d: &Data, u: u32) {
        self.creep_unmark_receding(d, u);
        let t = self.units[u as usize].unit_type;
        if !self.disabled(u) || d.dat.units.flags[t as usize] & 1 != 0 {
            self.units[u as usize].button_set = if super::command::has_rally(t) { 0xea } else { 0xe9 };
        }
        self.reset_unit_image(d, u, true);
        self.units[u as usize].hp = (d.dat.units.hit_points[t as usize] / 10) as i32;
        self.units[u as usize].order_flags |= 1;
        self.issue_order(d, u, 0x2d, true, (0, 0), NIL, 0xe4);
        self.prepare_next_order(d, u);
        let slot = self.units[u as usize].build_queue_slot as usize;
        self.units[u as usize].build_queue[slot] = 0xe4;
    }

    /// 0x45d500: IncompleteMorphing: animation stages by remaining build time, then
    /// the building completes (hatchery / lair / spire upgrades transform first).
    pub(super) fn order_incomplete_morphing(&mut self, d: &Data, u: u32) {
        let queued = self.building_queued(u);
        let upgrade = matches!(queued, 0x85 | 0x84 | 0x89 | 0x90 | 0x92);
        let t = if upgrade { queued } else { self.units[u as usize].unit_type };
        let total = d.dat.units.build_time[t as usize] as u32;
        let left = self.units[u as usize].remaining_build_time as u32;
        let s = self.units[u as usize].sprite;
        match self.units[u as usize].order_state {
            0 if left < total * 3 >> 2 => self.units[u as usize].order_state = 1,
            1 => {
                if self.units[u as usize].unit_type == 0x95 {
                    let mut i = self.sprites[s as usize].images.head;
                    while i != NIL && self.images[i as usize].image_id != 0x159 {
                        i = self.images[i as usize].link.next;
                    }
                    if i != NIL {
                        self.destroy_image(i);
                    }
                }
                self.sprite_images_run_anim(d, s, 0xd);
                self.units[u as usize].order_state = 2;
            }
            2 if left < total >> 1 => self.units[u as usize].order_state = 3,
            3 => {
                self.sprite_images_run_anim(d, s, 0xe);
                self.units[u as usize].order_state = 4;
            }
            4 if left == 0 => self.units[u as usize].order_state = 5,
            5 => {
                self.sprite_images_run_anim(d, s, 0xf);
                self.units[u as usize].order_state = 6;
                // 0x48f310: "morph complete" sound (UI).
            }
            6 => {
                let un = &mut self.units[u as usize];
                if un.order_signal & 4 == 0 {
                    return;
                }
                un.order_signal &= !4;
                if upgrade {
                    let old_hp = un.hp;
                    un.status |= 1;
                    self.modify_completed_counters(d, u, -1, false);
                    self.units[u as usize].status &= !1;
                    self.transform_unit(d, u, t);
                    let prev = self.units[u as usize].previous_unit_type;
                    let hp = (d.dat.units.hit_points[t as usize] as i32 - d.dat.units.hit_points[prev as usize] as i32 + old_hp).max(0x100);
                    self.set_unit_hp(d, u, hp);
                    self.units[u as usize].remaining_build_time = 0;
                }
                self.finish_unit_pre(d, u);
                self.finish_unit_post(d, u);
                self.creep_unmark_receding(d, u);
                self.unit_anim(d, u, 0xf);
                let (x, y) = self.unit_sprite_pos(u);
                self.creep_under_building(d, t, x as i32, y as i32);
                return;
            }
            _ => {}
        }
        let fast = self.game_options & 2 != 0;
        let un = &mut self.units[u as usize];
        let left = un.remaining_build_time;
        if left != 0 {
            un.remaining_build_time = if fast { (left as i16).wrapping_sub(0x10).max(0) as u16 } else { left - 1 };
        }
        if !upgrade {
            let gain = if fast { un.hp_gain.wrapping_shl(4) } else { un.hp_gain } as i32;
            let hp = un.hp + gain;
            self.set_unit_hp(d, u, hp);
        }
    }

    /// 0x4c1990: unit morph command: larvae and mutalisks morph into `t`, hydralisks
    /// into lurkers.
    pub fn command_unit_morph(&mut self, d: &Data, player: u8, sel: &[u32], t: u16) {
        for u in self.live_selection(sel) {
            let ut = self.units[u as usize].unit_type;
            let ok = if ut == 0x26 && t == 0x67 {
                self.can_use_tech(d, u, 0x20, player)
            } else if ut == 0x23 || ut == 0x2b {
                self.can_make_unit(d, u, t, player)
            } else {
                continue;
            };
            if ok != Ok(1) || self.units[u as usize].order == 0x2a || !self.queue_build(d, u, t) {
                continue;
            }
            self.units[u as usize].order_flags |= 1;
            self.issue_order(d, u, 0x2a, true, (0, 0), NIL, 0xe4);
            self.prepare_next_order(d, u);
        }
        // 0x68c1b0 etc.: button refresh (UI).
    }

    /// 0x4c1910: building morph command: the one selected zerg building morphs into `t`.
    pub fn command_building_morph(&mut self, d: &Data, player: u8, sel: &[u32], t: u16) {
        let [u] = self.live_selection(sel)[..] else { return };
        let ut = self.units[u as usize].unit_type;
        if !(0x82..=0x98).contains(&ut) || !(0x82..=0x98).contains(&t) {
            return;
        }
        if self.can_make_unit(d, u, t, player) != Ok(1) || !self.queue_build(d, u, t) || self.units[u as usize].order == 0x2b {
            return;
        }
        self.order_now(d, u, 0x2b);
        // 0x4c36c0: button refresh (UI).
    }

    /// 0x4e5d60: show button set `set` unless the unit is disabled.
    pub(super) fn set_button_set(&mut self, d: &Data, u: u32, set: u16) {
        if !self.disabled(u) || d.dat.units.flags[self.units[u as usize].unit_type as usize] & 1 != 0 || set == 0xe4 {
            self.units[u as usize].button_set = set;
        }
    }

    /// 0x45dea0: ZergUnitMorph: become an egg, wait out the build time, hatch.
    pub(super) fn order_zerg_unit_morph(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let ut = self.units[u as usize].unit_type;
                let egg = match ut {
                    0x23 => 0x24,
                    0x26 => 0x61,
                    0x2b => 0x3b,
                    _ => return,
                };
                let t = self.building_queued(u);
                let p = self.units[u as usize].player as usize;
                if !self.supply_ok_show(d, p, t) {
                    self.clear_build_queue(d, u);
                    let idle = self.idle_order_of(d, u);
                    return self.order_now(d, u, idle);
                }
                self.modify_completed_counters(d, u, -1, false);
                self.units[u as usize].status &= !1;
                self.transform_unit(d, u, egg);
                let ut = self.units[u as usize].unit_type;
                self.set_button_set(d, u, ut);
                self.units[u as usize].remaining_build_time = d.dat.units.build_time[t as usize];
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if self.build_time_elapsed(u) {
                    let s = self.units[u as usize].sprite;
                    self.sprite_images_run_anim(d, s, 0xd);
                    self.units[u as usize].order_state = 2;
                }
            }
            2 => {
                let un = &mut self.units[u as usize];
                if un.order_signal & 4 == 0 {
                    return;
                }
                un.order_signal &= !4;
                let prev = un.unit_type;
                let t = self.building_queued(u);
                self.transform_unit(d, u, t);
                let ut = self.units[u as usize].unit_type;
                if (ut == 0x29 || ut == 0x2a) && self.player_type(self.units[u as usize].player) == 1 {
                    todo_port!("0x4a2830", "AI: hatched unit joins the computer's forces");
                }
                // 0x48f440: "unit ready" sound (UI).
                let slot = self.units[u as usize].build_queue_slot as usize;
                self.units[u as usize].build_queue[slot] = 0xe4;
                if d.dat.units.construction_image[ut as usize] != 0 {
                    self.reset_unit_image(d, u, true);
                    self.unit_anim(d, u, 0xd);
                    return self.order_now(d, u, 0x29);
                }
                self.finish_unit_pre(d, u);
                self.finish_unit_post(d, u);
                if prev == 0x24 {
                    let h = self.units[u as usize].connected_unit;
                    self.send_to_rally(d, h, u);
                }
            }
            _ => {}
        }
    }

    /// 0x466940: count down the build time; true once it had run out.
    fn build_time_elapsed(&mut self, u: u32) -> bool {
        let fast = self.game_options & 2 != 0;
        let un = &mut self.units[u as usize];
        let t = un.remaining_build_time;
        if t == 0 {
            return true;
        }
        un.remaining_build_time = if fast { (t as i16).wrapping_sub(0x10).max(0) as u16 } else { t - 1 };
        false
    }

    /// 0x45dd60: ZergBirth: the hatched unit (and a zergling / scourge twin) comes out.
    pub(super) fn order_zerg_birth(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        if un.order_signal & 4 == 0 {
            return;
        }
        un.order_signal &= !4;
        let t = un.unit_type;
        let prev = un.previous_unit_type;
        let pair = d.dat.units.flags[t as usize] & 0x400 != 0;
        let nudge = !pair && prev != 0x3b;
        let ai = self.player_type(self.units[u as usize].player) == 1;
        let twin = if pair { self.hatch_twin(d, u) } else { NIL };
        if twin != NIL && ai {
            todo_port!("0x4a2830", "AI: twin joins the computer's forces");
        }
        if t != 0x29 && t != 0x2a && ai {
            todo_port!("0x4a2830", "AI: hatched unit joins the computer's forces");
        }
        self.reset_unit_image(d, u, false);
        self.finish_unit_pre(d, u);
        self.finish_unit_post(d, u);
        let (x, y) = self.unit_sprite_pos(u);
        let mut p = (x, y);
        if nudge {
            let dy = if d.dat.units.movement_flags[t as usize] == 0xc1 {
                -7
            } else if self.units[u as usize].status & 4 != 0 {
                -0x2a
            } else {
                0
            };
            p.1 = y.wrapping_add(dy);
        }
        self.clamp_unit_pos(d, t, &mut p);
        if p != (x, y) {
            self.set_unit_position(d, u, p.0, p.1);
        }
        if prev != 0x3b && prev != 0x61 {
            let h = self.units[u as usize].connected_unit;
            self.send_to_rally(d, h, u);
            if twin != NIL {
                let h = self.units[u as usize].connected_unit;
                self.send_to_rally(d, h, twin);
            }
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x45d910: the egg's second zergling / scourge, at the egg's second landing-dust
    /// LO offset; the first moves to the first.
    fn hatch_twin(&mut self, d: &Data, u: u32) -> u32 {
        let s = self.units[u as usize].sprite;
        let m = self.sprites[s as usize].main_image;
        let im = &self.images[m as usize];
        let flip = im.flags & iflag::FLIPPED != 0;
        let lo = |n| {
            let (x, y) = d.lo_offset(3, im.image_id, im.frame, n).unwrap_or((0, 0));
            (if flip { -(x as i16) } else { x as i16 }, y as i16)
        };
        let (o1, o2) = (lo(0), lo(1));
        let (sx, sy) = self.unit_sprite_pos(u);
        let t = self.units[u as usize].unit_type;
        let mut p1 = (sx.wrapping_add(o1.0), sy.wrapping_add(o1.1));
        let mut p2 = (sx.wrapping_add(o2.0), sy.wrapping_add(o2.1));
        self.clamp_unit_pos(d, t, &mut p1);
        self.clamp_unit_pos(d, t, &mut p2);
        self.set_unit_position(d, u, p1.0, p1.1);
        let player = self.units[u as usize].player;
        let Some(c) = self.create_unit(d, t, p2.0, p2.1, player) else {
            self.show_last_error(player);
            return NIL;
        };
        let dir = match t {
            0x25 => 0x90,
            0x2f => 0x10,
            _ => 0,
        };
        self.subunit_face(d, c, dir);
        self.finish_unit_pre(d, c);
        self.finish_unit_post(d, c);
        let (a, b) = (&self.units[u as usize], &self.units[c as usize]);
        let _ = b;
        let copy = (
            a.remove_timer,
            a.defense_matrix_damage,
            a.defense_matrix_timer,
            a.stim_timer,
            a.ensnare_timer,
            a.lockdown_timer,
            a.irradiate_timer,
            a.stasis_timer,
            a.plague_timer,
            a.storm_timer,
            a.irradiated_by,
            a.irradiate_player,
            a.parasite_flags,
            a.cycle_counter,
            a.is_blind,
            a.maelstrom_timer,
            a.acid_spore_count,
            a.acid_spore_time,
        );
        let b = &mut self.units[c as usize];
        (
            b.remove_timer,
            b.defense_matrix_damage,
            b.defense_matrix_timer,
            b.stim_timer,
            b.ensnare_timer,
            b.lockdown_timer,
            b.irradiate_timer,
            b.stasis_timer,
            b.plague_timer,
            b.storm_timer,
            b.irradiated_by,
            b.irradiate_player,
            b.parasite_flags,
            b.cycle_counter,
            b.is_blind,
            b.maelstrom_timer,
            b.acid_spore_count,
            b.acid_spore_time,
        ) = copy;
        let b = &self.units[c as usize];
        if b.defense_matrix_timer | b.lockdown_timer | b.maelstrom_timer | b.irradiate_timer | b.ensnare_timer | b.stasis_timer | b.plague_timer | b.acid_spore_count != 0 {
            todo_port!("0x492da0", "twin takes the egg's status overlays");
        }
        self.update_speed(d, c);
        // 0x45d040: local selection (UI).
        c
    }
}
