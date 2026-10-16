//! Terran addons: the build command for an addon, the PlaceAddon order that starts it next
//! to the building, the building's BuildAddon secondary order and attaching the finished
//! addon. The addon type being placed is kept in the building's +0xc4.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// C's (v + ((v >> 31) & 0x1f)) >> 5: pixels to tiles, rounding toward zero.
fn tile(v: i32) -> i16 {
    ((v + ((v >> 31) & 0x1f)) >> 5) as i16
}

fn addon_pos(d: &Data, t: u16) -> (i16, i16) {
    let v = d.dat.units.addon_position.get(t as usize).copied().unwrap_or(0);
    (v as u16 as i16, (v >> 16) as u16 as i16)
}

impl State {
    /// 0x48e0a0: build command for addon `t` at tile (tx, ty): the addon's spot and the
    /// building's spot it implies must both be free (0x473fb0 twice, errors 0x48d930).
    pub fn command_build_addon(&mut self, d: &Data, u: u32, order: u8, tx: i16, ty: i16, t: u16) {
        let p = self.units[u as usize].player;
        let own = self.units[u as usize].unit_type;
        let mut r = self.placement_check(d, u, p, tx, ty, t, 1, false, false, false);
        let (mut bx, mut by) = (0i16, 0i16);
        if r == 0 {
            let (ax, ay) = addon_pos(d, t);
            bx = tx - tile(ax as i32);
            by = ty - tile(ay as i32);
            r = self.placement_check(d, u, p, bx, by, own, 0, false, false, false);
        }
        if r != 0 {
            self.placement_error(p, r);
            return;
        }
        let (pw, ph) = super::build::placement(d, own);
        let pos = ((pw / 2) as i16 + (bx << 5), (ph / 2) as i16 + (by << 5));
        self.build_order(d, u, order, pos, t);
    }

    /// 0x4e6880: PlaceAddon. The building moves (lifts, flies, lands) to the order
    /// position if needed, then pays for and creates the addon next to itself.
    pub fn order_place_addon(&mut self, d: &Data, u: u32) {
        let at = self.units[u as usize].u16_at(0xc4);
        if self.units[u as usize].order_state == 0 {
            if d.dat.units.flags[at as usize] & 2 == 0 {
                return;
            }
            let p = self.units[u as usize].order_target_pos;
            let next = if self.units[u as usize].status & 2 == 0 {
                0x47
            } else {
                // 0x401240: exactly at the order position.
                let e = self.units[u as usize].fl.exact;
                let dist = super::path::approx_distance(e.0, (p.0 as i32) << 8, e.1, (p.1 as i32) << 8);
                if dist >> 8 == 0 {
                    if !self.set_move_target_xy(d, u, p) {
                        return;
                    }
                    let f = &mut self.units[u as usize].fl;
                    if f.next_target_waypoint != p {
                        f.next_target_waypoint = p;
                    }
                    self.units[u as usize].order_state = 1;
                    0
                } else {
                    0x48
                }
            };
            if next != 0 {
                // Land / lift there, then place the addon again.
                self.order_at(d, u, next, p);
                self.queue_order_at(d, u, 0x24, p, false);
                return;
            }
        }
        if self.units[u as usize].order_state != 1 {
            return;
        }
        if self.unit_at_move_target(u) == 0 {
            return;
        }
        let own = self.units[u as usize].unit_type;
        let (ow, oh) = super::build::placement(d, own);
        let (aw, ah) = super::build::placement(d, at);
        let (ax, ay) = addon_pos(d, at);
        {
            let un = &mut self.units[u as usize];
            let p = un.order_target_pos;
            un.order_target_pos = (
                p.0 - (ow / 2) as i16 + (aw / 2) as i16 + ax,
                p.1 - (oh / 2) as i16 + (ah / 2) as i16 + ay,
            );
        }
        let player = self.units[u as usize].player;
        // 0x42cf70: supply (addons need none).
        if !self.supply_ok_show(d, player as usize, at) {
            self.order_done_or_next(d, u);
            return;
        }
        let (m, g) = (d.dat.units.mineral_cost[at as usize] as i32, d.dat.units.gas_cost[at as usize] as i32);
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[player as usize] < m {
            self.local_error(player, 0x93 + race, 0x352);
            self.order_done_or_next(d, u);
            return;
        }
        if self.players.gas[player as usize] < g {
            self.local_error(player, 0x96 + race, 0x353);
            self.order_done_or_next(d, u);
            return;
        }
        let p = self.units[u as usize].order_target_pos;
        let half = |n: i32| {
            let t = tile(n) as i32;
            ((t - (t >> 31)) >> 1) as i16
        };
        let tx = tile(p.0 as i32) - half(aw);
        let ty = tile(p.1 as i32) - half(ah);
        if self.placement_check(d, u, player, tx, ty, at, 0, false, true, false) != 0 {
            self.advisor_error(d, u, 0x358);
            self.order_done_or_next(d, u);
            return;
        }
        let Some(a) = self.create_unit(d, at, p.0, p.1, player) else {
            self.show_last_error(player);
            return;
        };
        self.spend(player as usize, m, g);
        let idle = d.dat.units.ai_return_to_idle[own as usize];
        self.start_order_now(d, u, idle);
        self.set_secondary_order(u, 0x25);
        self.units[u as usize].current_build_unit = a;
        self.start_order_now(d, a, 0x2c);
        if self.units[u as usize].ai != NIL {
            todo_port!("0x435770", "AI: the new addon joins the town");
        }
        self.units[a as usize].connected_unit = u;
    }

    /// 0x4e6790: BuildAddon (secondary order): build the addon in `current_build_unit`,
    /// cancelling it if the building lifted off, attaching it when done.
    pub fn secondary_build_addon(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            return;
        }
        let a = un.current_build_unit;
        if a != NIL {
            if self.units[u as usize].status & 2 == 0 {
                if self.units[a as usize].status & 1 == 0 {
                    self.cancel_unit(d, a);
                }
            } else {
                let mut gain = self.units[a as usize].hp_gain as i32;
                if self.game_options & 2 != 0 {
                    gain = (gain << 4) & 0xffff;
                }
                self.build_progress(d, a, gain, false);
                if self.units[a as usize].status & 1 == 0 {
                    return;
                }
                self.construction_sound(d, u);
                self.attach_addon(d, u, a);
            }
        }
        self.units[u as usize].set_u16_at(0xc4, 0xe4);
        if self.units[u as usize].ai != NIL {
            todo_port!("0x4e6790", "AI building idles after its addon (0x475310)");
        }
        self.set_secondary_order(u, 0x17);
    }

    /// 0x463dd0: a terran building that lands or appears takes a neutral addon lined
    /// up with it (0x4404a0).
    pub(super) fn take_adjacent_addon(&mut self, d: &Data, b: u32) {
        if let Some(a) = self.find_adjacent_addon(d, b) {
            self.attach_addon(d, b, a);
        }
    }

    /// 0x4404a0: a neutral addon of `b`'s type at the addon position next to `b`
    /// (0x4402f0 over the units in `b`'s placement box grown by 0x20 each way).
    fn find_adjacent_addon(&mut self, d: &Data, b: u32) -> Option<u32> {
        let bt = self.units[b as usize].unit_type;
        let (pw, ph) = (d.dat.units.placement_box[bt as usize] as i16, (d.dat.units.placement_box[bt as usize] >> 16) as i16);
        let (bx, by) = self.unit_sprite_pos(b);
        let l = (bx as u16).wrapping_sub((pw / 2) as u16).wrapping_sub(0x20);
        let t = (by as u16).wrapping_sub((ph / 2) as u16).wrapping_sub(0x20);
        let r = (pw as u16).wrapping_add(l).wrapping_add(0x40);
        let bt_ = (ph as u16).wrapping_add(t).wrapping_add(0x40);
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, [l as i16, t as i16, r as i16, bt_ as i16]);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let tile = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
        found.into_iter().find(|&c| {
            let cu = &self.units[c as usize];
            let ct = cu.unit_type;
            if d.dat.units.flags[ct as usize] & 2 == 0 || cu.player != 0xb || super::requirements::addon_parent_type(ct) != bt {
                return false;
            }
            let (cx, cy) = self.unit_sprite_pos(c);
            let cbox = d.dat.units.placement_box[ct as usize];
            let (cpw, cph) = (cbox as i16 as i32, (cbox >> 16) as i16 as i32);
            let ap = d.dat.units.addon_position[ct as usize];
            let (ax, ay) = (ap as i16 as i32, (ap >> 16) as i16 as i32);
            let cxl = (cx.wrapping_sub((cpw / 2) as i16)) as i32 - ax;
            let bxl = (bx.wrapping_sub((pw / 2) as i16)) as i32;
            let cyl = (cy.wrapping_sub((cph / 2) as i16)) as i32 - ay;
            let byl = (by.wrapping_sub((ph / 2) as i16)) as i32;
            tile(bxl) == tile(cxl) && tile(byl) == tile(cyl)
        })
    }

    /// 0x463d50: building `b` takes addon `a`: the addon becomes its owner's and plays
    /// its landing ("attach") animation.
    pub fn attach_addon(&mut self, d: &Data, b: u32, a: u32) {
        self.units[b as usize].set_ptr_at(0xc0, a);
        let p = self.units[b as usize].player;
        self.change_owner(d, a, p, false);
        self.set_unit_colors(d, a, p);
        let s = self.units[a as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x11);
        if self.units[b as usize].unit_type == 0x6c && self.units[b as usize].ptr_at(0xd0) != NIL {
            self.change_owner(d, a, p, false);
        }
    }
}
