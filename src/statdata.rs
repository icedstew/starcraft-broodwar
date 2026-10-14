//! The selected unit's status panel (1.16.1 statdata.cpp): the `rez\statdata.bin` dialog
//! and the building panels that show a training queue, research or upgrade progress.

use crate::bw::{Data, State};

/// One control of a 1.16.1 dialog `.bin` (0x56 bytes each).
#[derive(Clone, Debug)]
pub struct Ctl {
    pub id: i16,
    /// left, top, right, bottom (inclusive), relative to the dialog.
    pub rect: (i32, i32, i32, i32),
    pub ty: u16,
    pub flags: u32,
}

pub struct Dialog {
    /// Screen position of the dialog.
    pub origin: (i32, i32),
    pub controls: Vec<Ctl>,
}

impl Dialog {
    pub fn parse(d: &[u8]) -> Dialog {
        let i16_at = |o: usize| d.get(o..o + 2).map_or(0, |b| i16::from_le_bytes([b[0], b[1]]));
        let u32_at = |o: usize| d.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        let ctl = |o: usize| Ctl {
            id: i16_at(o + 0x20),
            rect: (i16_at(o + 4) as i32, i16_at(o + 6) as i32, i16_at(o + 8) as i32, i16_at(o + 10) as i32),
            ty: i16_at(o + 0x22) as u16,
            flags: u32_at(o + 0x18),
        };
        let dlg = ctl(0);
        let mut controls = Vec::new();
        let mut o = 0x56;
        while o + 0x56 <= d.len() {
            controls.push(ctl(o));
            let next = u32_at(o) as usize;
            if next == 0 || next <= o {
                break;
            }
            o = next;
        }
        Dialog { origin: (dlg.rect.0, dlg.rect.1), controls }
    }

    pub fn get(&self, id: i16) -> Option<&Ctl> {
        self.controls.iter().find(|c| c.id == id)
    }

    /// Screen rect of control `id`.
    pub fn screen_rect(&self, id: i16) -> Option<(i32, i32, i32, i32)> {
        let c = self.get(id)?;
        let (x, y) = self.origin;
        Some((x + c.rect.0, y + c.rect.1, x + c.rect.2, y + c.rect.3))
    }
}

/// What a shown control displays.
#[derive(Clone, Debug, PartialEq)]
pub enum Show {
    /// Centered text (labels).
    Text(String),
    /// Progress bar 0..=100 (0x456b40).
    Progress(u16),
    /// A button: background frame `bg` of the race's cmdbtns.grp and an icon:
    /// a cmdicons.grp frame (set 3, enabled) or a cmdbtns.grp frame (set 6, disabled).
    Button { bg: u16, icon: u16, cmdicon: bool },
}

/// Panel ids remembered in 0x68c1e5 (5 training, 7 research, 8 upgrade).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel {
    Training,
    Research,
    Upgrade,
}

/// Race index used for race-specific strings (0x6637a0 group flags: zerg 0, terran 1,
/// protoss 2, else 4).
fn race_index(d: &Data, t: u16) -> u16 {
    let g = d.dat.units.staredit_group_flags[t as usize];
    if g & 1 != 0 {
        0
    } else if g & 4 != 0 {
        2
    } else if g & 2 != 0 {
        1
    } else {
        4
    }
}

/// stat_txt string `id` (1-based as the game passes it).
fn stat(strings: &[String], id: u16) -> String {
    if id == 0 {
        return String::new();
    }
    strings.get(id as usize - 1).map_or(String::new(), |s| s.split('\0').next().unwrap_or("").to_string())
}

/// A building is shown disabled (lockdown, stasis, maelstrom, or status 0x400).
fn disabled(st: &State, u: u32) -> bool {
    let un = &st.units[u as usize];
    un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0
}

/// 0x427890, the building branch the queue view depends on: which panel the selected
/// completed building `u` shows, with its controls. None when another panel applies.
pub fn building_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Option<(Panel, Vec<(i16, Show)>)> {
    let un = &st.units[u as usize];
    let t = un.unit_type;
    if (0xb0..=0xb2).contains(&t) || t == 0xbc || un.status & 1 == 0 || t == 0x6c {
        return None;
    }
    if !st.replay && un.player != st.local_player {
        return None;
    }
    // 0x4e6ba0: units whose panel shows their cargo, not a queue (todo: those panels).
    if un.status & 0x4000_0000 == 0 && (t != 0x2a || st.players.upgrade_sc[un.player as usize][0x18] != 0) && d.dat.units.space_provided[t as usize] != 0 {
        return None;
    }
    // 0x401e70: something in the first queue slot.
    if un.build_queue[un.build_queue_slot as usize % 5] < 0x6a {
        return Some((Panel::Training, training_panel(st, d, strings, u)));
    }
    if un.u8_at(0xc8) != 0x2c {
        return Some((Panel::Research, research_panel(st, d, strings, u)));
    }
    if un.u8_at(0xc9) != 0x3d {
        return Some((Panel::Upgrade, upgrade_panel(st, d, strings, u)));
    }
    None
}

/// 0x4669e0: percent done of unit `b` being built (eggs and morphing buildings count
/// their queued type).
fn build_progress(st: &State, d: &Data, b: u32) -> u16 {
    let bu = &st.units[b as usize];
    let morphing = bu.status & 1 == 0 && matches!(bu.build_queue[bu.build_queue_slot as usize % 5], 0x85 | 0x84 | 0x89 | 0x90 | 0x92);
    let t = if matches!(bu.unit_type, 0x24 | 0x3b | 0x61) || morphing { bu.build_queue[bu.build_queue_slot as usize % 5] } else { bu.unit_type };
    let total = d.dat.units.build_time.get(t as usize).copied().unwrap_or(1).max(1) as i32;
    ((total - bu.remaining_build_time as i32) * 100 / total) as u16
}

/// 0x4268d0: the training queue: five slots (ids 2..6), the progress of the unit in
/// production (id 7) and the "Building/Training" label (id -15).
fn training_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Vec<(i16, Show)> {
    let un = &st.units[u as usize];
    let mut out = Vec::new();
    // Progress: the unit an SCV/probe/drone builds, or the building's current unit.
    let target = if matches!(un.order, 0x1e | 0x22 | 0x21) || is_building_unit(st, u) {
        un.order_target_unit
    } else if matches!(un.secondary_order, 0x26 | 0x3f) || is_training(st, u) {
        un.current_build_unit
    } else {
        crate::bw::list::NIL
    };
    let progress = if target != crate::bw::list::NIL { build_progress(st, d, target) } else { 0 };
    for i in 0..5 {
        let t = un.build_queue[(un.build_queue_slot as usize + i) % 5];
        let bg = if i == 0 || i > 3 { 2 } else { 4 };
        if t == 0xe4 {
            out.push((2 + i as i16, Show::Button { bg, icon: 6 + i as u16, cmdicon: false }));
        } else {
            out.push((2 + i as i16, Show::Button { bg, icon: t, cmdicon: true }));
        }
    }
    out.push((7, Show::Progress(progress)));
    // id 8 is shown too (an image control without content here).
    let first = un.build_queue[un.build_queue_slot as usize % 5];
    let s = if un.status & 2 != 0 && disabled(st, u) {
        0x33c
    } else {
        let morph = un.status & 1 == 0 && matches!(first, 0x85 | 0x84 | 0x89 | 0x90 | 0x92);
        let ty = if morph { first } else { un.unit_type };
        if ty == 0xa0 || ty == 0xa7 { 0x303 } else { 0x306 }
    };
    out.push((-15, Show::Text(stat(strings, s + race_index(d, un.unit_type)))));
    out
}

/// 0x4e4c40: a worker building something incomplete (order 0x1f).
fn is_building_unit(st: &State, u: u32) -> bool {
    let un = &st.units[u as usize];
    let t = un.order_target_unit;
    un.order == 0x1f && un.status & 2 != 0 && t != crate::bw::list::NIL && st.units[t as usize].status & 1 == 0
}

/// 0x4e66b0: a building training (secondary order 0x25) an incomplete unit.
fn is_training(st: &State, u: u32) -> bool {
    let un = &st.units[u as usize];
    let t = un.current_build_unit;
    un.secondary_order == 0x25 && un.status & 2 != 0 && t != crate::bw::list::NIL && st.units[t as usize].status & 1 == 0
}

/// 0x4266f0: research progress (id 7), the tech's icon (id 15) and label (id -15).
fn research_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Vec<(i16, Show)> {
    let un = &st.units[u as usize];
    let tech = un.u8_at(0xc8) as usize;
    let total = d.dat.techdata.research_time[tech].max(1) as u32;
    let left = un.u16_at(0xc6) as u32;
    let progress = (total.wrapping_sub(left) * 100 / total) as u16;
    let icon = d.dat.techdata.icon[tech];
    let s = if disabled(st, u) { 0x33c } else { 0x300 };
    vec![
        (7, Show::Progress(progress)),
        (15, Show::Button { bg: 2, icon, cmdicon: true }),
        (-15, Show::Text(stat(strings, s + race_index(d, un.unit_type)))),
    ]
}

/// 0x426500: upgrade progress (id 7), the upgrade's icon (id 15) and label (id -15).
fn upgrade_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Vec<(i16, Show)> {
    let un = &st.units[u as usize];
    let up = un.u8_at(0xc9) as usize;
    // 0x453f70: the time of the level being researched.
    let lvl = st.players.upgrade_level(un.player, up) as u32;
    let total = (d.dat.upgrades.time_factor[up] as u32 * lvl + d.dat.upgrades.time_base[up] as u32) & 0xffff;
    let left = un.u16_at(0xc6) as u32;
    let progress = (total.max(1).wrapping_sub(left) * 100 / total.max(1)) as u16;
    let icon = d.dat.upgrades.icon[up];
    let s = if disabled(st, u) { 0x33c } else { 0x2fd };
    vec![
        (7, Show::Progress(progress)),
        (15, Show::Button { bg: 2, icon, cmdicon: true }),
        (-15, Show::Text(stat(strings, s + race_index(d, un.unit_type)))),
    ]
}

/// 0x4573a0: clicking a panel button: queue slot `id - 2` is cancelled (command 0x20);
/// the research / upgrade icon cancels it (0x423330 / 0x4232f0: commands 0x31 / 0x33).
pub fn click(panel: Panel, id: i16) -> Option<Vec<u8>> {
    match (panel, id) {
        (Panel::Training, 2..=6) => Some(vec![0x20, (id - 2) as u8, 0]),
        (Panel::Research, 15) => Some(vec![0x31]),
        (Panel::Upgrade, 15) => Some(vec![0x33]),
        _ => None,
    }
}
