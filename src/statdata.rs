//! The selected unit's status panel (1.16.1 statdata.cpp): the `rez\statdata.bin` dialog
//! and the building panels that show a training queue, research or upgrade progress.

use crate::bw::list::NIL;
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
    /// An info button (ids 9..12, 0x425600 and kin): a cmdicons.grp icon and the
    /// upgrade level / count shown with it ("%d ").
    Info { icon: u16, level: u8, tip: String },
    /// The unit's wireframe (control 1, statwire.cpp).
    Wireframe,
    /// A small wireframe of selected unit `u` (multiple selection, ids 33..44).
    GroupWire(u32),
    /// A passenger: cmdbtns.grp frame `bg` with its tranwire.grp wireframe.
    Cargo { bg: u16, unit: u32 },
}

/// Panel ids remembered in 0x68c1e5 (5 training, 7 research, 8 upgrade).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel {
    Training,
    Research,
    Upgrade,
    /// 0x426f50: a unit's (or idle building's) stats.
    Unit,
    /// 0x425960: several units selected.
    Group,
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
    // The status dispatcher 0x458120 uses 0x427890 for these types (table 0x5193a0).
    if !matches!(t, 0x6a..=0x76 | 0x78..=0x7c | 0x7e..=0x7f | 0x82..=0x98 | 0x9a..=0x9d | 0x9f..=0xbe | 0xc2..=0xca | 0xd6) {
        return None;
    }
    if un.status & 1 == 0 && !(0xb0..=0xb2).contains(&t) && t != 0xbc && (st.replay || un.player == st.local_player) {
        // 0x4260c0: under construction: progress (13) and "Under construction" (-30),
        // with the hit points, status line and name (0x427a50).
        let race = race_index(d, t);
        let mut out = vec![(13, Show::Progress(build_progress(st, d, u))), (-30, Show::Text(stat(strings, race + 0x31f)))];
        out.extend(unit_panel(st, d, strings, u).into_iter().filter(|(id, _)| matches!(id, 1 | -7 | -20 | -5)));
        return Some((Panel::Unit, out));
    }
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
    if d.dat.units.supply_provided[crate::ui::wire_type(st, u) as usize] != 0 && (st.replay || un.player == st.local_player) {
        return Some((Panel::Unit, supply_panel(st, d, strings, u)));
    }
    None
}

/// 0x427540: a supply provider: supplies used, provided by it, total and max
/// (ids -10..-13), next to the wireframe, name and hit points.
fn supply_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Vec<(i16, Show)> {
    let un = &st.units[u as usize];
    let race = race_index(d, un.unit_type);
    let p = un.player as usize;
    let r = race as usize;
    let used = if r < 3 { st.players.supply_used[r][p] } else { 0 };
    let total = if r < 3 { st.players.supply_provided[r][p].min(st.players.supply_max[r][p]) } else { 0 };
    let shown = crate::ui::wire_type(st, u) as usize;
    let mr = race_index(d, shown as u16) as usize;
    let max = if mr < 3 { st.players.supply_max[mr][p] } else { 0 };
    let mut out: Vec<(i16, Show)> = unit_panel(st, d, strings, u)
        .into_iter()
        .filter(|(id, _)| matches!(id, 1 | -5 | -7) || (*id == 9 && d.dat.units.shield_enable[un.unit_type as usize] != 0) || (*id == 9 && matches!(un.unit_type, 0x2a | 0x39)))
        .collect();
    out.push((-10, Show::Text(format!("{} {}", stat(strings, race + 0x335), (used + 1) / 2))));
    out.push((-11, Show::Text(format!("{} {}", stat(strings, race + 0x32f), (d.dat.units.supply_provided[shown] as i32 + 1) / 2))));
    out.push((-12, Show::Text(format!("{} {}", stat(strings, race + 0x332), (total + 1) / 2))));
    out.push((-13, Show::Text(format!("{} {}", stat(strings, race + 0x338), (max as u32 + 1) >> 1))));
    out
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
    let total = d.dat.techdata.research_time.get(tech).copied().unwrap_or(1).max(1) as u32;
    let left = un.u16_at(0xc6) as u32;
    let progress = (total.wrapping_sub(left) * 100 / total) as u16;
    let icon = d.dat.techdata.icon.get(tech).copied().unwrap_or(0);
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

/// 0x426f50: a selected unit's panel: wireframe (1), name (-5), hit points (-7),
/// energy (-8), status line (-20), kills (-21) and the info buttons (9..12).
pub fn unit_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Vec<(i16, Show)> {
    let un = &st.units[u as usize];
    let t = un.unit_type;
    let ts = t as usize;
    let mine = st.replay || un.player == st.local_player;
    let halluc_own = un.status & 0x4000_0000 != 0 && mine;
    let flags = d.dat.units.flags[ts];
    let shown = crate::ui::wire_type(st, u);
    let mut out = vec![(1, Show::Wireframe)];
    // 0x426c60: info buttons.
    let mut slot = 0i16;
    if !halluc_own {
        let mut push = |slot: &mut i16, icon: u16, level: u8, tip: String| {
            out.push((9 + *slot, Show::Info { icon, level, tip }));
            *slot += 1;
        };
        // 0x4576a0: "<name>\n<Damage:|Armor:> base[+bonus]".
        let line = |name: String, what: u16, base: u32, bonus: u32| {
            if bonus == 0 { format!("{name}\n{} {base}", stat(strings, what)) } else { format!("{name}\n{} {base}+{bonus}", stat(strings, what)) }
        };
        // 0x475f10 / 0x475e40: a weapon's damage and upgrade bonus, times its hits.
        let factor = |w: u8| match t {
            0x20 | 10 | 0x41 | 0x4d => 2,
            0x3a => 1,
            _ => d.dat.weapons.damage_factor[w as usize] as u32,
        };
        let weapon_tip = |w: u8, lvl: u8| {
            let wi = w as usize;
            let name = stat(strings, d.dat.weapons.label[wi]);
            let base = d.dat.weapons.damage[wi] as u32 * factor(w);
            let bonus = d.dat.weapons.damage_bonus[wi] as u32 * lvl as u32 * factor(w);
            line(name, 0x309, base, bonus)
        };
        let armor_up = d.dat.units.armor_upgrade[shown as usize];
        if armor_up != 0x3c {
            // 0x425600
            let lvl = st.armor_level(d, u);
            let tip = line(stat(strings, d.dat.upgrades.label[armor_up as usize]), 0x30a, d.dat.units.armor[shown as usize] as u32, lvl);
            push(&mut slot, d.dat.upgrades.icon[armor_up as usize], lvl as u8, tip);
        }
        if d.dat.units.shield_enable[ts] != 0 {
            // 0x425510: Plasma Shields
            let lvl = st.players.upgrade_sc[un.player as usize][0xf];
            let tip = line(stat(strings, d.dat.upgrades.label[0xf]), 0x30b, 0, lvl as u32);
            push(&mut slot, 0x136, lvl, tip);
        }
        let sub = un.subunit;
        let sub_w = |table: &[u8]| if sub != NIL { table[st.units[sub as usize].unit_type as usize] } else { 0x82 };
        let burrowed_lurker_off = t == 0x67 && un.status & 0x10 == 0;
        let gw = d.dat.units.ground_weapon[ts];
        let gw = if gw == 0x82 { sub_w(&d.dat.units.ground_weapon) } else { gw };
        let weapon_level = |w: u8| {
            let up = d.dat.weapons.damage_upgrade[w as usize] as usize;
            st.players.upgrade_level(un.player, up)
        };
        if !burrowed_lurker_off && gw != 0x82 {
            push(&mut slot, d.dat.weapons.icon[gw as usize], weapon_level(gw), weapon_tip(gw, weapon_level(gw)));
        }
        let aw = d.dat.units.air_weapon[ts];
        let aw = if aw == 0x82 { sub_w(&d.dat.units.air_weapon) } else { aw };
        let gw_cmp = if burrowed_lurker_off { 0x82 } else { gw };
        if aw != 0x82 && aw != gw_cmp {
            push(&mut slot, d.dat.weapons.icon[aw as usize], weapon_level(aw), weapon_tip(aw, weapon_level(aw)));
        }
        match t {
            0x48 | 0x52 => push(&mut slot, 0x49, un.u8_at(0xc9).wrapping_add(un.u8_at(0xc8)), stat(strings, 0x317)),
            0x53 | 0x51 => push(&mut slot, 0x55, un.u8_at(0xc8), stat(strings, 0x318)),
            _ => {}
        }
        if (t == 2 || t == 0x13) && st.can_use_tech(d, u, 3, un.player) == Ok(1) {
            // 0x426300: spider mines
            push(&mut slot, 0xd, un.u8_at(0xc0), stat(strings, 0x31a));
        }
        if shown == 0x6c {
            // 0x425310: a nuke in the silo
            push(&mut slot, 0xe, (un.ptr_at(0xd0) != NIL) as u8, stat(strings, 0x319));
        }
    }
    // 0x425b50: the status line.
    let status = if halluc_own {
        Some(stat(strings, 0x340))
    } else if flags & 1 != 0 && un.status & 1 != 0 && disabled(st, u) && un.u8_at(0xc8) == 0x2c && un.u8_at(0xc9) == 0x3d {
        Some(stat(strings, 0x33c + race_index(d, t)))
    } else if un.status & 1 != 0 && un.acid_spore_count != 0 {
        Some(format!("{} ({})", stat(strings, 0x514), un.acid_spore_count))
    } else if un.status & 1 != 0 && un.is_blind != 0 {
        Some(stat(strings, 0x513))
    } else if un.status & 1 != 0 && un.parasite_flags != 0 {
        Some(stat(strings, 0x33b))
    } else if flags & 0x8000 != 0 && un.status & 1 != 0 && !disabled(st, u) && un.is_blind == 0 {
        Some(stat(strings, 0x33f))
    } else if flags & 1 == 0 {
        rank(st, d, strings, u)
    } else {
        None
    };
    let status = if (0xb0..=0xb2).contains(&t) || (t == 0xbc) || (un.status & 1 != 0 && matches!(t, 0x6e | 0x95 | 0x9d)) {
        // 0x426190: resources left.
        let n = un.u16_at(0xd0);
        if (0xb0..=0xb2).contains(&t) {
            Some(format!("{} {n}", stat(strings, 0x31c)))
        } else if n == 0 {
            Some(stat(strings, 0x31e))
        } else {
            Some(format!("{} {n}", stat(strings, 0x31d)))
        }
    } else {
        status
    };
    if let Some(s) = status {
        out.push((-20, Show::Text(s)));
    }
    // 0x425dd0: kills, for units that can fight.
    if !(t == 0x2f || t == 0x32 || halluc_own) && (can_fight(st, d, u) || un.u8_at(0x8f) != 0) {
        out.push((-21, Show::Text(format!("{} {}", stat(strings, 0x2fc), un.u8_at(0x8f)))));
    }
    // 0x4263e0: hit points (red under 34%, yellow under 67%), shields first.
    let max_hp = d.dat.units.hit_points[ts] as i32 >> 8;
    let max_sh = d.dat.units.shields[shown as usize] as i32;
    if max_hp < 10000 && max_sh < 10000 && un.status & 0x400_0000 == 0 {
        let hp = (un.hp + 0xff) >> 8;
        let m = if max_hp == 0 { hp.max(1) } else { max_hp };
        let pct = hp * 100 / m;
        let c = if pct < 0x22 { '\x06' } else if pct < 0x43 { '\x03' } else { '\x07' };
        let s = if d.dat.units.shield_enable[ts] == 0 {
            format!("{c}{hp}/{max_hp}")
        } else {
            format!("{}/{} {c}{hp}/{max_hp}", un.shields >> 8, max_sh)
        };
        out.push((-7, Show::Text(s)));
    }
    // 0x425a30: energy (or a hallucination's / broodling's time left).
    if flags & 0x20_0000 != 0 && un.status & 0x4000_0000 == 0 && mine {
        out.push((-8, Show::Text(format!("\x04{}/{}", un.energy >> 8, st.max_energy(d, u) >> 8))));
    } else if (un.status & 0x4000_0000 != 0 || t == 0x28) && mine {
        let total = if un.status & 0x4000_0000 != 0 { 0x546 } else { 0x708 };
        out.push((-8, Show::Text(format!("\x04{}/{}", un.remove_timer / 10, total / 10))));
    }
    // 0x47b090: the name (heroes and the like have their own string table entry).
    out.push((-5, Show::Text(stat(strings, t + 1))));
    out
}

/// 0x42f370: a Terran unit's rank, a hero's title.
fn rank(st: &State, d: &Data, strings: &[String], u: u32) -> Option<String> {
    let un = &st.units[u as usize];
    let t = un.unit_type;
    let ts = t as usize;
    let flags = d.dat.units.flags[ts];
    if flags & 0x800 != 0 || t == 0xf || t == 0xd {
        return None;
    }
    if t == 0x52 {
        // ponytail: the hero carrier's own name table (0x660304) is not loaded.
        return Some(stat(strings, 0x23c));
    }
    if matches!(t, 0xc | 0x1d | 0x1c) && flags & 0x40 != 0 {
        return Some(stat(strings, 0x23b));
    }
    if race_index(d, t) != 1 {
        return None;
    }
    let r = d.dat.units.rank[ts].wrapping_add(un.u8_at(0x8e)) as u16;
    Some(stat(strings, r + 0x516))
}

/// 0x476180: the unit (or its turret) has a weapon, or carries interceptors / scarabs.
fn can_fight(st: &State, d: &Data, u: u32) -> bool {
    let un = &st.units[u as usize];
    let t = un.unit_type as usize;
    let gw = if un.unit_type == 0x67 && un.status & 0x10 == 0 { 0x82 } else { d.dat.units.ground_weapon[t] };
    if gw != 0x82 || d.dat.units.air_weapon[t] != 0x82 {
        return true;
    }
    match un.unit_type {
        0x48 | 0x52 if un.u8_at(0xc9).wrapping_add(un.u8_at(0xc8)) != 0 => return true,
        0x53 | 0x51 if un.u8_at(0xc8) != 0 => return true,
        _ => {}
    }
    let sub = un.subunit;
    if sub == NIL {
        return false;
    }
    let st_ = st.units[sub as usize].unit_type as usize;
    d.dat.units.ground_weapon[st_] != 0x82 || d.dat.units.air_weapon[st_] != 0x82
}

/// 0x425960: a multiple selection: each unit's small wireframe in ids 33.. (order of
/// the selection).
pub fn group_panel(sel: &[u32]) -> Vec<(i16, Show)> {
    sel.iter().take(12).enumerate().map(|(i, &u)| (33 + i as i16, Show::GroupWire(u))).collect()
}

/// 0x427c90: a transport (dropship, shuttle, bunker, ...) with passengers shows them
/// (0x424ba0) instead of its stats, with its hit points and name.
pub fn transport_panel(st: &State, d: &Data, strings: &[String], u: u32) -> Option<Vec<(i16, Show)>> {
    let un = &st.units[u as usize];
    if !(st.replay || un.player == st.local_player) || un.status & 0x4000_0000 != 0 || !st.has_loaded_units(u) {
        return None;
    }
    let cap = d.dat.units.space_provided[un.unit_type as usize];
    let small = cap < 7;
    let (mut big, mut mid, mut one) = (0x12 + if small { 2 } else { 0 }, 0x15 + small as i16, 0x19 + if small { 2 } else { 0 });
    let mut out = vec![];
    for size in [4u8, 2, 1] {
        for i in 0..8 {
            let Some(c) = st.loaded_unit(u, i) else { continue };
            let cu = &st.units[c as usize];
            let ct = cu.unit_type as usize;
            if d.dat.units.space_required[ct] != size {
                continue;
            }
            let hero = d.dat.units.flags[ct] & 0x40 != 0;
            let para = if cu.parasite_flags != 0 { 0xc } else { 0 };
            let halluc = cu.status & 0x4000_0000 != 0;
            let (id, bg) = match size {
                4 => (big, if halluc { 0x23 } else if hero { 0x17 } else { 0x11 + para }),
                2 => (mid, if halluc { 0x21 } else if hero { 0x15 } else { 0xf + para }),
                _ => (one, if halluc { 0x1f } else if hero { 0x13 } else { 0xd + para }),
            };
            out.push((id, Show::Cargo { bg, unit: c }));
            big += (size >> 2) as i16;
            mid += (size >> 1) as i16;
            one += size as i16;
        }
    }
    let base = unit_panel(st, d, strings, u);
    out.extend(base.into_iter().filter(|(id, _)| matches!(id, 1 | -7 | -5)));
    Some(out)
}
