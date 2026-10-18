//! Research and upgrades: the commands that start and cancel them, the ResearchTech and
//! Upgrade orders that count them down, and what finishing does.
//!
//! Building fields (1.16.1 offsets): +0xc6 time left, +0xc8 tech being researched (0x2c =
//! none), +0xc9 upgrade in progress (0x3d = none), +0xcd level the upgrade reaches.

use super::list::NIL;
use super::{Data, State};

const NO_TECH: u8 = 0x2c;
const NO_UPGRADE: u8 = 0x3d;

impl State {
    /// 0x4c1ba0: command 0x30, research `tech` with the one selected unit.
    pub fn command_research(&mut self, d: &Data, player: u8, sel: &[u32], tech: u8) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        if self.can_research(d, u, tech as u16, player) == Ok(1) && self.start_research(d, u, tech) {
            self.start_order_now(d, u, 0x4b);
            // 0x68c1b0 / 0x68ac74 / 0x68c1f8 / 0x68c1e8: the command card is redrawn.
        }
    }

    /// 0x4c1b20: command 0x32, upgrade `up` with the one selected unit.
    pub fn command_upgrade(&mut self, d: &Data, player: u8, sel: &[u32], up: u8) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        if self.can_upgrade(d, u, up as u16, player) == Ok(1) && self.start_upgrade(d, u, up) {
            self.start_order_now(d, u, 0x4c);
        }
    }

    /// 0x4c0070: command 0x31, cancel the selected building's research.
    pub fn command_cancel_research(&mut self, d: &Data, player: u8, sel: &[u32]) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        let un = &self.units[u as usize];
        if d.dat.units.flags[un.unit_type as usize] & 1 != 0 && un.status & 1 != 0 && un.player == player {
            self.cancel_research(d, u);
            // 0x4c36c0: the command card is redrawn.
        }
    }

    /// 0x4bffc0: command 0x33, cancel the selected unit's upgrade.
    pub fn command_cancel_upgrade(&mut self, d: &Data, player: u8, sel: &[u32]) {
        let units = self.live_selection(sel);
        let [u] = units[..] else { return };
        let un = &self.units[u as usize];
        if un.status & 1 != 0 && un.player == player {
            self.cancel_upgrade(d, u);
        }
    }


    /// Cost check message of 0x42d0c0 / 0x454b70: the advisor sound is the local
    /// player's race's (0x57f1e2).
    fn can_pay(&mut self, player: u8, minerals: i32, gas: i32) -> bool {
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[player as usize] < minerals {
            self.local_error(player, 0x93 + race, 0x352);
            return false;
        }
        if self.players.gas[player as usize] < gas {
            self.local_error(player, 0x96 + race, 0x353);
            return false;
        }
        true
    }

    /// 0x454b70: pay for and start researching `tech` in `u`.
    pub(super) fn start_research(&mut self, d: &Data, u: u32, tech: u8) -> bool {
        if tech > 0x2b {
            return false;
        }
        let t = tech as usize;
        let p = self.units[u as usize].player;
        let (m, g) = (d.dat.techdata.mineral_cost[t] as i32, d.dat.techdata.gas_cost[t] as i32);
        if !self.can_pay(p, m, g) {
            return false;
        }
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc8, tech);
        un.set_u16_at(0xc6, d.dat.techdata.research_time[t]);
        self.players.minerals[p as usize] -= m;
        self.players.gas[p as usize] -= g;
        self.players.research_in_progress[p as usize][t >> 3] |= 1 << (t & 7);
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x13);
        true
    }

    /// 0x453ed0: gas cost of the next level of upgrade `up` for `player`.
    pub fn upgrade_gas_cost(&self, d: &Data, player: u8, up: u8) -> u32 {
        let ud = &d.dat.upgrades;
        let i = up as usize;
        ud.gas_factor[i] as u32 * self.players.upgrade_level(player, i) as u32 + ud.gas_base[i] as u32
    }

    /// 0x453f20: mineral cost of the next level of upgrade `up` for `player`.
    pub fn upgrade_mineral_cost(&self, d: &Data, player: u8, up: u8) -> u32 {
        let ud = &d.dat.upgrades;
        let i = up as usize;
        ud.mineral_factor[i] as u32 * self.players.upgrade_level(player, i) as u32 + ud.mineral_base[i] as u32
    }

    /// 0x42d190 / 0x453f70 (minerals, gas, time) of the next level of `up` for `player`.
    pub fn upgrade_cost(&self, d: &Data, player: u8, up: u8) -> (i32, i32, u16) {
        let lvl = self.players.upgrade_level(player, up as usize) as u32;
        let ud = &d.dat.upgrades;
        let i = up as usize;
        let t = (ud.time_factor[i] as u32 * lvl + ud.time_base[i] as u32) as u16;
        (self.upgrade_mineral_cost(d, player, up) as i32, self.upgrade_gas_cost(d, player, up) as i32, t)
    }

    /// 0x454a80: pay for and start upgrade `up` in `u`.
    pub(super) fn start_upgrade(&mut self, d: &Data, u: u32, up: u8) -> bool {
        if up >= NO_UPGRADE {
            return false;
        }
        let p = self.units[u as usize].player;
        let (m, g, time) = self.upgrade_cost(d, p, up);
        if !self.can_pay(p, m, g) {
            return false;
        }
        let lvl = self.players.upgrade_level(p, up as usize);
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc9, up);
        un.set_u16_at(0xc6, time);
        un.set_u8_at(0xcd, lvl.wrapping_add(1));
        self.players.minerals[p as usize] -= m;
        self.players.gas[p as usize] -= g;
        self.players.upgrade_in_progress[p as usize][(up >> 3) as usize] |= 1 << (up & 7);
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x13);
        true
    }

    /// 0x453d30: refund 3/4 of the mineral and gas cost of `tech` to `player`.
    fn refund_tech(&mut self, d: &Data, player: u8, tech: u8) {
        let t = tech as usize;
        self.players.minerals[player as usize] += (d.dat.techdata.mineral_cost[t] as i32 * 3) >> 2;
        self.players.gas[player as usize] += (d.dat.techdata.gas_cost[t] as i32 * 3) >> 2;
    }

    /// 0x453dd0: a dying building drops its research, refunding 3/4 (0x453d30).
    pub(super) fn abort_research(&mut self, d: &Data, u: u32) {
        let tech = self.units[u as usize].u8_at(0xc8);
        if tech == NO_TECH {
            return;
        }
        let t = tech as usize;
        let p = self.units[u as usize].player as usize;
        self.refund_tech(d, p as u8, tech);
        self.players.research_in_progress[p][t >> 3] &= !(1 << (t & 7));
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc8, NO_TECH);
        un.set_u16_at(0xc6, 0);
    }

    /// 0x4540b0: refund 3/4 of the next-level cost of upgrade `up` to `player` (16-bit costs).
    fn refund_upgrade_partial(&mut self, d: &Data, player: u8, up: u8) {
        let m = self.upgrade_mineral_cost(d, player, up) as u16 as i32;
        let g = self.upgrade_gas_cost(d, player, up) as u16 as i32;
        self.players.minerals[player as usize] += (m * 3) >> 2;
        self.players.gas[player as usize] += (g * 3) >> 2;
    }

    /// 0x454170: refund the full next-level cost of upgrade `up` to `player` (16-bit costs).
    fn refund_upgrade_full(&mut self, d: &Data, player: u8, up: u8) {
        let m = self.upgrade_mineral_cost(d, player, up) as u16 as i32;
        let g = self.upgrade_gas_cost(d, player, up) as u16 as i32;
        self.players.minerals[player as usize] += m;
        self.players.gas[player as usize] += g;
    }

    /// 0x454220: a dying building drops its upgrade, refunding 3/4 (0x4540b0).
    pub(super) fn abort_upgrade(&mut self, d: &Data, u: u32) {
        let up = self.units[u as usize].u8_at(0xc9);
        if up == NO_UPGRADE {
            return;
        }
        let p = self.units[u as usize].player;
        self.refund_upgrade_partial(d, p, up);
        self.players.upgrade_in_progress[p as usize][(up >> 3) as usize] &= !(1 << (up & 7));
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc9, NO_UPGRADE);
        un.set_u16_at(0xc6, 0);
        un.set_u8_at(0xcd, 0);
    }

    /// 0x453e30: stop the research in `u` and refund it.
    pub fn cancel_research(&mut self, d: &Data, u: u32) {
        let tech = self.units[u as usize].u8_at(0xc8);
        if tech == NO_TECH {
            return;
        }
        let t = tech as usize;
        let p = self.units[u as usize].player as usize;
        self.players.minerals[p] += d.dat.techdata.mineral_cost[t] as i32;
        self.players.gas[p] += d.dat.techdata.gas_cost[t] as i32;
        self.players.research_in_progress[p][t >> 3] &= !(1 << (t & 7));
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc8, NO_TECH);
        un.set_u16_at(0xc6, 0);
        let s = un.sprite;
        self.sprite_images_run_anim(d, s, 0x14);
    }

    /// 0x454280: stop the upgrade in building `u` and refund it (0x454170).
    pub fn cancel_upgrade(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let up = un.u8_at(0xc9);
        if d.dat.units.flags[un.unit_type as usize] & 1 == 0 || up == NO_UPGRADE {
            return;
        }
        let p = un.player;
        self.refund_upgrade_full(d, p, up);
        self.players.upgrade_in_progress[p as usize][(up >> 3) as usize] &= !(1 << (up & 7));
        let un = &mut self.units[u as usize];
        un.set_u8_at(0xc9, NO_UPGRADE);
        un.set_u16_at(0xc6, 0);
        un.set_u8_at(0xcd, 0);
        let s = un.sprite;
        self.sprite_images_run_anim(d, s, 0x14);
    }

    /// 0x4548b0: ResearchTech. Counts the time down; the tech is researched when it runs
    /// out (or at once with the cheat flag 0x6d5a6c & 2), then the building idles.
    pub fn order_research_tech(&mut self, d: &Data, u: u32) {
        let tech = self.units[u as usize].u8_at(0xc8);
        if tech != NO_TECH {
            let p = self.units[u as usize].player;
            let left = self.units[u as usize].u16_at(0xc6);
            self.units[u as usize].set_u16_at(0xc6, left.wrapping_sub(1));
            if left != 0 && !self.tech_researched(p, tech as u16) && self.game_options & 2 == 0 {
                return;
            }
            self.advisor_done(d, u, 0x7b);
            if tech < 0x18 || self.is_expansion {
                self.players.tech_researched[p as usize][tech as usize] = 1;
            }
        }
        self.building_order_done(d, u, 0xc8, NO_TECH);
    }

    /// 0x4033d0: `p`'s level of upgrade `up` is below its maximum.
    pub fn upgrade_below_max(&self, p: u8, up: u8) -> bool {
        self.players.upgrade_level(p, up as usize) < self.players.upgrade_max[p as usize][up as usize]
    }

    /// 0x4546a0: Upgrade. Counts the time down; when it runs out the level is raised
    /// (0x4ce770) and its effects applied (0x454540).
    pub fn order_upgrade(&mut self, d: &Data, u: u32) {
        let up = self.units[u as usize].u8_at(0xc9);
        if self.units[u as usize].status & 2 != 0 && up != NO_UPGRADE {
            let p = self.units[u as usize].player;
            let target = self.units[u as usize].u8_at(0xcd);
            let left = self.units[u as usize].u16_at(0xc6);
            self.units[u as usize].set_u16_at(0xc6, left.wrapping_sub(1));
            if left != 0 && self.players.upgrade_level(p, up as usize) < target && self.game_options & 2 == 0 {
                return;
            }
            self.advisor_done(d, u, 0x83);
            let lvl = self.players.upgrade_level(p, up as usize);
            if lvl < target && self.upgrade_below_max(p, up) {
                // 0x4ce770
                if up < 0x2e {
                    self.players.upgrade_sc[p as usize][up as usize] = target;
                } else if self.is_expansion {
                    self.players.upgrade_bw[p as usize][up as usize - 0x2e] = target;
                }
                self.apply_upgrade(d, p, up);
            }
        }
        self.units[u as usize].set_u8_at(0xcd, 0);
        self.building_order_done(d, u, 0xc9, NO_UPGRADE);
    }

    /// The common end of 0x4548b0 / 0x4546a0: the building's done animation, the
    /// in-progress bit cleared, the field at `field` reset to `none`, then the next order.
    fn building_order_done(&mut self, d: &Data, u: u32, field: usize, none: u8) {
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x14);
        let p = self.units[u as usize].player as usize;
        let v = self.units[u as usize].u8_at(field);
        if field == 0xc8 {
            self.players.research_in_progress[p][(v >> 3) as usize] &= !(1 << (v & 7));
        } else {
            self.players.upgrade_in_progress[p][(v >> 3) as usize] &= !(1 << (v & 7));
        }
        self.units[u as usize].set_u8_at(field, none);
        if field == 0xc9 {
            self.units[u as usize].set_u8_at(0xcd, 0);
        }
        if self.units[u as usize].ai != NIL {
            // A computer's building queues ComputerAI (0x9c) after its interruptible
            // queued orders (and older 0x9c ones) are dropped.
            self.units[u as usize].order_flags |= 1;
            if self.units[u as usize].order != 0 {
                self.drop_queue_tail_for(d, u, 0x9c);
                self.order_queue_append(d, u, 0x9c, 0xe4, (0, 0), NIL);
            }
            self.order_nothing(d, u);
            return;
        }
        if self.units[u as usize].order_queue_head == NIL {
            self.start_order_now(d, u, d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize]);
        } else {
            self.units[u as usize].order_flags |= 1;
            self.order_nothing(d, u);
        }
    }

    /// 0x48f150 (`base` 0x7b, research) / 0x48f070 (0x83, upgrade): the owner's advisor
    /// announces it, by the building's race (units.dat group flags 0x6637a0).
    fn advisor_done(&mut self, d: &Data, u: u32, base: u16) {
        let un = &self.units[u as usize];
        if self.replay || un.player != self.local_player {
            return;
        }
        let g = d.dat.units.staredit_group_flags[un.unit_type as usize];
        let race = if g & 1 != 0 {
            0
        } else if g & 4 != 0 {
            2
        } else if g & 2 != 0 {
            1
        } else {
            4
        };
        // ponytail: the volume gate (0x68c4c3 / 0x64086c) and portrait talk 0x45efe0 are
        // not ported.
        self.ui_sounds.push(base + race);
    }

    /// 0x454540: a finished upgrade's effect on the owner's units: speed upgrades set
    /// status 0x10000000 and recompute speeds (0x454310); Adrenal Glands sets
    /// 0x20000000 (attack speed) only.
    pub fn apply_upgrade(&mut self, d: &Data, player: u8, up: u8) {
        let (types, speed, cooldown): (&[u16], bool, bool) = match up {
            0x11 => (&[2, 0x13], true, false),
            0x1a => (&[0x2a], true, false),
            0x1b => (&[0x25], true, false),
            0x1c => (&[0x25], false, true),
            0x1d => (&[0x26], true, false),
            0x22 => (&[0x41], true, false),
            0x25 => (&[0x45], true, false),
            0x27 => (&[0x54], true, false),
            0x2a => (&[0x46], true, false),
            0x35 => (&[0x27, 0x30], true, false),
            _ => return,
        };
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let next = self.units[u as usize].player_next;
            if types.contains(&self.units[u as usize].unit_type) {
                if cooldown {
                    self.units[u as usize].status |= 0x2000_0000;
                }
                if speed {
                    self.units[u as usize].status |= 0x1000_0000;
                    self.update_speed(d, u);
                }
            }
            u = next;
        }
    }
}
