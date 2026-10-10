//! Faithful port of the SC:R (BW 1.16-compatible) game engine.
//!
//! Structures mirror the binary's (field offsets noted), functions are
//! translated from the decompiled routines (addresses noted). Unported
//! callees are explicit `todo_port!` markers, never approximations.

pub mod create;
pub mod crt;
pub mod finder;
pub mod finish;
pub mod flingy;
pub mod frame;
pub mod iscript;
pub mod movement;
pub mod order_exec;
pub mod orders;
pub mod path;
pub mod players;
pub mod list;
pub mod map;
pub mod load;
pub mod rng;
pub mod sai;
pub mod sprite;
pub mod unit;

use crate::dat::Dat;
use list::{List, NIL};
use sprite::{Image, Sprite, Thingy, MAX_IMAGES, MAX_SPRITES, MAX_THINGIES};
use unit::Unit;

/// Marks a call into a routine that has not been ported yet.
#[macro_export]
macro_rules! todo_port {
    ($addr:literal, $what:literal) => {{
        #[cfg(debug_assertions)]
        eprintln!(concat!("todo_port ", $addr, " ", $what));
    }};
}

/// Read-only game data (dat tables, iscript, GRP sizes, LO overlay offsets).
pub struct Data<'a> {
    pub dat: &'a Dat,
    pub iscript: &'a [u8],
    /// (frame count, width, height) per images.dat id.
    pub grp: &'a dyn Fn(u16) -> (u16, u16, u16),
    /// LO files per [type 0..=4 (attack, damage, special, landing, liftoff)][image id].
    pub lo: &'a [Vec<Option<Vec<u8>>>; 5],
}

impl Data<'_> {
    pub fn grp_size(&self, image_id: u16) -> (u16, u16) {
        let (_, w, h) = (self.grp)(image_id);
        (w, h)
    }

    pub fn grp_frames(&self, image_id: u16) -> u16 {
        (self.grp)(image_id).0
    }

    pub fn iscript_u8(&self, at: usize) -> u8 {
        self.iscript.get(at).copied().unwrap_or(0)
    }

    pub fn iscript_u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.iscript_u8(at), self.iscript_u8(at + 1)])
    }

    /// 0x14018e5c0: header offset for an iscript id (table at the u32 at offset 0).
    pub fn iscript_header(&self, id: u32) -> Option<u16> {
        let mut p = u32::from_le_bytes(self.iscript.get(0..4)?.try_into().ok()?) as usize;
        loop {
            let k = self.iscript_u16(p);
            if k as u32 == id {
                return Some(self.iscript_u16(p + 2));
            }
            if k == 0xffff {
                return None;
            }
            p += 4;
        }
    }

    /// LO offset (x, y) for overlay `index` of `frame` in image `image_id`'s LO file of `kind`.
    pub fn lo_offset(&self, kind: usize, image_id: u16, frame: u16, index: i8) -> Option<(i8, i8)> {
        let lo = self.lo.get(kind)?.get(image_id as usize)?.as_ref()?;
        let o = u32::from_le_bytes(lo.get(8 + frame as usize * 4..12 + frame as usize * 4)?.try_into().ok()?) as usize;
        let at = (o as isize + index as isize * 2) as usize;
        Some((*lo.get(at)? as i8, *lo.get(at + 1)? as i8))
    }
}

pub struct State {
    pub images: Vec<Image>,
    pub free_images: List,
    pub sprites: Vec<Sprite>,
    pub free_sprites: List,
    /// Per tile-row sprite lists (heads 0x140e76c90, tails 0x140e77490).
    pub sprite_rows: Vec<List>,
    pub thingies: Vec<Thingy>,
    pub free_thingies: List,
    pub active_thingies: List,
    pub units: Vec<Unit>,
    pub free_units: List,      // 0x140e77fe0 / 0x140e77fd8
    pub visible_units: List,   // 0x140e77fc8 / 0x140e78078
    pub hidden_units: List,    // 0x140e77fe8 / 0x140e78088
    pub dead_units: List,      // 0x140e78080 / 0x140e77fd0
    pub unit_count: u32,       // 0x140e77ff8
    pub max_units: u32,        // 0x140e7e024
    /// First unit of each player's unit list (0x140e78000), linked via +0x90/+0x98.
    pub player_units: [u32; 12],
    pub rng: rng::Rng,
    /// Unit whose iscript is running (x86 0x6d11fc / x64 0x140e617d0), order unit (0x6d11f4 / 0x140e617d8),
    /// bullet (0x6d11f8 / 0x140e617e0).
    pub iscript_unit: Option<u32>,
    pub iscript_order_unit: Option<u32>,
    pub iscript_bullet: Option<u32>,
    pub map_px: (u16, u16),
    pub map_tiles: (usize, usize),
    pub tileset: u16,
    pub local_player: u8,
    pub replay: bool,
    pub reveal_all: bool,
    pub frame: u32,
    /// game_step 100-frame countdown (1.16.1 0x51ce98) and its "is 100th frame" flag (0x51ce9c).
    pub counter_100: u32,
    pub is_100th_frame: bool,
    /// 0x65eb10 random event countdown (random_event_tick 0x47e410).
    pub random_event_timer: u32,
    /// update_units stagger countdowns (150 / 300 frames).
    pub stagger_150: u32,
    pub stagger_300: u32,
    /// Unit finder search-id ring index (advances each frame).
    pub finder_ring: u32,
    /// Scanner sweep / special units list stepped last in update_units.
    pub scanner_units: List,
    /// 0x57eee8 player type per player (2 = human).
    pub player_types: [u8; 12],
    /// 0x658ae4: shared countdown for 0x4000-status dead-unit removal.
    pub dead_removal_counter: u32,
    /// 0x64dec8 ring rows, 0x64dea8, 0x6957d0.
    pub finder_rows: [[u32; 0x20]; 0x20],
    pub finder_64dea8: u32,
    pub var_6957d0: u32,
    /// 0x66ff70: unit moved to a new tile this frame (vision refresh).
    pub moved_flag: bool,
    /// CPath pool (unit +0x100).
    pub paths: Vec<movement::Path>,
    /// Unit finder (0x66ff74..).
    pub finder: finder::Finder,
    /// 0x6d5cd8: air repulse grid, 48px cells, 0xab per row.
    pub repulse: Vec<u8>,
    /// 0x59cca0: last error (stat_txt index) from unit creation and friends.
    pub last_error: u16,
    /// 0x57f0b0: local player's vision mask.
    pub local_vision_mask: u8,
    /// 0x58f440: playing Brood War (expansion) rules.
    pub is_expansion: bool,
    pub players: players::Players,
    /// COrder pool (0x6416a0), free list (0x64b2e0) and live count (0x641698).
    pub orders: Vec<orders::Order>,
    pub free_orders: List,
    pub order_count: u32,
    /// Flingy predict globals (0x63fec0..0x63ff40).
    pub mv: flingy::MoveScratch,
    /// Sounds requested this frame (sfx id, x, y) for the front end to play.
    pub sounds: Vec<(u16, i16, i16)>,
}

/// Free list built by inserting 1..n after element 0 with insert_after_head, as 1.16.1 does
/// for images (0x4d...), sprites (0x499900), thingies (0x488550) and orders (0x48c590):
/// allocation order is 0, n-1, n-2, ..., 1.
fn pool_after_head<T: Default + Clone + list::Node>(n: usize) -> (Vec<T>, List) {
    let mut v = vec![T::default(); n];
    let mut l = List::default();
    for i in 0..n as u32 {
        list::insert_after_head(&mut v, i, &mut l);
    }
    (v, l)
}

/// Free list in index order (units, 0x49e1f0).
fn pool<T: Default + Clone + list::Node>(n: usize) -> (Vec<T>, List) {
    // Free lists start in index order: element i -> i+1.
    let mut v = vec![T::default(); n];
    for i in 0..n {
        let l = v[i].link_mut();
        l.prev = if i == 0 { NIL } else { i as u32 - 1 };
        l.next = if i + 1 == n { NIL } else { i as u32 + 1 };
    }
    (v, List { head: 0, tail: n as u32 - 1 })
}

impl State {
    pub fn new(map_tiles: (usize, usize), tileset: u16, max_units: u32) -> State {
        let (images, free_images) = pool_after_head::<Image>(MAX_IMAGES);
        let (sprites, free_sprites) = pool_after_head(MAX_SPRITES);
        let (thingies, free_thingies) = pool_after_head(MAX_THINGIES);
        let (units, free_units) = pool(max_units as usize);
        let (orders, free_orders) = pool_after_head::<orders::Order>(orders::MAX_ORDERS);
        debug_assert!(free_images.head == 0 && images[0].link.next == MAX_IMAGES as u32 - 1);
        State {
            images,
            free_images,
            sprites,
            free_sprites,
            sprite_rows: vec![List::default(); map_tiles.1.max(1)],
            thingies,
            free_thingies,
            active_thingies: List::default(),
            units,
            free_units,
            visible_units: List::default(),
            hidden_units: List::default(),
            dead_units: List::default(),
            unit_count: 0,
            max_units,
            player_units: [NIL; 12],
            rng: rng::Rng::default(),
            iscript_unit: None,
            iscript_order_unit: None,
            iscript_bullet: None,
            map_px: ((map_tiles.0 * 32) as u16, (map_tiles.1 * 32) as u16),
            map_tiles,
            tileset,
            local_player: 0,
            replay: false,
            reveal_all: false,
            frame: 0,
            counter_100: 0,
            is_100th_frame: false,
            random_event_timer: 0,
            stagger_150: 1,
            stagger_300: 1,
            finder_ring: 0,
            scanner_units: List::default(),
            player_types: [0; 12],
            dead_removal_counter: 0,
            finder_rows: [[0; 0x20]; 0x20],
            finder_64dea8: 0,
            var_6957d0: 0,
            moved_flag: false,
            finder: finder::Finder::new(max_units as usize),
            repulse: vec![0; 0xab * 0xab],
            last_error: 0,
            local_vision_mask: 1,
            is_expansion: true,
            players: players::Players::default(),
            orders,
            free_orders,
            order_count: 0,
            paths: Vec::new(),
            mv: flingy::MoveScratch::default(),
            sounds: Vec::new(),
        }
    }

    pub fn host_is_hallucination(&self, u: u32) -> bool {
        self.units[u as usize].status & unit::status::HALLUCINATION != 0
    }

    pub fn host_is_cloaked(&self, u: u32) -> bool {
        self.units[u as usize].status & (unit::status::REQUIRES_DETECTION | unit::status::CLOAKED) != 0
    }

    pub fn player_type(&self, p: u8) -> u8 {
        self.player_types.get(p as usize).copied().unwrap_or(0)
    }

    pub fn host_unit_owner(&self, u: u32) -> u8 {
        self.units[u as usize].player
    }
}
