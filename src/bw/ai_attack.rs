//! The AI's attack planning (tai_attack): which region the attack force gathers in
//! (0x43abb0 and its helpers), whether more units are needed (0x43b570), whether an
//! enemy player's army is beatable (0x43b0b0), and the all-in (0x43e050).
//!
//! The scratch table the helpers share is one byte per region (0x9c4): 1 enemy
//! units in it, 2 / 3 / 4 flood fronts and done, 5 start (own armed units), 6 free
//! neighbor reached from a start.

use super::ai_town::{ai_index, ai_kind, AI_GUARD, AI_MILITARY};
use super::list::NIL;
use super::path::{approx_distance, region_at};
use super::{Data, State};

const REGIONS: usize = 0x9c4;

impl State {
    /// 0x43cd40 (EAX = `u`): a flying / ground unit of an AI sends itself at the nearest
    /// enemy it can use: its order target if that is an enemy, else one random player's
    /// (of those with an enemy first unit) units. An armed unit attacks (and its turret
    /// too; a sieged tank first checks it can hit from where it is), an unarmed one
    /// moves there. Then the unit leaves the AI.
    pub(super) fn ai_unit_attack_nearest(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].status & 0x2_0010 == 0 {
            return;
        }
        let me = self.units[u as usize].player as usize;
        let mut lists: Vec<u32> = Vec::new();
        let t = self.units[u as usize].order_target_unit;
        if t != NIL && self.alliances[me][self.attack_owner(t) as usize] == 0 {
            lists.push(t);
        } else {
            for p in 0..8 {
                let h = self.player_units[p];
                if h != NIL && self.alliances[me][self.attack_owner(h) as usize] == 0 {
                    lists.push(h);
                }
            }
            if lists.is_empty() {
                return;
            }
        }
        let n = lists.len() as u32;
        let mut idx = self.rng.rand(0x36) % n;
        let armed = self.is_armed_pub(d, u);
        let utype = self.units[u as usize].unit_type;
        for _ in 0..n {
            if idx >= n {
                idx = 0;
            }
            let (mut best, mut best_d) = (NIL, u32::MAX);
            let mut c = lists[idx as usize];
            while c != NIL {
                if self.units[c as usize].sprite != NIL && self.units[c as usize].order != 0 && self.ai_nearest_candidate(d, u, c, armed, utype) {
                    let (cp, up) = (self.unit_sprite_pos(c), self.unit_sprite_pos(u));
                    let (dx, dy) = ((cp.0 as i32).wrapping_sub(up.0 as i32), (cp.1 as i32).wrapping_sub(up.1 as i32));
                    let dist = (dy.wrapping_mul(dy)).wrapping_add(dx.wrapping_mul(dx)) as u32;
                    if dist < best_d {
                        best = c;
                        best_d = dist;
                    }
                }
                c = self.units[c as usize].player_next;
            }
            if best != NIL {
                self.ai_unit_go_to(d, u, best, armed);
                break;
            }
            idx += 1;
        }
    }

    /// The test 0x43cd40 makes on every unit `c` of the list: an unarmed `u` takes any;
    /// an armed, enabled one takes visible, attackable (hidden / invincible units and
    /// cloaked ones it cannot see not), by type: carriers any, reavers a ground unit in
    /// its region group, 0x2d / 0x31 damaged 0x6a only (0x402210), the AI's 0x47 none,
    /// else one it has a weapon against (0x475ad0, the air weapon).
    fn ai_nearest_candidate(&self, d: &Data, u: u32, c: u32, armed: bool, utype: u16) -> bool {
        if !armed {
            return true;
        }
        let cu = &self.units[c as usize];
        if self.disabled(u)
            || cu.status & 0x400_0000 != 0
            || self.sprites[cu.sprite as usize].flags & super::sprite::sflag::HIDDEN != 0
            || self.cloaked_from(c, u)
        {
            return false;
        }
        match utype {
            0x48 | 0x52 => true,
            0x53 | 0x51 => cu.status & 4 == 0 && self.units_same_group(u, c),
            0x2d | 0x31 => self.damaged_6a(d, c),
            0x47 if self.units[u as usize].ai != NIL => false,
            _ => self.weapon_against(d, u, c) != super::attack::NO_WEAPON,
        }
    }

    /// 0x43cd40 after the choice (`best`): an armed `u` (and its turret) attacks it (an
    /// interceptor counts as its carrier); an AI sieged tank (0x1e) that cannot hit it
    /// from here fights on (0x477160) when an enemy is near (0x442460), else tank-modes
    /// (order 0x63) and attacks as tank type 5. An unarmed `u` moves to its position.
    /// At the end the unit leaves the AI (0x439d60, 0x432760, 0x434c90, guard post,
    /// free medic).
    fn ai_unit_go_to(&mut self, d: &Data, u: u32, best: u32, armed: bool) {
        if armed {
            let mut tgt = best;
            if self.units[best as usize].unit_type == 0x49 && self.units[best as usize].ptr_at(0xc0) != NIL {
                tgt = self.units[best as usize].ptr_at(0xc0);
            }
            let pos = if tgt != NIL { self.unit_sprite_pos(tgt) } else { (0, 0) };
            let sub = self.units[u as usize].subunit;
            let mut who = vec![u];
            if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
                who.push(sub);
            }
            for w in who {
                let order = d.dat.units.ai_attack_unit[self.units[w as usize].unit_type as usize];
                self.units[w as usize].order_flags |= 1;
                self.issue_order(d, w, order, true, pos, tgt, 0xe4);
                self.prepare_next_order(d, w);
            }
            let un = &self.units[u as usize];
            if un.ai != NIL && un.unit_type == 0x1e && un.order != 0x63 && !self.can_hit_now(d, u, tgt) {
                if self.sieged_tank_has_enemy(d, u) {
                    self.ai_choose_target_help(d, u, true, true, false);
                } else {
                    self.order_now(d, u, 0x63);
                    // 0x474d10 (EAX = the target, no clearing): attack as tank type 5.
                    let order = d.dat.units.ai_attack_unit[5];
                    self.issue_order(d, u, order, false, pos, tgt, 0xe4);
                }
            }
        } else {
            let pos = self.unit_sprite_pos(best);
            self.units[u as usize].order_flags |= 1;
            if self.units[u as usize].order != 0 {
                loop {
                    let tail = self.units[u as usize].order_queue_tail;
                    if tail == NIL {
                        break;
                    }
                    let o = self.orders[tail as usize].order;
                    if d.dat.orders.interruptible[o as usize] != 0 || o == 6 {
                        self.order_remove(d, u, tail);
                    } else {
                        break;
                    }
                }
                self.order_queue_append(d, u, 6, 0xe4, pos, NIL);
            }
            self.prepare_next_order(d, u);
        }
        self.ai_forget_unit(d, u, false);
        self.ai_resource_forget(d, u);
        self.ai_release_unit_rest(d, u);
    }

    /// The player a unit counts for in the alliance table (a neutral unit's sprite owner).
    fn attack_owner(&self, u: u32) -> u8 {
        let un = &self.units[u as usize];
        if un.player == 0xb {
            self.sprites[un.sprite as usize].owner
        } else {
            un.player
        }
    }

    fn attack_enemy_of(&self, p: u8, u: u32) -> bool {
        self.alliances[p as usize][self.attack_owner(u) as usize] == 0
    }

    fn attack_unit_region(&self, u: u32) -> usize {
        let (x, y) = self.unit_sprite_pos(u);
        region_at(&self.sai, x as i32, y as i32) as usize
    }

    /// Region `r`'s neighbors (not counting the ones with region id 0).
    fn attack_neighbors(&self, r: usize) -> Vec<u16> {
        let Some(reg) = self.sai.regions.get(r) else { return Vec::new() };
        let n = (reg.neighbor_count as i32 - reg.zero_neighbors as i8 as i32).max(0) as usize;
        (0..n).map(|i| self.sai.neighbor(r, i)).collect()
    }

    /// 0x436cf0 (EBX = table): every region of state 2 becomes 3 and its neighbors with
    /// no state become 4.
    fn attack_close_fronts(&self, table: &mut [u8]) {
        let snap = table.to_vec();
        for r in 0..REGIONS {
            if snap[r] == 2 {
                table[r] = 3;
                for n in self.attack_neighbors(r) {
                    if table[n as usize] == 0 {
                        table[n as usize] = 4;
                    }
                }
            }
        }
    }

    /// 0x436d80 (EBX = table): the neighbors of regions at 5 and up are taken (a 1 goes to
    /// 2, a 0 to 6). True when one was.
    fn attack_spread(&self, table: &mut [u8]) -> bool {
        let snap = table.to_vec();
        let mut any = false;
        for r in 0..REGIONS {
            if snap[r] > 4 {
                for n in self.attack_neighbors(r) {
                    let c = &mut table[n as usize];
                    if *c == 1 {
                        *c = 2;
                        any = true;
                    } else if *c == 0 {
                        *c = 6;
                        any = true;
                    }
                }
            }
        }
        any
    }

    /// 0x438b30 (EBX = table): the regions of `p`'s armed buildings-or-units start at 5
    /// (units that are not subunits and have flag 1; when there are none, every such unit).
    fn attack_seed(&self, d: &Data, p: u8, table: &mut [u8]) {
        let mut any = false;
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            let f = d.dat.units.flags[un.unit_type as usize];
            if un.sprite != NIL && un.order != 0 && f & 0x10 == 0 && f & 1 != 0 {
                table[self.attack_unit_region(u)] = 5;
                any = true;
            }
            u = self.units[u as usize].player_next;
        }
        if any {
            return;
        }
        let mut u = self.player_units[p as usize];
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && d.dat.units.flags[un.unit_type as usize] & 0x10 == 0 {
                table[self.attack_unit_region(u)] = 5;
            }
            u = self.units[u as usize].player_next;
        }
    }

    /// 0x439b50: the regions with an armed (or flag 1) enemy of `p` that `p` sees get 1.
    fn attack_mark_enemies(&self, d: &Data, p: u8, table: &mut [u8]) {
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL
                && un.order != 0
                && (self.is_armed_pub(d, u) || d.dat.units.flags[un.unit_type as usize] & 1 != 0)
                && (un.status & 0x300 == 0 || un.visibility & (1u32 << (p & 0x1f)) != 0)
                && self.attack_enemy_of(p, u)
            {
                table[self.attack_unit_region(u)] = 1;
            }
            u = self.units[u as usize].link.next;
        }
    }

    /// 0x437770 (EAX = table, EDX = `start`): for a start region (table value 3), flood
    /// five steps through the free neighbor regions (table 6; own state 0 / 4 / 5) and pick
    /// the region the attack force gathers in: the closest to (x, y) of the first
    /// reached ring class holding a region of 0x3c+ tiles, else the largest. `all_classes`
    /// (EBX of 0x4399d0) also takes the rings 3, 2 (the ones before 4 are otherwise
    /// skipped); `flyers` (ECX of 0x4399d0) counts all the neighbors, not only the walkable
    /// ones. Returns the region plus one, 0 for none.
    fn attack_gather_region(&self, p: u8, table: &[u8], start: usize, x: i32, y: i32, all_classes: bool, flyers: bool) -> u32 {
        if table[start] != 3 {
            return 0;
        }
        let n = self.sai.region_count as i16 as i32 as u32 as usize;
        let mut vis = [0u8; REGIONS];
        vis[start] = 1;
        for level in 1..=5u8 {
            for r in 1..n {
                if vis[r] != level {
                    continue;
                }
                let reg = &self.sai.regions[r];
                let count = if flyers {
                    (reg.neighbor_count as i32 - reg.zero_neighbors as i8 as i32).max(0) as usize
                } else {
                    reg.walkable_neighbors as usize
                };
                for i in 0..count {
                    let nb = self.sai.neighbor(r, i) as usize;
                    let st = self.ai.regions[p as usize][nb].state;
                    if matches!(st, 0 | 5 | 4) && vis[nb] == 0 && table[nb] == 6 {
                        vis[nb] = level + 1;
                    }
                }
            }
        }
        let (mut best_tiles, mut last) = (0u16, 0u32);
        let (mut best_dist, mut near) = (99999999u32, 0u32);
        for class in [4u8, 3, 5, 6, 2] {
            if all_classes || class >= 4 {
                for r in 1..n {
                    if self.ai.regions[p as usize][r].flags & 0x20 == 0 && vis[r] == class {
                        let reg = &self.sai.regions[r];
                        if reg.tile_count > best_tiles {
                            last = r as u32 + 1;
                            best_tiles = reg.tile_count;
                        }
                        if reg.tile_count >= 0x3c {
                            let dist = approx_distance(x, reg.center.0 >> 8, y, reg.center.1 >> 8);
                            if dist < best_dist {
                                near = r as u32 + 1;
                                best_dist = dist;
                            }
                        }
                    }
                }
                if near != 0 {
                    return near;
                }
            }
        }
        if self.ai.players[p as usize].flags & 0x20 != 0 {
            last
        } else {
            0
        }
    }

    /// 0x438980: of the candidate start regions (`starts`), the index of the one closest
    /// (within 0x280 pixels of the best) to `p`'s first town with the least enemy
    /// strength within 0x200 pixels of its center. (A single start returns itself.)
    fn attack_nearest_start(&mut self, p: u8, starts: &[u32]) -> u32 {
        let count = starts.len();
        if count == 0 {
            return 0;
        }
        let town = self.ai.town_lists[p as usize];
        if town == NIL {
            return 0;
        }
        let (tx, ty) = self.ai.towns.items[town as usize].position;
        if count == 1 {
            return starts[0];
        }
        let (mut best_dist, mut best_str, mut best) = (99999999u32, 99999999u32, 0u32);
        for (i, &s) in starts.iter().enumerate() {
            let c = self.sai.regions[s as u16 as usize].center;
            let (cx, cy) = (c.0 >> 8, c.1 >> 8);
            let dist = approx_distance(tx as i32, cx, ty as i32, cy);
            if dist > best_dist.wrapping_add(0x280) {
                continue;
            }
            let b = [(cx - 0x200) as i16, (cy - 0x200) as i16, (cx + 0x200) as i16, (cy + 0x200) as i16];
            let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
            let start = search.units_in_finder_box(self, b);
            let found = search.results(start);
            search.pop();
            self.search = search;
            let (mut air, mut ground) = (0u32, 0u32);
            for u in found {
                if self.attack_enemy_of(p, u) {
                    let un = &self.units[u as usize];
                    air = air.wrapping_add(un.air_strength as u32);
                    ground = ground.wrapping_add(un.ground_strength as u32);
                }
            }
            let strength = air.max(ground);
            if strength < best_str || dist.wrapping_add(0x280) < best_dist {
                best = i as u32;
                best_str = strength;
                best_dist = dist;
            }
        }
        best
    }

    /// 0x4399d0 (ECX = `flyers`, EDX = `all_classes`): choose a start region (table 3)
    /// and its gathering region toward (x, y); the gathering region goes to state 8 with
    /// the start as its target and becomes the player's attack region. True when one was
    /// chosen. `force` resets the attack timer even when it is set.
    fn attack_choose_region(&mut self, d: &Data, p: u8, table: &[u8], x: i32, y: i32, force: bool, flyers: bool, all_classes: bool) -> bool {
        let mut res = vec![0u32; REGIONS];
        for r in 1..REGIONS {
            res[r] = self.attack_gather_region(p, table, r, x, y, all_classes, flyers);
        }
        let (mut starts, mut targets) = (Vec::new(), Vec::new());
        for r in 1..REGIONS {
            if res[r] != 0 {
                starts.push(r as u32);
                targets.push(res[r] - 1);
            }
        }
        let count = starts.len();
        if count == 0 {
            return false;
        }
        let idx = if count == 1 {
            0
        } else if self.ai.players[p as usize].flags & 0x20 == 0 {
            self.attack_nearest_start(p, &starts) as usize
        } else {
            // inlined sync_rand (site 0x28)
            self.rng.rand(0x28) as usize % count
        };
        let region = targets[idx];
        let elapsed = self.trig.elapsed;
        let pa = &mut self.ai.players[p as usize];
        if force || pa.last_attack_second == 0 {
            pa.last_attack_second = elapsed;
        }
        pa.attack_grouping_region = region as u16 + 1;
        self.set_ai_region_state(d, p, region as u16, 8);
        self.ai.regions[p as usize][region as usize].target_region_id = starts[idx] as u16;
        true
    }

    /// 0x43abb0 (stack: player, x, y, `force`, `retry`): `p` has no attack region yet:
    /// choose one toward (x, y) (`attack_choose_region`, twice; with `retry` and a force
    /// that is not all flyers twice more with flyer reach, else attack_failed = 1).
    /// Returns true when no new region was chosen (already failed, a region in a state
    /// other than 8, nothing found); false right after choosing one.
    pub(super) fn ai_choose_attack_region(&mut self, d: &Data, p: u8, x: i32, y: i32, force: bool, retry: bool) -> bool {
        let pi = p as usize;
        if self.ai.players[pi].attack_failed != 0 {
            return true;
        }
        let g = self.ai.players[pi].attack_grouping_region;
        if g != 0 {
            return self.ai.regions[pi][g as usize - 1].state != 8;
        }
        let mut table = vec![0u8; REGIONS];
        self.attack_seed(d, p, &mut table);
        self.attack_mark_enemies(d, p, &mut table);
        while self.attack_spread(&mut table) {
            self.attack_close_fronts(&mut table);
        }
        // flyers: every unit type of the attack force flies (flag 4)
        let force_list = self.ai.players[pi].attack_force;
        let mut flyers = force_list[0] != 0;
        for &t in force_list.iter().take_while(|&&t| t != 0) {
            if d.dat.units.flags[t as usize - 1] & 4 == 0 {
                flyers = false;
                break;
            }
        }
        self.ai.players[pi].flags |= 0x100;
        if self.attack_choose_region(d, p, &table, x, y, force, flyers, false)
            || self.attack_choose_region(d, p, &table, x, y, force, flyers, true)
        {
            return false;
        }
        if retry && !flyers {
            if self.attack_choose_region(d, p, &table, x, y, force, true, false)
                || self.attack_choose_region(d, p, &table, x, y, force, true, true)
            {
                return false;
            }
            self.ai.players[pi].attack_failed = 1;
        }
        true
    }

    /// 0x436b10: completed[43] + completed[144] * 2 + completed[38] of `p`.
    fn attack_weight_c(&self, p: u8) -> u32 {
        let c = &self.players.completed_counts;
        (c[43][p as usize] as u32)
            .wrapping_add((c[144][p as usize] as u32).wrapping_mul(2))
            .wrapping_add(c[38][p as usize] as u32)
    }

    /// 0x436b40: completed[38] + completed[146] * 2 of `p`.
    fn attack_weight_b(&self, p: u8) -> u32 {
        let c = &self.players.completed_counts;
        (c[38][p as usize] as u32).wrapping_add((c[146][p as usize] as u32).wrapping_mul(2))
    }

    /// 0x436b60: completed[0] counted twice up to 4 per completed[125], once above.
    fn attack_weight_a(&self, p: u8) -> u32 {
        let c = &self.players.completed_counts;
        let m = c[0][p as usize] as u32;
        let b = (c[125][p as usize] as u32).wrapping_mul(4);
        if m < b {
            m * 2
        } else {
            b.wrapping_add(m)
        }
    }

    /// 0x43b0b0 (ECX = x, EAX = y; opcode 0x49, `case` is its byte): after 0x43abb0, the
    /// enemy player owning the flag-1 unit nearest the attack region's start (preferring
    /// one within 0x140 of the previous best's owner) is `e`; the attack force and region
    /// are cleared (0x439500) and `case` says whether `e`'s army is small enough (true).
    /// The script jumps when this is false.
    pub(super) fn ai_enemy_beatable(&mut self, d: &Data, p: u8, x: i32, y: i32, case: u32) -> bool {
        if self.ai_choose_attack_region(d, p, x, y, false, false) {
            return false;
        }
        let pi = p as usize;
        let g = self.ai.players[pi].attack_grouping_region;
        let r = self.ai.regions[pi][g as usize - 1].target_region_id as usize;
        let (cx, cy) = {
            let c = self.sai.regions[r].center;
            (c.0 >> 8, c.1 >> 8)
        };
        let mut best: i32 = -1;
        let mut best_dist = 99999999u32;
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL
                && un.order != 0
                && d.dat.units.flags[un.unit_type as usize] & 1 != 0
                && self.attack_enemy_of(p, u)
            {
                let (sx, sy) = self.unit_sprite_pos(u);
                let dist = approx_distance(sx as i32, cx, sy as i32, cy);
                if dist <= 0x140 && best == un.player as i32 {
                    break;
                }
                if dist <= best_dist {
                    best = un.player as i32;
                    best_dist = dist;
                }
            }
            u = self.units[u as usize].link.next;
        }
        if best == -1 {
            return false;
        }
        let e = best as u8;
        self.ai_attack_clear(d, p);
        let (cnt, cc) = (&self.players.unit_counts, &self.players.completed_counts);
        let c = |t: usize| cc[t][e as usize] as u32;
        let (a, b, cw) = (self.attack_weight_a(e), self.attack_weight_b(e), self.attack_weight_c(e));
        match case {
            0 => cnt[111][e as usize] == 0 && cnt[142][e as usize] == 0 && cnt[160][e as usize] == 0,
            1 => a < 0x11 && b < 0xb && c(65) < 7,
            2 => a < 0x19 && cw < 0xb,
            3 => a < 6 && b < 3 && c(135) == 0 && c(65) < 2,
            4 => a < 0x11 && b < 0xb && c(65) < 9,
            5 => a < 7 && b < 7 && c(65) < 4,
            6 => a < 0xd && c(146) < 2 && c(66) < 2,
            7 => c(5) == 0 && c(45) == 0 && c(65) < 7,
            8 => a < 6 && b < 3 && c(65) < 2,
            9 => a < 10 && b < 5 && c(65) < 6,
            10 => a < 5 && b < 5 && c(65) < 3,
            0xb => a < 0xb && b < 0xb && c(65) < 6,
            0xc => a < 0x11 && cw < 6 && c(66).wrapping_add(c(70)) < 3,
            0xd => a < 0x19 && cw < 0xb && c(66).wrapping_add(c(70)) < 8,
            _ => false,
        }
    }

    /// 0x43b570 (ECX = x, EAX = y; opcode 0x42): true when the attack force is weaker
    /// (air or ground) than the force wanted (the strengths of the attack list) against
    /// the enemy units within 0x400 of the attack region's start. The script jumps when
    /// this is false (also when 0x43abb0 reports nothing to attack).
    pub(super) fn ai_attack_needs_more(&mut self, d: &Data, p: u8, x: i32, y: i32) -> bool {
        if self.ai_choose_attack_region(d, p, x, y, false, false) {
            return false;
        }
        let pi = p as usize;
        let (mut need_air, mut need_ground) = (0u32, 0u32);
        for &t in self.ai.players[pi].attack_force.iter().take_while(|&&t| t != 0) {
            need_air = need_air.wrapping_add(d.strength[0][t as usize - 1]);
            need_ground = need_ground.wrapping_add(d.strength[1][t as usize - 1]);
        }
        let g = self.ai.players[pi].attack_grouping_region;
        let r = self.ai.regions[pi][g as usize - 1].target_region_id as usize;
        let (cx, cy) = {
            let c = self.sai.regions[r].center;
            (c.0 >> 8, c.1 >> 8)
        };
        let (mut air, mut ground) = (0u32, 0u32);
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 && self.attack_enemy_of(p, u) {
                let (sx, sy) = self.unit_sprite_pos(u);
                if (approx_distance(sx as i32, cx, sy as i32, cy) as i32) < 0x400 {
                    air = air.wrapping_add(un.air_strength as u32);
                    ground = ground.wrapping_add(un.ground_strength as u32);
                }
            }
            u = self.units[u as usize].link.next;
        }
        air < need_air || ground < need_ground
    }

    /// 0x43e050 (AL = `mode`, stack = player): with `mode` 0 every military or guard unit
    /// of `p` not carrying units that is not in an attacking region joins the attack: the
    /// first one fixes the attack region (cleared, then 0x43abb0 toward the unit, suicide
    /// timer 0x18). With `mode` nonzero each idle unit gets a target (`ai_unit_attack_nearest`).
    pub(super) fn ai_attack_all_in(&mut self, d: &Data, p: u8, mode: u8) {
        let pi = p as usize;
        if mode != 0 {
            let mut u = self.player_units[pi];
            while u != NIL {
                let un = &self.units[u as usize];
                if un.sprite != NIL
                    && un.order != 0
                    && d.dat.units.flags[un.unit_type as usize] & 0x10 == 0
                    && un.unit_type != 0xe
                    && un.unit_type != 0x23
                    && self.sprites[un.sprite as usize].flags & 0x20 == 0
                    && !self.threatened_by_target(d, u)
                {
                    self.ai_unit_attack_nearest(d, u);
                }
                u = self.units[u as usize].player_next;
            }
            return;
        }
        if self.ai.players[pi].strategic_suicide_mission_cooldown != 0 {
            return;
        }
        let mut chosen: Option<u16> = None;
        let mut u = self.player_units[pi];
        while u != NIL {
            let un = &self.units[u as usize];
            let node = un.ai;
            if un.sprite != NIL && un.order != 0 && node != NIL && !self.can_carry(d, u)
                && (un.status & 0x20 == 0 || self.ai.players[pi].flags & 0x20 != 0)
            {
                let r = self.attack_unit_region(u) as u16;
                let mut skip = false;
                if ai_kind(node) == AI_GUARD {
                    let gi = ai_index(node) as usize;
                    let parent = self.ai.guards.items[gi].parent;
                    self.units[parent as usize].ai = NIL;
                    self.region_add_military(d, p, r, parent, true);
                    self.ai.guards.items[gi].parent = NIL;
                } else if ai_kind(node) != AI_MILITARY {
                    skip = true;
                }
                if !skip {
                    let node = self.units[u as usize].ai;
                    if node != NIL && ai_kind(node) == AI_MILITARY {
                        let (rp, rr) = self.ai.military.items[ai_index(node) as usize].region;
                        if matches!(self.reg(rp, rr).state, 1 | 8 | 9 | 2) {
                            skip = true;
                        }
                    }
                }
                if !skip {
                    if chosen.is_none() {
                        self.ai_attack_clear(d, p);
                        let (x, y) = self.unit_sprite_pos(u);
                        if !self.ai_choose_attack_region(d, p, x as i32, y as i32, false, true) {
                            self.ai.players[pi].strategic_suicide_mission_cooldown = 0x18;
                            chosen = Some(self.ai.players[pi].attack_grouping_region - 1);
                        } else {
                            skip = true;
                        }
                    }
                    if !skip {
                        self.ai_release_unit(d, u, false);
                        self.region_add_military(d, p, chosen.unwrap(), u, true);
                    }
                }
            }
            u = self.units[u as usize].player_next;
        }
    }
}
