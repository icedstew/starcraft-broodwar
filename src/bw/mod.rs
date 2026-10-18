//! Faithful port of the SC:R (BW 1.16-compatible) game engine.
//!
//! Structures mirror the binary's (field offsets noted), functions are
//! translated from the decompiled routines (addresses noted). Unported
//! callees are explicit `todo_port!` markers, never approximations.

pub mod addon;
pub mod ai;
pub mod ai_areas;
pub mod ai_attack;
pub mod ai_military;
pub mod ai_place;
pub mod ai_region;
pub mod ai_script;
pub mod ai_spells;
pub mod ai_town;
pub mod attack;
pub mod battery;
pub mod build;
pub mod bullet;
pub mod carrier;
pub mod buttons;
pub mod archon;
pub mod burrow;
pub mod fighters;
pub mod chk_load;
pub mod cloak;
pub mod command;
pub mod collide;
pub mod create;
pub mod creep;
pub mod crt;
pub mod death;
pub mod doodads;
pub mod finder;
pub mod finish;
pub mod flight;
pub mod freeflingy;
pub mod flingy;
pub mod frame;
pub mod harvest;
pub mod iscript;
pub mod movement;
pub mod order_exec;
pub mod orders;
pub mod path;
pub mod path_req;
pub mod players;
pub mod powerups;
pub mod protoss;
pub mod psi;
pub mod requirements;
pub mod research;
pub mod larva;
pub mod zerg;
pub mod list;
pub mod medic;
pub mod mines;
pub mod nuke;
pub mod map;
pub mod minimap;
pub mod load;
pub mod rng;
pub mod sai;
pub mod select;
pub mod short_path;
pub mod siege;
pub mod spells;
pub mod sprite;
pub mod train;
pub mod transport;
pub mod transform;
pub mod triggers;
pub mod unit;
pub mod vision;

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

/// 0x4215d0 / 0x4212c0: fatal error: the game shows `msg` with the last system error
/// and exits.
pub fn fatal_error(msg: &str) -> ! {
    panic!("fatal error: {msg}")
}

/// Read-only game data (dat tables, iscript, GRP sizes, LO overlay offsets).
pub struct Data<'a> {
    pub dat: &'a Dat,
    pub iscript: &'a [u8],
    /// (frame count, width, height) per images.dat id.
    pub grp: &'a dyn Fn(u16) -> (u16, u16, u16),
    /// LO files per [type 0..=4 (attack, damage, special, landing, liftoff)][image id].
    pub lo: &'a [Vec<Option<Vec<u8>>>; 6],
    /// 0x6bb210 / 0x6bb5a0: AI air / ground strength per unit type (0x4313c0, from the
    /// dat tables before the scenario's settings).
    pub strength: &'a [[u32; 228]; 2],
    /// scripts\aiscript.bin / scripts\bwscript.bin.
    pub aiscript: &'a [u8],
    pub bwscript: &'a [u8],
    /// GRP files by images.dat grp index (empty when missing).
    pub grp_files: &'a [Vec<u8>],
}

impl Data<'_> {
    /// The GRP file of an image (the images.dat grp column), or empty.
    pub fn grp_file(&self, image_id: u16) -> &[u8] {
        let i = self.dat.images.grp.get(image_id as usize).copied().unwrap_or(0) as usize;
        self.grp_files.get(i).map_or(&[], |v| v.as_slice())
    }

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

    /// 0x4d6640 / SC:R 0x14018e5c0: header offset for an iscript id (table at the u32 at
    /// offset 0).
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
    /// UI image pools beside the 5000 sim images, in the same vec: 0x5244b8 (64, placement
    /// highlights, indices MAX_IMAGES..) and 0x57d768 (80, selection circles, after those).
    pub ui_hl_pool: List,
    pub ui_circle_pool: List,
    pub sprites: Vec<Sprite>,
    pub free_sprites: List,
    /// Per tile-row sprite lists (heads 0x140e76c90, tails 0x140e77490).
    pub sprite_rows: Vec<List>,
    pub thingies: Vec<Thingy>,
    /// 0x63fee0: free flingies (one), active 0x63ff34, free 0x63ff38.
    pub free_flingies: Vec<freeflingy::FreeFlingy>,
    pub ff_active: List,
    pub ff_free: List,
    pub free_thingies: List,
    pub active_thingies: List,
    /// 0x652928 pool, 0x6509d0 free and 0x654868 active lists: the local player's fogged
    /// copies of buildings and resources (`hp` holds the unit type).
    pub fog_thingies: Vec<Thingy>,
    pub free_fog_thingies: List,
    pub active_fog_thingies: List,
    /// 0x652918: placeholder sprite 0x13e made by 0x488550.
    pub marker_thingy: Option<u32>,
    /// 0x652920: the right-click marker is shown (drawn by 0x488180 though hidden).
    pub marker_shown: bool,
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
    /// Watching a replay: 0x6cdfd4 game speed (0..=6), 0x50e058 speed multiplier,
    /// 0x6d11b0 paused, 0x6d0f31 the replay's length in frames.
    pub replay_speed: u8,
    pub replay_mult: u8,
    pub replay_paused: bool,
    pub replay_frames: u32,
    /// 0x58d700: a player's game result once settled (2 defeated / left, 3 victorious,
    /// 4 ignored, 5 draw).
    pub player_left: [u8; 12],
    /// Trigger state (0x51a280 lists, timers, switches, locations).
    pub trig: triggers::Triggers,
    /// Computer player AI.
    pub ai: ai::Ai,
    pub reveal_all: bool,
    /// 0x6d5bd0: a building or door changed the map (read by the AI timers 0x4a2a40).
    pub map_changed: bool,
    pub frame: u32,
    /// game_step 100-frame countdown (1.16.1 0x51ce98) and its "is 100th frame" flag (0x51ce9c).
    pub counter_100: u32,
    pub is_100th_frame: bool,
    /// 0x65eb10 random event countdown (random_event_tick 0x47e410).
    pub random_event_timer: u32,
    /// Creep tiles waiting to recede (0x658ae8 / 0x659b10 / 0x658b10).
    pub creep_rec: creep::Recession,
    /// update_units stagger countdowns (150 / 300 frames).
    pub stagger_150: u32,
    pub stagger_300: u32,
    /// 0x64eec8: current frame row of `lurker_hits` (advances each frame).
    pub lurker_hits_frame: u32,
    /// Scanner sweep / special units list stepped last in update_units.
    pub scanner_units: List,
    /// 0x57eee8 player type per player (2 = human).
    pub player_types: [u8; 12],
    /// 0x57eee9 (player struct +9): race (0 zerg, 1 terran, 2 protoss).
    pub player_races: [u8; 12],
    /// Player struct +0xb (0x57eeeb): the name.
    pub player_names: [String; 12],
    /// 0x57eeea (player struct +2): force 1..4.
    pub player_force: [u8; 12],
    /// 0x58d5b8: FORC force properties (1 random start, 2 allies, 4 allied victory, 8 shared vision).
    pub force_flags: [u8; 4],
    /// 0x57f1ec: players whose vision each player shares (bit per player).
    pub player_vision: [u32; 12],
    /// 0x657910 overrides (weapons.dat forward offsets changed by attkshiftproj).
    pub forward_offsets: Vec<u8>,
    /// 0x6d5a6c: game option flags (2 = fast build, 4 = no damage, 0x20000 = unlimited supply).
    pub game_options: u32,
    /// 0x63ff5c: head of the units linked through +0xf0 / +0xf4.
    pub cloak_list_head: u32,
    /// Bullets (0x64b2e8 pool).
    pub bullets: bullet::Bullets,
    /// 0x513a18: sight shapes per sight range.
    pub sights: Vec<vision::Sight>,
    /// 0x596865: the game template (a Templates\\*.got entry): +0 game type, +2 sub
    /// type, +8 victory condition (0x59686d), +0xc starting units (0x596871), +0xf
    /// alliances allowed (0x596874), +0x10 teams (0x596875), +0x12 (0x596877).
    pub template: [u8; 0x20],
    /// 0x658ae4: shared countdown for 0x4000-status dead-unit removal.
    pub dead_removal_counter: u32,
    /// 0x64dec8: lurker spine hits, 32 frames of 16 (lurker + 1, target + 1) pairs (0 = empty);
    /// 0x64dea8: pairs used in the current frame.
    pub lurker_hits: [[u32; 0x20]; 0x20],
    pub lurker_hits_count: u32,
    /// 0x6957d0.
    pub var_6957d0: u32,
    /// 0x66ff70: unit moved to a new tile this frame (vision refresh).
    pub moved_flag: bool,
    /// CPath pool (0x6bee8c, unit +0x100) and free list head (0x6bee84, index + 1).
    pub paths: Vec<path_req::Path>,
    pub free_path: u32,
    /// Pathing regions (0x6d5bfc).
    pub sai: sai::Sai,
    /// Map tiles and tile flags (Gamemap.cpp).
    pub terrain: map::GameMap,
    /// Waypoint search context (0x6c1038).
    pub walker: short_path::Walker,
    /// Unit finder (0x66ff74..).
    pub finder: finder::Finder,
    /// 0x6bee88: unit a path request may route through (the one collided with).
    pub path_avoid_unit: Option<u32>,
    /// 0x6bee80: path requests treat every unit as an obstacle.
    pub path_all_units: bool,
    /// 0x58d634: alliances [player][other] (0 = enemy).
    pub alliances: [[u8; 12]; 12],
    /// 0x6284e8: each player's selection as the simulation sees it (NIL-terminated).
    pub selections: [[u32; 12]; 8],
    /// 0x6284b8: the local (client) selection, at most 12 units (a unit with a subunit is
    /// held by its subunit, 0x49ae40).
    pub selected: Vec<u32>,
    /// 0x57f21c: each player slot's color slot (identity unless the colors are shuffled).
    pub color_slot: [u32; 8],
    /// 0x581d6a: each player's selection circle color row in tselect.pcx (own 0, ally 1, enemy 2),
    /// rebuilt by 0x49b870.
    pub circle_color: [u8; 12],
    /// 0x58f442: color slot overrides for the advisor portrait (set by 0x4b2de0, 0 none).
    pub color_override: [u8; 8],
    /// 0x49f7a0 / 0x4ed000 emptied the selection: the front end ends placement
    /// (0x48d9a0 / 0x48e310, 0x640880) and targeting (0x48ca10, 0x641694).
    pub selection_emptied: bool,
    /// 0x57fe60: unit handle groups per [player * 18 + group]: 0..=9 hotkeys, 10..=17
    /// the recent selections.
    pub hotkeys: Vec<[u32; 12]>,
    /// 0x63fe2c: when groups 10..=17 were last used, per [player][group - 10] (net turns).
    pub recent_used: [[u16; 8]; 12],
    /// 0x58d720: start locations from the scenario.
    pub start_locations: [(i16, i16); 8],
    /// 0x59bda8: per-player "use map-placed units" setting.
    pub melee_map_units: [u8; 8],
    /// Unit box search results and marks (0x6bb938, 0x6bd3d0).
    pub search: collide::UnitSearch,
    /// 0x6408f8: building placement result grids (build::placement_check).
    pub placement: Vec<u8>,
    /// 0x6d5cd8: air repulse grid, 48px cells, 0xab per row.
    pub repulse: Vec<u8>,
    /// 0x59cca0: last error (stat_txt index) from unit creation and friends.
    pub last_error: u16,
    /// Error the front end should show the local player (stat_txt id + 1), set by 0x49e530.
    pub shown_error: u16,
    /// Trigger texts for the front end (0x4c5770): map string id and the action's
    /// "always display" flag (else shown only with subtitles on, 0x6cdfec 0x400).
    /// The third field is the least time in ms it stays (0 = the text's reading time).
    pub shown_text: Vec<(u16, bool, u32)>,
    /// 0x45edd0 for the front end: a transmission starts (unit type, length in ms): the
    /// talking portrait shows for that long and the real-time timer then calls
    /// `trigger_wait_end` (0x45e610).
    pub transmissions: Vec<(u16, u32)>,
    /// 0x47eb70 for the front end: the screen scrolls to this top-left (x, y) and then calls
    /// `trigger_wait_end` (callback 0x4894a0).
    pub center_view: Option<(i32, i32)>,
    /// A front end drives the real-time timers above (set by `Scene::portrait_tick` with a
    /// nonzero `ms`); headless runs have none.
    pub ui_attached: bool,
    /// Notices for the front end's message lines (0x48d0c0): a network.tbl format
    /// (0-based) and the player its %s (name) or %d (number) names.
    pub notices: Vec<(u16, u8)>,
    /// The pause command greyed the palette (0x41e930 from 0x4c0bc0; 0x4c0b00 restores it).
    pub palette_grey: bool,
    /// 0x48d140 requests: a unit (type, x, y) that could not be placed; the front end prints
    /// network.tbl line 0x11 with the type's name (0x47b090).
    pub unplaced: Vec<(u16, i16, i16)>,
    /// Chat packets (0x5c) received, (sender slot, text) for 0x48d1c0 in the front end.
    pub chat: Vec<(u8, Vec<u8>)>,
    /// 0x66ff60: why the last requirement check failed (soft failures, Ok(-1), set it
    /// here; hard ones return it as the error).
    pub req_error: std::cell::Cell<u8>,
    /// 0x6957a0: the "any target will do" flag the AI spell target wrappers (0x442660 ...) leave
    /// for their predicates (read stale by some).
    pub ai_spell_flag: u32,
    /// 0x6d5dcc: builder 0x473fb0 checks units against instead of its own (set by the
    /// AI's spot search 0x4465c0).
    pub builder_override: u32,
    /// 0x692688 resource areas (slot 0 unused), 0x695568 their count, 0x69556c the
    /// recount timer.
    pub areas: Vec<ai_areas::ResourceArea>,
    pub area_count: u32,
    pub area_timer: u32,
    /// 0x6ceb20: the interface colors in the current palette (set by the front end).
    pub interface_colors: [u8; 26],
    /// 0x640870: frames until the next under-attack alert may play; 0x6d63cc: tick phase.
    pub alert_cooldown: u32,
    pub alert_tick_counter: u32,
    /// 0x57f0b0: local player's vision mask.
    pub local_vision_mask: u8,
    /// MSVC `rand` state (`_holdrand`), seeded with the game seed by 0x4a2c20.
    pub crt_seed: u32,
    /// 0x57f1d0 / 0x57f1d2: screen position in tiles (UI; read by 0x49c4c0).
    pub screen_tile: (u16, u16),
    /// 0x58f440: playing Brood War (expansion) rules.
    pub is_expansion: bool,
    pub players: players::Players,
    pub psi: psi::Psi,
    /// 0x662db8 after 0x49e010: units.dat target acquisition ranges, raised to weapon ranges.
    pub acquisition: Vec<u8>,
    /// COrder pool (0x6416a0), free list (0x64b2e0) and live count (0x641698).
    pub orders: Vec<orders::Order>,
    pub free_orders: List,
    pub order_count: u32,
    /// Flingy predict globals (0x63fec0..0x63ff40).
    pub mv: flingy::MoveScratch,
    /// Sounds requested this frame (sfx id, x, y) for the front end to play.
    pub sounds: Vec<(u16, i16, i16)>,
    /// Interface sounds (sfx ids) for the local player, such as advisor errors.
    pub ui_sounds: Vec<u16>,
    /// Trigger wavs for the local player (0x4c77d0): the map string id of the file's path
    /// in the scenario archive.
    pub trigger_wavs: Vec<u16>,
    /// Advisor and unit speech (0x48ed50): sfx id and the sound option bit it needs
    /// (0x6cdfec: 1 unit speech, 4 building sounds; 0 = always), the speaking unit (NIL for
    /// none) and the base volume.
    pub speech: Vec<(u16, u32, u32, i32)>,
    /// 0x48ed50 calls from the sim that passed its vision gate (State::unit_sound): sfx id,
    /// the sounding unit, the minimum volume and its sprite position (x, y).
    pub unit_sounds: Vec<(u16, u32, i32, i16, i16)>,
    /// 0x45efe0 calls this frame for the UI: (talking unit, sfx id).
    pub talked: Vec<(u32, u16)>,
    /// 0x4a34c0 calls this frame for the minimap: (sprite x, sprite y, type).
    pub minimap_pings: Vec<(i32, i32, u8)>,
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


impl State {
    pub fn new(map_tiles: (usize, usize), tileset: u16, max_units: u32) -> State {
        let (mut images, free_images) = pool_after_head::<Image>(MAX_IMAGES);
        images.resize(sprite::UI_HL_END, Image::default());
        let (mut ui_hl_pool, mut ui_circle_pool) = (List::default(), List::default());
        for i in MAX_IMAGES..sprite::UI_HL_END {
            let l = if i < sprite::UI_HL_START + 64 { &mut ui_hl_pool } else { &mut ui_circle_pool };
            list::insert_after_head(&mut images, i as u32, l);
        }
        let (sprites, free_sprites) = pool_after_head(MAX_SPRITES);
        let (thingies, free_thingies) = pool_after_head(MAX_THINGIES);
        let (fog_thingies, free_fog_thingies) = pool_after_head(MAX_THINGIES);
        // New game (0x49f380) builds the unit free list like the other pools.
        let (units, free_units) = pool_after_head::<Unit>(max_units as usize);
        let (orders, free_orders) = pool_after_head::<orders::Order>(orders::MAX_ORDERS);
        debug_assert!(free_images.head == 0 && images[0].link.next == MAX_IMAGES as u32 - 1);
        let mut st = State {
            images,
            free_images,
            ui_hl_pool,
            ui_circle_pool,
            sprites,
            free_sprites,
            sprite_rows: vec![List::default(); map_tiles.1.max(1)],
            thingies,
            free_flingies: vec![freeflingy::FreeFlingy::default()],
            ff_active: List::default(),
            ff_free: List { head: 0, tail: 0 },
            free_thingies,
            active_thingies: List::default(),
            fog_thingies,
            free_fog_thingies,
            active_fog_thingies: List::default(),
            marker_thingy: None,
            marker_shown: false,
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
            map_changed: false,
            frame: 0,
            counter_100: 0,
            is_100th_frame: false,
            random_event_timer: 0,
            creep_rec: creep::Recession::new(),
            stagger_150: 10, // 0x59cca4 = 10 (0x49f380)
            stagger_300: 0x96, // 0x6283e8 = 0x96
            lurker_hits_frame: 0,
            scanner_units: List::default(),
            player_types: [0; 12],
            player_races: [0; 12],
            player_names: Default::default(),
            player_force: [0; 12],
            force_flags: [0; 4],
            player_vision: [0; 12],
            sights: Vec::new(),
            bullets: Default::default(),
            cloak_list_head: NIL,
            game_options: 0,
            forward_offsets: Vec::new(),
            template: [0; 0x20],
            player_left: [0; 12],
            trig: Default::default(),
            ai: Default::default(),
            replay_speed: 6,
            replay_mult: 1,
            replay_paused: false,
            replay_frames: 0,
            dead_removal_counter: 0,
            lurker_hits: [[0; 0x20]; 0x20],
            lurker_hits_count: 0,
            var_6957d0: 0,
            moved_flag: false,
            finder: finder::Finder::new(max_units as usize),
            repulse: vec![0; 0xab * 0xab],
            last_error: 0,
            shown_error: 0,
            shown_text: Vec::new(),
            transmissions: Vec::new(),
            center_view: None,
            ui_attached: false,
            notices: Vec::new(),
            palette_grey: false,
            unplaced: Vec::new(),
            chat: Vec::new(),
            speech: Vec::new(),
            unit_sounds: Vec::new(),
            talked: Vec::new(),
            minimap_pings: Vec::new(),
            req_error: std::cell::Cell::new(0),
            ai_spell_flag: 0,
            builder_override: NIL,
            areas: vec![ai_areas::ResourceArea::default(); ai_areas::AREA_SLOTS],
            area_count: 0,
            area_timer: 0,
            interface_colors: [0; 26],
            alert_cooldown: 0,
            alert_tick_counter: 0,
            local_vision_mask: 1,
            crt_seed: 1,
            screen_tile: (0, 0),
            is_expansion: true,
            players: players::Players::default(),
            psi: psi::Psi { providers: list::NIL, ..Default::default() },
            acquisition: Vec::new(),
            orders,
            free_orders,
            order_count: 0,
            paths: Vec::new(),
            free_path: 0,
            search: collide::UnitSearch::default(),
            placement: vec![0; build::PLACEMENT_SLOTS * 0x30],
            start_locations: [(0, 0); 8],
            alliances: [[0; 12]; 12],
            selections: [[NIL; 12]; 8],
            selected: Vec::new(),
            color_slot: [0, 1, 2, 3, 4, 5, 6, 7],
            circle_color: [0; 12],
            color_override: [0; 8],
            selection_emptied: false,
            hotkeys: vec![[0; 12]; 12 * 18],
            recent_used: [[0; 8]; 12],
            path_avoid_unit: None,
            path_all_units: false,
            melee_map_units: [0; 8],
            sai: sai::Sai::new(),
            terrain: map::GameMap::new(map_tiles.0 as u16, map_tiles.1 as u16, &map::TilesetData { vf4: Vec::new(), cv5: Vec::new() }),
            walker: short_path::Walker::default(),
            mv: flingy::MoveScratch::default(),
            sounds: Vec::new(),
            ui_sounds: Vec::new(),
            trigger_wavs: Vec::new(),
        };
        st.path_pool_init();
        st
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
