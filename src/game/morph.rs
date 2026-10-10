//! Zerg larva/eggs/morphing and self-completing structures (Zerg morph, Protoss warp-in).
//!
//! order_ZergUnitMorph (0x1401eced0) turns larva (35) into an Egg (36), hydras (38)
//! into Lurker Eggs (97), mutas (43) into Cocoons (59); remaining time lives on the egg.

use super::economy::{self, TrainError};
use super::flingy::Flingy;
use super::{Game, UnitId};
use crate::iscript::{anim, Sprite, World};

pub const LARVA: usize = 35;
pub const EGG: usize = 36;
pub const HATCHERIES: [usize; 3] = [131, 132, 133];
/// ponytail: BW's SpawningLarva (0x1401efb70) didn't decompile; ~342 frames per larva is the known cadence.
pub const LARVA_TIME: u16 = 342;
/// units.dat flag: two units hatch from one egg (zerglings, scourge).
pub const TWO_IN_EGG: u32 = 0x400;

/// Swap a unit's type in place: new sprite/flingy, same position, owner and slot.
pub fn transform(g: &mut Game, w: &World, u: UnitId, kind: usize) {
    let (x, y) = g.unit_pos(u);
    let owner = g.units[u].owner;
    let old = g.units[u].sprite;
    g.sprites[old] = None;
    let d = w.dat;
    let sprite_id = d.flingy.sprite[d.units.flingy[kind] as usize];
    let mut s = Sprite::new(w, &mut g.rng, sprite_id, owner, x, y);
    s.elevation = d.units.elevation[kind];
    s.unit = Some(u);
    let heading = s.direction;
    let slot = g.add_sprite(s);
    let un = &mut g.units[u];
    un.kind = kind;
    un.sprite = slot;
    un.flingy = Flingy::new(&d.flingy, d.units.flingy[kind] as u16, x as i16, y as i16, heading);
    un.hp = d.units.hit_points[kind] as i32;
    un.shields = (d.units.shields[kind] as i32) << 8;
    un.order = d.units.ai_human_idle[kind];
    un.order_state = 0;
    un.order_queue.clear();
    un.path.clear();
}

/// Larva (or hydra/muta) starts morphing into `kind`.
pub fn morph_unit(g: &mut Game, w: &World, u: UnitId, kind: usize) -> Result<(), TrainError> {
    let owner = g.units[u].owner as usize;
    let ud = &w.dat.units;
    let (m, gas) = (ud.mineral_cost[kind] as u32, ud.gas_cost[kind] as u32);
    if g.players[owner].minerals < m {
        return Err(TrainError::Minerals);
    }
    if g.players[owner].gas < gas {
        return Err(TrainError::Gas);
    }
    let (used, max) = economy::supply(g, w, owner as u8, 0);
    let need = ud.supply_required[kind] as u32 * if ud.flags[kind] & TWO_IN_EGG != 0 { 2 } else { 1 };
    // The larva/source's own supply is freed by the morph.
    let freed = ud.supply_required[g.units[u].kind] as u32;
    if need > 0 && used + need > max + freed {
        return Err(TrainError::Supply);
    }
    g.players[owner].minerals -= m;
    g.players[owner].gas -= gas;
    let cocoon = match g.units[u].kind {
        38 => 97,
        43 => 59,
        _ => EGG,
    };
    transform(g, w, u, cocoon);
    let un = &mut g.units[u];
    un.build_queue.push_back(kind);
    un.build_timer = ud.build_time[kind];
    Ok(())
}

/// Per-frame Zerg/Protoss bookkeeping for unit `u`: eggs hatching, larva spawning,
/// structures completing on their own.
pub fn update(g: &mut Game, w: &World, u: UnitId) {
    let kind = g.units[u].kind;
    if matches!(kind, EGG | 59 | 97) {
        if g.units[u].build_timer > 0 {
            g.units[u].build_timer -= 1;
            return;
        }
        let Some(target) = g.units[u].build_queue.pop_front() else { return };
        let (x, y) = g.unit_pos(u);
        let owner = g.units[u].owner;
        transform(g, w, u, target);
        if w.dat.units.flags[target] & TWO_IN_EGG != 0 {
            g.create_unit(w, target, owner, x + 8, y + 8, 100);
        }
        return;
    }
    if HATCHERIES.contains(&kind) && g.units[u].completed {
        let larva = g.units.iter().filter(|l| !l.dead && l.kind == LARVA && l.parent == Some(u)).count();
        if larva >= 3 {
            g.units[u].larva_timer = LARVA_TIME;
            return;
        }
        if g.units[u].larva_timer > 0 {
            g.units[u].larva_timer -= 1;
            return;
        }
        g.units[u].larva_timer = LARVA_TIME;
        let (x, y) = economy::spawn_point(g, w, u, LARVA);
        let owner = g.units[u].owner;
        let l = g.create_unit(w, LARVA, owner, x, y, 100);
        g.units[l].parent = Some(u);
        return;
    }
    // Structures that build themselves (Zerg morphs, Protoss warp-ins).
    if !g.units[u].completed && g.units[u].self_building {
        let max = w.dat.units.hit_points[kind] as i32;
        let bt = w.dat.units.build_time[kind].max(1) as i32;
        let un = &mut g.units[u];
        un.hp = (un.hp + (max - max / 10) / bt).min(max);
        un.build_timer = un.build_timer.saturating_sub(1);
        if un.build_timer == 0 {
            un.completed = true;
            un.hp = max;
            let sprite = un.sprite;
            let mut fx = Vec::new();
            if let Some(s) = g.sprites[sprite].as_mut() {
                s.set_anim(w, &mut g.rng, anim::BUILT, &mut fx);
            }
            g.pending_fx.extend(fx);
        }
    }
}
