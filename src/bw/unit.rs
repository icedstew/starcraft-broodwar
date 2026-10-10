//! CUnit (x64 layout in re/NOTES.md). Fields are added as routines are ported.

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
    pub const REQUIRES_DETECTION: u32 = 0x100;
    pub const CLOAKED: u32 = 0x200;
    pub const DOODAD_DISABLED: u32 = 0x400;
    pub const CLOAKING_FOR_FREE: u32 = 0x800;
    pub const CAN_NOT_RECEIVE_ORDERS: u32 = 0x1000;
    pub const CAN_NOT_ATTACK: u32 = 0x8000;
    pub const CAN_TURN_AROUND_TO_ATTACK: u32 = 0x10000;
    pub const IS_BUILDING: u32 = 0x20000;
    pub const IGNORE_TILE_COLLISION: u32 = 0x40000;
    pub const UNMOVABLE: u32 = 0x80000;
    pub const IS_NORMAL: u32 = 0x100000;
    pub const INVINCIBLE: u32 = 0x4000000;
    pub const HOLD_POSITION: u32 = 0x8000000;
    pub const HALLUCINATION: u32 = 0x40000000;
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub link: Link,              // +0x00/+0x08
    pub hp: i32,                 // +0x10
    pub sprite: u32,             // +0x18
    /// +0x20..+0x63: the CFlingy part (move target, waypoints, flags, speeds...).
    pub fl: Flingy,
    pub move_target_unit: u32,   // +0x28
    pub player: u8,              // +0x68
    pub order: u8,               // +0x69
    pub order_state: u8,         // +0x6a
    pub order_signal: u8,        // +0x6b
    pub order_unit_type: u16,    // +0x6c
    pub order_timer: u8,         // +0x70
    pub ground_cooldown: u8,     // +0x71
    pub air_cooldown: u8,        // +0x72
    pub spell_cooldown: u8,      // +0x73
    pub order_target_pos: (i16, i16), // +0x78
    pub order_target_unit: u32,  // +0x80
    pub shields: i32,            // +0x88
    pub unit_type: u16,          // +0x8c
    pub player_prev: u32,        // +0x90
    pub player_next: u32,        // +0x98
    pub subunit: u32,            // +0xa0
    pub order_queue_head: u32,   // +0xa8
    pub order_queue_tail: u32,   // +0xb0
    pub auto_target: u32,        // +0xb8
    pub connected_unit: u32,     // +0xc0
    pub order_queue_count: u8,   // +0xc8
    pub order_process_timer: u8, // +0xc9
    pub last_attacking_player: u8, // +0xd4
    pub secondary_order_timer: u8, // +0xd5
    pub ai_action_flag: u8,      // +0xd6
    pub user_action_flags: u8,   // +0xd7
    pub button_set: u16,         // +0xd8
    pub is_cloaked: u8,          // +0xda
    pub movement_state: u8,      // +0xdb
    pub build_queue: [u16; 5],   // +0xdc
    pub energy: u16,             // +0xe6
    pub build_queue_slot: u8,    // +0xe8
    pub unique_id: u8,           // +0xe9
    pub secondary_order: u8,     // +0xea
    pub building_overlay_state: u8, // +0xeb
    pub hp_gain: u16,            // +0xec
    pub shield_gain: u16,        // +0xee
    pub remaining_build_time: u16, // +0xf0
    pub previous_hp: u16,        // +0xf2
    pub status: u32,             // +0x140
    pub resource_type: u8,       // +0x144
    pub wireframe_seed: u8,      // +0x145
    pub secondary_order_state: u8, // +0x146
    pub recent_order_timer: u8,  // +0x147
    pub visibility: i32,         // +0x148
    pub current_build_unit: u32, // +0x158
    pub path: u32,               // +0x178
    /// Resource containers: amount left (worker union at +0x110.. holds it for minerals/gas).
    pub resource_amount: u16,    // +0x128 (u16 view)
    pub is_being_healed: u8,     // +0x193
    pub stim_timer: u8,          // +0x1a1
    pub ensnare_timer: u8,       // +0x1a2
    pub remove_timer: u16,       // +0x19c
    pub status_timer_counter: u8, // +0x1b2
    pub ai: u32,                 // +0x1c8
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
            order_timer: 0,
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
            last_attacking_player: 0,
            secondary_order_timer: 0,
            ai_action_flag: 0,
            user_action_flags: 0,
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
            status: 0,
            resource_type: 0,
            wireframe_seed: 0,
            secondary_order_state: 0,
            recent_order_timer: 0,
            visibility: 0,
            current_build_unit: NIL,
            path: NIL,
            resource_amount: 0,
            is_being_healed: 0,
            stim_timer: 0,
            ensnare_timer: 0,
            remove_timer: 0,
            status_timer_counter: 0,
            ai: NIL,
        }
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
