//! Game simulation (being ported from the SC:R binary; see re/NOTES.md).
//!
//! Frame order (0x14031c610): enable RNG, timed events, AI, every-100-frame
//! updates, update_units, bullets, thingies, disable RNG. Only the parts
//! ported so far run here.

pub mod combat;
pub mod flingy;
pub mod orders;

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
    pub dead: bool,
}

pub struct Game {
    pub frame: u32,
    pub rng: Rng,
    pub units: Vec<Unit>,
    pub sprites: Vec<Option<Sprite>>,
    pub bullets: Vec<combat::Bullet>,
    /// Effects raised outside the sprite pass (anims started by orders), handled with the frame's effects.
    pub pending_fx: Vec<Effect>,
    pub tileset: u16,
}

impl Game {
    pub fn new(w: &World, chk: &Chk) -> Game {
        let mut g = Game { frame: 0, rng: Rng { seed: 0 }, units: Vec::new(), sprites: Vec::new(), bullets: Vec::new(), pending_fx: Vec::new(), tileset: chk.tileset() as u16 };
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
                self.create_unit(w, kind, owner, x, y, pct);
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

    fn update_unit_movement(&mut self, w: &World, id: UnitId, fx: &mut Vec<Effect>) {
        let u = &mut self.units[id];
        if w.dat.units.flags[u.kind] & 0x1 != 0 {
            return; // buildings don't move
        }
        let r = u.flingy.step(&w.dat.flingy);
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
