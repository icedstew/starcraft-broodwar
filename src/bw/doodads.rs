//! Map doodads and traps (UMS maps): doors, hidden guns, traps, enable / disable.

use super::list::NIL;
use super::{Data, State};

impl State {
    fn doodad_idle(&mut self, d: &Data, u: u32) {
        let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.order_now(d, u, o);
    }

    /// Queue the idle order and start it (0x474810 with clear, then 0x475000).
    fn doodad_queue_idle(&mut self, d: &Data, u: u32) {
        self.units[u as usize].order_flags |= 1;
        let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
        self.prepare_next_order(d, u);
    }

    /// 0x47bcd0: OpenDoor: once its animation signals, the door lets units through.
    pub(super) fn order_open_door(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_signal & 1 == 0 {
            return;
        }
        self.units[u as usize].order_signal &= !1;
        self.units[u as usize].status |= 0x20_0000;
        let s = self.units[u as usize].sprite;
        self.sprites[s as usize].elevation = 1;
        self.doodad_queue_idle(d, u);
        self.units[u as usize].status |= 0x400;
        self.order_next_or_idle(d, u);
        self.map_changed = true;
    }

    /// 0x47bc50: CloseDoor.
    pub(super) fn order_close_door(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_signal & 1 == 0 {
            return;
        }
        self.units[u as usize].order_signal &= !1;
        self.units[u as usize].status &= !0x20_0000;
        let t = self.units[u as usize].unit_type;
        let s = self.units[u as usize].sprite;
        self.sprites[s as usize].elevation = d.dat.units.elevation[t as usize];
        self.doodad_queue_idle(d, u);
        self.order_next_or_idle(d, u);
        self.update_unit_overlaps(d, u);
    }

    /// 0x47bf80: HiddenGun: cloaked until something comes in range, then it attacks.
    pub(super) fn order_hidden_gun(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            let un = &mut self.units[u as usize];
            un.status |= 0x20_0000;
            if matches!(un.unit_type, 0xcb | 0xd1) {
                let s = un.sprite;
                self.sprites[s as usize].elevation = 0;
            }
            let un = &mut self.units[u as usize];
            un.status |= 0x300;
            un.visibility = 0x8000_0000;
            un.ai_action_flag = 0;
            un.order_state = 1;
            return;
        }
        if self.units[u as usize].order_state != 1 || self.disabled(u) || self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let t = self.find_target_in_range(d, u);
        if t == NIL {
            return;
        }
        self.units[u as usize].status &= !0x20_0000;
        let ty = self.units[u as usize].unit_type;
        if matches!(ty, 0xcb | 0xd1) {
            let s = self.units[u as usize].sprite;
            self.sprites[s as usize].elevation = d.dat.units.elevation[ty as usize];
        }
        let un = &mut self.units[u as usize];
        un.status &= !0x300;
        un.ai_action_flag = 0;
        self.order_now(d, u, 0xab);
        // 0x474d10 / 0x474d60: attack it, then hide again (0xaa).
        let o = d.dat.units.ai_attack_unit[ty as usize];
        let p = self.unit_sprite_pos(t);
        self.issue_order(d, u, o, true, p, t, 0xe4);
        self.issue_order(d, u, 0xaa, false, (0, 0), NIL, 0xe4);
    }

    /// 0x47c0a0: HideTrap: attack anything in range (and hide again after), else play
    /// the hide animation.
    pub(super) fn order_hide_trap(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                if !self.disabled(u) {
                    let t = self.find_target_in_range(d, u);
                    if t != NIL {
                        let o = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
                        self.order_on(d, u, o, t);
                        if self.units[u as usize].order != 0 {
                            loop {
                                let tail = self.units[u as usize].order_queue_tail;
                                if tail == NIL || self.orders[tail as usize].order != 0xaa {
                                    break;
                                }
                                self.order_remove(d, u, tail);
                            }
                            self.order_queue_append(d, u, 0xaa, 0xe4, (0, 0), NIL);
                        }
                        self.order_next_or_idle(d, u);
                        return;
                    }
                }
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xe);
                self.units[u as usize].order_signal &= !1;
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if self.units[u as usize].order_signal & 1 != 0 {
                    self.units[u as usize].order_signal &= !1;
                    if self.units[u as usize].order_queue_head == NIL {
                        self.doodad_idle(d, u);
                    }
                    self.order_next_or_idle(d, u);
                }
            }
            _ => {}
        }
    }

    /// 0x47c1b0: RevealTrap.
    pub(super) fn order_reveal_trap(&mut self, d: &Data, u: u32) {
        match self.units[u as usize].order_state {
            0 => {
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xd);
                self.units[u as usize].order_signal &= !1;
                self.units[u as usize].order_state = 1;
            }
            1 => {
                if self.units[u as usize].order_signal & 1 != 0 {
                    self.units[u as usize].order_signal &= !1;
                    if self.units[u as usize].order_queue_head == NIL {
                        self.doodad_idle(d, u);
                    }
                    self.order_next_or_idle(d, u);
                }
            }
            _ => {}
        }
    }

    fn is_trap_doodad(t: u16) -> bool {
        matches!(t, 0xcd | 0xce | 0xcf | 0xd0)
    }

    /// 0x47be80: EnableDoodad.
    pub(super) fn order_enable_doodad(&mut self, d: &Data, u: u32) {
        if self.disabled(u) {
            if self.units[u as usize].order_queue_head == NIL {
                self.doodad_idle(d, u);
            }
            self.order_next_or_idle(d, u);
            return;
        }
        if self.units[u as usize].status & 0x2000 != 0 {
            return;
        }
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x1b);
        let t = self.units[u as usize].unit_type;
        if matches!(t, 0xcb | 0xd1) || State::is_trap_doodad(t) {
            self.sprites[s as usize].elevation = d.dat.units.elevation[t as usize];
        }
        if !State::is_trap_doodad(t) {
            if self.units[u as usize].order_queue_head == NIL {
                self.doodad_idle(d, u);
            }
            self.order_next_or_idle(d, u);
            return;
        }
        // 0x48ed50: the trap sound.
        self.order_queue_front(d, u, 0xa9, (0, 0));
        self.order_next_or_idle(d, u);
    }

    /// 0x47bd60: DisableDoodad.
    pub(super) fn order_disable_doodad(&mut self, d: &Data, u: u32) {
        if self.disabled(u) {
            if self.units[u as usize].order_queue_head == NIL {
                self.doodad_idle(d, u);
            }
            self.order_next_or_idle(d, u);
            return;
        }
        if self.units[u as usize].status & 0x2000 != 0 {
            return;
        }
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0x18);
        let t = self.units[u as usize].unit_type;
        if matches!(t, 0xcb | 0xd1) || State::is_trap_doodad(t) {
            self.sprites[s as usize].elevation = 1;
        }
        if State::is_trap_doodad(t) {
            // 0x48ed50: the trap sound.
            self.order_queue_front(d, u, 0xa8, (0, 0));
            self.order_next_or_idle(d, u);
            return;
        }
        let un = &mut self.units[u as usize];
        un.visibility = 0x8000_0000;
        un.ai_action_flag = 0;
        un.status |= 0x700;
        if un.order_queue_head == NIL {
            self.doodad_idle(d, u);
        }
        self.order_next_or_idle(d, u);
    }
}
