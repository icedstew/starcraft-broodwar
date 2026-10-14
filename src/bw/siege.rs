//! Siege tanks switching modes: the Siege / Unsiege commands and the Sieging (0x62) and
//! Unsieging (0x63) orders. Tank and turret both turn to their units.dat direction,
//! play the (un)siege animations and the tank changes unit type when they finish.

use super::list::NIL;
use super::{Data, State};

impl State {
    /// 0x4c1e80: Siege command: tanks in tank mode that may use Siege Mode (tech 5).
    pub fn command_siege(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            let un = &self.units[u as usize];
            if matches!(un.unit_type, 5 | 0x17) && un.order != 0x62 && self.can_use_tech(d, u, 5, player) == Ok(1) {
                self.command_order(d, u, 1, queued, 0x62, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// 0x4c1f10: Unsiege command.
    pub fn command_unsiege(&mut self, d: &Data, player: u8, sel: &[u32], queued: bool) {
        for u in self.live_selection(sel) {
            let un = &self.units[u as usize];
            if matches!(un.unit_type, 0x1e | 0x19) && un.order != 0x63 && self.can_use_tech(d, u, 5, player) == Ok(1) {
                self.command_order(d, u, 1, queued, 0x63, NIL, 0xe4, (0, 0));
            }
        }
    }

    /// 0x4011a0: aim `u` 16 pixels ahead in its units.dat direction.
    fn face_default_direction(&mut self, d: &Data, u: u32) {
        let dir = d.dat.units.direction[self.units[u as usize].unit_type as usize];
        let (vx, vy) = crate::game::flingy::direction_table()[(dir as usize * 8) & 0xff];
        let f = &mut self.units[u as usize].fl;
        let x = (((vx << 12) >> 8) + f.exact.0) >> 8;
        let y = (((vy << 12) >> 8) + f.exact.1) >> 8;
        let p = (x as i16, y as i16);
        if f.next_target_waypoint != p {
            f.next_target_waypoint = p;
        }
    }

    /// 0x474380: run `anim` on the images of `u`'s turret, as the turret.
    fn turret_anim(&mut self, d: &Data, u: u32, anim: u8) {
        let t = self.units[u as usize].subunit;
        self.iscript_unit = Some(t);
        let s = self.units[t as usize].sprite;
        self.sprite_images_run_anim(d, s, anim);
        self.iscript_unit = Some(u);
    }

    /// 0x464290: kill the grounded buildings (other than `u`, not 0x200000) under `u`;
    /// true when there was one (0x4e8830 with callback 0x463f90, which stops at the first).
    fn siege_crush(&mut self, d: &Data, u: u32) -> bool {
        let p = self.unit_sprite_pos(u);
        let b = self.unit_box_at(d, self.units[u as usize].unit_type, p.0, p.1);
        let start = self.units_in_box(d, b);
        let hit = self.search.results(start).into_iter().find(|&c| c != u && self.units[c as usize].status & 0x20_0002 == 2);
        self.search.pop();
        if let Some(c) = hit {
            self.kill_unit(d, c);
        }
        hit.is_some()
    }

    /// 0x463770: queue the idle order if nothing is queued, then move on.
    fn mode_switch_done(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_queue_head == NIL {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, o, false, (0, 0), NIL, 0xe4);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x464bd0: Sieging.
    pub fn order_sieging(&mut self, d: &Data, u: u32) {
        let mut st = self.units[u as usize].order_state;
        if st == 0 {
            if !matches!(self.units[u as usize].unit_type, 5 | 0x17) {
                self.order_done_or_next(d, u);
                return;
            }
            if self.units[u as usize].fl.flags & 2 != 0 {
                self.unit_halt_facing(d, u);
            }
            self.units[u as usize].order_state = 1;
            st = 1;
        }
        let t = self.units[u as usize].subunit;
        if st == 1 {
            if self.units[u as usize].fl.flags & 2 != 0 || self.units[t as usize].fl.flags & 2 != 0 || self.units[t as usize].status & 0x2000 != 0 {
                return;
            }
            self.face_default_direction(d, u);
            self.order_with_target(d, t, 0x18, u);
            self.face_default_direction(d, t);
            self.turret_anim(d, u, 0xd);
            self.units[u as usize].order_state = 2;
            if self.siege_crush(d, u) {
                return;
            }
            st = 2;
        }
        if st == 2 {
            if !self.facing_target(u) || !self.facing_target(t) {
                return;
            }
            let nt = match self.units[u as usize].unit_type {
                5 => 0x1e,
                0x17 => 0x19,
                x => x,
            };
            self.transform_unit(d, u, nt);
            self.units[u as usize].order_state = 3;
            st = 3;
        }
        if st == 3 && self.units[u as usize].order_signal & 1 != 0 {
            self.units[u as usize].order_signal &= !1;
            let h = self.units[u as usize].order_queue_head;
            let t = self.units[u as usize].subunit;
            if h != NIL && self.orders[h as usize].order != 0x64 {
                let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                self.order_now(d, u, o);
                self.order_next_or_idle(d, u);
                self.mode_switch_done(d, t);
                return;
            }
            self.mode_switch_done(d, u);
            self.mode_switch_done(d, t);
        }
    }

    /// 0x464ae0: Unsieging.
    pub fn order_unsieging(&mut self, d: &Data, u: u32) {
        let mut st = self.units[u as usize].order_state;
        let t = self.units[u as usize].subunit;
        if st == 0 {
            if !matches!(self.units[u as usize].unit_type, 0x1e | 0x19) {
                self.order_done_or_next(d, u);
                return;
            }
            if self.units[t as usize].status & 0x2000 != 0 {
                return;
            }
            self.order_with_target(d, t, 0x18, u);
            self.face_default_direction(d, t);
            self.turret_anim(d, u, 0xd);
            self.units[u as usize].order_state = 2;
            st = 2;
        }
        if st == 2 {
            if !self.facing_target(t) {
                return;
            }
            let s = self.units[u as usize].sprite;
            self.sprite_images_run_anim(d, s, 0xe);
            self.turret_anim(d, u, 0xe);
            self.units[u as usize].order_signal &= !1;
            self.units[u as usize].order_state = 3;
            st = 3;
        }
        if st == 3 && self.units[u as usize].order_signal & 1 != 0 {
            self.units[u as usize].order_signal &= !1;
            let nt = match self.units[u as usize].unit_type {
                0x19 => 0x17,
                0x1e => 5,
                x => x,
            };
            self.transform_unit(d, u, nt);
            self.mode_switch_done(d, u);
            let t = self.units[u as usize].subunit;
            self.mode_switch_done(d, t);
        }
    }
}
