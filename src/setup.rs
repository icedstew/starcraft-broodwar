//! Game setup (the front end without dialogs): the lobby / player slot tables, the map
//! header load for a game start, the Templates\*.got game type, the player profile
//! (characters\<name>.spc / .mpc) and the game-result bookkeeping.
//!
//! The native globals are modelled as one `Setup`; the glue (glue.rs) owns it. Storm and
//! network calls are not ported (0x4d3fc0, 0x4d3cc0, ...).

use crate::mpq::Mpq;
use crate::storage::Storage;
use crate::todo_port;
use std::path::PathBuf;

/// A player slot (0x24 bytes): the game's table 0x57eee0 and the lobby copy 0x59bdb0.
pub type Slot = [u8; 0x24];

/// Slot field offsets.
pub mod slot {
    /// u32: the slot's index.
    pub const INDEX: usize = 0;
    /// u32: the storm (network) player id, 0xffffffff when none.
    pub const STORM: usize = 4;
    /// Owner type (OWNR): 2 human, 6 open (human to join), 9 ...
    pub const TYPE: usize = 8;
    /// Race (SIDE); 5 user selectable, 6 random.
    pub const RACE: usize = 9;
    /// Force (1..4).
    pub const FORCE: usize = 0xa;
    /// The player name (0x19 bytes).
    pub const NAME: usize = 0xb;
}

/// 0x500560 + 0x28 * version: the map versions the header loader accepts (VER chunk) and
/// whether each needs the expansion (+0x24).
const MAP_VERSIONS: [(u32, bool); 3] = [(0x3b, false), (0x3f, false), (0xcd, true)];

/// 0x501584 (records of 0x18: file name 0x14, priority): the league certificate files
/// 0x4137f0 looks for in a map archive.
const LEAGUE_FILES: [(&str, u32); 9] = [
    ("Blizzard_2.cnd", 1),
    ("Ladder_2.cnd", 1),
    ("PGL_2.cnd", 3),
    ("Ladder_3.cnd", 1),
    ("Ladder_4.cnd", 2),
    ("KBK_2.cnd", 4),
    ("CompUSA_1.cnd", 3),
    ("WGT_1.cnd", 6),
    ("Tourn_1.cnd", 5),
];

/// The map header 0x4ccac0 / 0x4cc060 fill (4bf5d0's `short[16]`): +0 SPRP (scenario name
/// and description string ids), +4 FORC (0x14 bytes), +0x18 VER (u16), +0x1c league
/// priority.
pub type MapHeader = [u8; 0x20];

/// 0x5004a8 chunk handlers of the map header table (0x5001e8 and 0x5001cc).
#[derive(Clone, Copy, PartialEq)]
enum Chunk {
    Type,
    Ver,
    Dim,
    Era,
    Ownr,
    Side,
    Str,
    Sprp,
    Forc,
    Vcod,
}

/// 0x5001cc: the chunks 0x4cc060 reads first (tag, handler, required).
const VERSION_CHUNKS: [(&[u8; 4], Chunk, bool); 2] = [(b"TYPE", Chunk::Type, false), (b"VER ", Chunk::Ver, true)];
/// 0x5001e8: the header chunks every map version reads (9 of 12 bytes each).
const HEADER_CHUNKS: [(&[u8; 4], Chunk, bool); 9] = [
    (b"VER ", Chunk::Ver, true),
    (b"DIM ", Chunk::Dim, true),
    (b"ERA ", Chunk::Era, true),
    (b"OWNR", Chunk::Ownr, true),
    (b"SIDE", Chunk::Side, true),
    (b"STR ", Chunk::Str, true),
    (b"SPRP", Chunk::Sprp, true),
    (b"FORC", Chunk::Forc, true),
    (b"VCOD", Chunk::Vcod, true),
];

/// The front end's game-setup globals.
pub struct Setup {
    /// 0x57eee0: the 12 player slots of the running game (type +8, race +9, force +0xa).
    pub slots: [Slot; 12],
    /// 0x59bdb0: the lobby copy the map load fills (0x4a91e0 copies it to `slots`).
    pub lobby: [Slot; 12],
    /// 0x59bda8: per slot, the map's race was "user selectable" (SIDE 5, made random).
    pub user_selectable: [u8; 8],
    /// 0x57eec0: storm player id -> slot index (8 = none).
    pub slot_by_storm: [u32; 8],
    /// 0x57ee7c: second copy of `slot_by_storm`.
    pub slot_by_storm2: [u32; 8],
    /// 0x57ee9c: the local player's name (profile name).
    pub local_name: [u8; 0x20],
    /// 0x51268c: the local player's storm id (8 = none).
    pub local_storm_id: u32,
    /// 0x512678 / 0x51267c / 0x512680: reset to 8 by 0x4d4ac0.
    pub g512678: [u32; 3],
    /// 0x512684 / 0x512688: the local player's slot.
    pub local_slot: [u32; 2],
    /// 0x596bac: the slot a started game uses when 0x6d1218 is set.
    pub preset_slot: u32,
    /// 0x6d1218.
    pub preset_slot_used: bool,
    /// 0x58d700: each player's game result (1 defeated .. see bw.player_left).
    pub results: [u8; 12],
    /// 0x66fe20 (stride 0x22): net slot connected (1).
    pub net_slot: [u8; 8],
    /// 0x66fbfa.
    pub g66fbfa: u8,
    /// 0x57f0b4: a network game.
    pub multiplayer: bool,
    /// 0x59688c: the network provider id ('NETB' 0x424e4554 = battle.net).
    pub net_provider: u32,
    /// 0x596865: the game template (see bw: +0 game type, +1 sub type, +8 victory 0x59686d,
    /// +0xc starting units 0x596871, +0x10 teams 0x596875, +0x12 0x596877).
    pub template: [u8; 0x20],
    /// 0x6d0f14: the mission's map comes from memory (campaign): 0x6d0f24 data.
    pub mission: bool,
    pub mission_chk: Vec<u8>,
    /// 0x581d61: 2 / 1 / other, 0x581d62..: per player flags (0x4c4220).
    pub result_mode: u8,
    pub result_flags: [u8; 8],
    /// 0x6d5bc8 timer running, 0x6d5bcc seconds so far, 0x59cc7c start / 0x59cc88 end ticks.
    pub timer_running: bool,
    pub timer_base: u32,
    pub timer_start: u32,
    pub timer_end: u32,
    /// 0x58d5b0: each player's force (the FORC bytes + 1).
    pub player_force: [u8; 8],
    /// 0x58d5b8: FORC force flags; 0x58d5bc: the force names (0x1e bytes).
    pub force_flags: [u8; 4],
    pub force_names: [[u8; 0x1e]; 4],
    /// 0x5971e4 / 0x654a80: cleared per game (0x4c3cd0 / 0x4d3240).
    pub g5971e4: [u32; 8],
    pub g654a80: [u32; 8],
    pub g654aa0: u32,
    pub g6556e4: u32,
    pub g6554b0: u8,
    pub g6556e0: u32,
    pub g57eebc: u32,
    /// 0x596814.
    pub g596814: u32,
    /// 0x58f440: the expansion is installed.
    pub expansion: bool,
    /// 0x6d1225: the league id (League_%d.cnd).
    pub league_id: u8,
    /// 0x57f244: the map is read from a loose directory (not an archive).
    pub map_dir_flag: u16,
    /// 0x596bc4: the map archive.
    pub map_archive: Option<Mpq>,
    /// 0x57fd3c: the map path; 0x57fe40: the map title.
    pub map_path: [u8; 0x104],
    pub map_title: [u8; 0x20],
    /// 0x5994dc.
    pub map_loaded: u32,
    /// 0x57f1d4 / 0x57f1d6: map width and height (DIM).
    pub map_width: u16,
    pub map_height: u16,
    /// 0x57f1dc: the tileset (ERA).
    pub tileset: u16,
    /// 0x5993d4 / 0x5994d8: the map's string table (STR).
    pub map_tbl: Option<Vec<u8>>,
    /// 0x6d5a10: the error popup is suppressed.
    pub quiet: bool,
    /// Where the profiles live (the game's `characters\` next to the exe).
    pub characters_dir: PathBuf,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup {
            slots: [[0; 0x24]; 12],
            lobby: [[0; 0x24]; 12],
            user_selectable: [0; 8],
            slot_by_storm: [0; 8],
            slot_by_storm2: [0; 8],
            local_name: [0; 0x20],
            local_storm_id: 0,
            g512678: [0; 3],
            local_slot: [0; 2],
            preset_slot: 0,
            preset_slot_used: false,
            results: [0; 12],
            net_slot: [0; 8],
            g66fbfa: 0,
            multiplayer: false,
            net_provider: 0,
            template: [0; 0x20],
            mission: false,
            mission_chk: Vec::new(),
            result_mode: 0,
            result_flags: [0; 8],
            timer_running: false,
            timer_base: 0,
            timer_start: 0,
            timer_end: 0,
            player_force: [0; 8],
            force_flags: [0; 4],
            force_names: [[0; 0x1e]; 4],
            g5971e4: [0; 8],
            g654a80: [0; 8],
            g654aa0: 0,
            g6556e4: 0,
            g6554b0: 0,
            g6556e0: 0,
            g57eebc: 0,
            g596814: 0,
            expansion: false,
            league_id: 0,
            map_dir_flag: 0,
            map_archive: None,
            map_path: [0; 0x104],
            map_title: [0; 0x20],
            map_loaded: 0,
            map_width: 0,
            map_height: 0,
            tileset: 0,
            map_tbl: None,
            quiet: false,
            characters_dir: std::env::var_os("SCBW_CHARACTERS").map(PathBuf::from).unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/scbw/characters")
            }),
        }
    }
}

/// Ordinal 501 (SStrCopy): at most `max - 1` bytes of the C string `src`, then a NUL.
pub fn sstr_copy(dst: &mut [u8], src: &[u8], max: usize) {
    let n = src.iter().position(|&c| c == 0).unwrap_or(src.len()).min(max - 1);
    dst[..n].copy_from_slice(&src[..n]);
    dst[n] = 0;
}

fn cstr(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// The string `idx` (0-based) of a .tbl image (u16 count, u16 offsets); "" when out of
/// range (0x501b7d).
fn tbl_str(tbl: &[u8], idx: usize) -> &[u8] {
    let u16_at = |o: usize| tbl.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    match (u16_at(0), u16_at(2 + 2 * idx)) {
        (Some(n), Some(off)) if idx < n && off <= tbl.len() => cstr(&tbl[off..]),
        _ => b"",
    }
}

/// The standard CRC-32 table-driven loop of 0x410430 (seed 0xffffffff, no final xor).
fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in d {
        let mut t = (c ^ b as u32) & 0xff;
        for _ in 0..8 {
            t = if t & 1 != 0 { 0xedb8_8320 ^ (t >> 1) } else { t >> 1 };
        }
        c = t ^ (c >> 8);
    }
    c
}

fn unix_time() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

/// 0x4c3280: read `len` bytes written by 0x4c3450 from a file: crc, chunk count, then per
/// chunk (<= 0x2000 plain bytes) its stored size and bytes (PKWare imploded unless the
/// stored size equals the plain size).
fn read_packed(f: &[u8], len: usize) -> Option<Vec<u8>> {
    if len == 0 {
        return None;
    }
    let u32_at = |o: usize| f.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    let (crc, n) = (u32_at(0)?, u32_at(4)?);
    let (mut pos, mut out) = (8usize, Vec::new());
    for i in 0..n {
        let sz = u32_at(pos)?;
        pos += 4;
        if sz > len - out.len() {
            return None;
        }
        let stored = f.get(pos..pos + sz)?;
        pos += sz;
        let plain = (len.wrapping_sub(0x2000 * i)).min(0x2000);
        if sz == plain {
            out.extend_from_slice(stored);
        } else {
            let d = explode::explode(stored).ok()?;
            if d.is_empty() || d.len() > len - out.len() {
                return None;
            }
            out.extend_from_slice(&d);
        }
    }
    (out.len() == len && crc32(&out) as usize == crc).then_some(out)
}

/// A player profile (characters\<name>.spc / .mpc, 0x70 bytes).
#[derive(Clone)]
pub struct Profile(pub [u8; 0x70]);

impl Profile {
    /// +0: 0x112233.
    pub const MAGIC: u32 = 0x112233;
    pub fn magic(&self) -> u32 {
        u32::from_le_bytes(self.0[0..4].try_into().unwrap())
    }
    /// +0xc: the name (0x19 bytes).
    pub fn name(&self) -> &[u8] {
        cstr(&self.0[0xc..0x25])
    }
    /// +0x28 (classic) / +0x34 (Brood War): the best mission reached per race.
    fn best_at(expansion: bool, race: u8) -> usize {
        (if expansion { 0x34 } else { 0x28 }) + 4 * race as usize
    }
    pub fn best(&self, expansion: bool, race: u8) -> u32 {
        let o = Profile::best_at(expansion, race);
        u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    pub fn set_best(&mut self, expansion: bool, race: u8, v: u32) {
        let o = Profile::best_at(expansion, race);
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
}

impl Setup {
    /// 0x484dc0: the first of the 8 slots in force `force` whose type is 6 or 9; 8 if none.
    pub fn first_slot_in_force(&self, force: u8) -> u32 {
        for (i, s) in self.slots[..8].iter().enumerate() {
            if s[slot::FORCE] == force && (s[slot::TYPE] == 6 || s[slot::TYPE] == 9) {
                return i as u32;
            }
        }
        8
    }

    /// 0x484fc0: how many slots are in force `force` with type 6 or 9.
    pub fn count_in_force(&self, force: u8) -> u8 {
        let mut n = 0u8;
        for s in self.slots[..8].iter().rev() {
            if s[slot::FORCE] == force && (s[slot::TYPE] == 6 || s[slot::TYPE] == 9) {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 0x4855e0: the force that leads a team game (most open slots; ties go to the force
    /// with more humans); 0 if none.
    pub fn team_leader(&self) -> u8 {
        let (mut best_count, mut best_force) = (0u8, 0u8);
        let max = if self.template[0] != 0x0f { self.template[0x10] as i32 } else { 2 };
        let humans = |fc: u8| self.slots[..8].iter().filter(|s| s[slot::FORCE] == fc && s[slot::TYPE] == 2).count() as u8;
        let mut force = 1i32;
        while force <= max {
            let f = force as u8;
            if self.count_in_force(f) == best_count && best_force != 0 && humans(f) < humans(best_force) {
                best_count = self.count_in_force(f);
                best_force = f;
            }
            let n = self.count_in_force(f);
            if best_count < n {
                best_force = f;
                best_count = n;
            }
            force += 1;
        }
        best_force
    }

    /// 0x4a99c0: the slot a game started here uses: the first type-6 slot, or in a team
    /// game the first open slot of the leading force; 8 if none.
    pub fn open_slot(&self) -> u32 {
        if !self.multiplayer || self.template[0x10] == 0 {
            return self.slots[..8].iter().position(|s| s[slot::TYPE] == 6).map_or(8, |i| i as u32);
        }
        let leader = self.team_leader();
        if leader != 0 {
            self.first_slot_in_force(leader)
        } else {
            8
        }
    }

    /// 0x4a9a10: whether the open slot's race was "user selectable" in the map.
    pub fn open_slot_user_selectable(&self) -> u8 {
        let i = self.open_slot();
        if i >= 8 {
            0
        } else {
            self.user_selectable[i as usize]
        }
    }

    /// 0x4a8d40 (esi = 0x51268c, the local storm id): rebuild the storm id -> slot tables
    /// from the human slots (type 2), walking slots 7..0, and the local player's slot.
    pub fn index_human_slots(&mut self) {
        self.local_slot = [u32::MAX; 2];
        self.slot_by_storm = [8; 8];
        self.slot_by_storm2 = [8; 8];
        for i in (0..8).rev() {
            let s = &self.slots[i];
            if s[slot::TYPE] == 2 {
                let id = u32::from_le_bytes(s[slot::STORM..slot::STORM + 4].try_into().unwrap());
                if let Some(e) = self.slot_by_storm.get_mut(id as usize) {
                    *e = i as u32;
                    self.slot_by_storm2[id as usize] = i as u32;
                }
                if id == self.local_storm_id {
                    self.local_slot = [i as u32; 2];
                }
            }
        }
    }

    /// 0x4a91e0: copy the 12 lobby slots to the game's slot table.
    pub fn copy_lobby_to_slots(&mut self) {
        for i in (0..12).rev() {
            self.slots[i] = self.lobby[i];
        }
    }

    /// 0x4a9210: how many lobby slots are occupied (type 3..=7).
    pub fn lobby_occupied(&self) -> u8 {
        let mut n = 0u8;
        for s in self.lobby[..8].iter().rev() {
            if (3..=7).contains(&s[slot::TYPE]) {
                n = n.wrapping_add(1);
            }
        }
        n
    }

    /// 0x45ac10: the FORC chunk (`forc`, 0x14 bytes) into the force tables: each player's
    /// force + 1, the four force names (the map's string, or the default 0x6d1220 string
    /// 0x16..0x19 when the map has none) and flags. `glu_tbl` is the glue string table
    /// 0x6d1220.
    pub fn set_forces(&mut self, forc: &[u8; 0x14], glu_tbl: &[u8]) {
        self.player_force.copy_from_slice(&forc[..8]);
        for f in &mut self.player_force {
            *f = f.wrapping_add(1);
        }
        for k in (0..4).rev() {
            let id = u16::from_le_bytes([forc[8 + 2 * k], forc[9 + 2 * k]]) as usize;
            let from_map = match &self.map_tbl {
                Some(t) if id != 0 => Some(tbl_str(t, id - 1).to_vec()),
                _ => None,
            };
            let s = from_map.unwrap_or_else(|| tbl_str(glu_tbl, k + 1 + 0x15).to_vec());
            sstr_copy(&mut self.force_names[k], &s, 0x1e);
        }
        self.force_flags.copy_from_slice(&forc[0x10..0x14]);
    }

    /// 0x45ad90: give each occupied lobby slot its force (and the game slot); unoccupied
    /// players lose theirs.
    pub fn assign_forces(&mut self) {
        for i in 0..8 {
            if (3..=7).contains(&self.lobby[i][slot::TYPE]) {
                let f = self.player_force[i];
                self.lobby[i][slot::FORCE] = f;
                self.slots[i][slot::FORCE] = f;
            } else {
                self.player_force[i] = 0;
            }
        }
    }

    /// 0x4dbe50: slot setup for a game start: the local slot becomes human (2) with the
    /// profile name. False when no slot is free.
    pub fn local_slot_setup(&mut self) -> bool {
        let i = if self.preset_slot_used { self.preset_slot } else { self.open_slot() };
        self.local_slot = [i; 2];
        if i < 8 {
            let name = self.local_name;
            self.slots[i as usize][slot::TYPE] = 2;
            sstr_copy(&mut self.slots[i as usize][slot::NAME..], &name, 0x19);
            true
        } else {
            false
        }
    }

    /// 0x4c3cd0: clear the 8 per-player words at 0x5971e4.
    pub fn clear_5971e4(&mut self) {
        self.g5971e4 = [0; 8];
    }

    /// 0x4d3240: reset the per-game tables.
    pub fn reset_game_tables(&mut self) {
        self.g654a80 = [0; 8];
        self.g5971e4 = [0; 8];
        self.g6556e4 = 0;
        self.g6554b0 = 0;
        self.g6556e0 = 0;
        self.g57eebc = 0;
        self.g654aa0 = 0;
        self.g66fbfa = 1;
        if self.g596814 == 0 {
            self.results[..8].fill(0);
        }
    }

    /// 0x4d4ac0: the slot indices and storm-id tables back to "none" (8).
    pub fn reset_slot_indices(&mut self) {
        self.g512678 = [8; 3];
        self.local_slot = [8; 2];
        self.local_storm_id = 8;
        self.slot_by_storm = [8; 8];
        self.slot_by_storm2 = [8; 8];
    }

    /// 0x4a2ca0: elapsed game seconds (`now` = GetTickCount).
    pub fn elapsed_seconds(&self, now: u32) -> u32 {
        if self.timer_running {
            self.timer_base.wrapping_add(now.wrapping_sub(self.timer_start) / 1000)
        } else {
            self.timer_base.wrapping_add(self.timer_end.wrapping_sub(self.timer_start) / 1000)
        }
    }

    /// 0x4c4220: the end-of-game result of each storm player (the returned array is the
    /// native edi buffer: 0 none, 1 victory, 2 defeat, 3 disconnected, 4 ...) and the local
    /// player's result. May record a win for the last connected player in `results`.
    pub fn player_outcomes(&mut self, now: u32) -> ([u32; 8], u32) {
        let mut out = [0u32; 8];
        let local = self.local_storm_id as usize;
        if self.timer_running {
            self.timer_end = now;
            self.timer_running = false;
        }
        if self.multiplayer && self.net_provider == 0x424e4554 && self.elapsed_seconds(now) < 0x79 {
            return ([3; 8], 3);
        }
        let mut count = 0;
        for i in 0..8 {
            let b = self.slot_by_storm[i] as u8;
            if b < 8 {
                count += 1;
                match self.results[b as usize] {
                    1 => out[i] = 4,
                    2 => out[i] = 2,
                    3 => out[i] = 1,
                    5 => out[i] = 3,
                    _ => {}
                }
            }
        }
        if out.get(local) == Some(&0) {
            out[local] = 2;
        }
        let local_result = out.get(local).copied().unwrap_or(0);
        if self.multiplayer {
            let t = &self.template;
            let modes_off = t[8] == 0 && t[0xc] == 0 && t[0x12] == 0;
            // 0x4c4368: the cases that need a mission check before 0x4c4370.
            let to_4368 = !modes_off || if t[0] == 0x80 { t[1] != 0 } else { 0x10 < t[0] && t[0] < 0x17 };
            if count == 1 || !to_4368 || self.mission {
                // 0x4c4370: every connected id gets 3.
                for i in 0..8 {
                    if self.slot_by_storm[i] < 8 {
                        out[i] = 3;
                    }
                }
                return (out, local_result);
            }
        }
        let set = |out: &mut [u32; 8], i: usize, v: u32| {
            if let Some(e) = out.get_mut(i) {
                *e = v;
            }
        };
        match self.result_mode {
            2 => {
                for i in 0..8 {
                    if self.result_flags[i] != 0 {
                        out[i] = 0;
                    }
                }
                set(&mut out, local, 4);
            }
            1 => {
                for i in 0..8 {
                    if self.result_flags[i] != 0 {
                        out[i] = 4;
                    }
                }
                set(&mut out, local, 0);
            }
            _ => {
                for i in 0..8 {
                    if self.result_flags[i] != 0 {
                        out[i] = 4;
                    }
                }
            }
        }
        let (mut connected, mut last) = (0u8, 0usize);
        for i in 0..8 {
            if i != local && self.net_slot[i] == 1 {
                connected += 1;
                last = i;
            }
        }
        if connected == 1 {
            let computers = self.slots[..8].iter().filter(|s| s[slot::TYPE] == 1 || s[slot::TYPE] == 5).count();
            let slot_of = self.slot_by_storm[last] as usize;
            if computers == 0 && self.results.get(slot_of) == Some(&0) {
                out[last] = 1;
                self.results[slot_of] = 3;
            }
        }
        (out, out.get(local).copied().unwrap_or(0))
    }

    // ---- map header ----

    /// 0x4ca520: close the map archive.
    pub fn close_map_archive(&mut self) -> bool {
        self.map_archive.take();
        true
    }

    /// 0x4cc110: open the map `path` as an archive and look for a league certificate
    /// (0x4137f0 with 0x4ca540: priorities 6..1; the "Tourn" one needs League_<id>.cnd too).
    /// Returns the league priority (0 none); the archive is kept when `keep`.
    fn open_map_archive(&mut self, path: &str, keep: bool) -> Option<u32> {
        let a = Mpq::open(std::path::Path::new(path)).ok()?;
        let mut found = 0;
        'prio: for prio in (1..=6).rev() {
            for (name, p) in LEAGUE_FILES.iter().rev() {
                if *p != prio || a.read(name).is_err() {
                    continue;
                }
                if prio == 5 && a.read(&format!("League_{}.cnd", self.league_id)).is_err() {
                    continue;
                }
                found = prio;
                break 'prio;
            }
        }
        if found != 0 {
            // The signature must also check out.
            todo_port!("0x4bef80", "map archive SIGN check (league maps are treated as signed)");
        }
        if keep {
            self.map_archive = Some(a);
        }
        Some(found)
    }

    /// 0x4cc350: open the map archive for `path`, or (0x57f244 set) take `path` as the
    /// directory the map's files are in. `dir` receives that directory; `league` the
    /// league priority.
    fn open_map(&mut self, path: &str, dir: &mut String, league: Option<&mut u32>) -> bool {
        if self.map_dir_flag == 0 {
            dir.clear();
            match self.open_map_archive(path, true) {
                Some(l) => {
                    if let Some(p) = league {
                        *p = l;
                    }
                }
                None => return false,
            }
        } else {
            *dir = path.to_string();
        }
        true
    }

    /// 0x413670 / 0x4135c0 / 0x413550 (0x413710): walk the chunks of `data` and run the
    /// handlers of `spec` on each (in table order, then chunk order). False when a required
    /// chunk is missing or a handler fails.
    fn read_chunks(&mut self, data: &[u8], spec: &[(&[u8; 4], Chunk, bool)], hdr: &mut MapHeader, glu_tbl: &[u8]) -> bool {
        let mut chunks: Vec<([u8; 4], usize, i32)> = Vec::new();
        let end = data.len();
        if end >= 8 {
            let mut payload = 8usize;
            let mut tag: [u8; 4] = data[0..4].try_into().unwrap();
            let mut len = i32::from_le_bytes(data[4..8].try_into().unwrap());
            for _ in 0..=end {
                chunks.push((tag, payload, len));
                let next = payload as i64 + len as i64 + 8;
                if next > end as i64 || next < 8 {
                    break;
                }
                let h = next as usize - 8;
                tag = data[h..h + 4].try_into().unwrap();
                len = i32::from_le_bytes(data[h + 4..h + 8].try_into().unwrap());
                payload = next as usize;
            }
        }
        if spec.iter().any(|(t, _, req)| *req && !chunks.iter().any(|c| c.0 == **t)) {
            return false;
        }
        for (t, h, _) in spec {
            for &(_, payload, len) in chunks.iter().filter(|c| c.0 == **t) {
                // Handlers refuse chunks reaching past the end.
                let body = (len >= 0).then(|| data.get(payload..payload + len as usize)).flatten();
                if !self.chunk_handler(*h, body, len, hdr, glu_tbl) {
                    return false;
                }
            }
        }
        true
    }

    /// The handlers of the map header chunks: 0x4cb550 (TYPE), 0x4cb500 (VER), 0x4cb040
    /// (DIM), 0x4cb3a0 (ERA), 0x4cb420 (OWNR), 0x4cb490 (SIDE), 0x4cc650 (STR), 0x4caf40
    /// (SPRP), 0x4caee0 (FORC), 0x4cbc40 (VCOD).
    fn chunk_handler(&mut self, h: Chunk, body: Option<&[u8]>, len: i32, hdr: &mut MapHeader, glu_tbl: &[u8]) -> bool {
        let _ = glu_tbl;
        match h {
            Chunk::Type => true,
            Chunk::Ver => match body {
                Some(b) if len == 2 => {
                    hdr[0x18..0x1a].copy_from_slice(b);
                    true
                }
                _ => false,
            },
            Chunk::Dim => match body {
                Some(b) if len == 4 => {
                    self.map_width = u16::from_le_bytes([b[0], b[1]]);
                    self.map_height = u16::from_le_bytes([b[2], b[3]]);
                    true
                }
                _ => false,
            },
            Chunk::Era => match body {
                Some(b) if len == 2 => {
                    let v = u16::from_le_bytes([b[0], b[1]]);
                    self.tileset = v;
                    if v > 4 && !self.expansion {
                        return false;
                    }
                    self.tileset = v & 7;
                    true
                }
                _ => false,
            },
            Chunk::Ownr | Chunk::Side => match body {
                Some(b) if len == 0xc => {
                    let field = if h == Chunk::Ownr { slot::TYPE } else { slot::RACE };
                    for k in 0..12 {
                        self.lobby[k][field] = b[k];
                    }
                    true
                }
                _ => false,
            },
            Chunk::Str => match body {
                Some(b) => {
                    self.map_tbl = Some(b.to_vec());
                    todo_port!("0x4f6100", "register the string table size in the appaddex id set 0x6d5dd8");
                    true
                }
                None => false,
            },
            Chunk::Sprp => match body {
                Some(b) if len == 4 => {
                    hdr[0..4].copy_from_slice(b);
                    true
                }
                _ => false,
            },
            Chunk::Forc => match body {
                Some(b) if len <= 0x14 => {
                    hdr[4..0x18].fill(0);
                    hdr[4..4 + b.len()].copy_from_slice(b);
                    true
                }
                _ => false,
            },
            Chunk::Vcod => {
                todo_port!("0x4cbc40", "VCOD chunk hash check against the exe's VCOD resource (treated as passing)");
                true
            }
        }
    }

    /// 0x4cc060: read the version of a chunk image (TYPE / VER) into `hdr` and return the
    /// index into the map version table 0x500560; None for an unknown version or one that
    /// needs the expansion when it is not installed.
    fn map_version(&mut self, data: &[u8], hdr: &mut MapHeader, glu_tbl: &[u8]) -> Option<usize> {
        if data.is_empty() {
            return None;
        }
        if !self.read_chunks(data, &VERSION_CHUNKS, hdr, glu_tbl) {
            return None;
        }
        let ver = u16::from_le_bytes([hdr[0x18], hdr[0x19]]) as u32;
        let idx = MAP_VERSIONS.iter().rposition(|v| v.0 == ver)?;
        if MAP_VERSIONS[idx].1 && !self.expansion {
            return None;
        }
        Some(idx)
    }

    /// Reads a map file: the map archive first, then the game's archives.
    fn read_map_file(&self, name: &str, files: &Storage) -> Option<Vec<u8>> {
        if let Some(a) = &self.map_archive {
            if let Ok(d) = a.read(name) {
                return Some(d);
            }
        }
        std::fs::read(name.replace('\\', "/")).ok().or_else(|| files.read(name).ok())
    }

    /// 0x4ccac0: the map header of the map `path` (staredit\scenario.chk of its archive):
    /// version, then the header chunks. The archive is closed again.
    pub fn map_header_from_archive(&mut self, files: &Storage, path: &str, hdr: &mut MapHeader, glu_tbl: &[u8]) -> bool {
        if path.is_empty() {
            return false;
        }
        let mut dir = String::new();
        let mut league = 0u32;
        if !self.open_map(path, &mut dir, Some(&mut league)) {
            return false;
        }
        if self.map_dir_flag == 0 {
            hdr[0x1c..0x20].copy_from_slice(&league.to_le_bytes());
        }
        let name = if dir.is_empty() { "staredit\\scenario.chk".to_string() } else { format!("{dir}\\staredit\\scenario.chk") };
        let Some(chk) = self.read_map_file(&name, files) else {
            self.close_map_archive();
            return false;
        };
        let Some(_idx) = self.map_version(&chk, hdr, glu_tbl) else {
            self.close_map_archive();
            return false;
        };
        // Every version uses the header table 0x5001e8.
        let ok = self.read_chunks(&chk, &HEADER_CHUNKS, hdr, glu_tbl);
        self.close_map_archive();
        ok
    }

    /// 0x4bf5d0: load a map's (or the mission's) header for a game start: the lobby slots
    /// from the OWNR / SIDE chunks, the forces, the map path and title. `keep_dir` is the
    /// `param_3` flag (0: forget the loose-directory mode).
    pub fn load_map(&mut self, files: &Storage, path: &str, hdr: &mut MapHeader, keep_dir: bool, glu_tbl: &[u8]) -> bool {
        self.map_path[0] = 0;
        if !keep_dir {
            self.map_dir_flag = 0;
        }
        self.lobby = [[0; 0x24]; 12];
        self.user_selectable = [0; 8];
        *hdr = [0; 0x20];
        if self.mission {
            let chk = self.mission_chk.clone();
            if self.map_version(&chk, hdr, glu_tbl).is_none() {
                return false;
            }
            if !self.read_chunks(&chk, &HEADER_CHUNKS, hdr, glu_tbl) {
                return false;
            }
        } else if path.is_empty() || !self.map_header_from_archive(files, path, hdr, glu_tbl) {
            return false;
        }
        for i in (0..12).rev() {
            let s = &mut self.lobby[i];
            s[slot::INDEX..slot::INDEX + 4].copy_from_slice(&(i as u32).to_le_bytes());
            s[slot::STORM..slot::STORM + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            if s[slot::RACE] == 5 {
                s[slot::RACE] = 6;
                if i < 8 {
                    self.user_selectable[i] = 1;
                }
            }
            if i >= 8 {
                s[slot::TYPE] = 0;
                s[slot::RACE] = 0;
                s[slot::FORCE] = 0;
            }
        }
        self.copy_lobby_to_slots();
        let forc: [u8; 0x14] = hdr[4..0x18].try_into().unwrap();
        self.set_forces(&forc, glu_tbl);
        self.assign_forces();
        sstr_copy(&mut self.map_path, path.as_bytes(), 0x104);
        if !keep_dir {
            self.map_dir_flag = 0;
        }
        self.map_loaded = 1;
        let name_id = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
        let title: Option<Vec<u8>> = if name_id == 0 {
            Some(Vec::new())
        } else {
            self.map_tbl.as_ref().map(|t| tbl_str(t, name_id - 1).to_vec())
        };
        if let Some(t) = title {
            sstr_copy(&mut self.map_title, &t, 0x20);
        }
        true
    }

    // ---- game type template ----

    /// 0x4ab750: Templates\<name>.got: byte 0 must be 3 and the file 0x61 bytes; returns the
    /// game type record (file +0x41, 0x20 bytes) when its game type is <= 0x80 and its sub
    /// type < 8, with the two 0x20-byte blocks at +1 / +0x21 in `out1` / `out2`. A missing
    /// file is fatal natively; here None.
    pub fn load_got(files: &Storage, name: &str, out1: &mut [u8; 0x20], out2: &mut [u8; 0x20]) -> Option<[u8; 0x20]> {
        let f = files.read(&format!("Templates\\{name}.got")).ok()?;
        if f.len() != 0x61 || f[0] != 3 {
            return None;
        }
        out1.copy_from_slice(&f[1..0x21]);
        out2.copy_from_slice(&f[0x21..0x41]);
        let mut t = [0u8; 0x20];
        t.copy_from_slice(&f[0x41..0x61]);
        (t[0] < 0x81 && u16::from_le_bytes([t[2], t[3]]) < 8).then_some(t)
    }

    /// 0x4ab840: load the "Use Map Settings(1)" template (result unused).
    pub fn load_use_map_settings_got(files: &Storage) {
        let (mut a, mut b) = ([0u8; 0x20], [0u8; 0x20]);
        let _ = Setup::load_got(files, "Use Map Settings(1)", &mut a, &mut b);
    }

    // ---- profiles ----

    /// 0x4a8790: characters\<name>.spc (.mpc in a network game).
    fn profile_path(&self, name: &[u8]) -> PathBuf {
        let ext = if self.multiplayer { "mpc" } else { "spc" };
        // Separators are replaced: the name must stay inside `characters_dir` here.
        let n: String = String::from_utf8_lossy(cstr(name)).chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
        self.characters_dir.join(format!("{n}.{ext}"))
    }

    /// 0x4a8940: load the profile `name`. On a missing or bad file the profile is a new
    /// default one (name, creation time, network flag) and the result is false.
    pub fn load_profile(&self, name: &[u8]) -> (Profile, bool) {
        let mut p = Profile([0; 0x70]);
        // Windows file names are case-insensitive.
        let path = self.profile_path(name);
        let found = std::fs::read_dir(&self.characters_dir).ok().and_then(|d| {
            d.flatten().map(|e| e.path()).find(|e| e.file_name().is_some_and(|n| n.eq_ignore_ascii_case(path.file_name().unwrap_or_default())))
        });
        if let Ok(f) = std::fs::read(found.unwrap_or(path)) {
            // A file of the older 0x34-byte format is read too (the rest stays zero).
            let got = read_packed(&f, 0x70).map(|d| (d, 0x70)).or_else(|| read_packed(&f, 0x34).map(|d| (d, 0x34)));
            if let Some((d, n)) = got {
                p.0[..n].copy_from_slice(&d);
                if p.magic() == Profile::MAGIC {
                    if !p.name().eq_ignore_ascii_case(cstr(name)) {
                        sstr_copy(&mut p.0[0xc..], name, 0x19);
                    }
                    return (p, true);
                }
            }
        }
        (self.default_profile(name), false)
    }

    fn default_profile(&self, name: &[u8]) -> Profile {
        let mut p = Profile([0; 0x70]);
        sstr_copy(&mut p.0[0xc..], name, 0x19);
        p.0[8..12].copy_from_slice(&unix_time().to_le_bytes());
        p.0[4] = self.multiplayer as u8;
        p.0[0..4].copy_from_slice(&Profile::MAGIC.to_le_bytes());
        p
    }

    /// 0x4a8880: write the profile (creating `characters\`) in the 0x4c3450 format: crc,
    /// chunk count, then each 0x2000-byte chunk with its size.
    pub fn write_profile(&self, p: &Profile) -> bool {
        if std::fs::create_dir_all(&self.characters_dir).is_err() {
            return false;
        }
        let mut out = Vec::new();
        out.extend_from_slice(&crc32(&p.0).to_le_bytes());
        out.extend_from_slice(&(p.0.len().div_ceil(0x2000) as u32).to_le_bytes());
        for c in p.0.chunks(0x2000) {
            // 0x4c3200 stores a chunk uncompressed when the implode does not shrink it,
            // and 0x4c3280 reads that back.
            todo_port!("0x410b30", "PKWare implode of the profile chunk (stored uncompressed)");
            out.extend_from_slice(&(c.len() as u32).to_le_bytes());
            out.extend_from_slice(c);
        }
        std::fs::write(self.profile_path(p.name()), out).is_ok()
    }

    /// 0x4f3f20 (param_1 = 0): whether the file name `name` is unusable (control
    /// characters, a character of glue string 0x10, a reserved device name).
    fn profile_name_invalid(name: &[u8], glu_tbl: &[u8]) -> bool {
        if name.iter().any(|&c| c < 0x20) {
            return true;
        }
        let bad: Vec<u8> = tbl_str(glu_tbl, 0x10).iter().copied().take(0x3f).collect();
        let n = if name.len() >= 2 && name[0].is_ascii_alphabetic() && name[1] == b':' { &name[2..] } else { name };
        if n.iter().any(|c| bad.contains(c)) {
            return true;
        }
        if n.is_empty() {
            return true;
        }
        let base = &n[..n.iter().position(|&c| c == b'.').unwrap_or(n.len())];
        const RESERVED: [&str; 25] = [
            "nul", "con", "prn", "aux", "conin$", "conout$", "clock$", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ];
        RESERVED.iter().any(|r| base.eq_ignore_ascii_case(r.as_bytes()))
    }

    /// 0x4a8a40: create and save a new profile `name`; None when the name is unusable or
    /// the write fails.
    pub fn new_profile(&self, name: &[u8], glu_tbl: &[u8]) -> Option<Profile> {
        if Setup::profile_name_invalid(cstr(name), glu_tbl) {
            return None;
        }
        let p = self.default_profile(name);
        self.write_profile(&p).then_some(p)
    }

    /// 0x4dbee0: after a mission: raise the profile's best mission for the race to
    /// `reached` (`result` +2) and save. A missing profile named like glue string 0x47 (the
    /// default player) is created. `race` is `result` +6.
    pub fn record_mission(&mut self, reached: u16, race: u8, glu_tbl: &[u8]) {
        let name = self.local_name;
        let (mut p, ok) = self.load_profile(&name);
        if !ok {
            let created = if cstr(&name).eq_ignore_ascii_case(tbl_str(glu_tbl, 0x47)) { self.new_profile(&name, glu_tbl) } else { None };
            match created {
                Some(n) => p = n,
                None => {
                    if !self.quiet {
                        todo_port!("0x4bb300", "error popup (string 0x58)");
                    }
                }
            }
        }
        if p.best(self.expansion, race) < reached as u32 {
            p.set_best(self.expansion, race, reached as u32);
            self.write_profile(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_roundtrip() {
        let mut s = Setup::default();
        s.characters_dir = std::env::temp_dir().join(format!("scbw_chr_{}", std::process::id()));
        let mut p = s.default_profile(b"Tester");
        p.set_best(true, 1, 7);
        assert!(s.write_profile(&p));
        let (q, ok) = s.load_profile(b"tester");
        assert!(ok && q.best(true, 1) == 7 && q.name() == b"Tester");
        let _ = std::fs::remove_dir_all(&s.characters_dir);
    }
}
