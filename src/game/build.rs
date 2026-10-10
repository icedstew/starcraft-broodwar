//! Building placement and Terran-style construction (first pass).
//!
//! Orders: PlaceBuilding (30, 0x1401a5f00) walks the worker to the site and
//! starts the structure; ConstructingBuilding (33, 0x1401a6430) runs every frame.

use super::orders::{self, id};
use super::{Game, Target, UnitId};
use crate::iscript::{anim, World};

/// Placement footprint (w, h) in pixels from units.dat placement_box.
pub fn footprint(w: &World, kind: usize) -> (i32, i32) {
    let p = w.dat.units.placement_box[kind];
    ((p & 0xFFFF) as i32, (p >> 16 & 0xFFFF) as i32)
}

/// Geyser whose center is (x, y).
pub fn geyser_at(g: &Game, x: i32, y: i32) -> Option<UnitId> {
    (0..g.units.len()).find(|&i| !g.units[i].dead && g.units[i].kind == super::economy::GEYSER && g.unit_pos(i) == (x, y))
}

/// Snap a cursor position to the building center BW would use (top-left on the 32px grid).
pub fn snap_on(g: &Game, w: &World, kind: usize, x: i32, y: i32) -> (i32, i32) {
    // Gas buildings snap onto the geyser under the cursor.
    if super::economy::GAS_BUILDINGS.contains(&kind) {
        let d = w.dat.units.dimensions[super::economy::GEYSER];
        let (l, t, r, b) = ((d & 0xFFFF) as i32, (d >> 16 & 0xFFFF) as i32, (d >> 32 & 0xFFFF) as i32, (d >> 48) as i32);
        for i in 0..g.units.len() {
            let (gx, gy) = g.unit_pos(i);
            if !g.units[i].dead && g.units[i].kind == super::economy::GEYSER && (gx - l..=gx + r).contains(&x) && (gy - t..=gy + b).contains(&y) {
                return (gx, gy);
            }
        }
    }
    snap(w, kind, x, y)
}

pub fn snap(w: &World, kind: usize, x: i32, y: i32) -> (i32, i32) {
    let (fw, fh) = footprint(w, kind);
    let left = ((x - fw / 2 + 16) >> 5) << 5;
    let top = ((y - fh / 2 + 16) >> 5) << 5;
    (left + fw / 2, top + fh / 2)
}

/// Can `kind` go at center (x, y)? Terrain must be buildable and no ground unit
/// (other than `builder`) may stand in the footprint.
/// ponytail: no creep/psi rules or fog checks yet.
pub fn can_place(g: &Game, w: &World, kind: usize, (x, y): (i32, i32), builder: Option<UnitId>) -> bool {
    if super::economy::GAS_BUILDINGS.contains(&kind) {
        return geyser_at(g, x, y).is_some();
    }
    let (fw, fh) = footprint(w, kind);
    let (l, t) = (x - fw / 2, y - fh / 2);
    let tiles_w = g.map_tiles.0 as i32;
    for ty in (t >> 5)..((t + fh + 31) >> 5) {
        for tx in (l >> 5)..((l + fw + 31) >> 5) {
            if tx < 0 || ty < 0 || tx >= tiles_w || ty >= g.map_tiles.1 as i32 || !g.buildable[(ty * tiles_w + tx) as usize] {
                return false;
            }
        }
    }
    let flags = &w.dat.units.flags;
    g.units.iter().enumerate().all(|(i, o)| {
        if o.dead || Some(i) == builder || flags[o.kind] & (0x4 | 0x10) != 0 {
            return true;
        }
        let d = w.dat.units.dimensions[o.kind];
        let (ol, ot, or, ob) = ((d & 0xFFFF) as i32, (d >> 16 & 0xFFFF) as i32, (d >> 32 & 0xFFFF) as i32, (d >> 48) as i32);
        let (ox, oy) = (o.flingy.pos.0 as i32, o.flingy.pos.1 as i32);
        ox + or < l || ox - ol >= l + fw || oy + ob < t || oy - ot >= t + fh
    })
}

/// Worker command: build `kind` centered at `site`.
pub fn order_build(g: &mut Game, u: UnitId, kind: usize, site: (i32, i32)) {
    g.issue_order(u, id::PLACE_BUILDING, Target { pos: (site.0 as i16, site.1 as i16), unit: None }, false);
    g.units[u].build_kind = Some(kind);
}

/// order_PlaceBuilding (30).
pub fn order_place_building(g: &mut Game, w: &World, u: UnitId) {
    let Some(kind) = g.units[u].build_kind else {
        orders::order_done(g, w, u);
        return;
    };
    let site = g.units[u].order_target.pos;
    let (sx, sy) = (site.0 as i32, site.1 as i32);
    if g.units[u].order_state == 0 {
        g.move_unit(w, u, site.0, site.1);
        g.units[u].order_state = 1;
        return;
    }
    let (ux, uy) = g.unit_pos(u);
    let (fw, fh) = footprint(w, kind);
    let close = super::flingy::xy_length(ux, sx, uy, sy) <= (fw.max(fh) / 2) as u32;
    let stopped = g.units[u].flingy.pos == g.units[u].flingy.move_target && g.units[u].path.is_empty();
    if !close && !stopped {
        return;
    }
    let owner = g.units[u].owner as usize;
    let (m, gas) = (w.dat.units.mineral_cost[kind] as u32, w.dat.units.gas_cost[kind] as u32);
    if !close || !can_place(g, w, kind, (sx, sy), Some(u)) || g.players[owner].minerals < m || g.players[owner].gas < gas {
        // ponytail: BW shows an error ("Can't build there" / "Not enough minerals").
        g.units[u].build_kind = None;
        orders::order_done(g, w, u);
        return;
    }
    g.players[owner].minerals -= m;
    g.players[owner].gas -= gas;
    // A gas building replaces its geyser and inherits the remaining gas.
    let geyser = geyser_at(g, sx, sy);
    let worker_kind = g.units[u].kind;
    let b = g.create_unit(w, kind, owner as u8, sx, sy, 10);
    if let Some(gy) = geyser {
        g.units[b].resources = g.units[gy].resources;
        g.units[gy].dead = true;
        let s = g.units[gy].sprite;
        g.sprites[s] = None;
    }
    let bt = w.dat.units.build_time[kind].max(1);
    g.units[b].completed = false;
    g.units[b].build_timer = bt;
    g.units[u].build_kind = None;
    let p = g.units[u].flingy.pos;
    g.units[u].flingy.set_move_target(p.0, p.1);
    match worker_kind {
        // Drone is consumed by the morph; probe just warps it in and walks off.
        41 | 64 => {
            g.units[b].self_building = true;
            if worker_kind == 41 {
                g.units[u].dead = true;
                let s = g.units[u].sprite;
                g.sprites[s] = None;
            } else {
                orders::order_done(g, w, u);
            }
            return;
        }
        _ => {}
    }
    let un = &mut g.units[u];
    un.order = id::CONSTRUCTING_BUILDING;
    un.order_state = 0;
    un.order_target = Target { pos: site, unit: Some(b) };
}

/// order_ConstructingBuilding (33): every frame, advance the structure.
pub fn order_constructing(g: &mut Game, w: &World, u: UnitId) {
    let Some(b) = g.units[u].order_target.unit.filter(|&b| !g.units[b].dead && !g.units[b].completed) else {
        orders::order_done(g, w, u);
        return;
    };
    let kind = g.units[b].kind;
    let max = w.dat.units.hit_points[kind] as i32;
    let bt = w.dat.units.build_time[kind].max(1) as i32;
    let bu = &mut g.units[b];
    // HP climbs from 10% to 100% over the build time.
    bu.hp = (bu.hp + (max - max / 10) / bt).min(max);
    bu.build_timer = bu.build_timer.saturating_sub(1);
    if bu.build_timer == 0 {
        bu.completed = true;
        bu.hp = max;
        let sprite = bu.sprite;
        let mut fx = Vec::new();
        if let Some(s) = g.sprites[sprite].as_mut() {
            s.set_anim(w, &mut g.rng, anim::BUILT, &mut fx);
        }
        g.pending_fx.extend(fx);
        if super::economy::GAS_BUILDINGS.contains(&kind) {
            // The builder starts harvesting the refinery it just finished.
            g.issue_order(u, id::MOVE_TO_GAS, Target { pos: g.units[u].flingy.pos, unit: Some(b) }, false);
        } else {
            orders::order_done(g, w, u);
        }
    }
}
