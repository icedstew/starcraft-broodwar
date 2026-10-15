//! AI towns (tai_town.cpp): a computer player's bases, the workers and buildings each
//! one manages, and the AI region states the towns put their regions into.

use super::ai::{AiTown, BuildingAi, WorkerAi};
use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Unit `+0x134` (the AI) points at one of four pools; the pool is the entry's
/// `ai_type` byte (1 guard, 2 worker, 3 building, 4 military), kept here in the top
/// byte of the reference.
pub const AI_GUARD: u32 = 1;
pub const AI_WORKER: u32 = 2;
pub const AI_BUILDING: u32 = 3;
pub const AI_MILITARY: u32 = 4;

pub fn ai_ref(kind: u32, index: u32) -> u32 {
    kind << 24 | index
}

pub fn ai_kind(r: u32) -> u32 {
    r >> 24
}

pub fn ai_index(r: u32) -> u32 {
    r & 0xff_ffff
}

impl State {
    /// 0x4320d0: the race's gas building (extractor, refinery, assimilator).
    fn refinery_type(&self, player: u8) -> u16 {
        match self.player_races[player as usize] {
            1 => 0x6e,
            0 => 0x95,
            _ => 0x9d,
        }
    }

    /// The race's town hall (command center, hatchery, nexus).
    fn town_hall_type(&self, player: u8) -> u16 {
        match self.player_races[player as usize] {
            1 => 0x6a,
            0 => 0x83,
            _ => 0x9a,
        }
    }

    /// 0x401cf0: `u` is a finished resource depot (or a hatchery turning into a lair /
    /// hive, or a lair / hive still morphing).
    fn is_resource_depot(&self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        if un.status & 1 == 0 && t != 0x85 && t != 0x84 && (t != 0x83 || !self.morphing_zerg_building(u)) {
            return false;
        }
        un.status & 2 != 0 && d.dat.units.flags[t as usize] & 0x1000 != 0
    }

    /// 0x432020: a new town of `player` at (x, y), first in the player's town list.
    fn create_town(&mut self, player: u8, x: i16, y: i16) -> Option<u32> {
        let mut head = self.ai.town_lists[player as usize];
        let t = self.ai.towns.alloc(&mut head)?;
        self.ai.town_lists[player as usize] = head;
        let link = self.ai.towns.items[t as usize].link;
        self.ai.towns.items[t as usize] = AiTown { link, position: (x, y), player, ..AiTown::default() };
        Some(t)
    }

    /// 0x433dd0: town `town` takes `u`: workers get a worker AI, buildings (and larvae,
    /// eggs, overlords) a building AI; the first depot becomes the town's main building
    /// and gas buildings fill its gas slots.
    fn town_add_unit(&mut self, d: &Data, u: u32, town: u32) {
        let t = self.units[u as usize].unit_type;
        let flags = d.dat.units.flags[t as usize];
        if flags & 8 != 0 {
            let mut head = self.ai.towns.items[town as usize].workers;
            let Some(w) = self.ai.workers.alloc(&mut head) else { return };
            self.ai.towns.items[town as usize].workers = head;
            let link = self.ai.workers.items[w as usize].link;
            self.ai.workers.items[w as usize] = WorkerAi {
                link,
                ai_type: 2,
                target_resource: 1,
                reassign_count: 0,
                wait_timer: 0,
                last_update_second: self.trig.elapsed,
                parent: u,
                town,
            };
            self.units[u as usize].set_u8_at(0xc8, 0);
            self.units[u as usize].ai = ai_ref(AI_WORKER, w);
            let tw = &mut self.ai.towns.items[town as usize];
            tw.worker_count = tw.worker_count.wrapping_add(1);
            return;
        }
        if !((flags & 1 != 0 && t != 0xbc) || t == 0x23 || t == 0x24 || t == 0x2a) {
            return;
        }
        let mut head = self.ai.towns.items[town as usize].buildings;
        let Some(b) = self.ai.buildings.alloc(&mut head) else { return };
        self.ai.towns.items[town as usize].buildings = head;
        let link = self.ai.buildings.items[b as usize].link;
        self.ai.buildings.items[b as usize] = BuildingAi { link, ai_type: 3, parent: u, town, ..BuildingAi::default() };
        self.units[u as usize].ai = ai_ref(AI_BUILDING, b);
        if matches!(t, 0x83 | 0x85 | 0x84) && self.units[u as usize].status & 1 != 0 {
            self.order_now(d, u, 0x9c);
            self.set_secondary_order(u, 0x66);
        }
        let player = self.ai.towns.items[town as usize].player;
        let st = self.units[u as usize].status;
        if self.ai.players[player as usize].flags & 0x20 == 0
            || (st & 2 != 0 && !matches!(t, 0x7c | 0x7d | 0x8f | 0x90 | 0x92 | 0x9c | 0xa2))
        {
            self.ai_region_add_building(d, player, u);
        }
        let tw = &mut self.ai.towns.items[town as usize];
        if tw.inited == 0 {
            tw.inited = 1;
            tw.mineral = NIL;
            tw.gas_buildings = [NIL; 3];
        }
        let owner = self.units[u as usize].player;
        let hall = self.town_hall_type(owner);
        if t != hall && !self.is_resource_depot(d, u) {
            let gas = self.refinery_type(owner);
            if t != gas && t != 0xbc {
                return;
            }
            let tw = &mut self.ai.towns.items[town as usize];
            if let Some(slot) = tw.gas_buildings.iter_mut().find(|g| **g == NIL) {
                *slot = u;
            }
            if tw.resource_area != 0 {
                tw.resource_units_not_set = 1;
            }
            return;
        }
        let tw = &mut self.ai.towns.items[town as usize];
        if tw.main_building == NIL {
            tw.main_building = u;
        }
    }

    /// 0x430b50: the units in region `r` (by sprite position, from its box).
    pub(super) fn units_in_region(&mut self, r: u16) -> Vec<u32> {
        let b = self.sai.regions[r as usize].rect;
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
            .into_iter()
            .filter(|&u| {
                let s = &self.sprites[self.units[u as usize].sprite as usize];
                super::path::region_at(&self.sai, s.x as i32, s.y as i32) == r
            })
            .collect()
    }

    /// 0x432ad0: `player` has a building or worker in region `r`.
    fn player_has_base_unit_in(&mut self, d: &Data, r: u16, player: u8) -> bool {
        self.units_in_region(r).into_iter().any(|u| {
            let un = &self.units[u as usize];
            un.player == player && d.dat.units.flags[un.unit_type as usize] & 9 != 0
        })
    }

    /// 0x4390a0: AI region `r` of `player` goes to `state` (from idle its strengths are
    /// reset; the defense priority follows the state).
    pub(super) fn set_ai_region_state(&mut self, d: &Data, player: u8, r: u16, state: u8) {
        let p = player as usize;
        if self.ai.regions[p][r as usize].id == 0 {
            return;
        }
        let pflag20 = self.ai.players[p].flags & 0x20 != 0;
        if self.ai.regions[p][r as usize].state == 0 {
            let unwalkable = self.sai.regions[r as usize].flags == 0x1ffd;
            let v = if pflag20 { self.ai.players[p].default_min_strength_for_regions as u16 } else { 0 };
            let reg = &mut self.ai.regions[p][r as usize];
            reg.local_military_ground_strength = 0;
            reg.local_military_air_strength = 0;
            reg.all_military_ground_strength = 0;
            reg.all_military_air_strength = 0;
            reg.air_target = NIL;
            reg.ground_target = NIL;
            reg.slowest_military = NIL;
            reg.detector = NIL;
            reg.needed_ground_strength = if unwalkable { 0 } else { v };
            reg.needed_air_strength = if unwalkable { v } else { 0 };
            reg.enemy_air_strength = 0;
            reg.enemy_ground_strength = 0;
            if state != 7 {
                reg.target_region_id = 0;
            }
        }
        let reg = &mut self.ai.regions[p][r as usize];
        let old_flags = reg.flags;
        reg.flags &= !0x20;
        reg.state = state;
        let prio = match state {
            0 => {
                if old_flags & 0x40 != 0 && !pflag20 { 0x28 } else { 10 }
            }
            1 | 2 => if pflag20 { 0x46 } else { 0xa0 },
            3 => {
                reg.flags &= 0xde;
                10
            }
            4 => 0x28,
            5 => 0x32,
            7 => {
                reg.unk = 0;
                let target = reg.target_region_id;
                if self.player_has_base_unit_in(d, target, player) {
                    0x6e
                } else {
                    let own = self.ai.regions[p][r as usize].id;
                    if self.player_has_base_unit_in(d, own, player) { 0x6e } else { 100 }
                }
            }
            6 => {
                let own = reg.id;
                if self.player_has_base_unit_in(d, own, player) { 0x6e } else { 100 }
            }
            8 | 9 => if pflag20 { 0x32 } else { 0xa0 },
            _ => 10,
        };
        self.ai.regions[p][r as usize].defense_priority_add = prio;
    }

    /// 0x43a010: a building of `player` makes its region one the AI holds (state 4,
    /// and for allied computers too); town halls and depots get extra defense needs.
    fn ai_region_add_building(&mut self, d: &Data, player: u8, u: u32) {
        let p = player as usize;
        let s = &self.sprites[self.units[u as usize].sprite as usize];
        let r = super::path::region_at(&self.sai, s.x as i32, s.y as i32);
        let st = self.ai.regions[p][r as usize].state;
        if st == 0 || st == 4 {
            self.set_ai_region_state(d, player, r, 4);
            let reg = &mut self.ai.regions[p][r as usize];
            let f = reg.flags;
            reg.flags = f | 1;
            if self.ai.players[p].flags & 0x20 == 0 {
                reg.flags = f | 0x41;
            }
            for q in 0..8usize {
                if self.ai.players[q].flags & 0x20 == 0 && self.alliances[q][p] != 0 && self.ai.regions[q].get(r as usize).is_some_and(|x| x.state == 0) {
                    self.set_ai_region_state(d, q as u8, r, 4);
                }
            }
        }
        let hall = self.town_hall_type(player);
        let depot = self.is_resource_depot(d, u);
        if !depot && self.units[u as usize].unit_type != hall {
            return;
        }
        if self.ai.players[p].flags & 0x20 == 0 {
            self.set_ai_region_state(d, player, r, 4);
            let reg = &mut self.ai.regions[p][r as usize];
            reg.needed_ground_strength = 0x5dc;
            reg.needed_air_strength = 1000;
            reg.flags |= 0x40;
            reg.defense_priority_add = 0x2d;
        } else {
            let v = self.ai.players[p].default_min_strength_for_regions as u16 * 6;
            let reg = &mut self.ai.regions[p][r as usize];
            reg.needed_ground_strength = v;
            reg.needed_air_strength = v;
            self.set_ai_region_state(d, player, r, 4);
            self.ai.regions[p][r as usize].defense_priority_add = 0x2d;
        }
    }

    /// 0x431f90: the town's next free request slot (count 0) gets (kind 0 unit / 1
    /// upgrade / 2 tech, count, id, priority, flag). Computers only.
    pub(super) fn town_request(&mut self, town: u32, kind: u8, count: u8, id: u16, priority: u8, flag: u8) {
        if town == NIL {
            return;
        }
        let tw = &mut self.ai.towns.items[town as usize];
        if self.player_types[tw.player as usize] != 1 {
            return;
        }
        let Some(slot) = tw.town_units.iter_mut().find(|r| r.0 & 0xf8 == 0) else { return };
        *slot = ((count << 2 | kind & 3) << 1 | flag & 1, priority, id);
    }

    /// 0x4323d0: build `count` (< 0x1f) of unit type `t` (< 0xe4).
    pub(super) fn town_build(&mut self, town: u32, count: u8, t: u16, priority: u8, flag: u8) {
        if count < 0x1f && t < 0xe4 {
            self.town_request(town, 0, count, t, priority, flag);
        }
    }

    /// 0x466b70: `u` is about to produce unit type `t`: a worker ordered to build it
    /// (unless the building already exists), else how many of `t` its queue holds (just
    /// the current slot for eggs, cocoons and morphing units).
    fn producing(&self, d: &Data, u: u32, t: u16) -> u32 {
        let un = &self.units[u as usize];
        let ut = un.unit_type;
        let q = |i: usize| un.build_queue[(un.build_queue_slot as usize + i) % 5];
        if d.dat.units.flags[ut as usize] & 8 == 0 {
            if !matches!(ut, 0x24 | 0x3b | 0x61) && un.order != 0x2b && un.order != 0x2d {
                return (0..5).filter(|&i| q(i) == t).count() as u32;
            }
            return (un.build_queue[un.build_queue_slot as usize % 5] == t) as u32;
        }
        let o = un.order;
        let b = if matches!(o, 0x1e | 0x22 | 0x21) {
            Some(un.order_target_unit)
        } else if o == 0x1f && un.status & 2 != 0 && un.order_target_unit != NIL && self.units[un.order_target_unit as usize].status & 1 == 0 {
            // 0x4e4c40
            Some(un.order_target_unit)
        } else if un.secondary_order == 0x26 || un.secondary_order == 0x3f || (un.secondary_order == 0x25 && un.status & 2 != 0 && un.current_build_unit != NIL && self.units[un.current_build_unit as usize].status & 1 == 0) {
            Some(un.current_build_unit)
        } else {
            None
        };
        if let Some(b) = b {
            if b != NIL {
                return 0;
            }
        }
        (q(0) == t && matches!(o, 0x1e | 0x1f | 0x46 | 0x1a | 0x19)) as u32
    }

    /// One unit of a town counted by 0x433210.
    fn town_unit_matches(&self, d: &Data, u: u32, t: u16, completed: bool) -> u32 {
        let un = &self.units[u as usize];
        if completed && un.status & 1 == 0 {
            return 0;
        }
        if un.unit_type == t {
            1
        } else if !completed {
            self.producing(d, u, t)
        } else {
            0
        }
    }

    /// 0x433210: town units of type `t` (finished only, or also being made); hatcheries
    /// count lairs and hives, lairs hives, creep colonies sunkens and spores, spires
    /// greater spires.
    pub(super) fn town_unit_count(&self, d: &Data, town: u32, t: u16, completed: bool) -> u32 {
        let tw = &self.ai.towns.items[town as usize];
        let mut n = 0u32;
        let mut w = tw.workers;
        while w != NIL {
            let u = self.ai.workers.items[w as usize].parent;
            n = n.wrapping_add(self.town_unit_matches(d, u, t, completed));
            w = self.ai.workers.items[w as usize].link.next;
        }
        let mut b = tw.buildings;
        while b != NIL {
            let u = self.ai.buildings.items[b as usize].parent;
            n = n.wrapping_add(self.town_unit_matches(d, u, t, completed));
            let extra: &[u16] = match t {
                0x83 => &[0x84, 0x85],
                0x84 => &[0x85],
                0x8f => &[0x92, 0x90],
                0x8d => &[0x89],
                _ => &[],
            };
            for &e in extra {
                n = n.wrapping_add(self.town_unit_matches(d, u, e, completed));
            }
            b = self.ai.buildings.items[b as usize].link.next;
        }
        n
    }

    /// 0x432130: the race's worker.
    pub(super) fn worker_type(&self, player: u8) -> u16 {
        match self.player_races[player as usize] {
            1 => 7,
            0 => 0x29,
            _ => 0x40,
        }
    }

    /// 0x433110: an enemy of `player` has a building on the ground that (unless it sits
    /// where ground units can't path) is in the same region group as `p`.
    pub(super) fn enemy_building_reachable(&self, player: u8, p: (i16, i16)) -> bool {
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 {
                let owner = if un.player == 0xb { self.sprites[un.sprite as usize].owner } else { un.player };
                if self.alliances[player as usize][owner as usize] == 0 && un.status & 2 != 0 && (un.pathing_flags & 1 == 0 || self.same_group(u, p)) {
                    return true;
                }
            }
            u = un.link.next;
        }
        false
    }

    /// 0x4626e0: guards whose unit is gone go back to the pool.
    fn free_dead_guards(&mut self, player: u8) {
        let p = player as usize;
        let mut g = self.ai.guard_lists[p];
        while g != NIL {
            let next = self.ai.guards.items[g as usize].link.next;
            if self.ai.guards.items[g as usize].parent == NIL {
                let mut head = self.ai.guard_lists[p];
                self.ai.guards.release(&mut head, g);
                self.ai.guard_lists[p] = head;
            }
            g = next;
        }
    }

    /// 0x434220: start_town: a town at (x, y) with `player`'s finished units that have no
    /// AI yet; a new AI (no flag 0x20) also claims the regions around (0x445aa0,
    /// 0x43a8f0).
    pub(super) fn start_town(&mut self, d: &Data, player: u8, x: i16, y: i16, flags: u8) -> Option<u32> {
        let p = player as usize;
        let mut u = self.player_units[p];
        if u == NIL {
            return None;
        }
        let first = self.ai.town_lists[p] == NIL;
        if first {
            self.free_dead_guards(player);
        }
        let town = self.create_town(player, x, y)?;
        let tw = &mut self.ai.towns.items[town as usize];
        tw.resource_area = flags;
        tw.resource_units_not_set = (flags != 0) as u8;
        while u != NIL {
            let next = self.units[u as usize].player_next;
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && un.status & 1 != 0 && un.order != 0x6a && un.order != 0x96 && un.ai == NIL {
                self.town_add_unit(d, u, town);
            }
            u = next;
        }
        if self.ai.players[p].flags & 0x20 == 0 {
            let _ = if first { 5 } else { 2 };
            todo_port!("0x445aa0", "a new town claims the regions around it");
            todo_port!("0x43a8f0", "AI regions near the enemy start");
        }
        Some(town)
    }
}
