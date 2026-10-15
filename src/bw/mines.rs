//! Spider mines: the vulture's PlaceMine order (0x464fd0) and the mine's own
//! VultureMine order (0x463df0).

use super::list::NIL;
use super::{Data, State};

const MINE: u16 = 0xd;

impl State {
    /// 0x464fd0: PlaceMine: go to the spot (or unit) and lay a mine within 0x14.
    pub(super) fn order_place_mine(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            let t = self.units[u as usize].order_target_unit;
            if t == NIL {
                let p = self.units[u as usize].order_target_pos;
                let r = self.set_move_target_xy(d, u, p);
                let f = &mut self.units[u as usize].fl;
                if f.next_target_waypoint != p {
                    f.next_target_waypoint = p;
                }
                if r {
                    self.units[u as usize].order_state = 1;
                }
            } else {
                let r = self.move_to_unit(d, u, t);
                let p = self.unit_sprite_pos(t);
                let f = &mut self.units[u as usize].fl;
                if f.next_target_waypoint != p {
                    f.next_target_waypoint = p;
                }
                if r {
                    self.units[u as usize].order_target_pos = p;
                    self.units[u as usize].order_state = 1;
                }
            }
            return;
        }
        let player = self.units[u as usize].player;
        let stuck = {
            let un = &self.units[u as usize];
            un.fl.move_target == self.unit_sprite_pos(u) && un.status & 0x8_0000 != 0
        };
        if self.can_use_tech(d, u, 3, player).is_err() || stuck {
            if self.units[u as usize].order_queue_head != NIL {
                self.units[u as usize].order_flags |= 1;
                self.prepare_next_order(d, u);
            } else if self.units[u as usize].ai != NIL {
                self.order_now(d, u, 0x9c);
            } else {
                let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                self.order_now(d, u, o);
            }
            return;
        }
        let p = self.units[u as usize].order_target_pos;
        let f = &self.units[u as usize].fl;
        let dist = super::path::approx_distance(f.exact.0, (p.0 as i32) << 8, f.exact.1, (p.1 as i32) << 8);
        if dist & !0xff < 0x1401 {
            self.unit_halt_facing(d, u);
            let mut q = p;
            self.clamp_unit_pos(d, MINE, &mut q);
            let Some(m) = self.create_unit(d, MINE, q.0, q.1, player) else {
                self.show_last_error(player);
                self.order_done_or_next(d, u);
                return;
            };
            self.finish_unit_pre(d, m);
            self.finish_unit_post(d, m);
            let h = self.units[u as usize].fl.heading;
            self.set_heading_now(d, m, h);
            let n = self.units[u as usize].u8_at(0xc0);
            self.units[u as usize].set_u8_at(0xc0, n.wrapping_sub(1));
            self.order_done_or_next(d, u);
            return;
        }
        if self.units[u as usize].ai != NIL && self.unit_at_move_target(u) != 0 {
            self.order_done_or_next(d, u);
        }
    }

    /// 0x441270: the nearest enemy ground unit within the mine's acquisition range
    /// (filter 0x440ec0: not hovering, not a building, not invincible).
    fn mine_find_target(&mut self, d: &Data, u: u32) -> u32 {
        if self.units[u as usize].status & 0x8000 != 0 {
            return NIL;
        }
        let r = ((self.acquisition_range(u) as u8 as i32) << 5) as i16;
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(r), y.wrapping_sub(r), x.wrapping_add(r), y.wrapping_add(r)];
        self.nearest_unit_to(u, b, &mut |st: &mut State, c: u32| {
            let cu = &st.units[c as usize];
            st.is_enemy(u, c)
                && cu.status & 0x400_0000 == 0
                && cu.status & 6 == 0
                && d.dat.units.movement_flags[cu.unit_type as usize] != 0xc1
        })
    }

    /// 0x463df0: VultureMine: arm, burrow, wait for prey, come up and chase it.
    pub(super) fn order_vulture_mine(&mut self, d: &Data, u: u32) {
        let mut st = self.units[u as usize].order_state;
        if st == 5 {
            self.come_up(d, u);
            let t = self.units[u as usize].order_target_unit;
            if t != NIL && self.box_distance_within(d, u, 0x240, t) {
                self.intercept_move(u, t);
                if self.box_distance_within(d, u, 0x1e, t) {
                    let p = self.unit_sprite_pos(u);
                    let un = &mut self.units[u as usize];
                    un.order_target_unit = NIL;
                    un.order_target_pos = p;
                    let s = un.sprite;
                    self.sprite_images_run_anim(d, s, 0xd);
                    self.units[u as usize].order_state = 6;
                    st = 6;
                } else if self.unit_at_move_target(u) != 2 {
                    return;
                }
            }
            if st == 5 {
                self.unit_halt(d, u);
                self.units[u as usize].order_state = 1;
                return;
            }
        }
        if st == 6 {
            if self.units[u as usize].order_signal & 1 != 0 {
                self.kill_unit(d, u);
            }
            return;
        }
        if st == 0 {
            self.units[u as usize].ground_cooldown = 0x3c;
            self.units[u as usize].order_state = 1;
            st = 1;
        }
        if st == 1 {
            if self.units[u as usize].ground_cooldown != 0 {
                return;
            }
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0x19);
            self.units[u as usize].status |= 0x20_0000;
            self.units[u as usize].order_state = 2;
            st = 2;
        }
        if st == 2 {
            if self.units[u as usize].order_signal & 4 == 0 {
                return;
            }
            self.units[u as usize].order_signal &= !4;
            if self.units[u as usize].status & 0x4000_0000 == 0 {
                self.go_underground(u);
                self.mine_cloak(d, u);
            }
            self.units[u as usize].order_state = 3;
            st = 3;
        }
        if st == 3 {
            let t = self.mine_find_target(d, u);
            if t == NIL {
                return;
            }
            self.units[u as usize].order_target_unit = t;
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0x1a);
            self.sprites[s as usize].flags &= !0x40;
            self.set_secondary(u, 0x17);
            self.units[u as usize].order_state = 4;
            st = 4;
        }
        if st == 4 {
            if self.units[u as usize].order_signal & 4 == 0 {
                return;
            }
            self.units[u as usize].order_signal &= !4;
            let s = self.units[u as usize].sprite;
            self.sprites[s as usize].elevation = d.dat.units.elevation[self.units[u as usize].unit_type as usize];
            self.units[u as usize].status &= !0x20_0000;
            let t = self.units[u as usize].order_target_unit;
            if t == NIL {
                self.units[u as usize].order_state = 1;
                return;
            }
            self.intercept_move(u, t);
            self.units[u as usize].order_state = 5;
        }
    }
}
