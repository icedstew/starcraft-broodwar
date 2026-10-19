//! The score screen after a game (rez\gluScore.bin, screen function 0x4b4af0, dialog event
//! proc 0x4b4a30): one row per player with its race, name and color, three bars and
//! numbers that count up (the timer 0x22 every 20 ms, 0x4b3670) with sound 0xa8, and the
//! total score; the tabs (Overview, Units, Structures, Resources) switch what the rows show.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state. The controls'
//! own procs (table 0x51a848) and draws (+0x2e) are `Ctl::score` / `Update::Score`
//! (dlg.rs); the game's score arrays 0x581dfc.. are `GameEnd::players`.

use super::{AudioCmd, Glue, Open};
use crate::dlg::{flag, Dialog, Icon, Image, ScoreCtl, Update};
use crate::grp::Grp;
use crate::setup::slot;
use crate::storage::Storage;
use crate::todo_port;
use std::rc::Rc;

/// The controls of gluScore.bin: the player rows are groups of six from id 13 (+ 6 * row):
/// the backdrop (pInset.pcx), the name, three bars with their numbers, the total's box.
mod ids {
    pub const BACKGROUND: i16 = 1;
    pub const TITLE: i16 = 2;
    /// The tabs 3 .. 6 (Overview, Units, Structures, Resources).
    pub const TAB_FIRST: i16 = 3;
    pub const TAB_LAST: i16 = 6;
    pub const OK: i16 = 7;
    pub const TOTAL_LABEL: i16 = 8;
    pub const TIME: i16 = 9;
    pub const SAVE_REPLAY: i16 = 61;
    pub const ROWS: usize = 8;
    pub fn backdrop(row: usize) -> i16 {
        13 + 6 * row as i16
    }
    pub fn name(row: usize) -> i16 {
        14 + 6 * row as i16
    }
    pub fn bar(row: usize, j: usize) -> i16 {
        15 + 6 * row as i16 + j as i16
    }
    pub fn total(row: usize) -> i16 {
        18 + 6 * row as i16
    }
}

/// 0x512ae0: the gluAll.tbl strings of the three column headings of each tab.
const HEADINGS: [[usize; 3]; 4] = [[4, 5, 6], [7, 8, 9], [10, 11, 12], [13, 14, 15]];

/// 0x4ff90c: the milliseconds a frame takes at each game speed (0x6d0f6a).
const FRAME_MS: [u32; 7] = [167, 111, 83, 67, 56, 48, 42];

/// A player of the table 0x59b3d8 (0x48 bytes), made by 0x4b2df0.
#[derive(Clone, Default)]
struct Entry {
    /// +0: the name (0x19 bytes).
    name: Vec<u8>,
    /// +0x19: the force / team name (0x1e bytes).
    aux: Vec<u8>,
    /// +0x38: the player whose scores the row shows (the slot).
    pid: usize,
    /// +0x3c: the slot's storm id (+4 of the slot).
    storm: u32,
    /// +0x40: the slot's owner type.
    ty: u32,
    /// +0x44: the race.
    race: u8,
}

/// A score row (0x25c bytes of 0x59a0f0): the values of the tab, the shown (counting up)
/// values and their scale.
#[derive(Clone, Copy, Default)]
struct Row {
    /// +0x14.
    visible: bool,
    /// +0x18: the player's index in the table.
    player: usize,
    /// +0x19c: the three values; +0x1a8 shown; +0x1b4 target (value * scale); +0x1c0 scale.
    raw: [i32; 3],
    cur: [i32; 3],
    target: [i32; 3],
    mult: [i32; 3],
    /// +0x24c, +0x250, +0x254, +0x258: the same for the total.
    total_raw: i32,
    total_cur: i32,
    total_target: i32,
    total_mult: i32,
}

/// The score screen's globals.
#[derive(Default)]
pub(super) struct Score {
    /// 0x59b624: the tab shown.
    tab: usize,
    /// 0x59b3d8.
    table: [Entry; 8],
    /// 0x59a0e0: the players in the table.
    count: usize,
    /// 0x59b618.
    built: bool,
    /// 0x59a0f0.
    rows: [Row; 8],
    /// 0x59a0e4: the largest of each value, which the bars are scaled to.
    max: [i32; 3],
    /// 0x59b730: the first 12 pixels of tminimap.pcx, the players' colors.
    colors: [u8; 12],
    /// 0x59b73c: each storm player's result (0x4c4220); 0x59b3d0 the local one.
    outcomes: [u32; 8],
    local: u32,
    /// 0x59b72c: iScore.grp's frames.
    gfx: Option<Rc<Vec<Icon>>>,
    /// 0x59b628: the image directory.
    dir: String,
    /// The 20 ms timer 0x22 (0x4b3670): the ms to its next run.
    timer: Option<u32>,
}

/// The step of a count-up (0x4b3670): an eighth of what is left, at least 4, not past the
/// target. False when it is already there.
fn count_up(cur: &mut i32, target: i32) -> bool {
    let d = target.wrapping_sub(*cur);
    if d == 0 {
        return true;
    }
    let step = ((d + ((d >> 31) & 7)) >> 3).max(4);
    *cur = cur.wrapping_add(step).min(target);
    false
}

/// 0x4b2bd0: the scale of a value: 256 / value is 0 or 1 -> 1, up to 8 -> 8, more -> 16.
fn scale(v: i32) -> i32 {
    match if v == 0 { 0 } else { 0x100 / v } {
        0 | 1 => 1,
        2..=8 => 8,
        _ => 0x10,
    }
}

impl Glue {
    /// gluAll.tbl string `n` (1-based, 0x4ddd30).
    fn sglu(&self, n: usize) -> String {
        self.glu_all.get(n.wrapping_sub(1)).cloned().unwrap_or_default()
    }

    /// 0x4b4af0 up to the dialog: the image directory, iScore.grp (0x59b72c), the player
    /// colors of tminimap.pcx (0x59b730; `Ordinal_323` reads 12 pixels), the sound
    /// 0x4bca80 preloads (0xa8: `Audio` loads on demand), and the table of players.
    /// `i` is the screen's index (race * 2 + victory).
    pub(super) fn score_begin(&mut self, files: &Storage, i: u32, outcomes: [u32; 8], local: u32) {
        let race = ['Z', 'T', 'P'][(i as usize / 2).min(2)];
        let dir = format!("glue/score{race}{}/", if i % 2 == 1 { 'v' } else { 'd' });
        let read = |name: &str| {
            let path = format!("{dir}{name}");
            files.read(&path).unwrap_or_else(|_| crate::bw::fatal_error(&path))
        };
        let gfx = Grp::parse_raw(read("iScore.grp"));
        let gfx: Vec<Icon> = (0..gfx.frame_count())
            .filter_map(|k| {
                let f = *gfx.frame(k)?;
                let (px, w, h) = gfx.pixels(k)?;
                Some(Icon { x: f.x, y: f.y, w, h, px })
            })
            .collect();
        let mut colors = [0u8; 12];
        let (_, _, px) = crate::view::pcx(&read("tminimap.pcx"));
        for (c, p) in colors.iter_mut().zip(px) {
            *c = p;
        }
        self.score = Score { dir, gfx: Some(Rc::new(gfx)), colors, outcomes, local, ..Default::default() };
        todo_port!("0x4bca80", "preload sound 0xa8 into the sound cache (Audio loads on demand)");
        self.score_players();
    }

    /// 0x4b2df0 (natively at the game's start, 0x4ef100): the table of players from the slot
    /// table 0x57eee0: the computers and humans of the game (and with 0x57f0b4 all slots
    /// of those types) with their names, races and the player whose scores they show.
    fn score_players(&mut self) {
        let mp = self.setup.multiplayer;
        let t = self.setup.template;
        let mut table: [Entry; 8] = Default::default();
        let mut count = 0;
        for (n, e) in table.iter_mut().enumerate() {
            let s = &self.setup.slots[n];
            let ty = s[slot::TYPE];
            let name = |s: &[u8]| s.iter().copied().take_while(|&c| c != 0).take(0x18).collect::<Vec<u8>>();
            let mut shown = name(&s[slot::NAME..slot::NAME + 0x19]);
            match ty {
                1 | 0xb => {
                    // A computer without a name is not listed (offline); with 0x57f0b4 it is
                    // named by network.tbl string 0xb.
                    if mp {
                        shown = name(crate::setup::tbl_str(&self.network_raw, 10));
                    } else if s[slot::NAME] == 0 {
                        continue;
                    }
                }
                2 | 0xa => {}
                _ => {
                    e.ty = ty as u32;
                    e.storm = u32::from_le_bytes(s[slot::STORM..slot::STORM + 4].try_into().unwrap());
                    continue;
                }
            }
            e.name = shown;
            e.race = s[slot::RACE];
            if !mp || t[0x10] == 0 {
                e.pid = n;
            } else {
                let force = s[slot::FORCE] as usize;
                e.aux = crate::setup::tbl_str(&self.network_raw, force + 0x11).iter().copied().take(0x1e).collect();
                // 0x57f1cb[force]: the slot 0x4a9d80 placed last in the force.
                todo_port!("0x4a9d80", "0x57f1cb: the player each force shows (team games)");
                e.pid = n;
            }
            // No victory conditions, starting units or 0x596877: the force's own name.
            let force = s[slot::FORCE] as usize;
            if t[8] == 0 && t[0xc] == 0 && t[0x12] == 0 && (1..5).contains(&force) {
                e.aux = self.setup.force_names[force - 1].iter().copied().take_while(|&c| c != 0).collect();
            }
            count += 1;
            e.ty = ty as u32;
            e.storm = u32::from_le_bytes(s[slot::STORM..slot::STORM + 4].try_into().unwrap());
        }
        self.score.table = table;
        self.score.count = count;
        self.score.built = true;
    }

    /// 0x4b3140: the sort key of row `row` (at position `i`): lower rows by position, higher
    /// totals first, the winners above and the defeated below.
    fn score_key(&self, row: &Row, i: usize) -> i32 {
        if !row.visible {
            return -0x7fff_ffff;
        }
        let e = &self.score.table[row.player];
        let mut k = (row.total_raw * 8 + 8) - i as i32;
        let by_storm = |v: u32| self.score.outcomes.get(e.storm as usize).copied() == Some(v) || (self.setup.local_storm_id == e.storm && self.score.local == v);
        if e.ty != 1 && e.ty != 0xb && by_storm(1) {
            k += 0x3fff_ffff;
        }
        // 0x4b2d50.
        if e.ty != 1 && e.ty != 0xb && by_storm(2) {
            k -= 0x3fff_ffff;
        }
        k
    }

    /// The values of tab `tab` for player `p` (0x4b2b70 overall, 0x4b2b30 units, 0x4b2af0
    /// structures, 0x4b2a90 resources): the three columns and the total.
    fn score_values(&self, tab: usize, p: usize) -> ([i32; 3], i32) {
        let Some(g) = self.game_end.as_ref() else { return ([0; 3], 0) };
        let g = &g.players;
        match tab {
            0 => {
                let v = [g.kill_score[p] + g.unit_score[p], g.razing_score[p] + g.building_score[p], g.gas_gathered[p] + g.minerals_gathered[p]];
                (v, v[0] + v[1] + v[2])
            }
            1 => ([g.units_produced[p], g.men_killed[p], g.men_lost[p]], g.kill_score[p] + g.unit_score[p]),
            2 => ([g.buildings_constructed[p], g.buildings_razed[p], g.buildings_lost[p]], g.razing_score[p] + g.building_score[p]),
            _ => {
                let gathered = g.minerals_gathered[p] + g.gas_gathered[p];
                // The third column (spent) is not below 0.
                ([g.gas_gathered[p], g.minerals_gathered[p], (gathered - g.gas[p] - g.minerals[p]).max(0)], gathered)
            }
        }
    }

    /// 0x4b43f0: the rows for the tab shown: cleared, the sounds stopped and the timer
    /// 0x22 killed, the column headings, the values of the visible players (0x4b31c0 with the
    /// tab's function), sorted (0x4b34d0), bound to the controls (0x4b3bc0), the count-up timer
    /// (every 20 ms: 0x4b3670) and its sound 0xa8.
    fn score_refresh(&mut self) {
        let tab = self.score.tab;
        self.score.max = [0; 3];
        self.score.rows = Default::default();
        self.cmds.push(AudioCmd::StopSounds);
        self.score.timer = None;
        for j in 0..3 {
            let text = self.sglu(HEADINGS[tab][j]);
            if let Some(c) = self.dlg.as_mut().and_then(|(d, _)| d.get_mut(10 + j as i16)) {
                c.text = text.into_bytes();
            }
        }
        // 0x4b31c0.
        for i in 0..8 {
            let e = &self.score.table[i];
            if !matches!(e.ty, 1 | 2 | 10 | 11) {
                continue;
            }
            let (raw, total) = self.score_values(tab, e.pid);
            let r = &mut self.score.rows[i];
            r.visible = true;
            r.player = i;
            r.raw = raw;
            r.total_raw = total;
            // 0x4b2bd0.
            for j in 0..3 {
                r.mult[j] = scale(raw[j]);
                r.target[j] = raw[j].wrapping_mul(r.mult[j]);
            }
            r.total_mult = scale(total);
            r.total_target = total.wrapping_mul(r.total_mult);
        }
        // 0x4b34d0: selection sort by the key, then the largest of each value.
        for i in 0..8 {
            let mut best = i;
            let mut best_key = self.score_key(&self.score.rows[i], i);
            for j in i + 1..8 {
                let k = self.score_key(&self.score.rows[j], j);
                if k > best_key {
                    best_key = k;
                    best = j;
                }
            }
            if best > i {
                self.score.rows.swap(i, best);
            }
            let r = self.score.rows[i];
            if r.visible {
                for j in 0..3 {
                    self.score.max[j] = self.score.max[j].max(r.raw[j]);
                }
            }
        }
        self.score_bind();
        self.score.timer = Some(20);
        if self.score.max.iter().sum::<i32>() > 0 {
            self.cmds.push(AudioCmd::Sound(0xa8));
        }
    }

    /// 0x4b3bc0: each player's controls take the row (draws 0x4b38a0 / 0x4b3a40 / 0x4b3b80)
    /// and show; the rows of the other players hide. The local player's name is lit.
    fn score_bind(&mut self) {
        let Some(gfx) = self.score.gfx.clone() else { return };
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        let sc = &self.score;
        for row in 0..ids::ROWS {
            let mut all = vec![ids::backdrop(row), ids::name(row), ids::total(row)];
            all.extend((0..3).map(|j| ids::bar(row, j)));
            if row >= sc.count {
                for id in all {
                    if let Some(c) = dlg.get_mut(id) {
                        c.flags &= !flag::VISIBLE;
                    }
                }
                continue;
            }
            let r = sc.rows[row];
            let e = &sc.table[r.player];
            for id in &all {
                if let Some(c) = dlg.get_mut(*id) {
                    c.flags |= flag::VISIBLE;
                }
            }
            // The player's color slot (0x57f21c; 0x58f442 overrides are the advisor's, unset
            // here) and its pixel of tminimap.pcx.
            let slot_color = match self.game_end.as_ref() {
                Some(g) if e.pid < 8 => g.colors[e.pid],
                _ => e.pid as u8,
            };
            let mut aux: Vec<u8> = e.aux.iter().copied().filter(|&c| c >= 0x20).take(0x1e).collect();
            aux.truncate(0x1e);
            if let Some(c) = dlg.get_mut(ids::name(row)) {
                c.update = Update::Score;
                c.text = e.name.clone();
                c.score = Some(ScoreCtl::Name { gfx: gfx.clone(), race: e.race, color: sc.colors.get(slot_color as usize).copied().unwrap_or(0), aux });
                if r.player == self.setup.local_slot[0] as usize {
                    c.flags |= 0x4000_0000;
                } else {
                    c.flags &= !0x4000_0000;
                }
            }
            for j in 0..3 {
                if let Some(c) = dlg.get_mut(ids::bar(row, j)) {
                    c.update = Update::Score;
                    c.text.clear();
                    c.score = Some(ScoreCtl::Bar { gfx: gfx.clone(), value: 0, max: sc.max[j], color: slot_color });
                }
            }
            if let Some(c) = dlg.get_mut(ids::total(row)) {
                c.update = Update::Score;
                c.text.clear();
                c.score = Some(ScoreCtl::Box);
            }
        }
    }

    /// 0x4b3670 (the timer, every 20 ms): each player's total and values move toward their
    /// targets (an eighth of the distance, at least 4) and show divided by their scale; when
    /// all are there the timer and the sounds stop and 0xa9 plays.
    fn score_step(&mut self) {
        let mut done = true;
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        for row in 0..self.score.count {
            let r = &mut self.score.rows[row];
            done &= count_up(&mut r.total_cur, r.total_target);
            let text = (r.total_cur.checked_div(r.total_mult).unwrap_or(0) as u32).to_string();
            if let Some(c) = dlg.get_mut(ids::total(row)) {
                c.text = text.into_bytes();
            }
            for j in 0..3 {
                done &= count_up(&mut r.cur[j], r.target[j]);
                let v = r.cur[j].checked_div(r.mult[j]).unwrap_or(0);
                if let Some(c) = dlg.get_mut(ids::bar(row, j)) {
                    c.text = (v as u32).to_string().into_bytes();
                    if let Some(ScoreCtl::Bar { value, .. }) = c.score.as_mut() {
                        *value = v;
                    }
                }
            }
        }
        if done {
            self.score.timer = None;
            self.cmds.push(AudioCmd::StopSounds);
            self.cmds.push(AudioCmd::Sound(0xa9));
        }
    }

    /// 0x4b30a0 (event 1, after the Sleep(1000)): the type-1 control's picture (pMain.pcx) and
    /// the type-9 control's data (iScore.grp, 0x59b72c) are freed, 0x6d5a60 = 0.
    pub(super) fn score_end(&mut self) {
        self.g6d5a60 = 0;
        self.score.gfx = None;
        self.score.timer = None;
    }

    /// The timer 0x22 of the open score screen for `ms` elapsed.
    pub(super) fn score_tick(&mut self, ms: u32) {
        if !matches!(self.dlg, Some((_, Open::Score(_)))) {
            return;
        }
        let Some(mut left) = self.score.timer else { return };
        let mut ms = ms;
        while ms >= left {
            ms -= left;
            left = 20;
            self.score_step();
            if self.score.timer.is_none() {
                return;
            }
        }
        self.score.timer = Some(left - ms);
    }

    /// 0x4b4520 (a tab was chosen: pressed, its hotkey, or the focus came to it): the tab
    /// shown is the control's number; every tab is unchecked and the chosen one lit (bit 31)
    /// and, when another was lit, the rows are made again (0x4b43f0).
    fn score_tab(&mut self, chosen: i16) {
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        let mut prev = None;
        for id in ids::TAB_FIRST..=ids::TAB_LAST {
            let Some(c) = dlg.get_mut(id) else { continue };
            if c.checked {
                prev = Some(id);
            }
            c.flags = c.flags & 0x5fff_ffff | 0x2000_0000;
            c.checked = false;
        }
        if let Some(c) = dlg.get_mut(chosen) {
            c.flags |= 0x8000_0000;
            c.checked = true;
        }
        self.score.tab = (chosen - ids::TAB_FIRST).clamp(0, 3) as usize;
        if prev != Some(chosen) {
            self.score_refresh();
        }
    }

    /// The controls' event 10 (0x4b47c0 tabs: untab.pcx, 0x4b3fd0 backdrops: pInset.pcx,
    /// 0x4b4160 total boxes: scorebox.pcx) and 0x4b4960's event 0: Save Replay is disabled
    /// for the mission-less modes (0x6d5a60, a replay 0x6d5a68, a loose map 0x57f244, spawn).
    pub(super) fn score_controls(&mut self, files: &Storage, dlg: &mut Dialog) {
        let dir = self.score.dir.clone();
        let image = |name: &str| {
            let path = format!("{dir}{name}");
            let Ok(d) = files.read(&path) else { crate::bw::fatal_error(&path) };
            let (w, h, px) = crate::view::pcx(&d);
            Rc::new(Image { w: w as i32, h: h as i32, px })
        };
        let (untab, inset, boxed) = (image("untab.pcx"), image("pInset.pcx"), image("scorebox.pcx"));
        for id in ids::TAB_FIRST..=ids::TAB_LAST {
            if let Some(c) = dlg.get_mut(id) {
                c.image = Some(untab.clone());
                c.update = Update::Score;
                c.score = Some(ScoreCtl::Tab);
                c.flags |= 0x20;
            }
        }
        for row in 0..ids::ROWS {
            if let Some(c) = dlg.get_mut(ids::backdrop(row)) {
                c.image = Some(inset.clone());
                c.flags |= 0x20;
            }
            if let Some(c) = dlg.get_mut(ids::total(row)) {
                c.image = Some(boxed.clone());
                c.update = Update::Score;
                c.score = Some(ScoreCtl::Box);
                c.flags |= 0x20;
            }
        }
        if self.g6d5a60 != 0 || self.setup.map_dir_flag != 0 || self.spawn {
            todo_port!("0x4b4960", "0x6d5a68 (a replay game) also disables Save Replay");
            if let Some(c) = dlg.get_mut(ids::SAVE_REPLAY) {
                c.flags |= flag::DISABLED;
            }
        }
    }

    /// The dialog's event 0 (0x4b4a30 -> 0x4b4600, then 0x4de150 gives the focus to the
    /// first tab, which chooses it).
    pub(super) fn score_init(&mut self, files: &Storage) {
        self.score_event0(files);
        self.show_palette();
        self.score_tab(ids::TAB_FIRST);
    }

    /// 0x4b4600: the title by the local result (Defeat / Victory / Draw / Disconnect, or
    /// "Replay" with 0x6d5a60; a result without a string leaves the old title), the "Total
    /// Score" label lit, the screen's picture pMain.pcx and the elapsed time (0x4b42d0).
    fn score_event0(&mut self, files: &Storage) {
        let n = if self.g6d5a60 != 0 {
            0xb2
        } else {
            match self.score.local {
                0 | 3 => 0x13,
                1 => 0x12,
                2 => 0x11,
                4 => 0x14,
                _ => 0,
            }
        };
        let title = if n != 0 { self.sglu(n) } else { String::new() };
        let secs = self.score_seconds();
        let time = if secs / 3600 == 0 {
            format!("{:02}:{:02}", secs / 60 % 60, secs % 60)
        } else {
            format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
        };
        let time = self.sglu(0x10).replace("%s", &time);
        let path = format!("{}pMain.pcx", self.score.dir);
        let Ok(d) = files.read(&path) else { crate::bw::fatal_error(&path) };
        let (w, h, px) = crate::view::pcx(&d);
        let Some((dlg, _)) = self.dlg.as_mut() else { return };
        if n != 0 {
            if let Some(c) = dlg.get_mut(ids::TITLE) {
                c.text = title.into_bytes();
                c.flags |= 0x4000_0000;
            }
        }
        if let Some(c) = dlg.get_mut(ids::TOTAL_LABEL) {
            c.flags |= 0x4000_0000;
        }
        if let Some(c) = dlg.get_mut(ids::BACKGROUND) {
            c.image = Some(Rc::new(Image { w: w as i32, h: h as i32, px }));
        }
        if let Some(c) = dlg.get_mut(ids::TIME) {
            c.text = time.into_bytes();
        }
    }

    /// 0x4b42d0: the seconds of the game: 0x4a2ca0, or with 0x6d5a60 the frames
    /// (0x57f23c) at the game speed 0x6d0f6a.
    fn score_seconds(&self) -> u32 {
        let by_frames = |frames: u32| (FRAME_MS[self.create.speed as usize % FRAME_MS.len()] as u64 * frames as u64 / 1000) as u32;
        match self.game_end.as_ref() {
            // The wall-clock timer 0x59cc7c is not run by the game here: the game's frames.
            Some(g) => by_frames(g.frames),
            None if self.g6d5a60 != 0 => 0,
            None => self.setup.elapsed_seconds(self.clock_ms),
        }
    }

    /// The score dialog's event proc for an activated control (0x4b4a30 code 2, or the
    /// control's own proc): a tab chooses itself (0x4b47c0 -> 0x4b4520); OK stops the sounds
    /// (0x4bbf50) and ends the dialog (0x4dd040); Save Replay (0x4b4960) saves the replay.
    pub(super) fn score_activate(&mut self, id: i16) {
        match id {
            ids::TAB_FIRST..=ids::TAB_LAST => self.score_tab(id),
            ids::SAVE_REPLAY => {
                todo_port!("0x4dffc0", "save the replay (then the button reads gluAll 0xb1 and is disabled)");
            }
            _ => {
                self.cmds.push(AudioCmd::StopSounds);
                self.end_dialog(id as i32);
            }
        }
    }

    /// A finished game of two players for `SCBW_SCORE_TEST` (the score screen shown headless):
    /// the human (slot 0, terran) won over a zerg computer.
    pub fn score_test_game(&mut self) {
        let mut g = crate::view::GameEnd {
            players: Default::default(),
            results: [0; 12],
            types: [0; 12],
            races: [0; 12],
            names: Default::default(),
            colors: [0, 1, 2, 3, 4, 5, 6, 7],
            local: 0,
            frames: 24 * 60 * 14 + 24 * 31,
        };
        let p = &mut g.players;
        p.units_produced[0] = 61;
        p.unit_score[0] = 5100;
        p.men_killed[0] = 77;
        p.kill_score[0] = 6400;
        p.men_lost[0] = 20;
        p.buildings_constructed[0] = 19;
        p.building_score[0] = 2300;
        p.buildings_razed[0] = 8;
        p.razing_score[0] = 1900;
        p.buildings_lost[0] = 2;
        p.minerals_gathered[0] = 12000;
        p.gas_gathered[0] = 4100;
        p.minerals[0] = 300;
        p.gas[0] = 50;
        p.units_produced[1] = 40;
        p.unit_score[1] = 2800;
        p.men_killed[1] = 20;
        p.kill_score[1] = 1500;
        p.men_lost[1] = 77;
        p.buildings_constructed[1] = 12;
        p.building_score[1] = 1400;
        p.buildings_razed[1] = 2;
        p.razing_score[1] = 300;
        p.buildings_lost[1] = 8;
        p.minerals_gathered[1] = 7000;
        p.gas_gathered[1] = 1500;
        g.results[0] = 3;
        g.results[1] = 2;
        self.game_end = Some(g);
        self.state = 6;
        self.first = false;
        let put = |s: &mut crate::setup::Slot, ty: u8, race: u8, force: u8, storm: u32, name: &[u8]| {
            s[slot::TYPE] = ty;
            s[slot::RACE] = race;
            s[slot::FORCE] = force;
            s[slot::STORM..slot::STORM + 4].copy_from_slice(&storm.to_le_bytes());
            s[slot::NAME..slot::NAME + name.len()].copy_from_slice(name);
        };
        put(&mut self.setup.slots[0], 2, 1, 1, 0, b"Stubob");
        put(&mut self.setup.slots[1], 1, 0, 2, u32::MAX, b"Zerg");
        self.setup.slot_by_storm = [0, 8, 8, 8, 8, 8, 8, 8];
        self.setup.local_storm_id = 0;
        self.setup.local_slot = [0, 0];
    }
}
