//! Triggers (victory.cpp / trigger.cpp): per-player trigger lists (0x51a280), the
//! condition (0x515a98) and action (0x512800) tables, and the end-of-game resolution.
//!
//! A trigger is kept as its raw 0x960-byte record: 16 conditions of 0x14 bytes, 64
//! actions of 0x20 bytes, the execution flags at +0x940, the player flags at +0x944 and
//! the current action at +0x95f. The real list node adds 8 bytes of links in front.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

pub const TRIGGER_SIZE: usize = 0x960;
/// 0x5124d8: milliseconds per frame at each game speed.
pub const FRAME_MS: [u32; 7] = [0xa7, 0x6f, 0x53, 0x43, 0x38, 0x30, 0x2a];

/// 0x4deb90: milliseconds per frame for game speed `speed`; a replay's multiplier divides
/// it (a game's would multiply, never used here).
pub fn replay_frame_ms(speed: usize, mult: u32) -> u32 {
    (FRAME_MS[speed.min(6)] / mult.max(1)).max(1)
}

/// 0x4abaf0: the trigger file of the game type (template +8 and +4).
/// The capture the flag setup (0x4aba20, `State::ctf_setup`) runs before CTF.trg loads.
pub fn game_type_triggers(t: &[u8; 0x20]) -> Option<&'static str> {
    let v = u32::from_le_bytes([t[4], t[5], t[6], t[7]]);
    match t[8] {
        1 | 7 => Some("triggers\\Melee.trg"),
        5 => Some("triggers\\SuddenDeath.trg"),
        3 => match v {
            2500 => Some("triggers\\Greed2500.trg"),
            5000 => Some("triggers\\Greed5000.trg"),
            7500 => Some("triggers\\Greed7500.trg"),
            10000 => Some("triggers\\Greed10000.trg"),
            _ => None,
        },
        4 => Some("triggers\\CTF.trg"),
        6 => match v {
            15 => Some("triggers\\Slaughter15.trg"),
            30 => Some("triggers\\Slaughter30.trg"),
            45 => Some("triggers\\Slaughter45.trg"),
            60 => Some("triggers\\Slaughter60.trg"),
            _ => None,
        },
        _ => None,
    }
}
const FLAGS: usize = 0x940;
const CUR_ACTION: usize = 0x95f;

#[derive(Debug, Clone)]
pub struct Trigger(pub Box<[u8; TRIGGER_SIZE]>);

impl Trigger {
    pub fn from_bytes(b: &[u8]) -> Trigger {
        let mut t = Box::new([0u8; TRIGGER_SIZE]);
        t.copy_from_slice(&b[..TRIGGER_SIZE]);
        Trigger(t)
    }
    fn u32_at(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn flags(&self) -> u32 {
        self.u32_at(FLAGS)
    }
    fn set_flags(&mut self, f: u32) {
        self.0[FLAGS..FLAGS + 4].copy_from_slice(&f.to_le_bytes());
    }
    fn condition(&self, i: usize) -> Condition {
        Condition::parse(&self.0[i * 0x14..i * 0x14 + 0x14])
    }
    fn action(&self, i: usize) -> Action {
        let o = 0x140 + i * 0x20;
        Action::parse(&self.0[o..o + 0x20])
    }
    fn action_flags_mut(&mut self, i: usize) -> &mut u8 {
        &mut self.0[0x140 + i * 0x20 + 0x1c]
    }
}

/// A condition (0x14 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Condition {
    pub location: u32,
    pub group: u32,
    pub amount: u32,
    pub unit_type: u16,
    pub comparison: u8,
    pub id: u8,
    /// Resource type, score type or switch.
    pub kind: u8,
    pub flags: u8,
}

impl Condition {
    fn parse(b: &[u8]) -> Condition {
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Condition {
            location: u32_at(0),
            group: u32_at(4),
            amount: u32_at(8),
            unit_type: u16::from_le_bytes([b[0xc], b[0xd]]),
            comparison: b[0xe],
            id: b[0xf],
            kind: b[0x10],
            flags: b[0x11],
        }
    }
}

/// An action (0x20 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub location: u32,
    pub string: u32,
    pub wav: u32,
    pub time: u32,
    pub player: u32,
    pub number: u32,
    pub unit_type: u16,
    pub id: u8,
    pub modifier: u8,
    pub flags: u8,
}

impl Action {
    fn parse(b: &[u8]) -> Action {
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        Action {
            location: u32_at(0),
            string: u32_at(4),
            wav: u32_at(8),
            time: u32_at(0xc),
            player: u32_at(0x10),
            number: u32_at(0x14),
            unit_type: u16::from_le_bytes([b[0x18], b[0x19]]),
            id: b[0x1a],
            modifier: b[0x1b],
            flags: b[0x1c],
        }
    }
}

/// A location (MRGN, 0x14 bytes at 0x58dc60).
#[derive(Clone, Copy, Debug, Default)]
pub struct Location {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
    pub string: u16,
    pub flags: u16,
}

/// Unit counts the conditions read (0x45f7a0 copies the live ones; 0x4609d0 counts a
/// location's units instead).
#[derive(Debug, Clone)]
pub struct CountSnapshot {
    /// 0x68a708 men, 0x68a738 men completed, 0x68a858 buildings, 0x68a888 buildings
    /// completed, 0x68a9a8 factories, 0x68a9d8 factories completed.
    pub men: [i32; 12],
    pub men_completed: [i32; 12],
    pub buildings: [i32; 12],
    pub buildings_completed: [i32; 12],
    pub factories: [i32; 12],
    pub factories_completed: [i32; 12],
    /// 0x685188 completed, 0x687c48 all: [unit type][player].
    pub completed: Vec<[i32; 12]>,
    pub all: Vec<[i32; 12]>,
}

impl Default for CountSnapshot {
    fn default() -> CountSnapshot {
        CountSnapshot {
            men: [0; 12],
            men_completed: [0; 12],
            buildings: [0; 12],
            buildings_completed: [0; 12],
            factories: [0; 12],
            factories_completed: [0; 12],
            completed: vec![[0; 12]; 228],
            all: vec![[0; 12]; 228],
        }
    }
}

#[derive(Debug)]
pub struct Triggers {
    /// 0x51a280: each player's triggers.
    pub lists: [Vec<Trigger>; 8],
    /// 0x6509b0: the player whose triggers run; 0x6509ac: the running trigger.
    pub player: u32,
    pub current: usize,
    /// 0x6509a0: frames until the next trigger pass (every 30 frames, or the next frame
    /// after a wait ends).
    pub check_timer: u16,
    /// 0x6509b8 / 0x650980: a player's triggers wait (ms left; -1 = until a sound ends).
    pub waiting: [u8; 8],
    pub wait_ms: [u32; 8],
    /// 0x650974: result of this pass per player (2 defeat, 3 victory, 4 ignored, 5 draw).
    pub result: [u8; 8],
    /// 0x6509a4: players in the game when it started (0x489790).
    pub in_game: [u8; 8],
    /// 0x650970: frames until the victory / defeat screen.
    pub end_screen_timer: u16,
    /// 0x685178: the victory / defeat dialog is open.
    pub end_open: bool,
    /// 0x685174: the Battle.net result was reported.
    pub bnet_reported: bool,
    /// 0x68517c: an end dialog button was used; the dialog does not open again.
    pub end_done: bool,
    /// The front end opens the victory / defeat dialog (0x4615b0 / 0x461640): the local
    /// player's result 2 (defeat), 3 (victory) or 5 (draw), 0 for none.
    pub end_request: u8,
    /// 0x6509b4: the game was paused by a trigger.
    pub trigger_paused: bool,
    /// 0x6509c4: the game is paused.
    pub paused: bool,
    /// 0x6509c0: frames to the next game second; 0x65097c: frames to the next
    /// leaderboard refresh.
    pub second_timer: u16,
    pub leaderboard_timer: u32,
    /// 0x58d6f8 elapsed game seconds, 0x58d6f4 countdown timer, 0x58f04c countdown paused.
    pub elapsed: u32,
    pub countdown: u32,
    pub countdown_paused: bool,
    /// 0x58dc40: switches.
    pub switches: [u32; 8],
    /// 0x58d6c4: mission objective string per player.
    pub objectives: [u32; 8],
    /// 0x58d708: computer players in leaderboards; 0x58d70c..0x58d714: leaderboard
    /// action, location, type, goal and string.
    pub leaderboard_computers: u32,
    pub leaderboard: (u8, u8, u16, u32, u32),
    /// 0x68c308: the leaderboard's rows (player or u32::MAX, value), sorted by 0x4538b0.
    pub leaderboard_table: [(u32, u32); 8],
    /// 0x685180: which snapshot 0x685188.. holds (1 live counts, 2 a location's);
    /// 0x68ac48: that location.
    pub snapshot_kind: u8,
    pub snapshot_location: u8,
    pub snap: CountSnapshot,
    /// 0x58dc60: locations (255 in Brood War maps, 64 in original ones).
    pub locations: Vec<Location>,
    /// 0x596cd8: UPRP unit property slots.
    pub unit_properties: Vec<[u8; 0x14]>,
    /// 0x63ff44: set once 0x4a0ad0 created a unit.
    pub unit_created: bool,
}

impl Default for Triggers {
    fn default() -> Triggers {
        Triggers {
            lists: Default::default(),
            player: 0,
            current: 0,
            check_timer: 1,
            waiting: [0; 8],
            wait_ms: [0; 8],
            result: [0; 8],
            in_game: [0; 8],
            end_screen_timer: 0,
            end_open: false,
            end_done: false,
            bnet_reported: false,
            end_request: 0,
            trigger_paused: false,
            paused: false,
            second_timer: 0xf,
            leaderboard_timer: 0xf,
            elapsed: 0,
            countdown: 0,
            countdown_paused: false,
            switches: [0; 8],
            objectives: [0; 8],
            leaderboard_computers: 1,
            leaderboard: (0, 0, 0, 0, 0),
            leaderboard_table: [(u32::MAX, u32::MAX); 8],
            snapshot_kind: 0,
            snapshot_location: 0,
            snap: CountSnapshot::default(),
            locations: vec![Location::default(); 255],
            unit_properties: vec![[0; 0x14]; 64],
            unit_created: false,
        }
    }
}

/// 0x42c720-style comparison: 0 at least, 1 at most, 10 exactly.
fn compare(value: u32, comparison: u8, amount: u32) -> bool {
    match comparison {
        0 => value >= amount,
        1 => value <= amount,
        10 => value == amount,
        _ => false,
    }
}

/// Which per-player counter a group sum uses.
#[derive(Clone, Copy)]
pub(super) enum Count {
    /// 0x4606b0: units of a type (true: completed only).
    Units(u16, bool),
    /// 0x4605c0: resources (0 ore, 1 gas, 2 both).
    Resources(u8),
    /// 0x4604a0: kills of a unit type.
    Kills(u16),
    /// 0x460380: deaths of a unit type.
    Deaths(u16),
    /// 0x45ffe0: score of a type.
    Score(u8),
}

impl State {
    /// 0x4aba20: capture the flag setup: a flag beacon for every human and computer slot
    /// (slots 7..0), or in a team game for each force's leader (forces teams..1, or 2 for
    /// Top vs Bottom).
    pub fn ctf_setup(&mut self, d: &Data) {
        if !self.multiplayer || self.template[0x10] == 0 {
            for p in (0..8u8).rev() {
                if matches!(self.player_types[p as usize], 1 | 2) {
                    self.ctf_flag(d, p);
                }
            }
            return;
        }
        let n = if self.template[0] == 0x0f { 2 } else { self.template[0x10] };
        for force in (1..=n).rev() {
            if self.force_is_playing(force) {
                self.ctf_flag(d, self.team_leaders[force as usize]);
            }
        }
    }

    /// 0x4ab980 (edi = `p`): the flag beacon of `p`'s race (unit 0xc5 + race) placed near its
    /// start location by the first of its units that is a building-type flag 8 unit.
    fn ctf_flag(&mut self, d: &Data, p: u8) {
        let ty = 0xc5u16.wrapping_add(self.player_races[p as usize] as u16);
        let mut b = self.player_units[p as usize];
        while b != NIL && d.dat.units.flags[self.units[b as usize].unit_type as usize] & 8 == 0 {
            b = self.units[b as usize].player_next;
        }
        if b == NIL {
            return;
        }
        let radius = if ty == 0x7c { 0x28 } else { 0x40 };
        let Some((x, y)) = self.ai_find_build_spot(d, b, ty, self.start_locations[p as usize], radius) else { return };
        if let Some(u) = self.create_unit(d, ty, x, y, p) {
            self.finish_unit_pre(d, u);
            self.finish_unit_post(d, u);
        }
    }

    /// 0x484ff0: the next player after `*it` whose triggers can run (`*it` starts at -1);
    /// 8 when there are no more.
    pub fn next_trigger_player(&self, it: &mut u32) -> u32 {
        let ok = |p: u32| match self.player_types[p as usize] {
            3 | 4 | 7 => true,
            1 | 2 => !matches!(self.player_left[p as usize], 1 | 2),
            _ => false,
        };
        // Team games (0x57f0b4 and template +0x10): the team leaders 0x57f1cc[0..4].
        if self.multiplayer && self.template[0x10] != 0 {
            loop {
                *it = it.wrapping_add(1);
                if *it >= 4 {
                    return 8;
                }
                let p = self.team_leaders[*it as usize + 1] as u32;
                if p < 8 && ok(p) {
                    return p;
                }
            }
        }
        loop {
            *it = it.wrapping_add(1);
            let p = *it;
            if p >= 8 {
                return 8;
            }
            if ok(p) {
                return p;
            }
        }
    }

    /// 0x489790: remember who is in the game.
    pub fn init_trigger_players(&mut self) {
        self.trig.in_game = [0; 8];
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            self.trig.in_game[p as usize] = 1;
        }
    }

    /// 0x4894c0: give a trigger record to every player it is for (+0x944: players 0..11,
    /// 17 all players, 18..21 forces 1..4).
    pub fn add_trigger(&mut self, rec: &[u8]) {
        let t = Trigger::from_bytes(rec);
        for i in 0..27usize {
            if t.0[0x944 + i] == 0 {
                continue;
            }
            let mut it = u32::MAX;
            loop {
                let p = self.next_trigger_player(&mut it);
                if p >= 8 {
                    break;
                }
                let take = match i {
                    17 => true,
                    18..=21 => self.player_force[p as usize] as usize == i - 17,
                    0..=12 => p as usize == i,
                    _ => false,
                };
                if take {
                    self.trig.lists[p as usize].push(t.clone());
                }
            }
        }
    }

    /// 0x4cbfa0 (TRIG) / 0x4ab620 (a .trg file): every 0x960-byte record.
    pub fn load_triggers(&mut self, data: &[u8]) -> bool {
        if data.len() % TRIGGER_SIZE != 0 {
            return false;
        }
        for rec in data.chunks_exact(TRIGGER_SIZE) {
            self.add_trigger(rec);
        }
        true
    }

    /// 0x4cb2f0 / 0x4cb2a0: MRGN, 64 (0x500 bytes) or 255 (0x13ec) locations.
    pub fn load_locations(&mut self, data: &[u8]) -> bool {
        if data.len() != 0x500 && data.len() != 0x13ec {
            return false;
        }
        for (i, r) in data.chunks_exact(0x14).enumerate() {
            let u32_at = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
            self.trig.locations[i] = Location {
                left: u32_at(0),
                top: u32_at(4),
                right: u32_at(8),
                bottom: u32_at(0xc),
                string: u16::from_le_bytes([r[0x10], r[0x11]]),
                flags: u16::from_le_bytes([r[0x12], r[0x13]]),
            };
        }
        true
    }

    /// 0x48a410: counts the frames to the victory / defeat dialog (not in a replay); when it
    /// runs out it restarts at 0x2d and the local player's result opens the dialog:
    /// 0x4615b0 for defeat, 0x461640 for victory and draw (the same but for the dialog).
    fn end_screen_countdown(&mut self) {
        if self.replay {
            return;
        }
        let t = self.trig.end_screen_timer;
        self.trig.end_screen_timer = t.wrapping_sub(1);
        if t != 0 {
            return;
        }
        self.trig.end_screen_timer = 0x2d;
        match self.player_left[self.local_player as usize] {
            2 => self.defeat_screen(),
            r @ (3 | 5) => self.victory_screen(r),
            _ => {}
        }
    }

    /// 0x461640: victory / draw (`r` 3 / 5) of the local player; same state part as 0x4615b0.
    /// Not yet open (0x685178) or used (0x68517c): a Battle.net game reports once (0x4c4790,
    /// 0x685174), 0x685178 = 1, the front end ends placement and targeting (0x48e310 /
    /// 0x48ca10), resets the handlers (0x484cc0), 0x6d051c = 0 and opens the dialog (0x4f57a0).
    fn victory_screen(&mut self, r: u8) {
        self.end_screen(r);
    }

    /// 0x4615b0: defeat of the local player (result 2); see `victory_screen`.
    fn defeat_screen(&mut self) {
        self.end_screen(2);
    }

    fn end_screen(&mut self, r: u8) {
        if self.trig.end_open || self.trig.end_done {
            return;
        }
        // 0x57f0b4 && 0x59688c == 'BNET' (0x424e4554) && !0x685174.
        if self.multiplayer && !self.trig.bnet_reported {
            self.trig.bnet_reported = true;
            todo_port!("0x4c4790", "Battle.net: report the game result once");
        }
        self.trig.end_open = true;
        // 0x48e310 / 0x48ca10 / 0x484cc0 / 0x6d051c = 0 / 0x4f57a0: the front end ends
        // placement and targeting and opens the dialog (rez\lmission.bin / wmission.bin).
        self.trig.end_request = r;
    }

    /// 0x48a460: the trigger pass after each game step; `ms` is the frame time of the
    /// game speed (0x5124d8).
    pub fn run_triggers(&mut self, d: &Data, ms: u32) {
        if self.trig.paused && !self.trig.trigger_paused {
            return;
        }
        self.end_screen_countdown();
        self.trigger_timers(d, ms);
        let t = self.trig.check_timer;
        self.trig.check_timer = t.wrapping_sub(1);
        if t != 0 {
            return;
        }
        self.trig.result = [0; 8];
        self.trig.check_timer = 0x1e;
        self.trig.snapshot_kind = 0;
        let mut any = false;
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            self.trig.player = p;
            if p >= 8 {
                break;
            }
            if !self.trig.lists[p as usize].is_empty() {
                self.run_trigger_list(d, p as usize);
                any = true;
            }
        }
        if any {
            self.resolve_game_end(d);
        }
    }

    /// 0x45e610: a transmission's timer ran out (single player): the local player's trigger
    /// wait ends and the next pass runs at once.
    /// 0x4894a0: the same when the center view scroll arrived.
    pub fn trigger_wait_end(&mut self) {
        let p = self.local_player as usize;
        self.trig.waiting[p] = 0;
        self.trig.wait_ms[p] = 0;
        self.trig.check_timer = 1;
    }

    /// 0x489c30: the countdown timer text: "h:mm:ss" from an hour on, else "m:ss" (minutes
    /// not padded) for `t` = 0x58d6f4 seconds.
    pub fn countdown_text(t: u32) -> String {
        if t / 0xe10 != 0 {
            format!("{}:{:02}:{:02}", t / 0xe10, t % 0xe10 / 0x3c, t % 0x3c)
        } else {
            format!("{}:{:02}", t / 0x3c, t % 0x3c)
        }
    }

    /// 0x489cc0: wait timers, the leaderboard refresh (0x4538b0), the game clock and the
    /// countdown timer.
    fn trigger_timers(&mut self, d: &Data, ms: u32) {
        let tr = &mut self.trig;
        for p in 0..8 {
            if tr.waiting[p] != 0 && tr.wait_ms[p] != u32::MAX {
                if ms < tr.wait_ms[p] {
                    tr.wait_ms[p] -= ms;
                } else {
                    tr.waiting[p] = 0;
                    tr.wait_ms[p] = 0;
                    tr.check_timer = 1;
                }
            }
        }
        if tr.paused {
            return;
        }
        let t = tr.leaderboard_timer;
        tr.leaderboard_timer = t.wrapping_sub(1);
        if t == 0 {
            tr.leaderboard_timer = 0xf;
            self.leaderboard_update(d);
        }
        let tr = &mut self.trig;
        let t = tr.second_timer;
        tr.second_timer = t.wrapping_sub(1);
        if t == 0 {
            tr.elapsed = tr.elapsed.wrapping_add(1);
            if tr.countdown != 0 && !tr.countdown_paused {
                tr.countdown -= 1;
                if tr.countdown == 0 {
                    tr.check_timer = 1;
                }
            }
            // 0x489c30 (`countdown_text`) / 0x4e5600: the timer text (UI).
            tr.second_timer = 0xf;
        }
    }

    /// 0x489450: run player `p`'s triggers.
    fn run_trigger_list(&mut self, d: &Data, p: usize) {
        let mut i = 0;
        while i < self.trig.lists[p].len() {
            if self.trig.lists[p][i].flags() & 8 == 0 {
                self.trig.current = i;
                if self.trig.lists[p][i].flags() & 1 == 0 {
                    if !self.trigger_conditions(d, p, i) {
                        i += 1;
                        continue;
                    }
                    self.trig.lists[p][i].0[CUR_ACTION] = 0;
                }
                self.trigger_actions(d, p, i);
            }
            i += 1;
        }
    }

    /// 0x489200: every enabled condition holds (up to the first empty one).
    fn trigger_conditions(&mut self, d: &Data, p: usize, i: usize) -> bool {
        for k in 0..16 {
            let c = self.trig.lists[p][i].condition(k);
            if c.flags & 2 != 0 {
                continue;
            }
            if c.id == 0 {
                return true;
            }
            if !self.condition(d, &c) {
                return false;
            }
        }
        true
    }

    /// 0x489130: run actions from the current one until one blocks or all are done.
    fn trigger_actions(&mut self, d: &Data, p: usize, i: usize) {
        let f = self.trig.lists[p][i].flags();
        self.trig.lists[p][i].set_flags(f | 1);
        if f & 2 != 0 {
            self.trig.result[self.trig.player as usize] = 4;
        }
        loop {
            let k = self.trig.lists[p][i].0[CUR_ACTION];
            if k > 0x3f {
                break;
            }
            let a = self.trig.lists[p][i].action(k as usize);
            if a.flags & 2 != 0 {
                self.trig.lists[p][i].0[CUR_ACTION] = k + 1;
                continue;
            }
            if a.id == 0 {
                self.trig.lists[p][i].0[CUR_ACTION] = 0x40;
                break;
            }
            if !self.action(d, p, i, k as usize, &a) {
                break;
            }
            // The action may have dropped the list (a player leaving).
            let Some(t) = self.trig.lists.get_mut(p).and_then(|l| l.get_mut(i)) else { return };
            t.0[CUR_ACTION] += 1;
        }
        let Some(t) = self.trig.lists.get_mut(p).and_then(|l| l.get_mut(i)) else { return };
        if t.0[CUR_ACTION] > 0x3f {
            if t.flags() & 0x20 != 0 {
                // 0x512818: action 6, UnpauseGame.
                self.action_unpause_game(p, i);
            }
            let t = &mut self.trig.lists[p][i];
            let f = t.flags();
            if f & 4 != 0 {
                t.0[CUR_ACTION] = 0;
                t.set_flags(f & 0xffff_ffae);
            } else {
                t.set_flags(f | 8);
            }
        }
    }

    /// 0x515a98: condition table.
    fn condition(&mut self, d: &Data, c: &Condition) -> bool {
        let cur = self.trig.player;
        match c.id {
            0 | 0x16 => true,
            0xd | 0x17 => false,
            // 0x42c720: countdown timer.
            1 => compare(self.trig.countdown, c.comparison, c.amount),
            // 0x42cc20: command.
            2 => {
                self.snapshot_counts();
                let v = self.group_count(c.group, Count::Units(c.unit_type, c.comparison != 1));
                compare(v, c.comparison, c.amount)
            }
            // 0x42cd70: bring.
            3 => {
                if c.location as u8 == 0 {
                    return false;
                }
                self.snapshot_location(d, c.location as u8 - 1);
                let v = self.group_count(c.group, Count::Units(c.unit_type, c.comparison != 1));
                compare(v, c.comparison, c.amount)
            }
            // 0x42cbe0: accumulate.
            4 => compare(self.group_count(c.group, Count::Resources(c.kind)), c.comparison, c.amount),
            // 0x42cba0: kill.
            5 => compare(self.group_count(c.group, Count::Kills(c.unit_type)), c.comparison, c.amount),
            // 0x42ca70: command the most.
            6 => {
                self.snapshot_counts();
                self.most(Count::Units(c.unit_type, true), cur)
            }
            // 0x42ccf0: command the most at.
            7 => {
                if c.location as u8 == 0 {
                    return false;
                }
                self.snapshot_location(d, c.location as u8 - 1);
                self.most(Count::Units(c.unit_type, true), cur)
            }
            // 0x42c990: most kills; 0x42c8b0: highest score; 0x42c7d0: most resources.
            8 => self.most(Count::Kills(c.unit_type), cur),
            9 => self.most(Count::Score(c.kind), cur),
            0xa => self.most(Count::Resources(c.kind), cur),
            // 0x42c610: switch (2 set, 3 cleared).
            0xb => {
                let bit = self.trig.switches[(c.kind >> 5) as usize] & (1 << (c.kind & 0x1f));
                match c.comparison {
                    2 => bit != 0,
                    3 => bit == 0,
                    _ => false,
                }
            }
            // 0x42c6e0: elapsed time.
            0xc => compare(self.trig.elapsed, c.comparison, c.amount),
            // 0x42cb20: opponents.
            0xe => compare(self.opponents(c.group) & 0xff, c.comparison, c.amount),
            // 0x42cb60: deaths.
            0xf => compare(self.group_count(c.group, Count::Deaths(c.unit_type)), c.comparison, c.amount),
            // 0x42ca00: command the least.
            0x10 => {
                self.snapshot_counts();
                self.least(Count::Units(c.unit_type, true), cur)
            }
            // 0x42cc70: command the least at.
            0x11 => {
                if c.location as u8 == 0 {
                    return false;
                }
                self.snapshot_location(d, c.location as u8 - 1);
                self.least(Count::Units(c.unit_type, true), cur)
            }
            // 0x42c920: least kills; 0x42c840: lowest score; 0x42c760: least resources.
            0x12 => self.least(Count::Kills(c.unit_type), cur),
            0x13 => self.least(Count::Score(c.kind), cur),
            0x14 => self.least(Count::Resources(c.kind), cur),
            // 0x42cae0: score.
            0x15 => compare(self.group_count(c.group, Count::Score(c.kind)), c.comparison, c.amount),
            _ => false,
        }
    }

    /// The current player has at least as much as every player (0x42ca70 pattern).
    fn most(&self, what: Count, cur: u32) -> bool {
        let (mut best, mut mine) = (0u32, 0u32);
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            let v = self.player_count(p, what);
            if p == cur {
                mine = v;
            }
            best = best.max(v);
        }
        mine >= best
    }

    /// The current player has at most as much as every player (0x42ca00 pattern).
    fn least(&self, what: Count, cur: u32) -> bool {
        let (mut low, mut mine) = (u32::MAX, u32::MAX);
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            let v = self.player_count(p, what);
            if p == cur {
                mine = v;
            }
            low = low.min(v);
        }
        mine <= low
    }

    /// The players of `group` (0x45fbe0..0x45fdd0 and the 13 = current player case);
    /// None for a single player id.
    fn group_players(&self, group: u32) -> Option<Vec<u32>> {
        let cur = self.trig.player;
        let al = |a: u32, b: u32| self.alliances[a as usize][b as usize];
        let mut out = Vec::new();
        let mut it = u32::MAX;
        let mut active = Vec::new();
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            active.push(p);
        }
        match group {
            // Foes / allies / neutral players (alliance 0 / 2 / 1 with the current player).
            14 => out.extend(active.iter().copied().filter(|&p| p != cur && al(cur, p) == 0)),
            15 => out.extend(active.iter().copied().filter(|&p| p != cur && al(cur, p) == 2)),
            16 => out.extend(active.iter().copied().filter(|&p| p != cur && al(cur, p) == 1)),
            // All players: the active ones, then 8..11.
            17 => {
                out.extend(active.iter().copied());
                out.extend(8..12);
            }
            18..=21 => out.extend(active.iter().copied().filter(|&p| self.player_force[p as usize] as u32 == group - 17)),
            22..=25 => {}
            // Non-allied-victory players.
            26 => out.extend(active.iter().copied().filter(|&p| p != cur && !(al(p, cur) == 2 && al(cur, p) == 2))),
            _ => return None,
        }
        Some(out)
    }

    /// A per-player counter summed over `group` (13 = the current player).
    fn group_count(&self, group: u32, what: Count) -> u32 {
        let p = if group == 13 { self.trig.player } else { group };
        match self.group_players(p) {
            Some(ps) => ps.iter().fold(0u32, |s, &q| s.wrapping_add(self.player_count(q, what))),
            None => self.player_count(p, what),
        }
    }

    pub(super) fn player_count(&self, p: u32, what: Count) -> u32 {
        // ponytail: player ids past 11 read past the 1.16.1 arrays; they count as 0 here.
        if p >= 12 {
            return 0;
        }
        let p = p as usize;
        let pl = &self.players;
        let s = &self.trig.snap;
        let v = match what {
            Count::Units(t, true) => match t {
                0xe5 => s.buildings_completed[p] + s.men_completed[p],
                0xe6 => s.men_completed[p],
                0xe7 => s.buildings_completed[p],
                0xe8 => s.factories_completed[p],
                t => s.completed.get(t as usize).map_or(0, |r| r[p]),
            },
            Count::Units(t, false) => match t {
                0xe5 => s.buildings[p] + s.men[p],
                0xe6 => s.men[p],
                0xe7 => s.buildings[p],
                0xe8 => s.factories[p],
                t => s.all.get(t as usize).map_or(0, |r| r[p]),
            },
            Count::Resources(k) => match k {
                0 => pl.minerals[p],
                1 => pl.gas[p],
                2 => pl.gas[p].wrapping_add(pl.minerals[p]),
                _ => 0,
            },
            Count::Kills(t) => match t {
                0xe5 => pl.buildings_razed[p] + pl.men_killed[p],
                0xe6 => pl.men_killed[p],
                0xe7 => pl.buildings_razed[p],
                0xe8 => pl.factories_razed[p],
                t => pl.kills.get(t as usize).map_or(0, |r| r[p]),
            },
            Count::Deaths(t) => match t {
                0xe5 => pl.buildings_lost[p] + pl.men_lost[p],
                0xe6 => pl.men_lost[p],
                0xe7 => pl.buildings_lost[p],
                0xe8 => pl.factories_lost[p],
                t => pl.deaths.get(t as usize).map_or(0, |r| r[p]),
            },
            Count::Score(k) => match k {
                0 => pl.kill_score[p] + pl.razing_score[p] + pl.building_score[p] + pl.unit_score[p] + pl.custom_score[p],
                1 => pl.unit_score[p],
                2 => pl.building_score[p],
                3 => pl.building_score[p] + pl.unit_score[p],
                4 => pl.kill_score[p],
                5 => pl.razing_score[p],
                6 => pl.razing_score[p] + pl.kill_score[p],
                7 => pl.custom_score[p],
                _ => 0,
            },
        };
        v as u32
    }

    /// 0x460160: how many players are against `group`'s player (13 = current).
    fn opponents(&self, group: u32) -> u32 {
        let p = if group == 13 { self.trig.player } else { group };
        if let Some(ps) = self.group_players(p) {
            return ps.iter().fold(0u32, |s, &q| s.wrapping_add(self.player_opponents(q)));
        }
        self.player_opponents(p)
    }

    fn player_opponents(&self, p: u32) -> u32 {
        if p >= 8 || matches!(self.player_types[p as usize], 3 | 7) {
            return 0;
        }
        let al = |a: u32, b: u32| self.alliances[a as usize][b as usize];
        let mutual = |a: u32, b: u32| al(a, b) == 2 && al(b, a) == 2;
        let mut seen = [false; 8];
        let mut it = u32::MAX;
        loop {
            let b = self.next_trigger_player(&mut it);
            if b >= 8 {
                break;
            }
            if matches!(self.player_types[b as usize], 3 | 7) || (b != p && !mutual(p, b)) {
                continue;
            }
            let mut it2 = u32::MAX;
            loop {
                let a = self.next_trigger_player(&mut it2);
                if a >= 8 {
                    break;
                }
                if a != b && !matches!(self.player_types[a as usize], 3 | 7) && !mutual(b, a) {
                    seen[a as usize] = true;
                }
            }
        }
        seen.iter().filter(|&&s| s).count() as u32
    }

    /// 0x45f7a0: copy the live counts once per trigger pass.
    pub(super) fn snapshot_counts(&mut self) {
        if self.trig.snapshot_kind == 1 {
            return;
        }
        let pl = &self.players;
        let s = &mut self.trig.snap;
        s.men = pl.men;
        s.men_completed = pl.men_completed;
        s.buildings = pl.buildings;
        s.buildings_completed = pl.buildings_completed;
        s.factories = pl.factories;
        s.factories_completed = pl.factories_completed;
        s.completed.clone_from(&pl.completed_counts);
        s.all.clone_from(&pl.unit_counts);
        self.trig.snapshot_kind = 1;
    }

    /// 0x4609d0: count the units in location `loc` instead.
    pub(super) fn snapshot_location(&mut self, d: &Data, loc: u8) {
        if self.trig.snapshot_kind == 2 && self.trig.snapshot_location == loc {
            return;
        }
        self.trig.snapshot_kind = 2;
        self.trig.snap = CountSnapshot::default();
        self.trig.snapshot_location = loc;
        let l = self.trig.locations[loc as usize];
        let b = [l.left as i16, l.top as i16, l.right as i16, l.bottom as i16];
        let start = self.units_in_box(d, b);
        for u in self.search.results(start) {
            self.count_location_unit(d, u, l.flags);
        }
        self.search.pop();
    }

    /// 0x460980: count `u` unless the location's elevation flags exclude it.
    fn count_location_unit(&mut self, d: &Data, u: u32, flags: u16) {
        if self.location_excludes(u, flags) {
            return;
        }
        let t = self.units[u as usize].unit_type as usize;
        if self.units[u as usize].status & 0x4000_0000 != 0 || d.dat.units.flags[t] & 0x10 != 0 {
            return;
        }
        // 0x45f800
        let p = self.units[u as usize].player as usize;
        let g = d.dat.units.staredit_group_flags[t];
        let s = &mut self.trig.snap;
        s.all[t][p] += 1;
        if g & 0x20 != 0 {
            s.factories[p] += 1;
        }
        if g & 8 != 0 || (g & 0x10 == 0 && matches!(t, 0x24 | 0x3b | 0x61)) {
            s.men[p] += 1;
        } else if g & 0x10 != 0 {
            s.buildings[p] += 1;
        }
        if self.units[u as usize].status & 1 != 0 {
            self.count_completed_unit(d, u);
        }
    }

    /// 0x45f8d0: location flags 1/2/4 leave out ground units on low/middle/high ground,
    /// 8/0x10/0x20 air units over it.
    fn location_excludes(&self, u: u32, flags: u16) -> bool {
        let un = &self.units[u as usize];
        let s = &self.sprites[un.sprite as usize];
        let level = self.ground_level(s.x as i32, s.y as i32);
        let air = un.status & 4 != 0;
        let bit = match (air, level) {
            (false, 0) => 1,
            (false, 1) => 2,
            (false, 2) => 4,
            (true, 0) => 8,
            (true, 1) => 0x10,
            (true, 2) => 0x20,
            _ => 0,
        };
        flags & bit != 0
    }

    /// 0x4bd0f0: elevation level (0 low, 1 middle, 2 high) of the minitile at (x, y).
    pub fn ground_level(&self, x: i32, y: i32) -> u32 {
        let m = &self.terrain;
        let i = (x / 32 + (y / 32) * m.width as i32) as usize;
        let mut tile = m.creep_saved.get(i).copied().unwrap_or(0);
        if tile == 0 {
            tile = m.mtxm.get(i).copied().unwrap_or(0);
        }
        let mega = m.tileset.megatile(tile) as usize;
        let mini = (((y >> 3) & 3) * 4 + ((x >> 3) & 3)) as usize;
        let f = m.tileset.vf4.get(mega * 16 + mini).copied().unwrap_or(0);
        if f & 4 != 0 {
            2
        } else {
            (f as u32 >> 1) & 1
        }
    }

    /// 0x460860: count a completed unit, with its passengers, hangar and carried items.
    /// 0x45f870: each loaded unit is counted too (inline here).
    /// 0x45fe80: a carrier / reaver counts its hangar interceptors / scarabs (inline here).
    fn count_completed_unit(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        let (t, p) = (un.unit_type as usize, un.player as usize);
        let g = d.dat.units.staredit_group_flags[t];
        self.trig.snap.completed[t][p] += 1;
        if un.status & 0x4000_0000 == 0
            && (t != 0x2a || self.players.upgrade_sc[p][0x18] != 0)
            && d.dat.units.space_provided[t] != 0
        {
            // 0x45f870: count each loaded unit
            for i in 0..8 {
                if let Some(c) = self.unit_from_handle(self.units[u as usize].loaded_units[i] as u32) {
                    self.count_completed_unit(d, c);
                }
            }
        }
        if super::unit::has_hangar(t as u16) {
            // 0x45fe80 (types 0x48 / 0x52 / 0x53 / 0x51): interceptors / scarabs in the hangar.
            let n = self.units[u as usize].u8_at(0xc8) as i32;
            let h = if matches!(t, 0x48 | 0x52) { 0x49 } else { 0x55 };
            self.trig.snap.completed[h][p] += n;
        }
        if d.dat.units.flags[t] & 8 != 0 {
            let c = self.units[u as usize].ptr_at(0xc0);
            if c != NIL {
                let ct = self.units[c as usize].unit_type as usize;
                if ct == 0xd7 {
                    self.count_completed_unit(d, c);
                } else {
                    self.trig.snap.completed[ct][p] += 1;
                }
            }
        }
        if t == 0x6c && self.units[u as usize].ptr_at(0xd4) != NIL {
            self.trig.snap.completed[0xe][p] += 1;
        }
        let s = &mut self.trig.snap;
        if g & 0x20 != 0 {
            s.factories_completed[p] += 1;
        }
        if g & 8 != 0 {
            s.men_completed[p] += 1;
        } else if g & 0x10 != 0 {
            s.buildings_completed[p] += 1;
        }
    }

    /// 0x489e80: "%s was eliminated." (a computer's name too in a replay) or "Player %d
    /// was eliminated." for a computer, in melee-style games (0x59686d / 71 / 77).
    // ponytail: team games (0x57f0b4 / 0x596875: the team's name) are never set here.
    fn defeat_notice(&mut self, p: u8) {
        let m = super::chk_load::ScenarioModes::of_template(&self.template);
        if !(m.flag_59686d || m.flag_596871 || m.flag_596877) {
            return;
        }
        match self.player_types[p as usize] {
            2 => self.notices.push((0x34, p)),
            _ if self.replay => self.notices.push((0x34, p)),
            1 => self.notices.push((0x35, p)),
            _ => {}
        }
    }

    /// 0x4c77d0: the action's wav (map string, a path in the scenario archive) plays for the
    /// local player (not in a replay) unless the trigger was skipped (0x10); the front end
    /// does 0x4bc490. No wait is set here.
    // ponytail: the campaign's path (0x57f244 set: "%s\\%s" of the campaign's directory)
    // is not handled; maps play their own file.
    fn action_play_wav(&mut self, p: usize, i: usize, a: &Action) {
        if !self.replay && self.trig.player as usize == self.local_player as usize && a.wav != 0
            && self.trig.lists[p][i].flags() & 0x10 == 0
        {
            self.trigger_wavs.push(a.wav as u16);
        }
    }

    /// 0x512800: action table. False when the trigger waits here.
    fn action(&mut self, _d: &Data, p: usize, i: usize, k: usize, a: &Action) -> bool {
        let cur = self.trig.player as usize;
        match a.id {
            // 0x4c5350: no action, comment.
            0 | 0x2f => true,
            // 0x4c5310 victory, 0x4c52c0 defeat, 0x4c5070 draw.
            1 => self.set_result(cur, 3, false),
            2 => self.set_result(cur, 2, true),
            0x38 => self.set_result(cur, 5, true),
            // 0x4c52a0: preserve trigger.
            3 => {
                let t = &mut self.trig.lists[p][i];
                let f = t.flags();
                t.set_flags(f | 4);
                true
            }
            // 0x4c5250: wait.
            4 => {
                if self.trig.waiting[cur] != 0 {
                    return false;
                }
                let fl = self.trig.lists[p][i].action_flags_mut(k);
                if *fl & 1 != 0 {
                    *fl &= !1;
                    return true;
                }
                if self.trig.lists[p][i].flags() & 0x10 != 0 {
                    return true;
                }
                self.trig.wait_ms[cur] = a.time;
                self.trig.waiting[cur] = 1;
                *self.trig.lists[p][i].action_flags_mut(k) |= 1;
                false
            }
            // 0x4c5810: unpause game.
            6 => {
                self.action_unpause_game(p, i);
                true
            }
            // 0x4c5770: display text, for the local player unless the trigger was
            // skipped (0x489da0 sets 0x10); the front end shows it (0x48cf20).
            9 => {
                let id = a.string as u16;
                if cur == self.local_player as usize && self.trig.lists[p][i].flags() & 0x10 == 0 && id != 0 {
                    self.shown_text.push((id, a.flags & 4 != 0, 0));
                }
                true
            }
            // 0x4c77d0 play wav.
            8 => {
                self.action_play_wav(p, i, a);
                true
            }
            // 0x4c54b0 minimap ping,
            // 0x4c6990 / 0x4c6940 mute / unmute unit speech: UI only (local player's audio).
            0x1c | 0x1e | 0x1f => true,
            // 0x4c50e0: next scenario: the map string (id in the action's string field, 0 = no
            // change) is copied (SStrCopy, 0x20 bytes) into 0x57f246 for the campaign flow
            // (0x4dc1c0 via the front end); no table or id 0 (low word) clears it.
            0x29 => {
                if a.string != 0 {
                    let name: &[u8] = match (&_d.dat.map_tbl, a.string as u16) {
                        (Some(t), id) if id != 0 => crate::setup::tbl_str(t, id as usize - 1),
                        _ => b"",
                    };
                    self.next_scenario = name[..name.len().min(0x1f)].to_vec();
                }
                true
            }
            0x3a => {
                let t = &mut self.trig.lists[p][i];
                let f = t.flags();
                t.set_flags(f | 0x40);
                true
            }
            0x3b => {
                let t = &mut self.trig.lists[p][i];
                let f = t.flags();
                t.set_flags(f & !0x40);
                true
            }
            // 0x4c51b0: mission objectives.
            0xc => {
                self.trig.objectives[cur] = a.string;
                true
            }
            // 0x4c5590: set switch.
            0xd => {
                if a.number < 0x100 {
                    let (w, b) = ((a.number >> 5) as usize, 1u32 << (a.number & 0x1f));
                    let set = match a.modifier {
                        4 => true,
                        5 => false,
                        6 => {
                            self.trig.switches[w] ^= b;
                            return true;
                        }
                        0xb => (self.rng.rand(0x39) as u8 as i8) < 0,
                        _ => return true,
                    };
                    if set {
                        self.trig.switches[w] |= b;
                    } else {
                        self.trig.switches[w] &= !b;
                    }
                }
                true
            }
            // 0x4c5160: set countdown timer.
            0xe => {
                let v = match a.modifier {
                    7 => a.time as i32,
                    8 => self.trig.countdown.wrapping_add(a.time) as i32,
                    9 => self.trig.countdown.wrapping_sub(a.time) as i32,
                    _ => self.trig.countdown as i32,
                };
                self.trig.countdown = (v.max(0) as u32).min(0x57e3f);
                true
            }
            // 0x4c5520: leaderboards (what they show; UI).
            0x11..=0x15 | 0x21..=0x25 | 0x28 => {
                self.trig.leaderboard_timer = 1;
                let lb = &mut self.trig.leaderboard;
                if a.id == 0x12 || a.id == 0x22 {
                    if a.location as u8 == 0 {
                        return true;
                    }
                    lb.1 = a.location as u8 - 1;
                } else if a.id == 0x28 {
                    lb.0 = a.id;
                    lb.3 = a.number;
                    lb.2 = 2;
                    return true;
                }
                lb.4 = a.string;
                lb.3 = a.number;
                lb.2 = a.unit_type;
                lb.0 = a.id;
                true
            }
            // 0x4c5210: leaderboard computer players.
            0x20 => {
                match a.modifier {
                    4 => self.trig.leaderboard_computers = 1,
                    5 => self.trig.leaderboard_computers = 0,
                    6 => self.trig.leaderboard_computers ^= 1,
                    _ => {}
                }
                true
            }
            // 0x4c6d80 set resources, 0x4c6d20 set score, 0x4c6cc0 set deaths.
            0x1a | 0x1b | 0x2d => {
                if matches!(a.modifier, 7..=9) {
                    self.for_group(a.player, |st, q| st.modify_player_value(a.id, a.modifier, q, a.unit_type, a.number));
                }
                true
            }
            // 0x4c50d0 / 0x4c50c0: pause / unpause the countdown timer.
            0x36 => {
                self.trig.countdown_paused = true;
                true
            }
            0x37 => {
                self.trig.countdown_paused = false;
                true
            }
            // 0x4c8940 kill unit, 0x4c88f0 kill unit at location, 0x4c88c0 remove unit,
            // 0x4c8870 remove unit at location.
            0x16 | 0x17 | 0x18 | 0x19 => {
                let at = matches!(a.id, 0x17 | 0x19);
                if at && a.location as u8 == 0 {
                    return true;
                }
                let (loc, count) = if at {
                    (a.location as u8 as u32 - 1, if a.modifier == 0 { 999_999 } else { a.modifier as u32 })
                } else {
                    (0xff, 999_999)
                };
                let remove = matches!(a.id, 0x18 | 0x19);
                self.for_group(a.player, |st, q| st.trigger_kill_units(_d, q, a.unit_type, loc, count, remove));
                self.trig.snapshot_kind = 0;
                true
            }
            // 0x4c8970: move unit.
            0x27 => {
                if a.location as u8 == 0 || a.number == 0 {
                    return true;
                }
                let count = if a.modifier == 0 { 999_999 } else { a.modifier as u32 };
                let src = a.location as u8 - 1;
                let dst = (a.number as u8).wrapping_sub(1);
                self.trigger_units_in(_d, src, |st, u, flags, left: &mut u32| st.trigger_move_unit(_d, u, a.player, a.unit_type, flags, dst, left), count);
                self.trig.snapshot_kind = 0;
                true
            }
            // 0x4c79f0: order.
            0x2e => {
                if a.location as u8 == 0 || a.number == 0 {
                    return true;
                }
                let dl = self.trig.locations[a.number as usize - 1];
                let dest = (((dl.left as i32 + dl.right as i32) / 2) as i16, ((dl.top as i32 + dl.bottom as i32) / 2) as i16);
                self.trigger_units_in(_d, a.location as u8 - 1, |st, u, flags, _: &mut u32| {
                    st.trigger_order_unit(_d, u, a.player, a.unit_type, flags, dest, a.modifier);
                    false
                }, 0);
                true
            }
            // 0x4c8200 give, 0x4c7c20 hp, 0x4c7230 energy, 0x4c7170 shields, 0x4c70b0
            // resources, 0x4c8b60 hangar: the generic per-unit loop 0x4c69c0.
            0x30 | 0x31 | 0x32 | 0x33 | 0x34 | 0x35 => {
                if a.location as u8 == 0 {
                    return true;
                }
                let mut count: u16 = if a.modifier == 0 { 0xffff } else { a.modifier as u16 };
                let (id, value) = (a.id, a.number);
                self.trigger_units_in(_d, a.location as u8 - 1, |st, u, flags, _: &mut u32| {
                    if count != 0 && !st.location_excludes(u, flags) && st.unit_in_group(a.player, u) && st.trigger_type_matches(_d, a.unit_type, u) {
                        let done = match id {
                            0x30 => st.give_unit_with_parts(_d, u, value as u8),
                            0x31 => st.trigger_set_hp(_d, u, value),
                            0x32 => st.trigger_set_energy(_d, u, value),
                            0x33 => st.trigger_set_shields(_d, u, value),
                            0x34 => st.trigger_set_resources(_d, u, value),
                            _ => st.trigger_add_fighters(_d, u, value),
                        };
                        if done {
                            count -= 1;
                        }
                    }
                    false
                }, 0);
                true
            }
            // 0x4c72f0: set invincibility.
            0x2b => {
                if a.location as u8 == 0 {
                    return true;
                }
                self.trigger_units_in(_d, a.location as u8 - 1, |st, u, flags, _: &mut u32| {
                    st.trigger_invincibility(_d, u, a.player, a.unit_type, Some(flags), a.modifier);
                    false
                }, 0);
                true
            }
            // 0x4c86d0: set alliance status.
            0x39 => {
                let status = a.unit_type;
                self.for_group(a.player, |st, q| st.trigger_set_alliance(q, status));
                // 0x4c04d0: the current player's units drop targets that are now allies.
                let cur = self.trig.player;
                if cur < 12 {
                    let mut u = self.player_units[cur as usize];
                    while u != NIL {
                        let t = self.units[u as usize].order_target_unit;
                        let o = self.units[u as usize].order;
                        if t != NIL && _d.dat.orders.use_weapon_targeting[o as usize] != 0 {
                            let tp = self.units[t as usize].player as usize;
                            if self.alliances[cur as usize][tp] != 0 {
                                let owner = if tp == 11 { self.sprites[self.units[t as usize].sprite as usize].owner as usize } else { tp };
                                if self.alliances[self.units[u as usize].player as usize][owner] != 0 {
                                    self.units[u as usize].order_target_unit = NIL;
                                }
                            }
                        }
                        u = self.units[u as usize].player_next;
                    }
                }
                self.selection_circle_colors(_d);
                true
            }
            // 0x4c7460: center location `number` on a unit in location (or that location).
            0x26 => {
                if a.location as u8 == 0 || a.number == 0 || a.number > 0xff {
                    return true;
                }
                let src = a.location as u8 - 1;
                let (cx, cy) = match self.find_trigger_unit(_d, src, a.unit_type, a.player) {
                    Some(u) => {
                        let p = self.unit_sprite_pos(u);
                        (p.0 as i32, p.1 as i32)
                    }
                    None => {
                        let l = self.trig.locations[src as usize];
                        ((l.left as i32 + l.right as i32) / 2, (l.top as i32 + l.bottom as i32) / 2)
                    }
                };
                let (mw, mh) = (self.map_px.0 as i32, self.map_px.1 as i32);
                let l = &mut self.trig.locations[a.number as usize - 1];
                let (mut x0, mut y0, mut x1, mut y1) = (l.left as i32, l.top as i32, l.right as i32, l.bottom as i32);
                let dx = cx - (x1 + x0) / 2;
                let dy = cy - (y1 + y0) / 2;
                x0 += dx;
                x1 += dx;
                y0 += dy;
                y1 += dy;
                if x0 < 0 {
                    x1 -= x0;
                    x0 = 0;
                }
                if y0 < 0 {
                    y1 -= y0;
                    y0 = 0;
                }
                if x1 >= mw {
                    x0 += mw - x1 - 1;
                    x1 = mw - 1;
                }
                if y1 >= mh {
                    y0 += mh - y1 - 1;
                    y1 = mh - 1;
                }
                (l.left, l.top, l.right, l.bottom) = (x0 as u32, y0 as u32, x1 as u32, y1 as u32);
                self.trig.snapshot_kind = 0;
                true
            }
            // 0x4c75b0: set doodad state.
            0x2a => {
                if a.location as u8 == 0 {
                    return true;
                }
                if let Some(u) = self.find_trigger_unit(_d, a.location as u8 - 1, a.unit_type, a.player) {
                    let un = &self.units[u as usize];
                    let disabled = un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0;
                    match a.modifier {
                        4 => self.trigger_doodad_state(_d, u, true),
                        5 => self.trigger_doodad_state(_d, u, false),
                        0 | 6 => self.trigger_doodad_state(_d, u, disabled),
                        _ => {}
                    }
                }
                true
            }
            // 0x4c8d90 create unit with properties, 0x2c create unit.
            0xb | 0x2c => {
                if a.location as u8 == 0 {
                    return true;
                }
                // 0x51ca10 (the game's start seed) reads 0x75bcd15 meanwhile; nothing in
                // unit creation uses it.
                let n = if a.modifier == 0 { 1 } else { a.modifier as u32 };
                let loc = a.location as u8 - 1;
                let props = a.number;
                for _ in 0..n {
                    self.for_group(a.player, |st, q| st.trigger_create_unit(_d, q, a.unit_type, loc, props));
                }
                true
            }
            // 0x4c5720: run AI script; 0x4c56c0: run AI script at location.
            0xf => {
                self.trigger_run_ai_script(_d, a.number, None);
                true
            }
            0x10 => {
                let l = a.location as u8;
                if l != 0 {
                    let loc = self.trig.locations[l as usize - 1];
                    self.trigger_run_ai_script(_d, a.number, Some([loc.left as i32, loc.top as i32, loc.right as i32, loc.bottom as i32]));
                }
                true
            }
            // 0x4c7570: talking portrait (local player, UI).
            0x1d => true,
            // 0x4c7ac0: pause game (single player).
            5 => {
                let ty = self.player_types[cur];
                if !self.trig.trigger_paused && !matches!(ty, 1 | 3 | 4 | 7) && self.trig.lists[p][i].flags() & 0x10 == 0 {
                    // 0x6d120c / 0x5968e0 dialog handlers, 0x4be0b0: UI.
                    // 0x488800
                    self.trig.paused = true;
                    let f = self.trig.lists[p][i].flags();
                    self.trig.lists[p][i].set_flags(f | 0x20);
                    self.trig.trigger_paused = true;
                }
                true
            }
            // 0x4c6de0: center view (single player: the local player scrolls the screen to
            // the location and the trigger waits for it, callback 0x4894a0).
            0xa => {
                if a.location as u8 == 0 || self.trig.paused || matches!(self.player_types[cur], 1 | 3 | 4 | 7) {
                    return true;
                }
                if self.trig.waiting[cur] != 0 {
                    return false;
                }
                let fl = self.trig.lists[p][i].action_flags_mut(k);
                if *fl & 1 != 0 {
                    *fl &= !1;
                    // 0x47eb30: puts the mouse back to the screen center (UI only).
                    return true;
                }
                if self.trig.lists[p][i].flags() & 0x10 != 0 {
                    return true;
                }
                if cur == self.local_player as usize {
                    let l = self.trig.locations[a.location as u8 as usize - 1];
                    let half = |a: u32, b: u32| {
                        let v = a.wrapping_add(b) as i32;
                        (v + (v < 0) as i32) >> 1
                    };
                    let (mut x, mut y) = (half(l.left, l.right) - 0x140, half(l.top, l.bottom) - 0xc8);
                    let (mw, mh) = (self.map_px.0 as i32, self.map_px.1 as i32);
                    if x < 0 {
                        x = 0;
                    } else if x + 0x280 >= mw {
                        x = mw - 0x281;
                    }
                    if y < 0 {
                        y = 0;
                    } else if y + 0x190 >= mh {
                        y = mh - 0x191;
                    }
                    // 0x47eb70: the front end scrolls to (x & !7, y & !7).
                    self.center_view = Some((x & !7, y & !7));
                }
                *self.trig.lists[p][i].action_flags_mut(k) |= 1;
                self.trig.waiting[cur] = 1;
                // ponytail: without a front end (headless) there is no scroll to wait for:
                // the wait ends at the next pass instead of when 0x4894a0 fires.
                self.trig.wait_ms[cur] = if self.ui_attached { u32::MAX } else { 0 };
                false
            }
            // 0x4c7890: transmission (single player).
            7 => {
                if a.location as u8 == 0 || matches!(self.player_types[cur], 1 | 3 | 4 | 7) {
                    return true;
                }
                if self.trig.waiting[cur] != 0 {
                    return false;
                }
                let fl = self.trig.lists[p][i].action_flags_mut(k);
                if *fl & 1 != 0 {
                    *fl &= !1;
                    return true;
                }
                if self.trig.lists[p][i].flags() & 0x10 != 0 {
                    return true;
                }
                let ms = match a.modifier {
                    7 => a.number,
                    8 => a.time.wrapping_add(a.number),
                    9 => a.time.saturating_sub(a.number),
                    _ => a.time,
                };
                *self.trig.lists[p][i].action_flags_mut(k) |= 1;
                // Only the local player's transmission ever ends (0x45e610); the others
                // wait for good. ponytail: without a front end (headless) the wait is the
                // transmission's length in game ms instead of the real-time timer.
                self.trig.wait_ms[cur] = if cur == self.local_player as usize && !self.ui_attached { ms } else { u32::MAX };
                self.trig.waiting[cur] = 1;
                if cur != self.local_player as usize {
                    return false;
                }
                self.action_play_wav(p, i, a);
                // 0x4c7380 (group 0x11): the unit flashes its selection circle and pings.
                let mut at = None;
                if let Some(u) = self.find_trigger_unit(_d, a.location as u8 - 1, a.unit_type, 0x11) {
                    let sp = self.units[u as usize].sprite as usize;
                    self.sprites[sp].selection_timer = 0x2d;
                    let (x, y) = (self.sprites[sp].x as i32, self.sprites[sp].y as i32);
                    self.minimap_pings.push((x, y, 0x11));
                    at = Some((x, y));
                }
                // 0x45edd0: the talking portrait and the timer.
                // ponytail: its (x, y) hint (0x57fd34 / 0x57fd38, `at` clamped at 0x140 / 200)
                // and the campaign-only portrait swaps are not kept.
                let _ = at;
                self.transmissions.push((a.unit_type, ms));
                // Subtitles (0x6cdfec & 0x400, decided by the front end) or the action's
                // "always display" flag; stays for the longer of the length and the text's
                // reading time (0x4c51d0).
                if a.string != 0 {
                    self.shown_text.push((a.string as u16, a.flags & 4 != 0, ms));
                }
                false
            }
            // The table has 0x3c entries (ids 0..=0x3b) and 0x489130 indexes it with the raw
            // byte, unchecked; later slots hold data (call 1 / call 0), so a map with such an
            // id crashes the original. ponytail: skipped as a no-op instead.
            _ => true,
        }
    }

    /// 0x4c8c20 (one player): create a `t` for player `p` (< 8) in the middle of location
    /// `loc`, with UPRP slot `props` (1..=64) applied.
    fn trigger_create_unit(&mut self, d: &Data, p: u32, t: u16, loc: u8, props: u32) {
        if p >= 8 {
            return;
        }
        // Resources go to the neutral player.
        let owner = if matches!(t, 0xb0 | 0xb1 | 0xb2 | 0xbc) { 0xb } else { p as u8 };
        let l = self.trig.locations[loc as usize];
        let (x, y) = ((l.left as i32 + l.right as i32) / 2, (l.top as i32 + l.bottom as i32) / 2);
        let Some(u) = self.create_unit_checked(d, t, x, y, owner) else { return };
        if props != 0 && props <= 0x40 {
            let slot = self.trig.unit_properties[props as usize - 1];
            self.chk_unit_props(d, u, &slot);
        }
    }

    /// 0x4a0ad0 / 0x4c82d0: keep a `t` at (x, y) inside the map by its box.
    fn clamp_to_map(&self, d: &Data, t: u16, x: i32, y: i32) -> (i32, i32) {
        let dim = d.dat.units.dimensions[t as usize];
        let (l, up, r, dn) = (dim as i16, (dim >> 16) as i16, (dim >> 32) as i16, (dim >> 48) as i16);
        let (mw, mh) = (self.map_px.0 as i32, self.map_px.1 as i32);
        let (mut x, mut y) = (x, y);
        let a = (x as i16).wrapping_sub(l);
        if a < 0 {
            x -= a as i32;
        } else {
            let e = r.wrapping_add(x as i16) as i32;
            if e >= mw {
                x += mw - e - 1;
            }
        }
        let a = (y as i16).wrapping_sub(up);
        if a < 0 {
            y -= a as i32;
        } else {
            let e = dn.wrapping_add(y as i16) as i32;
            if e >= mh {
                y += mh - e - 1;
            }
        }
        (x, y)
    }

    /// A building's top-left corner on the tile grid (inside the map), back to its centre.
    fn snap_building(&self, d: &Data, t: u16, x: i32, y: i32) -> (i32, i32) {
        let v = d.dat.units.placement_box[t as usize];
        let (hw, hh) = ((v as i16 as i32) / 2, ((v >> 16) as i16 as i32) / 2);
        let (mw, mh) = (self.map_px.0 as i32, self.map_px.1 as i32);
        let bx = (x - hw).clamp(0, mw - 1);
        let by = (y - hh).clamp(0, mh - 1);
        ((bx & !0x1f) + hw, (by & !0x1f) + hh)
    }

    /// 0x4c82d0: move the completed building `u` to (x, y) if it fits there; a terran one
    /// drops its addon (and takes one lined up at the new spot), zerg creep follows.
    /// 0x463330: the building whose addon (+0xc0) is `u`, from the visible units (inline here).
    fn trigger_move_building(&mut self, d: &Data, u: u32, x: i32, y: i32) {
        if self.units[u as usize].status & 1 == 0 {
            return;
        }
        let t = self.units[u as usize].unit_type;
        let (x, y) = self.clamp_to_map(d, t, x, y);
        let (x, y) = self.snap_building(d, t, x, y);
        let v = d.dat.units.placement_box[t as usize];
        let (hw, hh) = ((v as i16 as i32) / 2, ((v >> 16) as i16 as i32) / 2);
        let tile = |v: i32| (v + ((v >> 31) & 0x1f)) >> 5;
        let pl = self.units[u as usize].player;
        if self.creation_placement_check(d, pl, (x as u32 >> 5) as i16 - tile(hw) as i16, (y as u32 >> 5) as i16 - tile(hh) as i16, t, 0) != 0 {
            return;
        }
        if !self.unit_fits_in_map(d, t, x, y) {
            return;
        }
        let (ox, oy) = self.unit_sprite_pos(u);
        let terran_building = self.units[u as usize].status & 2 != 0 && self.race_of(d, u) == 1;
        if terran_building {
            if self.units[u as usize].ptr_at(0xc0) != NIL {
                self.detach_addon(d, u);
            }
            // 0x4e66b0 / 0x4e66e0: an addon being built is cancelled.
            let un = &self.units[u as usize];
            let b = un.current_build_unit;
            if un.secondary_order == 0x25 && b != NIL && self.units[b as usize].status & 1 == 0 {
                self.cancel_unit(d, b);
            }
            if d.dat.units.flags[t as usize] & 2 != 0 {
                // 0x463330: the building this addon belongs to.
                let mut p = self.visible_units.head;
                while p != NIL && self.units[p as usize].ptr_at(0xc0) != u {
                    p = self.units[p as usize].link.next;
                }
                if p != NIL {
                    self.detach_addon(d, p);
                }
            }
        }
        self.lift_off_map(d, u);
        self.set_unit_position(d, u, x as i16, y as i16);
        self.put_back_on_map(d, u);
        if self.units[u as usize].status & 2 != 0 && self.race_of(d, u) == 1 {
            self.take_adjacent_addon(d, u);
        }
        self.creep_start_receding(d, t, ox as i32, oy as i32, true);
        let colony = matches!(t, 0x83 | 0x8f | 0x84 | 0x85 | 0x90 | 0x92);
        let (nx, ny) = self.unit_sprite_pos(u);
        if colony {
            self.creep_under_building(d, t, nx as i32, ny as i32);
            self.creep_unmark_receding(d, u);
            self.set_secondary_order(u, 0x66);
        } else if d.dat.units.flags[t as usize] & 0x2_0000 != 0 {
            self.spread_creep_instantly(d, t, nx as i32, ny as i32);
            self.creep_unmark_receding(d, u);
        }
        self.trig.unit_created = true;
    }

    /// 0x4a0ad0: create a `t` at (x, y) (kept inside the map; buildings snapped to the
    /// tile grid and checked for room), finished and placed, with creep and starting
    /// resources.
    pub fn create_unit_checked(&mut self, d: &Data, t: u16, x: i32, y: i32, player: u8) -> Option<u32> {
        if t > 0xe3 || d.dat.units.staredit_availability[t as usize] & 2 == 0 {
            return None;
        }
        let ti = t as usize;
        let flags = d.dat.units.flags[ti];
        let (x, y) = self.clamp_to_map(d, t, x, y);
        let (x, y) = if flags & 1 != 0 { self.snap_building(d, t, x, y) } else { (x, y) };
        if flags & 1 != 0 {
            let v = d.dat.units.placement_box[ti];
            let (hw, hh) = ((v as i16 as i32) / 2, ((v >> 16) as i16 as i32) / 2);
            let (tx, ty) = ((x / 32 - hw / 32) as i16, (y / 32 - hh / 32) as i16);
            if self.creation_placement_check(d, player, tx, ty, t, 0) != 0 {
                self.unit_unplaceable(t, x as i16, y as i16);
                return None;
            }
        }
        let Some(u) = self.create_unit(d, t, x as i16, y as i16, player) else {
            self.show_last_error(player);
            return None;
        };
        let (ux, uy) = self.unit_sprite_pos(u);
        let colony = matches!(t, 0x83 | 0x8f | 0x84 | 0x85 | 0x90 | 0x92);
        if colony {
            self.creep_under_building(d, t, ux as i32, uy as i32);
            self.creep_unmark_receding(d, u);
        } else if flags & 0x2_0000 != 0 {
            self.spread_creep_instantly(d, t, ux as i32, uy as i32);
            self.creep_unmark_receding(d, u);
        }
        if !self.finish_and_place(d, u) {
            let (ux, uy) = self.unit_sprite_pos(u);
            self.unit_unplaceable(t, ux, uy);
            self.kill_unit(d, u);
            return None;
        }
        if colony {
            self.creep_unmark_receding(d, u);
        }
        if d.dat.units.flags[self.units[u as usize].unit_type as usize] & 2 != 0 {
            self.change_owner(d, u, 0xb, false);
        }
        if self.units[u as usize].status & 2 != 0 && self.race_of(d, u) == 1 {
            self.take_adjacent_addon(d, u);
        }
        self.ai_take_unit(d, u);
        self.trig.unit_created = true;
        let ut = self.units[u as usize].unit_type;
        let gas_done = self.units[u as usize].status & 1 != 0 && matches!(ut, 0x6e | 0x9d | 0x95);
        if ut == 0xbc || gas_done {
            self.units[u as usize].set_u16_at(0xd0, 5000);
        } else if super::ai_areas::is_mineral(ut) {
            self.units[u as usize].set_u16_at(0xd0, 1500);
            self.mineral_frames(d, u);
        }
        Some(u)
    }

    /// 0x4c58f0 (one player): the current player's alliance with `q` (0 enemy, 1 ally,
    /// 2 allied victory); a new non-enemy level also applies to every existing ally.
    fn trigger_set_alliance(&mut self, q: u32, status: u16) {
        let cur = self.trig.player as usize;
        if q >= 0x1b || q as usize == cur || cur >= 12 || q >= 12 {
            return;
        }
        let v = match status {
            1 => 1,
            2 => 2,
            _ => 0,
        };
        self.alliances[cur][q as usize] = v;
        if v != 0 && v != self.alliances[cur][cur] {
            for j in 0..12 {
                if self.alliances[cur][j] != 0 {
                    self.alliances[cur][j] = v;
                }
            }
        }
    }

    /// 0x4c7380: the first unit in location `loc` of `group` and type `t` (a unit
    /// carrying or transporting one counts).
    fn find_trigger_unit(&mut self, d: &Data, loc: u8, t: u16, group: u32) -> Option<u32> {
        let l = self.trig.locations[loc as usize];
        let b = [l.left as i16, l.top as i16, l.right as i16, l.bottom as i16];
        let start = self.units_in_box(d, b);
        let mut found = None;
        for u in self.search.results(start) {
            if self.trigger_unit_found(d, u, t, group, Some(l.flags)) {
                found = Some(u);
                break;
            }
        }
        self.search.pop();
        found
    }

    /// 0x4c6f70
    fn trigger_unit_found(&self, d: &Data, u: u32, t: u16, group: u32, flags: Option<u16>) -> bool {
        if let Some(f) = flags {
            if self.location_excludes(u, f) {
                return false;
            }
        }
        let ty = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[ty] & 8 != 0 {
            let c = self.units[u as usize].ptr_at(0xc0);
            if c != NIL && self.trigger_unit_found(d, c, t, group, None) {
                return true;
            }
        }
        if !self.unit_in_group(group, u) {
            return false;
        }
        if self.can_carry(d, u) {
            for i in 0..d.dat.units.space_provided[ty] as usize {
                if self.units[u as usize].loaded_units[i] == 0 {
                    continue;
                }
                if let Some(c) = self.loaded_unit(u, i) {
                    if self.trigger_unit_found(d, c, t, group, None) {
                        return true;
                    }
                }
            }
        }
        self.trigger_type_matches(d, t as u16, u)
    }

    /// 0x47ba00 (enable) / 0x47b960 (disable): a doodad's state changes through the
    /// queued EnableDoodad (0xac) / DisableDoodad (0xad) order.
    fn trigger_doodad_state(&mut self, d: &Data, u: u32, enable: bool) {
        let un = &self.units[u as usize];
        let disabled = un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0;
        if disabled != enable {
            return;
        }
        let order = if enable {
            self.units[u as usize].status &= !0x400;
            0xac
        } else {
            0xad
        };
        self.units[u as usize].order_flags |= 1;
        if self.units[u as usize].order != 0 {
            loop {
                let tail = self.units[u as usize].order_queue_tail;
                if tail == NIL {
                    break;
                }
                let o = self.orders[tail as usize].order;
                if d.dat.orders.interruptible[o as usize] == 0 && o != order {
                    break;
                }
                self.order_remove(d, u, tail);
            }
            self.order_queue_append(d, u, order, 0xe4, (0, 0), NIL);
        }
        self.prepare_next_order(d, u);
    }

    /// Run `f` on each unit in location `loc` (0x4e8830 over the location's box) until
    /// it returns true; `f` gets the location's elevation flags and the shared count.
    fn trigger_units_in(&mut self, d: &Data, loc: u8, mut f: impl FnMut(&mut State, u32, u16, &mut u32) -> bool, mut count: u32) {
        let l = self.trig.locations[loc as usize];
        let b = [l.left as i16, l.top as i16, l.right as i16, l.bottom as i16];
        let start = self.units_in_box(d, b);
        for u in self.search.results(start) {
            if f(self, u, l.flags, &mut count) {
                break;
            }
        }
        self.search.pop();
    }

    /// 0x45fef0: `u`'s owner is in `group` (13 = the current player).
    fn unit_in_group(&self, group: u32, u: u32) -> bool {
        let owner = self.units[u as usize].player as u32;
        let p = if group == 13 { self.trig.player } else { group };
        match self.group_players(p) {
            Some(ps) => ps.contains(&owner),
            None => owner == p,
        }
    }

    /// The unit type test of the location actions (0xe5..0xe8 groups, else exact).
    fn trigger_type_matches(&self, d: &Data, t: u16, u: u32) -> bool {
        let ut = self.units[u as usize].unit_type;
        let g = d.dat.units.staredit_group_flags[ut as usize];
        match t {
            0xe5 => true,
            0xe6 => g & 8 != 0,
            0xe7 => g & 0x10 != 0,
            0xe8 => g & 0x20 != 0,
            t => ut == t,
        }
    }

    /// 0x4c8700: move `u` to the middle of location `dst`. True once `left` is used up.
    /// 0x4c7cf0: lift the unit off the map, place it, find a free spot (back at its old spot on failure; inline here).
    /// 0x4688e0: mineral fields and geysers excluded (inline here).
    /// 0x401430: unit types 0xcb..=0xd5 excluded (inline here).
    #[allow(clippy::too_many_arguments)]
    fn trigger_move_unit(&mut self, d: &Data, u: u32, group: u32, t: u16, flags: u16, dst: u8, left: &mut u32) -> bool {
        if self.location_excludes(u, flags) || !self.trigger_unit_alive(u) || !self.unit_in_group(group, u) {
            return false;
        }
        let un = &self.units[u as usize];
        let ty = un.unit_type;
        let uf = d.dat.units.flags[ty as usize];
        if uf & 1 != 0 && matches!(un.order, 0x47 | 0x48) {
            return false;
        }
        // nukes, 0xca, resources (0x4688e0 and the resource flag), 0xcb..=0xd5 (0x401430)
        if ty == 0xe || ty == 0xca || matches!(ty, 0xb0 | 0xb1 | 0xb2 | 0xbc) || uf & 0x2000 != 0 || (0xcb..=0xd5).contains(&ty) {
            return false;
        }
        if !self.trigger_type_matches(d, t, u) {
            return false;
        }
        let l = self.trig.locations[dst as usize];
        let (x, y) = (((l.left as i32 + l.right as i32) / 2) as i16, ((l.top as i32 + l.bottom as i32) / 2) as i16);
        if self.units[u as usize].status & 2 != 0 {
            self.trigger_move_building(d, u, x as i32, y as i32);
        } else {
            // 0x4c7cf0
            let orig = self.unit_sprite_pos(u);
            self.lift_off_map(d, u);
            self.place_unit_at(d, u, x, y);
            match self.find_unit_placement(d, u, (x, y), None, false) {
                None => {
                    self.set_unit_position(d, u, orig.0, orig.1);
                    self.put_back_on_map(d, u);
                }
                Some(q) => {
                    self.set_unit_position(d, u, q.0, q.1);
                    self.put_back_on_map(d, u);
                    if self.units[u as usize].status & 1 != 0 {
                        self.order_done_or_next(d, u);
                    }
                }
            }
        }
        *left = left.wrapping_sub(1);
        *left == 0
    }

    /// 0x4c7630: order `u` to move (0), patrol (1) or attack-move (2) to `dest`.
    #[allow(clippy::too_many_arguments)]
    fn trigger_order_unit(&mut self, d: &Data, u: u32, group: u32, t: u16, flags: u16, dest: (i16, i16), kind: u8) {
        if self.location_excludes(u, flags) || !self.unit_in_group(group, u) {
            return;
        }
        let un = &self.units[u as usize];
        let ty = un.unit_type;
        if d.dat.units.flags[ty as usize] & 1 != 0 && matches!(un.order, 0x47 | 0x48) {
            return;
        }
        if un.status & 0x20000 == 0 || ty == 0xe || ty == 0x49 || ty == 0x55 || matches!(self.player_types[un.player as usize], 3 | 4) {
            return;
        }
        if !self.trigger_type_matches(d, t, u) {
            return;
        }
        let order = match kind {
            0 => 6,
            1 => 0x98,
            2 => 0xe,
            _ => 0x17,
        };
        if self.units[u as usize].status & 0x10 != 0 {
            self.unburrow(d, u);
            self.queue_order_at(d, u, order, dest, false);
        } else {
            let p = self.units[u as usize].player;
            if self.can_issue_order(d, u, order, p) != Ok(()) {
                return;
            }
            self.order_at(d, u, order, dest);
        }
        // A guard's unit moved or attack-moved by a trigger gets `dest` as its post.
        let r = self.units[u as usize].ai;
        if r != NIL && matches!(order, 6 | 0xe) && super::ai_town::ai_kind(r) == super::ai_town::AI_GUARD {
            self.ai.guards.items[super::ai_town::ai_index(r) as usize].home = dest;
        }
    }

    /// 0x4c8a30: up to `n` new interceptors / scarabs docked in `u` while its owner may
    /// make them; true when at least one was made.
    fn trigger_add_fighters(&mut self, d: &Data, u: u32, n: u32) -> bool {
        let ht = match self.units[u as usize].unit_type {
            0x48 | 0x52 => 0x49,
            0x53 | 0x51 => 0x55,
            _ => return false,
        };
        let mut made = 0;
        while (made as i32) < n as i32 {
            let pl = self.units[u as usize].player;
            if self.can_make_unit(d, u, ht, pl) != Ok(1) {
                break;
            }
            let (x, y) = self.unit_sprite_pos(u);
            if let Some(c) = self.create_unit(d, ht, x, y, pl) {
                self.finish_unit_pre(d, c);
                self.finish_unit_post(d, c);
                self.units[c as usize].set_ptr_at(0xc0, u);
                self.dock_fighter(d, u, c);
            }
            made += 1;
        }
        // 0x68c1e8 / 0x68c1ec / 0x68c1b0 / 0x68ac74 / 0x68c1f8: UI refresh.
        made != 0
    }

    /// 0x4c7b60: hp to `pct` percent of the maximum (at least 1/256).
    fn trigger_set_hp(&mut self, d: &Data, u: u32, pct: u32) -> bool {
        let ty = self.units[u as usize].unit_type as usize;
        let max = d.dat.units.hit_points[ty];
        let hp = ((max.wrapping_mul(pct)) / 100).max(1);
        self.set_unit_hp(d, u, hp as i32);
        let un = &mut self.units[u as usize];
        let halluc = un.status & 0x4000_0000 != 0;
        if halluc || un.unit_type == 0x28 {
            let t = if halluc { 0x546 } else { 0x708 };
            if un.remove_timer == 0 || t < un.remove_timer {
                un.remove_timer = t;
            }
        }
        true
    }

    /// 0x4c6aa0: energy to `pct` percent of the maximum (casters, not hallucinations).
    /// 0x4013b0: the energy field is min(maximum, value) << 8 (inline here).
    fn trigger_set_energy(&mut self, d: &Data, u: u32, pct: u32) -> bool {
        let ty = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[ty] & 0x20_0000 == 0 || self.units[u as usize].status & 0x4000_0000 != 0 {
            return false;
        }
        let max = self.max_energy(d, u) >> 8;
        let v = ((max.wrapping_mul(pct)) / 100) as u8 as u32;
        // 0x4013b0
        self.units[u as usize].energy = (max.min(v) << 8) as u16;
        true
    }

    /// 0x4c5a20: shields to `pct` percent of the maximum.
    fn trigger_set_shields(&mut self, d: &Data, u: u32, pct: u32) -> bool {
        let ty = self.units[u as usize].unit_type as usize;
        if d.dat.units.shield_enable[ty] == 0 {
            return false;
        }
        let max = d.dat.units.shields[ty] as i32;
        let v = ((max as u32).wrapping_mul(pct) / 100) as i32;
        self.units[u as usize].shields = max.min(v) << 8;
        true
    }

    /// 0x4c5670: a resource's amount.
    fn trigger_set_resources(&mut self, d: &Data, u: u32, v: u32) -> bool {
        let ty = self.units[u as usize].unit_type;
        if d.dat.units.flags[ty as usize] & 0x2000 == 0 {
            return false;
        }
        self.units[u as usize].set_u16_at(0xd0, v as u16);
        if super::ai_areas::is_mineral(ty) {
            self.mineral_frames(d, u);
        }
        true
    }

    /// 0x4c6b00: set (4), clear (5) or toggle (6) invincibility; passengers too.
    #[allow(clippy::too_many_arguments)]
    fn trigger_invincibility(&mut self, d: &Data, u: u32, group: u32, t: u16, flags: Option<u16>, modifier: u8) {
        if let Some(f) = flags {
            if self.location_excludes(u, f) {
                return;
            }
        }
        if !self.unit_in_group(group, u) {
            return;
        }
        let ty = self.units[u as usize].unit_type as usize;
        let pl = self.units[u as usize].player as usize;
        if self.units[u as usize].status & 0x4000_0000 == 0
            && (ty != 0x2a || self.players.upgrade_sc[pl][0x18] != 0)
            && d.dat.units.space_provided[ty] != 0
        {
            for i in 0..d.dat.units.space_provided[ty] as usize {
                if self.units[u as usize].loaded_units[i] == 0 {
                    continue;
                }
                if let Some(c) = self.loaded_unit(u, i) {
                    self.trigger_invincibility(d, c, group, t, None, modifier);
                }
            }
        }
        if !self.trigger_type_matches(d, t, u) {
            return;
        }
        let dat_invincible = d.dat.units.flags[ty] & 0x2000_0000 != 0;
        let st = &mut self.units[u as usize].status;
        match modifier {
            4 => *st |= 0x400_0000,
            5 => {
                if !dat_invincible {
                    *st &= !0x400_0000;
                }
            }
            6 => {
                if *st & 0x400_0000 == 0 {
                    *st |= 0x400_0000;
                } else if !dat_invincible {
                    *st &= !0x400_0000;
                }
            }
            _ => {}
        }
    }

    /// 0x4c53d0 / 0x4c5400 / 0x4c5430 / 0x4c5460 / 0x4c5490: `u` is player `p`'s and of
    /// unit type `t` (0xe5 any, 0xe6 men, 0xe7 buildings, 0xe8 factories).
    fn trigger_unit_matches(&self, d: &Data, p: u32, t: u16, u: u32) -> bool {
        let un = &self.units[u as usize];
        if un.player as u32 != p {
            return false;
        }
        let g = d.dat.units.staredit_group_flags[un.unit_type as usize];
        match t {
            0xe5 => true,
            0xe6 => g & 8 != 0,
            0xe7 => g & 0x10 != 0,
            0xe8 => g & 0x20 != 0,
            t => un.unit_type == t,
        }
    }

    /// The unit is alive on the map (sprite, not dying).
    fn trigger_unit_alive(&self, u: u32) -> bool {
        let un = &self.units[u as usize];
        un.sprite != NIL && un.order != 0
    }

    /// 0x4c8570 (one player): kill or remove up to `count` of player `p`'s units of type
    /// `t`, everywhere (`loc` 0xff, 0x4c7d80) or in location `loc` (0x4c8170).
    /// 0x4c7d80: the player's unit list walked, a do-while on `count` (0 kills all; inline here).
    /// 0x4c8170: the units of the location's box (0x4e8830 stops at the first callback hit; inline here).
    fn trigger_kill_units(&mut self, d: &Data, p: u32, t: u16, loc: u32, count: u32, remove: bool) {
        if p >= 12 {
            return;
        }
        if loc >= 0xff {
            let mut left = count;
            let mut u = self.player_units[p as usize];
            while u != NIL {
                let next = self.units[u as usize].player_next;
                let ty = self.units[u as usize].unit_type as usize;
                if d.dat.units.flags[ty] & 0x10 == 0 && self.trigger_unit_alive(u) && self.trigger_unit_matches(d, p, t, u) {
                    let carrier = self.units[u as usize].ptr_at(0xd4);
                    if d.dat.units.flags[ty] & 0x800 != 0 && carrier != NIL {
                        self.kill_carried_powerup(d, carrier);
                    } else {
                        if remove {
                            self.hide_unit(d, u);
                        }
                        self.kill_unit(d, u);
                    }
                    left = left.wrapping_sub(1);
                    if left == 0 {
                        return;
                    }
                }
                u = next;
            }
            return;
        }
        let l = self.trig.locations[loc as usize];
        let b = [l.left as i16, l.top as i16, l.right as i16, l.bottom as i16];
        let start = self.units_in_box(d, b);
        let mut left = count;
        for u in self.search.results(start) {
            if self.trigger_kill_at(d, u, p, t, Some(l.flags), remove, &mut left, false) {
                break;
            }
        }
        self.search.pop();
    }

    /// 0x4c7e20: kill / remove `u` (and first its passengers and carried item) if it
    /// matches; `elevation`: the location's flags to check (none for passengers).
    /// True once `left` reaches 0.
    #[allow(clippy::too_many_arguments)]
    fn trigger_kill_at(&mut self, d: &Data, u: u32, p: u32, t: u16, elevation: Option<u16>, remove: bool, left: &mut u32, inner: bool) -> bool {
        if let Some(f) = elevation {
            if self.location_excludes(u, f) {
                return false;
            }
        }
        let ty = self.units[u as usize].unit_type as usize;
        let pl = self.units[u as usize].player as usize;
        let transport = self.units[u as usize].status & 0x4000_0000 == 0
            && (ty != 0x2a || self.players.upgrade_sc[pl][0x18] != 0)
            && d.dat.units.space_provided[ty] != 0;
        if transport {
            for i in 0..d.dat.units.space_provided[ty] as usize {
                if let Some(c) = self.unit_from_handle(self.units[u as usize].loaded_units[i] as u32) {
                    self.trigger_kill_at(d, c, p, t, None, remove, left, true);
                }
            }
        }
        if d.dat.units.flags[ty] & 8 != 0 {
            let c = self.units[u as usize].ptr_at(0xc0);
            if c != NIL {
                self.trigger_kill_at(d, c, p, t, None, remove, left, true);
            }
        }
        if !self.trigger_unit_alive(u) || !self.trigger_unit_matches(d, p, t, u) {
            return false;
        }
        let carrier = self.units[u as usize].ptr_at(0xd4);
        if d.dat.units.flags[ty] & 0x800 != 0 && carrier != NIL {
            self.kill_carried_powerup(d, carrier);
        } else {
            if remove {
                if transport {
                    self.units[u as usize].order_flags |= 4;
                } else {
                    self.hide_unit(d, u);
                }
            }
            self.kill_unit(d, u);
        }
        if !inner {
            *left = left.wrapping_sub(1);
        }
        *left == 0
    }

    /// Victory (3), defeat (2) and draw (5): only humans and computers get a result; a
    /// player already victorious or ignored keeps it (defeat / draw wait).
    fn set_result(&mut self, cur: usize, r: u8, wait_if_won: bool) -> bool {
        if wait_if_won && matches!(self.trig.result[cur], 3 | 4) {
            return false;
        }
        if matches!(self.player_types[cur], 1 | 2) {
            self.trig.result[cur] = r;
        }
        self.trig.end_screen_timer = if self.trig.trigger_paused { 1 } else { 0x2d };
        true
    }

    /// 0x4c5810: a trigger that paused the game lets it go on.
    fn action_unpause_game(&mut self, p: usize, i: usize) {
        // ponytail: only single player games pause (0x57f0b4 is never set here).
        let ty = self.player_types[self.trig.player as usize];
        if !self.trig.trigger_paused || matches!(ty, 1 | 3 | 4 | 7) {
            return;
        }
        if let Some(t) = self.trig.lists.get_mut(p).and_then(|l| l.get_mut(i)) {
            let f = t.flags();
            t.set_flags(f & !0x20);
        }
        self.trig.trigger_paused = false;
        self.trig.paused = false;
        // 0x4deb90 / 0x4be060 / cursor: UI.
    }

    /// Apply `f` to each player of an action's player group (13 = current, 0..7 itself).
    fn for_group(&mut self, group: u32, mut f: impl FnMut(&mut State, u32)) {
        let p = if group == 13 { self.trig.player } else { group };
        if !matches!(p, 14..=26) {
            return f(self, p);
        }
        self.sum_group(p, u32::MAX, true, &mut |st, q| {
            f(st, q);
            0
        });
    }

    /// 0x45fc40 / 0x45fca0 / 0x45fd00 / 0x45fd60 / 0x45fdd0 / 0x45fbe0 (`early` false) and
    /// 0x45f9a0 / 0x45fa10 / 0x45fa80 / 0x45faf0 / 0x45f930 / 0x45fb60 (`early` true, `count`
    /// = EBX / the stack count): the sum of `f(player)` over the players of group 14 (foes,
    /// 0x45fd60 / 0x45faf0), 15 (allied victory, 0x45fd00 / 0x45fa80), 16 (allies,
    /// 0x45fca0 / 0x45fa10), 17 (all: 0x484ff0 players, then 8..11, 0x45fc40 / 0x45f9a0),
    /// 18..21 (force, 0x45fbe0 / 0x45f930) or 26 (not allied victory, 0x45fdd0 / 0x45fb60).
    /// Players come from 0x484ff0 lazily; the early variants stop once the sum == `count`.
    /// Groups 22..25 have no iterator (callers return 0).
    fn sum_group(&mut self, group: u32, count: u32, early: bool, f: &mut dyn FnMut(&mut State, u32) -> u32) -> u32 {
        let mut sum = 0u32;
        let mut it = u32::MAX;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            let cur = self.trig.player;
            let al = |a: u32, b: u32| self.alliances[a as usize][b as usize];
            let hit = match group {
                14 => p != cur && al(cur, p) == 0,
                15 => p != cur && al(cur, p) == 2,
                16 => p != cur && al(cur, p) == 1,
                17 => true,
                18..=21 => self.player_force[p as usize] as u32 == group - 17,
                26 => p != cur && !(al(p, cur) == 2 && al(cur, p) == 2),
                _ => return 0,
            };
            if hit {
                sum = sum.wrapping_add(f(self, p));
                if early && sum == count {
                    return sum;
                }
            }
        }
        if group == 17 {
            for p in 8..12 {
                sum = sum.wrapping_add(f(self, p));
                if early && sum == count {
                    break;
                }
            }
        }
        sum
    }

    /// 0x4c6830.. (resources), 0x4c6400.. (score), 0x4c5dd0.. (deaths): set (7), add (8)
    /// or subtract (9, floored at 0) `v` for player `p` (< 8).
    fn modify_player_value(&mut self, action: u8, modifier: u8, p: u32, kind: u16, v: u32) {
        if p >= 8 {
            return;
        }
        let p = p as usize;
        let apply = |x: &mut i32, v: u32| {
            *x = match modifier {
                7 => v as i32,
                8 => (*x as u32).wrapping_add(v) as i32,
                _ => (*x as u32).saturating_sub(v) as i32,
            }
        };
        let pl = &mut self.players;
        match action {
            0x1a => {
                if kind == 0 || kind == 2 {
                    apply(&mut pl.minerals[p], v);
                    match modifier {
                        7 => pl.minerals_gathered[p] = v as i32,
                        8 => pl.minerals_gathered[p] = pl.minerals_gathered[p].wrapping_add(v as i32),
                        _ => {}
                    }
                }
                if kind == 1 || kind == 2 {
                    apply(&mut pl.gas[p], v);
                    match modifier {
                        7 => pl.gas_gathered[p] = v as i32,
                        8 => pl.gas_gathered[p] = pl.gas_gathered[p].wrapping_add(v as i32),
                        _ => {}
                    }
                }
            }
            0x1b => {
                let half = (v.wrapping_add(1)) >> 1;
                let quarter = (v.wrapping_add(3)) >> 2;
                match kind {
                    0 => {
                        for x in [&mut pl.unit_score[p], &mut pl.building_score[p], &mut pl.kill_score[p], &mut pl.razing_score[p]] {
                            apply(x, quarter);
                        }
                    }
                    1 => apply(&mut pl.unit_score[p], v),
                    2 => apply(&mut pl.building_score[p], v),
                    3 => {
                        apply(&mut pl.unit_score[p], half);
                        apply(&mut pl.building_score[p], half);
                    }
                    4 => apply(&mut pl.kill_score[p], v),
                    5 => apply(&mut pl.razing_score[p], v),
                    6 => {
                        apply(&mut pl.kill_score[p], half);
                        apply(&mut pl.razing_score[p], half);
                    }
                    7 => apply(&mut pl.custom_score[p], v),
                    _ => {}
                }
            }
            _ => match kind {
                0xe6 => apply(&mut pl.men_lost[p], v),
                0xe7 => apply(&mut pl.buildings_lost[p], v),
                0xe8 => apply(&mut pl.factories_lost[p], v),
                t if t < 0xe4 => apply(&mut pl.deaths[t as usize][p], v),
                _ => {}
            },
        }
    }

    /// 0x48a200: settle victory and defeat after a trigger pass.
    fn resolve_game_end(&mut self, d: &Data) {
        // 0x6d5a6c cheats (single player): 0x80 victory, 0x40 defeat, 0x100 no defeat.
        let local = self.local_player as usize;
        if local < 8 {
            if self.game_options & 0x80 != 0 {
                self.trig.result[local] = 3;
            } else if self.game_options & 0x40 != 0 {
                self.trig.result[local] = 2;
            }
            if self.game_options & 0x100 != 0 && self.trig.result[local] > 1 {
                self.trig.result[local] = 0;
            }
        }
        let mut it = u32::MAX;
        let mut someone_won = false;
        loop {
            let p = self.next_trigger_player(&mut it);
            if p >= 8 {
                break;
            }
            if self.trig.result[p as usize] > 2 {
                someone_won = true;
                break;
            }
        }
        if someone_won {
            let mut it = u32::MAX;
            loop {
                let p = self.next_trigger_player(&mut it);
                if p >= 8 {
                    break;
                }
                if self.trig.result[p as usize] == 0 {
                    self.trig.result[p as usize] = 2;
                    if p as usize == local {
                        self.trig.end_screen_timer = 0x2d;
                    }
                }
            }
            let (mut n, mut won) = (0, 0);
            for p in 0..8 {
                if self.trig.in_game[p] != 0 {
                    n += 1;
                    if self.trig.result[p] > 2 {
                        won += 1;
                    }
                }
            }
            if n == won {
                for p in 0..8 {
                    if self.trig.in_game[p] != 0 {
                        self.trig.result[p] = 5;
                    }
                }
            }
        }
        for p in 0..8 {
            let r = self.trig.result[p];
            if r != 0 && self.player_left[p] == 0 {
                self.player_left[p] = r;
                if r == 2 {
                    self.defeat_notice(p as u8);
                    self.player_defeated(d, p as u8);
                }
            }
        }
    }
}
