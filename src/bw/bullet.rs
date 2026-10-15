//! Bullets (CBullet.cpp): creation, the per-frame state machine and weapon damage.

use super::list::{self, Link, List, Node, NIL};
use super::{Data, State};
use crate::game::flingy::{direction_table, Flingy};
use crate::todo_port;

/// 0x64b2e8: 100 bullets of 0x70 bytes.
pub const MAX_BULLETS: usize = 100;

#[derive(Clone, Debug, Default)]
pub struct Bullet {
    pub link: Link,
    /// +0x8
    pub hp: i32,
    /// +0xc
    pub sprite: u32,
    /// +0x10..0x4b (move target unit +0x14 is kept in `move_target_unit`).
    pub fl: Flingy,
    pub move_target_unit: u32,
    /// +0x4c
    pub player: u8,
    /// +0x4d: state (0 init, 1 move to point, 2 move to unit, 3 bounce, 4 damage over
    /// time, 5 dying, 6 move near unit).
    pub state: u8,
    /// +0x4e
    pub order_state: u8,
    /// +0x4f: bits set by the iscript `sigorder`.
    pub order_signal: u8,
    /// +0x50
    pub unit_type: u16,
    /// +0x58
    pub target_pos: (i16, i16),
    /// +0x5c
    pub target: u32,
    /// +0x60
    pub weapon: u8,
    /// +0x61
    pub remaining_time: u8,
    /// +0x62: 1 = missed, 2 = from a hallucination.
    pub flags: u8,
    /// +0x63
    pub bounces: u8,
    /// +0x64
    pub source: u32,
    /// +0x68
    pub previous_bounce_target: u32,
    /// +0x6c
    pub spread_index: u8,
}

impl Node for Bullet {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

/// Bullet pool and lists (0x64eed8 free, 0x64dec4 active, 0x64debc count).
#[derive(Clone, Debug)]
pub struct Bullets {
    pub pool: Vec<Bullet>,
    pub free: List,
    pub active: List,
    pub count: u32,
    /// 0x64deb0 / 0x64dec0: last unit that fired a spinning projectile, and the side.
    pub spin_source: u32,
    pub spin_side: u32,
}

impl Default for Bullets {
    /// 0x48a690
    fn default() -> Bullets {
        let mut pool = vec![Bullet::default(); MAX_BULLETS];
        let mut free = List::default();
        for i in 0..MAX_BULLETS as u32 {
            list::insert_after_head(&mut pool, i, &mut free);
        }
        Bullets { pool, free, active: List::default(), count: 0, spin_source: NIL, spin_side: 0 }
    }
}

impl State {
    pub(super) fn bullet_sprite_pos(&self, b: u32) -> (i16, i16) {
        let s = &self.sprites[self.bullets.pool[b as usize].sprite as usize];
        (s.x, s.y)
    }

    fn unit_pos(&self, u: u32) -> (i16, i16) {
        let s = &self.sprites[self.units[u as usize].sprite as usize];
        (s.x, s.y)
    }

    /// 0x48c260: create a bullet of `weapon` for `source` at (x, y).
    pub fn create_bullet(&mut self, d: &Data, source: u32, weapon: u8, x: i16, y: i16, player: u8, dir: u8) -> Option<u32> {
        let b = self.bullets.free.head;
        if b == NIL || (weapon == 0x67 && self.bullets.count >= 0x50) {
            return None;
        }
        if self.units[source as usize].status & 0x8000 != 0
            && !matches!(weapon, 6 | 0x20..=0x22 | 0x32 | 0x33 | 0x36 | 0x38..=0x3d | 0x44 | 0x45 | 0x52..=0x54 | 0x66 | 0x69..=0x6c)
        {
            return None;
        }
        if !self.init_bullet(d, b, source, weapon, x, y, player, dir) {
            return None;
        }
        let bl = &mut self.bullets;
        list::remove(&mut bl.pool, b, &mut bl.free);
        list::insert_after_head(&mut bl.pool, b, &mut bl.active);
        bl.count += 1;
        Some(b)
    }

    /// 0x496360 for a bullet: flingy fields and the sprite.
    fn init_bullet_flingy(&mut self, d: &Data, b: u32, fid: u16, x: i16, y: i16, player: u8, dir: u8) -> bool {
        let fl = &d.dat.flingy;
        let i = fid as usize;
        let bu = &mut self.bullets.pool[b as usize];
        let f = &mut bu.fl;
        f.flingy_id = fid;
        f.flags = 0;
        f.next_speed = 0;
        f.top_speed = fl.top_speed[i] as i32;
        f.acceleration = fl.acceleration[i];
        f.turn_rate = fl.turn_rate[i];
        f.movement_type = fl.movement_control[i];
        f.pos = (x, y);
        f.exact = ((x as i32) << 8, (y as i32) << 8);
        if f.move_target != (x, y) {
            f.move_target = (x, y);
            bu.move_target_unit = NIL;
            f.flags = 1;
            f.next_waypoint = f.move_target;
        }
        if f.next_target_waypoint != (x, y) {
            f.next_target_waypoint = (x, y);
        }
        f.heading = dir;
        f.velocity_dir_prev = dir;
        bu.hp = 1;
        let s = self.create_sprite(d, fl.sprite[i], x, y, player);
        self.bullets.pool[b as usize].sprite = s.unwrap_or(NIL);
        let Some(s) = s else { return false };
        self.sprite_images_set_direction(d, s, dir);
        true
    }

    /// 0x48bec0
    #[allow(clippy::too_many_arguments)]
    fn init_bullet(&mut self, d: &Data, b: u32, source: u32, weapon: u8, x: i16, y: i16, player: u8, dir: u8) -> bool {
        let w = weapon as usize;
        let fid = d.dat.weapons.flingy[w] as u16;
        if !self.init_bullet_flingy(d, b, fid, x, y, player, dir) {
            return false;
        }
        {
            let bu = &mut self.bullets.pool[b as usize];
            bu.player = player;
            bu.unit_type = 0xe4;
            bu.state = 0;
            bu.order_state = 0;
            bu.target = NIL;
            bu.target_pos = (0, 0);
            bu.order_signal = 0;
            bu.weapon = weapon;
            bu.remaining_time = d.dat.weapons.remove_after[w];
            bu.bounces = 0;
            bu.flags = 0;
            bu.fl.flags |= 8;
        }
        self.bullet_state_init(d, b);
        let spin = d.dat.weapons.launch_spin[w];
        if spin != 0 {
            let side = if source == self.bullets.spin_source {
                (self.bullets.spin_side == 0) as u32
            } else {
                self.rng.rand(0) & 1
            };
            self.bullets.spin_side = side;
            let s = if side == 0 { spin.wrapping_neg() } else { spin };
            let f = &mut self.bullets.pool[b as usize].fl;
            f.velocity_dir_prev = f.velocity_dir_prev.wrapping_add(s);
            f.heading = f.velocity_dir_prev;
            self.bullets.spin_source = source;
        }
        let src = &self.units[source as usize];
        let mut owner = source;
        if d.dat.units.flags[src.unit_type as usize] & 0x10 != 0 {
            owner = src.subunit;
        }
        if src.unit_type == 0x55 {
            // A scarab's hit belongs to its reaver.
            owner = src.ptr_at(0xc0);
        }
        let halluc = src.status & 0x4000_0000 != 0;
        let target = src.order_target_unit;
        let bs = self.bullets.pool[b as usize].sprite;
        if target != NIL {
            let e = self.sprites[self.units[target as usize].sprite as usize].elevation;
            self.sprites[bs as usize].elevation = e.wrapping_add(1);
            let tp = self.unit_pos(target);
            let bu = &mut self.bullets.pool[b as usize];
            bu.target = target;
            bu.target_pos = tp;
        } else {
            let e = self.sprites[src.sprite as usize].elevation;
            let tp = src.order_target_pos;
            self.sprites[bs as usize].elevation = e.wrapping_add(1);
            self.bullets.pool[b as usize].target_pos = tp;
        }
        {
            let bu = &mut self.bullets.pool[b as usize];
            bu.source = owner;
            if halluc {
                bu.flags |= 2;
            }
            bu.previous_bounce_target = NIL;
        }
        let missed = |st: &mut State| -> bool {
            let chance = st.miss_chance(d, target, owner);
            (st.rng.rand(1) & 0xff) <= chance
        };
        let (tx, ty) = direction_table()[dir as usize];
        match d.dat.weapons.behavior[w] {
            2 | 4 => {
                if target != NIL && owner != NIL && missed(self) {
                    let p = self.bullet_sprite_pos(b);
                    let np = (p.0.wrapping_sub(((tx * 0x1e) >> 8) as i16), p.1.wrapping_sub(((ty * 0x1e) >> 8) as i16));
                    self.bullet_set_pos(b, np);
                    self.bullets.pool[b as usize].flags |= 1;
                }
            }
            3 => {
                let p = self.bullets.pool[b as usize].target_pos;
                self.bullet_set_pos(b, p);
            }
            5 => {}
            6 => {
                if owner != NIL {
                    self.units[owner as usize].status |= 0x8000_0000;
                    self.units[owner as usize].order_flags |= 4;
                    self.kill_unit(d, owner);
                }
            }
            8 => {
                // 0x513650: the spread pattern, ended by a 0 x.
                const SPREAD: [(i16, i16); 15] = [(1, 1), (51, 31), (7, 12), (-36, -52), (-49, 25), (40, -46), (-4, -19), (28, 50), (50, -15), (-54, -13), (11, -53), (15, -9), (-19, 50), (-17, 8), (0, 0)];
                let n = self.units[source as usize].bullet_spread;
                let i = n as u8 as usize;
                let bu = &mut self.bullets.pool[b as usize];
                bu.spread_index = n as u8;
                bu.target_pos = (bu.target_pos.0.wrapping_sub(SPREAD[i].0), bu.target_pos.1.wrapping_sub(SPREAD[i].1));
                let p = bu.target_pos;
                self.bullet_set_move_target(b, p);
                let next = n.wrapping_add(1);
                self.units[source as usize].bullet_spread = if SPREAD.get(next as usize).map_or(true, |e| e.0 == 0) { 0 } else { next };
                return true;
            }
            9 => {
                let r = ((d.dat.weapons.max_range[w] + 0x14) << 8) as i32;
                let (vx, vy) = direction_table()[self.bullets.pool[b as usize].fl.heading as usize];
                let sp = self.unit_pos(source);
                let p = (((vx * r) >> 16) as i16).wrapping_add(sp.0);
                let q = (((vy * r) >> 16) as i16).wrapping_add(sp.1);
                self.bullets.pool[b as usize].target_pos = (p, q);
                self.bullet_set_move_target(b, (p, q));
            }
            v @ (0 | 1 | 7) => {
                if v == 7 {
                    self.bullets.pool[b as usize].bounces = 3;
                }
                if target != NIL && owner != NIL && missed(self) {
                    let bu = &mut self.bullets.pool[b as usize];
                    bu.target_pos = (bu.target_pos.0.wrapping_sub(((tx * 0x1e) >> 8) as i16), bu.target_pos.1.wrapping_sub(((ty * 0x1e) >> 8) as i16));
                    bu.flags |= 1;
                }
                let p = self.bullets.pool[b as usize].target_pos;
                self.bullet_set_move_target(b, p);
            }
            _ => return false,
        }
        true
    }

    /// 0x4765b0: chance (out of 256) that `owner`'s shot at `target` misses.
    fn miss_chance(&mut self, d: &Data, target: u32, owner: u32) -> u32 {
        let c = self.cover_chance(d, target);
        if self.units[owner as usize].status & 4 == 0 && self.units[target as usize].status & 4 == 0 {
            let (a, b) = (self.unit_pos(owner), self.unit_pos(target));
            if self.los_elevation(a.0 as i32, a.1 as i32) < self.los_elevation(b.0 as i32, b.1 as i32) && c < 0x78 {
                return 0x77;
            }
        }
        c
    }

    pub(super) fn cover_chance_pub(&mut self, d: &Data, t: u32) -> u32 {
        self.cover_chance(d, t)
    }

    /// 0x476210: miss chance from where `target` stands: all shots miss a ground unit
    /// under a dark swarm, 0x77 / 256 on cover tiles.
    fn cover_chance(&mut self, d: &Data, target: u32) -> u32 {
        let t = &self.units[target as usize];
        if t.status & 4 != 0 {
            return 0;
        }
        // 0x5873f0: completed dark swarms (unit 0xca) of player 11.
        if d.dat.units.flags[t.unit_type as usize] & 1 == 0 && self.players.completed_counts[0xca][11] != 0 {
            let b = self.unit_box(d, target);
            let start = self.units_in_box(d, b);
            let swarm = self.search.results(start).into_iter().any(|c| self.units[c as usize].unit_type == 0xca);
            self.search.pop();
            if swarm {
                return 0xff;
            }
        }
        let (x, y) = self.unit_pos(target);
        let i = (y as i32 / 32) as usize * self.terrain.width as usize + (x as i32 / 32) as usize;
        if self.terrain.tile_flags[i] & 0x10_0000 != 0 { 0x77 } else { 0 }
    }

    /// 0x4955c0: place a bullet at `p`.
    fn bullet_set_pos(&mut self, b: u32, p: (i16, i16)) {
        let bu = &mut self.bullets.pool[b as usize];
        bu.fl.exact = ((p.0 as i32) << 8, (p.1 as i32) << 8);
        bu.fl.pos = p;
        let s = bu.sprite;
        self.move_sprite(s, p.0, p.1);
    }

    /// 0x494b70: move target (no unit).
    fn bullet_set_move_target(&mut self, b: u32, p: (i16, i16)) {
        let bu = &mut self.bullets.pool[b as usize];
        if bu.fl.move_target != p {
            bu.fl.move_target = p;
            bu.move_target_unit = NIL;
            bu.fl.flags = (bu.fl.flags & 0xfb) | 1;
            bu.fl.next_waypoint = p;
        }
    }

    /// 0x495240: move target and target waypoint.
    fn bullet_set_targets(&mut self, b: u32, p: (i16, i16)) {
        self.bullet_set_move_target(b, p);
        let f = &mut self.bullets.pool[b as usize].fl;
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// 0x48aa80 and the inlined copies: switch state, re-aiming at the target unit.
    fn bullet_set_state(&mut self, b: u32, state: u8) {
        let t = self.bullets.pool[b as usize].target;
        let tp = if t != NIL { Some(self.unit_pos(t)) } else { None };
        let bu = &mut self.bullets.pool[b as usize];
        bu.state = state;
        match tp {
            Some(p) => bu.target_pos = p,
            None => bu.target = NIL,
        }
        bu.order_state = 0;
        bu.unit_type = 0xe4;
    }

    fn bullet_run_anim(&mut self, d: &Data, b: u32, anim: u8) {
        let s = self.bullets.pool[b as usize].sprite;
        self.sprite_images_run_anim(d, s, anim);
    }

    /// Switch to the dying state (5) and play the death animation.
    fn bullet_die(&mut self, d: &Data, b: u32) {
        self.bullet_set_state(b, 5);
        self.bullet_run_anim(d, b, 1);
    }

    /// 0x48c3d0: update_bullets.
    pub fn update_bullets(&mut self, d: &Data) {
        let mut b = self.bullets.active.head;
        while b != NIL {
            let next = self.bullets.pool[b as usize].link.next;
            self.iscript_order_unit = None;
            self.iscript_bullet = Some(b);
            self.bullet_update_visibility(b);
            let s = self.bullets.pool[b as usize].sprite;
            if s != NIL {
                self.update_unit_sprite(d, s);
                if self.sprites[s as usize].images.head == NIL {
                    self.bullets.pool[b as usize].sprite = NIL;
                }
            }
            self.bullet_step(d, b);
            b = next;
        }
        self.iscript_bullet = None;
    }

    /// 0x48bcf0
    fn bullet_step(&mut self, d: &Data, b: u32) {
        match self.bullets.pool[b as usize].state {
            0 => self.bullet_state_init(d, b),
            1 => self.bullet_state_move(d, b),
            2 => self.bullet_state_follow(d, b),
            3 => self.bullet_state_bounce(d, b),
            4 => self.bullet_state_persist(d, b),
            5 => self.bullet_state_dying(b),
            6 => self.bullet_state_near(d, b),
            _ => {}
        }
    }

    /// 0x4878f0: sprite visibility from the fog at the bullet's tile.
    fn bullet_update_visibility(&mut self, b: u32) {
        let s = self.bullets.pool[b as usize].sprite;
        if s == NIL || self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN != 0 {
            return;
        }
        let sp = &self.sprites[s as usize];
        let i = (sp.y as i32 / 32) as usize * self.terrain.width as usize + (sp.x as i32 / 32) as usize;
        let v = !(self.terrain.tile_flags[i] as u8);
        if sp.visibility != v {
            self.sprite_set_visibility(s, v);
        }
    }

    /// 0x48bd50: state 0, waiting for the iscript to signal (sigorder 1).
    fn bullet_state_init(&mut self, d: &Data, b: u32) {
        let bu = &mut self.bullets.pool[b as usize];
        if bu.order_signal & 1 == 0 {
            return;
        }
        bu.order_signal &= 0xfe;
        let behavior = d.dat.weapons.behavior[bu.weapon as usize];
        let s = bu.sprite;
        match behavior {
            2 | 4 | 5 | 6 => {
                self.bullet_set_state(b, 5);
                // 0x499d00(1, 1)
                self.sprite_images_run_anim(d, s, 1);
            }
            1 | 7 => {
                self.bullet_set_state(b, behavior_state(behavior));
                self.sprite_images_run_anim(d, s, 2);
                self.bullet_step(d, b);
                return;
            }
            3 => {
                self.bullet_set_state(b, behavior_state(behavior));
                self.sprite_images_run_anim(d, s, 0xe);
            }
            _ => {
                self.bullet_set_state(b, behavior_state(behavior));
                self.sprite_images_run_anim(d, s, 2);
            }
        }
        self.bullet_step(d, b);
    }

    /// 0x48b250: one flingy step toward the move target; time runs out off the map.
    fn bullet_move(&mut self, d: &Data, b: u32) {
        let mut m = self.mv;
        super::flingy::predict(&mut self.bullets.pool[b as usize].fl, d, &mut m);
        self.mv = m;
        self.bullet_apply_move(d, b);
        let p = self.bullets.pool[b as usize].fl.pos;
        let (mw, mh) = (self.map_px.0 as i32, self.map_px.1 as i32);
        if p.0 < 0 || p.0 as i32 >= mw || p.1 < 0 || p.1 as i32 >= mh {
            self.bullets.pool[b as usize].remaining_time = 0;
        }
        // 0x4958c0
        let f = &mut self.bullets.pool[b as usize].fl;
        let mut moved = false;
        if f.pos.0 < 0 {
            f.pos.0 = 0;
            f.exact = (0, (f.pos.1 as i32) << 8);
            moved = true;
        } else if f.pos.0 as i32 >= mw {
            f.pos.0 = (mw - 1) as i16;
            f.exact = ((f.pos.0 as i32) << 8, (f.pos.1 as i32) << 8);
            moved = true;
        }
        if moved {
            let (p, s) = (f.pos, self.bullets.pool[b as usize].sprite);
            self.move_sprite(s, p.0, p.1);
        }
        let f = &mut self.bullets.pool[b as usize].fl;
        let mut moved = false;
        if f.pos.1 < 0 {
            f.pos.1 = 0;
            f.exact = ((f.pos.0 as i32) << 8, 0);
            moved = true;
        } else if f.pos.1 as i32 >= mh {
            f.pos.1 = (mh - 1) as i16;
            f.exact = ((f.pos.0 as i32) << 8, (f.pos.1 as i32) << 8);
            moved = true;
        }
        if moved {
            let (p, s) = (f.pos, self.bullets.pool[b as usize].sprite);
            self.move_sprite(s, p.0, p.1);
        }
    }

    /// 0x496030 for a bullet.
    fn bullet_apply_move(&mut self, d: &Data, b: u32) {
        let m = self.mv;
        let bu = &mut self.bullets.pool[b as usize];
        let f = &mut bu.fl;
        self.mv.old_pos = f.pos;
        f.velocity_dir_prev = f.velocity_dir;
        f.pos = m.new_pos;
        f.flags = m.new_flags;
        f.exact = m.new_exact;
        f.next_speed = f.speed;
        let s = bu.sprite;
        self.move_sprite(s, m.new_pos.0, m.new_pos.1);
        super::flingy::turn_heading(&mut self.bullets.pool[b as usize].fl);
        let h = self.bullets.pool[b as usize].fl.heading;
        self.sprite_images_set_direction(d, s, h);
        if m.stopping {
            if self.sprites[s as usize].flags & 0x80 == 0 {
                self.sprite_images_run_anim(d, s, 0xc);
            }
        } else if m.starting {
            self.sprite_images_run_anim(d, s, 0xb);
        }
    }

    /// 0x48b3b0: state 1, fly to the target point.
    fn bullet_state_move(&mut self, d: &Data, b: u32) {
        self.bullet_move(d, b);
        let bu = &mut self.bullets.pool[b as usize];
        let t = bu.remaining_time;
        bu.remaining_time = t.wrapping_sub(1);
        if t == 0 || bu.fl.move_target == bu.fl.pos {
            self.bullet_die(d, b);
        }
    }

    /// 0x48b500: state 2, follow the target unit.
    fn bullet_state_follow(&mut self, d: &Data, b: u32) {
        let bu = &self.bullets.pool[b as usize];
        let t = bu.target;
        if t != NIL && bu.flags & 1 == 0 {
            let p = self.unit_pos(t);
            self.bullet_set_targets(b, p);
        } else if t != NIL {
            self.bullet_set_state(b, 1);
        } else {
            let bu = &mut self.bullets.pool[b as usize];
            bu.state = 1;
            bu.target = NIL;
            bu.order_state = 0;
            bu.unit_type = 0xe4;
        }
        self.bullet_state_move(d, b);
    }

    /// 0x48b440: state 6, follow the target unit (spread offsets).
    fn bullet_state_near(&mut self, d: &Data, b: u32) {
        let bu = &self.bullets.pool[b as usize];
        let t = bu.target;
        if t != NIL && bu.flags & 1 == 0 {
            todo_port!("0x48b440", "spread offset target point (0x495240 args)");
            let p = self.unit_pos(t);
            self.bullet_set_targets(b, p);
        } else if t != NIL {
            self.bullet_set_state(b, 1);
        } else {
            let bu = &mut self.bullets.pool[b as usize];
            bu.state = 1;
            bu.target = NIL;
            bu.order_state = 0;
            bu.unit_type = 0xe4;
        }
        self.bullet_state_move(d, b);
    }

    /// 0x48b2d0: state 3, bounce between targets.
    fn bullet_state_bounce(&mut self, d: &Data, b: u32) {
        let bu = &self.bullets.pool[b as usize];
        if bu.target != NIL && bu.flags & 1 == 0 {
            let p = self.unit_pos(bu.target);
            self.bullet_set_targets(b, p);
        }
        self.bullet_move(d, b);
        let bu = &mut self.bullets.pool[b as usize];
        if bu.fl.move_target != bu.fl.pos {
            return;
        }
        bu.bounces = bu.bounces.wrapping_sub(1);
        if bu.bounces != 0 {
            let t = self.next_bounce_target(d, b);
            let bu = &mut self.bullets.pool[b as usize];
            bu.previous_bounce_target = bu.target;
            if t != NIL {
                self.bullet_run_anim(d, b, 0xd);
                self.bullets.pool[b as usize].target = t;
                return;
            }
        }
        self.bullet_die(d, b);
    }

    /// 0x48b1e0: the first unit within 0x60 of the bullet that its source can attack
    /// (filter 0x48afd0: attackable, enemy, neither the current nor the previous target).
    fn next_bounce_target(&mut self, d: &Data, b: u32) -> u32 {
        let bu = &self.bullets.pool[b as usize];
        let src = bu.source;
        if src == NIL {
            return NIL;
        }
        let (cur, prev) = (bu.target, bu.previous_bounce_target);
        let sp = &self.sprites[bu.sprite as usize];
        let (x, y) = (sp.x, sp.y);
        let bx = [x.wrapping_sub(0x60), y.wrapping_sub(0x60), x.wrapping_add(0x60), y.wrapping_add(0x60)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, bx);
        let found = search.results(start);
        search.pop();
        self.search = search;
        found
            .into_iter()
            .find(|&c| self.can_attack(d, src, c, true) && self.is_enemy(src, c) && c != cur && c != prev)
            .unwrap_or(NIL)
    }

    /// 0x48bc70: state 4, damage every 7 frames until the time runs out.
    fn bullet_state_persist(&mut self, d: &Data, b: u32) {
        let bu = &mut self.bullets.pool[b as usize];
        let t = bu.remaining_time;
        bu.remaining_time = t.wrapping_sub(1);
        if t == 0 {
            self.bullet_die(d, b);
        } else if t.wrapping_sub(1) % 7 == 0 {
            self.bullet_damage(d, b);
        }
    }

    /// 0x48a560: state 5, free the bullet once its sprite is gone.
    fn bullet_state_dying(&mut self, b: u32) {
        if self.bullets.pool[b as usize].sprite != NIL {
            return;
        }
        let bl = &mut self.bullets;
        list::remove(&mut bl.pool, b, &mut bl.active);
        list::insert_after_head(&mut bl.pool, b, &mut bl.free);
        bl.count -= 1;
    }

    /// 0x48b770: apply the weapon's effect (iscript `domissiledmg`).
    pub fn bullet_damage(&mut self, d: &Data, b: u32) {
        let bu = &self.bullets.pool[b as usize];
        let w = bu.weapon as usize;
        match d.dat.weapons.explosion_type[w] {
            0 => {}
            1 => {
                if bu.target == NIL {
                    return;
                }
                if bu.flags & 1 == 0 {
                    let mut mult = 1u32;
                    if d.dat.weapons.behavior[w] == 7 {
                        for _ in 0..2u8.wrapping_sub(bu.bounces) {
                            mult *= 3;
                        }
                    }
                    self.weapon_hit(d, b, bu.target, mult as u8);
                } else if self.units[bu.target as usize].ai != NIL && bu.source != NIL {
                    todo_port!("0x4795d0", "AI notified of a miss");
                }
            }
            0xf => {
                if bu.target != NIL {
                    self.weapon_hit(d, b, bu.target, 1);
                }
            }
            2 | 3 | 5 => {
                let r = d.dat.weapons.outer_splash[w] as i16;
                let sp = &self.sprites[bu.sprite as usize];
                let (x, y) = (sp.x, sp.y);
                let bx = [x.wrapping_sub(r), y.wrapping_sub(r), x.wrapping_add(r), y.wrapping_add(r)];
                let lurker = w == 0x6d;
                let start = self.units_in_box(d, bx);
                for u in self.search.results(start) {
                    if lurker {
                        self.lurker_spine_hit(d, b, u);
                    } else {
                        // 0x48b750: 0x64deb8 = 1 (inner radius does full damage).
                        self.splash_hit(d, b, u, true);
                    }
                }
                self.search.pop();
            }
            4 | 6 | 7 | 8 | 9 | 0xa | 0xb | 0xc | 0xd | 0xe | 0x10 | 0x11 | 0x12 | 0x15 | 0x16 => self.bullet_spell_effect(d, b, d.dat.weapons.explosion_type[w]),
            0x18 => {
                // Air splash: the target (or a random unit in the inner radius) takes a
                // full hit, then everything else in range splash damage.
                let r = d.dat.weapons.outer_splash[w] as i16;
                let sp = &self.sprites[bu.sprite as usize];
                let (x, y) = (sp.x, sp.y);
                let bx = [x.wrapping_sub(r), y.wrapping_sub(r), x.wrapping_add(r), y.wrapping_add(r)];
                let start = self.units_in_box(d, bx);
                let mut picks: Vec<u32> = Vec::new();
                for u in self.search.results(start) {
                    if self.air_splash_candidate(d, b, u, &mut picks) {
                        break;
                    }
                }
                self.search.pop();
                if !picks.is_empty() {
                    let i = if picks.len() == 1 { 0 } else { self.rng.rand(0x3a) as usize % picks.len() };
                    self.weapon_hit(d, b, picks[i], 1);
                }
                let start = self.units_in_box(d, bx);
                for u in self.search.results(start) {
                    // 0x48b730: 0x64deb8 = 0.
                    self.splash_hit(d, b, u, false);
                }
                self.search.pop();
            }
            _ => todo_port!("0x48b770", "other weapon explosion types"),
        }
    }

    /// 0x48b680: lurker spines hit `u` at full damage, at most once per lurker attack
    /// (pairs remembered for 32 frames in 0x64dec8).
    fn lurker_spine_hit(&mut self, d: &Data, b: u32, u: u32) {
        let bu = &self.bullets.pool[b as usize];
        let (w, source, target, player) = (bu.weapon, bu.source, bu.target, bu.player);
        if u == source || (self.units[u as usize].player == player && u != target) {
            return;
        }
        if !self.weapon_can_target(d, w, u, u) || !self.splash_within(d, b, u, d.dat.weapons.inner_splash[w as usize] as u32) {
            return;
        }
        if source != NIL {
            let pair = [source + 1, u + 1];
            if self.lurker_hits.iter().flat_map(|r| r.chunks(2)).any(|p| p == pair) {
                return;
            }
            if self.lurker_hits_count != 0x10 {
                let i = self.lurker_hits_count as usize * 2;
                let row = &mut self.lurker_hits[self.lurker_hits_frame as usize];
                row[i] = pair[0];
                row[i + 1] = pair[1];
                self.lurker_hits_count += 1;
            }
        }
        self.weapon_hit(d, b, u, 1);
    }

    /// 0x48b570: splash damage to `u` by bullet `b`: full within the inner radius
    /// (when `inner`), half within the medium one, a quarter within the outer one.
    fn splash_hit(&mut self, d: &Data, b: u32, u: u32, inner: bool) {
        let bu = &self.bullets.pool[b as usize];
        let (w, source, target, player) = (bu.weapon, bu.source, bu.target, bu.player);
        let wi = w as usize;
        if u == source && w != 0x54 {
            return;
        }
        let et = d.dat.weapons.explosion_type[wi];
        if (et == 3 || et == 0x18) && self.units[u as usize].player == player && u != target {
            return;
        }
        if !self.weapon_can_target(d, w, u, u) || !self.splash_within(d, b, u, d.dat.weapons.outer_splash[wi] as u32) {
            return;
        }
        if w == 0x54 {
            if self.units[u as usize].storm_timer != 0 {
                return;
            }
            self.units[u as usize].storm_timer = 1;
        }
        if inner && self.splash_within(d, b, u, d.dat.weapons.inner_splash[wi] as u32) {
            self.weapon_hit(d, b, u, 1);
            return;
        }
        let un = &self.units[u as usize];
        if un.status & 0x10 == 0 && (inner || (u != target && un.unit_type != 0x49)) {
            if self.splash_within(d, b, u, d.dat.weapons.medium_splash[wi] as u32) {
                self.weapon_hit(d, b, u, 2);
            } else {
                self.weapon_hit(d, b, u, 4);
            }
        }
    }

    /// 0x48b150: collect `u` (0x64eee0) if the air splash may pick it; the target inside
    /// the inner radius ends the search as the only pick.
    fn air_splash_candidate(&self, d: &Data, b: u32, u: u32, picks: &mut Vec<u32>) -> bool {
        let bu = &self.bullets.pool[b as usize];
        let w = bu.weapon as usize;
        if u == bu.source || (self.units[u as usize].player == bu.player && u != bu.target) {
            return false;
        }
        if !self.weapon_can_target(d, bu.weapon, u, u) || !self.splash_within(d, b, u, d.dat.weapons.outer_splash[w] as u32) {
            return false;
        }
        if self.splash_within(d, b, u, d.dat.weapons.inner_splash[w] as u32) && u == bu.target {
            picks.clear();
            picks.push(u);
            return true;
        }
        picks.push(u);
        false
    }

    /// 0x48adb0: the bullet is within `r` of `u`'s box.
    fn splash_within(&self, d: &Data, b: u32, u: u32, r: u32) -> bool {
        let us = &self.sprites[self.units[u as usize].sprite as usize];
        let bs = &self.sprites[self.bullets.pool[b as usize].sprite as usize];
        let dim = d.dat.units.dimensions[self.units[u as usize].unit_type as usize];
        let (l, t, rr, bt) = (dim as i16, (dim >> 16) as i16, (dim >> 32) as i16, (dim >> 48) as i16);
        let (bx, by) = (bs.x as i32, bs.y as i32);
        let mut dx = bx - us.x.wrapping_add(rr).wrapping_add(1) as i32;
        if dx < 0 {
            dx = us.x.wrapping_sub(l) as i32 - bx;
            if dx < 0 {
                dx = 0;
            }
        }
        let mut dy = by - bt.wrapping_add(us.y).wrapping_add(1) as i32;
        if dy < 0 {
            dy = us.y.wrapping_sub(t) as i32 - by;
            if dy < 0 {
                dy = 0;
            }
        }
        super::path::approx_distance(0, dx, 0, dy) <= r
    }

    /// 0x475ce0: weapon `w` may hit `t` (weapons.dat target flags); `owner` for the
    /// "own units only" flag.
    pub(super) fn weapon_can_target(&self, d: &Data, w: u8, t: u32, owner: u32) -> bool {
        if w >= 0x82 {
            return false;
        }
        let f = d.dat.weapons.target_flags[w as usize];
        if t == NIL {
            return f & 0x40 != 0;
        }
        let tu = &self.units[t as usize];
        if tu.status & 0x400_0000 != 0 {
            return false;
        }
        let uf = d.dat.units.flags[tu.unit_type as usize];
        let air_or_ground = if tu.status & 4 != 0 { 1 } else { 2 };
        f & air_or_ground != 0
            && (f & 4 == 0 || uf & 0x4000_0000 != 0)
            && (f & 0x20 == 0 || uf & 0x4000 == 0)
            && (f & 8 == 0 || uf & 0x1_0000 != 0)
            && (f & 0x10 == 0 || uf & 1 == 0)
            && (f & 0x80 == 0 || uf & 0x4001_0000 != 0)
            && (f & 0x100 == 0 || self.units[owner as usize].player == tu.player)
    }

    /// 0x479730: a hallucination's hit does no damage, but the target still notices
    /// it (and its shields flash).
    pub(super) fn hallucination_hit(&mut self, d: &Data, t: u32, source: u32, w: u8) {
        let tu = &self.units[t as usize];
        if tu.hp == 0 || tu.status & 0x400_0000 != 0 {
            return;
        }
        if source != NIL {
            self.attacked_by(d, t, source, w != 0x22);
            if w != 0x22 && self.units[t as usize].player != self.units[source as usize].player {
                self.under_attack_alert(t);
            }
        }
        let tu = &self.units[t as usize];
        if d.dat.units.shield_enable[tu.unit_type as usize] != 0 && tu.shields > 0xff && d.dat.weapons.damage_type[w as usize] != 0 {
            todo_port!("0x4e6140", "shield hit overlay");
        }
    }

    /// 0x479ae0: damage the bullet's target.
    pub(super) fn weapon_hit(&mut self, d: &Data, b: u32, target: u32, divisor: u8) {
        let bu = &self.bullets.pool[b as usize];
        if bu.flags & 2 != 0 {
            let (w, source) = (bu.weapon, bu.source);
            self.hallucination_hit(d, target, source, w);
            return;
        }
        let (w, player, dir, source) = (bu.weapon, bu.player, bu.fl.heading, bu.source);
        let dmg = self.weapon_damage(d, b, target);
        self.damage_unit(d, target, dmg, w, divisor as u32, dir, source, player);
    }

    /// 0x48acd0: weapon damage (fp8) with upgrades.
    fn weapon_damage(&self, d: &Data, b: u32, t: u32) -> u32 {
        let bu = &self.bullets.pool[b as usize];
        let w = bu.weapon as usize;
        let ws = &d.dat.weapons;
        if ws.explosion_type[w] == 5 {
            // Nuclear missile: two thirds of `t`'s max hp (+ shields), at least 500.
            let tu = &self.units[t as usize];
            let ty = tu.unit_type as usize;
            let mut hp = d.dat.units.hit_points[ty] as i32 >> 8;
            if hp == 0 {
                hp = (tu.hp + 0xff) >> 8;
                if hp == 0 {
                    hp = 1;
                }
            }
            if d.dat.units.shield_enable[ty] != 0 {
                hp += d.dat.units.shields[ty] as i32;
            }
            return ((hp * 2 / 3).max(500) as u32) << 8;
        }
        let up = ws.damage_upgrade[w] as usize;
        let level = if up < 0x2e {
            self.players.upgrade_sc[bu.player as usize][up]
        } else {
            self.players.upgrade_bw[bu.player as usize][up - 0x2e]
        } as u32;
        (ws.damage_bonus[w] as u32 * level + ws.damage[w] as u32) << 8
    }

    /// 0x479930: defense matrix, shields, armor and the damage type / size
    /// multiplier, then hp.
    #[allow(clippy::too_many_arguments)]
    pub fn damage_unit(&mut self, d: &Data, t: u32, mut dmg: u32, w: u8, divisor: u32, dir: u8, source: u32, player: u8) {
        let tu = &self.units[t as usize];
        if tu.hp == 0 || tu.status & 0x400_0000 != 0 {
            return;
        }
        todo_port!("0x479950", "no-damage game option 0x6d5a6c & 4");
        if tu.status & 0x4000_0000 != 0 {
            dmg = dmg.wrapping_mul(2);
        }
        let mut v = ((tu.acid_spore_count as u32) << 8).wrapping_add(dmg / divisor) as i32;
        if v < 0x80 {
            v = 0x80;
        }
        if tu.defense_matrix_damage != 0 {
            let absorbed = v.min(tu.defense_matrix_damage as i32);
            v -= absorbed;
            self.defense_matrix_hit(d, t, absorbed);
        }
        let tu = &self.units[t as usize];
        let ut = tu.unit_type as usize;
        let dtype = d.dat.weapons.damage_type[w as usize];
        let mut sh = 0;
        if d.dat.units.shield_enable[ut] != 0 && tu.shields >= 0x100 {
            if dtype != 4 {
                let plasma = (self.players.upgrade_sc[tu.player as usize][0xf] as i32) << 8;
                if plasma >= v {
                    v = 0x80;
                } else {
                    v -= plasma;
                }
            }
            sh = v.min(tu.shields);
            v -= sh;
        }
        if dtype != 4 {
            let armor = ((self.armor_level(d, t) as i32) + d.dat.units.armor[ut] as i32) << 8;
            v -= v.min(armor);
        }
        let mult = DAMAGE_MULT[dtype as usize * 5 + d.dat.units.size[ut] as usize];
        let mut hp_dmg = ((mult as u32).wrapping_mul(v as u32) >> 8) as i32;
        if sh == 0 && hp_dmg < 0x80 {
            hp_dmg = 0x80;
        }
        self.apply_damage(d, t, hp_dmg, source, player, w != 0x22);
        if sh != 0 {
            let tu = &mut self.units[t as usize];
            tu.shields -= sh;
            if dtype != 0 && tu.shields != 0 {
                let _ = dir;
                todo_port!("0x4e6140", "shield hit overlay");
            }
        }
        todo_port!("0x431800", "AI strength refresh after damage");
    }

    /// 0x453fc0: armor upgrade level (+2 for ultralisks with chitinous plating in BW).
    pub fn armor_level(&self, d: &Data, t: u32) -> u32 {
        let tu = &self.units[t as usize];
        let p = tu.player as usize;
        let up = d.dat.units.armor_upgrade[tu.unit_type as usize] as usize;
        let lvl = if up > 0x2d { self.players.upgrade_bw[p][up - 0x2e] } else { self.players.upgrade_sc[p][up] } as u32;
        let ultra = self.is_expansion
            && matches!(tu.unit_type, 0x27 | 0x30)
            && (d.dat.units.flags[tu.unit_type as usize] & 0x40 != 0 || self.players.upgrade_bw[p][6] != 0);
        (lvl + if ultra { 2 } else { 0 }) & 0xff
    }

    pub(super) fn apply_damage_pub(&mut self, d: &Data, t: u32, dmg: i32, source: u32, player: u8, notify: bool) {
        self.apply_damage(d, t, dmg, source, player, notify);
    }

    /// 0x4797b0: take `dmg` (fp8) hp, killing the unit at 0.
    fn apply_damage(&mut self, d: &Data, t: u32, dmg: i32, source: u32, player: u8, notify: bool) {
        if self.units[t as usize].hp == 0 {
            return;
        }
        if source != NIL {
            self.attacked_by(d, t, source, notify);
        }
        // Game option 0x6d5a6c & 4: only human players' attacks do damage.
        let dmg = if self.game_options & 4 != 0 && self.player_type(player) != 2 { 0 } else { dmg };
        let tu = &mut self.units[t as usize];
        if dmg < tu.hp {
            tu.hp -= dmg;
            todo_port!("0x431800", "AI unit strength after damage");
            let s = self.units[t as usize].sprite;
            let main = self.sprites[s as usize].main_image;
            if self.units[t as usize].status & 1 != 0 && main != NIL && d.has_damage_overlays(self.images[main as usize].image_id) {
                self.update_damage_overlays(d, t);
            }
            if source != NIL && self.units[source as usize].player != self.units[t as usize].player && notify {
                self.under_attack_alert(t);
            }
        } else {
            // Passengers of a (non-building) transport die with it.
            let tu = &self.units[t as usize];
            let tt = tu.unit_type;
            if tu.status & 0x4000_0000 == 0
                && (tt != 0x2a || self.players.upgrade_sc[tu.player as usize][0x18] != 0)
                && d.dat.units.space_provided[tt as usize] != 0
                && d.dat.units.flags[tt as usize] & 1 == 0
            {
                for i in 0..8 {
                    if let Some(c) = self.loaded_unit(t, i) {
                        if self.units[c as usize].order != 0 {
                            self.kill_unit(d, c);
                            self.count_death(d, c, player);
                        }
                    }
                }
            }
            self.units[t as usize].hp = 0;
            self.kill_unit(d, t);
            self.count_death(d, t, player);
            if source != NIL && self.alliances[self.units[source as usize].player as usize][self.alliance_owner_pub(t)] == 0 {
                // 0x4759c0
                let mut k = source;
                loop {
                    let ku = &mut self.units[k as usize];
                    if ku.kill_count != 0xff {
                        ku.kill_count += 1;
                    }
                    if ku.unit_type != 0x49 || ku.connected_unit == NIL {
                        break;
                    }
                    k = ku.connected_unit;
                }
            }
        }
    }
}

/// 0x515b88: damage multiplier (fp8) by [damage type * 5 + unit size].
const DAMAGE_MULT: [u32; 25] = [
    0, 0, 0, 0, 1, //
    0, 128, 192, 256, 2, //
    0, 256, 128, 64, 3, //
    0, 256, 256, 256, 4, //
    0, 256, 256, 256, 25296903,
];

/// State a bullet of weapon behavior `v` moves into from state 0.
fn behavior_state(v: u8) -> u8 {
    match v {
        0 => 1,
        1 => 2,
        3 => 4,
        7 => 3,
        8 => 6,
        _ => 1,
    }
}
