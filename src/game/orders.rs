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
    pub const MOVE_TO_MINERALS: u8 = 85;
    pub const WAIT_FOR_MINERALS: u8 = 86;
    pub const MINING_MINERALS: u8 = 87;
    pub const RETURN_MINERALS: u8 = 90;
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
    unit.ground_cooldown = unit.ground_cooldown.saturating_sub(1);
    unit.air_cooldown = unit.air_cooldown.saturating_sub(1);
    // BW forces an order tick as soon as an attacking unit's weapon is ready (0x1401e6f30).
    if unit.order == id::ATTACK_UNIT && (unit.ground_cooldown == 0 || unit.air_cooldown == 0) {
        unit.order_process_timer = 0;
    }
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
        id::ATTACK_UNIT => super::combat::order_attack_unit(g, w, u),
        id::ATTACK_MOVE | id::PATROL => order_attack_move(g, w, u),
        id::MOVE_TO_MINERALS => super::economy::order_move_to_minerals(g, w, u),
        id::WAIT_FOR_MINERALS => super::economy::order_wait_for_minerals(g, w, u),
        id::MINING_MINERALS => super::economy::order_mining_minerals(g, w, u),
        id::RETURN_MINERALS => super::economy::order_return_minerals(g, w, u),
        id::GUARD | id::PLAYER_GUARD | id::TURRET_GUARD | id::HOLD_POSITION => order_guard(g, w, u),
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
            g.move_unit(w, u, x, y);
            g.units[u].order_state = 1;
        }
        1 => {
            if g.units[u].flingy.pos == g.units[u].flingy.move_target && g.units[u].path.is_empty() {
                order_done(g, w, u);
            }
        }
        _ => {}
    }
}

/// 0x1401c5d10: stop moving, then finish the order.
fn order_stop(g: &mut Game, w: &World, u: UnitId) {
    let p = g.units[u].flingy.pos;
    g.units[u].path.clear();
    g.units[u].flingy.set_move_target(p.0, p.1);
    order_done(g, w, u);
}

/// Idle guard orders: attack the nearest enemy in acquisition range.
/// Hold position comes back to hold after the fight (and never chases, see combat).
fn order_guard(g: &mut Game, w: &World, u: UnitId) {
    if g.units[u].order == id::HOLD_POSITION && g.units[u].order_state == 0 {
        let p = g.units[u].flingy.pos;
        g.units[u].path.clear();
        g.units[u].flingy.set_move_target(p.0, p.1);
        g.units[u].order_state = 1;
    }
    if let Some(t) = super::combat::acquire_target(g, w, u) {
        let unit = &mut g.units[u];
        if unit.order == id::HOLD_POSITION {
            unit.order_queue.push_front((id::HOLD_POSITION, unit.order_target));
        }
        unit.order = id::ATTACK_UNIT;
        unit.order_state = 0;
        unit.order_target = Target { pos: (0, 0), unit: Some(t) };
    }
}

/// Attack-move / patrol: walk toward the target, fighting anything acquired on the way.
/// ponytail: BW's versions (0x1401b3a10, 0x1401b44a0) use the 0x1401b3940-family acquire rules.
fn order_attack_move(g: &mut Game, w: &World, u: UnitId) {
    if g.units[u].order_state == 0 {
        let dest = g.units[u].order_target.pos;
        if g.units[u].order == id::PATROL {
            g.units[u].patrol_origin = g.units[u].flingy.pos;
        }
        g.move_unit(w, u, dest.0, dest.1);
        g.units[u].order_state = 1;
        return;
    }
    if let Some(t) = super::combat::acquire_target(g, w, u) {
        let unit = &mut g.units[u];
        // Resume this order (re-pathing from wherever the fight ends) after the target dies.
        let resume = (unit.order, unit.order_target);
        unit.order_queue.push_front(resume);
        unit.order = id::ATTACK_UNIT;
        unit.order_state = 0;
        unit.order_target = Target { pos: (0, 0), unit: Some(t) };
        return;
    }
    let unit = &g.units[u];
    if unit.flingy.pos == unit.flingy.move_target && unit.path.is_empty() {
        if unit.order == id::PATROL {
            // Turn around: patrol back to where this leg started.
            let back = unit.patrol_origin;
            let here = unit.flingy.pos;
            let unit = &mut g.units[u];
            unit.order_target = Target { pos: back, unit: None };
            unit.patrol_origin = here;
            g.move_unit(w, u, back.0, back.1);
        } else {
            order_done(g, w, u);
        }
    }
}

/// 0x1401c9ce0: next queued order, else the unit type's return-to-idle order.
pub fn order_done(g: &mut Game, w: &World, u: UnitId) {
    if g.units[u].attacking {
        // Leaving an attack cycle: GndAttkToIdle (ponytail: air units use AirAttkToIdle).
        let sprite = g.units[u].sprite;
        let mut fx = Vec::new();
        if let Some(s) = g.sprites[sprite].as_mut() {
            s.set_anim(w, &mut g.rng, crate::iscript::anim::GND_ATTK_TO_IDLE, &mut fx);
        }
        g.pending_fx.extend(fx);
    }
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
    unit.attacking = false;
}
