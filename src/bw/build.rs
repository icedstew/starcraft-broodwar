//! Terran construction: the Build command (0x48de70), building placement checks
//! (0x473fb0 and its helpers), PlaceBuilding (0x467fd0), ConstructingBuilding
//! (0x467a70), IncompleteBuilding (0x467760) and the building tile flags (0x469f60).

use super::list::NIL;
use super::train::NONE;
use super::{Data, State};
use crate::todo_port;

/// 0x6408f8: placement result grids, 8 rows of 6 tiles per slot.
// ponytail: the original slot count is unknown; the game uses slots 0 and 1.
pub const PLACEMENT_SLOTS: usize = 12;

/// Tile flag: occupied by a building (0x469f60).
const TILE_BUILDING: u32 = 0x800_0000;

/// units.dat placement box in pixels.
fn placement(d: &Data, t: u16) -> (i32, i32) {
    let v = d.dat.units.placement_box[t as usize];
    (v as i16 as i32, (v >> 16) as i16 as i32)
}

fn dims(d: &Data, t: u16) -> [i16; 4] {
    let v = d.dat.units.dimensions[t as usize];
    [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16]
}

fn is_gas_building(t: u16) -> bool {
    matches!(t, 0x6e | 0x9d | 0x95)
}

impl State {
    fn tile_flag(&self, x: i32, y: i32) -> u32 {
        self.terrain.tile_flags[(y * self.terrain.width as i32 + x) as usize]
    }

    /// Fog (`explored` false) or unexplored mask of a human player; computers see all.
    fn human_tile_mask(&self, p: u8, unexplored: bool) -> u32 {
        if self.player_type(p) != 2 {
            return 0;
        }
        1 << ((p as u32 + if unexplored { 8 } else { 0 }) & 0x1f)
    }

    /// 0x469f60 (set) / 0x469ec0 (clear): the tiles under building `u` at `p`.
    pub fn mark_building_tiles(&mut self, d: &Data, u: u32, p: (i16, i16), set: bool) {
        let (pw, ph) = placement(d, self.units[u as usize].unit_type);
        let (w, h) = (pw / 32, ph / 32);
        let x = p.0 as i32 / 32 - w / 2;
        let y = p.1 as i32 / 32 - h / 2;
        if set {
            self.terrain.set_tile_flags(x as usize, y as usize, w as usize, h as usize, TILE_BUILDING);
        } else {
            self.terrain.clear_tile_flags(x as usize, y as usize, w as usize, h as usize, TILE_BUILDING);
        }
    }

    /// 0x473fb0: can `player` (with builder `u`, or NIL) place `t` at tile (tx, ty)?
    /// 0 = yes; otherwise the worst tile result. Fills placement grid `slot`.
    #[allow(clippy::too_many_arguments)]
    pub fn placement_check(&mut self, d: &Data, u: u32, player: u8, tx: i16, ty: i16, t: u16, slot: usize, a7: bool, a8: bool, a9: bool) -> u8 {
        let flags = d.dat.units.flags[t as usize];
        let (pw, ph) = placement(d, t);
        let (w, h) = ((pw / 32) as i16, (ph / 32) as i16);
        let r = 'fail: {
            if tx < 0 || ty < 0 || w as i32 + tx as i32 > self.terrain.width as i32 || h as i32 + ty as i32 > self.terrain.height as i32 {
                break 'fail 3;
            }
            if u != NIL && self.units[u as usize].status & 6 == 0 && !self.can_reach_spot(u, (tx.wrapping_mul(32), ty.wrapping_mul(32))) {
                break 'fail 7;
            }
            if is_gas_building(t) {
                break 'fail self.gas_spot_check(d, tx as i32, ty as i32, player, a7);
            }
            if flags & 0x8_0000 != 0 {
                // 0x473920: the building's centre must be powered.
                let (pw, ph) = placement(d, t);
                if !self.psi_powered(d, t, pw / 2 + tx as i32 * 32, ph / 2 + ty as i32 * 32, player) {
                    break 'fail 6;
                }
            }
            let terrain = if u != NIL && self.units[u as usize].unit_type == 0x86 {
                todo_port!("0x473150", "nydus exit placement");
                0
            } else if flags & 0x2_0000 != 0 {
                self.creep_placement(tx, ty, player, w, h, slot, a7, a9)
            } else {
                self.terrain_check(d, t, tx, ty, player, w, h, slot, a7)
            };
            // 0x6d5dcc (a builder override) is never set during a game.
            let units = self.units_check(d, u, tx, ty, player, t, w, h, slot, flags & 2 == 0, a8, a9);
            return terrain.max(units);
        };
        self.placement[slot * 0x30..slot * 0x30 + 0x30].fill(r);
        r
    }

    /// 0x42fa00: a ground unit can walk to `p` (same region group).
    fn can_reach_spot(&self, u: u32, p: (i16, i16)) -> bool {
        self.units[u as usize].pathing_flags & 1 == 0 || self.same_group(u, p)
    }

    /// 0x473db0: a gas building at tile (tx, ty) needs a geyser there (10 = unexplored,
    /// 8 = no geyser).
    fn gas_spot_check(&mut self, d: &Data, tx: i32, ty: i32, player: u8, a7: bool) -> u8 {
        let (gw, gh) = placement(d, 0xbc);
        let x = gw / 2 + tx * 32;
        let y = gh / 2 + ty * 32;
        if self.player_type(player) == 2 {
            let (w, h) = (gw / 32, gh / 32);
            let unexplored = self.human_tile_mask(player, true);
            let fogged = self.human_tile_mask(player, false);
            let rect = |st: &State, mask: u32, any: bool| -> bool {
                (ty..ty + h).any(|yy| (tx..tx + w).any(|xx| (st.tile_flag(xx, yy) & mask != 0) == any))
            };
            // 0x4027f0: any tile unexplored.
            if rect(self, unexplored, true) {
                return 10;
            }
            // 0x402770: every tile fogged.
            if !rect(self, fogged, false) {
                if a7 {
                    todo_port!("0x487c70", "remembered geyser fog sprite");
                    return 8;
                }
                return 0;
            }
        }
        if self.unit_of_type_at(0xbc, x as i16, y as i16) != NIL { 0 } else { 8 }
    }

    /// 0x4734b0: a unit of type `t` on the same tile as (x, y) among the units at (x, y).
    pub(super) fn unit_of_type_at(&mut self, t: u16, x: i16, y: i16) -> u32 {
        let found = self.units_at_point(x, y);
        let (tx, ty) = (x as i32 / 32, y as i32 / 32);
        found
            .into_iter()
            .find(|&c| {
                let (cx, cy) = self.unit_sprite_pos(c);
                self.units[c as usize].unit_type == t && cx as i32 / 32 == tx && cy as i32 / 32 == ty
            })
            .unwrap_or(NIL)
    }

    /// 0x4300e0: units whose finder box contains (x, y).
    fn units_at_point(&mut self, x: i16, y: i16) -> Vec<u32> {
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let b = [x, y, x.wrapping_add(search.max_w as i16), y.wrapping_add(search.max_h as i16)];
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
            .into_iter()
            .filter(|&c| {
                let [l, r, t, b] = self.finder_box(c);
                l <= x as i32 && (x as i32) < r && t <= y as i32 && (y as i32) < b
            })
            .collect()
    }

    /// A unit's finder box (0x66ff7c / 0x6769bc entries).
    fn finder_box(&self, c: u32) -> [i32; 4] {
        let fi = self.units[c as usize].finder_index;
        [self.finder.x[fi[0] as usize].1, self.finder.x[fi[1] as usize].1, self.finder.y[fi[2] as usize].1, self.finder.y[fi[3] as usize].1]
    }

    fn finder_box_on_tile(&self, c: u32, x: i32, y: i32) -> bool {
        let [l, r, t, b] = self.finder_box(c);
        l < x * 32 + 32 && x * 32 < r && t < y * 32 + 32 && y * 32 < b
    }

    /// 0x473a10: terrain per tile (10 unexplored, 4 unbuildable, 2 too close to
    /// resources for a town hall).
    #[allow(clippy::too_many_arguments)]
    fn terrain_check(&mut self, d: &Data, t: u16, tx: i16, ty: i16, player: u8, w: i16, h: i16, slot: usize, a7: bool) -> u8 {
        let beacon = (0xc2..=0xc7).contains(&t);
        let flags = d.dat.units.flags[t as usize];
        let unexplored = self.human_tile_mask(player, true);
        let fogged = self.human_tile_mask(player, false);
        let mut blocking = 0x2080_0000u32;
        if d.dat.units.staredit_group_flags[t as usize] & 1 == 0 {
            blocking |= if a7 { 0x4000_0000 } else { 0x40_0000 };
        }
        let mut worst = 0;
        for row in 0..h.max(0) as i32 {
            for col in 0..w.max(0) as i32 {
                let (x, y) = (tx as i32 + col, ty as i32 + row);
                let f = self.tile_flag(x, y);
                let v = if unexplored & f != 0 {
                    10
                } else if (!a7 && fogged & f != 0) || blocking & f == 0 || (beacon && (f as i32) < 0) {
                    if flags & 0x1000 != 0 && self.near_resource(d, a7, x as i16, y as i16) { 2 } else { 0 }
                } else {
                    4
                };
                self.placement[(row as usize + slot * 8) * 6 + col as usize] = v;
                worst = worst.max(v);
            }
        }
        worst
    }

    /// 0x473010: Zerg buildings need creep on every tile (5 = no creep, 10 unexplored).
    #[allow(clippy::too_many_arguments)]
    fn creep_placement(&mut self, tx: i16, ty: i16, player: u8, w: i16, h: i16, slot: usize, a7: bool, a9: bool) -> u8 {
        let unexplored = self.human_tile_mask(player, true);
        let fogged = self.human_tile_mask(player, false);
        let creep = if a7 { 0x4000_0000 } else { 0x40_0000 };
        let mut worst = 0;
        for row in 0..h.max(0) as i32 {
            for col in 0..w.max(0) as i32 {
                let f = self.tile_flag(tx as i32 + col, ty as i32 + row);
                let mut v = 0;
                if unexplored & f != 0 {
                    v = 10;
                    worst = 10;
                }
                if (a9 || a7 || fogged & f == 0) && creep & f == 0 {
                    v = 5;
                    if worst < 6 {
                        worst = 5;
                    }
                }
                self.placement[(row as usize + slot * 8) * 6 + col as usize] = v;
            }
        }
        worst
    }

    /// 0x473990: a resource within 3 tiles of tile (x, y).
    fn near_resource(&mut self, d: &Data, a7: bool, x: i16, y: i16) -> bool {
        if a7 {
            todo_port!("0x487b00", "remembered resource fog sprites");
        }
        let b = [x.wrapping_sub(3).wrapping_mul(32), y.wrapping_sub(3).wrapping_mul(32), x.wrapping_add(4).wrapping_mul(32), y.wrapping_add(4).wrapping_mul(32)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        // 0x4e8280 with 0x473490: units.dat flag 0x2000 (resource container).
        found.into_iter().any(|c| d.dat.units.flags[self.units[c as usize].unit_type as usize] & 0x2000 != 0)
    }

    /// 0x473720: units in the way, per tile not already failed (4 blocked, 1 blocked by
    /// a unit the player can't see).
    #[allow(clippy::too_many_arguments)]
    fn units_check(&mut self, d: &Data, u: u32, tx: i16, ty: i16, player: u8, t: u16, w: i16, h: i16, slot: usize, a7: bool, a8: bool, a9: bool) -> u8 {
        let fogged = self.human_tile_mask(player, false);
        let (pw, ph) = placement(d, t);
        let cx = (pw / 2 + tx as i32 * 32) as i16;
        let cy = (ph / 2 + ty as i32 * 32) as i16;
        let [l, up, r, dn] = dims(d, t);
        let b = [cx.wrapping_sub(l), cy.wrapping_sub(up), cx.wrapping_add(r), cy.wrapping_add(dn)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        let mut worst = 0;
        for row in 0..h.max(0) as i32 {
            for col in 0..w.max(0) as i32 {
                let i = (row as usize + slot * 8) * 6 + col as usize;
                let (x, y) = (tx as i32 + col, ty as i32 + row);
                let f = self.tile_flag(x, y);
                if self.placement[i] != 0 || (!a9 && f & fogged != 0) {
                    continue;
                }
                let v = if f & TILE_BUILDING != 0 && !self.own_building_on_tile(u, x, y) {
                    4
                } else {
                    self.unit_in_the_way(&found, u, x, y, a7, a8)
                };
                worst = worst.max(v);
                self.placement[i] = v;
            }
        }
        search.pop();
        self.search = search;
        worst
    }

    /// 0x473410 (inverted): `u` is itself a building covering tile (x, y).
    fn own_building_on_tile(&self, u: u32, x: i32, y: i32) -> bool {
        if u == NIL {
            return false;
        }
        let un = &self.units[u as usize];
        un.status & 2 != 0 && un.unit_type != 0x86 && !self.sprite_hidden(u) && self.finder_box_on_tile(u, x, y)
    }

    fn sprite_hidden(&self, u: u32) -> bool {
        self.sprites[self.units[u as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN != 0
    }

    /// 0x473300: the first unit of `found` blocking tile (x, y).
    fn unit_in_the_way(&self, found: &[u32], u: u32, x: i32, y: i32, a7: bool, a8: bool) -> u8 {
        for &o in found {
            let ou = &self.units[o as usize];
            let unseen = u != NIL && ou.status & 0x300 != 0 && ou.visibility & (1 << (self.units[u as usize].player & 0x1f)) == 0;
            if (a7 || ou.status & 0x2_0000 == 0)
                && o != u
                && ou.status & 6 == 0
                && ou.unit_type != 0xca
                && ou.unit_type != 0x69
                && (!unseen || a8)
                && !self.sprite_hidden(o)
                && self.finder_box_on_tile(o, x, y)
            {
                return if unseen { 1 } else { 4 };
            }
        }
        0
    }

    /// 0x466a70: drop build queue entry `i` (relative to the current slot), refunding it.
    fn cancel_queued(&mut self, d: &Data, u: u32, i: usize) {
        if i >= 5 {
            return;
        }
        let mut j = (self.units[u as usize].build_queue_slot as usize + i) % 5;
        let t = self.units[u as usize].build_queue[j];
        if t == NONE {
            return;
        }
        if i == 0 && self.units[u as usize].current_build_unit != NIL {
            let c = self.units[u as usize].current_build_unit;
            self.cancel_unit(d, c);
        } else if d.dat.units.flags[t as usize] & 1 == 0 {
            let p = self.units[u as usize].player as usize;
            self.refund(d, p, t);
        }
        if self.units[u as usize].ai != NIL {
            todo_port!("0x466a70", "AI town build queue shift");
        }
        for _ in 0..4 - i {
            if self.units[u as usize].build_queue[j] == NONE {
                break;
            }
            let n = (j + 1) % 5;
            self.units[u as usize].build_queue[j] = self.units[u as usize].build_queue[n];
            j = n;
        }
        self.units[u as usize].build_queue[j] = NONE;
    }

    /// 0x466e80: empty the build queue, refunding it.
    pub(super) fn clear_build_queue(&mut self, d: &Data, u: u32) {
        for i in (0..5).rev() {
            let j = (self.units[u as usize].build_queue_slot as usize + i) % 5;
            if self.units[u as usize].build_queue[j] != NONE {
                self.cancel_queued(d, u, i);
            }
        }
        let un = &mut self.units[u as usize];
        un.build_queue = [NONE; 5];
        un.build_queue_slot = 0;
    }

    /// Build command: 0x48e010 (placement check at tile (tx, ty)) then 0x48de70.
    pub fn command_build(&mut self, d: &Data, u: u32, order: u8, tx: i16, ty: i16, t: u16) {
        let p = self.units[u as usize].player;
        let r = self.placement_check(d, u, p, tx, ty, t, 1, false, false, false);
        if r != 0 {
            return;
        }
        let (pw, ph) = placement(d, t);
        let pos = ((pw / 2 + tx as i32 * 32) as i16, (ph / 2 + ty as i32 * 32) as i16);
        self.build_order(d, u, order, pos, t);
    }

    /// 0x48de70: order `u` to build `t` at `pos`.
    pub fn build_order(&mut self, d: &Data, u: u32, order: u8, pos: (i16, i16), t: u16) {
        if order == 0x47 {
            self.order_issue_now(d, u, 0x47, pos, NIL, 0xe4);
            return;
        }
        let un = &self.units[u as usize];
        if un.unit_type == 0x29 && ((un.order == 0x46 && un.status & 0x3000 != 0) || un.order == 0x1a) {
            return;
        }
        if (un.unit_type == 0x86 && order == 0x2e) || order == 0x9b {
            self.order_issue_now(d, u, order, pos, NIL, 0xe4);
            return;
        }
        let p = un.player;
        if !self.supply_ok(d, p as usize, t) {
            return;
        }
        let (m, g) = (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32);
        if self.players.minerals[p as usize] < m || self.players.gas[p as usize] < g {
            todo_port!("0x48ee30", "not enough minerals / gas message");
            return;
        }
        self.order_issue_now(d, u, order, pos, NIL, 0xe4);
        let ut = self.units[u as usize].unit_type;
        if d.dat.units.flags[ut as usize] & 1 != 0 && d.dat.units.flags[t as usize] & 1 != 0 {
            self.clear_build_queue(d, u);
            self.units[u as usize].set_u16_at(0xc4, t);
            return;
        }
        self.clear_build_queue(d, u);
        self.queue_build(d, u, t);
    }

    /// 0x401240: `u`'s exact position is within `dist` of `p`.
    pub(super) fn within(&self, u: u32, p: (i16, i16), dist: u32) -> bool {
        let e = self.units[u as usize].fl.exact;
        super::path::approx_distance(e.0, (p.0 as i32) << 8, e.1, (p.1 as i32) << 8) >> 8 <= dist
    }

    /// 0x4eb9c0: move to `p`, facing it.
    pub(super) fn move_to_facing(&mut self, d: &Data, u: u32, p: (i16, i16)) -> bool {
        let r = self.set_move_target_xy(d, u, p);
        let f = &mut self.units[u as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        r
    }

    /// 0x467030: check placement again and pay for the building.
    pub(super) fn pay_for_building(&mut self, d: &Data, u: u32) -> bool {
        let un = &self.units[u as usize];
        let t = un.build_queue[un.build_queue_slot as usize];
        let p = un.player;
        let (m, g) = (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32);
        if !self.supply_ok(d, p as usize, t) {
            return false;
        }
        if self.players.minerals[p as usize] < m || self.players.gas[p as usize] < g {
            todo_port!("0x48ee30", "not enough minerals / gas message");
            return false;
        }
        let (pw, ph) = placement(d, t);
        let pos = self.units[u as usize].order_target_pos;
        let tx = ((pos.0 as i32 - pw / 2) / 32) as i16;
        let ty = ((pos.1 as i32 - ph / 2) / 32) as i16;
        if self.placement_check(d, u, p, tx, ty, t, 0, false, true, false) != 0 {
            // 0x48f6c0: error sound.
            return false;
        }
        self.players.minerals[p as usize] -= m;
        self.players.gas[p as usize] -= g;
        true
    }

    pub(super) fn idle_order_of(&self, d: &Data, u: u32) -> u8 {
        d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize]
    }

    /// 0x467fd0: PlaceBuilding.
    pub fn order_place_building(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].ai != NIL {
            self.units[u as usize].status |= 0x80_0000;
        }
        match self.units[u as usize].order_state {
            0 => {
                let t = self.units[u as usize].order_target_unit;
                if t != NIL {
                    self.units[u as usize].order_target_pos = self.unit_sprite_pos(t);
                }
                let p = self.units[u as usize].order_target_pos;
                if self.move_to_facing(d, u, p) {
                    self.units[u as usize].order_state = 1;
                }
            }
            1 => {
                match self.unit_at_move_target(u) {
                    0 => return,
                    2 => return self.order_done_or_next(d, u),
                    _ => {}
                }
                let p = self.units[u as usize].order_target_pos;
                if !self.within(u, p, 0x80) || !self.pay_for_building(d, u) {
                    return self.order_done_or_next(d, u);
                }
                self.start_construction(d, u);
            }
            _ => {}
        }
    }

    /// The rest of 0x467fd0 once paid: create the building and start constructing it.
    fn start_construction(&mut self, d: &Data, u: u32) {
        let un = &mut self.units[u as usize];
        un.status &= !0x10_0000;
        let s = un.sprite;
        let ut = un.unit_type;
        self.sprites[s as usize].elevation = d.dat.units.elevation[ut as usize] + 1;
        let head = self.units[u as usize].order_queue_head;
        if head != NIL {
            self.order_insert_before(d, u, 0x96, 0xe4, (0, 0), NIL, head);
        } else {
            self.order_queue_append(d, u, 0x96, 0xe4, (0, 0), NIL);
            let idle = self.idle_order_of(d, u);
            self.order_queue_append(d, u, idle, 0xe4, (0, 0), NIL);
        }
        let un = &self.units[u as usize];
        let slot = un.build_queue_slot as usize;
        let t = un.build_queue[slot];
        let (pos, player) = (un.order_target_pos, un.player);
        let b = if t == 0x6e {
            self.take_over_geyser(d, u, t)
        } else {
            self.create_unit(d, t, pos.0, pos.1, player)
        };
        self.units[u as usize].build_queue[slot] = NONE;
        let Some(b) = b else {
            todo_port!("0x49e530", "show the creation error");
            let idle = self.idle_order_of(d, u);
            return self.order_now(d, u, idle);
        };
        self.units[b as usize].connected_unit = u;
        let un = &mut self.units[u as usize];
        un.order = 0x21;
        un.order_state = 3;
        un.order_target_unit = b;
        // 0x48dda0 / 0x48d9a0 / 0x48e310 / 0x4c36c0: UI refresh.
        self.order_now(d, b, 0x2c);
        if self.player_type(self.units[b as usize].player) == 1 {
            todo_port!("0x435770", "AI: new building joins its town");
        }
    }

    /// Run `anim` on every image of `u`'s sprite (0x4d8470 per image).
    pub(super) fn unit_anim(&mut self, d: &Data, u: u32, anim: u8) {
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, anim);
    }

    /// 0x467a70: ConstructingBuilding.
    pub fn order_constructing_building(&mut self, d: &Data, u: u32) {
        let b = self.units[u as usize].order_target_unit;
        if b == NIL || self.units[b as usize].status & 1 != 0 {
            return self.construction_done(d, u, b);
        }
        match self.units[u as usize].order_state {
            0 | 2 => {
                if self.units[u as usize].order_state == 0 {
                    if !self.move_to_unit_aim(d, u, b) {
                        return;
                    }
                    self.units[u as usize].order_state = 2;
                }
                let r = self.unit_at_move_target(u);
                if r == 0 {
                    return;
                }
                if r != 2 {
                    // 0x4c36c0: UI refresh.
                    let c = self.units[b as usize].connected_unit;
                    if c == NIL || c == u || self.units[c as usize].order_target_unit != b {
                        self.units[b as usize].connected_unit = u;
                        let un = &mut self.units[u as usize];
                        un.status &= !0x10_0000;
                        let s = un.sprite;
                        let ut = un.unit_type;
                        self.sprites[s as usize].elevation = d.dat.units.elevation[ut as usize] + 1;
                        self.issue_order(d, u, 0x96, false, (0, 0), NIL, 0xe4);
                        let idle = self.idle_order_of(d, u);
                        self.issue_order(d, u, idle, false, (0, 0), NIL, 0xe4);
                        let p = self.unit_sprite_pos(b);
                        if self.move_to_facing(d, u, p) {
                            self.units[u as usize].order_state = 3;
                        }
                        return;
                    }
                }
                return self.order_done_or_next(d, u);
            }
            3 => {
                if self.unit_at_move_target(u) != 0 {
                    self.units[u as usize].order_state = 4;
                }
                return;
            }
            4 => {
                if !self.pick_construction_spot(d, u, b) {
                    return;
                }
                self.units[u as usize].order_state = 5;
            }
            5 => self.units[u as usize].order_state = 6,
            6 => {
                let p = self.units[u as usize].order_target_pos;
                if self.within(u, p, 0x14) {
                    self.unit_halt(d, u);
                    let bp = self.unit_sprite_pos(b);
                    let f = &mut self.units[u as usize].fl;
                    if f.next_target_waypoint != bp {
                        f.next_target_waypoint = bp;
                    }
                    let up = self.unit_sprite_pos(u);
                    let wp = self.units[u as usize].fl.next_target_waypoint;
                    let dir = crate::game::flingy::direction_to(up.0 as i32, up.1 as i32, wp.0 as i32, wp.1 as i32);
                    let (vx, vy) = crate::game::flingy::direction_table()[dir as usize];
                    let un = &mut self.units[u as usize];
                    un.order_target_pos = ((up.0 as u16).wrapping_add(((vx * 20) >> 8) as u16) as i16, up.1.wrapping_add(((vy * 20) >> 8) as i16));
                    un.order_state = 7;
                }
            }
            7 => {
                let un = &self.units[u as usize];
                let facing = if un.fl.next_target_waypoint == un.fl.pos {
                    true
                } else {
                    let up = self.unit_sprite_pos(u);
                    let wp = un.fl.next_target_waypoint;
                    let dir = crate::game::flingy::direction_to(up.0 as i32, up.1 as i32, wp.0 as i32, wp.1 as i32) as i32;
                    let mut diff = un.fl.heading as i32 - dir;
                    if diff < 0 {
                        diff += 0x100;
                    }
                    if diff > 0x80 {
                        diff = 0x100 - diff;
                    }
                    let w = self.ground_weapon(d, u);
                    diff <= d.dat.weapons.attack_angle[w as usize] as i32
                };
                if facing {
                    let r = self.rng.rand(0xa);
                    self.units[u as usize].main_order_timer = ((r as u8) & 0x3f) + 0x1e;
                    self.unit_anim(d, u, 0xf);
                    self.units[u as usize].order_state = 8;
                }
            }
            8 => {
                if self.units[u as usize].main_order_timer == 0 {
                    self.unit_anim(d, u, 8);
                    self.units[u as usize].order_state = 4;
                }
            }
            _ => {}
        }
        let mut gain = self.units[b as usize].hp_gain as i32;
        if self.game_options & 2 != 0 {
            gain = (gain << 4) & 0xffff;
        }
        self.build_progress(d, b, gain, false);
        if self.units[b as usize].status & 1 != 0 {
            self.construction_done(d, u, b);
        }
    }

    /// State 4 of 0x467a70: walk to a random spot on the building.
    fn pick_construction_spot(&mut self, d: &Data, u: u32, b: u32) -> bool {
        let bs = &self.sprites[self.units[b as usize].sprite as usize];
        let (bw, bh) = (bs.width as u16, bs.height as u16);
        let (bx, by) = (bs.x as u16, bs.y as u16);
        let x0 = bx.wrapping_sub(bw >> 1) as i16;
        let x1 = bw.wrapping_add(x0 as u16).wrapping_sub(1) as i16;
        let y0 = by.wrapping_sub(bh >> 1) as i16;
        let y1 = bh.wrapping_add(y0 as u16).wrapping_sub(1) as i16;
        let bt = self.units[b as usize].unit_type;
        let (pw, ph) = placement(d, bt);
        let ut = self.units[u as usize].unit_type;
        let [l, up, r, dn] = dims(d, ut);
        let lx0 = (bx as i32 - pw / 2) as i16;
        let ly0 = (by as i32 - ph / 2) as i16;
        let mut lx = lx0.wrapping_add(l).wrapping_add(1);
        let mut rx = (pw as i16).wrapping_add(lx0).wrapping_sub(1).wrapping_add(-1i16).wrapping_sub(r);
        let mut ty = ly0.wrapping_add(up).wrapping_add(1);
        let mut by_ = (ph as i16).wrapping_add(ly0).wrapping_sub(1).wrapping_add(-1i16).wrapping_sub(dn);
        if lx < x0 {
            lx = x0;
        }
        if ty < y0 {
            ty = y0;
        }
        if rx > x1 {
            rx = x1;
        }
        if by_ > y1 {
            by_ = y1;
        }
        let mut p = (self.rng.rand_between(9, x0 as i32, x1 as i32) as i16, self.rng.rand_between(9, y0 as i32, y1 as i32) as i16);
        self.clamp_unit_pos(d, ut, &mut p);
        let up_ = self.unit_sprite_pos(u);
        self.units[u as usize].order_target_pos = p;
        if self.terrain_blocks_move(d, u, p.0.wrapping_sub(up_.0), p.1.wrapping_sub(up_.1)) {
            p = (self.rng.rand_between(9, lx as i32, rx as i32) as i16, self.rng.rand_between(9, ty as i32, by_ as i32) as i16);
            self.clamp_unit_pos(d, ut, &mut p);
            self.units[u as usize].order_target_pos = p;
        }
        self.move_to_facing(d, u, p)
    }

    /// The end of 0x467a70: stop building animations and move on.
    fn construction_done(&mut self, d: &Data, u: u32, b: u32) {
        self.unit_anim(d, u, 0xc);
        if b != NIL {
            self.units[b as usize].connected_unit = NIL;
            // 0x48f310: "construction complete" sound.
            let bu = &self.units[b as usize];
            let own_gas = bu.status & 1 != 0 && is_gas_building(bu.unit_type) && bu.player == self.units[u as usize].player;
            if own_gas || (0xb0..=0xb2).contains(&bu.unit_type) {
                return self.order_with_target(d, u, 0x4f, b);
            }
        }
        if self.units[u as usize].order_queue_head != NIL {
            self.units[u as usize].order_flags |= 1;
            return self.prepare_next_order(d, u);
        }
        if self.units[u as usize].ai != NIL {
            return self.order_now(d, u, 0x9c);
        }
        let idle = self.idle_order_of(d, u);
        self.order_now(d, u, idle);
    }

    /// 0x4e65e0: give `u`'s sprite a fresh main image: the units.dat construction image
    /// when `construction` (and the type has one), else the sprite's own image.
    pub fn reset_unit_image(&mut self, d: &Data, u: u32, construction: bool) {
        if self.units[u as usize].status & 0x300 != 0 {
            todo_port!("0x497fd0", "cloaked unit: re-cloak the new images");
        }
        let saved = self.iscript_unit;
        self.iscript_unit = Some(u);
        let s = self.units[u as usize].sprite;
        let ut = self.units[u as usize].unit_type as usize;
        let c = d.dat.units.construction_image[ut] as u16;
        let img = if construction && c != 0 { c } else { d.dat.sprites.image[self.sprites[s as usize].sprite_id as usize] };
        let heading = self.units[u as usize].fl.heading;
        self.replace_sprite_images(d, s, img, heading);
        self.iscript_unit = saved;
        // 0x492da0: status overlays (ensnare, plague, ...) are re-added here; new units have none.
        let main = self.sprites[s as usize].main_image;
        self.units[u as usize].building_overlay_state = d.damage_overlay_count(self.images[main as usize].image_id);
    }

    /// 0x499bb0: replace every image of sprite `s` with a new main image `img`.
    /// Selection circles / health bars (UI-only images) are not modelled.
    pub(super) fn replace_sprite_images(&mut self, d: &Data, s: u32, img: u16, heading: u8) {
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            let next = self.images[i as usize].link.next;
            self.destroy_image(i);
            i = next;
        }
        self.sprites[s as usize].main_image = NIL;
        // 0x498e00(0, 0, 0)
        self.create_image(d, s, NIL, img, 0, 0, 0);
        self.sprite_images_set_direction(d, s, heading);
    }

    /// 0x467760: IncompleteBuilding: construction animation stages by hp.
    pub fn order_incomplete_building(&mut self, d: &Data, u: u32) {
        let max = d.dat.units.hit_points[self.units[u as usize].unit_type as usize];
        let hp = self.units[u as usize].hp;
        let past = |n: u32| (((max.wrapping_mul(n)) / 5) as i32) < hp;
        match self.units[u as usize].order_state {
            0 if past(1) => self.units[u as usize].order_state = 1,
            1 => {
                self.unit_anim(d, u, 0xd);
                self.units[u as usize].order_state = 2;
            }
            2 if past(2) => self.units[u as usize].order_state = 3,
            3 => {
                self.unit_anim(d, u, 0xe);
                self.units[u as usize].order_state = 4;
            }
            4 if past(3) => self.units[u as usize].order_state = 5,
            5 => {
                self.reset_unit_image(d, u, false);
                self.unit_anim(d, u, 0xf);
                self.units[u as usize].order_state = 6;
            }
            6 if past(4) => self.order_now(d, u, 0x17),
            _ => {}
        }
    }
}
