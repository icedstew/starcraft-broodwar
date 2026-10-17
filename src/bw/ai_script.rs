//! AI script interpreter (0x45b850): aiscript.bin / bwscript.bin threads.
//!
//! An opcode that is not ported yet stops its thread where it is (`todo_port!`), so the
//! script never runs past code it can't execute.

use super::list::NIL;
use super::path::{approx_distance, region_at};
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
                    for r in 0..self.sai.region_count {
                        self.ai_region_step(d, p as u8, r);
                    }
                }
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
            todo_port!("0x43a8f0", "AI regions near the enemy start");
        }
    }
}
