//! The saved game's head after the text and the 0xb5-byte header (0x4d02d0 writes it,
//! 0x4cf5f0 reads it when the save is opened): the player table 0x57eee0 (0x1b0) and the
//! game data 0x57f0f0 (96000 bytes), packed, then u32 0x512688.
//!
//! The game data is the globals region 0x57f0f0..0x5967f0 as one block. `game_data` maps
//! every field the Rust state keeps to its native address (one table for save and load);
//! the rest of the region is written as 0 and ignored on load.

use super::{In, Out};
use crate::bw::{Data, State};
use crate::setup::slot;

/// 0x57f0f0: the first byte of the saved game data.
const BASE: usize = 0x57f0f0;
/// 0x17700: its length.
const LEN: usize = 96000;
/// 0x57eee0: the player table, 12 entries of 0x24 bytes.
const TABLE_LEN: usize = 0x1b0;
/// The save version 0x4cf160 writes (u16 at the header's +2); older ones get fix-ups.
const VERSION: u16 = 3;

/// A little-endian value of the region.
trait Pod: Copy {
    const N: usize;
    fn put(self, b: &mut [u8]);
    fn get(b: &[u8]) -> Self;
}

macro_rules! pod {
    ($($t:ty),*) => {$(
        impl Pod for $t {
            const N: usize = std::mem::size_of::<$t>();
            fn put(self, b: &mut [u8]) {
                b[..Self::N].copy_from_slice(&self.to_le_bytes());
            }
            fn get(b: &[u8]) -> Self {
                Self::from_le_bytes(b[..Self::N].try_into().unwrap())
            }
        }
    )*};
}
pod!(u8, u16, i16, u32, i32);

/// The region being written (`save`) or read, by native address.
struct Io<'a> {
    buf: &'a mut [u8],
    save: bool,
    /// Bytes some field covers.
    cov: Vec<bool>,
}

impl Io<'_> {
    /// `v` at native address `a`: written to the region, or read from it.
    fn put<T: Pod>(&mut self, a: usize, v: &mut [T]) {
        let o = a - BASE;
        for (i, x) in v.iter_mut().enumerate() {
            let at = o + i * T::N;
            if self.save {
                x.put(&mut self.buf[at..]);
            } else {
                *x = T::get(&self.buf[at..]);
            }
            self.cov[at..at + T::N].fill(true);
        }
    }

    fn one<T: Pod>(&mut self, a: usize, v: &mut T) {
        self.put(a, std::slice::from_mut(v));
    }

    /// `rows` consecutive arrays of `v`, `stride` bytes apart from `a`.
    fn rows<T: Pod, const N: usize>(&mut self, a: usize, stride: usize, v: &mut [[T; N]]) {
        for (i, r) in v.iter_mut().enumerate() {
            self.put(a + i * stride, r);
        }
    }
}

impl State {
    /// The fields of 0x57f0f0..0x5967f0 the Rust state keeps, at their native addresses.
    fn game_data(&mut self, io: &mut Io) {
        let pl = &mut self.players;
        // Resources: 0x57f0f0 minerals, 0x57f120 gas, 0x57f150 gas gathered, 0x57f180 minerals gathered.
        io.put(0x57f0f0, &mut pl.minerals);
        io.put(0x57f120, &mut pl.gas);
        io.put(0x57f150, &mut pl.gas_gathered);
        io.put(0x57f180, &mut pl.minerals_gathered);
        io.put(0x57f1c0, &mut self.seat_races);
        io.put(0x57f1cb, &mut self.team_leaders);
        let mut sc = [self.screen_tile.0, self.screen_tile.1];
        io.put(0x57f1d0, &mut sc);
        self.screen_tile = (sc[0], sc[1]);
        // 0x57f1d4 map size, 0x57f1dc tileset: the map is the game's own, so what the file
        // holds is not taken over.
        io.put(0x57f1d4, &mut [self.map_tiles.0 as u16, self.map_tiles.1 as u16]);
        io.one(0x57f1dc, &mut self.tileset.clone());
        io.put(0x57f1ec, &mut self.player_vision);
        io.put(0x57f21c, &mut self.color_slot);
        io.one(0x57f1e3, &mut self.camp_cont);
        io.one(0x57f23c, &mut self.frame);
        io.one(0x57f240, &mut self.game_secs);
        // 0x57f27c units each player may build.
        io.rows(0x57f27c, 228, &mut pl.unit_available);
        // 0x57fe60 unit groups of the 8 players (the 4 extra Rust players have no room).
        for (i, g) in self.hotkeys.iter_mut().take(8 * 18).enumerate() {
            io.put(0x57fe60 + i * 0x30, g);
        }
        io.put(0x581d6a, &mut self.circle_color);
        // Score and count tables, 12 i32 each.
        io.put(0x581de4, &mut pl.men);
        io.put(0x581e14, &mut pl.men_completed);
        io.put(0x581e44, &mut pl.units_produced);
        io.put(0x581e74, &mut pl.men_lost);
        io.put(0x581ea4, &mut pl.men_killed);
        io.put(0x581ed4, &mut pl.unit_score);
        io.put(0x581f04, &mut pl.kill_score);
        io.put(0x581f34, &mut pl.buildings);
        io.put(0x581f64, &mut pl.buildings_completed);
        io.put(0x581f94, &mut pl.buildings_constructed);
        io.put(0x581fc4, &mut pl.buildings_lost);
        io.put(0x581ff4, &mut pl.buildings_razed);
        io.put(0x582024, &mut pl.building_score);
        io.put(0x582054, &mut pl.razing_score);
        io.put(0x582084, &mut pl.factories);
        io.put(0x5820b4, &mut pl.factories_completed);
        io.put(0x5820e4, &mut pl.factories_lost);
        io.put(0x582114, &mut pl.factories_razed);
        // Supply per race (zerg, terran, protoss; stride 0x90): provided, used, max.
        io.rows(0x582144, 0x90, &mut pl.supply_provided);
        io.rows(0x582174, 0x90, &mut pl.supply_used);
        io.rows(0x5821a4, 0x90, &mut pl.supply_max);
        io.put(0x5822f4, &mut pl.custom_score);
        // [unit type][player] tables: counts, completed, kills, deaths.
        io.rows(0x582324, 0x30, &mut pl.unit_counts);
        io.rows(0x584de4, 0x30, &mut pl.completed_counts);
        io.rows(0x5878a4, 0x30, &mut pl.kills);
        io.rows(0x58a364, 0x30, &mut pl.deaths);
        // Techs: the classic 24 per player, then the Brood War 20 (our 44 are both in a row).
        for p in 0..12 {
            for (a, b, s) in [(0x58ce24, 0x58f050, &mut pl.tech_available[p]), (0x58cf44, 0x58f140, &mut pl.tech_researched[p])] {
                io.put(a + p * 24, &mut s[..24]);
                io.put(b + p * 20, &mut s[24..]);
            }
            // Upgrade caps and levels: 46 classic, then 15 Brood War per player.
            io.put(0x58d088 + p * 46, &mut pl.upgrade_max[p][..46]);
            io.put(0x58f278 + p * 15, &mut pl.upgrade_max[p][46..]);
        }
        io.rows(0x58d2b0, 46, &mut pl.upgrade_sc);
        io.rows(0x58f32c, 15, &mut pl.upgrade_bw);
        io.rows(0x58f230, 6, &mut pl.research_in_progress);
        io.rows(0x58f3e0, 8, &mut pl.upgrade_in_progress);
        // 0x58d5b8 force flags, 0x58d634 alliances, 0x58d6c4 objectives.
        io.put(0x58d5b8, &mut self.force_flags);
        io.rows(0x58d634, 12, &mut self.alliances);
        let t = &mut self.trig;
        io.put(0x58d6c4, &mut t.objectives);
        io.one(0x58d6f4, &mut t.countdown);
        io.one(0x58d6f8, &mut t.elapsed);
        // 0x58d700 results of players 0..7 (the Rust extra four have no room before 0x58d708).
        io.put(0x58d700, &mut self.player_left[..8]);
        io.one(0x58d708, &mut t.leaderboard_computers);
        let lb = &mut t.leaderboard;
        io.one(0x58d70c, &mut lb.0);
        io.one(0x58d70d, &mut lb.1);
        io.one(0x58d70e, &mut lb.2);
        io.one(0x58d710, &mut lb.3);
        io.one(0x58d714, &mut lb.4);
        let mut sl = self.start_locations.map(|(x, y)| [x, y]);
        io.rows(0x58d720, 4, &mut sl);
        self.start_locations = sl.map(|[x, y]| (x, y));
        // 0x58dc40 switches, 0x58dc60 locations (255 of 0x14 bytes).
        io.put(0x58dc40, &mut t.switches);
        for (i, l) in t.locations.iter_mut().take(255).enumerate() {
            let a = 0x58dc60 + i * 0x14;
            let mut r = [l.left, l.top, l.right, l.bottom];
            io.put(a, &mut r);
            io.one(a + 0x10, &mut l.string);
            io.one(a + 0x12, &mut l.flags);
            [l.left, l.top, l.right, l.bottom] = r;
        }
        io.one(0x58f04c, &mut (t.countdown_paused as u32));
        let mut ex = self.is_expansion as u8;
        io.one(0x58f440, &mut ex);
        self.is_expansion = ex != 0;
        io.put(0x58f442, &mut self.color_override);
    }

    /// Fills the region from the state (`save`), or the state from `buf`; returns the bytes
    /// of the region no field covers.
    fn game_data_io(&mut self, buf: &mut [u8], save: bool) -> Vec<bool> {
        let mut io = Io { buf, save, cov: vec![false; LEN] };
        self.game_data(&mut io);
        io.cov
    }

    /// 0x57eee0 as the Rust state holds it: 12 entries of 0x24 bytes.
    pub fn player_table(&self) -> Vec<u8> {
        let mut t = vec![0u8; TABLE_LEN];
        for p in 0..12 {
            let e = &mut t[p * 0x24..(p + 1) * 0x24];
            e[slot::INDEX..slot::INDEX + 4].copy_from_slice(&(p as u32).to_le_bytes());
            // not kept: the storm (network) player id (+4)
            e[slot::TYPE] = self.player_types[p];
            e[slot::RACE] = self.player_races[p];
            e[slot::FORCE] = self.player_force[p];
            let n = self.player_names[p].as_bytes();
            let n = &n[..n.len().min(0x18)];
            e[slot::NAME..slot::NAME + n.len()].copy_from_slice(n);
        }
        t
    }

    /// 0x4d02d0: packed 0x57eee0 (0x1b0), packed 0x57f0f0 (0x17700), u32 0x512688.
    pub fn save_head(&mut self, _d: &Data, o: &mut Out) -> bool {
        let t = self.player_table();
        let mut ok = o.packed(&t);
        let mut g = vec![0u8; LEN];
        self.game_data_io(&mut g, true);
        ok &= o.packed(&g);
        // 0x512688: the local player's slot.
        o.u32(self.local_player as u32);
        ok
    }

    /// 0x4cf5f0 (after the header): the player table, the game data and u32 0x596bac, then
    /// its fix-ups for older save versions.
    pub fn load_head(&mut self, _d: &Data, i: &mut In) -> bool {
        let was_expansion = self.is_expansion;
        let Some(t) = i.packed(TABLE_LEN) else { return false };
        // A multiplayer game reads the table into a local (0x596888, the description being
        // in place, is not kept: taken as 0); otherwise it is 0x57eee0.
        if !self.multiplayer {
            for p in 0..12 {
                let e = &t[p * 0x24..(p + 1) * 0x24];
                self.player_types[p] = e[slot::TYPE];
                self.player_races[p] = e[slot::RACE];
                self.player_force[p] = e[slot::FORCE];
                let n = &e[slot::NAME..slot::NAME + 0x19];
                let n = &n[..n.iter().position(|&c| c == 0).unwrap_or(n.len())];
                self.player_names[p] = String::from_utf8_lossy(n).into_owned();
            }
        }
        let Some(mut g) = i.packed(LEN) else { return false };
        // u32 0x596bac: not kept.
        if i.u32().is_none() {
            return false;
        }
        // Older saves: 0x4cf5f0's copies, on the region before it is taken over.
        if VERSION < 2 {
            g.copy_within(0x58d6fc - BASE..0x58d6fc - BASE + 4, 0x58dc40 - BASE);
            g.copy_within(0x58d740 - BASE..0x58d740 - BASE + 0x500, 0x58dc60 - BASE);
        }
        if VERSION < 3 {
            g.copy_within(0x58d064 - BASE..0x58d064 - BASE + 0x24, 0x58f230 - BASE);
            g.copy_within(0x58d4d8 - BASE..0x58d4d8 - BASE + 0x48, 0x58f3e0 - BASE);
        }
        self.game_data_io(&mut g, false);
        // A Brood War save needs 0x6d11e4 (not kept, not checked) and both must agree.
        self.is_expansion == was_expansion
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Save, load into a fresh state, save again: the same bytes; lists what no field covers.
    #[test]
    fn region_roundtrip() {
        let mut a = State::new((64, 64), 0, 100);
        a.players.minerals[3] = 77;
        a.players.upgrade_max[5][50] = 2;
        a.players.tech_researched[2][30] = 1;
        a.trig.locations[200].left = 9;
        a.hotkeys[17][11] = 5;
        a.start_locations[7] = (3, 4);
        a.frame = 1234;
        let mut g = vec![0u8; LEN];
        let cov = a.game_data_io(&mut g, true);
        let mut b = State::new((64, 64), 0, 100);
        let mut g2 = g.clone();
        b.game_data_io(&mut g2, false);
        let mut g3 = vec![0u8; LEN];
        b.game_data_io(&mut g3, true);
        assert!(g == g3 && b.players.minerals[3] == 77 && b.trig.locations[200].left == 9);
        let mut s = String::new();
        let mut i = 0;
        while i < LEN {
            if cov[i] {
                i += 1;
                continue;
            }
            let j = (i..LEN).find(|&j| cov[j]).unwrap_or(LEN);
            s += &format!("{:#x}..{:#x} ", BASE + i, BASE + j);
            i = j;
        }
        eprintln!("UNCOVERED {s}");
    }
}
