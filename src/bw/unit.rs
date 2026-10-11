//! CUnit. Offsets: x86 1.16.1 (primary reference) / x64 SC:R (re/NOTES.md).
//! Pointers are slot indices (NIL = null).

use super::list::{Link, Node, NIL};
use crate::game::flingy::Flingy;

pub mod status {
    pub const COMPLETED: u32 = 0x1;
    pub const GROUNDED_BUILDING: u32 = 0x2;
    pub const IN_AIR: u32 = 0x4;
    pub const DISABLED: u32 = 0x8;
    pub const BURROWED: u32 = 0x10;
    pub const IN_BUILDING: u32 = 0x20;
    pub const IN_TRANSPORT: u32 = 0x40;
    pub const CAN_BE_CHASED: u32 = 0x80;
    pub const REQUIRES_DETECTION: u32 = 0x100;
    pub const CLOAKED: u32 = 0x200;
    pub const DOODAD_DISABLED: u32 = 0x400;
    pub const CLOAKING_FOR_FREE: u32 = 0x800;
    pub const CAN_NOT_RECEIVE_ORDERS: u32 = 0x1000;
    pub const NO_BRK_CODE_START: u32 = 0x2000;
    pub const UNKNOWN_4000: u32 = 0x4000;
    pub const CAN_NOT_ATTACK: u32 = 0x8000;
    pub const CAN_TURN_AROUND_TO_ATTACK: u32 = 0x10000;
    pub const IS_BUILDING: u32 = 0x20000;
    pub const IGNORE_TILE_COLLISION: u32 = 0x40000;
    pub const UNMOVABLE: u32 = 0x80000;
    pub const IS_NORMAL: u32 = 0x100000;
    pub const NO_COLLIDE: u32 = 0x200000;
    pub const IS_GATHERING: u32 = 0x800000;
    pub const PASSIVE_REVEAL: u32 = 0x1000000;
    pub const INVINCIBLE: u32 = 0x4000000;
    pub const HOLD_POSITION: u32 = 0x8000000;
    pub const SPEED_UPGRADE: u32 = 0x10000000;
    pub const COOLDOWN_UPGRADE: u32 = 0x20000000;
    pub const HALLUCINATION: u32 = 0x40000000;
    pub const SELF_DESTRUCTING: u32 = 0x80000000;
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub link: Link,                   // x86 0x00/0x04  x64 0x00/0x08
    pub hp: i32,                      // 0x08 / 0x10
    pub sprite: u32,                  // 0x0C / 0x18
    /// 0x10..0x4B / 0x20..0x63: the CFlingy part.
    pub fl: Flingy,
    pub move_target_unit: u32,        // 0x14 / 0x28
    pub player: u8,                   // 0x4C / 0x68
    pub order: u8,                    // 0x4D / 0x69
    pub order_state: u8,              // 0x4E / 0x6a
    pub order_signal: u8,             // 0x4F / 0x6b
    pub order_unit_type: u16,         // 0x50 / 0x6c
    pub main_order_timer: u8,         // 0x54 / 0x70
    pub ground_cooldown: u8,          // 0x55 / 0x71
    pub air_cooldown: u8,             // 0x56 / 0x72
    pub spell_cooldown: u8,           // 0x57 / 0x73
    pub order_target_pos: (i16, i16), // 0x58 / 0x78
    pub order_target_unit: u32,       // 0x5C / 0x80
    pub shields: i32,                 // 0x60 / 0x88
    pub unit_type: u16,               // 0x64 / 0x8c
    pub player_prev: u32,             // 0x68 / 0x90
    pub player_next: u32,             // 0x6C / 0x98
    pub subunit: u32,                 // 0x70 / 0xa0
    pub order_queue_head: u32,        // 0x74 / 0xa8
    pub order_queue_tail: u32,        // 0x78 / 0xb0
    pub auto_target: u32,             // 0x7C / 0xb8
    pub connected_unit: u32,          // 0x80 / 0xc0
    pub order_queue_count: u8,        // 0x84 / 0xc8
    pub order_process_timer: u8,      // 0x85 / 0xc9
    pub unknown_86: u8,               // 0x86
    pub attack_notify_timer: u8,      // 0x87
    pub previous_unit_type: u16,      // 0x88
    pub last_event_timer: u8,         // 0x8A
    pub last_event_color: u8,         // 0x8B
    pub rank_increase: u8,            // 0x8D
    pub kill_count: u8,               // 0x8E
    pub unknown_8f: u8,               // 0x8F
    pub last_attacking_player: u8,    // 0x90 / 0xd5 (8 = none)
    pub ai_action_flag: u8,           // 0x91 / 0xd6 (cloak/vision stagger counter in update_units)
    pub user_action_flags: u8,        // 0x92 / 0xd7
    /// 0x93: bit 0 = order queued/started by command, bit 1 = random wait before pathing.
    pub order_flags: u8,              // 0x93
    pub button_set: u16,              // 0x94 / 0xd8
    pub is_cloaked: u8,               // 0x96 / 0xda
    pub movement_state: u8,           // 0x97 / 0xdb
    pub build_queue: [u16; 5],        // 0x98 / 0xdc
    pub energy: u16,                  // 0xA2 / 0xe6
    pub build_queue_slot: u8,         // 0xA4 / 0xe8
    pub unique_id: u8,                // 0xA5 / 0xe9
    pub secondary_order: u8,          // 0xA6 / 0xea
    pub building_overlay_state: u8,   // 0xA7 / 0xeb
    pub hp_gain: u16,                 // 0xA8 / 0xec
    pub shield_gain: u16,             // 0xAA / 0xee
    pub remaining_build_time: u16,    // 0xAC / 0xf0
    pub previous_hp: u16,             // 0xAE / 0xf2
    pub loaded_units: [u16; 8],       // 0xB0 / 0xf4
    /// 0xC0..0xDB unions (worker, building, carrier, resource, ...) as raw bytes; see the accessors below.
    pub union_c0: [u8; 0x1c],
    pub status: u32,                  // 0xDC / 0x140
    pub resource_type: u8,            // 0xE0 / 0x144
    pub wireframe_seed: u8,           // 0xE1 / 0x145
    pub secondary_order_state: u8,    // 0xE2 / 0x146
    pub recent_order_timer: u8,       // 0xE3 / 0x147
    pub visibility: u32,              // 0xE4 / 0x148
    pub secondary_order_pos: (i16, i16), // 0xE8
    pub current_build_unit: u32,      // 0xEC / 0x158
    pub prev_burrowed: u32,           // 0xF0
    pub next_burrowed: u32,           // 0xF4
    pub rally_pos: (i16, i16),        // 0xF8
    pub rally_unit: u32,              // 0xFC
    pub path: u32,                    // 0x100
    pub pathing_collision_interval: u8, // 0x104
    pub pathing_flags: u8,            // 0x105
    pub is_being_healed: u8,          // 0x107 / 0x193
    pub contour_bounds: [i16; 4],     // 0x108
    pub remove_timer: u16,            // 0x110 / 0x19c
    pub defense_matrix_damage: u16,   // 0x112
    pub defense_matrix_timer: u8,     // 0x114
    pub stim_timer: u8,               // 0x115 / 0x1a1
    pub ensnare_timer: u8,            // 0x116 / 0x1a2
    pub lockdown_timer: u8,           // 0x117
    pub irradiate_timer: u8,          // 0x118
    pub stasis_timer: u8,             // 0x119
    pub plague_timer: u8,             // 0x11A
    pub storm_timer: u8,              // 0x11B
    pub irradiated_by: u32,           // 0x11C
    pub irradiate_player: u8,         // 0x120
    pub parasite_flags: u8,           // 0x121
    pub cycle_counter: u8,            // 0x122 / 0x1b2
    pub is_blind: u8,                 // 0x123
    pub maelstrom_timer: u8,          // 0x124
    pub acid_spore_count: u8,         // 0x126
    pub acid_spore_time: [u8; 9],     // 0x127
    pub ai: u32,                      // 0x134 / 0x1c8
    pub air_strength: u16,            // 0x138
    pub ground_strength: u16,         // 0x13A
    pub finder_index: [i32; 4],       // 0x13C (left, right, top, bottom)
    pub repulse_unknown: u8,          // 0x14C
    pub repulse_angle: u8,            // 0x14D
    pub repulse_x: u8,                // 0x14E
    pub repulse_y: u8,                // 0x14F
}

impl Default for Unit {
    fn default() -> Unit {
        Unit {
            link: Link::default(),
            hp: 0,
            sprite: NIL,
            fl: Flingy::default(),
            move_target_unit: NIL,
            player: 0,
            order: 0,
            order_state: 0,
            order_signal: 0,
            order_unit_type: 0xe4,
            main_order_timer: 0,
            ground_cooldown: 0,
            air_cooldown: 0,
            spell_cooldown: 0,
            order_target_pos: (0, 0),
            order_target_unit: NIL,
            shields: 0,
            unit_type: 0,
            player_prev: NIL,
            player_next: NIL,
            subunit: NIL,
            order_queue_head: NIL,
            order_queue_tail: NIL,
            auto_target: NIL,
            connected_unit: NIL,
            order_queue_count: 0,
            order_process_timer: 0,
            unknown_86: 0,
            attack_notify_timer: 0,
            previous_unit_type: 0,
            last_event_timer: 0,
            last_event_color: 0,
            rank_increase: 0,
            kill_count: 0,
            unknown_8f: 0,
            last_attacking_player: 0,
            ai_action_flag: 0,
            user_action_flags: 0,
            order_flags: 0,
            button_set: 0,
            is_cloaked: 0,
            movement_state: 0,
            build_queue: [0xe4; 5],
            energy: 0,
            build_queue_slot: 0,
            unique_id: 0,
            secondary_order: 0,
            building_overlay_state: 0,
            hp_gain: 0,
            shield_gain: 0,
            remaining_build_time: 0,
            previous_hp: 0,
            loaded_units: [0; 8],
            union_c0: [0; 0x1c],
            status: 0,
            resource_type: 0,
            wireframe_seed: 0,
            secondary_order_state: 0,
            recent_order_timer: 0,
            visibility: 0,
            secondary_order_pos: (0, 0),
            current_build_unit: NIL,
            prev_burrowed: NIL,
            next_burrowed: NIL,
            rally_pos: (0, 0),
            rally_unit: NIL,
            path: NIL,
            pathing_collision_interval: 0,
            pathing_flags: 0,
            is_being_healed: 0,
            contour_bounds: [0; 4],
            remove_timer: 0,
            defense_matrix_damage: 0,
            defense_matrix_timer: 0,
            stim_timer: 0,
            ensnare_timer: 0,
            lockdown_timer: 0,
            irradiate_timer: 0,
            stasis_timer: 0,
            plague_timer: 0,
            storm_timer: 0,
            irradiated_by: NIL,
            irradiate_player: 0,
            parasite_flags: 0,
            cycle_counter: 0,
            is_blind: 0,
            maelstrom_timer: 0,
            acid_spore_count: 0,
            acid_spore_time: [0; 9],
            ai: NIL,
            air_strength: 0,
            ground_strength: 0,
            finder_index: [0; 4],
            repulse_unknown: 0,
            repulse_angle: 0,
            repulse_x: 0,
            repulse_y: 0,
        }
    }
}

impl Unit {
    /// Read/write a byte of the 0xC0 union by its x86 CUnit offset.
    pub fn u8_at(&self, off: usize) -> u8 {
        self.union_c0[off - 0xc0]
    }
    pub fn set_u8_at(&mut self, off: usize, v: u8) {
        self.union_c0[off - 0xc0] = v;
    }
    pub fn u16_at(&self, off: usize) -> u16 {
        let i = off - 0xc0;
        u16::from_le_bytes([self.union_c0[i], self.union_c0[i + 1]])
    }
    pub fn set_u16_at(&mut self, off: usize, v: u16) {
        let i = off - 0xc0;
        self.union_c0[i..i + 2].copy_from_slice(&v.to_le_bytes());
    }
    /// Resource amount of a mineral field / geyser (x86 +0xD0).
    pub fn resource_amount(&self) -> u16 {
        self.u16_at(0xd0)
    }
}

impl Node for Unit {
    fn link(&self) -> &Link {
        &self.link
    }
    fn link_mut(&mut self) -> &mut Link {
        &mut self.link
    }
}
