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
        if super::unit::is_hatchery(t) && self.units[u as usize].status & 1 != 0 {
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
        if t != hall && !self.is_drop_off(d, u) {
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
    pub(super) fn player_has_base_unit_in(&mut self, d: &Data, r: u16, player: u8) -> bool {
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
        let depot = self.is_drop_off(d, u);
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

    /// 0x4325a0: `player`'s level of upgrade `up` plus the units upgrading it now.
    fn upgrade_level_or_going(&self, player: u8, up: u16) -> u32 {
        let mut n = self.players.upgrade_level(player, up as usize) as u32;
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order == 0x4c && un.u8_at(0xc9) as u16 == up {
                n += 1;
            }
            u = un.player_next;
        }
        n
    }

    /// 0x432530: `player` has tech `t` or a unit is researching it.
    fn tech_have_or_going(&self, player: u8, t: u16) -> bool {
        if self.tech_researched(player, t) {
            return true;
        }
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order == 0x4b && un.u8_at(0xc8) as u16 == t {
                return true;
            }
            u = un.player_next;
        }
        false
    }

    /// 0x4326a0: the town's own buildings keep only their current build queue entry.
    fn town_trim_queues(&mut self, d: &Data, town: u32) {
        let player = self.ai.towns.items[town as usize].player;
        let mut b = self.ai.towns.items[town as usize].buildings;
        while b != NIL {
            let u = self.ai.buildings.items[b as usize].parent;
            if self.units[u as usize].player == player {
                for i in 1..5 {
                    self.cancel_queued(d, u, i);
                }
            }
            b = self.ai.buildings.items[b as usize].link.next;
        }
    }

    /// 0x436320: turn each of `player`'s towns' wanted units, upgrades and techs into
    /// spending requests for what is missing; then the last town's supply, gas and
    /// defense checks (0x433730, 0x43ebb0, 0x43aae0).
    pub(super) fn ai_town_requests(&mut self, d: &Data, player: u8) {
        let p = player as usize;
        self.ai.players[p].flags |= 0x400;
        let mut counter = 0u32;
        let mut too_many_workers = false;
        let mut town = self.ai.town_lists[p];
        let mut last = NIL;
        while town != NIL {
            self.town_trim_queues(d, town);
            let tw = &self.ai.towns.items[town as usize];
            if tw.workers == NIL {
                let m = tw.main_building;
                if m == NIL || self.units[m as usize].status & 1 != 0 || self.units[m as usize].unit_type != 0x83 {
                    self.town_take_workers(d, town, 1);
                }
            } else if tw.worker_count > tw.worker_limit {
                too_many_workers = true;
            }
            last = town;
            for slot in 0..100 {
                let (flags, _, id) = self.ai.towns.items[town as usize].town_units[slot];
                if flags & 0xf8 == 0 {
                    break;
                }
                let kind = flags >> 1 & 3;
                let have = match kind {
                    0 => {
                        let n = if flags & 1 != 0 {
                            self.players.unit_counts[id as usize][p] as u32
                        } else {
                            self.town_count_making(d, town, id)
                        };
                        if self.ai.towns.items[town as usize].in_battle != 0 {
                            continue;
                        }
                        n
                    }
                    1 => self.upgrade_level_or_going(player, id),
                    _ => self.tech_have_or_going(player, id) as u32,
                };
                let mut want = (flags >> 3) as u32;
                let worker = match self.player_races[p] {
                    1 => 7,
                    0 => 0x29,
                    _ => 0x40,
                };
                let limit = self.ai.towns.items[town as usize].worker_limit as u32;
                if id == worker && want > limit {
                    want = limit;
                }
                if have >= want {
                    continue;
                }
                for _ in 0..want - have {
                    let (flags, prio, id) = self.ai.towns.items[town as usize].town_units[slot];
                    let mut priority = prio;
                    let kind = flags >> 1 & 3;
                    let fl = self.ai.players[p].flags;
                    if (kind == 1 || kind == 2) && fl & 0x20 != 0 {
                        if (priority as u32) < counter {
                            priority = 0;
                        } else {
                            priority -= counter as u8;
                            counter += 1;
                        }
                        self.ai.players[p].flags = fl & 0xfbff;
                    }
                    let tp = self.ai.towns.items[town as usize].player;
                    if self.ai.players[tp as usize].flags & 0x20 == 0 && flags & 6 == 0 && id == self.town_hall_type(tp) && self.players.minerals[tp as usize] < 600 {
                        priority = 0x96;
                    }
                    let ty = match kind {
                        0 => {
                            let uf = d.dat.units.flags[id as usize];
                            if uf & 8 != 0 {
                                4
                            } else {
                                ((uf & 2) << 1 | 3) as u8
                            }
                        }
                        1 => 5,
                        _ => 6,
                    };
                    self.ai_add_need(d, tp, ty, id);
                    self.ai_push_request(tp, ty, id, priority, town);
                }
            }
            let tw = &self.ai.towns.items[town as usize];
            if tw.worker_count > 1 || (tw.worker_count == 1 && tw.main_building != NIL && self.player_races[tw.player as usize] == 0) {
                self.town_gas_request(d, town);
            }
            town = self.ai.towns.items[town as usize].link.next;
        }
        if too_many_workers {
            let mut t = self.ai.town_lists[p];
            while t != NIL {
                let tw = &self.ai.towns.items[t as usize];
                let m = tw.main_building;
                if m != NIL && self.units[m as usize].status & 1 != 0 && tw.worker_count < tw.worker_limit {
                    self.town_take_workers(d, t, 1);
                }
                t = self.ai.towns.items[t as usize].link.next;
            }
        }
        if last != NIL {
            self.town_supply_request(d, last);
            todo_port!("0x43ebb0", "AI town defense requests");
            todo_port!("0x43aae0", "AI town requests 0x43aae0");
        }
    }

    /// The count 0x436320 makes of unit type `t` in a town: its workers and buildings
    /// that are `t` or about to make one (0x466b70), with a hatchery also counting
    /// lairs and hives, a lair hives, a spire greater spires and a creep colony sunken
    /// and spore colonies.
    fn town_count_making(&self, d: &Data, town: u32, t: u16) -> u32 {
        let one = |u: u32, t: u16| if self.units[u as usize].unit_type == t { 1 } else { self.producing(d, u, t) };
        let mut n = 0u32;
        let mut w = self.ai.towns.items[town as usize].workers;
        while w != NIL {
            n += one(self.ai.workers.items[w as usize].parent, t);
            w = self.ai.workers.items[w as usize].link.next;
        }
        let mut b = self.ai.towns.items[town as usize].buildings;
        while b != NIL {
            let u = self.ai.buildings.items[b as usize].parent;
            n += one(u, t);
            let extra: &[u16] = match t {
                0x83 => &[0x84, 0x85],
                0x84 => &[0x85],
                0x8d => &[0x89],
                0x8f => &[0x92, 0x90],
                _ => &[],
            };
            for &e in extra {
                n += one(u, e);
            }
            b = self.ai.buildings.items[b as usize].link.next;
        }
        n
    }

    /// 0x435900: a set-up town finds its resource area (0x443a20), gas buildings
    /// (0x444460) and mineral field (0x4445e0), works out how many workers it wants
    /// (+0x1b) and how many go for gas, resets workers idle for a minute (0x9c) and
    /// hands out the targets: 1 minerals, 2.. gas.
    pub(super) fn town_step(&mut self, d: &Data, town: u32) {
        let tw = &self.ai.towns.items[town as usize];
        if tw.inited == 0 {
            return;
        }
        let player = tw.player;
        if tw.resource_area == 0 {
            let (x, y) = tw.position;
            let a = self.nearest_resource_area(player, x, y);
            let tw = &mut self.ai.towns.items[town as usize];
            tw.resource_area = a as u8;
            tw.resource_units_not_set = 1;
        }
        if self.ai.towns.items[town as usize].resource_units_not_set != 0 {
            self.ai.towns.items[town as usize].resource_units_not_set = 0;
            let a = self.ai.towns.items[town as usize].resource_area as u32;
            let gas = self.area_gas_buildings(d, a);
            self.ai.towns.items[town as usize].gas_buildings = gas;
            for g in gas {
                if g != NIL {
                    self.ai_region_add_building(d, player, g);
                }
            }
        }
        if self.ai.towns.items[town as usize].mineral == NIL {
            let a = self.ai.towns.items[town as usize].resource_area as u32;
            let m = self.area_first_mineral(a);
            self.ai.towns.items[town as usize].mineral = m;
            if m != NIL {
                self.ai_region_add_building(d, player, m);
            }
        }
        let area = self.ai.towns.items[town as usize].resource_area as u32;
        if area != 0 {
            let limit = if area < 0xfa { self.area_mining_spots(area, player) + self.areas[area as usize].geysers as u32 * 3 } else { 0 };
            self.ai.towns.items[town as usize].worker_limit = limit as u8;
        }
        let mut mineral_kind = 1u8;
        let mut gas_kind = 2u8;
        if self.town_gas(town, 3, 1) == NIL && self.town_gas(town, 2, 1) == NIL {
            gas_kind = 1;
        } else if self.ai.towns.items[town as usize].mineral == NIL {
            mineral_kind = 2;
        }
        let (gas_pct, _) = self.ai_need_split(player);
        let workers = self.ai.towns.items[town as usize].worker_count as u32;
        let mut on_gas = workers * gas_pct as u32 / 100;
        let geyser_room = if area < 0xfa { self.areas[area as usize].geysers as u32 * 4 } else { 0 };
        let worker_type = match self.player_races[player as usize] {
            1 => 7,
            0 => 0x29,
            _ => 0x40,
        };
        let have = self.players.completed_counts[worker_type][player as usize] as u32;
        if on_gas > geyser_room {
            on_gas = geyser_room;
        } else if self.players.gas[player as usize] < 800 {
            let min = if have >= 0x14 {
                4
            } else if have >= 0xf {
                3
            } else if have >= 10 {
                2
            } else if have >= 5 {
                1
            } else {
                0
            };
            if on_gas < min {
                on_gas = min;
            }
        }
        let mut n = 0u32;
        let mut w = self.ai.towns.items[town as usize].workers;
        while w != NIL {
            let u = self.ai.workers.items[w as usize].parent;
            let un = &self.units[u as usize];
            let now = self.trig.elapsed;
            if matches!(un.order, 0x21 | 0x60 | 0x1a) || un.status & 0x40 != 0 {
                self.ai.workers.items[w as usize].last_update_second = now;
            } else if now.wrapping_sub(self.ai.workers.items[w as usize].last_update_second) >= 0x3c {
                self.order_now(d, u, 0x9c);
                let wk = &mut self.ai.workers.items[w as usize];
                wk.wait_timer = 10;
                wk.last_update_second = now;
                wk.reassign_count = wk.reassign_count.wrapping_add(1);
                let t = wk.town;
                self.town_take_workers(d, t, 1);
            }
            let kind = if on_gas != 0 {
                on_gas -= 1;
                gas_kind
            } else {
                mineral_kind
            };
            self.ai.workers.items[w as usize].target_resource = kind;
            if kind != 1 {
                n += 1;
                if n >= 4 {
                    n = 0;
                    if gas_kind == kind {
                        gas_kind += 1;
                    }
                    if mineral_kind == kind {
                        mineral_kind += 1;
                    }
                    if gas_kind > 4 {
                        gas_kind = 2;
                    }
                    if mineral_kind > 4 {
                        mineral_kind = 2;
                    }
                }
            }
            w = self.ai.workers.items[w as usize].link.next;
        }
    }

    /// 0x444460: the (up to 3) geysers / gas buildings of area `a`, nearest first.
    fn area_gas_buildings(&self, d: &Data, a: u32) -> [u32; 3] {
        let mut g = [NIL; 3];
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            let t = un.unit_type;
            if un.sprite != NIL && un.order != 0 && d.dat.units.flags[t as usize] & 0x2000 != 0 && !(0xb0..=0xb2).contains(&t) && un.u8_at(0xd8) as u32 == a {
                if g[0] == NIL {
                    g[0] = u;
                } else if g[1] == NIL {
                    g[1] = u;
                } else {
                    g[2] = u;
                }
            }
            u = un.link.next;
        }
        let ap = self.areas[a as usize].pos;
        let dist = |st: &State, u: u32| {
            if u == NIL {
                return 99_999_999u32;
            }
            let p = st.unit_sprite_pos(u);
            super::path::approx_distance(ap.0 as i32, p.0 as i32, ap.1 as i32, p.1 as i32)
        };
        let (mut d0, mut d1, d2) = (dist(self, g[0]), dist(self, g[1]), dist(self, g[2]));
        if d0 > d1 {
            g.swap(0, 1);
            std::mem::swap(&mut d0, &mut d1);
        }
        if d1 > d2 {
            g.swap(1, 2);
            d1 = d2;
        }
        if d0 > d1 {
            g.swap(0, 1);
        }
        g
    }

    /// 0x4445e0: the first mineral field of area `a`.
    fn area_first_mineral(&self, a: u32) -> u32 {
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && (0xb0..=0xb2).contains(&un.unit_type) && un.u8_at(0xd8) as u32 == a {
                return u;
            }
            u = un.link.next;
        }
        NIL
    }

    /// 0x4a1e50 (and the same steps in 0x475710 / 0x4a0740): unit `u` leaves the AI:
    /// region references and enemy strength (0x439d60), its town (0x434c90), a guard
    /// post loses it, the free medic is replaced (0x4477c0).
    pub(super) fn ai_release_unit(&mut self, d: &Data, u: u32, keep_region: bool) {
        self.ai_forget_unit(d, u, keep_region);
        self.town_remove_unit(d, u);
        let r = self.units[u as usize].ai;
        if r != NIL && ai_kind(r) == AI_GUARD {
            let g = &mut self.ai.guards.items[ai_index(r) as usize];
            g.parent = NIL;
            if g.times_died < 100 {
                g.times_died += 1;
            }
        }
        // 0x4477c0
        if self.units[u as usize].unit_type == 0x22 {
            let p = self.units[u as usize].player as usize;
            if self.ai.players[p].free_medic == u {
                self.ai.players[p].free_medic = NIL;
                let mut m = self.player_units[p];
                while m != NIL {
                    if self.units[m as usize].unit_type == 0x22 {
                        self.ai.players[p].free_medic = m;
                        break;
                    }
                    m = self.units[m as usize].player_next;
                }
            }
        }
        self.units[u as usize].ai = NIL;
    }

    /// 0x439d60: computers forget `u` as a region's target, slowest unit or detector;
    /// enemies of it lose its type's strength from the region it is in; a military
    /// unit leaves its region (which, when `fight_on` and the region is lost, goes to
    /// state 3). True for a military unit.
    fn ai_forget_unit(&mut self, d: &Data, u: u32, fight_on: bool) -> bool {
        let t = self.units[u as usize].unit_type as usize;
        let owner = {
            let un = &self.units[u as usize];
            if un.player == 0xb { self.sprites[un.sprite as usize].owner } else { un.player }
        };
        let n = self.sai.region_count as usize;
        for p in 0..8usize {
            if self.player_types[p] != 1 {
                continue;
            }
            for r in 1..n {
                let reg = &mut self.ai.regions[p][r];
                for f in [&mut reg.air_target, &mut reg.ground_target, &mut reg.slowest_military, &mut reg.detector] {
                    if *f == u {
                        *f = NIL;
                    }
                }
            }
            if self.alliances[p][owner as usize] == 0 {
                let (x, y) = self.unit_sprite_pos(u);
                let r = super::path::region_at(&self.sai, x as i32, y as i32) as usize;
                let (air, ground) = (d.strength[0][t], d.strength[1][t]);
                let reg = &mut self.ai.regions[p][r];
                reg.needed_air_strength = if air < reg.needed_air_strength as u32 { reg.needed_air_strength - air as u16 } else { 0 };
                reg.needed_ground_strength = if ground < reg.needed_ground_strength as u32 { reg.needed_ground_strength - ground as u16 } else { 0 };
            }
        }
        let r = self.units[u as usize].ai;
        if r == NIL || ai_kind(r) != AI_MILITARY {
            return false;
        }
        let m = ai_index(r);
        let (rp, rid) = self.ai.military.items[m as usize].region;
        let state = self.ai.regions[rp as usize][rid as usize].state;
        if state == 8 {
            // 0x447040: the attack force forgets one unit of this type.
            let ty = if t == 0x1e { 5 } else { t as u16 };
            let pl = self.units[u as usize].player as usize;
            if let Some(e) = self.ai.players[pl].attack_force.iter_mut().take(0x3f).find(|e| **e == ty + 1) {
                *e = 0xe5;
            }
        } else if fight_on && state != 0 && !matches!(state, 1 | 8 | 9 | 2) {
            let (x, y) = self.unit_sprite_pos(u);
            let here = super::path::region_at(&self.sai, x as i32, y as i32);
            if here != rid && !self.player_has_base_unit_in(d, rid, rp) {
                self.set_ai_region_state(d, rp, rid, 3);
            }
        }
        // 0x4371d0
        let reg = &mut self.ai.regions[rp as usize][rid as usize];
        if self.units[u as usize].status & 4 == 0 {
            reg.ground_unit_count = reg.ground_unit_count.wrapping_sub(1);
        }
        let mut head = reg.military;
        self.ai.military.release(&mut head, m);
        self.ai.regions[rp as usize][rid as usize].military = head;
        true
    }

    /// 0x434c90: unit `u` leaves its town: a worker (count down) or a building, larva,
    /// egg or overlord; the town forgets it as main building or builder and is removed
    /// when empty (0x4347e0). Other units keep their AI.
    fn town_remove_unit(&mut self, d: &Data, u: u32) -> bool {
        // 0x432760: a resource leaves every town's slots.
        let t = self.units[u as usize].unit_type;
        let flags = d.dat.units.flags[t as usize];
        if flags & 0x2000 != 0 {
            for p in 0..8 {
                let mut tw = self.ai.town_lists[p];
                while tw != NIL {
                    let town = &mut self.ai.towns.items[tw as usize];
                    if town.mineral == u {
                        town.mineral = NIL;
                    }
                    for g in town.gas_buildings.iter_mut() {
                        if *g == u {
                            *g = NIL;
                        }
                    }
                    if town.resource_area != 0 {
                        town.resource_units_not_set = 1;
                    }
                    tw = town.link.next;
                }
            }
        }
        let r = self.units[u as usize].ai;
        if r == NIL {
            return false;
        }
        let building_like = (flags & 1 != 0 && t != 0xbc) || matches!(t, 0x23 | 0x24 | 0x2a);
        let town = if building_like {
            if ai_kind(r) != AI_BUILDING {
                todo_port!("0x434c90", "a building-type unit with a non-town AI");
                return false;
            }
            let b = ai_index(r);
            let town = self.ai.buildings.items[b as usize].town;
            let mut head = self.ai.towns.items[town as usize].buildings;
            self.ai.buildings.release(&mut head, b);
            self.ai.towns.items[town as usize].buildings = head;
            town
        } else {
            if flags & 8 == 0 {
                return false;
            }
            if ai_kind(r) != AI_WORKER {
                todo_port!("0x434c90", "a worker with a non-worker AI");
                return false;
            }
            let w = ai_index(r);
            let town = self.ai.workers.items[w as usize].town;
            let mut head = self.ai.towns.items[town as usize].workers;
            self.ai.workers.release(&mut head, w);
            let tw = &mut self.ai.towns.items[town as usize];
            tw.workers = head;
            tw.worker_count = tw.worker_count.wrapping_sub(1);
            town
        };
        if town != NIL {
            let tw = &mut self.ai.towns.items[town as usize];
            if tw.building_scv == u {
                tw.building_scv = NIL;
            }
            if tw.main_building == u {
                tw.main_building = NIL;
            }
            self.town_remove_if_empty(d, town);
        }
        self.units[u as usize].ai = NIL;
        true
    }

    /// 0x4347e0: a town with nothing left (or only overlords, whose units move to the
    /// nearest other town, 0x434330) is removed with its scripts (0x45ae20); resources
    /// lose their AI marks and its area its town flag.
    fn town_remove_if_empty(&mut self, d: &Data, town: u32) {
        if !self.town_empty_or_merged(d, town) {
            return;
        }
        // 0x45ae20
        let mut s = self.ai.active_scripts;
        while s != NIL {
            let next = self.ai.scripts.items[s as usize].link.next;
            if self.ai.scripts.items[s as usize].town == town {
                let mut head = self.ai.active_scripts;
                self.ai.scripts.release(&mut head, s);
                self.ai.active_scripts = head;
            }
            s = next;
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && d.dat.units.flags[un.unit_type as usize] & 0x2000 != 0 {
                self.units[u as usize].set_u8_at(0xd9, 0);
            }
            u = self.units[u as usize].link.next;
        }
        let tw = &self.ai.towns.items[town as usize];
        let (area, player) = (tw.resource_area as usize, tw.player as usize);
        if area != 0 {
            self.areas[area].flags &= !2;
        }
        let mut head = self.ai.town_lists[player];
        self.ai.towns.release(&mut head, town);
        self.ai.town_lists[player] = head;
    }

    /// 0x434330: town `town` can go: it has no buildings and no workers, or only
    /// overlords and there is another town (the nearest takes its units).
    fn town_empty_or_merged(&mut self, d: &Data, town: u32) -> bool {
        if self.ai.town_merging {
            return false;
        }
        let tw = &self.ai.towns.items[town as usize];
        if tw.buildings == NIL {
            if tw.workers == NIL {
                return true;
            }
        } else {
            let mut b = tw.buildings;
            while b != NIL {
                if self.units[self.ai.buildings.items[b as usize].parent as usize].unit_type != 0x2a {
                    return false;
                }
                b = self.ai.buildings.items[b as usize].link.next;
            }
        }
        let (player, pos) = (tw.player as usize, tw.position);
        let mut best = 99_999_999u32;
        let mut into = NIL;
        let mut o = self.ai.town_lists[player];
        while o != NIL {
            if o != town {
                let op = self.ai.towns.items[o as usize].position;
                let dist = super::path::approx_distance(pos.0 as i32, op.0 as i32, pos.1 as i32, op.1 as i32);
                if dist < best {
                    best = dist;
                    into = o;
                }
            }
            o = self.ai.towns.items[o as usize].link.next;
        }
        if into == NIL {
            return false;
        }
        self.ai.town_merging = true;
        for list in [true, false] {
            let head = |st: &State| if list { st.ai.towns.items[town as usize].buildings } else { st.ai.towns.items[town as usize].workers };
            let mut cur = head(self);
            while cur != NIL && head(self) != NIL {
                let (next, unit) = if list {
                    let b = &self.ai.buildings.items[cur as usize];
                    (b.link.next, b.parent)
                } else {
                    let w = &self.ai.workers.items[cur as usize];
                    (w.link.next, w.parent)
                };
                self.ai_release_unit(d, unit, false);
                self.town_add_unit(d, unit, into);
                cur = next;
            }
        }
        self.ai.town_merging = false;
        true
    }

    /// 0x448400: request building `t` for `town` at `priority` (soon after a building
    /// was hit only gas buildings and urgent ones, 0x8c, go through).
    fn town_request_building(&mut self, d: &Data, player: u8, t: u16, priority: u8, town: u32) {
        let hit = self.ai.players[player as usize].previous_building_hit_second;
        if hit != 0 && self.trig.elapsed.wrapping_sub(hit) < 0x1e && t != self.refinery_type(player) && priority != 0x8c {
            return;
        }
        let flags = d.dat.units.flags[t as usize];
        let ty = if flags & 8 != 0 { 4 } else { ((flags & 2) << 1 | 3) as u8 };
        self.ai_add_need(d, player, ty, t);
        self.ai_push_request(player, ty, t, priority, town);
    }

    /// 0x433660: a town asks for a gas building on its geyser: right away (0x5a) when
    /// it has no working one, else (0x50) once it has 15 workers or all it wants and
    /// 500 minerals; not while minerals are all that is needed.
    fn town_gas_request(&mut self, d: &Data, town: u32) {
        let player = self.ai.towns.items[town as usize].player;
        let (gas_pct, _) = self.ai_need_split(player);
        if gas_pct == 0 && self.ai.towns.items[town as usize].mineral != NIL {
            return;
        }
        let priority = if self.town_gas(town, 3, 1) != NIL {
            if self.town_gas(town, 1, 1) == NIL {
                return;
            }
            let tw = &self.ai.towns.items[town as usize];
            if tw.worker_count < tw.worker_limit && tw.worker_count < 0xf {
                return;
            }
            if self.players.minerals[player as usize] < 500 {
                return;
            }
            0x50
        } else {
            if self.town_gas(town, 1, 1) == NIL {
                return;
            }
            0x5a
        };
        let t = self.refinery_type(player);
        self.town_request_building(d, player, t, priority, town);
    }

    /// 0x433730: more supply when free supply drops under 13 (or a protoss building
    /// lacks power): enough depots / overlords / pylons beyond the town hall's for what
    /// is used plus 13, at 0x78 (0x8c when 8 or less is free).
    fn town_supply_request(&mut self, d: &Data, town: u32) {
        let player = self.ai.towns.items[town as usize].player;
        let p = player as usize;
        let race = self.player_races[p];
        let depot: u16 = match race {
            1 => 0x6d,
            0 => 0x2a,
            _ => 0x9c,
        };
        let worker: u16 = match race {
            1 => 7,
            0 => 0x29,
            _ => 0x40,
        };
        let free = self.supply_free(d, p, worker);
        let want = if self.unpowered_building(d, player) {
            self.player_count_making(d, player, depot) + 1
        } else {
            if free >= 0xd {
                return;
            }
            let g = d.dat.units.staredit_group_flags[depot as usize];
            let r = if g & 1 != 0 { 0 } else if g & 2 != 0 { 1 } else if g & 4 != 0 { 2 } else { 3 };
            // 0x4888c0
            let used = if r < 3 { self.players.supply_used[r][p] } else { 0 };
            let need = (used + 0xd) as u32;
            let hall = self.town_hall_type(player);
            let hall_supply = d.dat.units.supply_provided[hall as usize] as u32;
            if need <= hall_supply {
                return;
            }
            (need - hall_supply) / d.dat.units.supply_provided[depot as usize] as u32 + 1
        };
        if free != 0 && self.ai.players[p].flags & 0x10 != 0 && !self.unpowered_building(d, player) {
            return;
        }
        if want > self.player_count_making(d, player, depot) {
            let priority = if free > 8 { 0x78 } else { 0x8c };
            self.town_request_building(d, player, depot, priority, town);
        }
    }

    /// 0x433410: no pylon is being built and a building of `player` without power
    /// stands in a safe region.
    fn unpowered_building(&self, _d: &Data, player: u8) -> bool {
        let p = player as usize;
        if self.players.completed_counts[0x9c][p] < self.players.unit_counts[0x9c][p] {
            return false;
        }
        let mut u = self.player_units[p];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && un.status & 0x400 != 0 && self.ai_building_safe(u) {
                return true;
            }
            u = un.player_next;
        }
        false
    }

    /// 0x433470: unit type `t` in all of `player`'s towns, made or about to be.
    fn player_count_making(&self, d: &Data, player: u8, t: u16) -> u32 {
        let mut n = 0;
        let mut town = self.ai.town_lists[player as usize];
        while town != NIL {
            n += self.town_count_making(d, town, t);
            town = self.ai.towns.items[town as usize].link.next;
        }
        n
    }

    /// 0x4355c0: town `town` takes up to `n` idle finished workers from the player's
    /// other towns with more than one worker (from the last town backwards).
    pub(super) fn town_take_workers(&mut self, _d: &Data, town: u32, _n: u32) {
        let tw = &self.ai.towns.items[town as usize];
        if tw.in_battle != 0 || (tw.worker_count != 0 && tw.worker_limit <= tw.worker_count) {
            return;
        }
        todo_port!("0x4355c0", "AI town takes workers from other towns");
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

impl State {
    /// 0x4a2450: per-type AI behaviour that replaces the normal unit AI this round
    /// (true when it gave an order).
    pub(super) fn ai_special_unit_order(&mut self, d: &Data, u: u32) -> bool {
        self.ai_decloak_when_safe(d, u);
        let t = self.units[u as usize].unit_type;
        if matches!(t, 2 | 5 | 0xb | 0x45 | 0x2a | 0x47 | 0x48 | 0x53 | 0x7c | 0x90 | 0x92 | 0xa2 | 0xac) {
            todo_port!("0x4a2450", "AI per-type behaviour");
        }
        false
    }

    /// 0x4a1240: a cloaked AI unit (secondary order 0x6d, cloakable) with no threat
    /// (target, auto target, armed enemy within 0x100) decloaks (0x6e), unless its
    /// order is 0x7b..=0x81.
    fn ai_decloak_when_safe(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.secondary_order != 0x6d || d.dat.units.flags[un.unit_type as usize] & 0x200 == 0 {
            return;
        }
        if self.threatened_by_target(d, u) || self.units[u as usize].auto_target != NIL || self.armed_enemy_near(d, u) {
            return;
        }
        if !(0x7b..=0x81).contains(&self.units[u as usize].order) {
            self.set_secondary(u, 0x6e);
        }
    }

    /// 0x476930 (EAX = 0): `u`'s order target is an enemy that can hit it from where it
    /// is (a damaged hallucination never counts; a loaded bunker always does).
    fn threatened_by_target(&self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].order_target_unit;
        if t == NIL {
            return false;
        }
        let tu = &self.units[t as usize];
        let owner = if tu.player == 0xb { self.sprites[tu.sprite as usize].owner } else { tu.player };
        if self.alliances[self.units[u as usize].player as usize][owner as usize] != 0 {
            return false;
        }
        if tu.status & 0x4000_0000 != 0 && self.life_max(d, t) != self.life_now(d, t) {
            return false;
        }
        if self.can_attack(d, t, u, true) {
            return !self.cannot_hit_now(d, t, u);
        }
        tu.unit_type == 0x7d && self.has_loaded_units(t)
    }

    /// 0x441370 with 0x440a20: an armed unit `u`'s player is not allied with stands
    /// within 0x100 of it.
    fn armed_enemy_near(&mut self, d: &Data, u: u32) -> bool {
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x100), y.wrapping_sub(0x100), x.wrapping_add(0x100), y.wrapping_add(0x100)];
        let p = self.units[u as usize].player as usize;
        self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
            let cu = &st.units[c as usize];
            let owner = if cu.player == 0xb { st.sprites[cu.sprite as usize].owner } else { cu.player };
            c != u && st.alliances[p][owner as usize] == 0 && st.is_armed_pub(d, c)
        }) != NIL
    }

    /// 0x435210: a worker AI's round: repair or build what the town needs, else gather.
    pub(super) fn worker_ai(&mut self, d: &Data, u: u32) {
        let r = self.units[u as usize].ai;
        if r == NIL {
            return;
        }
        let w = ai_index(r) as usize;
        let town = self.ai.workers.items[w].town;
        if self.ai.workers.items[w].wait_timer != 0 {
            self.ai.workers.items[w].wait_timer -= 1;
            return;
        }
        self.ai.workers.items[w].last_update_second = self.trig.elapsed;
        let g = d.dat.units.staredit_group_flags[self.units[u as usize].unit_type as usize];
        if g & 5 == 0 && g & 2 != 0 && self.ai_repair(d, u, town) {
            return;
        }
        if self.ai_worker_build(d, town, u) {
            return;
        }
        let tw = &self.ai.towns.items[town as usize];
        let (main, mineral) = (tw.main_building, tw.mineral);
        if main == NIL || !self.is_drop_off(d, main) {
            return;
        }
        let target = self.ai.workers.items[w].target_resource;
        if target == 1 {
            let t = self.units[u as usize].order_target_unit;
            if t != NIL && matches!(self.units[t as usize].unit_type, 0xb0..=0xb2) {
                self.ai_send_gather(d, u, t, NIL);
                return;
            }
            let mut m = mineral;
            if m == NIL || self.units[m as usize].u8_at(0xd9) != 0 {
                let pos = self.unit_sprite_pos(u);
                let few = self.ai.towns.items[town as usize].worker_count <= 0xc;
                m = self.find_free_mineral(d, u, pos, few);
                if m == NIL {
                    m = mineral;
                }
            }
            let gas = match self.town_gas(town, 3, 1) {
                NIL => self.town_gas(town, 2, 1),
                g => g,
            };
            self.ai_send_gather(d, u, m, gas);
            return;
        }
        // Gas: a worker that wants gas (2, 3 or other) takes the n-th working gas
        // building, preferring finished ones (state 3) to ones being built (state 2).
        let first = match target {
            2 => NIL,
            3 => self.town_gas(town, 3, 2),
            _ => self.town_gas(town, 3, 3),
        };
        let mut gas = first;
        for (state, n) in [(3, 1), (3, 2), (3, 3), (2, 1), (2, 2), (2, 3)] {
            if gas != NIL {
                break;
            }
            gas = self.town_gas(town, state, n);
        }
        self.ai_send_gather(d, u, gas, mineral);
    }

    /// 0x4326e0: state of gas building `g` for `player`: 0 none / dead, 1 a geyser,
    /// 2 the race's refinery, 3 one with a worker inside (+0xd0).
    pub(super) fn gas_state(&self, g: u32, player: u8) -> u8 {
        if g == NIL {
            return 0;
        }
        let gu = &self.units[g as usize];
        if gu.is_dead() {
            return 0;
        }
        if gu.unit_type == 0xbc {
            return 1;
        }
        if gu.unit_type != self.refinery_type(player) {
            return 0;
        }
        2 + (gu.u16_at(0xd0) != 0) as u8
    }

    /// 0x432b90: the `n`-th of the town's gas slots in `state` (NIL if fewer).
    fn town_gas(&self, town: u32, state: u8, n: u32) -> u32 {
        let tw = &self.ai.towns.items[town as usize];
        let mut count = 0;
        for g in tw.gas_buildings {
            if self.gas_state(g, tw.player) == state {
                count += 1;
                if count == n {
                    return g;
                }
            }
        }
        NIL
    }

    /// 0x433b90: send AI worker `u` to `res` (else `fallback`) unless it carries
    /// something: carried minerals go back first (0x5a), gas too (0x54).
    fn ai_send_gather(&mut self, d: &Data, u: u32, res: u32, fallback: u32) {
        let rt = self.units[u as usize].resource_type;
        if rt & 2 != 0 {
            self.units[u as usize].order_flags |= 1;
            if self.units[u as usize].order != 0 {
                self.drop_queue_tail_for(d, u, 0x5a);
                self.order_queue_append(d, u, 0x5a, 0xe4, (0, 0), NIL);
            }
            self.prepare_next_order(d, u);
            return;
        }
        if rt & 1 != 0 {
            self.order_now(d, u, 0x54);
            return;
        }
        let ok = |st: &State, r: u32| r != NIL && st.units[r as usize].unit_type != 0xbc && st.units[r as usize].status & 1 != 0;
        let r = if ok(self, res) { res } else { fallback };
        if ok(self, r) {
            self.order_with_target(d, u, 0x4f, r);
            self.units[r as usize].set_u8_at(0xd9, 1);
        }
    }

    /// Drop interruptible orders (and `order`) from the end of `u`'s queue.
    fn drop_queue_tail_for(&mut self, d: &Data, u: u32, order: u8) {
        loop {
            let t = self.units[u as usize].order_queue_tail;
            if t == NIL {
                break;
            }
            let o = self.orders[t as usize].order;
            if d.dat.orders.interruptible[o as usize] == 0 && o != order {
                break;
            }
            self.order_remove(d, u, t);
        }
    }

    /// 0x433c70: a terran AI worker repairs the town's most damaged building (or finishes
    /// one under construction); the town's chosen SCV (+0x28) also repairs units near
    /// it that ask for it. True if it was sent.
    fn ai_repair(&mut self, d: &Data, u: u32, town: u32) -> bool {
        let mut best = NIL;
        let mut most = 0u32;
        let mut b = self.ai.towns.items[town as usize].buildings;
        while b != NIL {
            let bu = self.ai.buildings.items[b as usize].parent;
            if self.units[bu as usize].connected_unit == NIL && self.ai_building_safe(bu) {
                let un = &self.units[bu as usize];
                if un.status & 1 == 0 {
                    best = bu;
                    break;
                }
                let hp = (un.hp + 0xff) >> 8;
                let mut max = d.dat.units.hit_points[un.unit_type as usize] as i32 >> 8;
                if max == 0 && hp == 0 {
                    max = 1;
                }
                let missing = (max - hp) as u32;
                if most < missing {
                    most = missing;
                    best = bu;
                }
            }
            b = self.ai.buildings.items[b as usize].link.next;
        }
        if best == NIL {
            let tw = &mut self.ai.towns.items[town as usize];
            if tw.building_scv == NIL {
                tw.building_scv = u;
            }
            if tw.building_scv != u {
                return false;
            }
            best = self.ai_repair_target_near(d, u);
            if best == NIL {
                return false;
            }
        }
        self.units[u as usize].order_flags |= 1;
        let pos = self.unit_sprite_pos(best);
        if self.units[u as usize].order != 0 {
            self.drop_queue_tail_for(d, u, 0x22);
            self.order_queue_append(d, u, 0x22, 0xe4, pos, best);
        }
        self.prepare_next_order(d, u);
        self.units[best as usize].connected_unit = u;
        true
    }

    /// 0x438910: building `b` stands in an AI region that is not under attack (state 3,
    /// targets, flag 0x20).
    fn ai_building_safe(&self, b: u32) -> bool {
        let un = &self.units[b as usize];
        if un.is_dead() {
            return false;
        }
        let (x, y) = self.unit_sprite_pos(b);
        let id = super::path::region_at(&self.sai, x as i32, y as i32);
        let r = &self.ai.regions[un.player as usize][id as usize];
        r.state != 3 && r.air_target == NIL && r.ground_target == NIL && r.flags & 0x20 == 0
    }

    /// 0x443160 with 0x4424c0: nearest own non-worker, non-building unit within 0x800
    /// whose order targets SCV `u`, in its region group, that `u` can repair.
    fn ai_repair_target_near(&mut self, d: &Data, u: u32) -> u32 {
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x800), y.wrapping_sub(0x800), x.wrapping_add(0x800), y.wrapping_add(0x800)];
        self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
            let cu = &st.units[c as usize];
            cu.player == st.units[u as usize].player
                && d.dat.units.flags[cu.unit_type as usize] & 9 == 0
                && cu.order_target_unit == u
                && st.same_group(c, st.unit_sprite_pos(u))
                && st.order_target_valid(d, u, 0x22, c).is_ok()
        })
    }

    /// 0x432b60: `b` can't make `id` now, or its player is at the build limit for it.
    fn ai_cannot_make(&self, d: &Data, b: u32, id: u16) -> bool {
        let p = self.units[b as usize].player;
        self.can_make_unit(d, b, id, p) != Ok(1) || self.ai_at_build_limit(p, id)
    }

    /// 0x462880: `player` has a guard post for `id` without a unit, in a safe region.
    fn ai_empty_guard_safe(&self, player: u8, id: u16) -> bool {
        let mut g = self.ai.guard_lists[player as usize];
        while g != NIL {
            let gd = &self.ai.guards.items[g as usize];
            if gd.parent == NIL && gd.unit_id == id {
                let (x, y) = gd.other_home;
                let r = super::path::region_at(&self.sai, x as i32, y as i32);
                let ar = &self.ai.regions[player as usize][r as usize];
                if ar.state != 3 && ar.air_target == NIL && ar.ground_target == NIL && ar.flags & 0x20 == 0 {
                    return true;
                }
            }
            g = gd.link.next;
        }
        false
    }

    /// 0x462760: empty guard posts of type `id` (siege mode counts as tank mode) that
    /// were updated get their update time reset to 1.
    fn ai_touch_empty_guards(&mut self, player: u8, id: u16) {
        let mut g = self.ai.guard_lists[player as usize];
        while g != NIL {
            let gd = &mut self.ai.guards.items[g as usize];
            if gd.parent == NIL && gd.previous_update != 0 {
                let t = if gd.unit_id == 0x1e { 5 } else { gd.unit_id };
                if t == id {
                    gd.previous_update = 1;
                }
            }
            g = gd.link.next;
        }
    }

    /// 0x433890: what idle production building `b` should train for `player` (the
    /// script's pick +0x220, else with 600 / 300 banked and no flag 4 a fixed choice per
    /// building); requests it at `priority`. True if a request is now on top.
    pub(super) fn ai_pick_training(&mut self, d: &Data, player: u8, b: u32, priority: u8) -> bool {
        let un = &self.units[b as usize];
        if un.status & 1 == 0 || un.order == 0x6a || un.order == 0x96 || un.secondary_order != 0x17 {
            return false;
        }
        let p = player as usize;
        let mut pick: i32 = -1;
        let script = self.ai.players[p].train_unit_id;
        if script != 0 {
            pick = script as i32 - 1;
            self.ai.players[p].train_unit_id = 0;
        }
        if pick == -1 {
            if self.players.minerals[p] < 600 || self.players.gas[p] < 300 || self.ai.players[p].flags & 4 != 0 {
                return false;
            }
        } else if self.ai_empty_guard_safe(player, pick as u16) {
            self.ai_touch_empty_guards(player, pick as u16);
            pick = -1;
        }
        let force = pick != -1;
        let id = if pick != -1 {
            if pick == 0xe4 {
                return false;
            }
            pick as u16
        } else {
            // First choice if allowed, else the second (if that one is allowed).
            let (a, b2, a_by_limit) = match self.units[b as usize].unit_type {
                0x23 => (0x26, 0x2b, false),
                0x6f => (0, 1, true),
                0x71 => (3, 5, false),
                0x72 => (8, 0xc, true),
                0x9b => (0x53, 0x53, false),
                0xa0 => (0x42, 0x41, false),
                0xa7 => (0x46, 0x48, true),
                _ => return false,
            };
            let first_ok = if a_by_limit { !self.ai_at_build_limit(player, a) } else { !self.ai_cannot_make(d, b, a) };
            if first_ok {
                a
            } else {
                let second_blocked = if self.units[b as usize].unit_type == 0xa0 { self.ai_at_build_limit(player, b2) } else { self.ai_cannot_make(d, b, b2) };
                if second_blocked {
                    return false;
                }
                b2
            }
        };
        let (x, y) = self.unit_sprite_pos(b);
        let region = super::path::region_at(&self.sai, x as i32, y as i32);
        let val = super::ai::ai_region_ref(player, region);
        self.ai_request_unit(d, player, id, priority, val, force, true);
        self.ai_top_request_type(player) != 0
    }

    /// 0x432480: a unit of `maker`'s race (by StarEdit group) can make `id`; else
    /// error 0x19.
    fn ai_same_race(&self, d: &Data, maker: u32, id: u16) -> bool {
        let g = &d.dat.units.staredit_group_flags;
        let (want, have) = (g[id as usize], g[self.units[maker as usize].unit_type as usize]);
        let ok = if want & 2 != 0 {
            have & 2 != 0
        } else if want & 1 != 0 {
            have & 1 != 0
        } else {
            have & 4 != 0
        };
        if !ok {
            self.req_error.set(0x19);
        }
        ok
    }

    /// Error code of a requirement check (0x66ff60).
    fn req_code(&self, r: Result<i32, u8>) -> u8 {
        match r {
            Err(c) => c,
            Ok(_) => self.req_error.get(),
        }
    }

    /// 0x4361a0: a building AI's round: the top spending request decides (train, worker,
    /// upgrade, tech, addon, ...); with nothing to do, an idle production building may
    /// pick a unit to train itself (0x433890).
    pub(super) fn building_ai(&mut self, d: &Data, u: u32) -> bool {
        match self.units[u as usize].unit_type {
            0x2a => todo_port!("0x434df0", "AI overlord"),
            0x6b => todo_port!("0x4a13c0", "AI comsat"),
            0x6c => todo_port!("0x434d50", "AI nuke silo"),
            _ => {}
        }
        let ai = self.units[u as usize].ai;
        if ai == NIL || ai_kind(ai) != AI_BUILDING {
            return true;
        }
        let b = ai_index(ai);
        let player = self.units[u as usize].player;
        let p = player as usize;
        let mut pick = false;
        if self.players.minerals[p] > 0x5dc && self.players.gas[p] > 0x5dc && !self.busy_building(u) && self.ai_pick_training(d, player, u, 0xa0) {
            pick = true;
        }
        let pa = &self.ai.players[p];
        let ty = if pa.request_count != 0 { pa.requests[0].ty } else { 0 };
        match ty {
            0 => {
                if !self.ai_pick_training(d, player, u, 0x32) {
                    return true;
                }
                self.ai_train(d, player, u, true)
            }
            1 | 2 => self.ai_train(d, player, u, pick),
            3 => {
                todo_port!("0x434ff0", "AI building places a building (zerg)");
                true
            }
            4 => {
                self.ai_train_worker(d, player, u, b);
                true
            }
            5 => {
                self.ai_start_upgrade(d, player, u);
                true
            }
            6 => {
                self.ai_start_research(d, player, u);
                true
            }
            7 => {
                todo_port!("0x434e70", "AI addon");
                true
            }
            8 => {
                todo_port!("0x435130", "AI request type 8");
                true
            }
            _ => true,
        }
    }

    /// 0x435f10: train the top request's unit in `u` (morphs pick their own units);
    /// requests that can never be met are dropped and, unless `picked`, the building
    /// tries again (0x4361a0).
    fn ai_train(&mut self, d: &Data, player: u8, u: u32, picked: bool) -> bool {
        let p = player as usize;
        let id = self.ai.players[p].requests[0].id;
        let maker = match id {
            0x2c | 0x3e | 0x44 | 0x3f | 0x67 => {
                todo_port!("0x435f10", "AI morph requests (0x4375f0, 0x437410, 0x437320, 0x437570)");
                return false;
            }
            _ => u,
        };
        let mut ok = self.ai_same_race(d, maker, id);
        let mut code = self.req_error.get();
        if ok {
            let r = self.can_make_unit(d, maker, id, player);
            ok = r == Ok(1);
            code = self.req_code(r);
        }
        if ok {
            if !self.ai_can_afford(d, player, id) {
                if self.ai.players[p].flags & 0x20 != 0 {
                    return false;
                }
                return self.ai_drop_request_retry(d, player, u, picked);
            }
            if self.ai_at_build_limit(player, id) {
                self.ai_request_done(d, player, false);
                self.ai_pop_request(player);
                return false;
            }
            // 0x4672f0
            let val = self.ai.players[p].requests[0].val;
            let ty = self.ai_top_request_type(player);
            let slot = self.free_build_slot(maker);
            if slot == 5 || !self.queue_build(d, maker, id) {
                return false;
            }
            let bai = ai_index(self.units[maker as usize].ai) as usize;
            self.ai.buildings.items[bai].train_queue_types[slot] = ty;
            self.ai.buildings.items[bai].train_queue_values[slot] = val;
            self.ai_request_done(d, player, true);
            self.ai_pop_request(player);
            if matches!(self.units[maker as usize].unit_type, 0x23 | 0x2b | 0x26) {
                self.order_now(d, maker, 0x2a);
            } else {
                self.set_secondary(maker, 0x26);
            }
            return true;
        }
        if matches!(code, 2 | 0x17 | 0x15 | 8 | 7 | 0xd) {
            return self.ai_drop_request_retry(d, player, u, picked);
        }
        false
    }

    /// 0x436173 in 0x435f10: drop the top request; unless `picked`, run the building's
    /// round again.
    fn ai_drop_request_retry(&mut self, d: &Data, player: u8, u: u32, picked: bool) -> bool {
        self.ai_request_done(d, player, false);
        self.ai_pop_request(player);
        if !picked {
            self.building_ai(d, u);
        }
        false
    }

    /// 0x434720: a town hall (or larva) of the request's town trains the requested worker.
    fn ai_train_worker(&mut self, d: &Data, player: u8, u: u32, b: u32) {
        let p = player as usize;
        if self.ai.players[p].requests[0].val != self.ai.buildings.items[b as usize].town {
            return;
        }
        let id = self.ai.players[p].requests[0].id;
        if !self.ai_same_race(d, u, id) || self.can_make_unit(d, u, id, player) != Ok(1) || !self.ai_can_afford(d, player, id) {
            return;
        }
        if self.ai_at_build_limit(player, id) {
            self.ai_request_done(d, player, false);
            self.ai_pop_request(player);
            return;
        }
        if !self.queue_build(d, u, id) {
            return;
        }
        if self.units[u as usize].unit_type == 0x23 {
            self.order_now(d, u, 0x2a);
        } else {
            self.set_secondary(u, 0x26);
        }
        self.ai_request_done(d, player, true);
        self.ai_pop_request(player);
    }

    /// 0x434670: start the requested upgrade (0x448750 checks the AI's budget).
    fn ai_start_upgrade(&mut self, d: &Data, player: u8, u: u32) {
        let p = player as usize;
        let id = self.ai.players[p].requests[0].id;
        let r = self.can_upgrade(d, u, id, player);
        if r == Ok(1) {
            let (m, g, _) = self.upgrade_cost(d, player, id as u8);
            let (m, g) = (m as u16 as u32, g as u16 as u32);
            // 0x42d190 / 0x42d0c0 without messages.
            if self.players.minerals[p] < m as i32 || self.players.gas[p] < g as i32 {
                return;
            }
            let pa = &self.ai.players[p];
            if m <= pa.minerals_available && g <= pa.gas_available && self.start_upgrade(d, u, id as u8) {
                self.order_now(d, u, 0x4c);
                self.ai_request_done(d, player, true);
                self.ai_pop_request(player);
            }
        } else if matches!(self.req_code(r), 2 | 0x17 | 0x15 | 8 | 7 | 0xd) {
            self.ai_request_done(d, player, false);
            self.ai_pop_request(player);
            self.building_ai(d, u);
        }
    }

    /// 0x4345c0: start the requested research (0x4486c0 checks the AI's budget).
    fn ai_start_research(&mut self, d: &Data, player: u8, u: u32) {
        let p = player as usize;
        let id = self.ai.players[p].requests[0].id;
        let r = self.can_research(d, u, id, player);
        if r == Ok(1) {
            let (m, g) = (d.dat.techdata.mineral_cost[id as usize] as u32, d.dat.techdata.gas_cost[id as usize] as u32);
            let pa = &self.ai.players[p];
            let ok = m as i32 <= self.players.minerals[p] && g as i32 <= self.players.gas[p] && m <= pa.minerals_available && g <= pa.gas_available;
            if ok && self.start_research(d, u, id as u8) {
                self.order_now(d, u, 0x4b);
                self.ai_request_done(d, player, true);
                self.ai_pop_request(player);
            }
        } else if matches!(self.req_code(r), 2 | 0x17 | 0x15 | 8 | 7 | 0xd) {
            self.ai_request_done(d, player, false);
            self.ai_pop_request(player);
            self.building_ai(d, u);
        }
    }

    /// 0x4348c0: the town's first spending request is a building (type 3) for this
    /// town (or a 0x6d request) that this worker should place. True if it was sent.
    fn ai_worker_build(&mut self, d: &Data, town: u32, u: u32) -> bool {
        let player = self.units[u as usize].player;
        let p = player as usize;
        let pa = &self.ai.players[p];
        if pa.request_count == 0 || pa.requests[0].ty != 3 {
            return false;
        }
        let w = ai_index(self.units[u as usize].ai) as usize;
        if (pa.requests[0].val != town && pa.requests[0].id != 0x6d) || self.ai.workers.items[w].reassign_count >= 3 {
            return false;
        }
        let id = pa.requests[0].id;
        if !self.ai_same_race(d, u, id) || self.can_make_unit(d, u, id, player) != Ok(1) || !self.ai_can_afford(d, player, id) {
            return false;
        }
        let tp = self.ai.towns.items[town as usize].player;
        let spot = if id == self.refinery_type(tp) {
            let g = self.town_gas(town, 1, 1);
            if g == NIL {
                return false;
            }
            self.unit_sprite_pos(g)
        } else {
            let mut pos = self.ai.towns.items[town as usize].position;
            let radius = match id {
                0x7d => {
                    todo_port!("0x4348c0", "AI bunker placement (0x4376f0, 0x444eb0, 0x4390a0)");
                    return false;
                }
                0x7c => {
                    todo_port!("0x4348c0", "AI turret placement (0x437670)");
                    return false;
                }
                0x83 if self.ai.towns.items[town as usize].main_building != NIL => {
                    todo_port!("0x444630", "AI expansion hatchery spot");
                    return false;
                }
                0x8f | 0xa2 => {
                    todo_port!("0x444eb0", "AI defense building region bookkeeping");
                    0x40
                }
                _ => 0x40,
            };
            if self.ai.players[p].build_cooldown != 0 {
                return self.ai_drop_build_request(d, u);
            }
            match self.ai_find_build_spot(d, u, id, pos, radius) {
                Some(s) => pos = s,
                None => {
                    self.ai.players[p].build_cooldown = 1;
                    return self.ai_drop_build_request(d, u);
                }
            }
            pos
        };
        self.ai_request_done(d, tp, true);
        self.ai_pop_request(tp);
        let order = match self.units[u as usize].unit_type {
            7 => 0x1e,
            0x29 => 0x19,
            0x40 => 0x1f,
            _ => return false,
        };
        self.order_at(d, u, order, spot);
        self.clear_build_queue(d, u);
        self.queue_build(d, u, id);
        true
    }

    /// 0x433640: drop the top request and let `u` run a building AI round (0x4361a0).
    fn ai_drop_build_request(&mut self, d: &Data, u: u32) -> bool {
        let player = self.units[u as usize].player;
        self.ai_request_done(d, player, false);
        self.ai_pop_request(player);
        self.building_ai(d, u);
        false
    }
}
