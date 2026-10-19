//! AI script interpreter (0x45b850): aiscript.bin / bwscript.bin threads.
//!
use super::list::NIL;
use super::path::{approx_distance, region_at};
use super::{Data, State};

/// What an opcode does to the thread after it ran.
enum Step {
    /// Go on with the next opcode.
    Next,
    /// Stop for this frame (the wait is set).
    Yield,
    /// The thread ended (it went back to the pool).
    End,
}

impl State {
    /// 0x45b030: the next byte of the thread's script.
    fn script_byte(&mut self, d: &Data, s: u32) -> u8 {
        let sc = &mut self.ai.scripts.items[s as usize];
        let bin = if sc.flags & 1 != 0 { d.bwscript } else { d.aiscript };
        let v = bin.get(sc.pos as usize).copied().unwrap_or(0);
        sc.pos = sc.pos.wrapping_add(1);
        v
    }

    /// 0x45b010
    fn script_u16(&mut self, d: &Data, s: u32) -> u16 {
        let lo = self.script_byte(d, s) as u16;
        let hi = self.script_byte(d, s) as u16;
        lo | hi << 8
    }

    /// 0x45aff0
    fn script_u32(&mut self, d: &Data, s: u32) -> u32 {
        let lo = self.script_u16(d, s) as u32;
        let hi = self.script_u16(d, s) as u32;
        lo | hi << 16
    }

    /// 0x403380: the thread goes back to the pool.
    fn end_script(&mut self, s: u32) {
        let mut head = self.ai.active_scripts;
        self.ai.scripts.release(&mut head, s);
        self.ai.active_scripts = head;
    }

    /// 0x45b850: run thread `s` until it waits or ends.
    pub(super) fn run_script(&mut self, d: &Data, s: u32) {
        loop {
            let start = self.ai.scripts.items[s as usize].pos;
            let op = self.script_byte(d, s);
            if std::env::var("SCBW_AIOPS").is_ok() {
                eprintln!("f{} script {s} p{} pos {start:#x} op {op:#x}", self.frame, self.ai.scripts.items[s as usize].player);
            }
            match self.script_op(d, s, op, start) {
                Step::Next => {}
                Step::Yield | Step::End => return,
            }
        }
    }

    /// Retry the opcode at `start` in 30 frames.
    fn script_retry(&mut self, s: u32, start: u32) -> Step {
        let sc = &mut self.ai.scripts.items[s as usize];
        sc.pos = start;
        sc.wait = 0x1e;
        Step::Yield
    }

    fn script_op(&mut self, d: &Data, s: u32, op: u8, start: u32) -> Step {
        let p = self.ai.scripts.items[s as usize].player as usize;
        let jump = |st: &mut State, to: u16| st.ai.scripts.items[s as usize].pos = to as u32;
        match op {
            // goto
            0x00 => {
                let to = self.script_u16(d, s);
                jump(self, to);
            }
            // notowns_jump: jump when the player has no unit of the type.
            0x01 => {
                let t = self.script_u16(d, s);
                let to = self.script_u16(d, s);
                if self.players.unit_counts.get(t as usize).map_or(0, |c| c[p & 0xff]) == 0 {
                    jump(self, to);
                }
            }
            // wait
            0x02 => {
                let w = self.script_u16(d, s);
                self.ai.scripts.items[s as usize].wait = w as u32;
                return Step::Yield;
            }
            // start_town / start_areatown
            0x03 | 0x04 => {
                let flags = self.ai.scripts.items[s as usize].flags;
                let town = if op == 3 {
                    let (cx, cy) = self.ai.scripts.items[s as usize].center;
                    self.start_town(d, p as u8, cx as i16, cy as i16, (flags >> 3) as u8)
                } else {
                    let a = self.ai.scripts.items[s as usize].area;
                    self.start_areatown(d, p as u8, a, (flags >> 3) as u8)
                };
                self.ai.scripts.items[s as usize].town = town.unwrap_or(NIL);
                if town.is_none() {
                    self.end_script(s);
                    return Step::End;
                }
                self.ai.players[p].flags |= 2;
                self.ai.players[p].difficulty = self.ai.unk_692628;
                self.ai_step_all_regions(d, p);
            }
            // wait_build (finished) / wait_buildstart (also being made)
            0x09 | 0x0a => {
                let mut n = self.script_byte(d, s);
                let t = self.script_u16(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                if t == self.worker_type(p as u8) && self.ai.towns.items[town as usize].resource_area != 0 {
                    let w = self.area_workers(self.ai.towns.items[town as usize].resource_area as u32, p as u8);
                    if w < n as u32 {
                        n = w as u8;
                    }
                }
                if self.town_unit_count(d, town, t, op == 9) < n as u32 {
                    let sc = &mut self.ai.scripts.items[s as usize];
                    sc.pos = sc.pos.wrapping_sub(4);
                    sc.wait = 0x1e;
                    return Step::Yield;
                }
            }
            // build / upgrade / tech / guard-style build (0x45, priority 0x50, flag 1)
            0x06 => {
                let n = self.script_byte(d, s);
                let t = self.script_u16(d, s);
                let pr = self.script_byte(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                self.town_build(town, n, t, pr, 0);
            }
            0x45 => {
                let n = self.script_byte(d, s);
                let t = self.script_u16(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                self.town_build(town, n, t, 0x50, 1);
            }
            0x07 => {
                let lvl = self.script_byte(d, s);
                let id = self.script_u16(d, s);
                let pr = self.script_byte(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                self.town_request(town, 1, lvl, id, pr, 0);
            }
            0x08 => {
                let id = self.script_u16(d, s);
                let pr = self.script_byte(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                self.town_request(town, 2, 1, id, pr, 0);
            }
            // jump while an enemy has a building the thread's centre can walk to
            0x3c => {
                let to = self.script_u16(d, s);
                let (cx, cy) = self.ai.scripts.items[s as usize].center;
                if self.enemy_building_reachable(p as u8, (cx as i16, cy as i16)) {
                    jump(self, to);
                }
            }
            // wait until the AI flag 1 is set (start_campaign / town started).
            0x0f => {
                if self.ai.players[p].flags & 1 == 0 {
                    return self.script_retry(s, start);
                }
            }
            // defaultmin
            0x22 => {
                let v = self.script_byte(d, s);
                self.ai.players[p].default_min_strength_for_regions = v;
            }
            // transports_off
            0x23 => self.ai.players[p].flags |= 4,
            // stop
            0x24 => {
                self.end_script(s);
                return Step::End;
            }
            // debug: the message (UI), then jump.
            0x27 => {
                let to = self.script_u16(d, s);
                jump(self, to);
            }
            // illegal: an error message (UI).
            0x28 => {}
            // farms_notiming / farms_timing
            0x2b => self.ai.players[p].flags |= 8,
            0x2c => self.ai.players[p].flags &= !8,
            // nuke_rate
            0x2d => {
                let v = self.script_byte(d, s);
                self.ai.players[p].nuke_rate_minutes = v;
            }
            // max_force
            0x2e => {
                let v = self.script_u16(d, s);
                self.ai.players[p].max_region_strength_need = v;
            }
            // random_jump: when a random byte is at most the chance.
            0x30 => {
                let chance = self.script_byte(d, s);
                let to = self.script_u16(d, s);
                if (self.rng.rand(0x34) & 0xff) <= chance as u32 {
                    jump(self, to);
                }
            }
            // time_jump: after `minutes` game minutes.
            0x31 => {
                let m = self.script_byte(d, s) as u32;
                let to = self.script_u16(d, s);
                if m * 0x3c <= self.trig.elapsed {
                    jump(self, to);
                }
            }
            // farms_on / farms_off (flag 0x10)
            0x32 => self.ai.players[p].flags |= 0x10,
            0x33 => self.ai.players[p].flags &= !0x10,
            // transports_on
            0x36 => self.ai.players[p].flags &= !4,
            // stop_town / town flag 0x20
            0x38 => self.ai.players[p].flags |= 0x20,
            // call / return (one return address, 0x6d63e0, for all threads)
            0x40 => {
                let to = self.script_u16(d, s);
                self.ai.script_return = self.ai.scripts.items[s as usize].pos;
                jump(self, to);
            }
            0x41 => {
                self.ai.scripts.items[s as usize].pos = self.ai.script_return;
                self.ai.script_return = 0;
            }
            // panic
            0x44 => {
                let v = self.script_u16(d, s);
                self.ai.players[p].panic_script_pos = v;
            }
            // wait until flag 0x400 is set
            0x47 => {
                if self.ai.players[p].flags & 0x400 == 0 {
                    return self.script_retry(s, start);
                }
            }
            // multirun: a new thread of the same player and file shares the town.
            0x48 => {
                let pos = self.script_u16(d, s) as u32;
                let bw = self.ai.scripts.items[s as usize].flags & 1 != 0;
                if let Some(n) = self.start_ai_script(p as u32, pos, bw, None) {
                    let town = self.ai.scripts.items[s as usize].town;
                    self.ai.scripts.items[n as usize].town = town;
                }
            }
            // a u16 nobody reads
            0x4a => {
                self.script_u16(d, s);
            }
            // define_max (build limit)
            0x4b => {
                let v = self.script_byte(d, s);
                let t = self.script_u16(d, s) as usize;
                if let Some(l) = self.ai.players[p].build_limits.get_mut(t) {
                    *l = v;
                }
            }
            // flag 0x800
            0x4d => self.ai.players[p].flags |= 0x800,
            // wait until `n` completed units of the type (tanks / goliaths count both modes)
            0x4e => {
                let n = self.script_byte(d, s) as u32;
                let t = self.script_u16(d, s) as usize;
                let pp = p & 0xff;
                let cc = |tt: usize| self.players.completed_counts.get(tt).map_or(0, |c| c[pp]) as u32;
                let short = if t == 0x1e { cc(t).wrapping_add(cc(5)) < n } else if t == 5 { cc(t).wrapping_add(cc(0x1e)) < n } else { cc(t) < n };
                if short {
                    let sc = &mut self.ai.scripts.items[s as usize];
                    sc.pos = sc.pos.wrapping_sub(4);
                    sc.wait = 0x1e;
                    return Step::Yield;
                }
            }
            // unk_attack byte
            0x4f => {
                let v = self.script_byte(d, s);
                self.ai.players[p].unk_attack = v;
            }
            // resources_jump: when the player has the minerals and gas.
            0x54 => {
                let m = self.script_u16(d, s) as i32;
                let g = self.script_u16(d, s) as i32;
                let to = self.script_u16(d, s);
                if m <= self.players.minerals[p] && g <= self.players.gas[p] {
                    jump(self, to);
                }
            }
            // give_vision / take_vision
            0x57 => {
                let q = self.script_byte(d, s) as usize;
                if let Some(v) = self.player_vision.get_mut(q) {
                    *v |= 1 << (p & 0x1f);
                }
            }
            0x58 => {
                let q = self.script_byte(d, s) as usize;
                if let Some(v) = self.player_vision.get_mut(q) {
                    *v &= !(1 << (p & 0x1f));
                }
            }
            // nop
            0x5a => {}
            // enemyresources_jump: the first unit owned by an enemy decides.
            0x5e => {
                let m = self.script_u16(d, s) as i32;
                let g = self.script_u16(d, s) as i32;
                let to = self.script_u16(d, s);
                let mut u = self.visible_units.head;
                while u != NIL {
                    let un = &self.units[u as usize];
                    let owner = if un.player == 0xb { self.sprites[un.sprite as usize].owner } else { un.player };
                    if self.alliances[p][owner as usize] == 0 {
                        let o = un.player as usize;
                        if m <= self.players.minerals[o] && g <= self.players.gas[o] {
                            jump(self, to);
                        }
                        break;
                    }
                    u = un.link.next;
                }
            }
            // difficulty jump: 0 = jump when difficulty < v, else when > v
            0x5f => {
                let kind = self.script_byte(d, s);
                let v = self.script_byte(d, s);
                let to = self.script_u16(d, s);
                let diff = self.ai.players[p].difficulty;
                if (kind == 0 && diff < v) || (kind != 0 && v < diff) {
                    jump(self, to);
                }
            }
            // kill every thread flagged 2
            0x61 => {
                let mut t = self.ai.active_scripts;
                while t != NIL {
                    let sc = &mut self.ai.scripts.items[t as usize];
                    if sc.flags & 2 != 0 {
                        sc.flags |= 4;
                    }
                    t = sc.link.next;
                }
            }
            // flag this thread 2
            0x62 => self.ai.scripts.items[s as usize].flags |= 2,
            // last nuke time
            0x66 => self.ai.players[p].previous_nuke_timer = self.trig.elapsed.wrapping_add(1),
            // 0x51ca10 (the start seed copy; nothing in the game reads it back)
            0x69 => {
                self.script_u32(d, s);
            }
            // if_owned
            0x6a => {
                let t = self.script_u16(d, s);
                let to = self.script_u16(d, s);
                if self.players.unit_counts.get(t as usize).map_or(0, |c| c[p & 0xff]) != 0 {
                    jump(self, to);
                }
            }
            // expand: when the player has at most `n` towns, claim the best free area
            0x05 => {
                let n = self.script_byte(d, s);
                let v = self.script_u16(d, s);
                if self.ai_town_count(p as u8) <= n as u32 {
                    self.script_expand(d, s, v, n);
                }
            }
            // guard_resources-style: a guard post at every free area without one nearby
            0x3f => {
                let v = self.script_u16(d, s);
                self.script_guard_areas(p as u8, v);
            }
            // nuke_pos
            0x59 => {
                let g = self.script_find_caster(p as u8, 1, 0);
                if g != NIL {
                    let silo = self.script_find_silo(g);
                    if silo != NIL {
                        let a = self.ai.scripts.items[s as usize].area;
                        let b = [a[0] as i16, a[1] as i16, a[2] as i16, a[3] as i16];
                        let t = self.script_unit_at_area(d, g, b);
                        if t != NIL {
                            self.order_on(d, g, 0x7e, t);
                        } else {
                            self.order_at(d, g, 0x7e, (((a[0] + a[2]) / 2) as i16, ((a[1] + a[3]) / 2) as i16));
                        }
                        let nuke = self.units[silo as usize].ptr_at(0xd0);
                        self.units[nuke as usize].connected_unit = g;
                    }
                }
            }
            // disruption_web (0x67) / recall (0x68) at the centre of the area
            0x67 | 0x68 => {
                let (ty, tech, order) = if op == 0x67 { (0x3c, 0x19, 0xb5) } else { (0x47, 0x15, 0x89) };
                let e = d.dat.techdata.energy_cost[tech] as u32;
                let c = self.script_find_caster(p as u8, ty, e);
                if c != NIL {
                    let a = self.ai.scripts.items[s as usize].area;
                    let pos = ((a[2] - a[0]) / 2 + a[0], (a[3] - a[1]) / 2 + a[1]);
                    self.order_at(d, c, order, (pos.0 as i16, pos.1 as i16));
                }
            }
            // claim area `a` (when free) and start the script at `v` there
            0x6f => {
                let a = self.script_byte(d, s);
                let v = self.script_u16(d, s);
                if self.area_claim(a as u32) {
                    self.script_start_area(d, s, a as u32, v);
                }
            }
            // jump when the player has exactly `n` towns
            0x70 => {
                let n = self.script_byte(d, s);
                let to = self.script_u16(d, s);
                if self.ai_town_count(p as u8) == n as u32 {
                    jump(self, to);
                }
            }
            // attack_clear (0x0b) / attack_clear plus a byte for 0x43e050 (0x1f): the attack
            // force is emptied and regions that were attacking go idle.
            0x0b | 0x1f => {
                let mode = if op == 0x1f { self.script_byte(d, s) } else { 0 };
                self.ai_attack_clear(d, p as u8);
                if op == 0x1f {
                    self.ai_attack_all_in(d, p as u8, mode);
                    self.ai.players[p].last_attack_second = self.trig.elapsed.wrapping_sub(0xaf);
                }
            }
            // attack_prepare (0x0d), quick_attack-style wait (0x0e: retried every 30 frames
            // until 0x43abb0 succeeds), attack_do (0x64, then the attack timer restarts).
            0x0d | 0x0e | 0x64 => {
                let c = self.ai.scripts.items[s as usize].center;
                let ok = self.ai_choose_attack_region(d, p as u8, c.0, c.1, true, false);
                if op == 0x0e && !ok {
                    let sc = &mut self.ai.scripts.items[s as usize];
                    sc.pos = sc.pos.wrapping_sub(1);
                    sc.wait = 0x1e;
                    return Step::Yield;
                }
                if op == 0x64 {
                    self.ai.players[p].last_attack_second = self.trig.elapsed.wrapping_sub(0xaf);
                }
            }
            // enemy_beatable-style jump (0x49: case byte, then the target; jumps when 0x43b0b0
            // finds none).
            0x49 => {
                let case = self.script_byte(d, s);
                let to = self.script_u16(d, s);
                let c = self.ai.scripts.items[s as usize].center;
                if !self.ai_enemy_beatable(d, p as u8, c.0, c.1, case as u32) {
                    jump(self, to);
                }
            }
            // 0x42: jumps unless 0x43b570 says the attack force needs more.
            0x42 => {
                let to = self.script_u16(d, s);
                let c = self.ai.scripts.items[s as usize].center;
                if !self.ai_attack_needs_more(d, p as u8, c.0, c.1) {
                    jump(self, to);
                }
            }
            // attack_add (0x0c) / attack_add_to (0x53) / attack_add_easy-style (0x60: difficulty 0 only)
            0x0c | 0x60 => {
                let n = self.script_byte(d, s);
                let id = self.script_u16(d, s);
                if op == 0x0c || self.ai.players[p].difficulty == 0 {
                    self.ai_attack_force_add(p as u32, n as u32, id as u32);
                }
            }
            0x53 => {
                let a = self.script_byte(d, s) as i32;
                let b = self.script_byte(d, s) as i32;
                let id = self.script_u16(d, s);
                let c = |t: usize| self.players.unit_counts.get(t).map_or(0, |c| c[p & 0xff]);
                let mut n = c(id as usize);
                if id == 0x1e {
                    n = n.wrapping_add(c(5));
                } else if id == 5 {
                    n = n.wrapping_add(c(0x1e));
                }
                let n = n.wrapping_sub(a).max(b);
                self.ai_attack_force_add(p as u32, n as u32, id as u32);
            }
            // 0x10: per-region flags
            0x10 => self.ai_region_flags(d, p as u8, false),
            // 0x11 / 0x34: mark regions that want a turret / bunker
            0x11 => self.ai_mark_turret_regions(d, p as u8),
            0x34 => self.ai_mark_bunker_regions(p as u8),
            // 0x12 / 0x35: wait until no region has the flag 2 / 0x10
            0x12 | 0x35 => {
                if !self.ai_regions_lack_flag(p, if op == 0x12 { 2 } else { 0x10 }) {
                    return self.script_retry(s, start);
                }
            }
            // defense_unit rows: add `n` copies of a unit type
            0x13..=0x1a => {
                let n = self.script_byte(d, s);
                let id = self.script_u16(d, s);
                self.ai_defense_add(p, (op - 0x13) as usize, n as u32, id as u32);
            }
            // clear two defense rows (k and k + 4)
            0x1b..=0x1e => {
                self.ai_clear_defense_rows(p, (op - 0x1b) as usize);
            }
            // 0x20 / 0x21: the owners of the units in the area become enemies (0) / allies of the
            // player (2)
            0x20 | 0x21 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_area_alliance(p, a, if op == 0x20 { 0 } else { 2 });
            }
            0x25 => self.ai_make_rescuable(d, p as u32),
            0x26 => {
                let (cx, cy) = self.ai.scripts.items[s as usize].center;
                self.ai_medics_to_region(d, p as u32, cx, cy);
            }
            0x29 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_units_enter_bunkers(d, p as u8, a);
            }
            0x2a => {
                let c = self.ai.scripts.items[s as usize].center;
                self.ai_region_to_defend(d, p as u8, c);
            }
            0x2f => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_regions_in_area_idle(d, p as u8, a);
            }
            // attack_add_repeat: 0x447bb0 with the divisor read from the script
            0x37 => {
                let v = self.script_u16(d, s);
                self.ai_attack_force_repeat(p as u8, v);
            }
            // jump by the race of the nearest enemy building (zerg / terran / protoss)
            0x39 => {
                let first = self.script_u16(d, s);
                let second = self.script_u16(d, s);
                let third = self.script_u16(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                match self.town_enemy_race(town) {
                    0 => jump(self, second),
                    1 => jump(self, first),
                    2 => jump(self, third),
                    _ => {}
                }
            }
            // 0x3b: the script's town takes `n` workers from the player's other towns (0x4355c0)
            0x3b => {
                let n = self.script_byte(d, s) as u32;
                let town = self.ai.scripts.items[s as usize].town;
                self.town_take_workers(d, town, n);
            }
            // jump when the regions around the town have less than `n` * 32 tiles
            0x3a => {
                let cap = (self.script_byte(d, s) as u32) << 5;
                let to = self.script_u16(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                if self.town_region_tiles(town, cap) < cap {
                    jump(self, to);
                }
            }
            // guard post of unit type `id` at the town's position / mineral / gas
            0x3d => {
                let id = self.script_u16(d, s);
                let sel = self.script_byte(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                self.script_guard_post(p as u8, town, sel, id);
            }
            // train_unit-style waits: 0x3e / 0x46 count the finished units, 0x4c all of them
            0x3e | 0x46 | 0x4c => {
                let n = self.script_byte(d, s);
                let id = self.script_u16(d, s);
                if self.ai_type_count(p as u8, id, op != 0x4c) < n as i32 {
                    self.ai.players[p].train_unit_id = id.wrapping_add(1);
                    if op != 0x46 {
                        let sc = &mut self.ai.scripts.items[s as usize];
                        sc.pos = sc.pos.wrapping_sub(4);
                        sc.wait = 0x1e;
                        return Step::Yield;
                    }
                }
            }
            // flag 0x80 by the byte (4 sets it)
            0x43 => {
                let v = self.script_byte(d, s);
                self.ai_set_flag80(p, v);
            }
            // the area is the player's general command area
            0x50 => self.ai_set_gencmd_area(p, self.ai.scripts.items[s as usize].area),
            // units in the area go to the general command area
            0x51 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_units_to_gencmd_area(d, p as u8, a);
            }
            // the minerals and gas of the player count 2000 more when below 500
            0x52 => {
                self.ai_boost_resources(p);
            }
            0x55 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_units_enter_transports(d, p as u8, a);
            }
            0x56 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_units_load_into_carriers(d, p as u8, a);
            }
            // order 0xbb for every unit of the player in the area
            0x65 => {
                let a = self.ai.scripts.items[s as usize].area;
                self.ai_area_units_order_bb(d, p as u8, a);
            }
            0x5b => self.ai_regions_enclosed(d, p as u8),
            0x5c => {
                let mut u = self.player_units[p];
                while u != NIL {
                    self.ai_region_add_building(d, p as u8, u);
                    u = self.units[u as usize].player_next;
                }
            }
            // jump when a visible unit of the type belongs to an enemy
            0x5d => {
                let t = self.script_u16(d, s);
                let to = self.script_u16(d, s);
                let mut u = self.visible_units.head;
                while u != NIL {
                    let un = &self.units[u as usize];
                    if un.unit_type == t && self.unit_owner_is_enemy(u, p as u8) {
                        jump(self, to);
                        break;
                    }
                    u = un.link.next;
                }
            }
            // wait while a region of the player is being fought for
            0x63 => {
                if self.ai_regions_in_battle(p as u8) {
                    return self.script_retry(s, start);
                }
            }
            0x6b => {
                let silo = self.script_find_empty_silo(p as u8);
                if silo != NIL {
                    self.silo_make_nuke(d, silo);
                }
            }
            // create a unit at (x, y); a cloaker cloaks at once when it may
            0x6c => {
                let ty = self.script_u16(d, s);
                let x = self.script_u16(d, s);
                let y = self.script_u16(d, s);
                if let Some(u) = self.create_unit_checked(d, ty, x as i32, y as i32, p as u8) {
                    let tech = cloak_tech(self.units[u as usize].unit_type);
                    if tech != 0x2c {
                        let pl = self.units[u as usize].player;
                        if self.can_use_tech(d, u, tech, pl) == Ok(1) {
                            self.burrowed_cloak(d, u);
                        }
                    }
                }
            }
            // nuke_pos at a given spot
            0x6d => {
                let x = self.script_u16(d, s);
                let y = self.script_u16(d, s);
                let g = self.script_find_caster(p as u8, 1, 0);
                if g != NIL {
                    let silo = self.script_find_silo(g);
                    if silo != NIL {
                        self.order_at(d, g, 0x7e, (x as i16, y as i16));
                        let nuke = self.units[silo as usize].ptr_at(0xd0);
                        self.units[nuke as usize].connected_unit = g;
                    }
                }
            }
            0x6e => self.ai_allied_regions_follow(d, p as u8),
            // Past the jump table's 0x71 entries (`ja` 0x45c9b3): the thread ends.
            _ => {
                self.end_script(s);
                return Step::End;
            }
        }
        Step::Next
    }
}

/// What 0x4453e0 collects per slot (0x695570.., 0x695594..): the best path distance
/// through safe regions and the best straight distance, with their areas.
struct ExpandScore {
    near: [u32; 9],
    near_area: [u32; 9],
    far: [u32; 9],
    far_area: [u32; 9],
}

/// 0x51b1e8 (file offset 0x11b1e8 of StarCraft.exe, 3 rows of 10 dwords; 9 ends a row):
/// the order the slots are tried in: the first row when the script allows one town, the
/// second when the gas is ahead, else the third. Read once; the identity order when the
/// file is missing or short.
fn expand_order() -> &'static [[usize; 10]; 3] {
    static T: std::sync::OnceLock<[[usize; 10]; 3]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let from_exe = || -> Option<[[usize; 10]; 3]> {
            let exe = std::fs::read(crate::install_dir().join("StarCraft.exe")).ok()?;
            let b = exe.get(0x11b1e8..0x11b1e8 + 120)?;
            let mut t = [[0; 10]; 3];
            for (i, c) in b.chunks_exact(4).enumerate() {
                t[i / 10][i % 10] = (u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize).min(9);
            }
            Some(t)
        };
        from_exe().unwrap_or([[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]; 3])
    })
}

impl State {
    /// 0x431f40: the towns of `player`.
    fn ai_town_count(&self, player: u8) -> u32 {
        let mut n = 0;
        let mut t = self.ai.town_lists[player as usize];
        while t != NIL {
            n += 1;
            t = self.ai.towns.items[t as usize].link.next;
        }
        n
    }

    /// 0x443a00: claim area `a` when no town or script has it (flag 2).
    fn area_claim(&mut self, a: u32) -> bool {
        match self.areas.get_mut(a as usize) {
            Some(ar) if ar.flags & 3 == 0 => {
                ar.flags |= 2;
                true
            }
            _ => false,
        }
    }

    /// 0x443590: step `*i` to the next area (after it) nobody has; false past the last.
    fn area_next_free(&self, i: &mut u32) -> bool {
        *i += 1;
        while *i <= self.area_count {
            if self.areas[*i as usize].flags & 3 == 0 {
                return true;
            }
            *i += 1;
        }
        false
    }

    /// 0x444430: how many workers area `a` keeps busy: its mining spots and 3 per geyser.
    fn area_workers(&self, a: u32, player: u8) -> u32 {
        if a == 0 || a >= 0xfa {
            return 0;
        }
        self.area_mining_spots(a, player) + self.areas[a as usize].geysers as u32 * 3
    }

    /// 0x443470: `id` is in the list of area candidates (0x69262c, count 0x69265c).
    #[allow(dead_code)]
    fn area_candidate_listed(&self, id: u16) -> bool {
        self.ai.area_candidates.contains(&id)
    }

    /// 0x462670: a guard post for `player` at (x, y) for unit type `id`.
    pub(super) fn guard_create(&mut self, player: u8, id: u16, x: i16, y: i16) {
        let mut head = self.ai.guard_lists[player as usize];
        let Some(g) = self.ai.guards.alloc(&mut head) else { return };
        self.ai.guard_lists[player as usize] = head;
        let gd = &mut self.ai.guards.items[g as usize];
        gd.unit_id = id;
        gd.ai_type = 1;
        gd.parent = NIL;
        gd.home = (x, y);
        gd.other_home = (x, y);
        gd.times_died = 0;
    }

    /// 0x45b260: a guard post for unit type `id` at every free area that has none of the
    /// player's within 0x120.
    fn script_guard_areas(&mut self, player: u8, id: u16) {
        let mut i = 0;
        while self.area_next_free(&mut i) {
            let (x, y) = self.areas[i as usize].pos;
            let mut g = self.ai.guard_lists[player as usize];
            let mut near = false;
            while g != NIL {
                let gd = &self.ai.guards.items[g as usize];
                if approx_distance(gd.home.0 as i32, x as i32, gd.home.1 as i32, y as i32) <= 0x120 {
                    near = true;
                    break;
                }
                g = gd.link.next;
            }
            if !near {
                self.guard_create(player, id, x, y);
            }
        }
    }

    /// 0x45b210: the player's first unit of type `ty` with at least `energy` whole points
    /// that is idle for the AI (orders 0x9c, 0xa0 and 2).
    fn script_find_caster(&self, player: u8, ty: u16, energy: u32) -> u32 {
        let mut u = self.player_units[player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.unit_type == ty && energy <= (un.energy >> 8) as u32 && matches!(un.order, 0x9c | 0xa0 | 2) {
                return u;
            }
            u = un.player_next;
        }
        NIL
    }

    /// 0x463360 (with the flag set): the owner's silo whose nuke is ready and not yet on
    /// its way for another ghost than `ghost`.
    fn script_find_silo(&self, ghost: u32) -> u32 {
        let mut u = self.player_units[self.units[ghost as usize].player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.unit_type == 0x6c && un.u32_at(0xd4) != 0 {
                let nuke = un.ptr_at(0xd0);
                if nuke == NIL {
                    return u;
                }
                let c = self.units[nuke as usize].connected_unit;
                if c == NIL || c == ghost || self.units[c as usize].unit_type == 0x6c {
                    return u;
                }
            }
            u = un.player_next;
        }
        NIL
    }

    /// 0x440b20: adds what `c` is worth (hit points, or 800 for a working detector, a
    /// cloaked-unit spotter or a bunker-like type) when it is an enemy of `r`'s player.
    fn nuke_strength_add(&self, d: &Data, c: u32, r: u32, sum: &mut u32) {
        let cu = &self.units[c as usize];
        if cu.status & 0x400_0000 != 0 {
            return;
        }
        let owner = if cu.player == 0xb { self.sprites[cu.sprite as usize].owner } else { cu.player };
        if self.alliances[self.units[r as usize].player as usize][owner as usize] != 0 {
            return;
        }
        let f = d.dat.units.flags[cu.unit_type as usize];
        if f & 8 != 0 || cu.status & 0x2_0000 == 0 {
            *sum = sum.wrapping_add(self.life_now(d, c) as u32);
        }
        // 0x403430: a working detector.
        let det = f & 0x8000 != 0
            && cu.status & 1 != 0
            && cu.status & 0x400 == 0
            && cu.lockdown_timer == 0
            && cu.stasis_timer == 0
            && cu.maelstrom_timer == 0
            && cu.is_blind == 0;
        if f & 1 != 0 && (det || cu.unit_type == 0x92 || cu.unit_type == 0x67) {
            *sum = 800;
        }
    }

    /// 0x442260: the nearest unit to `g` that 0x440ff0 accepts (inside `b`, an enemy with
    /// 800 strength around it), else the last enemy found inside `b`.
    fn script_unit_at_area(&mut self, d: &Data, g: u32, b: [i16; 4]) -> u32 {
        let mut last = NIL;
        let whole = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
        let r = self.nearest_unit_to(g, whole, &mut |st: &mut State, c: u32| {
            // 0x440ff0
            let (x, y) = st.unit_sprite_pos(c);
            if x < b[0] || x > b[2] || y < b[1] || y > b[3] {
                return false;
            }
            let cu = &st.units[c as usize];
            let owner = if cu.player == 0xb { st.sprites[cu.sprite as usize].owner } else { cu.player };
            if st.alliances[st.units[g as usize].player as usize][owner as usize] != 0 {
                return false;
            }
            last = c;
            let around = [x.wrapping_sub(0xc0), y.wrapping_sub(0xc0), x.wrapping_add(0xc0), y.wrapping_add(0xc0)];
            let mut search = std::mem::replace(&mut st.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(st, around);
            let found = search.results(start);
            search.pop();
            st.search = search;
            let mut sum = 0u32;
            for f in found {
                st.nuke_strength_add(d, f, g, &mut sum);
            }
            sum >= 800
        });
        if r == NIL { last } else { r }
    }

    /// 0x444fd0: score area `cand` against `base` for slot `slot`: by path distance when
    /// `player`'s units get there safely, else (only with `far_ok`) by straight distance.
    fn area_score(&mut self, d: &Data, sc: &mut ExpandScore, base: u32, player: u8, cand: u32, far_ok: bool, slot: usize) {
        let bp = self.areas[base as usize].pos;
        let cp = self.areas[cand as usize].pos;
        let cr = region_at(&self.sai, cp.0 as i32, cp.1 as i32);
        let br = region_at(&self.sai, bp.0 as i32, bp.1 as i32);
        if self.ai_region_reachable(d, player, br, cr) {
            if self.ai_path_safe(br, cr, player) || !far_ok {
                let t = self.ai_path_tiles((bp.0 as i32, bp.1 as i32), (cp.0 as i32, cp.1 as i32));
                if t <= sc.near[slot] {
                    sc.near[slot] = t;
                    sc.near_area[slot] = cand;
                }
                return;
            }
        } else if !far_ok {
            return;
        }
        let t = approx_distance(bp.0 as i32, cp.0 as i32, bp.1 as i32, cp.1 as i32);
        if t <= sc.far[slot] {
            sc.far[slot] = t;
            sc.far_area[slot] = cand;
        }
    }

    /// 0x4453e0: the best free area for `player`'s next town, measured from `base` (its
    /// town's area); it is claimed (flag 2). 0 when none.
    pub(super) fn best_expansion_area(&mut self, d: &Data, player: u8, base: u32, n: u8) -> u32 {
        const INF: u32 = 99999999;
        let mut sc = ExpandScore { near: [INF; 9], near_area: [0; 9], far: [INF; 9], far_area: [0; 9] };
        let p = player as usize;
        let cc = |t: usize| self.players.completed_counts[t][p];
        // a shuttle, a dropship, or an overlord with ventral sacs
        let far_ok = cc(0x45) != 0 || cc(0x0b) != 0 || (cc(0x2a) != 0 && self.players.upgrade_sc[p][0x18] != 0);
        for i in 1..=self.area_count {
            if self.areas[i as usize].flags & 3 != 0 {
                continue;
            }
            self.area_score(d, &mut sc, base, player, i, far_ok, 8);
            let a = self.areas[i as usize];
            let slot = if a.gas <= 1000 {
                if a.geysers == 0 {
                    if a.minerals > 2000 {
                        2
                    } else if a.minerals == 0 {
                        continue;
                    } else {
                        6
                    }
                } else if a.minerals > 2000 {
                    1
                } else if a.minerals == 0 {
                    7
                } else {
                    4
                }
            } else if a.minerals > 2000 {
                0
            } else if a.minerals == 0 {
                5
            } else {
                3
            };
            self.area_score(d, &mut sc, base, player, i, far_ok, slot);
        }
        let (m, g) = (self.players.minerals[p], self.players.gas[p]);
        let row = if n == 1 {
            0
        } else if m.abs_diff(g) < 0x1f5 {
            2
        } else {
            (m <= g) as usize + 1
        };
        let mut found = 0;
        for &k in expand_order()[row].iter().take_while(|&&k| k != 9) {
            if sc.near[k] != INF {
                found = sc.near_area[k];
                break;
            }
            if sc.far[k] != INF {
                found = sc.far_area[k];
                break;
            }
        }
        if found != 0 {
            self.areas[found as usize].flags |= 2;
        }
        found
    }

    /// 0x45b530: thread `s` (or, from 0x45b590, a stand-in) claims the best free area
    /// and starts the script at `v` there; only while the player is short of minerals or
    /// gas, or a campaign AI.
    pub(super) fn script_expand(&mut self, d: &Data, s: u32, v: u16, n: u8) {
        let (p, town) = {
            let sc = &self.ai.scripts.items[s as usize];
            (sc.player as u8, sc.town)
        };
        self.expand_from(d, p, town, self.ai.scripts.items[s as usize].flags & 1 != 0, v, n);
    }

    /// 0x45b530 with its inputs: the player, the town the thread has and the file.
    pub(super) fn expand_from(&mut self, d: &Data, player: u8, town: u32, bw: bool, v: u16, n: u8) {
        let pi = player as usize;
        let poor = self.players.minerals[pi] < 5000 || self.players.gas[pi] < 3000 || self.ai.players[pi].flags & 0x20 != 0;
        if !poor || town == NIL {
            return;
        }
        let base = self.ai.towns.items[town as usize].resource_area as u32;
        if base == 0 {
            return;
        }
        let a = self.best_expansion_area(d, player, base, n);
        if a != 0 {
            self.start_area_script(d, player, bw, a, v);
        }
    }

    /// 0x45b440 for thread `s`.
    fn script_start_area(&mut self, d: &Data, s: u32, area: u32, v: u16) {
        let sc = &self.ai.scripts.items[s as usize];
        let (p, bw) = (sc.player as u8, sc.flags & 1 != 0);
        self.start_area_script(d, p, bw, area, v);
    }

    /// 0x45b440: start the script at `v` over a box around `area`'s spot (the thread's
    /// flags carry the area), mark the region there as taken (state 4, flag 1) and let
    /// 0x43a8f0 mark the regions nearby.
    fn start_area_script(&mut self, d: &Data, player: u8, bw: bool, area: u32, v: u16) {
        let (x, y) = self.areas[area as usize].pos;
        let (xi, yi) = (x as i32, y as i32);
        let Some(n) = self.start_ai_script(player as u32, v as u32, bw, Some([xi - 1, yi - 1, xi + 1, yi + 1])) else { return };
        let sc = &mut self.ai.scripts.items[n as usize];
        sc.flags = (sc.flags & 7) | area << 3;
        let p = player as usize;
        let r = region_at(&self.sai, xi, yi);
        if self.ai.regions[p].get(r as usize).is_some_and(|g| g.state == 0) {
            self.set_ai_region_state(d, player, r, 4);
            let campaign = self.ai.players[p].flags & 0x20 != 0;
            let reg = &mut self.ai.regions[p][r as usize];
            reg.flags |= 1;
            if campaign {
                reg.flags |= 0x40;
            }
        }
        if self.ai.players[p].flags & 0x20 == 0 {
            self.ai_claim_enemy_paths(d, player, (x, y));
        }
    }
}

/// 0x447230 / 0x4474f0 on a list: `count` copies of `id + 1` go behind the entries in use
/// (the first free slot, none when the first `cap` are in use).
fn fill_unit_list(list: &mut [u16], cap: usize, count: u32, id: u32) {
    let Some(mut i) = list[..cap].iter().position(|&e| e == 0) else { return };
    let mut n = count;
    while n != 0 {
        n -= 1;
        if i >= cap {
            return;
        }
        list[i] = id as u16 + 1;
        i += 1;
    }
}

impl State {
    /// 0x4384a0: a unit of `player` of type `ty` is in region `r`.
    fn scr_region_has_unit(&mut self, r: u16, ty: u16, player: u8) -> bool {
        self.units_in_region(r).into_iter().any(|u| {
            let un = &self.units[u as usize];
            un.unit_type == ty && un.player == player
        })
    }

    /// 0x439500: every region of `p` in state 8 goes idle (0x4390a0, state 0).
    fn ai_regions_state8_idle(&mut self, d: &Data, p: u8) {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        for r in 1..n {
            if self.ai.regions[p as usize][r].state == 8 {
                self.set_ai_region_state(d, p, r as u16, 0);
            }
        }
    }

    /// The part of opcodes 0x0b / 0x1f before the fork: the attack force and its region
    /// are cleared (0x690118 x 0x20 dwords, 0x690106, 0x690110), then 0x439500.
    pub(super) fn ai_attack_clear(&mut self, d: &Data, p: u8) {
        let ai = &mut self.ai.players[p as usize];
        ai.attack_force = [0; 0x40];
        ai.attack_grouping_region = 0;
        ai.last_attack_second = 0;
        self.ai_regions_state8_idle(d, p);
    }

    /// 0x447230: `count` copies of unit type `id` (+ 1) are appended to the attack force
    /// of `player` (at most 0x3f entries).
    fn ai_attack_force_add(&mut self, player: u32, count: u32, id: u32) {
        if player < 8 && count < 0x3f && id < 0xe4 {
            fill_unit_list(&mut self.ai.players[player as usize].attack_force, 0x3f, count, id);
        }
    }

    /// 0x4474f0: the same for defense row `row` (0x14 entries, at most 10 copies).
    fn ai_defense_add(&mut self, p: usize, row: usize, count: u32, id: u32) {
        if count < 0xb && id < 0xe4 {
            fill_unit_list(&mut self.ai.players[p].defense_units[row], 0x14, count, id);
        }
    }

    /// 0x439c00: for each region of `p` (those with at most 10 tiles when `thin`): clear
    /// flag 1 (and 0x40 for campaign AIs); the neighbors of regions in state 4 and up are
    /// then taken (state 4, flag 1, 0x40) unless they have a state already. Runs twice
    /// (all regions, then only the thin ones) when called with `thin` false.
    fn ai_region_flags(&mut self, d: &Data, p: u8, mut thin: bool) {
        let pi = p as usize;
        loop {
            let n = (self.sai.region_count as usize).min(self.ai.regions[pi].len());
            let mut mark = vec![false; self.sai.regions.len().max(n) + 1];
            let campaign = self.ai.players[pi].flags & 0x20 != 0;
            for r in 1..n {
                if thin && self.sai.regions[r].tile_count > 10 {
                    continue;
                }
                let reg = &mut self.ai.regions[pi][r];
                reg.flags &= 0xfe;
                if campaign {
                    reg.flags &= 0xbf;
                }
                if reg.state > 3 {
                    let id = reg.id as usize;
                    let sr = &self.sai.regions[id];
                    let cnt = (sr.neighbor_count as i32 - sr.zero_neighbors as i8 as i32).max(0);
                    for i in 0..cnt as usize {
                        let nb = self.sai.neighbor(id, i) as usize;
                        if let Some(m) = mark.get_mut(nb) {
                            *m = true;
                        }
                    }
                }
            }
            for r in 1..n {
                if mark[r] && self.ai.regions[pi][r].state == 0 {
                    self.set_ai_region_state(d, p, r as u16, 4);
                    let reg = &mut self.ai.regions[pi][r];
                    reg.flags |= 1;
                    if campaign {
                        reg.flags |= 0x40;
                    }
                }
            }
            if thin {
                return;
            }
            thin = true;
        }
    }

    /// 0x438870: up to 3 regions with flag 1 where `p` has a building or worker but no
    /// Bunker (0x7d) get flag 2.
    fn ai_mark_turret_regions(&mut self, d: &Data, p: u8) {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        let mut found = 0;
        for r in 1..n {
            if self.ai.regions[p as usize][r].flags & 1 != 0
                && self.player_has_base_unit_in(d, r as u16, p)
                && !self.scr_region_has_unit(r as u16, 0x7d, p)
            {
                self.ai.regions[p as usize][r].flags |= 2;
                found += 1;
                if found == 3 {
                    return;
                }
            }
        }
    }

    /// 0x4387e0: up to 6 regions with flag 1 and no Missile Turret (0x7c) of `p` get flag 0x10.
    fn ai_mark_bunker_regions(&mut self, p: u8) {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        let mut found = 0;
        for r in 1..n {
            if self.ai.regions[p as usize][r].flags & 1 != 0 && !self.scr_region_has_unit(r as u16, 0x7c, p) {
                self.ai.regions[p as usize][r].flags |= 0x10;
                found += 1;
                if found == 6 {
                    return;
                }
            }
        }
    }

    /// 0x436bd0 (mask 2) / 0x436b90 (mask 0x10): no region of `p` has a flag of `mask`
    /// (false with no regions at all: the loop's count is compared with 1).
    fn ai_regions_lack_flag(&self, p: usize, mask: u8) -> bool {
        let n = self.sai.region_count as usize;
        let mut i = 1;
        while i < n && self.ai.regions[p].get(i).map_or(0, |g| g.flags) & mask == 0 {
            i += 1;
        }
        i == n
    }

    /// 0x446d40: flag 0x80 of the player's AI set when `v` is 4, else cleared.
    fn ai_set_flag80(&mut self, p: usize, v: u8) {
        if v == 4 {
            self.ai.players[p].flags |= 0x80;
        } else {
            self.ai.players[p].flags &= !0x80;
        }
    }

    /// 0x446b10: the player's general command area.
    fn ai_set_gencmd_area(&mut self, p: usize, a: [i32; 4]) {
        self.ai.players[p].gencmd_area = a;
    }

    /// 0x446b40: minerals and gas below 500 count 2000 more.
    fn ai_boost_resources(&mut self, p: usize) {
        if self.players.minerals[p] < 500 {
            self.players.minerals[p] += 2000;
        }
        if self.players.gas[p] < 500 {
            self.players.gas[p] += 2000;
        }
    }

    /// 0x4474a0 / 0x447450 / 0x447400 / 0x4473b0 (`k` 0..3): clear defense rows k and k + 4.
    fn ai_clear_defense_rows(&mut self, p: usize, k: usize) {
        self.ai.players[p].defense_units[k] = [0; 0x14];
        self.ai.players[p].defense_units[k + 4] = [0; 0x14];
    }

    /// 0x43be50: each unit of player `p` in the area gets the interruptible orders dropped and
    /// order 0xbb (type 0xe4) queued.
    fn ai_area_units_order_bb(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            if self.units[u as usize].player == p {
                self.units[u as usize].order_flags |= 1;
                self.issue_order(d, u, 0xbb, true, (0, 0), NIL, 0xe4);
                self.prepare_next_order(d, u);
            }
        }
    }

    /// 0x45b810: if the player is a computer player, step every region (0x43fc60).
    fn ai_step_all_regions(&mut self, d: &Data, p: usize) {
        if self.player_types[p] == 1 {
            for r in 0..self.sai.region_count {
                self.ai_region_step(d, p as u8, r);
            }
        }
    }

    /// The units whose finder boxes overlap `b` (0x42ff80), as a list.
    fn scr_units_in_box(&mut self, b: [i16; 4]) -> Vec<u32> {
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
    }

    /// 0x430b00 with the filter 0x432420: the finder's units that belong to `player`.
    fn scr_player_units_in_box(&mut self, b: [i16; 4], player: u8) -> Vec<u32> {
        let mut list = self.scr_units_in_box(b);
        list.retain(|&u| self.units[u as usize].player == player);
        list
    }

    /// The box of a thread's area (0x4a0e20 / 0x43c260 copy the low words of the four dwords).
    fn scr_area_box(a: [i32; 4]) -> [i16; 4] {
        [a[0] as i16, a[1] as i16, a[2] as i16, a[3] as i16]
    }

    /// 0x4a0e20: the owners of the units in the area (other than `p`) get alliance `v`
    /// from `p` (0x58d634).
    fn ai_area_alliance(&mut self, p: usize, a: [i32; 4], v: u8) {
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            let o = self.units[u as usize].player as usize;
            if o != p {
                self.alliances[p][o] = v;
            }
        }
    }

    /// 0x4a2130: `p` becomes a rescuable player (type 3) allied with everyone; its units
    /// leave the AI, drop their interruptible orders and start over (order 0xa1).
    fn ai_make_rescuable(&mut self, d: &Data, p: u32) {
        let pi = p as usize;
        self.player_types[pi] = 3;
        for i in 0..12 {
            self.alliances[pi][i] = 1;
            self.alliances[i][pi] = 1;
        }
        let mut u = self.player_units[pi];
        while u != NIL {
            if self.units[u as usize].sprite != NIL && self.units[u as usize].order != 0 {
                // 0x439d60, 0x432760, then 0x434c90 / guard post / 0x4477c0.
                self.ai_forget_unit(d, u, false);
                self.ai_resource_forget(d, u);
                self.ai_release_unit_rest(d, u);
                self.units[u as usize].order_flags |= 1;
                if self.units[u as usize].order != 0 {
                    loop {
                        let h = self.units[u as usize].order_queue_head;
                        if h == NIL {
                            break;
                        }
                        let o = self.orders[h as usize].order;
                        if d.dat.orders.interruptible[o as usize] == 0 && o != 0xa1 {
                            break;
                        }
                        self.order_remove(d, u, h);
                    }
                    self.order_queue_append(d, u, 0xa1, 0xe4, (0, 0), NIL);
                }
                self.prepare_next_order(d, u);
                self.units[u as usize].ai = NIL;
            }
            u = self.units[u as usize].player_next;
        }
    }

    /// 0x4a2380: the units of type 0x4a / 0x3d of `p` leave the AI and join the military
    /// of the region at (x, y).
    fn ai_medics_to_region(&mut self, d: &Data, p: u32, x: i32, y: i32) {
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            let (alive, ai) = (un.sprite != NIL && un.order != 0 && matches!(un.unit_type, 0x4a | 0x3d), un.ai);
            if alive {
                if ai != NIL {
                    self.ai_release_unit(d, u, false);
                }
                let r = region_at(&self.sai, x, y);
                let up = self.units[u as usize].player;
                self.region_add_military(d, up, r, u, true);
            }
            u = self.units[u as usize].player_next;
        }
    }

    /// 0x43c260: units of `p` in the area go into a Bunker of theirs (order 0x5c) when `p`
    /// has a finished one with room (0x440220 finds the nearest, 0x4e6e00 checks room).
    fn ai_units_enter_bunkers(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            if self.units[u as usize].player != p || self.players.completed_counts[0x7d][p as usize] == 0 {
                continue;
            }
            let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
            let bunker = self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
                // 0x440220
                st.units[c as usize].unit_type == 0x7d && st.units[c as usize].player == st.units[u as usize].player
            });
            if bunker != NIL && self.can_load(d, bunker, u) {
                self.order_on(d, u, 0x5c, bunker);
            }
        }
    }

    /// 0x4396f0: the region at the thread's centre is made state 5 (when it is below) and
    /// needs at least 200 strength on the ground and in the air.
    fn ai_region_to_defend(&mut self, d: &Data, p: u8, c: (i32, i32)) {
        let r = region_at(&self.sai, c.0, c.1);
        if self.ai.regions[p as usize].get(r as usize).is_some_and(|g| g.state < 5) {
            self.set_ai_region_state(d, p, r, 5);
            let g = &mut self.ai.regions[p as usize][r as usize];
            g.needed_ground_strength = g.needed_ground_strength.max(200);
            g.needed_air_strength = g.needed_air_strength.max(200);
        }
    }

    /// 0x439660: the regions of `p` whose centre is inside the area go idle (0x4390a0, state 0).
    fn ai_regions_in_area_idle(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        for r in 1..n {
            let c = self.sai.regions[r].center;
            let (x, y) = (c.0 >> 8, c.1 >> 8);
            if a[0] <= x && x <= a[2] && a[1] <= y && y <= a[3] {
                self.set_ai_region_state(d, p, r as u16, 0);
            }
        }
    }
}

/// 0x491a50: the tech a unit type cloaks with: personnel cloaking (10) for ghosts and
/// their heroes, cloaking field (9) for wraiths, none (0x2c) otherwise.
pub(super) fn cloak_tech(unit_type: u16) -> u16 {
    match unit_type {
        1 | 0x10 | 100 | 99 | 0x68 | 0x33 => 10,
        8 | 0x15 => 9,
        _ => 0x2c,
    }
}

impl State {
    /// 0x4a0f90: the nearest (0x40c360, to the centre of the region at (x, y)) finished
    /// ground building with an order that is not an ally of `player`; 0 (NIL) when none.
    fn ai_nearest_enemy_building(&self, x: i32, y: i32, player: u8) -> u32 {
        let r = region_at(&self.sai, x, y);
        let c = self.sai.regions[r as usize].center;
        let (cx, cy) = (c.0 >> 8, c.1 >> 8);
        let mut best = NIL;
        let mut best_d = 99999999u32;
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && un.status & 2 != 0 && !self.unit_owner_allied(u, player) {
                let sp = &self.sprites[un.sprite as usize];
                let dist = approx_distance(cx, sp.x as i32, cy, sp.y as i32);
                if dist < best_d {
                    best = u;
                    best_d = dist;
                }
            }
            u = un.link.next;
        }
        best
    }

    /// The owner of `u` (the sprite's for a neutral unit) is allied to `player` (0x58d634).
    fn unit_owner_allied(&self, u: u32, player: u8) -> bool {
        let un = &self.units[u as usize];
        let owner = if un.player == 0xb { self.sprites[un.sprite as usize].owner } else { un.player };
        self.alliances[player as usize][owner as usize] != 0
    }

    /// 0x47b740: `u` (a unit seen by `player`) belongs to an enemy of `player`.
    fn unit_owner_is_enemy(&self, u: u32, player: u8) -> bool {
        !self.unit_owner_allied(u, player)
    }

    /// 0x447bb0: units on the attack force's side are repeated: the strength of the
    /// units of the nearest enemy building's owner, in steps of `div`, adds up to three
    /// more copies of the force (at most 0x3f entries).
    fn ai_attack_force_repeat(&mut self, p: u8, div: u16) {
        let pi = p as usize;
        let region = self.ai.players[pi].attack_grouping_region;
        if region == 0 {
            return;
        }
        let c = self.sai.regions[region as usize].center;
        let t = self.ai_nearest_enemy_building(c.0 >> 8, c.1 >> 8, p);
        if t == NIL {
            return;
        }
        let mut sum = 0u32;
        let mut u = self.player_units[self.units[t as usize].player as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 {
                sum += un.air_strength.max(un.ground_strength) as u32;
            }
            u = un.player_next;
        }
        // A zero divisor is a division fault in the game.
        if sum == 0 || div == 0 {
            return;
        }
        let mut n = ((sum + div as u32 - 1) / div as u32 - 1).min(3);
        let copy = self.ai.players[pi].attack_force;
        let mut len = copy.iter().position(|&e| e == 0).unwrap_or(0x40);
        while n > 1 {
            n -= 1;
            let mut j = 0;
            while j < 0x40 && copy[j] != 0 {
                if len >= 0x40 {
                    return;
                }
                self.ai.players[pi].attack_force[len] = copy[j];
                len += 1;
                if len == 0x3f {
                    return;
                }
                j += 1;
            }
        }
    }

    /// 0x432aa0: the race (0 zerg, 1 terran, 2 protoss) of the owner of the building
    /// nearest to the town's position that is an enemy of the town's player; terran
    /// when there is none.
    fn town_enemy_race(&self, town: u32) -> u8 {
        let tw = &self.ai.towns.items[town as usize];
        let t = self.ai_nearest_enemy_building(tw.position.0 as i32, tw.position.1 as i32, tw.player);
        if t == NIL {
            return 1;
        }
        self.player_races[self.units[t as usize].player as usize]
    }

    /// 0x432260: tiles of the regions around the town's position, a breadth-first walk
    /// over the walkable neighbors (a ring buffer of 0x271 entries, regions are not
    /// marked) that stops at `cap`.
    fn town_region_tiles(&self, town: u32, cap: u32) -> u32 {
        let (x, y) = self.ai.towns.items[town as usize].position;
        let mut q = [0u32; 0x271];
        q[0] = region_at(&self.sai, x as i32, y as i32) as u32;
        let (mut head, mut tail) = (0usize, 1usize);
        let mut sum = 0u32;
        loop {
            let r = q[head] as u16 as usize;
            head += 1;
            if head == 0x271 {
                head = 0;
            }
            sum += self.sai.regions[r].tile_count as u32;
            if cap <= sum {
                return cap;
            }
            for i in 0..self.sai.regions[r].walkable_neighbors as usize {
                q[tail] = self.sai.neighbor(r, i) as u32;
                tail += 1;
                if tail == 0x271 {
                    tail = 0;
                }
            }
            if head == tail {
                return sum;
            }
        }
    }

    /// 0x433190 + 0x462e70: a guard post of `id` at the town's position (0), mineral
    /// (1) or first finished (else first unfinished) gas building; none when the town
    /// has no such thing.
    fn script_guard_post(&mut self, p: u8, town: u32, sel: u8, id: u16) {
        let tw = &self.ai.towns.items[town as usize];
        let pos = match sel {
            0 => Some(tw.position),
            1 => (tw.mineral != NIL).then(|| self.unit_sprite_pos(tw.mineral)),
            _ => {
                let mut g = self.town_gas(town, 3, 1);
                if g == NIL {
                    g = self.town_gas(town, 2, 1);
                }
                (g != NIL).then(|| self.unit_sprite_pos(g))
            }
        };
        if let Some((x, y)) = pos {
            self.guard_create(p, id, x, y);
        }
    }

    /// 0x45b100: how many units of type `id` the player has (finished ones when
    /// `completed`, else all); tanks and goliaths count both modes.
    fn ai_type_count(&self, p: u8, id: u16, completed: bool) -> i32 {
        let c = if completed { &self.players.completed_counts } else { &self.players.unit_counts };
        let n = |t: usize| c[t][p as usize & 0xff];
        match id {
            0x1e => n(0x1e) + n(5),
            5 => n(5) + n(0x1e),
            _ => n(id as usize),
        }
    }

    /// Common tail of 0x43c330 / 0x43c050 / 0x43bf00: `u` is ordered (its interruptible
    /// orders and earlier copies of `order` leave the queue) and starts it.
    fn script_force_order(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), target: u32) {
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].order != 0 {
            self.drop_queue_tail_for(d, u, order);
            self.order_queue_append(d, u, order, 0xe4, pos, target);
        }
        self.prepare_next_order(d, u);
    }

    /// 0x43c330: the `p` units in the area (flag 0x20000 set) go to the centre of the
    /// player's general command area (order 0x98).
    fn ai_units_to_gencmd_area(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        let g = self.ai.players[p as usize].gencmd_area;
        let pos = (((g[2] - g[0]) / 2 + g[0]) as i16, ((g[3] - g[1]) / 2 + g[1]) as i16);
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            let un = &self.units[u as usize];
            if un.player == p && un.status & 0x2_0000 != 0 {
                self.script_force_order(d, u, 0x98, pos, NIL);
            }
        }
    }

    /// 0x4394a0 (first part, also 0x43bf00): `c` is a transport: not flag 0x40000000,
    /// an Overlord only with ventral sacs, and with room to give.
    fn script_carrier_ok(&self, d: &Data, c: u32) -> bool {
        let cu = &self.units[c as usize];
        cu.status & 0x4000_0000 == 0
            && (cu.unit_type != 0x2a || self.players.upgrade_sc[cu.player as usize][0x18] != 0)
            && d.dat.units.space_provided[cu.unit_type as usize] != 0
    }

    /// 0x43c050: the `p` units in the area walk to the nearest transport of theirs that
    /// can take them (order 0x5c).
    fn ai_units_enter_transports(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            if self.units[u as usize].player != p {
                continue;
            }
            let b = [0, 0, self.map_px.0 as i16, self.map_px.1 as i16];
            let t = self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
                // 0x4394a0
                st.script_carrier_ok(d, c) && st.units[c as usize].player == st.units[u as usize].player && st.can_load(d, c, u)
            });
            if t != NIL {
                let pos = self.unit_sprite_pos(t);
                self.script_force_order(d, u, 0x5c, pos, t);
            }
        }
    }

    /// 0x43bf00: the `p` transports in the area wait to load units (order 0x70).
    fn ai_units_load_into_carriers(&mut self, d: &Data, p: u8, a: [i32; 4]) {
        for u in self.scr_units_in_box(Self::scr_area_box(a)) {
            if self.units[u as usize].player == p && self.script_carrier_ok(d, u) {
                let pos = self.unit_sprite_pos(u);
                self.script_force_order(d, u, 0x70, pos, u);
            }
        }
    }

    /// 0x4393e0: an idle region (state below 4) of `p` whose neighbors are all below 4
    /// too, but at most one, becomes state 4.
    fn ai_regions_enclosed(&mut self, d: &Data, p: u8) {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        for r in 1..n {
            if self.ai.regions[p as usize][r].state >= 4 {
                continue;
            }
            let id = self.ai.regions[p as usize][r].id as usize;
            let sr = &self.sai.regions[id];
            let cnt = (sr.neighbor_count as u32).wrapping_sub(sr.zero_neighbors as i8 as u32);
            let (mut i, mut seen) = (0u32, false);
            if cnt != 0 {
                loop {
                    let nb = self.sai.neighbor(id, i as usize) as usize;
                    if self.ai.regions[p as usize][nb].state < 4 {
                        if seen {
                            break;
                        }
                        seen = true;
                    }
                    i += 1;
                    if i >= cnt {
                        break;
                    }
                }
            }
            if i == cnt {
                self.set_ai_region_state(d, p, r as u16, 4);
            }
        }
    }

    /// 0x439f50: for each player allied with `p` both ways, the regions `p` holds
    /// (state not 0) that the ally has idle become state 4 for the ally.
    fn ai_allied_regions_follow(&mut self, d: &Data, p: u8) {
        let pi = p as usize;
        for q in 0..8usize {
            if self.alliances[pi][q] == 0 || self.alliances[q][pi] == 0 {
                continue;
            }
            let n = (self.sai.region_count as usize).min(self.ai.regions[pi].len()).min(self.ai.regions[q].len());
            for r in 1..n {
                if self.ai.regions[pi][r].state != 0 && self.ai.regions[q][r].state == 0 {
                    self.set_ai_region_state(d, q as u8, r as u16, 4);
                }
            }
        }
    }

    /// 0x436c90: a region of `p` is in state 1, 2, 8 or 9.
    fn ai_regions_in_battle(&self, p: u8) -> bool {
        let n = (self.sai.region_count as usize).min(self.ai.regions[p as usize].len());
        (1..n).any(|r| matches!(self.ai.regions[p as usize][r].state, 1 | 2 | 8 | 9))
    }

    /// 0x45b0d0: the first silo (0x6c) of `p` without a nuke.
    fn script_find_empty_silo(&self, p: u8) -> u32 {
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.unit_type == 0x6c && un.u32_at(0xd4) == 0 {
                return u;
            }
            u = un.player_next;
        }
        NIL
    }

    /// 0x45b7a0: silo `silo` (empty) builds a nuke (type 0xe) at once and takes it in
    /// (hidden); the AI of the silo's player gets it.
    fn silo_make_nuke(&mut self, d: &Data, silo: u32) {
        let su = &self.units[silo as usize];
        if su.unit_type != 0x6c || su.u32_at(0xd4) != 0 {
            return;
        }
        let player = su.player;
        let (x, y) = self.unit_sprite_pos(silo);
        let Some(n) = self.create_unit(d, 0xe, x, y, player) else { return };
        self.finish_unit_pre(d, n);
        self.finish_unit_post(d, n);
        self.hide_unit(d, n);
        self.units[silo as usize].set_ptr_at(0xd0, n);
        self.units[silo as usize].set_u32_at(0xd4, 1);
        self.units[n as usize].connected_unit = silo;
        self.ai_unit_joins(d, silo, n);
    }

    /// 0x434060: a town of `player` over the area: the player's units in it (those
    /// without an AI) join it; its position is a worker or town hall found there
    /// (the town hall wins), else the middle of the area. `res` is the resource area.
    fn start_areatown(&mut self, d: &Data, player: u8, a: [i32; 4], res: u8) -> Option<u32> {
        let list = self.scr_player_units_in_box(Self::scr_area_box(a), player);
        if list.is_empty() {
            return None;
        }
        let town = self.create_town(player, 0, 0)?;
        let tw = &mut self.ai.towns.items[town as usize];
        tw.resource_area = res;
        tw.resource_units_not_set = (res != 0) as u8;
        let mut best = NIL;
        for u in list {
            if self.units[u as usize].ai != NIL {
                continue;
            }
            self.town_add_unit(d, u, town);
            match self.units[u as usize].unit_type {
                7 | 0x29 | 0x40 => {
                    if best == NIL {
                        best = u;
                    }
                }
                0x6a | 0x83 | 0x84 | 0x85 | 0x9a => best = u,
                _ => {}
            }
        }
        let pos = if best == NIL {
            (((a[2] - a[0]) / 2 + a[0]) as i16, ((a[3] - a[1]) / 2 + a[1]) as i16)
        } else {
            self.unit_sprite_pos(best)
        };
        self.ai.towns.items[town as usize].position = pos;
        Some(town)
    }
}
