//! Unit voices (front end): the "what" / annoyed lines when a unit is selected
//! (0x48f910) and the "yes" lines when it is given an order (0x48f4d0). They pick with
//! the C runtime's rand(), not the synced RNG, and only play for the local player's
//! units outside replays.

use crate::bw::{self, Data};

#[derive(Default)]
pub struct Voices {
    /// MSVC _holdrand (starts at 1).
    holdrand: u32,
    /// 0x6d5be0: the line played last (not repeated twice in a row).
    last: u32,
    /// 0x640874 / 0x640548 / 0x64087c: unit clicked last, clicks on it, repeat delay.
    clicked: u32,
    clicks: i32,
    delay: u32,
    /// 0x6cdfec (2 acknowledgements, 4 building sounds) and whether the digital volume
    /// 0x6cdfe4 is above 0: 0x48ed50 plays nothing otherwise.
    pub sound_flags: u32,
    pub muted: bool,
}

impl Voices {
    pub fn new() -> Voices {
        Voices { holdrand: 1, clicked: bw::list::NIL, sound_flags: 7, ..Default::default() }
    }

    fn rand(&mut self) -> u32 {
        self.holdrand = self.holdrand.wrapping_mul(214013).wrapping_add(2531011);
        (self.holdrand >> 16) & 0x7fff
    }

    /// 0x48e940: a line from lo..=hi, not the last one again.
    fn pick(&mut self, lo: u32, hi: u32) -> u32 {
        if lo == hi {
            self.last = lo;
            return lo;
        }
        let mut v = (self.rand() >> 8) % (hi - lo + 1) + lo;
        if v == self.last {
            v += 1;
            if v > hi {
                v = lo;
            }
        }
        self.last = v;
        v
    }

    /// Once per frame (the repeat delay counts down).
    pub fn tick(&mut self) {
        self.delay = self.delay.saturating_sub(1);
    }

    /// 0x48f4d0: `u` acknowledges an order.
    pub fn yes(&mut self, st: &bw::State, d: &Data, u: u32) -> Option<u16> {
        self.clicked = bw::list::NIL;
        let un = &st.units[u as usize];
        if st.replay || un.player != st.local_player || disabled(un) {
            return None;
        }
        // 0x48eab0
        let t = un.unit_type as usize;
        let lo = *d.dat.units.yes_sound_start.get(t)? as u32;
        if t >= 0x6a || lo == 0 {
            return None;
        }
        let last = self.last;
        let id = self.pick(lo, d.dat.units.yes_sound_end[t] as u32) as u16;
        if self.sound_flags & 2 == 0 || self.muted {
            self.last = last;
            return None;
        }
        Some(id)
    }

    /// 0x48f910: `u` was selected.
    pub fn what(&mut self, st: &bw::State, d: &Data, u: u32) -> Option<u16> {
        let un = &st.units[u as usize];
        // 0x48eb30: own units (and neutral minerals, geysers and critters) outside replays.
        let neutral_ok = un.player == 0xb && matches!(un.unit_type, 0xb0..=0xb2 | 0xbc | 0x59..=0x5a | 0x5d | 0x5e | 0x5f | 0x60);
        if st.replay || !(un.player == st.local_player || neutral_ok) {
            self.clicked = bw::list::NIL;
            self.clicks = 0;
            return None;
        }
        if self.clicked == u && self.delay != 0 {
            return None;
        }
        self.delay = 10;
        if self.clicked != u {
            self.clicked = u;
            self.clicks = 0;
        }
        let t = un.unit_type as usize;
        let building = d.dat.units.flags[t] & 1 != 0 && un.status & 1 != 0;
        let last = self.last;
        let g = d.dat.units.staredit_group_flags[t];
        let id = if !building {
            let n = (self.clicks - 3).max(0);
            // ponytail: critters explode after many clicks (0x47b450 / 0x475710) not ported.
            if disabled(un) { 0xf } else { self.what_line(d, t, n) }
        } else {
            let pct = hp_percent(d, un);
            if pct < 0x22 && g & 2 != 0 {
                0x193
            } else if pct < 0x22 && g & 4 != 0 {
                0x12
            } else if (0x22..0x43).contains(&pct) && g & 6 != 0 {
                0x12
            } else if disabled(un) {
                // 0x47b150: race + 0x72
                let race = if g & 1 != 0 { 0 } else if g & 2 != 0 { 1 } else if g & 4 != 0 { 2 } else { 4 };
                race + 0x72
            } else {
                self.what_line(d, t, 0)
            }
        };
        if id == 0 {
            return None;
        }
        // 0x48ed50: buildings by flag 4, units by flag 2; unplayed lines are not
        // remembered (0x6d5be0 restored).
        let bit = if d.dat.units.flags[t] & 1 != 0 { 4 } else { 2 };
        if self.sound_flags & bit == 0 || self.muted {
            self.last = last;
            return None;
        }
        if !building {
            self.clicks += 1;
        }
        Some(id as u16)
    }

    /// 0x48ea20: the annoyed line `n` once clicked enough, else a "what" line.
    fn what_line(&mut self, d: &Data, t: usize, n: i32) -> u32 {
        if t < 0x6a {
            let ps = d.dat.units.piss_sound_start[t] as i32;
            if ps != 0 {
                if d.dat.units.piss_sound_end[t] as i32 - ps + 1 < n {
                    self.clicks = 0;
                } else if n != 0 {
                    return (ps - 1 + n) as u32;
                }
            }
        }
        self.pick(d.dat.units.what_sound_start[t] as u32, d.dat.units.what_sound_end[t] as u32)
    }
}

/// 0x4020b0
fn disabled(un: &bw::unit::Unit) -> bool {
    un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0
}

fn hp_percent(d: &Data, un: &bw::unit::Unit) -> i32 {
    let hp = (un.hp + 0xff) >> 8;
    let mut max = d.dat.units.hit_points[un.unit_type as usize] as i32 >> 8;
    if max == 0 {
        max = hp.max(1);
    }
    hp * 100 / max
}
