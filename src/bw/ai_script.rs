//! AI script interpreter (0x45b850): aiscript.bin / bwscript.bin threads.
//!
//! An opcode that is not ported yet stops its thread where it is (`todo_port!`), so the
//! script never runs past code it can't execute.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

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
                    todo_port!("0x434060", "start_areatown");
                    None
                };
                self.ai.scripts.items[s as usize].town = town.unwrap_or(NIL);
                if town.is_none() {
                    self.end_script(s);
                    return Step::End;
                }
                self.ai.players[p].flags |= 2;
                self.ai.players[p].difficulty = self.ai.unk_692628;
                // 0x45b810: every region of a computer player gets a step (0x43fc60).
                if self.player_types[p] == 1 {
                    todo_port!("0x43fc60", "AI region step");
                }
            }
            // wait_build (finished) / wait_buildstart (also being made)
            0x09 | 0x0a => {
                let mut n = self.script_byte(d, s);
                let t = self.script_u16(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                if t == self.worker_type(p as u8) && self.ai.towns.items[town as usize].resource_area != 0 {
                    todo_port!("0x444430", "workers a resource area holds");
                    let _ = &mut n;
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
            0x00..=0x70 => {
                todo_port!("0x45b850", "AI script opcode");
                return self.script_retry(s, start);
            }
            // Anything else ends the thread.
            _ => {
                self.end_script(s);
                return Step::End;
            }
        }
        Step::Next
    }
}
