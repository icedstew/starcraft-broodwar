//! Game simulation (being ported from the SC:R binary; see re/NOTES.md).
//!
//! Frame order (0x14031c610): enable RNG, timed events, AI, every-100-frame
//! updates, update_units, bullets, thingies, disable RNG. Only the parts
//! ported so far run here.

pub mod build;
pub mod buttons;
pub mod combat;
pub mod economy;
pub mod flingy;
pub mod orders;
pub mod path;

use crate::dat;
use crate::iscript::{anim, Effect, Rng, Sprite, World};
use crate::map::Chk;
use flingy::Flingy;
use std::collections::VecDeque;

pub type UnitId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pos: (i16, i16),
    pub unit: Option<UnitId>,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub kind: usize,       // unit type (unit_type(u))
    pub owner: u8,         // +0x68
    pub hp: i32,           // +0x10, 24.8 fixed point
    pub shields: i32,      // +0x88, 24.8 fixed point
    pub flingy: Flingy,
    pub sprite: usize,     // slot in Game::sprites
    pub order: u8,         // +0x69
    pub order_state: u8,   // +0x6a
    pub order_target: Target, // +0x78 pos, +0x80 unit
    pub order_timer: u8,   // +0x70
    pub order_process_timer: u8, // +0xc9
    pub order_queue: VecDeque<(u8, Target)>, // +0xa8 head
    pub subunit: Option<UnitId>, // +0xa0
    pub ground_cooldown: u8, // +0x71
    pub air_cooldown: u8,    // +0x72
    /// In an attack cycle (next attack uses the Rpt anim instead of Init).
    pub attacking: bool,
    /// False while a structure is under construction.
    pub completed: bool,
    /// Worker: structure it is on its way to place.
    pub build_kind: Option<usize>,
    /// Resource units: amount left. Workers: cargo carried.
    pub resources: u32,
    pub carry: u8,
    /// Worker: mineral field it harvests. Mineral field: worker currently mining it.
    pub gather_field: Option<UnitId>,
    pub miner: Option<UnitId>,
    /// Production queue (unit types) and frames left on the front item.
    pub build_queue: VecDeque<usize>,
    pub build_timer: u16,
    /// Frames spent blocked by another unit (collision).
    pub blocked: u8,
    /// Where a Patrol order started (it walks back here, then out again).
    pub patrol_origin: (i16, i16),
    /// Remaining waypoints toward move_target (ground units).
    pub path: VecDeque<(i16, i16)>,
    pub dead: bool,
}

pub struct Game {
    pub frame: u32,
    pub rng: Rng,
    pub units: Vec<Unit>,
    pub sprites: Vec<Option<Sprite>>,
    pub bullets: Vec<combat::Bullet>,
    pub players: [economy::Player; 12],
    /// Effects raised outside the sprite pass (anims started by orders), handled with the frame's effects.
    pub pending_fx: Vec<Effect>,
    pub tileset: u16,
    pub walk: path::WalkGrid,
    /// Buildable 32px tiles, row-major over map_tiles (w, h).
    pub buildable: Vec<bool>,
    pub map_tiles: (usize, usize),
}

impl Game {
    pub fn new(w: &World, chk: &Chk, walk: path::WalkGrid, buildable: Vec<bool>) -> Game {
        let mut g = Game { frame: 0, rng: Rng { seed: 0 }, units: Vec::new(), sprites: Vec::new(), bullets: Vec::new(), players: [economy::Player { minerals: 50, gas: 0 }; 12], pending_fx: Vec::new(), tileset: chk.tileset() as u16, walk, buildable, map_tiles: chk.dims() };
        g.place_map_objects(w, chk);
        g
    }

    pub fn add_sprite(&mut self, s: Sprite) -> usize {
        if let Some(i) = self.sprites.iter().position(Option::is_none) {
            self.sprites[i] = Some(s);
            i
        } else {
            self.sprites.push(Some(s));
            self.sprites.len() - 1
        }
    }

    pub fn create_unit(&mut self, w: &World, kind: usize, owner: u8, x: i32, y: i32, hp_pct: u32) -> UnitId {
        let d = w.dat;
        let u = &d.units;
        let sprite_id = d.flingy.sprite[u.flingy[kind] as usize];
        let mut s = Sprite::new(w, &mut self.rng, sprite_id, owner, x, y);
        s.elevation = u.elevation[kind];
        let id = self.units.len();
        s.unit = Some(id);
        let fl = Flingy::new(&d.flingy, u.flingy[kind] as u16, x as i16, y as i16, s.direction);
        let sprite = self.add_sprite(s);
        self.units.push(Unit {
            kind,
            owner,
            hp: (u.hit_points[kind] as i64 * hp_pct as i64 / 100) as i32,
            shields: (u.shields[kind] as i32) << 8,
            flingy: fl,
            sprite,
            order: u.ai_human_idle[kind],
            order_state: 0,
            order_target: Target { pos: (x as i16, y as i16), unit: None },
            order_timer: 0,
            order_process_timer: 0,
            order_queue: VecDeque::new(),
            subunit: None,
            ground_cooldown: 0,
            air_cooldown: 0,
            attacking: false,
            completed: true,
            build_kind: None,
            resources: 0,
            carry: 0,
            gather_field: None,
            miner: None,
            build_queue: VecDeque::new(),
            build_timer: 0,
            blocked: 0,
            patrol_origin: (x as i16, y as i16),
            path: VecDeque::new(),
            dead: false,
        });
        if (u.subunit1[kind] as usize) < dat::Units::COUNT {
            let sub = self.create_unit(w, u.subunit1[kind] as usize, owner, x, y, 100);
            if let Some(s) = self.sprites[self.units[sub].sprite].as_mut() {
                s.elevation = u.elevation[kind] + 1;
            }
            self.units[id].subunit = Some(sub);
        }
        id
    }

    fn place_map_objects(&mut self, w: &World, chk: &Chk) {
        let u = &w.dat.units;
        if let Some(units) = chk.section(b"UNIT") {
            for r in units.chunks_exact(36) {
                let r16 = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let (x, y, kind, owner) = (r16(4) as i32, r16(6) as i32, r16(8) as usize, r[16]);
                if kind >= dat::Units::COUNT || kind == 214 {
                    continue; // 214 = start location, never drawn
                }
                // UNIT record: hp% at offset 0x11.
                let pct = if r[17] == 0 { 100 } else { r[17] as u32 };
                let id = self.create_unit(w, kind, owner, x, y, pct);
                // UNIT record: resource amount at offset 20.
                self.units[id].resources = u32::from_le_bytes(r[20..24].try_into().unwrap());
            }
        }
        if let Some(thg2) = chk.section(b"THG2") {
            for r in thg2.chunks_exact(10) {
                let r16 = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let (id, x, y, owner, flags) = (r16(0), r16(2) as i32, r16(4) as i32, r[6], r16(8));
                // Flag 0x1000: pure sprite; otherwise id is a unit type (doodad unit).
                let sprite = if flags & 0x1000 != 0 {
                    id
                } else if (id as usize) < dat::Units::COUNT {
                    w.dat.flingy.sprite[u.flingy[id as usize] as usize]
                } else {
                    continue;
                };
                let s = Sprite::new(w, &mut self.rng, sprite, owner, x, y);
                self.add_sprite(s);
            }
        }
    }

    /// Move toward (x, y), pathing around terrain for ground units.
    pub fn move_unit(&mut self, w: &World, id: UnitId, x: i16, y: i16) {
        let u = &mut self.units[id];
        let flyer = w.dat.units.flags[u.kind] & 0x4 != 0;
        let from = (u.flingy.pos.0 as i32, u.flingy.pos.1 as i32);
        u.path.clear();
        if !flyer {
            match self.walk.find(from, (x as i32, y as i32)) {
                Some(p) => u.path.extend(p),
                None => {
                    // Unreachable: stay put (BW would move to the closest reachable region).
                    u.flingy.set_move_target(u.flingy.pos.0, u.flingy.pos.1);
                    return;
                }
            }
        }
        let first = u.path.pop_front().unwrap_or((x, y));
        u.flingy.set_move_target(first.0, first.1);
    }

    pub fn unit_pos(&self, id: UnitId) -> (i32, i32) {
        let p = self.units[id].flingy.pos;
        (p.0 as i32, p.1 as i32)
    }

    /// Issue an order the way a player command does (replaces the queue unless queued).
    pub fn issue_order(&mut self, id: UnitId, order: u8, target: Target, queued: bool) {
        let u = &mut self.units[id];
        if queued {
            u.order_queue.push_back((order, target));
        } else {
            u.order_queue.clear();
            u.order = order;
            u.order_state = 0;
            u.order_target = target;
            u.order_process_timer = 0;
        }
    }

    /// One game frame. Returns sounds (sfx id, x, y) requested this frame.
    pub fn step(&mut self, w: &World) -> Vec<(u16, i32, i32)> {
        let mut fx = Vec::new();
        for id in 0..self.units.len() {
            if self.units[id].dead {
                continue;
            }
            orders::update_unit(self, w, id);
            economy::update_production(self, w, id);
            self.update_unit_movement(w, id, &mut fx);
        }
        combat::update_bullets(self, w);
        // Thingies / sprites: run every sprite's iscript.
        for slot in 0..self.sprites.len() {
            let Some(mut s) = self.sprites[slot].take() else { continue };
            let alive = s.step(w, &mut self.rng, &mut fx);
            if alive {
                self.sprites[slot] = Some(s);
            } else if let Some(b) = s.bullet {
                self.bullets[b].dead = true;
            }
        }
        let mut sounds = Vec::new();
        let mut pending = std::mem::take(&mut self.pending_fx);
        pending.extend(fx);
        // Effects can raise more effects (an attack anim fires a weapon); drain until quiet.
        let mut queue: std::collections::VecDeque<Effect> = pending.into();
        while let Some(e) = queue.pop_front() {
            match e {
                Effect::Attack { unit } => {
                    if let Some(t) = self.units[unit].order_target.unit.filter(|&t| !self.units[t].dead) {
                        let wpn = combat::weapon_for(self, w, unit, t);
                        combat::fire_weapon(self, w, unit, wpn);
                    }
                }
                Effect::AttackWith { unit, slot } => {
                    let k = self.units[unit].kind;
                    let wpn = if slot == 2 { w.dat.units.air_weapon[k] } else { w.dat.units.ground_weapon[k] };
                    combat::fire_weapon(self, w, unit, wpn);
                }
                Effect::UseWeapon { unit, weapon } => combat::fire_weapon(self, w, unit, weapon),
                Effect::MeleeHit { unit } => combat::melee_hit(self, w, unit),
                Effect::BulletHit { bullet } => combat::bullet_hit(self, w, bullet),
                Effect::RepeatAttack { .. } => {}
                // ponytail: speed upgrades (e.g. leg enhancements) not applied to iscript moves yet.
                Effect::Move { unit, dist } => self.units[unit].flingy.next_speed = (dist as i32) << 8,
                Effect::SpawnSprite { sprite, x, y, elevation_delta } => {
                    let mut s = Sprite::new(w, &mut self.rng, sprite, 11, x, y);
                    s.elevation = s.elevation.saturating_add_signed(elevation_delta);
                    self.add_sprite(s);
                }
                Effect::Sound { id, x, y } => sounds.push((id, x, y)),
            }
            queue.extend(self.pending_fx.drain(..));
        }
        self.frame += 1;
        sounds
    }

    /// Would ground unit `id` moving from `from` to `to` run into another ground unit or building?
    /// Only overlaps that are new at `to` count, so already-stacked units can separate.
    /// ponytail: O(n) scan per moving unit; BW uses contour lists and pushes idle units aside.
    fn blocked_at(&self, w: &World, id: UnitId, from: (i16, i16), to: (i16, i16)) -> bool {
        let flags = &w.dat.units.flags;
        let me = &self.units[id];
        if flags[me.kind] & (0x4 | 0x10) != 0 {
            return false; // flyers and subunits don't collide
        }
        let dim = |k: usize| {
            let d = w.dat.units.dimensions[k];
            ((d & 0xFFFF) as i32, (d >> 16 & 0xFFFF) as i32, (d >> 32 & 0xFFFF) as i32, (d >> 48) as i32)
        };
        let (l, t, r, b) = dim(me.kind);
        let rect = |p: (i16, i16)| (p.0 as i32 - l, p.1 as i32 - t, p.0 as i32 + r, p.1 as i32 + b);
        let (a0, a1) = (rect(from), rect(to));
        self.units.iter().enumerate().any(|(j, o)| {
            if j == id || o.dead || flags[o.kind] & (0x4 | 0x10) != 0 {
                return false;
            }
            let (ol, ot, or, ob) = dim(o.kind);
            let (ox, oy) = (o.flingy.pos.0 as i32, o.flingy.pos.1 as i32);
            let ob_rect = (ox - ol, oy - ot, ox + or, oy + ob);
            let hit = |a: (i32, i32, i32, i32)| a.0 <= ob_rect.2 && a.2 >= ob_rect.0 && a.1 <= ob_rect.3 && a.3 >= ob_rect.1;
            hit(a1) && !hit(a0)
        })
    }

    fn update_unit_movement(&mut self, w: &World, id: UnitId, fx: &mut Vec<Effect>) {
        let u = &mut self.units[id];
        if w.dat.units.flags[u.kind] & 0x1 != 0 {
            return; // buildings don't move
        }
        // Advance along the path when a waypoint is reached.
        if u.flingy.pos == u.flingy.move_target {
            if let Some(next) = u.path.pop_front() {
                u.flingy.set_move_target(next.0, next.1);
            }
        }
        let old = (u.flingy.pos, u.flingy.exact);
        let r = u.flingy.step(&w.dat.flingy);
        if r.new_pos != old.0 && self.blocked_at(w, id, old.0, r.new_pos) {
            // Blocked: stay put; after a while give up and settle here (avoids stacking).
            let u = &mut self.units[id];
            u.flingy.pos = old.0;
            u.flingy.exact = old.1;
            u.blocked = u.blocked.saturating_add(1);
            if u.blocked > 12 {
                u.path.clear();
                let p = u.flingy.pos;
                u.flingy.set_move_target(p.0, p.1);
                u.blocked = 0;
            }
            return;
        }
        let u = &mut self.units[id];
        u.blocked = 0;
        let heading = u.flingy.heading;
        let sub = u.subunit;
        let Some(s) = self.sprites[u.sprite].as_mut() else { return };
        s.x = r.new_pos.0 as i32;
        s.y = r.new_pos.1 as i32;
        s.set_direction(w, heading);
        if r.starting {
            s.set_anim(w, &mut self.rng, anim::WALKING, fx);
        } else if r.stopping {
            s.set_anim(w, &mut self.rng, anim::WALKING_TO_IDLE, fx);
        }
        // Subunits (turrets) ride on their parent.
        if let Some(sub) = sub {
            self.units[sub].flingy.pos = r.new_pos;
            self.units[sub].flingy.exact = r.new_exact;
            if let Some(ss) = self.sprites[self.units[sub].sprite].as_mut() {
                ss.x = r.new_pos.0 as i32;
                ss.y = r.new_pos.1 as i32;
                ss.set_direction(w, heading);
            }
        }
    }
}
