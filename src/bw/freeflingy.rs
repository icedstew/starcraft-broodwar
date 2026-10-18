//! Free flingies (0x63fee0, a pool of one; active list 0x63ff34, free list 0x63ff38):
//! sprites that fly off on their own, made by iscript `uflunstable` (op 0x12).

use super::list::{self, Link, Node, NIL};
use super::{Data, State};
use crate::game::flingy::Flingy;

#[derive(Clone, Debug)]
pub struct FreeFlingy {
    pub link: Link,
    /// +0x8
    pub hp: i32,
    /// +0xc
    pub sprite: u32,
    pub fl: Flingy,
}

impl Default for FreeFlingy {
    fn default() -> FreeFlingy {
        FreeFlingy { link: Link::default(), hp: 0, sprite: NIL, fl: Flingy::default() }
    }
}

impl Node for FreeFlingy {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}

impl State {
    /// 0x496460 / 0x496360: take a free flingy of type `id` at (x, y) (no owner,
    /// facing 0) with its sprite; NIL when the pool is empty or there is no sprite.
    fn create_free_flingy(&mut self, d: &Data, id: u16, x: i16, y: i16) -> u32 {
        let n = self.ff_free.head;
        if n == NIL {
            return NIL;
        }
        let i = id as usize;
        let fd = &d.dat.flingy;
        let f = &mut self.free_flingies[n as usize].fl;
        *f = Flingy::default();
        f.flingy_id = id;
        f.flags = 0;
        f.next_speed = 0;
        f.top_speed = fd.top_speed[i] as i32;
        f.acceleration = fd.acceleration[i];
        f.turn_rate = fd.turn_rate[i];
        f.movement_type = fd.movement_control[i];
        f.pos = (x, y);
        f.exact = ((x as i32) << 8, (y as i32) << 8);
        f.move_target = (x, y);
        f.flags = 1;
        f.next_waypoint = (x, y);
        f.next_target_waypoint = (x, y);
        f.heading = 0;
        f.velocity_dir_prev = 0;
        self.free_flingies[n as usize].hp = 1;
        let Some(s) = self.create_sprite(d, fd.sprite[i], x, y, 0) else {
            self.free_flingies[n as usize].sprite = NIL;
            return NIL;
        };
        self.free_flingies[n as usize].sprite = s;
        self.sprite_images_set_direction(d, s, 0);
        let mut fl = self.ff_free;
        list::remove(&mut self.free_flingies, n, &mut fl);
        self.ff_free = fl;
        let mut al = self.ff_active;
        list::insert_after_head(&mut self.free_flingies, n, &mut al);
        self.ff_active = al;
        n
    }

    /// iscript `uflunstable`: a free flingy of type `id` at (x, y) flies off in a
    /// random direction (`free_flingy_random_flight`).
    pub(super) fn spawn_unstable_flingy(&mut self, d: &Data, id: u16, x: i16, y: i16) {
        let n = self.create_free_flingy(d, id, x, y);
        if n == NIL {
            return;
        }
        self.free_flingy_random_flight(n);
    }

    /// 0x4955f0 (EAX = the free flingy `n`): a random direction (site 2); it heads for
    /// the point one table step away.
    fn free_flingy_random_flight(&mut self, n: u32) {
        let r = self.rng.rand(2) as u8;
        let s = self.free_flingies[n as usize].sprite;
        let (sx, sy) = (self.sprites[s as usize].x as i32, self.sprites[s as usize].y as i32);
        let (vx, vy) = crate::game::flingy::direction_table()[r as usize];
        let p = ((((vx << 8) >> 8) + sx) as i16, (((vy << 8) >> 8) + sy) as i16);
        let f = &mut self.free_flingies[n as usize].fl;
        f.velocity_dir_prev = r;
        f.heading = r;
        if f.move_target != p {
            f.move_target = p;
            f.flags = (f.flags & !4) | 1;
            f.next_waypoint = p;
        }
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
        self.sprite_fog_visibility(s);
    }

    /// 0x496330: move the free flingies; they die off the map or at their target.
    pub fn update_free_flingies(&mut self, d: &Data) {
        let mut n = self.ff_active.head;
        while n != NIL {
            let next = self.free_flingies[n as usize].link.next;
            self.update_free_flingy(d, n);
            n = next;
        }
    }

    /// 0x4961f0
    fn update_free_flingy(&mut self, d: &Data, n: u32) {
        let s = self.free_flingies[n as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.free_flingies[n as usize].sprite = NIL;
            }
        }
        let s = self.free_flingies[n as usize].sprite;
        if s == NIL {
            let mut al = self.ff_active;
            list::remove(&mut self.free_flingies, n, &mut al);
            self.ff_active = al;
            let mut fl = self.ff_free;
            list::insert_after_head(&mut self.free_flingies, n, &mut fl);
            self.ff_free = fl;
            return;
        }
        let mut m = self.mv;
        super::flingy::predict(&mut self.free_flingies[n as usize].fl, d, &mut m);
        self.mv = m;
        // 0x496030
        let m = self.mv;
        let f = &mut self.free_flingies[n as usize].fl;
        self.mv.old_pos = f.pos;
        f.velocity_dir_prev = f.velocity_dir;
        f.pos = m.new_pos;
        f.flags = m.new_flags;
        f.exact = m.new_exact;
        f.next_speed = f.speed;
        self.move_sprite(s, m.new_pos.0, m.new_pos.1);
        super::flingy::turn_heading(&mut self.free_flingies[n as usize].fl);
        let h = self.free_flingies[n as usize].fl.heading;
        self.sprite_images_set_direction(d, s, h);
        if m.stopping {
            if self.sprites[s as usize].flags & 0x80 == 0 {
                self.sprite_images_run_anim(d, s, 0xc);
            }
        } else if m.starting {
            self.sprite_images_run_anim(d, s, 0xb);
        }
        let sp = &self.sprites[s as usize];
        let f = &self.free_flingies[n as usize].fl;
        if sp.x as u16 as u32 >= self.map_px.0 as u32 || sp.y as u16 as u32 >= self.map_px.1 as u32 || f.move_target == f.pos {
            self.sprite_images_run_anim(d, s, 1);
        }
    }
}
