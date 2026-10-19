//! The trigger leaderboard's values (statlb.cpp's data side): 0x4538b0 fills the table
//! 0x68c308 (player, value) x 8 from the leaderboard action's settings 0x58d70c.. and
//! sorts it; the dialog (rez\statlb.bin, 0x4be670 / 0x4be860) shows it (`crate::leaderboard`).

use super::crt::qsort;
use super::triggers::Count;
use super::{Data, State};
use std::cmp::Ordering;

/// An unused table row (0x68c308 = -1).
pub const NO_PLAYER: u32 = u32::MAX;

/// 0x4536c0: the qsort order of the leaderboard values: largest first, unused rows last.
fn cmp_desc(a: &(u32, u32), b: &(u32, u32)) -> Ordering {
    let r: i32 = if a.0 == NO_PLAYER {
        (b.0 != NO_PLAYER) as i32
    } else if b.0 != NO_PLAYER && a.1 <= b.1 {
        (a.1 < b.1) as i32
    } else {
        -1
    };
    r.cmp(&0)
}

/// 0x453680: the order of the goal leaderboards: smallest first, unused rows last.
fn cmp_asc(a: &(u32, u32), b: &(u32, u32)) -> Ordering {
    let r: i32 = if a.0 == NO_PLAYER {
        (b.0 != NO_PLAYER) as i32
    } else if b.0 != NO_PLAYER && b.1 <= a.1 {
        (b.1 < a.1) as i32
    } else {
        -1
    };
    r.cmp(&0)
}

impl State {
    /// 0x453790 (EBX = the counter, DX = 0x58d70e): every listed player (human, or any with
    /// 0x58d708) gets the counter's value.
    fn leaderboard_values_of(&mut self, what: Count) {
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            if self.player_types[p as usize] == 2 || self.trig.leaderboard_computers != 0 {
                let v = self.player_count(p, what);
                self.trig.leaderboard_table[p as usize] = (p, v);
            }
        }
    }

    /// 0x453710 (EBX = the counter's type): adds to every listed player what is missing to
    /// the goal 0x58d710.
    fn leaderboard_goal_of(&mut self, what: Count) {
        let goal = self.trig.leaderboard.3;
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            if self.player_types[p as usize] == 2 || self.trig.leaderboard_computers != 0 {
                let v = self.player_count(p, what);
                let missing = if v < goal { goal - v } else { 0 };
                let row = &mut self.trig.leaderboard_table[p as usize];
                row.1 = row.1.wrapping_add(missing);
                row.0 = p;
            }
        }
    }

    /// 0x453800: the score goal of the total (0), units + buildings (3) and kills + razings
    /// (6) types: each of its parts is a goal on its own.
    fn leaderboard_score_goal(&mut self) {
        let ty = self.trig.leaderboard.2;
        if ty == 0 {
            for k in [7, 1, 2] {
                self.leaderboard_goal_of(Count::Score(k));
            }
        } else if ty == 3 {
            for k in [1, 2] {
                self.leaderboard_goal_of(Count::Score(k));
            }
            return;
        } else if ty != 6 {
            return;
        }
        for k in [4, 5] {
            self.leaderboard_goal_of(Count::Score(k));
        }
    }

    /// 0x4538b0 (every 16 game frames, 0x489cc0): the leaderboard table for the action
    /// 0x58d70c: 0x11..0x15 the values of units (completed), resources, kills or score;
    /// 0x21..0x25 and 0x28 how far each player is from the goal 0x58d710.
    pub fn leaderboard_update(&mut self, d: &Data) {
        self.trig.leaderboard_table = [(NO_PLAYER, 0); 8];
        let (mode, loc, ty) = (self.trig.leaderboard.0, self.trig.leaderboard.1, self.trig.leaderboard.2);
        let res = |ty: u16| Count::Resources(if ty < 256 { ty as u8 } else { 255 });
        match mode {
            0x11 | 0x12 => {
                if mode == 0x11 {
                    self.snapshot_counts();
                } else {
                    self.snapshot_location(d, loc);
                }
                self.leaderboard_values_of(Count::Units(ty, true));
            }
            0x13 => self.leaderboard_values_of(res(ty)),
            0x14 => self.leaderboard_values_of(Count::Kills(ty)),
            0x15 => self.leaderboard_values_of(Count::Score(if ty < 256 { ty as u8 } else { 255 })),
            _ if matches!(mode, 0x21..=0x25 | 0x28) => {
                match mode {
                    0x21 | 0x22 => {
                        if mode == 0x21 {
                            self.snapshot_counts();
                        } else {
                            self.snapshot_location(d, loc);
                        }
                        self.leaderboard_goal_of(Count::Units(ty, true));
                    }
                    0x23 if ty != 2 => self.leaderboard_goal_of(res(ty)),
                    0x24 => self.leaderboard_goal_of(Count::Kills(ty)),
                    0x25 if !matches!(ty, 0 | 3 | 6) => self.leaderboard_goal_of(Count::Score(if ty < 256 { ty as u8 } else { 255 })),
                    0x25 => self.leaderboard_score_goal(),
                    // 0x23 with both resources, 0x28.
                    _ => {
                        self.leaderboard_goal_of(Count::Resources(0));
                        self.leaderboard_goal_of(Count::Resources(1));
                    }
                }
                qsort(&mut self.trig.leaderboard_table, cmp_asc);
                return;
            }
            _ => return,
        }
        qsort(&mut self.trig.leaderboard_table, cmp_desc);
    }
}
