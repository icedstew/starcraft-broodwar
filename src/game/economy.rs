//! Resources, harvesting and unit production (first pass).
//!
//! Harvest orders follow SC:R's order_MoveToMinerals (0x1401bb8c0),
//! order_WaitForMinerals (0x1401bbbc0), order_MiningMinerals (0x1401bb6d0):
//! one worker per mineral field, 75-frame mining (iscript AlmostBuilt), 8 per trip.

use super::combat::units_distance;
use super::orders::{self, id};
use super::{Game, Target, UnitId};
use crate::iscript::{anim, World};

pub const MINERAL_FIELDS: std::ops::RangeInclusive<usize> = 176..=178;
pub const MINE_TIME: u8 = 75;
pub const MINERALS_PER_TRIP: u32 = 8;

pub mod flag {
    pub const BUILDING: u32 = 0x1;
    pub const WORKER: u32 = 0x8;
    pub const RESOURCE_DEPOT: u32 = 0x1000;
    pub const RESOURCE_CONTAINER: u32 = 0x2000;
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Player {
    pub minerals: u32,
    pub gas: u32,
}

pub fn is_worker(w: &World, kind: usize) -> bool {
    w.dat.units.flags[kind] & flag::WORKER != 0
}

pub fn is_mineral_field(kind: usize) -> bool {
    MINERAL_FIELDS.contains(&kind)
}

/// Arrived next to `t`: touching, or stopped trying to get closer (blocked by other workers).
fn arrived(g: &Game, w: &World, u: UnitId, t: UnitId) -> bool {
    let un = &g.units[u];
    units_distance(g, w, u, t) <= 4 || (un.flingy.pos == un.flingy.move_target && un.path.is_empty())
}

fn nearest(g: &Game, w: &World, u: UnitId, pred: impl Fn(&super::Unit) -> bool) -> Option<UnitId> {
    let (x, y) = g.unit_pos(u);
    let _ = w;
    (0..g.units.len())
        .filter(|&i| !g.units[i].dead && pred(&g.units[i]))
        .min_by_key(|&i| {
            let (ix, iy) = g.unit_pos(i);
            super::flingy::xy_length(x, ix, y, iy)
        })
}

/// order_MoveToMinerals (85).
pub fn order_move_to_minerals(g: &mut Game, w: &World, u: UnitId) {
    let field = g.units[u].order_target.unit.filter(|&f| !g.units[f].dead && is_mineral_field(g.units[f].kind));
    let field = match field.or_else(|| nearest(g, w, u, |o| is_mineral_field(o.kind))) {
        Some(f) => f,
        None => {
            orders::order_done(g, w, u);
            return;
        }
    };
    g.units[u].order_target.unit = Some(field);
    g.units[u].gather_field = Some(field);
    if g.units[u].carry > 0 {
        set_order(g, u, id::RETURN_MINERALS, None);
        return;
    }
    match g.units[u].order_state {
        0 => {
            let (fx, fy) = g.unit_pos(field);
            g.move_unit(w, u, fx as i16, fy as i16);
            g.units[u].order_state = 1;
        }
        _ => {
            if arrived(g, w, u, field) {
                stop(g, u);
                set_order(g, u, id::WAIT_FOR_MINERALS, Some(field));
            }
        }
    }
}

/// order_WaitForMinerals (86): start mining once the field is free.
pub fn order_wait_for_minerals(g: &mut Game, w: &World, u: UnitId) {
    let Some(field) = g.units[u].order_target.unit.filter(|&f| !g.units[f].dead) else {
        set_order(g, u, id::MOVE_TO_MINERALS, None);
        return;
    };
    let busy = g.units[field].miner.is_some_and(|m| m != u && !g.units[m].dead && g.units[m].order == id::MINING_MINERALS);
    if busy {
        return;
    }
    g.units[field].miner = Some(u);
    // Face the field, then mine.
    let (fx, fy) = g.unit_pos(field);
    g.units[u].flingy.next_target_waypoint = (fx as i16, fy as i16);
    g.units[u].flingy.flags |= super::flingy::mflags::TURNING;
    set_order(g, u, id::MINING_MINERALS, Some(field));
    let _ = w;
}

/// order_MiningMinerals (87).
pub fn order_mining_minerals(g: &mut Game, w: &World, u: UnitId) {
    let Some(field) = g.units[u].order_target.unit.filter(|&f| !g.units[f].dead) else {
        set_order(g, u, id::MOVE_TO_MINERALS, None);
        return;
    };
    match g.units[u].order_state {
        0 => {
            run_anim(g, w, u, anim::ALMOST_BUILT);
            g.units[u].order_timer = MINE_TIME;
            g.units[u].order_state = 5;
        }
        5 if g.units[u].order_timer == 0 => {
            run_anim(g, w, u, anim::GND_ATTK_TO_IDLE);
            let take = g.units[field].resources.min(MINERALS_PER_TRIP);
            g.units[field].resources -= take;
            g.units[u].carry = take as u8;
            g.units[field].miner = None;
            if g.units[field].resources == 0 {
                super::combat::kill_unit(g, w, field);
            }
            set_order(g, u, id::RETURN_MINERALS, None);
        }
        _ => {}
    }
}

/// ReturnMinerals (90): bring cargo to the nearest own resource depot.
pub fn order_return_minerals(g: &mut Game, w: &World, u: UnitId) {
    let owner = g.units[u].owner;
    let flags = &w.dat.units.flags;
    let Some(depot) = nearest(g, w, u, |o| o.owner == owner && flags[o.kind] & flag::RESOURCE_DEPOT != 0) else {
        orders::order_done(g, w, u);
        return;
    };
    match g.units[u].order_state {
        0 => {
            let (dx, dy) = g.unit_pos(depot);
            g.move_unit(w, u, dx as i16, dy as i16);
            g.units[u].order_state = 1;
        }
        _ => {
            if arrived(g, w, u, depot) {
                stop(g, u);
                g.players[owner as usize].minerals += g.units[u].carry as u32;
                g.units[u].carry = 0;
                let field = g.units[u].gather_field;
                set_order(g, u, id::MOVE_TO_MINERALS, field);
            }
        }
    }
}

fn stop(g: &mut Game, u: UnitId) {
    let p = g.units[u].flingy.pos;
    g.units[u].path.clear();
    g.units[u].flingy.set_move_target(p.0, p.1);
}

fn set_order(g: &mut Game, u: UnitId, order: u8, target: Option<UnitId>) {
    let p = g.units[u].flingy.pos;
    let un = &mut g.units[u];
    un.order = order;
    un.order_state = 0;
    un.order_target = Target { pos: p, unit: target };
    un.order_process_timer = 0;
}

fn run_anim(g: &mut Game, w: &World, u: UnitId, a: usize) {
    let sprite = g.units[u].sprite;
    let mut fx = Vec::new();
    if let Some(s) = g.sprites[sprite].as_mut() {
        s.set_anim(w, &mut g.rng, a, &mut fx);
    }
    g.pending_fx.extend(fx);
}

/// (used, max) supply in half-units for `race` (0 zerg, 1 terran, 2 protoss), as units.dat stores it.
pub fn supply(g: &Game, w: &World, player: u8, race: usize) -> (u32, u32) {
    let u = &w.dat.units;
    let race_flag = [1u8, 2, 4][race];
    let (mut used, mut max) = (0u32, 0u32);
    for un in g.units.iter().filter(|m| !m.dead && m.owner == player && u.staredit_group_flags[m.kind] & race_flag != 0) {
        used += u.supply_required[un.kind] as u32;
        max += u.supply_provided[un.kind] as u32;
        // Units still in a build queue already count against supply.
        used += un.build_queue.iter().map(|&k| u.supply_required[k] as u32).sum::<u32>();
    }
    (used, max.min(400))
}

pub fn race_of(w: &World, kind: usize) -> usize {
    match w.dat.units.staredit_group_flags[kind] & 7 {
        1 => 0,
        4 => 2,
        _ => 1,
    }
}

#[derive(Debug, PartialEq)]
pub enum TrainError {
    Minerals,
    Gas,
    Supply,
    QueueFull,
}

/// Queue `kind` at building `b` (secondary order Train): pays up front like BW.
pub fn train(g: &mut Game, w: &World, b: UnitId, kind: usize) -> Result<(), TrainError> {
    let u = &w.dat.units;
    if g.units[b].build_queue.len() >= 5 {
        return Err(TrainError::QueueFull);
    }
    let owner = g.units[b].owner as usize;
    let (m, gas) = (u.mineral_cost[kind] as u32, u.gas_cost[kind] as u32);
    if g.players[owner].minerals < m {
        return Err(TrainError::Minerals);
    }
    if g.players[owner].gas < gas {
        return Err(TrainError::Gas);
    }
    let (used, max) = supply(g, w, owner as u8, race_of(w, kind));
    if u.supply_required[kind] > 0 && used + u.supply_required[kind] as u32 > max {
        return Err(TrainError::Supply);
    }
    g.players[owner].minerals -= m;
    g.players[owner].gas -= gas;
    if g.units[b].build_queue.is_empty() {
        g.units[b].build_timer = u.build_time[kind];
    }
    g.units[b].build_queue.push_back(kind);
    Ok(())
}

/// Advance a building's production by one frame; spawn finished units below it.
pub fn update_production(g: &mut Game, w: &World, b: UnitId) {
    let Some(&kind) = g.units[b].build_queue.front() else { return };
    if g.units[b].build_timer > 0 {
        g.units[b].build_timer -= 1;
        return;
    }
    g.units[b].build_queue.pop_front();
    if let Some(&next) = g.units[b].build_queue.front() {
        g.units[b].build_timer = w.dat.units.build_time[next];
    }
    let owner = g.units[b].owner;
    let (x, y) = spawn_point(g, w, b, kind);
    let n = g.create_unit(w, kind, owner, x, y, 100);
    // Freshly trained units idle as their human-idle order (like map-placed units).
    let _ = n;
}

/// Free spot for a new unit: start just below the building and spiral out.
/// ponytail: BW searches with its own placement routine; this just avoids overlaps.
fn spawn_point(g: &Game, w: &World, b: UnitId, kind: usize) -> (i32, i32) {
    let (bx, by) = g.unit_pos(b);
    let dim = |k: usize| {
        let d = w.dat.units.dimensions[k];
        ((d & 0xFFFF) as i32, (d >> 16 & 0xFFFF) as i32, (d >> 32 & 0xFFFF) as i32, (d >> 48) as i32)
    };
    let (_, _, _, bb) = dim(g.units[b].kind);
    let (l, t, r, bo) = dim(kind);
    let free = |x: i32, y: i32| {
        g.units.iter().all(|o| {
            if o.dead || w.dat.units.flags[o.kind] & (0x4 | 0x10) != 0 {
                return true;
            }
            let (ol, ot, or, ob) = dim(o.kind);
            let (ox, oy) = (o.flingy.pos.0 as i32, o.flingy.pos.1 as i32);
            x + r < ox - ol || x - l > ox + or || y + bo < oy - ot || y - t > oy + ob
        }) && g.walk.walkable(x >> 3, y >> 3)
    };
    let (sx, sy) = (bx, by + bb + t + 1);
    for ring in 0..20 {
        for i in -ring..=ring {
            for (x, y) in [(sx + i * 16, sy + ring * 16), (sx + i * 16, sy - ring * 16), (sx + ring * 16, sy + i * 16), (sx - ring * 16, sy + i * 16)] {
                if free(x, y) {
                    return (x, y);
                }
            }
        }
    }
    (sx, sy)
}
