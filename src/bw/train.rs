//! Training units: the build queue (+0x98, current slot +0xa4), costs and supply,
//! and the Train secondary order (0x468420).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Build queue "empty" marker.
pub const NONE: u16 = 0xe4;

impl State {
    /// Supply race index from units.dat group flags (0 zerg, 1 terran, 2 protoss).
    fn supply_race(d: &Data, t: u16) -> Option<usize> {
        let g = d.dat.units.staredit_group_flags[t as usize];
        if g & 1 != 0 {
            Some(0)
        } else if g & 2 != 0 {
            Some(1)
        } else if g & 4 != 0 {
            Some(2)
        } else {
            None
        }
    }

    /// 0x488990: supply cap of `t`'s race.
    fn supply_max(&self, d: &Data, p: usize, t: u16) -> i32 {
        Self::supply_race(d, t).map_or(0, |r| self.players.supply_max[r][p])
    }

    /// 0x4889d0: free supply of `t`'s race (provided, capped, minus used).
    pub(super) fn supply_free(&self, d: &Data, p: usize, t: u16) -> i32 {
        let Some(r) = Self::supply_race(d, t) else { return 0 };
        let max = self.players.supply_max[r][p];
        let used = self.players.supply_used[r][p];
        if self.game_options & 0x2_0000 != 0 {
            return max - used;
        }
        let mut prov = self.players.supply_provided[r][p];
        if (max as u32) < prov as u32 {
            prov = max;
        }
        prov - used
    }

    /// 0x42cf70: enough supply to make `t`; otherwise the advisor sound and stat_txt
    /// error 0x48ee30 would show.
    pub(super) fn supply_error(&self, d: &Data, p: usize, t: u16) -> Option<(u16, u16)> {
        let flags = d.dat.units.flags[t as usize];
        let mut req = d.dat.units.supply_required[t as usize] as i32;
        if flags & 0x400 != 0 {
            req *= 2;
        }
        if req == 0 || flags & 0x100_0000 != 0 {
            return None;
        }
        if t == 0x67 {
            req -= d.dat.units.supply_required[0x26] as i32;
        }
        let race = Self::supply_race(d, t);
        let used = race.map_or(0, |r| self.players.supply_used[r][p]);
        let r = race.map_or(4, |r| r as u16);
        if self.supply_max(d, p, t) < req + used {
            return Some((r + 1, 0x34f + r));
        }
        if self.supply_free(d, p, t) - req < 0 {
            return Some((0x99 + r, 0x34c + r));
        }
        None
    }

    /// Race index of a unit type as 1.16.1's sounds use it (0 zerg, 1 terran, 2 protoss, else 4).
    pub(super) fn supply_race_of(d: &Data, t: u16) -> u16 {
        Self::supply_race(d, t).map_or(4, |r| r as u16)
    }

    pub(super) fn supply_ok(&self, d: &Data, p: usize, t: u16) -> bool {
        self.supply_error(d, p, t).is_none()
    }

    /// 0x42cf70 called with errors shown (its third argument 1).
    pub(super) fn supply_ok_show(&mut self, d: &Data, p: usize, t: u16) -> bool {
        match self.supply_error(d, p, t) {
            Some((sound, msg)) => {
                self.local_error(p as u8, sound, msg);
                false
            }
            None => true,
        }
    }

    /// 0x42d140: can pay for `t` (and, with `check_supply`, house it), errors shown.
    fn can_afford(&mut self, d: &Data, p: usize, t: u16, check_supply: bool) -> bool {
        if check_supply && !self.supply_ok_show(d, p, t) {
            return false;
        }
        // 0x42d0c0: the advisor sound is the local player's race's (0x57f1e2).
        let (m, g) = (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32);
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[p] < m {
            self.local_error(p as u8, 0x93 + race, 0x352);
            return false;
        }
        if self.players.gas[p] < g {
            self.local_error(p as u8, 0x96 + race, 0x353);
            return false;
        }
        true
    }

    /// 0x4669b0: first free build queue slot from the current one (5 = full).
    fn free_build_slot(&self, u: u32) -> usize {
        let un = &self.units[u as usize];
        let mut i = un.build_queue_slot as usize;
        for _ in 0..5 {
            if i > 4 {
                i = 0;
            }
            if un.build_queue[i] == NONE {
                return i;
            }
            i += 1;
        }
        5
    }

    /// 0x467250: queue `t` in `u`'s build queue and pay for it.
    pub fn queue_build(&mut self, d: &Data, u: u32, t: u16) -> bool {
        let slot = self.free_build_slot(u);
        if slot == 5 {
            return false;
        }
        let p = self.units[u as usize].player as usize;
        let current = slot == self.units[u as usize].build_queue_slot as usize;
        if !self.can_afford(d, p, t, current) {
            return false;
        }
        self.units[u as usize].build_queue[slot] = t;
        if d.dat.units.flags[t as usize] & 1 == 0 {
            self.players.minerals[p] -= d.dat.units.mineral_cost[t as usize] as i32;
            self.players.gas[p] -= d.dat.units.gas_cost[t as usize] as i32;
        }
        true
    }

    /// Train command (0x4c1c20 after its selection/requirement checks).
    pub fn command_train(&mut self, d: &Data, u: u32, t: u16) {
        if t < 0x6a && self.queue_build(d, u, t) {
            self.set_secondary_order(u, 0x26);
        }
    }

    /// 0x42cec0: refund `t`.
    pub(super) fn refund(&mut self, d: &Data, p: usize, t: u16) {
        self.players.minerals[p] += d.dat.units.mineral_cost[t as usize] as i32;
        self.players.gas[p] += d.dat.units.gas_cost[t as usize] as i32;
    }

    /// 0x468420: Train.
    pub fn secondary_train(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            return;
        }
        if d.dat.units.staredit_group_flags[un.unit_type as usize] & 1 != 0 && un.unit_type != 0x82 {
            return;
        }
        let state = un.secondary_order_state;
        if state <= 1 {
            let t = un.build_queue[un.build_queue_slot as usize];
            if t == NONE {
                self.set_secondary_order(u, 0x17);
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0x14);
                return;
            }
            let new = self.create_trainee(d, u, t, state == 0);
            self.units[u as usize].current_build_unit = new;
            if new == NIL {
                self.units[u as usize].secondary_order_state = 1;
                return;
            }
            self.units[u as usize].secondary_order_state = 2;
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0x13);
            return;
        }
        if state != 2 {
            return;
        }
        let c = self.units[u as usize].current_build_unit;
        if c != NIL {
            let gain = self.units[c as usize].hp_gain;
            if self.build_progress(d, c, gain as i32, true) {
                if self.units[c as usize].status & 1 == 0 {
                    return;
                }
                if self.player_type(self.units[u as usize].player) == 1 {
                    todo_port!("0x4a2830", "AI: trained unit joins the computer's forces");
                }
                if self.units[c as usize].unit_type == 0xe {
                    // 0x4e6340: the nuke goes into the silo.
                    self.hide_unit(d, c);
                } else {
                    self.send_to_rally(d, u, c);
                }
                let un = &mut self.units[u as usize];
                let slot = un.build_queue_slot as usize;
                un.build_queue[slot] = NONE;
                un.build_queue_slot = ((slot + 1) % 5) as u8;
            } else {
                // Couldn't place the unit: cancel it and shift the queue down.
                let un = &self.units[u as usize];
                let mut i = un.build_queue_slot as usize % 5;
                let t = un.build_queue[i];
                if t != NONE {
                    let c = self.units[u as usize].current_build_unit;
                    if c != NIL {
                        self.cancel_unit(d, c);
                    } else if d.dat.units.flags[t as usize] & 1 == 0 {
                        let p = self.units[u as usize].player as usize;
                        self.refund(d, p, t);
                    }
                    for _ in 0..4 {
                        if self.units[u as usize].build_queue[i] == NONE {
                            break;
                        }
                        let j = (i + 1) % 5;
                        self.units[u as usize].build_queue[i] = self.units[u as usize].build_queue[j];
                        i = j;
                    }
                    self.units[u as usize].build_queue[i] = NONE;
                }
            }
        }
        let un = &mut self.units[u as usize];
        un.current_build_unit = NIL;
        un.secondary_order_state = 0;
    }

    /// 0x468200: create the unit being trained (hidden until done).
    pub(super) fn create_trainee(&mut self, d: &Data, b: u32, t: u16, show_error: bool) -> u32 {
        let bu = &self.units[b as usize];
        let ok_builder = bu.status & 2 != 0 || matches!(bu.unit_type, 0x48 | 0x52 | 0x53 | 0x51 | 0x24);
        let p = bu.player;
        if !ok_builder || !(if show_error { self.supply_ok_show(d, p as usize, t) } else { self.supply_ok(d, p as usize, t) }) {
            return NIL;
        }
        let (x, y) = self.unit_sprite_pos(b);
        match self.create_unit(d, t, x, y, p) {
            Some(n) => n,
            None => {
                self.show_last_error(p);
                NIL
            }
        }
    }

    /// 0x4679a0: one frame of construction. False when the finished unit could not
    /// be placed.
    pub fn build_progress(&mut self, d: &Data, c: u32, gain: i32, place: bool) -> bool {
        let hp = self.units[c as usize].hp + gain;
        self.set_unit_hp(d, c, hp);
        let left = self.units[c as usize].remaining_build_time;
        if left != 0 {
            if self.game_options & 2 != 0 {
                let v = left.wrapping_sub(0x10);
                self.units[c as usize].remaining_build_time = if (v as i16) < 0 { 0 } else { v };
            } else {
                self.units[c as usize].remaining_build_time = left - 1;
            }
            return true;
        }
        self.finish_unit_pre(d, c);
        if place && !self.place_unit(d, c) {
            return false;
        }
        self.finish_unit_post(d, c);
        if self.units[c as usize].status & 2 == 0 {
            // 0x48f440: "unit ready" sound for the local player.
            return true;
        }
        let g = d.dat.units.staredit_group_flags[self.units[c as usize].unit_type as usize];
        if g & 5 == 0 && g & 2 != 0 {
            self.take_adjacent_addon(d, c);
        }
        true
    }

    /// 0x466f50: send a freshly trained unit to the builder's rally point.
    pub(super) fn send_to_rally(&mut self, d: &Data, b: u32, c: u32) {
        if b == NIL {
            return;
        }
        let bu = &self.units[b as usize];
        let has_rally = matches!(bu.unit_type, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b);
        if !has_rally || bu.rally_unit == b || bu.rally_pos.0 == 0 {
            return;
        }
        let r = bu.rally_unit;
        if r != NIL && self.units[c as usize].player == self.units[r as usize].player {
            return self.order_with_target(d, c, 0x31, r);
        }
        let p = bu.rally_pos;
        self.order_target(d, c, 6, p, NIL);
    }

    /// 0x45d410: cancel a building morph (hatchery to lair, creep colony to sunken, ...):
    /// 3/4 of the queued type's cost back, the building completes again as it was.
    fn cancel_building_morph(&mut self, d: &Data, c: u32) {
        let un = &mut self.units[c as usize];
        let slot = un.build_queue_slot as usize % 5;
        let (t, p) = (un.build_queue[slot] as usize, un.player as usize);
        un.build_queue[slot] = NONE;
        let hp = un.hp;
        self.players.minerals[p] += (d.dat.units.mineral_cost[t] as i32 * 3) / 4;
        self.players.gas[p] += (d.dat.units.gas_cost[t] as i32 * 3) / 4;
        self.units[c as usize].status |= 1;
        self.modify_completed_counters(d, c, -1, true);
        self.units[c as usize].status &= !1;
        self.finish_unit_pre(d, c);
        self.finish_unit_post(d, c);
        self.set_unit_hp(d, c, hp);
        // 0x48ed50: the cancel sound (UI).
        let s = self.units[c as usize].sprite;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.image_run_anim(d, i, 0xf);
            i = next;
        }
    }

    /// 0x45da40: cancel a zerg building under construction: the drone comes back with
    /// the hp it had (+0xae) and 3/4 of the cost is refunded.
    fn cancel_zerg_building(&mut self, d: &Data, c: u32) {
        let un = &self.units[c as usize];
        let queued = un.build_queue[un.build_queue_slot as usize % 5];
        if un.status & 1 == 0 && matches!(queued, 0x85 | 0x84 | 0x89 | 0x90 | 0x92) {
            return self.cancel_building_morph(d, c);
        }
        let (t, p) = (un.unit_type, un.player as usize);
        let hp = (un.previous_hp as i32) << 8;
        // 0x42ce70
        self.players.minerals[p] += (d.dat.units.mineral_cost[t as usize] as i32 * 3) >> 2;
        self.players.gas[p] += (d.dat.units.gas_cost[t as usize] as i32 * 3) >> 2;
        self.players.unit_score[p] -= d.dat.units.build_score[0x29] as i32;
        self.players.units_produced[p] -= 1;
        let s = self.units[c as usize].sprite;
        let (x, y) = (self.sprites[s as usize].x, self.sprites[s as usize].y);
        if t == 0x95 {
            // An extractor: a new drone at its position; the extractor dies.
            let Some(drone) = self.create_unit(d, 0x29, x, y, p as u8) else {
                self.show_last_error(p as u8);
                return self.kill_unit(d, c);
            };
            self.finish_unit_pre(d, drone);
            self.finish_unit_post(d, drone);
            self.set_unit_hp(d, drone, hp);
            return self.kill_unit(d, c);
        }
        // 0x488210: sprite 0xb6 one layer above the building.
        if let Some(th) = self.create_thingy(d, 0xb6, x, y, 0) {
            let ts = self.thingies[th as usize].sprite;
            self.sprites[ts as usize].elevation = self.sprites[s as usize].elevation.wrapping_add(1);
            self.sprite_fog_visibility(ts);
        }
        // 0x48ed50: the cancel sound (UI).
        self.transform_unit(d, c, 0x29);
        if self.units[c as usize].ai != NIL {
            todo_port!("0x433fe0", "AI: a computer's cancelled building");
        }
        self.finish_unit_pre(d, c);
        self.finish_unit_post(d, c);
        // The shadow (last image, draw function 10) sits 7 pixels down.
        let s = self.units[c as usize].sprite;
        let tail = self.sprites[s as usize].images.tail;
        if tail != NIL {
            let im = &mut self.images[tail as usize];
            if im.modifier == 10 && im.y != 7 {
                im.flags |= super::sprite::iflag::REDRAW;
                im.y = 7;
            }
        }
        // 0x4983a0: image screen positions (UI).
        self.order_now(d, c, 0x96);
        let idle = d.dat.units.ai_return_to_idle[0x29];
        self.issue_order(d, c, idle, false, (0, 0), NIL, 0xe4);
        self.set_unit_hp(d, c, hp);
        self.creep_start_receding(d, t, x as i32, y as i32, false);
    }

    /// 0x468280: cancel a unit under construction or in production: refund it (75% for
    /// buildings), then cocoons turn back into their unit and anything else dies.
    pub(super) fn cancel_unit(&mut self, d: &Data, c: u32) {
        let un = &self.units[c as usize];
        if un.sprite == NIL || un.order == 0 || un.status & 1 != 0 {
            return;
        }
        let t = un.unit_type;
        if matches!(t, 0x2c | 0x67 | 0x3e | 0x2b | 0x26) || (t == 0x86 && un.ptr_at(0xd0) != NIL) {
            return;
        }
        let p = un.player as usize;
        if un.status & 2 != 0 {
            if Self::supply_race_of(d, t) == 0 {
                return self.cancel_zerg_building(d, c);
            }
            // 0x42ce70
            self.players.minerals[p] += (d.dat.units.mineral_cost[t as usize] as i32 * 3) >> 2;
            self.players.gas[p] += (d.dat.units.gas_cost[t as usize] as i32 * 3) >> 2;
        } else {
            let paid = if matches!(t, 0x24 | 0x3b | 0x61) { un.build_queue[un.build_queue_slot as usize % 5] } else { t };
            self.refund(d, p, paid);
        }
        match t {
            0x3b | 0x61 => {
                self.transform_unit(d, c, if t == 0x3b { 0x2b } else { 0x26 });
                let un = &mut self.units[c as usize];
                un.remaining_build_time = 0;
                let slot = un.build_queue_slot as usize;
                un.build_queue[slot] = NONE;
                let prev = un.previous_unit_type as usize;
                let img = d.dat.sprites.image[d.dat.flingy.sprite[d.dat.units.flingy[prev] as usize] as usize];
                let s = un.sprite;
                self.replace_sprite_images(d, s, img, 0);
                self.units[c as usize].order_signal &= !4;
                let mut i = self.sprites[s as usize].images.head;
                while i != NIL {
                    let next = self.images[i as usize].link.next;
                    self.image_run_anim(d, i, 0xe);
                    i = next;
                }
                self.order_now(d, c, 0x29);
            }
            _ => {
                if t == 0xe {
                    let silo = self.units[c as usize].connected_unit;
                    if silo != NIL {
                        self.units[silo as usize].set_ptr_at(0xd0, NIL);
                        self.units[silo as usize].order_state = 0;
                    }
                }
                self.kill_unit(d, c);
            }
        }
    }
}
