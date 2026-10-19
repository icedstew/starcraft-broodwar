//! cunitsave.cpp: the units.

use super::{In, Out};
use crate::bw::ai_town::{ai_index, ai_kind, ai_ref};
use crate::bw::list::{self, List, NIL};
use crate::bw::unit::Unit;
use crate::bw::{Data, State};

/// Units in the game (0x6a4); the file's unit masks and records are of this size.
const MAX: usize = 0x6a4;

/// A unit's 0x150 native bytes.
struct Raw([u8; 0x150]);

impl Raw {
    fn w8(&mut self, o: usize, v: u8) {
        self.0[o] = v;
    }
    fn w16(&mut self, o: usize, v: u16) {
        self.0[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn w32(&mut self, o: usize, v: u32) {
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn r8(&self, o: usize) -> u8 {
        self.0[o]
    }
    fn r16(&self, o: usize) -> u16 {
        u16::from_le_bytes([self.0[o], self.0[o + 1]])
    }
    fn r32(&self, o: usize) -> u32 {
        u32::from_le_bytes([self.0[o], self.0[o + 1], self.0[o + 2], self.0[o + 3]])
    }
}

/// 0x47b2e0: the production buildings that have a rally point.
fn has_rally(t: u16) -> bool {
    matches!(t, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b)
}

/// A slot index as the savers store it: index + 1, 0 = null.
fn idx1(v: u32) -> u32 {
    if v == NIL { 0 } else { v + 1 }
}

/// A stored index + 1 as a slot.
fn slot1(v: u32) -> u32 {
    if v == 0 { NIL } else { v - 1 }
}

/// A native field that Rust keeps as a slot (NIL = null) but 1.16.1 does not convert for
/// this unit: a stored 0 is null.
fn raw_ptr(v: u32) -> u32 {
    if v == NIL { 0 } else { v }
}
fn raw_ptr_back(v: u32) -> u32 {
    if v == 0 { NIL } else { v }
}

impl State {
    /// 0x4ea970 / 0x4e3410 / 0x47b1d0: a unit pointer as the savers store it: index + 1 with
    /// the target's unique id (+0xa5) shifted by 11; 0 for null.
    fn unit_ref(&self, u: u32) -> u32 {
        if u == NIL || u as usize >= self.units.len() || u as usize + 1 > MAX {
            return 0;
        }
        self.unit_handle(u)
    }

    /// The same for a field of the 0xc0 union, which keeps unit pointers as index + 1.
    fn unit_ref1(&self, v: u32) -> u32 {
        if v == 0 { 0 } else { self.unit_ref(v - 1) }
    }

    /// 0x4ea9d0: one byte per unit that is in the visible, dead, hidden or scanner list, or
    /// is the subunit (flag 0x10) of one that is.
    fn unit_save_mask(&self, d: &Data) -> Vec<u8> {
        let mut m = vec![0u8; MAX];
        for head in [self.visible_units.head, self.dead_units.head, self.hidden_units.head, self.scanner_units.head] {
            for u in list::iter(&self.units, head) {
                let s = self.units[u as usize].subunit;
                if s != NIL && d.dat.units.flags[self.units[s as usize].unit_type as usize] & 0x10 != 0 {
                    m[s as usize] = 1;
                }
                m[u as usize] = 1;
            }
        }
        m
    }

    /// 0x4e3410, the part that changes the game: a unit in the finder leaves it (and its
    /// tiles / repulse cell), marked by finder index [2] = 1. 0x4e2e20 puts it back. (The
    /// pointer to index conversion is `unit_image`: the Rust state keeps indices.)
    fn unit_unlink_world(&mut self, d: &Data, u: u32) {
        let ui = u as usize;
        if self.units[ui].finder_index[0] >= 0 {
            self.finder_remove(u);
            let st = self.units[ui].status;
            if st & 2 != 0 {
                let s = &self.sprites[self.units[ui].sprite as usize];
                let p = (s.x, s.y);
                self.mark_building_tiles(d, u, p, false);
            }
            if st & 4 != 0 {
                self.repulse_remove(d, u);
            }
            self.units[ui].finder_index[2] = 1;
        }
    }

    /// 0x4e2e20, the part that changes the game: a unit that 0x4e3410 took out of the finder
    /// (finder index [2] == 1) goes back in, and a grounded building that has a research
    /// but no upgrade level gets it (level + 1).
    fn unit_relink_world(&mut self, d: &Data, u: u32) {
        let ui = u as usize;
        let st = self.units[ui].status;
        if self.units[ui].finder_index[2] == 1 {
            self.units[ui].finder_index[2] = -1;
            self.finder_insert(d, u);
            if st & 2 != 0 {
                let s = &self.sprites[self.units[ui].sprite as usize];
                let p = (s.x, s.y);
                self.mark_building_tiles(d, u, p, true);
            }
            self.update_unit_overlaps(d, u);
            if st & 4 != 0 {
                self.repulse_init(d, u);
            }
        }
        if st & 2 != 0 {
            let b = self.units[ui].u8_at(0xc9);
            if b != 0x3d && self.units[ui].u8_at(0xcd) == 0 {
                let l = self.players.upgrade_level(self.units[ui].player, b as usize);
                self.units[ui].set_u8_at(0xcd, l.wrapping_add(1));
            }
        }
    }

    /// 0x4e3410: the 0x150 bytes of unit `u` as 1.16.1 holds them once its pointers are
    /// indices (prev / next too).
    fn unit_image(&self, d: &Data, u: u32) -> Raw {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let f = d.dat.units.flags.get(t as usize).copied().unwrap_or(0);
        let fl = &un.fl;
        let mut b = Raw([0; 0x150]);
        b.w32(0x00, self.unit_ref(un.link.prev));
        b.w32(0x04, self.unit_ref(un.link.next));
        b.w32(0x08, un.hp as u32);
        b.w32(0x0c, idx1(un.sprite)); // 0x479ec0
        b.w16(0x10, fl.move_target.0 as u16);
        b.w16(0x12, fl.move_target.1 as u16);
        b.w32(0x14, self.unit_ref(un.move_target_unit)); // 0x479ec0
        b.w16(0x18, fl.next_waypoint.0 as u16);
        b.w16(0x1a, fl.next_waypoint.1 as u16);
        b.w16(0x1c, fl.next_target_waypoint.0 as u16);
        b.w16(0x1e, fl.next_target_waypoint.1 as u16);
        b.w8(0x20, fl.flags);
        b.w8(0x21, fl.heading);
        b.w8(0x22, fl.turn_rate);
        b.w8(0x23, fl.velocity_dir_prev);
        b.w16(0x24, fl.flingy_id);
        // not kept: +0x26
        b.w8(0x27, fl.movement_type);
        b.w16(0x28, fl.pos.0 as u16);
        b.w16(0x2a, fl.pos.1 as u16);
        b.w32(0x2c, fl.exact.0 as u32);
        b.w32(0x30, fl.exact.1 as u32);
        b.w32(0x34, fl.top_speed as u32);
        b.w32(0x38, fl.speed as u32);
        b.w32(0x3c, fl.next_speed as u32);
        b.w32(0x40, fl.velocity.0 as u32);
        b.w32(0x44, fl.velocity.1 as u32);
        b.w16(0x48, fl.acceleration);
        b.w8(0x4a, fl.velocity_dir);
        b.w8(0x4b, fl.desired_dir);
        b.w8(0x4c, un.player);
        b.w8(0x4d, un.order);
        b.w8(0x4e, un.order_state);
        b.w8(0x4f, un.order_signal);
        b.w16(0x50, un.order_unit_type);
        // not kept: +0x52 (2 bytes)
        b.w8(0x54, un.main_order_timer);
        b.w8(0x55, un.ground_cooldown);
        b.w8(0x56, un.air_cooldown);
        b.w8(0x57, un.spell_cooldown);
        b.w16(0x58, un.order_target_pos.0 as u16);
        b.w16(0x5a, un.order_target_pos.1 as u16);
        b.w32(0x5c, self.unit_ref(un.order_target_unit)); // 0x479ec0
        b.w32(0x60, un.shields as u32);
        b.w16(0x64, t);
        // not kept: +0x66 (2 bytes)
        b.w32(0x68, self.unit_ref(un.player_prev));
        b.w32(0x6c, self.unit_ref(un.player_next));
        b.w32(0x70, self.unit_ref(un.subunit));
        b.w32(0x74, idx1(un.order_queue_head)); // order pool 0x6416a0, stride 0x14
        b.w32(0x78, idx1(un.order_queue_tail));
        b.w32(0x7c, self.unit_ref(un.auto_target));
        b.w32(0x80, self.unit_ref(un.connected_unit));
        b.w8(0x84, un.order_queue_count);
        b.w8(0x85, un.order_process_timer);
        b.w8(0x86, un.unknown_86);
        b.w8(0x87, un.attack_notify_timer);
        b.w16(0x88, un.previous_unit_type);
        b.w8(0x8a, un.last_event_timer);
        b.w8(0x8b, un.last_event_color);
        // not kept: +0x8c
        b.w8(0x8d, un.rank_increase);
        b.w8(0x8e, un.unknown_8e);
        b.w8(0x8f, un.kill_count);
        b.w8(0x90, un.last_attacking_player);
        b.w8(0x91, un.ai_action_flag);
        b.w8(0x92, un.user_action_flags);
        b.w8(0x93, un.order_flags);
        b.w16(0x94, un.button_set);
        b.w8(0x96, un.is_cloaked);
        b.w8(0x97, un.movement_state);
        for (k, &q) in un.build_queue.iter().enumerate() {
            b.w16(0x98 + 2 * k, q);
        }
        b.w16(0xa2, un.energy);
        b.w8(0xa4, un.build_queue_slot);
        b.w8(0xa5, un.unique_id);
        b.w8(0xa6, un.secondary_order);
        b.w8(0xa7, un.building_overlay_state);
        b.w16(0xa8, un.hp_gain);
        b.w16(0xaa, un.shield_gain);
        b.w16(0xac, un.remaining_build_time);
        b.w16(0xae, un.previous_hp);
        for (k, &q) in un.loaded_units.iter().enumerate() {
            b.w16(0xb0 + 2 * k, q);
        }
        b.0[0xc0..0xdc].copy_from_slice(&un.union_c0);
        // The union's unit pointers (the Rust union keeps index + 1; ghost / pylon 0xd0 are a
        // thingy / sprite index + 1, as stored).
        let mut conv = |o: usize| {
            let v = self.unit_ref1(b.r32(o));
            b.w32(o, v);
        };
        if un.status & 2 != 0 {
            conv(0xc0);
        }
        if f & 8 != 0 {
            conv(0xc0);
            conv(0xc8);
        }
        if matches!(t, 0x48 | 0x52 | 0x53 | 0x51) {
            conv(0xc0);
            conv(0xc4);
        }
        if matches!(t, 0x49 | 0x55) {
            conv(0xc0);
            conv(0xc4);
            conv(0xc8);
        }
        if t == 0x6c || t == 0x86 {
            conv(0xd0);
        }
        if f & 0x800 != 0 {
            conv(0xd4);
        }
        if f & 0x2000 != 0 {
            conv(0xd4);
        }
        if f & 8 != 0 {
            conv(0xd0);
            conv(0xd4);
            conv(0xd8);
        }
        b.w32(0xdc, un.status);
        b.w8(0xe0, un.resource_type);
        b.w8(0xe1, un.wireframe_seed);
        b.w8(0xe2, un.secondary_order_state);
        b.w8(0xe3, un.recent_order_timer);
        b.w32(0xe4, un.visibility);
        b.w16(0xe8, un.secondary_order_pos.0 as u16);
        b.w16(0xea, un.secondary_order_pos.1 as u16);
        b.w32(0xec, self.unit_ref(un.current_build_unit));
        if f & 1 == 0 {
            b.w32(0xf0, self.unit_ref(un.prev_burrowed));
            b.w32(0xf4, self.unit_ref(un.next_burrowed));
        } else {
            b.w32(0xf0, raw_ptr(un.prev_burrowed));
            b.w32(0xf4, raw_ptr(un.next_burrowed));
        }
        if t == 0x9c {
            b.w32(0xf8, self.unit_ref(un.psi_prev));
            b.w32(0xfc, self.unit_ref(un.psi_next));
        } else {
            b.w16(0xf8, un.rally_pos.0 as u16);
            b.w16(0xfa, un.rally_pos.1 as u16);
            b.w32(0xfc, if has_rally(t) { self.unit_ref(un.rally_unit) } else { raw_ptr(un.rally_unit) });
        }
        // The path is stored as its offset in the pool (0x6bee8c; 0x80 bytes each).
        b.w32(0x100, if un.path == NIL { 0 } else { un.path.wrapping_mul(0x80) });
        b.w8(0x104, un.pathing_collision_interval);
        b.w8(0x105, un.pathing_flags);
        // not kept: +0x106
        b.w8(0x107, un.is_being_healed);
        for (k, &q) in un.contour_bounds.iter().enumerate() {
            b.w16(0x108 + 2 * k, q as u16);
        }
        b.w16(0x110, un.remove_timer);
        b.w16(0x112, un.defense_matrix_damage);
        b.w8(0x114, un.defense_matrix_timer);
        b.w8(0x115, un.stim_timer);
        b.w8(0x116, un.ensnare_timer);
        b.w8(0x117, un.lockdown_timer);
        b.w8(0x118, un.irradiate_timer);
        b.w8(0x119, un.stasis_timer);
        b.w8(0x11a, un.plague_timer);
        b.w8(0x11b, un.storm_timer);
        b.w32(0x11c, self.unit_ref(un.irradiated_by));
        b.w8(0x120, un.irradiate_player);
        b.w8(0x121, un.parasite_flags);
        b.w8(0x122, un.cycle_counter);
        b.w8(0x123, un.is_blind);
        b.w8(0x124, un.maelstrom_timer);
        // not kept: +0x125
        b.w8(0x126, un.acid_spore_count);
        b.0[0x127..0x130].copy_from_slice(&un.acid_spore_time);
        b.w16(0x130, un.bullet_spread);
        // not kept: +0x132 (2 bytes)
        // 0x4e2d30: the AI as kind << 16 | index + 1 in its pool.
        b.w32(0x134, if un.ai == NIL {
            0
        } else {
            let k = ai_kind(un.ai);
            (k << 16) | if (1..=4).contains(&k) { ai_index(un.ai) + 1 } else { 0 }
        });
        b.w16(0x138, un.air_strength);
        b.w16(0x13a, un.ground_strength);
        for (k, &q) in un.finder_index.iter().enumerate() {
            b.w32(0x13c + 4 * k, q as u32);
        }
        b.w8(0x14c, un.repulse_unknown);
        b.w8(0x14d, un.repulse_angle);
        b.w8(0x14e, un.repulse_x);
        b.w8(0x14f, un.repulse_y);
        b
    }

    /// 0x4e2e20: the unit a record's 0x150 bytes describe, their stored indices turned into
    /// slots (the game turns the pointers back; the finder and the other effects are
    /// `unit_relink_world`).
    fn unit_from_image(&self, d: &Data, b: &Raw) -> Unit {
        let n = self.units.len() as u32;
        let hd = |v: u32| -> u32 {
            let i = v & 0x7ff;
            if v == 0 || i == 0 || i > n { NIL } else { i - 1 }
        };
        // A union field that keeps a unit as index + 1.
        let hd1 = |v: u32| -> u32 {
            let i = v & 0x7ff;
            if v == 0 || i > n { 0 } else { i }
        };
        let t = b.r16(0x64);
        let status = b.r32(0xdc);
        let f = d.dat.units.flags.get(t as usize).copied().unwrap_or(0);
        let mut un = Unit::default();
        un.link.prev = hd(b.r32(0x00));
        un.link.next = hd(b.r32(0x04));
        un.hp = b.r32(0x08) as i32;
        un.sprite = slot1(b.r32(0x0c));
        let fl = &mut un.fl;
        fl.move_target = (b.r16(0x10) as i16, b.r16(0x12) as i16);
        un.move_target_unit = hd(b.r32(0x14));
        let fl = &mut un.fl;
        fl.next_waypoint = (b.r16(0x18) as i16, b.r16(0x1a) as i16);
        fl.next_target_waypoint = (b.r16(0x1c) as i16, b.r16(0x1e) as i16);
        fl.flags = b.r8(0x20);
        fl.heading = b.r8(0x21);
        fl.turn_rate = b.r8(0x22);
        fl.velocity_dir_prev = b.r8(0x23);
        fl.flingy_id = b.r16(0x24);
        fl.movement_type = b.r8(0x27);
        fl.pos = (b.r16(0x28) as i16, b.r16(0x2a) as i16);
        fl.exact = (b.r32(0x2c) as i32, b.r32(0x30) as i32);
        fl.top_speed = b.r32(0x34) as i32;
        fl.speed = b.r32(0x38) as i32;
        fl.next_speed = b.r32(0x3c) as i32;
        fl.velocity = (b.r32(0x40) as i32, b.r32(0x44) as i32);
        fl.acceleration = b.r16(0x48);
        fl.velocity_dir = b.r8(0x4a);
        fl.desired_dir = b.r8(0x4b);
        un.player = b.r8(0x4c);
        un.order = b.r8(0x4d);
        un.order_state = b.r8(0x4e);
        un.order_signal = b.r8(0x4f);
        un.order_unit_type = b.r16(0x50);
        un.main_order_timer = b.r8(0x54);
        un.ground_cooldown = b.r8(0x55);
        un.air_cooldown = b.r8(0x56);
        un.spell_cooldown = b.r8(0x57);
        un.order_target_pos = (b.r16(0x58) as i16, b.r16(0x5a) as i16);
        un.order_target_unit = hd(b.r32(0x5c));
        un.shields = b.r32(0x60) as i32;
        un.unit_type = t;
        un.player_prev = hd(b.r32(0x68));
        un.player_next = hd(b.r32(0x6c));
        un.subunit = hd(b.r32(0x70));
        un.order_queue_head = slot1(b.r32(0x74));
        un.order_queue_tail = slot1(b.r32(0x78));
        un.auto_target = hd(b.r32(0x7c));
        un.connected_unit = hd(b.r32(0x80));
        un.order_queue_count = b.r8(0x84);
        un.order_process_timer = b.r8(0x85);
        un.unknown_86 = b.r8(0x86);
        un.attack_notify_timer = b.r8(0x87);
        un.previous_unit_type = b.r16(0x88);
        un.last_event_timer = b.r8(0x8a);
        un.last_event_color = b.r8(0x8b);
        un.rank_increase = b.r8(0x8d);
        un.unknown_8e = b.r8(0x8e);
        un.kill_count = b.r8(0x8f);
        un.last_attacking_player = b.r8(0x90);
        un.ai_action_flag = b.r8(0x91);
        un.user_action_flags = b.r8(0x92);
        un.order_flags = b.r8(0x93);
        un.button_set = b.r16(0x94);
        un.is_cloaked = b.r8(0x96);
        un.movement_state = b.r8(0x97);
        for k in 0..5 {
            un.build_queue[k] = b.r16(0x98 + 2 * k);
        }
        un.energy = b.r16(0xa2);
        un.build_queue_slot = b.r8(0xa4);
        un.unique_id = b.r8(0xa5);
        un.secondary_order = b.r8(0xa6);
        un.building_overlay_state = b.r8(0xa7);
        un.hp_gain = b.r16(0xa8);
        un.shield_gain = b.r16(0xaa);
        un.remaining_build_time = b.r16(0xac);
        un.previous_hp = b.r16(0xae);
        for k in 0..8 {
            un.loaded_units[k] = b.r16(0xb0 + 2 * k);
        }
        un.union_c0.copy_from_slice(&b.0[0xc0..0xdc]);
        let conv = |un: &mut Unit, o: usize| {
            let v = hd1(un.u32_at(o));
            un.set_u32_at(o, v);
        };
        if status & 2 != 0 {
            conv(&mut un, 0xc0);
        }
        if f & 8 != 0 {
            conv(&mut un, 0xc0);
            conv(&mut un, 0xc8);
        }
        if matches!(t, 0x48 | 0x52 | 0x53 | 0x51) {
            conv(&mut un, 0xc0);
            conv(&mut un, 0xc4);
        }
        if matches!(t, 0x49 | 0x55) {
            conv(&mut un, 0xc0);
            conv(&mut un, 0xc4);
            conv(&mut un, 0xc8);
        }
        if t == 0x6c || t == 0x86 {
            conv(&mut un, 0xd0);
        }
        if f & 0x800 != 0 {
            conv(&mut un, 0xd4);
        }
        if f & 0x2000 != 0 {
            conv(&mut un, 0xd4);
        }
        if f & 8 != 0 {
            conv(&mut un, 0xd0);
            conv(&mut un, 0xd4);
            conv(&mut un, 0xd8);
        }
        un.status = status;
        un.resource_type = b.r8(0xe0);
        un.wireframe_seed = b.r8(0xe1);
        un.secondary_order_state = b.r8(0xe2);
        un.recent_order_timer = b.r8(0xe3);
        un.visibility = b.r32(0xe4);
        un.secondary_order_pos = (b.r16(0xe8) as i16, b.r16(0xea) as i16);
        un.current_build_unit = hd(b.r32(0xec));
        if f & 1 == 0 {
            un.prev_burrowed = hd(b.r32(0xf0));
            un.next_burrowed = hd(b.r32(0xf4));
        } else {
            un.prev_burrowed = raw_ptr_back(b.r32(0xf0));
            un.next_burrowed = raw_ptr_back(b.r32(0xf4));
        }
        if t == 0x9c {
            un.psi_prev = hd(b.r32(0xf8));
            un.psi_next = hd(b.r32(0xfc));
        } else {
            un.rally_pos = (b.r16(0xf8) as i16, b.r16(0xfa) as i16);
            un.rally_unit = if has_rally(t) { hd(b.r32(0xfc)) } else { raw_ptr_back(b.r32(0xfc)) };
        }
        let p = b.r32(0x100);
        un.path = if p == 0 || p / 0x80 >= self.paths.len() as u32 { NIL } else { p / 0x80 };
        un.pathing_collision_interval = b.r8(0x104);
        un.pathing_flags = b.r8(0x105);
        un.is_being_healed = b.r8(0x107);
        for k in 0..4 {
            un.contour_bounds[k] = b.r16(0x108 + 2 * k) as i16;
        }
        un.remove_timer = b.r16(0x110);
        un.defense_matrix_damage = b.r16(0x112);
        un.defense_matrix_timer = b.r8(0x114);
        un.stim_timer = b.r8(0x115);
        un.ensnare_timer = b.r8(0x116);
        un.lockdown_timer = b.r8(0x117);
        un.irradiate_timer = b.r8(0x118);
        un.stasis_timer = b.r8(0x119);
        un.plague_timer = b.r8(0x11a);
        un.storm_timer = b.r8(0x11b);
        un.irradiated_by = hd(b.r32(0x11c));
        un.irradiate_player = b.r8(0x120);
        un.parasite_flags = b.r8(0x121);
        un.cycle_counter = b.r8(0x122);
        un.is_blind = b.r8(0x123);
        un.maelstrom_timer = b.r8(0x124);
        un.acid_spore_count = b.r8(0x126);
        un.acid_spore_time.copy_from_slice(&b.0[0x127..0x130]);
        un.bullet_spread = b.r16(0x130);
        // 0x4e2cb0: kind << 16 | index + 1 back to the AI reference; a kind outside 1..=4
        // stays as stored, an index of 0 is null.
        let a = b.r32(0x134);
        un.ai = if a == 0 {
            NIL
        } else if (1..=4).contains(&(a >> 16)) {
            if a & 0xffff == 0 { NIL } else { ai_ref(a >> 16, (a & 0xffff) - 1) }
        } else {
            a
        };
        un.air_strength = b.r16(0x138);
        un.ground_strength = b.r16(0x13a);
        for k in 0..4 {
            un.finder_index[k] = b.r32(0x13c + 4 * k) as i32;
        }
        un.repulse_unknown = b.r8(0x14c);
        un.repulse_angle = b.r8(0x14d);
        un.repulse_x = b.r8(0x14e);
        un.repulse_y = b.r8(0x14f);
        un
    }

    /// 0x4eaaf0: the mask of saved units (0x4ea9d0, packed), the number of them and their
    /// records (index and 0x150 bytes, packed), the head of the 0x63ff5c list (0x4915c0),
    /// then the heads and tails of the visible, hidden, dead and scanner unit lists (0x4ea970)
    /// and the 12 players' unit list heads. 0x4e3410 takes each saved unit out of the
    /// finder before and 0x4e2e20 puts it back after.
    pub(super) fn save_units(&mut self, d: &Data, o: &mut Out) -> bool {
        let mask = self.unit_save_mask(d);
        let mut count = 0u32;
        for u in (0..MAX).rev() {
            if mask[u] != 0 {
                count += 1;
                self.unit_unlink_world(d, u as u32);
            }
        }
        let mut ok = o.packed(&mask);
        if ok {
            o.u32(count);
            if count != 0 {
                let mut recs = Vec::with_capacity(count as usize * 0x154);
                for u in 0..MAX {
                    if mask[u] != 0 {
                        recs.extend_from_slice(&(u as u32).to_le_bytes());
                        recs.extend_from_slice(&self.unit_image(d, u as u32).0);
                    }
                }
                ok = o.packed(&recs);
            }
        }
        for u in (0..MAX).rev() {
            if mask[u] != 0 {
                self.unit_relink_world(d, u as u32);
            }
        }
        if !ok {
            return false;
        }
        o.u32(self.unit_ref(self.cloak_list_head)); // 0x4915c0
        let l = [self.visible_units, self.hidden_units, self.dead_units, self.scanner_units];
        for x in l {
            o.u32(self.unit_ref(x.head));
        }
        for x in l {
            o.u32(self.unit_ref(x.tail));
        }
        for p in 0..12 {
            o.u32(self.unit_ref(self.player_units[p]));
        }
        true
    }

    /// 0x49e790: the records of a save of format 1 or 2 (0x144 bytes: the index, the unit's
    /// bytes 0..0x124 and 0x134..0x150), as full images; buildings' research ids written by
    /// the old game are renumbered.
    fn load_unit_records_old(&self, d: &Data, i: &mut In, count: u32) -> Option<Vec<(usize, Raw)>> {
        let buf = i.packed(count as usize * 0x144)?;
        let mut v = Vec::new();
        for r in buf.chunks_exact(0x144) {
            let idx = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
            let mut b = Raw([0; 0x150]);
            b.0[..0x124].copy_from_slice(&r[4..0x128]);
            b.0[0x134..].copy_from_slice(&r[0x128..]);
            let t = b.r16(0x64);
            if d.dat.units.flags.get(t as usize).copied().unwrap_or(0) & 1 != 0 {
                if b.r8(0xc8) == 0x18 {
                    b.w8(0xc8, 0x2c);
                }
                if b.r8(0xc9) == 0x2e {
                    b.w8(0xc9, 0x3d);
                }
            }
            v.push((idx, b));
        }
        Some(v)
    }

    /// 0x49e6c0: the records (the index and the unit's 0x150 bytes, 0x154 each).
    fn load_unit_records(&self, i: &mut In, count: u32) -> Option<Vec<(usize, Raw)>> {
        let buf = i.packed(count as usize * 0x154)?;
        Some(
            buf.chunks_exact(0x154)
                .map(|r| {
                    let mut b = Raw([0; 0x150]);
                    b.0.copy_from_slice(&r[4..]);
                    (u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize, b)
                })
                .collect(),
        )
    }

    /// 0x49e910: the units back: the mask, the count, the records (0x49e790 below format 3,
    /// else 0x49e6c0; all other units are zeroed), the pointers to slots and the finder again
    /// (0x4e2e20), the 0x63ff5c head, the list heads and tails, the free list of the unused
    /// units (0x49e1f0) and the players' list heads.
    pub(super) fn load_units(&mut self, d: &Data, i: &mut In) -> bool {
        let Some(mask) = i.packed(MAX) else { return false };
        let Some(count) = i.u32() else { return false };
        for u in self.units.iter_mut() {
            *u = Unit::default();
        }
        if count != 0 {
            let recs = if i.format < 3 { self.load_unit_records_old(d, i, count) } else { self.load_unit_records(i, count) };
            let Some(recs) = recs else { return false };
            for (idx, b) in recs {
                // 1.16.1 does not check the index.
                if idx >= self.units.len() {
                    return false;
                }
                self.units[idx] = self.unit_from_image(d, &b);
            }
        }
        for u in (0..MAX.min(self.units.len())).rev() {
            if mask[u] != 0 {
                self.unit_relink_world(d, u as u32);
            }
        }
        let n = self.units.len() as u32;
        let hd = |v: u32| -> u32 {
            let i = v & 0x7ff;
            if v == 0 || i == 0 || i > n { NIL } else { i - 1 }
        };
        let Some(v) = i.u32() else { return false }; // 0x491570
        self.cloak_list_head = hd(v);
        let mut w = [NIL; 8];
        for x in w.iter_mut() {
            let Some(v) = i.u32() else { return false };
            *x = hd(v);
        }
        self.visible_units = List { head: w[0], tail: w[4] };
        self.hidden_units = List { head: w[1], tail: w[5] };
        self.dead_units = List { head: w[2], tail: w[6] };
        self.scanner_units = List { head: w[3], tail: w[7] };
        // 0x49e1f0: every unit not in the file goes after the head of the free list.
        let mut free = List::default();
        for u in 0..MAX.min(self.units.len()) {
            if mask[u] == 0 {
                list::insert_after_head(&mut self.units, u as u32, &mut free);
            }
        }
        self.free_units = free;
        for p in 0..12 {
            let Some(v) = i.u32() else { return false };
            self.player_units[p] = hd(v);
        }
        true
    }
}
