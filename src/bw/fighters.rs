//! Carrier interceptors and reaver scarabs: the Train Fighter command and the
//! TrainFighter secondary order (0x3f), with the hangar list (+0xc0 / +0xc4 / +0xc8).
//!
//! Hangar fields: the carrier / reaver keeps the docked fighters in a list at +0xc0
//! (count at +0xc8); a fighter links to its parent at +0xc0, next / previous at +0xc4 /
//! +0xc8, and +0xcc = 0 while docked.

use super::list::NIL;
use super::{Data, State};

impl State {
    /// 0x4c1800: Train Fighter command: carriers queue an interceptor (0x49), reavers a
    /// scarab (0x55).
    pub fn command_train_fighter(&mut self, d: &Data, player: u8, sel: &[u32]) {
        for u in self.live_selection(sel) {
            let t = match self.units[u as usize].unit_type {
                0x48 | 0x52 => 0x49,
                0x53 | 0x51 => 0x55,
                _ => continue,
            };
            if self.can_make_unit(d, u, t, player) != Ok(1) || !self.queue_build(d, u, t) {
                continue;
            }
            let un = &mut self.units[u as usize];
            let fighter_parent = super::unit::has_hangar(un.unit_type);
            if !fighter_parent || un.secondary_order_state != 2 {
                un.secondary_order = 0x3f;
                un.secondary_order_pos = (0, 0);
                un.current_build_unit = NIL;
                un.secondary_order_state = 0;
            }
        }
        // 0x68c1e8 etc.: button refresh (UI).
    }

    /// 0x466790: TrainFighter: build the queued interceptors / scarabs one at a time and
    /// dock them; with nothing queued, carriers repair docked interceptors.
    pub(super) fn secondary_train_fighter(&mut self, d: &Data, u: u32) {
        let ut = self.units[u as usize].unit_type;
        if !super::unit::has_hangar(ut) {
            return;
        }
        match self.units[u as usize].secondary_order_state {
            st @ (0 | 1) => {
                let un = &self.units[u as usize];
                let t = un.build_queue[un.build_queue_slot as usize % 5];
                if t == 0xe4 {
                    let un = &mut self.units[u as usize];
                    un.secondary_order_state = 3;
                    un.current_build_unit = NIL;
                    return;
                }
                let f = self.create_trainee(d, u, t, st == 0);
                self.units[u as usize].current_build_unit = f;
                if f == NIL {
                    self.units[u as usize].secondary_order_state = 1;
                    return;
                }
                self.units[f as usize].set_ptr_at(0xc0, u);
                self.units[u as usize].secondary_order_state = 2;
            }
            2 => {
                let f = self.units[u as usize].current_build_unit;
                if f != NIL {
                    let gain = self.build_hp_gain(f);
                    self.build_progress(d, f, gain, false);
                    if self.units[f as usize].status & 1 == 0 {
                        return;
                    }
                    self.dock_fighter(d, u, f);
                    let un = &mut self.units[u as usize];
                    let slot = un.build_queue_slot as usize % 5;
                    un.build_queue[slot] = 0xe4;
                    un.build_queue_slot += 1;
                    if un.build_queue_slot >= 5 {
                        un.build_queue_slot = 0;
                    }
                }
                // 0x4c36c0: button refresh (UI).
                let un = &mut self.units[u as usize];
                un.current_build_unit = NIL;
                un.secondary_order_state = 0;
            }
            3 => {
                if matches!(ut, 0x53 | 0x51) {
                    self.units[u as usize].secondary_order_state = 4;
                    return;
                }
                let max = d.dat.units.hit_points[0x49] as i32;
                let mut f = self.units[u as usize].ptr_at(0xc0);
                while f != NIL {
                    if self.units[f as usize].hp < max {
                        let hp = self.units[f as usize].hp + 0x80;
                        return self.set_unit_hp(d, f, hp);
                    }
                    f = self.units[f as usize].ptr_at(0xc4);
                }
            }
            _ => {}
        }
    }

    /// 0x466300: fighter `f` goes into `parent`'s hangar (off the map, first in the list).
    pub(super) fn dock_fighter(&mut self, d: &Data, parent: u32, f: u32) {
        self.hide_unit(d, f);
        let head = self.units[parent as usize].ptr_at(0xc0);
        self.units[f as usize].set_ptr_at(0xc4, head);
        self.units[f as usize].set_ptr_at(0xc8, NIL);
        if head != NIL {
            self.units[head as usize].set_ptr_at(0xc8, f);
        }
        self.units[parent as usize].set_ptr_at(0xc0, f);
        let n = self.units[parent as usize].u8_at(0xc8);
        self.units[parent as usize].set_u8_at(0xc8, n.wrapping_add(1));
        self.units[f as usize].set_u8_at(0xcc, 0);
    }
}
