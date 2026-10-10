//! Unit update and order execution (0x1401e7650 update_unit, 0x1401e5c60 execute_main_order).

use super::{Game, Target, UnitId};
use crate::iscript::World;

#[allow(dead_code)]
pub mod id {
    pub const DIE: u8 = 0;
    pub const STOP: u8 = 1;
    pub const GUARD: u8 = 2;
    pub const PLAYER_GUARD: u8 = 3;
    pub const TURRET_GUARD: u8 = 4;
    pub const MOVE: u8 = 6;
    pub const ATTACK_UNIT: u8 = 10;
    pub const ATTACK_MOVE: u8 = 14;
    pub const NOTHING: u8 = 23;
    pub const HOLD_POSITION: u8 = 107;
    pub const PATROL: u8 = 152;
    pub const COMPUTER_AI: u8 = 156;
}

/// 0x1401e7650 (subset).
pub fn update_unit(g: &mut Game, w: &World, u: UnitId) {
    update_unit_timers(g, w, u);
    execute_main_order(g, w, u);
}

/// 0x1401e6f30 (subset): cooldown timers and shield regeneration.
fn update_unit_timers(g: &mut Game, w: &World, u: UnitId) {
    let unit = &mut g.units[u];
    unit.order_timer = unit.order_timer.saturating_sub(1);
    let units = &w.dat.units;
    if units.shield_enable[unit.kind] != 0 {
        let max = (units.shields[unit.kind] as i32) << 8;
        if unit.shields != max {
            unit.shields = (unit.shields + 7).min(max);
        }
    }
}

/// 0x1401e5c60 (subset): orders that run on the 9-frame order-process tick.
fn execute_main_order(g: &mut Game, w: &World, u: UnitId) {
    let unit = &mut g.units[u];
    if unit.order_process_timer != 0 {
        unit.order_process_timer -= 1;
        return;
    }
    unit.order_process_timer = 8;
    match unit.order {
        id::MOVE => order_move(g, w, u),
        id::STOP => order_stop(g, w, u),
        // Idle orders: nothing to do until the guard/acquire logic is ported.
        _ => {}
    }
}

/// 0x1401c5830
fn order_move(g: &mut Game, w: &World, u: UnitId) {
    match g.units[u].order_state {
        0 => {
            let t = g.units[u].order_target;
            let (x, y) = match t.unit {
                Some(tu) => {
                    let p = g.unit_pos(tu);
                    (p.0 as i16, p.1 as i16)
                }
                None => t.pos,
            };
            // ponytail: straight-line move target; BW's set_move_target also runs the pathfinder.
            g.units[u].flingy.set_move_target(x, y);
            g.units[u].order_state = 1;
        }
        1 => {
            if g.units[u].flingy.pos == g.units[u].flingy.move_target {
                order_done(g, w, u);
            }
        }
        _ => {}
    }
}

/// 0x1401c5d10: stop moving, then finish the order.
fn order_stop(g: &mut Game, w: &World, u: UnitId) {
    let p = g.units[u].flingy.pos;
    g.units[u].flingy.set_move_target(p.0, p.1);
    order_done(g, w, u);
}

/// 0x1401c9ce0: next queued order, else the unit type's return-to-idle order.
pub fn order_done(g: &mut Game, w: &World, u: UnitId) {
    let unit = &mut g.units[u];
    let (order, target) = match unit.order_queue.pop_front() {
        Some(o) => o,
        None => {
            let p = unit.flingy.pos;
            (w.dat.units.ai_return_to_idle[unit.kind], Target { pos: p, unit: None })
        }
    };
    unit.order = order;
    unit.order_state = 0;
    unit.order_target = target;
    unit.order_process_timer = 0;
}
