//! Unit voices (front end): the "what" / annoyed lines when a unit is selected
//! (0x48f910) and the "yes" lines when it is given an order (0x48f4d0). They pick with
//! the C runtime's rand(), not the synced RNG, and only play for the local player's
//! units outside replays.

use crate::bw::{self, Data};
use crate::todo_port;

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
    /// 0x48eab0: the yes sound pick, type < 0x6a with a yes range (inline here).
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
    /// 0x48eb30: own units (and neutral minerals, geysers, critters) outside replays (inline here).
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

/// 0x4556d0: `u` can take part in an acknowledged order (not disabled, status 0x10 only for
/// type 0x67, not in the busy order states of a few types, right-click kind != 0).
pub fn can_order(st: &bw::State, d: &Data, u: u32) -> bool {
    let un = &st.units[u as usize];
    let (t, o) = (un.unit_type, un.order);
    !disabled(un)
        && (un.status & 0x10 == 0 || t == 0x67)
        && !(t == 7 && o == 0x21)
        && !(matches!(t, 1 | 0x10 | 100 | 99 | 0x68) && o == 0x81)
        && !(t == 0x44 && o == 0x6a)
        && st.right_click_kind(d, u) != 0
}

/// 0x455660: `u` has a right-click kind of 1, 2, 4, 5 or 6 (0x4e5ea0 kind).
pub fn can_right_click(st: &bw::State, d: &Data, u: u32) -> bool {
    matches!(st.right_click_kind(d, u), 1 | 2 | 4 | 5 | 6)
}

/// The two selection searches shared by 0x4563a0 and 0x4564e0: some selected unit
/// passes 0x4556d0 and (a later search) some passes 0x455660.
fn order_gate(st: &bw::State, d: &Data, sel: &bw::buttons::Selection, right_click: bool) -> bool {
    let any = |f: &dyn Fn(u32) -> bool| sel.units.iter().any(|&u| u != bw::list::NIL && f(u));
    any(&|u| can_order(st, d, u)) && (!right_click || any(&|u| can_right_click(st, d, u)))
}

/// 0x4563a0 (the voice part): the active unit (0x597248) acknowledges the order
/// when it is the local player's and the selection passes both searches. The click
/// marker (0x485bd0 / 0x488660) is the caller's.
pub fn order_ack_gate(st: &bw::State, d: &Data, sel: &bw::buttons::Selection) -> bool {
    sel.active != bw::list::NIL
        && st.units[sel.active as usize].player == st.local_player
        && order_gate(st, d, sel, true)
}

impl bw::State {
    /// 0x4564e0: the mouse moved over the map at map-screen point `p` (event x, y; `screen`
    /// = 0x62848c / 0x6284a8; `over_hud` = 0x4d1140). `sent0` is the first unit of the
    /// selection sent to the sim (0x59724c). Sends the right click (0x4c0380; `queued` =
    /// 0x596a28) and returns the point, sprite id and whether 0x48f4d0 voices the active unit.
    #[allow(dead_code)]
    pub fn hover_cursor(&mut self, d: &Data, sel: &bw::buttons::Selection, sent0: u32, p: (i32, i32), screen: (i32, i32), over_hud: bool, queued: bool) -> Option<((i32, i32), u16, bool)> {
        let nil = bw::list::NIL;
        if over_hud {
            return None;
        }
        let owner = if sent0 == nil { 0xff } else { self.units[sent0 as usize].player };
        let active = sel.active;
        if owner != self.local_player || active == nil || !self.is_local_unit(active) || !order_gate(self, d, sel, false) {
            return None;
        }
        let (x, y) = (p.0 + screen.0, p.1 + screen.1);
        let t = self.unit_at_point(d, x as i16, y as i16);
        let rally_self = t != nil && self.units[t as usize].status & 2 != 0 && bw::command::has_rally(self.units[t as usize].unit_type);
        if sel.count == 1 && active == t && !rally_self {
            return None;
        }
        if t == nil && !order_gate_rc(self, d, sel) {
            return None;
        }
        // 0x456490: the fog sprite under the point (0x487a10) is not ported; else the
        // unit's circle flashes or the marker shows at the point.
        todo_port!("0x487a10", "fog sprite at point");
        if t != nil {
            let s = self.units[t as usize].sprite;
            self.sprites[s as usize].selection_timer = 0x1f;
        } else {
            self.show_marker(d, x as i16, y as i16);
        }
        // 0x4c0380: the right click command (`queued` = the shift state).
        let player = self.local_player;
        self.command_right_click_packet(d, player, &self.selected.clone(), (x as i16, y as i16), t, 0xe4, queued);
        let ack = self.right_click_accepted(d, &self.selected.clone(), active, t);
        Some(((x, y), 0xe4, ack))
    }
}

fn order_gate_rc(st: &bw::State, d: &Data, sel: &bw::buttons::Selection) -> bool {
    sel.units.iter().any(|&u| u != bw::list::NIL && can_right_click(st, d, u))
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
