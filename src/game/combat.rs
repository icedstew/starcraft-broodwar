//! Weapons, bullets, damage, death (first pass).
//!
//! Structure follows SC:R's attack code (order_AttackUnit 0x1401b3b60,
//! attack_with_weapon 0x1401b5100); details not yet verified against the
//! decomp are marked `ponytail:`.

use super::flingy::{direction_to, xy_length, Flingy};
use super::orders::{self, id};
use super::{Game, Target, UnitId};
use crate::iscript::{anim, Sprite, World};

pub const NO_WEAPON: u8 = 130;

/// Damage multipliers [damage_type][unit_size], 256 = 100% (table at 0x140db1670).
const DAMAGE_MULT: [[i32; 4]; 5] = [
    [0, 0, 0, 0],       // independent
    [0, 128, 192, 256], // explosive
    [0, 256, 128, 64],  // concussive
    [0, 256, 256, 256], // normal
    [0, 256, 256, 256], // ignore armor
];

pub struct Bullet {
    pub weapon: u8,
    pub source: Option<UnitId>,
    pub target: Target,
    pub sprite: usize,
    pub flingy: Flingy,
    pub flying: bool,
    pub owner: u8,
    pub dead: bool,
}

pub fn is_flyer(w: &World, kind: usize) -> bool {
    w.dat.units.flags[kind] & 0x4 != 0
}

/// Bounding box (left, top, right, bottom) around a unit's position.
fn bounds(g: &Game, w: &World, u: UnitId) -> (i32, i32, i32, i32) {
    let (x, y) = g.unit_pos(u);
    let dm = w.dat.units.dimensions[g.units[u].kind];
    let (l, t, r, b) = ((dm & 0xFFFF) as i32, (dm >> 16 & 0xFFFF) as i32, (dm >> 32 & 0xFFFF) as i32, (dm >> 48) as i32);
    (x - l, y - t, x + r, y + b)
}

/// Edge-to-edge distance between two units' bounding boxes.
pub fn units_distance(g: &Game, w: &World, a: UnitId, b: UnitId) -> u32 {
    let (al, at, ar, ab) = bounds(g, w, a);
    let (bl, bt, br, bb) = bounds(g, w, b);
    let dx = if bl > ar { bl - ar } else if al > br { al - br } else { 0 };
    let dy = if bt > ab { bt - ab } else if at > bb { at - bb } else { 0 };
    xy_length(0, dx, 0, dy)
}

pub fn is_enemy(g: &Game, a: UnitId, b: UnitId) -> bool {
    // ponytail: no alliance table yet; everyone except neutral (11) and self is hostile.
    let (oa, ob) = (g.units[a].owner, g.units[b].owner);
    oa != ob && oa < 8 && ob < 8
}

/// Weapon this unit (or its turret) would use against `target`.
pub fn weapon_for(g: &Game, w: &World, u: UnitId, target: UnitId) -> u8 {
    let k = g.units[u].kind;
    if is_flyer(w, g.units[target].kind) { w.dat.units.air_weapon[k] } else { w.dat.units.ground_weapon[k] }
}

/// Guard/idle orders: pick the nearest enemy in acquisition range. ponytail: O(n) scan, BW uses the unit finder.
pub fn acquire_target(g: &Game, w: &World, u: UnitId) -> Option<UnitId> {
    let uk = g.units[u].kind;
    let units = &w.dat.units;
    if units.ground_weapon[uk] == NO_WEAPON && units.air_weapon[uk] == NO_WEAPON {
        return None;
    }
    let range = (units.target_acquisition_range[uk] as u32) * 32;
    let mut best: Option<(u32, UnitId)> = None;
    for t in 0..g.units.len() {
        if t == u || g.units[t].dead || !is_enemy(g, u, t) || weapon_for(g, w, u, t) == NO_WEAPON {
            continue;
        }
        let d = units_distance(g, w, u, t);
        let reach = range.max(w.dat.weapons.max_range[weapon_for(g, w, u, t) as usize]);
        if d <= reach && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, t));
        }
    }
    best.map(|(_, t)| t)
}

/// order_AttackUnit (0x1401b3b60), simplified: chase into range, face, fire on cooldown.
pub fn order_attack_unit(g: &mut Game, w: &World, u: UnitId) {
    let Some(t) = g.units[u].order_target.unit.filter(|&t| !g.units[t].dead) else {
        orders::order_done(g, w, u);
        return;
    };
    let wpn = weapon_for(g, w, u, t);
    if wpn == NO_WEAPON {
        orders::order_done(g, w, u);
        return;
    }
    let (tx, ty) = g.unit_pos(t);
    let range = w.dat.weapons.max_range[wpn as usize];
    let is_sub = w.dat.units.flags[g.units[u].kind] & 0x10 != 0;
    // Units fighting from Hold Position never chase.
    let holding = g.units[u].order_queue.front().is_some_and(|(o, _)| *o == id::HOLD_POSITION);
    if units_distance(g, w, u, t) > range {
        if holding {
            orders::order_done(g, w, u);
            return;
        }
        // Re-path only when the target moved away from our current goal (pathing is not free).
        let goal_far = {
            let un = &g.units[u];
            let goal = un.path.back().copied().unwrap_or(un.flingy.move_target);
            (goal.0 as i32 - tx).abs() + (goal.1 as i32 - ty).abs() > 32
        };
        if !is_sub && goal_far {
            g.move_unit(w, u, tx as i16, ty as i16);
        }
        return;
    }
    // In range: stop and face the target (BW sets next_target_waypoint so the flingy turns in place).
    let fl = &mut g.units[u].flingy;
    if !is_sub && fl.move_target != fl.pos {
        let p = fl.pos;
        fl.set_move_target(p.0, p.1);
        g.units[u].path.clear();
    }
    let fl = &mut g.units[u].flingy;
    fl.next_target_waypoint = (tx as i16, ty as i16);
    fl.flags |= super::flingy::mflags::TURNING;
    let (ux, uy) = (fl.pos.0 as i32, fl.pos.1 as i32);
    let want = direction_to(ux, uy, tx, ty);
    let diff = want.wrapping_sub(fl.heading) as i8;
    if diff.unsigned_abs() > w.dat.weapons.attack_angle[wpn as usize] {
        return;
    }
    let air = is_flyer(w, g.units[t].kind);
    let unit = &mut g.units[u];
    let cd = if air { &mut unit.air_cooldown } else { &mut unit.ground_cooldown };
    if *cd != 0 {
        return;
    }
    // ponytail: BW adds a small random jitter to the cooldown; not applied yet.
    *cd = w.dat.weapons.cooldown[wpn as usize];
    let first = !unit.attacking;
    unit.attacking = true;
    let a = match (air, first) {
        (false, true) => anim::GND_ATTK_INIT,
        (false, false) => anim::GND_ATTK_RPT,
        (true, true) => anim::AIR_ATTK_INIT,
        (true, false) => anim::AIR_ATTK_RPT,
    };
    let sprite = unit.sprite;
    let mut fx = Vec::new();
    if let Some(s) = g.sprites[sprite].as_mut() {
        s.set_anim(w, &mut g.rng, a, &mut fx);
    }
    g.pending_fx.extend(fx);
}

/// Fire `wpn` from `u` at its current target: spawn the bullet sprite.
pub fn fire_weapon(g: &mut Game, w: &World, u: UnitId, wpn: u8) {
    if wpn == NO_WEAPON {
        return;
    }
    let Some(t) = g.units[u].order_target.unit.filter(|&t| !g.units[t].dead) else { return };
    let wd = &w.dat.weapons;
    let wi = wpn as usize;
    let behavior = wd.behavior[wi];
    let (sx, sy) = g.unit_pos(u);
    let (tx, ty) = g.unit_pos(t);
    // Behaviors: 0 fly, 1 fly & follow, 2 appear on target unit, 3 persist on site,
    // 4 appear on target site, 5 appear on attacker, 6 attack & self-destruct, 7 bounce, 8 area, 9 max range.
    let at_target = matches!(behavior, 2..=4);
    let (bx, by) = if at_target { (tx, ty) } else { (sx, sy) };
    let flingy_id = wd.flingy[wi] as u16;
    let sprite_id = w.dat.flingy.sprite[flingy_id as usize];
    let heading = direction_to(sx, sy, tx, ty);
    let mut s = Sprite::new(w, &mut g.rng, sprite_id, g.units[u].owner, bx, by);
    s.elevation = w.dat.units.elevation[g.units[u].kind] + 1;
    let mut fl = Flingy::new(&w.dat.flingy, flingy_id, bx as i16, by as i16, heading);
    let flying = matches!(behavior, 0 | 1);
    if flying {
        fl.set_move_target(tx as i16, ty as i16);
    }
    let bi = g.bullets.len();
    s.bullet = Some(bi);
    s.set_direction(w, heading);
    let mut fx = Vec::new();
    if !flying {
        s.set_anim(w, &mut g.rng, anim::DEATH, &mut fx);
    }
    let sprite = g.add_sprite(s);
    let owner = g.units[u].owner;
    g.bullets.push(Bullet { weapon: wpn, source: Some(u), target: Target { pos: (tx as i16, ty as i16), unit: Some(t) }, sprite, flingy: fl, flying, owner, dead: false });
    g.pending_fx.extend(fx);
}

/// Move flying bullets; on arrival play their Death anim (which does the damage via domissiledmg).
pub fn update_bullets(g: &mut Game, w: &World) {
    for b in 0..g.bullets.len() {
        if g.bullets[b].dead || !g.bullets[b].flying {
            continue;
        }
        if let Some(t) = g.bullets[b].target.unit.filter(|&t| !g.units[t].dead) {
            let (tx, ty) = g.unit_pos(t);
            if w.dat.weapons.behavior[g.bullets[b].weapon as usize] == 1 {
                g.bullets[b].flingy.set_move_target(tx as i16, ty as i16);
            }
        }
        let r = g.bullets[b].flingy.step(&w.dat.flingy);
        let heading = g.bullets[b].flingy.heading;
        let arrived = g.bullets[b].flingy.pos == g.bullets[b].flingy.move_target;
        let slot = g.bullets[b].sprite;
        let mut fx = Vec::new();
        if let Some(s) = g.sprites[slot].as_mut() {
            s.x = r.new_pos.0 as i32;
            s.y = r.new_pos.1 as i32;
            s.set_direction(w, heading);
            if arrived {
                s.set_anim(w, &mut g.rng, anim::DEATH, &mut fx);
            }
        }
        if arrived {
            g.bullets[b].flying = false;
        }
        g.pending_fx.extend(fx);
    }
}

/// domissiledmg: apply the bullet's weapon damage to its target.
pub fn bullet_hit(g: &mut Game, w: &World, b: usize) {
    let bl = &g.bullets[b];
    if bl.dead {
        return;
    }
    let (wpn, target, src) = (bl.weapon, bl.target.unit, bl.source);
    // ponytail: single-target only; splash radii (inner/medium/outer) not applied yet.
    if let Some(t) = target {
        deal_damage(g, w, t, wpn, src);
    }
    g.bullets[b].dead = true;
}

/// Melee (attackmelee): weapon damage straight to the current target.
pub fn melee_hit(g: &mut Game, w: &World, u: UnitId) {
    let Some(t) = g.units[u].order_target.unit.filter(|&t| !g.units[t].dead) else { return };
    let wpn = weapon_for(g, w, u, t);
    if wpn != NO_WEAPON {
        deal_damage(g, w, t, wpn, Some(u));
    }
}

/// 0x1401b2d30 weapon damage (with 0x140185470 for the base amount).
/// ponytail: upgrades, acid spores, defensive matrix and hallucinations not modelled yet.
pub fn deal_damage(g: &mut Game, w: &World, t: UnitId, wpn: u8, src: Option<UnitId>) {
    let wd = &w.dat.weapons;
    let wi = wpn as usize;
    let unit = &g.units[t];
    if unit.dead || unit.hp == 0 {
        return;
    }
    let kind = unit.kind;
    let units = &w.dat.units;
    // Base: explosion type 5 deals max(500, 2/3 of current hp+shields); otherwise damage + level*bonus.
    let base = if wd.explosion_type[wi] == 5 {
        let hp = unit.hp >> 8;
        let total = if units.shield_enable[kind] != 0 { hp + units.shields[kind] as i32 } else { hp };
        (total * 2 / 3).max(500)
    } else {
        wd.damage[wi] as i32
    };
    let divisor = 1;
    let mut dmg = ((base << 8) / divisor).max(0x80);
    let dtype = (wd.damage_type[wi] as usize).min(4);
    let mut absorbed = 0;
    if units.shield_enable[kind] != 0 && unit.shields > 0xFF {
        // Shield armor upgrade level would be subtracted here (unless ignore-armor).
        absorbed = dmg.min(unit.shields);
        dmg -= absorbed;
    }
    if dtype != 4 {
        let armor = (units.armor[kind] as i32) << 8;
        dmg -= armor.min(dmg);
    }
    let size = (units.size[kind] as usize).min(3);
    let mut hp_dmg = ((dmg as u32 * DAMAGE_MULT[dtype][size] as u32) >> 8) as i32;
    if absorbed == 0 && hp_dmg < 0x80 {
        hp_dmg = 0x80;
    }
    let unit = &mut g.units[t];
    unit.shields -= absorbed;
    unit.hp -= hp_dmg;
    if unit.hp <= 0 {
        kill_unit(g, w, t);
    } else if let Some(s) = src {
        // Retaliate if idle (ponytail: BW's reaction logic is more involved).
        if g.units[t].order == w.dat.units.ai_human_idle[kind] && acquire_ok(g, w, t, s) {
            g.issue_order(t, id::ATTACK_UNIT, Target { pos: (0, 0), unit: Some(s) }, false);
        }
    }
}

fn acquire_ok(g: &Game, w: &World, u: UnitId, t: UnitId) -> bool {
    !g.units[t].dead && is_enemy(g, u, t) && weapon_for(g, w, u, t) != NO_WEAPON
}

pub fn kill_unit(g: &mut Game, w: &World, u: UnitId) {
    if g.units[u].dead {
        return;
    }
    g.units[u].dead = true;
    g.units[u].hp = 0;
    let sprite = g.units[u].sprite;
    let mut fx = Vec::new();
    if let Some(s) = g.sprites[sprite].as_mut() {
        s.unit = None;
        s.set_anim(w, &mut g.rng, anim::DEATH, &mut fx);
    }
    g.pending_fx.extend(fx);
    if let Some(sub) = g.units[u].subunit {
        g.units[sub].dead = true;
        if let Some(s) = g.sprites[g.units[sub].sprite].take() {
            drop(s);
        }
    }
}
