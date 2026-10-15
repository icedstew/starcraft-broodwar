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
            if super::unit::is_gun_trap(un.unit_type) {
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
        if super::unit::is_gun_trap(ty) {
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
        if super::unit::is_gun_trap(t) || State::is_trap_doodad(t) {
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
        if super::unit::is_gun_trap(t) || State::is_trap_doodad(t) {
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

impl State {
    /// 0x493920: Interrupted: the shield recharge overlay goes, then the next order.
    pub(super) fn order_interrupted(&mut self, d: &Data, u: u32) {
        let s = self.units[u as usize].sprite;
        let mut i = self.sprites[s as usize].images.head;
        while i != NIL {
            if (0x170..0x173).contains(&self.images[i as usize].image_id) {
                self.destroy_image(i);
                break;
            }
            i = self.images[i as usize].link.next;
        }
        if self.units[u as usize].order_queue_head == NIL {
            let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
            self.issue_order(d, u, o, true, (0, 0), NIL, 0xe4);
        }
        self.order_next_or_idle(d, u);
    }

    /// 0x47c210: JunkYardDog: wander to random spots, attacking whatever turns up.
    pub(super) fn order_junk_yard_dog(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].order_state == 0 {
            let dir = self.rng.rand_between(0x30, 0, 0xff);
            let dist = self.rng.rand_between(0x30, 0x400, 0x800);
            let (vx, vy) = crate::game::flingy::direction_table()[dir as usize];
            let (x, y) = self.unit_sprite_pos(u);
            let mut p = ((((vx * dist) >> 8) as i16).wrapping_add(x), (((vy * dist) >> 8) as i16).wrapping_add(y));
            let t = self.units[u as usize].unit_type;
            self.clamp_unit_pos(d, t, &mut p);
            self.units[u as usize].order_target_pos = p;
            self.set_move_target_xy(d, u, p);
            self.units[u as usize].order_state = 1;
            self.units[u as usize].spell_cooldown = 0xff;
        }
        let t = self.units[u as usize].auto_target;
        if t != NIL
            && self.is_enemy(u, t)
            && self.sprites[self.units[t as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN == 0
            && self.units[t as usize].status & 0x400_0000 == 0
            && !self.cloaked_from(t, u)
        {
            let o = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
            self.order_on(d, u, o, t);
            self.order_queue_append(d, u, 0xbb, 0xe4, (0, 0), NIL);
            self.units[u as usize].auto_target = NIL;
            self.set_auto_targeting(u);
            return;
        }
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 0xf;
        let un = &self.units[u as usize];
        let arrived = un.fl.move_target == self.unit_sprite_pos(u);
        if arrived || (un.spell_cooldown == 0 && un.status & 4 == 0) {
            self.units[u as usize].order_state = 0;
            return;
        }
        let t = self.find_auto_target(d, u);
        if t != NIL {
            let o = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
            self.order_on(d, u, o, t);
            self.order_queue_append(d, u, 0xbb, 0xe4, (0, 0), NIL);
            self.set_auto_targeting(u);
        }
    }

    /// 0x4026a0: `u` (and its turret) pick their own targets from now on.
    pub(super) fn set_auto_targeting(&mut self, u: u32) {
        self.units[u as usize].status |= 0x800_0000;
        let sub = self.units[u as usize].subunit;
        if sub != NIL {
            self.units[sub as usize].status |= 0x800_0000;
        }
    }
}

impl State {
    /// 0x463040: a computer player's AI takes over `u` (towns, the medic list).
    pub(super) fn ai_take_unit(&mut self, u: u32) {
        let p = self.units[u as usize].player;
        if p < 8 && self.player_type(p) == 1 {
            crate::todo_port!("0x463040", "AI: computer player takes the unit");
        }
    }

    /// 0x4a1c20: Neutral: once something attacks it (+0x7c), its player turns
    /// computer-controlled and hostile to the attacker, and its units fight back.
    pub(super) fn order_neutral(&mut self, d: &Data, u: u32) {
        let a = self.units[u as usize].auto_target;
        if a == NIL {
            return;
        }
        let p = self.units[u as usize].player;
        let ap = self.units[a as usize].player;
        let shown = self.sprites[self.units[a as usize].sprite as usize].flags & super::sprite::sflag::HIDDEN == 0;
        if shown && !self.cloaked_from(a, u) && p != ap {
            self.player_types[p as usize] = 1;
            self.alliances[p as usize][ap as usize] = 0;
            self.alliances[ap as usize][p as usize] = 0;
            if self.can_attack(d, u, a, true) {
                self.clear_order_target(d, u);
                let sub = self.units[u as usize].subunit;
                let o = d.dat.units.ai_attack_unit[self.units[u as usize].unit_type as usize];
                self.order_on(d, u, o, a);
                if sub != NIL && self.units[sub as usize].sprite != NIL {
                    let o = d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize];
                    self.order_on(d, sub, o, a);
                }
            } else {
                let o = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
                self.order_now(d, u, o);
            }
            self.ai_take_unit(u);
            let mut c = self.player_units[p as usize];
            while c != NIL {
                let next = self.units[c as usize].player_next;
                let cu = &self.units[c as usize];
                let alive = cu.sprite != NIL && cu.order != 0;
                if alive && d.dat.units.flags[cu.unit_type as usize] & 0x10 == 0 && c != u {
                    let st = cu.status;
                    if st & 0x40 == 0 {
                        let o = d.dat.units.ai_return_to_idle[cu.unit_type as usize];
                        self.order_now(d, c, o);
                    } else if st & 0x20 != 0 {
                        self.order_now(d, c, 5);
                    }
                    let sub = self.units[c as usize].subunit;
                    if sub != NIL && self.units[sub as usize].sprite != NIL {
                        let o = d.dat.units.ai_attack_unit[self.units[sub as usize].unit_type as usize];
                        self.order_on(d, sub, o, a);
                    }
                    self.ai_take_unit(c);
                }
                c = next;
            }
        }
        self.units[u as usize].auto_target = NIL;
    }

    /// 0x4a1ef0: RescuePassive: a human player's unit within 0x40 rescues it (a rescuable
    /// building with units.dat flag 0x1000 brings its whole player along).
    pub(super) fn order_rescue_passive(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].main_order_timer != 0 {
            return;
        }
        self.units[u as usize].main_order_timer = 1;
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x40), y.wrapping_sub(0x40), x.wrapping_add(0x40), y.wrapping_add(0x40)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        let Some(r) = found.into_iter().find(|&c| self.player_type(self.units[c as usize].player) == 2) else {
            return;
        };
        let rp = self.units[r as usize].player;
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let st = un.status;
        let mut whole_player = st & 1 != 0 || t == 0x85 || t == 0x84;
        if !whole_player && t == 0x83 {
            // 0x45cd00: a hatchery morphing into a lair / hive counts as completed.
            let q = un.build_queue[un.build_queue_slot as usize % 5];
            whole_player = matches!(q, 0x85 | 0x84 | 0x89 | 0x90 | 0x92);
        }
        whole_player = whole_player && st & 2 != 0 && d.dat.units.flags[t as usize] & 0x1000 != 0;
        let rescue = |st: &mut State, c: u32| {
            st.change_owner(d, c, rp, true);
            let o = if st.units[c as usize].status & 0x20 != 0 { 5 } else { d.dat.units.ai_computer_idle[st.units[c as usize].unit_type as usize] };
            st.units[c as usize].order_flags |= 1;
            st.issue_order(d, c, o, true, (0, 0), NIL, 0xe4);
            st.prepare_next_order(d, c);
        };
        if whole_player {
            let mut c = self.player_units[self.units[u as usize].player as usize];
            while c != NIL {
                let next = self.units[c as usize].player_next;
                if self.units[c as usize].sprite != NIL && self.units[c as usize].order != 0 {
                    rescue(self, c);
                }
                c = next;
            }
        } else {
            rescue(self, u);
        }
        // 0x48ed50: the rescue sound for the local player (UI).
    }
}
