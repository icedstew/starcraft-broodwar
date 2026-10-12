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
    fn supply_free(&self, d: &Data, p: usize, t: u16) -> i32 {
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

    /// 0x42cf70: enough supply to make `t`.
    fn supply_ok(&self, d: &Data, p: usize, t: u16) -> bool {
        let flags = d.dat.units.flags[t as usize];
        let mut req = d.dat.units.supply_required[t as usize] as i32;
        if flags & 0x400 != 0 {
            req *= 2;
        }
        if req == 0 || flags & 0x100_0000 != 0 {
            return true;
        }
        if t == 0x67 {
            req -= d.dat.units.supply_required[0x26] as i32;
        }
        let used = Self::supply_race(d, t).map_or(0, |r| self.players.supply_used[r][p]);
        if self.supply_max(d, p, t) < req + used {
            todo_port!("0x48ee30", "not enough supply message");
            return false;
        }
        if self.supply_free(d, p, t) - req < 0 {
            todo_port!("0x48ee30", "not enough supply message");
            return false;
        }
        true
    }

    /// 0x42d140: can pay for `t` (and, with `check_supply`, house it).
    fn can_afford(&self, d: &Data, p: usize, t: u16, check_supply: bool) -> bool {
        if check_supply && !self.supply_ok(d, p, t) {
            return false;
        }
        // 0x42d0c0
        let (m, g) = (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32);
        self.players.minerals[p] >= m && self.players.gas[p] >= g
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
    fn refund(&mut self, d: &Data, p: usize, t: u16) {
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
                    todo_port!("0x4e6340", "nuke goes into the silo");
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
    fn create_trainee(&mut self, d: &Data, b: u32, t: u16, show_error: bool) -> u32 {
        let bu = &self.units[b as usize];
        let ok_builder = bu.status & 2 != 0 || matches!(bu.unit_type, 0x48 | 0x52 | 0x53 | 0x51 | 0x24);
        let p = bu.player;
        let _ = show_error;
        if !ok_builder || !self.supply_ok(d, p as usize, t) {
            return NIL;
        }
        let (x, y) = self.unit_sprite_pos(b);
        match self.create_unit(d, t, x, y, p) {
            Some(n) => n,
            None => {
                todo_port!("0x49e530", "show the creation error");
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
            todo_port!("0x4404a0", "terran building completion (addon / AI hooks)");
        }
        true
    }

    /// 0x466f50: send a freshly trained unit to the builder's rally point.
    fn send_to_rally(&mut self, d: &Data, b: u32, c: u32) {
        let bu = &self.units[b as usize];
        let has_rally = matches!(bu.unit_type, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b);
        if !has_rally || bu.rally_unit == b || bu.rally_pos.0 == 0 {
            return;
        }
        let r = bu.rally_unit;
        if r != NIL && self.units[c as usize].player == self.units[r as usize].player {
            todo_port!("0x466f50", "rally on a unit: Follow (0x31)");
            return;
        }
        let p = bu.rally_pos;
        self.order_target(d, c, 6, p, NIL);
    }

    /// 0x468280: cancel a unit under construction (refund and remove).
    fn cancel_unit(&mut self, d: &Data, c: u32) {
        let _ = d;
        todo_port!("0x468280", "cancel a unit under construction");
        let _ = c;
    }
}
