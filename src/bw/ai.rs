//! Computer player AI (tai_*.cpp, sai_*.cpp): unit strengths, player AI data, AI
//! regions and the per-frame update (ai_update 0x4a2b00).
//!
//! Memory map and field names: agent/re-notes.md ("AI memory map").

use super::list::NIL;
use super::{Data, State};
use crate::dat::Dat;

/// 0x431150: a unit type's weapon strength, adjusted (workers and scourge count less,
/// zealots and mutalisks double, mines / interceptors / scarabs nothing).
fn adjust_strength(t: usize, v: u32) -> u32 {
    match t {
        7 | 0x29 | 0x40 => v >> 2,
        13 | 0x49 | 0x55 => 0,
        0x20 | 0x2b | 0x41 => v * 2,
        0x2f | 0x32 => v >> 4,
        0x53 => v / 10,
        _ => v,
    }
}

/// 0x431270: strength of unit type `t` with weapon `w`: sqrt of its damage rate times
/// its durability, scaled by 7.58.
fn weapon_strength(dat: &Dat, t: usize, w: u8) -> u32 {
    let u = &dat.units;
    let sh = if u.shield_enable[t] != 0 { u.shields[t] as u32 } else { 0 };
    let life = ((u.hit_points[t] as i32 >> 8) as u32).wrapping_add(sh);
    if life == 0 {
        return 0;
    }
    let ws = &dat.weapons;
    let wi = w as usize;
    let factor = ws.damage_factor[wi] as u32;
    let cooldown = ws.cooldown[wi] as u32;
    let damage = ws.damage[wi] as u32;
    let range = (ws.max_range[wi] / cooldown).wrapping_mul(factor).wrapping_mul(damage);
    let rate = if w == 0x82 { 0 } else { (((factor * damage) << 11) as i32 / cooldown as i32) as u32 };
    let v = (rate.wrapping_mul(life) >> 8).wrapping_add(range);
    let mut f = v as i32 as f64;
    if (v as i32) < 0 {
        f += 4294967296.0;
    }
    // 0x4f7b70 (_ftol): truncation.
    (f.sqrt() * 7.58) as u32
}

/// 0x4313c0: air (0) and ground (1) strength of every unit type (the AI's 0x6bb210 /
/// 0x6bb5a0), from the turret's or hangar fighter's weapons. A unit with only the other
/// kind of weapon gets 0 instead of 1.
pub fn strength_table(dat: &Dat) -> [[u32; 228]; 2] {
    let mut out = [[0u32; 228]; 2];
    for t in 0..228usize {
        let skip = matches!(t, 0x23 | 0x24 | 0x3b | 0x61);
        let weapon_unit = || match t {
            0x48 | 0x52 => 0x49,
            0x53 | 0x51 => 0x55,
            _ => {
                let s = dat.units.subunit1[t];
                if s != 0xe4 { s as usize } else { t }
            }
        };
        let air = if skip {
            0
        } else {
            let wu = weapon_unit();
            let w = dat.units.air_weapon[wu];
            if w == 0x82 { 1 } else { adjust_strength(t, weapon_strength(dat, t, w)) }
        };
        let ground = if skip {
            0
        } else {
            let wu = weapon_unit();
            let w = dat.units.ground_weapon[wu];
            if w == 0x82 { 1 } else { adjust_strength(t, weapon_strength(dat, t, w)) }
        };
        out[0][t] = if air == 1 && ground > 1 { 0 } else { air };
        out[1][t] = if ground == 1 && air > 1 { 0 } else { ground };
    }
    out
}

impl State {
    /// 0x4026d0: current hp + shields (whole points).
    pub(super) fn life_now(&self, d: &Data, u: u32) -> i32 {
        let un = &self.units[u as usize];
        let sh = if d.dat.units.shield_enable[un.unit_type as usize] != 0 { un.shields >> 8 } else { 0 };
        ((un.hp + 0xff) >> 8) + sh
    }

    /// 0x401400: max hp in whole points (the current hp, at least 1, when the type has
    /// none).
    pub(super) fn max_hp_points(&self, d: &Data, u: u32) -> i32 {
        let un = &self.units[u as usize];
        let mut m = d.dat.units.hit_points[un.unit_type as usize] as i32 >> 8;
        if m == 0 {
            m = (un.hp + 0xff) >> 8;
            if m == 0 {
                m = 1;
            }
        }
        m
    }

    /// 0x4022c0: `u` is under 34% of its hit points.
    pub(super) fn hp_below_third(&self, d: &Data, u: u32) -> bool {
        let hp = (self.units[u as usize].hp + 0xff) >> 8;
        (hp.wrapping_mul(100) as u32) / (self.max_hp_points(d, u) as u32) < 0x22
    }

    /// 0x47b490: `u`'s owner is a rescuable or neutral (type 3 / 4) player.
    pub(super) fn owner_rescuable_or_neutral(&self, u: u32) -> bool {
        matches!(self.player_type(self.units[u as usize].player), 3 | 4)
    }

    /// 0x402c40: the hit points `u` gains per frame of construction (x16 with the
    /// fast-build cheat).
    pub(super) fn build_hp_gain(&self, u: u32) -> i32 {
        let gain = self.units[u as usize].hp_gain as i32;
        if self.game_options & 2 != 0 { gain << 4 } else { gain }
    }

    /// 0x401da0: `u` (and its turret) drop their order target.
    pub(super) fn clear_order_target(&mut self, d: &Data, u: u32) {
        self.units[u as usize].order_target_unit = NIL;
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.units[sub as usize].order_target_unit = NIL;
        }
    }

    /// 0x402710: max hp + max shields (whole points).
    pub(super) fn life_max(&self, d: &Data, u: u32) -> i32 {
        let t = self.units[u as usize].unit_type as usize;
        let sh = if d.dat.units.shield_enable[t] != 0 { d.dat.units.shields[t] as i32 } else { 0 };
        self.max_hp_points(d, u) + sh
    }

    /// 0x4e7170: space taken by `u`'s passengers.
    pub(super) fn loaded_space(&self, d: &Data, u: u32) -> u32 {
        (0..8)
            .filter_map(|i| self.unit_from_handle(self.units[u as usize].loaded_units[i] as u32))
            .map(|c| d.dat.units.space_required[self.units[c as usize].unit_type as usize] as u32)
            .sum()
    }

    /// 0x4316d0: `u`'s full-health strength of `kind` (0 air, 1 ground): a bunker counts
    /// its passengers as marines, casters add half their energy.
    pub(super) fn base_strength(&self, d: &Data, u: u32, kind: usize) -> u32 {
        let un = &self.units[u as usize];
        let t = un.unit_type as usize;
        let mut v = d.strength[kind][t];
        if t == 0x7d {
            v = self.loaded_space(d, u).wrapping_mul(d.strength[kind][0]);
        }
        if d.dat.units.flags[t] & 0x20_0000 != 0 && un.status & 0x4000_0000 == 0 {
            v = v.wrapping_add(((un.energy >> 8) as u32) >> 1);
        }
        v
    }

    /// 0x431800: `u`'s strength of `kind` scaled by its remaining hp + shields
    /// (damaged hallucinations count nothing).
    pub(super) fn unit_strength(&self, d: &Data, u: u32, kind: usize) -> u32 {
        let t = self.units[u as usize].unit_type;
        if matches!(t, 0x23 | 0x24 | 0x3b | 0x61) {
            return 0;
        }
        if self.units[u as usize].status & 0x4000_0000 != 0 && self.life_now(d, u) < self.life_max(d, u) {
            return 0;
        }
        let v = self.base_strength(d, u, kind);
        let now = self.life_now(d, u);
        let sh = if d.dat.units.shield_enable[t as usize] != 0 { d.dat.units.shields[t as usize] as i32 } else { 0 };
        let mut max = (self.max_hp_points(d, u) + sh) as u32;
        if max == 0 {
            max = 1;
        }
        (now as u32).wrapping_mul(v) / max
    }

    /// The `+0x138` / `+0x13a` refresh the hp and completion code does (0x431800(0/1)).
    pub(super) fn refresh_strength(&mut self, d: &Data, u: u32) {
        let air = self.unit_strength(d, u, 0) as u16;
        let ground = self.unit_strength(d, u, 1) as u16;
        let un = &mut self.units[u as usize];
        un.air_strength = air;
        un.ground_strength = ground;
    }
}

/// An AI pool entry's links (BW: +0 next, +4 prev); the pools hand out entries from a
/// free list kept in array order at start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiLink {
    pub next: u32,
    pub prev: u32,
}

impl Default for AiLink {
    fn default() -> AiLink {
        AiLink { next: NIL, prev: NIL }
    }
}

pub trait AiNode: Default + Clone {
    fn ai_link(&mut self) -> &mut AiLink;
}

/// A fixed AI pool (`AiArray`): 1000 guards / workers / buildings / military, 100 towns
/// and scripts.
#[derive(Debug, Clone)]
pub struct AiPool<T: AiNode> {
    pub items: Vec<T>,
    pub free: u32,
}

impl<T: AiNode> AiPool<T> {
    /// 0x402c60 / 0x403650 / 0x403db0 / 0x404280 / 0x404550 / 0x403130: every entry free,
    /// linked in array order.
    pub fn new(n: usize) -> AiPool<T> {
        let mut items = vec![T::default(); n];
        for (i, it) in items.iter_mut().enumerate() {
            let l = it.ai_link();
            l.prev = if i == 0 { NIL } else { i as u32 - 1 };
            l.next = if i + 1 == n { NIL } else { i as u32 + 1 };
        }
        AiPool { items, free: if n == 0 { NIL } else { 0 } }
    }

    /// Take the first free entry and put it first in the list `head`.
    pub fn alloc(&mut self, head: &mut u32) -> Option<u32> {
        let n = self.free;
        if n == NIL {
            return None;
        }
        let next_free = self.items[n as usize].ai_link().next;
        self.free = next_free;
        if next_free != NIL {
            self.items[next_free as usize].ai_link().prev = NIL;
        }
        *self.items[n as usize].ai_link() = AiLink { next: *head, prev: NIL };
        if *head != NIL {
            self.items[*head as usize].ai_link().prev = n;
        }
        *head = n;
        Some(n)
    }

    /// Unlink `n` from the list `head` and put it first in the free list.
    /// 0x404470: the worker pool's (free head at +24000).
    /// 0x404500: the building pool's (free head at +44000).
    pub fn release(&mut self, head: &mut u32, n: u32) {
        let AiLink { next, prev } = *self.items[n as usize].ai_link();
        if prev != NIL {
            self.items[prev as usize].ai_link().next = next;
        }
        if next != NIL {
            self.items[next as usize].ai_link().prev = prev;
        }
        if *head == n {
            *head = next;
        }
        *self.items[n as usize].ai_link() = AiLink { next: self.free, prev: NIL };
        if self.free != NIL {
            self.items[self.free as usize].ai_link().prev = n;
        }
        self.free = n;
    }
}

macro_rules! ai_node {
    ($($t:ident),*) => {$(impl AiNode for $t { fn ai_link(&mut self) -> &mut AiLink { &mut self.link } })*};
}

/// 0x20 bytes: a unit the AI keeps at a guard post.
#[derive(Clone, Default, Debug)]
pub struct GuardAi {
    pub link: AiLink,
    pub ai_type: u8,
    pub times_died: u8,
    pub parent: u32,
    pub unit_id: u16,
    pub home: (i16, i16),
    pub other_home: (i16, i16),
    pub previous_update: u32,
}

/// 0x18 bytes.
#[derive(Clone, Default, Debug)]
pub struct WorkerAi {
    pub link: AiLink,
    pub ai_type: u8,
    pub target_resource: u8,
    pub reassign_count: u8,
    pub wait_timer: u8,
    pub last_update_second: u32,
    pub parent: u32,
    pub town: u32,
}

/// 0x2c bytes.
#[derive(Clone, Default, Debug)]
pub struct BuildingAi {
    pub link: AiLink,
    pub ai_type: u8,
    pub train_queue_types: [u8; 5],
    pub parent: u32,
    pub town: u32,
    pub train_queue_values: [u32; 5],
}

/// 0x14 bytes.
#[derive(Clone, Default, Debug)]
pub struct MilitaryAi {
    pub link: AiLink,
    pub ai_type: u8,
    pub unk9: u8,
    pub unka: u16,
    pub parent: u32,
    /// (player, region index).
    pub region: (u8, u16),
}

/// 0x1cc bytes.
#[derive(Clone, Debug)]
pub struct AiTown {
    pub link: AiLink,
    pub workers: u32,
    pub buildings: u32,
    pub player: u8,
    pub inited: u8,
    pub worker_count: u8,
    pub worker_limit: u8,
    pub resource_area: u8,
    pub resource_units_not_set: u8,
    pub in_battle: u8,
    pub unk1f: u8,
    pub position: (i16, i16),
    pub main_building: u32,
    pub building_scv: u32,
    pub mineral: u32,
    pub gas_buildings: [u32; 3],
    /// (flags and count, priority, id)
    pub town_units: [(u8, u8, u16); 0x64],
}

impl Default for AiTown {
    fn default() -> AiTown {
        AiTown {
            link: AiLink::default(),
            workers: NIL,
            buildings: NIL,
            player: 0,
            inited: 0,
            worker_count: 0,
            worker_limit: 0,
            resource_area: 0,
            resource_units_not_set: 0,
            in_battle: 0,
            unk1f: 0,
            position: (0, 0),
            main_building: NIL,
            building_scv: NIL,
            mineral: NIL,
            gas_buildings: [NIL; 3],
            town_units: [(0, 0, 0); 0x64],
        }
    }
}

/// 0x34 bytes: a running AI script thread.
#[derive(Clone, Default, Debug)]
pub struct AiScript {
    pub link: AiLink,
    pub pos: u32,
    pub wait: u32,
    pub player: u32,
    pub area: [i32; 4],
    pub center: (i32, i32),
    pub town: u32,
    pub flags: u32,
}

ai_node!(GuardAi, WorkerAi, BuildingAi, MilitaryAi, AiTown, AiScript);

/// 0x34 bytes per region per player (0x69a604).
#[derive(Clone, Debug)]
pub struct AiRegion {
    pub id: u16,
    pub target_region_id: u16,
    pub player: u8,
    pub state: u8,
    pub defense_priority_add: u8,
    pub unk_count: u8,
    pub flags: u8,
    pub unk: u8,
    pub ground_unit_count: u16,
    pub needed_ground_strength: u16,
    pub needed_air_strength: u16,
    pub local_military_ground_strength: u16,
    pub local_military_air_strength: u16,
    pub all_military_ground_strength: u16,
    pub all_military_air_strength: u16,
    /// +0x18 / +0x1a
    pub enemy_air_strength: u16,
    pub enemy_ground_strength: u16,
    pub air_target: u32,
    pub ground_target: u32,
    pub slowest_military: u32,
    pub detector: u32,
    /// Its military AIs (list in the shared pool 0x6957e0).
    pub military: u32,
}

impl AiRegion {
    pub(super) fn new(id: u16, player: u8) -> AiRegion {
        AiRegion {
            id,
            target_region_id: 0,
            player,
            state: 0,
            defense_priority_add: 0,
            unk_count: 0,
            flags: 0,
            unk: 0,
            ground_unit_count: 0,
            needed_ground_strength: 0,
            needed_air_strength: 0,
            local_military_ground_strength: 0,
            local_military_air_strength: 0,
            all_military_ground_strength: 0,
            all_military_air_strength: 0,
            enemy_air_strength: 0,
            enemy_ground_strength: 0,
            air_target: NIL,
            ground_target: NIL,
            slowest_military: NIL,
            detector: NIL,
            military: NIL,
        }
    }
}

/// 8 bytes.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpendingRequest {
    pub priority: u8,
    pub ty: u8,
    pub id: u16,
    pub val: u32,
}

/// 0x4e8 bytes per player (0x68fee8).
#[derive(Clone, Debug)]
pub struct PlayerAi {
    pub mineral_need: u32,
    pub gas_need: u32,
    pub supply_need: u32,
    pub minerals_available: u32,
    pub gas_available: u32,
    pub supply_available: u32,
    pub requests: [SpendingRequest; 0x3f],
    pub request_count: u8,
    pub build_cooldown: u8,
    pub nuke_rate_minutes: u8,
    pub unk_attack: u8,
    pub previous_nuke_timer: u32,
    pub flags: u16,
    pub panic_script_pos: u16,
    pub max_region_strength_need: u16,
    pub attack_grouping_region: u16,
    pub train_unit_id: u16,
    pub default_min_strength_for_regions: u8,
    pub defense_priority_base: u8,
    pub previous_building_hit_second: u32,
    pub last_attack_second: u32,
    pub strategic_suicide_mission_cooldown: u8,
    pub spell_cooldown: u8,
    pub attack_failed: u8,
    pub difficulty: u8,
    pub attack_force: [u16; 0x40],
    pub defense_units: [[u16; 0x14]; 8],
    pub build_limits: [u8; 0xe4],
    pub free_medic: u32,
    pub gencmd_area: [i32; 4],
}

impl Default for PlayerAi {
    fn default() -> PlayerAi {
        PlayerAi {
            mineral_need: 0,
            gas_need: 0,
            supply_need: 0,
            minerals_available: 0,
            gas_available: 0,
            supply_available: 0,
            requests: [SpendingRequest::default(); 0x3f],
            request_count: 0,
            build_cooldown: 0,
            nuke_rate_minutes: 0,
            unk_attack: 0,
            previous_nuke_timer: 0,
            flags: 0,
            panic_script_pos: 0,
            max_region_strength_need: 0,
            attack_grouping_region: 0,
            train_unit_id: 0,
            default_min_strength_for_regions: 0,
            defense_priority_base: 0,
            previous_building_hit_second: 0,
            last_attack_second: 0,
            strategic_suicide_mission_cooldown: 0,
            spell_cooldown: 0,
            attack_failed: 0,
            difficulty: 0,
            attack_force: [0; 0x40],
            defense_units: [[0; 0x14]; 8],
            build_limits: [0; 0xe4],
            free_medic: NIL,
            gencmd_area: [0; 4],
        }
    }
}

/// Everything the AI keeps.
#[derive(Debug, Clone)]
pub struct Ai {
    pub players: Vec<PlayerAi>,
    /// 0x68fee4: the player 0x448920 looks at next.
    pub update_player: u32,
    /// 0x692628
    pub unk_692628: u8,
    /// 0x69262c (count 0x69265c): area candidates.
    pub area_candidates: Vec<u16>,
    /// 0x69a604: regions per player.
    pub regions: Vec<Vec<AiRegion>>,
    /// 0x6957e0: military AIs of every region.
    pub military: AiPool<MilitaryAi>,
    /// 0x67d400 and the per-player guard lists 0x685108.
    pub guards: AiPool<GuardAi>,
    pub guard_lists: [u32; 8],
    /// Workers 0x6b5448?, buildings, towns 0x6aa090 (lists 0x6aa050 per player).
    pub workers: AiPool<WorkerAi>,
    pub buildings: AiPool<BuildingAi>,
    pub towns: AiPool<AiTown>,
    pub town_lists: [u32; 8],
    /// 0x6aa04c: the player 0x435bd0 looks at next.
    pub town_update_player: u32,
    /// 0x6d63e8: 0x434330 is moving a town's units (no town is removed meanwhile).
    pub town_merging: bool,
    /// 0x69a628 (0x9c4 regions per player), 0x69f448 and 0x6957b0: 0x437070's region
    /// flood, when and from where it was made.
    pub flood: Vec<Vec<u8>>,
    pub flood_time: [u32; 8],
    pub flood_start: [u32; 8],
    /// 0x68aca8 script pool, 0x68c100 active scripts.
    pub scripts: AiPool<AiScript>,
    pub active_scripts: u32,
    /// 0x6d63e0: the call opcode's return position (one for all threads).
    pub script_return: u32,
    /// 0x6957d4 / 0x6957d8: 0x43fd80's step counter and player.
    pub region_step: u32,
    pub region_player: u32,
    /// 0x59cc8c / 0x59cc90 / 0x59cc94: 0x4a2a40's timers.
    pub timer_300: u32,
    pub timer_6: u32,
    pub timer_tick: u32,
}

impl Default for Ai {
    fn default() -> Ai {
        Ai {
            players: vec![PlayerAi::default(); 8],
            update_player: 0,
            unk_692628: 0,
            area_candidates: Vec::new(),
            regions: vec![Vec::new(); 8],
            military: AiPool::new(1000),
            guards: AiPool::new(1000),
            guard_lists: [NIL; 8],
            workers: AiPool::new(1000),
            buildings: AiPool::new(1000),
            towns: AiPool::new(100),
            town_lists: [NIL; 8],
            town_update_player: 0,
            town_merging: false,
            flood: vec![vec![0; 0x9c4]; 8],
            flood_time: [0; 8],
            flood_start: [0; 8],
            scripts: AiPool::new(100),
            active_scripts: NIL,
            script_return: 0,
            region_step: 0,
            region_player: 0,
            timer_300: 0xf,
            timer_6: 6,
            timer_tick: 0x3c,
        }
    }
}

impl State {
    /// 0x462a50: the guard pool (0x402c60) reset and the 8 per-player guard lists (0x685108..)
    /// emptied.
    pub fn init_ai_guards(&mut self) {
        self.ai.guards = AiPool::new(1000);
        self.ai.guard_lists = [NIL; 8];
    }

    /// 0x432610: the per-game town reset: the worker pool 0x6b5448 (0x403db0), the building
    /// pool 0x69f468 (0x404280) and the town pool 0x6aa090 (0x404550) all free, the 8 town
    /// lists 0x6aa050 emptied and the town update player 0x6aa04c cleared.
    pub fn init_ai_towns(&mut self) {
        self.ai.workers = AiPool::new(1000);
        self.ai.buildings = AiPool::new(1000);
        self.ai.towns = AiPool::new(100);
        self.ai.town_lists = [NIL; 8];
        self.ai.town_update_player = 0;
    }

    /// 0x447680: every player's AI data cleared; resources from the players' start.
    pub fn init_player_ai(&mut self) {
        let ai = &mut self.ai;
        ai.update_player = 0;
        for p in 0..8 {
            ai.players[p] = PlayerAi { minerals_available: self.players.minerals[p] as u32, gas_available: self.players.gas[p] as u32, difficulty: 1, ..PlayerAi::default() };
        }
        ai.unk_692628 = 1;
    }

    /// 0x437a70(0) / 0x437a70(1): the AI regions of every player, one per pathing region
    /// (after the scenario loads); the region minimum strength default is 100.
    pub fn init_ai_regions(&mut self) {
        let n = self.sai.region_count as usize;
        for p in 0..8 {
            self.ai.regions[p] = (0..n).map(|i| AiRegion::new(i as u16, p as u8)).collect();
            self.ai.players[p].default_min_strength_for_regions = 100;
        }
        self.ai.region_step = 0;
        self.ai.region_player = 0;
        // 0x437a70 also forgets the region floods (0x69a628, 0x69f448).
        self.ai.flood = vec![vec![0; 0x9c4]; 8];
        self.ai.flood_time = [0; 8];
    }
}

impl State {
    /// 0x4a2b00 ai_update (start of game_step).
    pub fn ai_update(&mut self, d: &Data) {
        self.run_ai_scripts(d);
        self.ai_region_steps(d);
        self.refresh_resource_areas(d);
        self.ai_timers(d);
    }

    /// 0x45cbd0: run the AI scripts whose wait is over (killed ones go back to the pool).
    fn run_ai_scripts(&mut self, d: &Data) {
        let mut s = self.ai.active_scripts;
        while s != NIL && self.ai.active_scripts != NIL {
            let next = self.ai.scripts.items[s as usize].link.next;
            let w = self.ai.scripts.items[s as usize].wait;
            self.ai.scripts.items[s as usize].wait = w.wrapping_sub(1);
            if w == 0 {
                if self.ai.scripts.items[s as usize].flags & 4 == 0 {
                    self.run_script(d, s);
                } else {
                    let mut head = self.ai.active_scripts;
                    self.ai.scripts.release(&mut head, s);
                    self.ai.active_scripts = head;
                }
            }
            s = next;
        }
    }

    /// 0x43fd80: 0x14d steps a frame over each computer player's regions (0x43fc60); a
    /// full sweep of 0x9c4 steps moves on to the next player, recounting enemy
    /// strength per region (0x43a250) and the player's attack timers (0x43b9e0).
    fn ai_region_steps(&mut self, d: &Data) {
        let rc = self.sai.region_count as u32;
        for _ in 0..0x14d {
            let p = self.ai.region_player as usize;
            if self.ai.region_step < rc && self.player_types[p] == 1 {
                self.ai_region_step(d, p as u8, self.ai.region_step as u16);
            }
            self.ai.region_step += 1;
            if self.ai.region_step == 0x9c4 {
                self.ai.region_player = (self.ai.region_player + 1) % 8;
                self.ai.region_step = 1;
                self.count_enemy_strength(d);
                let p = self.ai.region_player as usize;
                if self.player_types[p] == 1 && self.ai.players[p].flags & 0x20 == 0 {
                    self.ai_attack_timers(d, p as u8);
                }
            }
        }
    }

    /// 0x43a250: once per sweep (player 0), every region's enemy air / ground strength
    /// from scratch (0x438c20 per unit).
    fn count_enemy_strength(&mut self, _d: &Data) {
        if self.ai.region_player != 0 {
            return;
        }
        for p in 0..8 {
            for r in self.ai.regions[p].iter_mut().skip(1) {
                r.enemy_air_strength = 0;
                r.enemy_ground_strength = 0;
            }
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            if un.sprite != NIL && un.order != 0 {
                self.add_enemy_strength(u);
            }
            u = self.units[u as usize].link.next;
        }
    }

    /// 0x438c20: `u` counts as enemy strength in its region for each computer player
    /// not allied with its owner (a neutral unit counts for its sprite's owner).
    fn add_enemy_strength(&mut self, u: u32) {
        let un = &self.units[u as usize];
        let s = un.air_strength.max(un.ground_strength);
        if s == 0 {
            return;
        }
        let sp = &self.sprites[un.sprite as usize];
        let r = super::path::region_at(&self.sai, sp.x as i32, sp.y as i32) as usize;
        let owner = if un.player == 0xb { sp.owner } else { un.player };
        let air = un.status & 4 != 0;
        for q in 0..8usize {
            if self.player_types[q] != 1 || un.player as usize == q {
                continue;
            }
            if self.alliances[q][owner as usize] != 0 {
                continue;
            }
            let Some(reg) = self.ai.regions[q].get_mut(r) else { continue };
            if air {
                reg.enemy_air_strength = reg.enemy_air_strength.wrapping_add(s);
            } else {
                reg.enemy_ground_strength = reg.enemy_ground_strength.wrapping_add(s);
            }
        }
    }

    /// 0x4a2a40: every 300 * 6 frames (sooner after a map change) units lose status
    /// 0x80; every 0x25 frames (7 in the first 0x1194 seconds) the computer players' AI
    /// timers, towns and army comparison run.
    fn ai_timers(&mut self, d: &Data) {
        if self.ai.timer_300 == 0 {
            self.ai.timer_300 = 300;
            let reset = if self.ai.timer_6 == 0 {
                true
            } else {
                self.ai.timer_6 -= 1;
                self.map_changed
            };
            if reset {
                self.ai.timer_6 = 6;
                self.map_changed = false;
                let mut u = self.visible_units.head;
                while u != NIL {
                    self.units[u as usize].status &= !0x80;
                    u = self.units[u as usize].link.next;
                }
            }
        } else {
            self.ai.timer_300 -= 1;
        }
        let t = self.ai.timer_tick;
        self.ai.timer_tick = t.wrapping_sub(1);
        if t == 0 {
            self.ai.timer_tick = if self.trig.elapsed < 0x1194 { 7 } else { 0x25 };
            self.ai_player_tick(d);
            self.ai_town_tick(d);
            self.ai_compare_armies(d);
        }
    }

    /// 0x448920: one computer player's cooldowns and checks.
    fn ai_player_tick(&mut self, d: &Data) {
        self.ai.update_player = (self.ai.update_player + 1) % 8;
        let p = self.ai.update_player as usize;
        if self.player_types[p] != 1 || self.ai.players[p].flags & 2 == 0 {
            return;
        }
        let pa = &mut self.ai.players[p];
        pa.build_cooldown = pa.build_cooldown.saturating_sub(1);
        pa.strategic_suicide_mission_cooldown = pa.strategic_suicide_mission_cooldown.saturating_sub(1);
        pa.spell_cooldown = pa.spell_cooldown.saturating_sub(1);
        pa.attack_failed = pa.attack_failed.saturating_sub(1);
        self.ai_reset_budget(d, p as u8);
        self.ai_guard_requests(d, p as u8);
        if self.trig.elapsed > 9 {
            self.ai_region_requests(d, p as u8);
        }
        self.ai_town_requests(d, p as u8);
    }

    /// 0x434480: the minerals and gas of `player` once the buildings its workers are
    /// about to place (orders 0x1e / 0x1f / 0x46 / 0x1a / 0x19) are paid.
    fn ai_pending_build_costs(&mut self, d: &Data, player: u8) -> (i32, i32) {
        let p = player as usize;
        let (mut m, mut g) = (self.players.minerals[p], self.players.gas[p]);
        let mut town = self.ai.town_lists[p];
        while town != NIL {
            let mut w = self.ai.towns.items[town as usize].workers;
            while w != NIL {
                let u = self.ai.workers.items[w as usize].parent;
                let un = &self.units[u as usize];
                if matches!(un.order, 0x1e | 0x1f | 0x46 | 0x1a | 0x19) {
                    let t = un.build_queue[un.build_queue_slot as usize % 5];
                    // A probe's slot is already empty (0xe4) in PlaceProtossBuilding's last
                    // state: the u16 reads land past mineral_cost (0x663a50) in the
                    // ai_attack_move bytes and past gas_cost (0x65fec8) in the armor
                    // bytes; supply_required[0xe4] is padding (0), so supply passes.
                    let (bm, bg, supply_ok) = if (t as usize) < d.dat.units.mineral_cost.len() {
                        (d.dat.units.mineral_cost[t as usize] as i32, d.dat.units.gas_cost[t as usize] as i32, self.supply_ok_show(d, p, t))
                    } else {
                        let lo = |a: &[u8]| a[0] as i32 | (a[1] as i32) << 8;
                        (lo(&d.dat.units.ai_attack_move), lo(&d.dat.units.armor), true)
                    };
                    if supply_ok {
                        let race = self.player_races[self.local_player as usize] as u16;
                        if self.players.minerals[p] < bm {
                            self.local_error(player, 0x93 + race, 0x352);
                        } else if self.players.gas[p] < bg {
                            self.local_error(player, 0x96 + race, 0x353);
                        } else {
                            m = m.wrapping_sub(bm);
                            g = g.wrapping_sub(bg);
                        }
                    }
                }
                w = self.ai.workers.items[w as usize].link.next;
            }
            town = self.ai.towns.items[town as usize].link.next;
        }
        (m, g)
    }

    /// 0x448890: start a new round of spending: no requests or needs, flag 1, and what
    /// is available is the bank minus the buildings workers are about to place
    /// (`ai_pending_build_costs`) and the free supply of the race's worker.
    fn ai_reset_budget(&mut self, d: &Data, player: u8) {
        let p = player as usize;
        let pa = &mut self.ai.players[p];
        pa.flags |= 1;
        pa.mineral_need = 0;
        pa.gas_need = 0;
        pa.supply_need = 0;
        pa.request_count = 0;
        let (m, g) = self.ai_pending_build_costs(d, player);
        let worker = match self.player_races[p] {
            1 => 7,
            0 => 0x29,
            _ => 0x40,
        };
        let free = self.supply_free(d, p, worker);
        let pa = &mut self.ai.players[p];
        pa.minerals_available = m as u32;
        pa.gas_available = g as u32;
        pa.supply_available = free as u32;
        pa.defense_priority_base = 9;
    }

    /// 0x435bd0: the next player's towns take their step (0x435900); a player whose
    /// towns are all set up and running low on minerals (or rich) may expand (0x45b590).
    fn ai_town_tick(&mut self, d: &Data) {
        self.ai.town_update_player = (self.ai.town_update_player + 1) % 8;
        let p = self.ai.town_update_player as usize;
        if self.ai.town_lists[p] == NIL {
            return;
        }
        let (mut towns, mut with_gas, mut mines) = (0u32, 0u32, 0u32);
        let mut unready = false;
        let mut t = self.ai.town_lists[p];
        while t != NIL {
            towns += 1;
            if self.town_has_gas(t) {
                with_gas += 1;
            }
            let tw = &self.ai.towns.items[t as usize];
            if tw.inited == 0 || tw.main_building == NIL || self.units[tw.main_building as usize].status & 1 == 0 {
                unready = true;
            }
            self.town_step(d, t);
            let tw = &self.ai.towns.items[t as usize];
            if tw.resource_area != 0 {
                mines += self.area_mining_spots(tw.resource_area as u32, tw.player);
            }
            let tw = &mut self.ai.towns.items[t as usize];
            tw.in_battle = tw.in_battle.saturating_sub(1);
            t = tw.link.next;
        }
        if unready {
            return;
        }
        let low = if mines == 0 {
            towns > 1 && with_gas < 2
        } else if mines < 0x10 {
            true
        } else if towns > 1 {
            mines < 0x14 || with_gas < 2
        } else {
            false
        };
        let expand = low || (self.ai_rich(p as u8) && self.player_mining_spots(p as u8) < 0x18);
        if expand {
            self.ai_town_expand(d, p as u8);
        }
    }

    /// 0x45b590 (EDX = the player): when the player has a panic script position
    /// (0x690102), 0x45b530 runs for it with n = 2; the first town stands in for the
    /// script thread.
    fn ai_town_expand(&mut self, d: &Data, p: u8) {
        let first = self.ai.town_lists[p as usize];
        if first != NIL && self.ai.players[p as usize].panic_script_pos != 0 {
            let v = self.ai.players[p as usize].panic_script_pos;
            self.expand_from(d, p, first, false, v, 2);
        }
    }

    /// The town's first finished gas building with a worker in it, else its first geyser
    /// (0x435bd0 counts towns with one).
    fn town_has_gas(&self, t: u32) -> bool {
        let tw = &self.ai.towns.items[t as usize];
        let slots = tw.gas_buildings;
        let pl = tw.player;
        slots.iter().any(|&g| g != NIL && self.gas_state(g, pl) == 3) || slots.iter().any(|&g| g != NIL && self.gas_state(g, pl) == 1)
    }

    /// 0x4439b0: how many workers area `a` keeps busy on minerals: its fields, or with
    /// little left (minerals / 500 at least the player's town fields 0x432320) that
    /// amount capped at twice the fields.
    pub(super) fn area_mining_spots(&self, a: u32, player: u8) -> u32 {
        if a == 0 || a >= 0xfa {
            return 0;
        }
        let ar = &self.areas[a as usize];
        let left = ar.minerals / 500;
        if left < self.player_mining_spots(player) {
            return ar.mineral_fields as u32;
        }
        left.min(ar.mineral_fields as u32 * 2)
    }

    /// 0x432320: mineral fields in the areas of `player`'s towns.
    pub(super) fn player_mining_spots(&self, player: u8) -> u32 {
        let mut n = 0u32;
        let mut t = self.ai.town_lists[player as usize];
        while t != NIL {
            let a = self.ai.towns.items[t as usize].resource_area as usize;
            if a != 0 && a < 0xfa {
                n += self.areas[a].mineral_fields as u32;
            }
            t = self.ai.towns.items[t as usize].link.next;
        }
        n
    }

    /// 0x432360: 2000..5000 minerals, 1000..3000 gas and flag 0x200.
    fn ai_rich(&self, p: u8) -> bool {
        let (m, g) = (self.players.minerals[p as usize], self.players.gas[p as usize]);
        (2000..5000).contains(&m) && (1000..3000).contains(&g) && self.ai.players[p as usize].flags & 0x200 != 0
    }

    /// 0x4475b0: (gas, minerals) share in percent of what the AI still needs.
    pub(super) fn ai_need_split(&self, player: u8) -> (u8, u8) {
        let p = player as usize;
        let pa = &self.ai.players[p];
        if pa.gas_need == 0 {
            return (0, 100);
        }
        if pa.mineral_need == 0 {
            return (100, 0);
        }
        let g = pa.gas_need.wrapping_sub(self.players.gas[p] as u32) as i32;
        let m = pa.mineral_need.wrapping_sub(self.players.minerals[p] as u32) as i32;
        let ratio = (m > 0 && g > 0) || (m <= 0 && g <= 0);
        if !ratio {
            return if m > 0 { (0, 100) } else { (100, 0) };
        }
        let gas = (pa.gas_need.wrapping_mul(100) / pa.gas_need.wrapping_add(pa.mineral_need)) as u8;
        (gas, 100u8.wrapping_sub(gas))
    }

    /// 0x43e7d0: after 0x167 seconds (unless a computer has flag 0x20): compare each
    /// computer's ground strength with its enemies' and allies'; flag 2 (+0x219) when
    /// outnumbered, and 0x43e050 (give up / attack) when far ahead.
    fn ai_compare_armies(&mut self, d: &Data) {
        if self.trig.elapsed <= 0x167 {
            return;
        }
        if self.ai.players.iter().any(|p| p.flags & 0x20 != 0) {
            return;
        }
        let mut own = [0u32; 8];
        let mut enemy = [0u32; 8];
        let mut allied = [0u32; 8];
        let al = |a: usize, b: usize| self.alliances[a][b];
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &self.units[u as usize];
            let p = un.player as usize;
            if un.sprite != NIL && un.order != 0 && p < 8 {
                let v = un.ground_strength as u32;
                own[p] = own[p].wrapping_add(v);
                if d.dat.units.flags[un.unit_type as usize] & 9 == 0 {
                    for q in 0..8 {
                        if p == q || al(p, q) == 2 {
                            allied[q] = allied[q].wrapping_add(v);
                        }
                    }
                }
                for q in 0..8 {
                    if al(q, p) == 0 || al(p, q) == 0 {
                        enemy[q] = enemy[q].wrapping_add(v);
                    }
                }
            }
            u = self.units[u as usize].link.next;
        }
        let mut best = [0u32; 8];
        for p in 0..8 {
            for q in 0..8 {
                if al(p, q) == 0 && best[q] < allied[p] {
                    best[q] = allied[p];
                }
            }
        }
        for p in 0..8 {
            if self.player_types[p] != 1 {
                continue;
            }
            let pa = &mut self.ai.players[p];
            if best[p] < own[p] {
                pa.flags |= 0x200;
            } else {
                pa.flags &= !0x200;
            }
            if (allied[p] > 5999 || self.trig.elapsed > 0xa8b) && enemy[p] != 0 && enemy[p] * 2 < allied[p] {
                self.ai_attack_all_in(d, p as u8, 0);
            }
        }
    }
}

/// 0x51acc0: scripts any player's trigger may run.
const ALWAYS_ALLOWED_SCRIPTS: [&[u8; 4]; 21] = [
    b"EnBk", b"EnTr", b"ExTr", b"DWHe", b"PB3F", b"+Vi0", b"+Vi1", b"+Vi2", b"+Vi3", b"+Vi4", b"+Vi5", b"+Vi6", b"+Vi7", b"-Vi0",
    b"-Vi1", b"-Vi2", b"-Vi3", b"-Vi4", b"-Vi5", b"-Vi6", b"-Vi7",
];

fn rd32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map_or(0, |v| u32::from_le_bytes(v.try_into().unwrap()))
}

/// 0x45b170: a script's start: aiscript.bin's header entries (0x10 bytes: id, position)
/// first, then bwscript.bin's (8 bytes). Some((position, from bwscript.bin)).
pub fn find_ai_script(d: &Data, id: u32) -> Option<(u32, bool)> {
    let a = d.aiscript;
    let mut o = rd32(a, 0) as usize;
    loop {
        let k = rd32(a, o);
        if k == 0 {
            break;
        }
        if k == id {
            let pos = rd32(a, o + 4);
            if pos != 0 {
                return Some((pos, false));
            }
            break;
        }
        o += 0x10;
    }
    let b = d.bwscript;
    let mut o = rd32(b, 0) as usize;
    loop {
        let k = rd32(b, o);
        if k == 0 {
            return None;
        }
        if k == id {
            let pos = rd32(b, o + 4);
            return if pos != 0 { Some((pos, true)) } else { None };
        }
        o += 8;
    }
}

impl State {
    /// 0x45aea0: `player` may run script `id` (computers any, neutral only "Rscu").
    pub fn ai_script_allowed(&self, id: u32, player: u8) -> bool {
        if ALWAYS_ALLOWED_SCRIPTS.iter().any(|s| u32::from_le_bytes(**s) == id) {
            return true;
        }
        match self.player_type(player) {
            1 => true,
            7 => id == u32::from_le_bytes(*b"Rscu"),
            _ => false,
        }
    }

    /// 0x45aef0: start a script thread for `player` at `pos` (in bwscript.bin when
    /// `bw`), optionally over `area` (its centre recorded too).
    pub fn start_ai_script(&mut self, player: u32, pos: u32, bw: bool, area: Option<[i32; 4]>) -> Option<u32> {
        let mut head = self.ai.active_scripts;
        let s = self.ai.scripts.alloc(&mut head)?;
        self.ai.active_scripts = head;
        let sc = &mut self.ai.scripts.items[s as usize];
        sc.pos = pos;
        sc.player = player;
        sc.wait = 0;
        sc.flags = (sc.flags & !1) | bw as u32;
        match area {
            Some(a) => {
                sc.area = a;
                sc.center = (a[0] + (a[2] - a[0]) / 2, a[1] + (a[3] - a[1]) / 2);
            }
            None => {
                sc.area = [0; 4];
                sc.center = (0, 0);
            }
        }
        sc.town = NIL;
        sc.flags &= 7;
        Some(s)
    }

    /// 0x4c5720 / 0x4c56c0: RunAIScript (at location): the trigger's player runs the
    /// script if allowed.
    pub fn trigger_run_ai_script(&mut self, d: &Data, id: u32, area: Option<[i32; 4]>) {
        let p = self.trig.player;
        if !self.ai_script_allowed(id, p as u8) {
            return;
        }
        if let Some((pos, bw)) = find_ai_script(d, id) {
            self.start_ai_script(p, pos, bw, area);
        }
    }
}

impl State {
    /// 0x4abaf0 head: melee-style game types start the computers' race scripts
    /// (0x4ab230); other games mark every player's AI with flag 0x20.
    pub fn start_game_type_ai(&mut self, d: &Data) {
        let t = self.template;
        if matches!(t[0xe], 1 | 3) && (t[8] != 0 || t[0xc] != 0 || t[0x12] != 0) {
            self.start_melee_ai(d);
        } else {
            for p in self.ai.players.iter_mut() {
                p.flags |= 0x20;
            }
        }
    }

    /// 0x4ab230: each computer runs its race's melee script ("ZMCx" / "TMCx" / "PMCx";
    /// "..Cu" without Brood War) around its start location.
    fn start_melee_ai(&mut self, d: &Data) {
        for p in 0..8usize {
            if self.player_types[p] == 1 {
                self.start_melee_ai_for(d, p);
            }
        }
    }

    /// 0x4ab230's body for one computer `p`.
    pub(super) fn start_melee_ai_for(&mut self, d: &Data, p: usize) {
        let r = match self.player_races[p] {
            0 => b'Z',
            1 => b'T',
            2 => b'P',
            _ => return,
        };
        let id = u32::from_le_bytes([r, b'M', b'C', if self.is_expansion { b'x' } else { b'u' }]);
        let (x, y) = self.start_locations[p];
        let (x, y) = (x as i32, y as i32);
        if let Some((pos, bw)) = find_ai_script(d, id) {
            self.start_ai_script(p as u32, pos, bw, Some([x - 1, y - 1, x + 1, y + 1]));
        }
    }
}

/// 0x438bf0: an AI region as a request value (`player`'s region `id`).
pub fn ai_region_ref(player: u8, id: u16) -> u32 {
    (player as u32) << 16 | id as u32
}

/// Spending request types (`SpendingRequest::ty`).
pub const REQ_UNIT: u8 = 1;
pub const REQ_BUILDING: u8 = 3;
pub const REQ_UPGRADE: u8 = 5;
pub const REQ_TECH: u8 = 6;

/// The AI's spending requests: a max-heap on priority per player (0x68ff00, count +0x210).
impl State {
    /// 0x447830: `player` has reached its build limit for unit `id` (0xff: never build;
    /// siege mode tanks count as tanks).
    pub(super) fn ai_at_build_limit(&self, player: u8, id: u16) -> bool {
        let lim = self.ai.players[player as usize].build_limits[id as usize];
        if lim == 0 {
            return false;
        }
        if lim == 0xff {
            return true;
        }
        let c = &self.players.unit_counts;
        let mut n = c[id as usize][player as usize] as u32;
        if id == 5 {
            n = n.wrapping_add(c[0x1e][player as usize] as u32);
        }
        lim as u32 <= n
    }

    /// 0x447980: push a request (types 1, 2, 4 and 8 only below the build limit); a full
    /// heap overwrites its last slot.
    pub(super) fn ai_push_request(&mut self, player: u8, ty: u8, id: u16, priority: u8, val: u32) {
        if matches!(ty, 1 | 2 | 4 | 8) && self.ai_at_build_limit(player, id) {
            return;
        }
        let pa = &mut self.ai.players[player as usize];
        let mut i = if pa.request_count == 0x3f {
            0x3e
        } else {
            pa.request_count += 1;
            pa.request_count as usize - 1
        };
        pa.requests[i] = SpendingRequest { priority, ty, id, val };
        while i > 0 {
            let parent = (i - 1) / 2;
            if pa.requests[i].priority <= pa.requests[parent].priority {
                return;
            }
            pa.requests.swap(i, parent);
            i = parent;
        }
    }

    /// 0x4476d0: drop the top request.
    pub(super) fn ai_pop_request(&mut self, player: u8) {
        let pa = &mut self.ai.players[player as usize];
        pa.request_count = pa.request_count.wrapping_sub(1);
        let n = pa.request_count as usize;
        if n == 0 {
            return;
        }
        pa.requests[0] = pa.requests[n];
        if n == 1 {
            return;
        }
        let mut i = 0usize;
        loop {
            let (a, b) = (i * 2 + 1, i * 2 + 2);
            let c = if n <= b || pa.requests[b].priority < pa.requests[a].priority { a } else { b };
            if pa.requests[c].priority <= pa.requests[i].priority {
                return;
            }
            pa.requests.swap(i, c);
            i = c;
            if i * 2 + 1 > n - 1 {
                return;
            }
        }
    }

    /// 0x447590: type of the top request (0 when there is none).
    pub(super) fn ai_top_request_type(&self, player: u8) -> u8 {
        let pa = &self.ai.players[player as usize];
        if pa.request_count != 0 { pa.requests[0].ty } else { 0 }
    }

    /// 0x4487b0: `player` can pay for unit `id` now (errors shown as for a human) and
    /// out of what the AI has set aside (+0xc, +0x10, +0x14).
    pub(super) fn ai_can_afford(&mut self, d: &Data, player: u8, id: u16) -> bool {
        let p = player as usize;
        let (m, g) = (d.dat.units.mineral_cost[id as usize] as u32, d.dat.units.gas_cost[id as usize] as u32);
        if !self.supply_ok_show(d, p, id) {
            return false;
        }
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[p] < m as i32 {
            self.local_error(player, 0x93 + race, 0x352);
            return false;
        }
        if self.players.gas[p] < g as i32 {
            self.local_error(player, 0x96 + race, 0x353);
            return false;
        }
        // 0x6ca4bc: the supply 0x42cf70 worked out.
        let flags = d.dat.units.flags[id as usize];
        let mut s = d.dat.units.supply_required[id as usize] as u32;
        if flags & 0x400 != 0 {
            s *= 2;
        }
        if s != 0 && flags & 0x100_0000 == 0 && id == 0x67 {
            s = s.wrapping_sub(d.dat.units.supply_required[0x26] as u32);
        }
        let pa = &self.ai.players[p];
        m <= pa.minerals_available && g <= pa.gas_available && s <= pa.supply_available
    }

    /// 0x447b30 (0x42cdf0 for units): (minerals, gas, supply) of request `ty`/`id`.
    fn ai_request_cost(&self, d: &Data, player: u8, ty: u8, id: u16) -> (u32, u32, u8) {
        let i = id as usize;
        match ty {
            REQ_UPGRADE => {
                let (m, g, _) = self.upgrade_cost(d, player, id as u8);
                (m as u16 as u32, g as u16 as u32, 0)
            }
            REQ_TECH => (d.dat.techdata.mineral_cost[i] as u32, d.dat.techdata.gas_cost[i] as u32, 0),
            _ => {
                let u = &d.dat.units;
                let mut s = u.supply_required[i];
                if u.flags[i] & 0x400 != 0 {
                    s = s.wrapping_shl(1);
                }
                (u.mineral_cost[i] as u32, u.gas_cost[i] as u32, s)
            }
        }
    }

    /// 0x447dc0: add the cost of `ty`/`id` to the player's needs.
    pub(super) fn ai_add_need(&mut self, d: &Data, player: u8, ty: u8, id: u16) {
        let (m, g, s) = self.ai_request_cost(d, player, ty, id);
        let pa = &mut self.ai.players[player as usize];
        pa.mineral_need = pa.mineral_need.wrapping_add(m);
        pa.gas_need = pa.gas_need.wrapping_add(g);
        pa.supply_need = pa.supply_need.wrapping_add(s as u32);
    }

    /// 0x447d00: the top request is done with: take its cost off the needs (not for
    /// morphs, whose parts were requested separately) and, when `spent`, off what is
    /// available.
    pub(super) fn ai_request_done(&mut self, d: &Data, player: u8, spent: bool) {
        let top = self.ai.players[player as usize].requests[0];
        if matches!(top.id, 0x2c | 0x67 | 0x3e | 0x44 | 0x3f) {
            return;
        }
        let (m, g, s) = self.ai_request_cost(d, player, top.ty, top.id);
        let pa = &mut self.ai.players[player as usize];
        pa.mineral_need = pa.mineral_need.saturating_sub(m);
        pa.gas_need = pa.gas_need.saturating_sub(g);
        pa.supply_need = pa.supply_need.saturating_sub(s as u32);
        if spent {
            pa.minerals_available = pa.minerals_available.wrapping_sub(m);
            pa.gas_available = pa.gas_available.wrapping_sub(g);
            pa.supply_available = pa.supply_available.wrapping_sub(s as u32);
        }
    }

    /// 0x4485b0: request unit `id` (with the units a morph is made from, 0x4484a0).
    /// Without `force` nothing is requested while flag 0x200 is set; without `keep` flag
    /// 1 is cleared. Flag 0x40 is set.
    pub(super) fn ai_request_unit(&mut self, d: &Data, player: u8, id: u16, priority: u8, val: u32, force: bool, keep: bool) {
        let p = player as usize;
        if !force && self.ai.players[p].flags & 0x200 != 0 {
            return;
        }
        if !keep {
            self.ai.players[p].flags &= !1;
        }
        self.ai.players[p].flags |= 0x40;
        self.ai_add_need(d, player, REQ_UNIT, id);
        self.ai_push_request(player, REQ_UNIT, id, priority, val);
        // 0x4484a0
        let (part, n) = match id {
            0x2c | 0x3e => (0x2b, 1),
            0x44 => (0x43, 2),
            0x3f => (0x3d, 2),
            0x67 => (0x26, 1),
            _ => return,
        };
        for _ in 0..n {
            self.ai_add_need(d, player, REQ_UNIT, part);
            self.ai_push_request(player, REQ_UNIT, part, priority.wrapping_sub(1), val);
        }
    }
}
