//! Phase 3b (agent/areas/net.md): a live networked game. The host builds a melee
//! `GameSetup` on a map, sends it with Start on the lobby stream, and every peer plays the
//! same game through the rollback `Session` inside `view::frame_loop_end` (`NetGame` is the
//! `view::NetDriver` it drives). `scbw nethost <map> [port] [players] [delay]` and
//! `scbw netjoin <code> [delay]`.
//!
//! SCBW_NET_HEADLESS=<K> runs without a window for K frames and prints
//! `frame K hash <state_hash>`; SCBW_NET_SCRIPT="<frame>:<hex command>;..." injects commands
//! into this peer's local turn at that frame (replay-format bytes; `selw` stands for the
//! select command of the first worker of the local player; `say <text>` submits a chat line
//! to all, as Shift+Enter would). SCBW_NET_RACES="t,z,p,r" picks
//! the host's slot races (default: random by the seed), SCBW_NET_BIND the host address.

#[cfg(not(target_arch = "wasm32"))]
use super::transport::ANY;
use super::transport::{Event, HOST, Transport};
use super::Session;
use crate::view;
use crate::view::{GameSetup, NetDriver, Scene};
use std::collections::HashMap;
use std::time::Duration;
use web_time::Instant;

/// How long a finished headless peer keeps serving datagrams for the others.
const LINGER: Duration = Duration::from_secs(3);

struct Out(Vec<u8>);

impl Out {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
}

struct In<'a>(&'a [u8]);

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (a, b) = (self.0.get(..n)?, self.0.get(n..)?);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

/// The wire form of a `GameSetup` (not the map archive, campaign directory or a save) and
/// the humans in storm id order (`melee_setup`).
pub fn encode_setup(g: &GameSetup, order: &[u8]) -> Vec<u8> {
    let mut o = Out(Vec::new());
    o.bytes(&g.chk);
    o.0.extend_from_slice(&g.types);
    o.0.extend_from_slice(&g.races);
    for n in &g.names {
        o.bytes(n.as_bytes());
    }
    o.0.extend_from_slice(&g.forces);
    o.0.extend_from_slice(&g.colors);
    o.0.extend_from_slice(&g.template);
    o.u32(g.seed);
    for v in g.objectives {
        o.u32(v);
    }
    o.0.extend_from_slice(&g.mission.to_le_bytes());
    o.0.push(g.camp_cont as u8 | (g.expansion as u8) << 1 | (g.multiplayer as u8) << 2 | (g.enhanced as u8) << 3);
    // Team games and shared control: who controls which slot, the extra humans' names, the
    // leaders and seat races, and the humans in storm id order (`order`, empty = the slots).
    o.bytes(&g.humans);
    o.u32(g.human_names.len() as u32);
    for n in &g.human_names {
        o.bytes(n.as_bytes());
    }
    o.0.extend_from_slice(&g.team_leaders);
    o.0.extend_from_slice(&g.seat_races);
    o.bytes(order);
    o.0
}

/// `encode_setup`'s inverse; `local` is the receiving peer's human (its slot, or past 8).
/// Also returns the session table: (human, storm id).
pub fn decode_setup(b: &[u8], local: u8) -> Option<(GameSetup, Vec<(u8, u8)>)> {
    let mut i = In(b);
    let chk = i.bytes()?.to_vec();
    let types = i.take(12)?.try_into().ok()?;
    let races = i.take(12)?.try_into().ok()?;
    let mut names: [String; 12] = Default::default();
    for n in names.iter_mut() {
        *n = String::from_utf8_lossy(i.bytes()?).into_owned();
    }
    let forces = i.take(8)?.try_into().ok()?;
    let colors = i.take(8)?.try_into().ok()?;
    let template = i.take(0x20)?.try_into().ok()?;
    let seed = i.u32()?;
    let mut objectives = [0u32; 8];
    for v in objectives.iter_mut() {
        *v = i.u32()?;
    }
    let mission = i.u16()?;
    let fl = i.u8()?;
    let humans = i.bytes()?.to_vec();
    let mut human_names = Vec::new();
    for _ in 0..i.u32()? {
        human_names.push(String::from_utf8_lossy(i.bytes()?).into_owned());
    }
    let team_leaders = i.take(5)?.try_into().ok()?;
    let seat_races = i.take(8)?.try_into().ok()?;
    let order = i.bytes()?;
    let table = order.iter().enumerate().map(|(i, &h)| (h, i as u8)).collect();
    let g = GameSetup {
        chk,
        mpq: None,
        map_dir: None,
        types,
        races,
        names,
        forces,
        colors,
        template,
        local: humans.get(local as usize).copied().unwrap_or(local),
        local_human: local,
        humans,
        human_names,
        team_leaders,
        seat_races,
        seed,
        objectives,
        save: None,
        camp_cont: fl & 1 != 0,
        mission,
        expansion: fl & 2 != 0,
        multiplayer: fl & 4 != 0,
        enhanced: fl & 8 != 0,
    };
    Some((g, table))
}

#[cfg(not(target_arch = "wasm32"))]
/// The scenario.chk of a map argument (.scm / .scx archive, bare .chk, or a replay's).
pub(crate) fn load_chk(path: &str) -> std::io::Result<Vec<u8>> {
    let l = path.to_ascii_lowercase();
    if l.ends_with(".rep") {
        return Ok(crate::replay::Replay::parse(&std::fs::read(path)?)?.chk);
    }
    if l.ends_with(".chk") {
        return std::fs::read(path);
    }
    crate::mpq::Mpq::open(path.as_ref())?.read("staredit\\scenario.chk")
}

#[cfg(not(target_arch = "wasm32"))]
/// The melee game on `chk` for `humans` players: they take the map's first human slots that
/// have a start location, in slot order; other slots stay empty. Races come from
/// SCBW_NET_RACES (`z t p r`, in slot order) or are dealt from the seed. Returns the setup
/// (local = the first human) and the humans in storm id order.
///
/// SCBW_NET_TEAMS=`a,b[,c,d]` plays a team game instead (SCBW_NET_GAME: 11 Team Melee, the
/// default, 12 Team FFA, 13 Team CTF): `a` humans control the first team's slot, `b` the
/// second's... As 1.16.1's lobby leaves it (0x485160): each team's leader takes a start slot,
/// the other members the map's free human slots (type 2, the team's force, no units) and
/// the seats 0x57f1c0 of a team hold its members' races. Ours: members past the 8 slots
/// (up to 12 humans per team) have no slot; they get humans 8.. sharing the leader's slot.
pub fn melee_setup(a: &view::Assets, chk_raw: Vec<u8>, humans: usize, seed: u32) -> Result<(GameSetup, Vec<u8>), String> {
    let teams: Vec<usize> = std::env::var("SCBW_NET_TEAMS").unwrap_or_default().split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if !teams.is_empty() {
        return team_setup(a, chk_raw, &teams, humans, seed);
    }
    let chk = crate::map::Chk::parse(&chk_raw);
    let owners = chk.owners();
    let starts: Vec<u8> = chk.section(b"UNIT").unwrap_or(&[]).chunks_exact(36).filter(|r| u16::from_le_bytes([r[8], r[9]]) == 0xd6 && r[16] < 8).map(|r| r[16]).collect();
    let slots: Vec<u8> = (0..8u8).filter(|&p| owners[p as usize] == 6 && starts.contains(&p)).take(humans).collect();
    if slots.len() < humans {
        return Err(format!("the map has only {} human slots with a start location", slots.len()));
    }
    let wanted: Vec<char> = std::env::var("SCBW_NET_RACES").unwrap_or_default().split(',').filter_map(|s| s.trim().chars().next()).collect();
    let mut rng = seed ^ 0x9e37_79b9;
    let mut g = GameSetup {
        chk: chk_raw,
        mpq: None,
        map_dir: None,
        types: [0; 12],
        races: [0; 12],
        names: Default::default(),
        forces: [0; 8],
        colors: [0, 1, 2, 3, 4, 5, 6, 7],
        template: view::load_template(a, 2, 1),
        local: slots[0],
        local_human: slots[0],
        humans: Vec::new(),
                human_names: Vec::new(),
                team_leaders: [0xff; 5],
                seat_races: [0; 8],
        seed,
        objectives: [0; 8],
        save: None,
        camp_cont: false,
        mission: 0,
        expansion: true,
        multiplayer: true,
        enhanced: std::env::var_os("SCBW_NET_ENHANCED").is_some(),
    };
    let forc = chk.section(b"FORC").filter(|f| f.len() >= 8);
    for (i, &p) in slots.iter().enumerate() {
        g.types[p as usize] = 2;
        g.names[p as usize] = format!("Player {}", i + 1);
        g.races[p as usize] = match wanted.get(i).map(|c| c.to_ascii_lowercase()) {
            Some('z') => 0,
            Some('t') => 1,
            Some('p') => 2,
            _ => {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                (rng % 3) as u8
            }
        };
        g.forces[p as usize] = forc.map_or(1, |f| f[p as usize].wrapping_add(1));
    }
    Ok((g, slots))
}

/// A race from SCBW_NET_RACES' letter, else dealt from `rng`.
fn pick_race(c: Option<&char>, rng: &mut u32) -> u8 {
    match c.map(|c| c.to_ascii_lowercase()) {
        Some('z') => 0,
        Some('t') => 1,
        Some('p') => 2,
        _ => {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 17;
            *rng ^= *rng << 5;
            (*rng % 3) as u8
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// `melee_setup`'s team game: `teams[k]` humans on team k (see there).
fn team_setup(a: &view::Assets, chk_raw: Vec<u8>, teams: &[usize], humans: usize, seed: u32) -> Result<(GameSetup, Vec<u8>), String> {
    if !(2..=4).contains(&teams.len()) || teams.contains(&0) || teams.iter().any(|&n| n > 12) {
        return Err("SCBW_NET_TEAMS: 2 to 4 teams of 1 to 12 humans".into());
    }
    if teams.iter().sum::<usize>() != humans || humans > 32 {
        return Err(format!("SCBW_NET_TEAMS adds up to {} humans, not {humans} (at most 32)", teams.iter().sum::<usize>()));
    }
    let chk = crate::map::Chk::parse(&chk_raw);
    let owners = chk.owners();
    let starts: Vec<u8> = chk.section(b"UNIT").unwrap_or(&[]).chunks_exact(36).filter(|r| u16::from_le_bytes([r[8], r[9]]) == 0xd6 && r[16] < 8).map(|r| r[16]).collect();
    let leaders: Vec<u8> = (0..8u8).filter(|&p| owners[p as usize] == 6 && starts.contains(&p)).take(teams.len()).collect();
    if leaders.len() < teams.len() {
        return Err(format!("the map has only {} human slots with a start location", leaders.len()));
    }
    let mut free = (0..8u8).filter(|&p| owners[p as usize] == 6 && !leaders.contains(&p));
    let gt = std::env::var("SCBW_NET_GAME").ok().and_then(|v| v.parse().ok()).unwrap_or(11);
    let template = view::load_template(a, gt, teams.len() as u16 - 1);
    if template[0x10] as usize != teams.len() {
        return Err(format!("no template for game type {gt} with {} teams", teams.len()));
    }
    let size = crate::bw::players::team_size(template[0x10]) as usize;
    let wanted: Vec<char> = std::env::var("SCBW_NET_RACES").unwrap_or_default().split(',').filter_map(|s| s.trim().chars().next()).collect();
    let mut rng = seed ^ 0x9e37_79b9;
    let mut g = GameSetup {
        chk: chk_raw,
        mpq: None,
        map_dir: None,
        types: [0; 12],
        races: [0; 12],
        names: Default::default(),
        forces: [0; 8],
        colors: [0, 1, 2, 3, 4, 5, 6, 7],
        template,
        local: leaders[0],
        local_human: leaders[0],
        humans: (0..8).collect(),
        human_names: Vec::new(),
        team_leaders: [0xff; 5],
        seat_races: [0; 8],
        seed,
        objectives: [0; 8],
        save: None,
        camp_cont: false,
        mission: 0,
        expansion: true,
        multiplayer: true,
        enhanced: std::env::var_os("SCBW_NET_ENHANCED").is_some(),
    };
    let mut order = Vec::new();
    for (k, (&n, &lead)) in teams.iter().zip(&leaders).enumerate() {
        let force = k as u8 + 1;
        g.team_leaders[force as usize] = lead;
        for j in 0..n {
            let i = order.len();
            let race = pick_race(wanted.get(i), &mut rng);
            let name = format!("Player {}", i + 1);
            let h = if j == 0 { Some(lead) } else { free.next() };
            let h = match h {
                Some(slot) => {
                    g.types[slot as usize] = 2;
                    g.races[slot as usize] = race;
                    g.names[slot as usize] = name;
                    g.forces[slot as usize] = force;
                    g.humans[slot as usize] = lead;
                    slot
                }
                None => {
                    g.humans.push(lead);
                    g.human_names.push(name);
                    g.humans.len() as u8 - 1
                }
            };
            if j < size {
                g.seat_races[k * size + j] = race;
            }
            order.push(h);
        }
    }
    Ok((g, order))
}

/// One scripted injection: (frame, bytes), or `selw`.
enum Cmd {
    Raw(Vec<u8>),
    SelectWorker,
    Say(Vec<u8>),
}

fn parse_script(s: &str) -> Result<Vec<(u32, Cmd)>, String> {
    let mut v = Vec::new();
    for item in s.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (f, c) = item.split_once(':').ok_or_else(|| format!("bad script item {item:?}"))?;
        let f: u32 = f.trim().parse().map_err(|_| format!("bad frame in {item:?}"))?;
        let c = c.trim();
        if let Some(t) = c.strip_prefix("say ") {
            v.push((f, Cmd::Say(t.as_bytes().to_vec())));
            continue;
        }
        if c == "selw" {
            v.push((f, Cmd::SelectWorker));
            continue;
        }
        let hex: String = c.chars().filter(|c| !c.is_whitespace()).collect();
        let b: Result<Vec<u8>, _> = (0..hex.len() / 2).map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2).unwrap_or("zz"), 16)).collect();
        v.push((f, Cmd::Raw(b.map_err(|_| format!("bad hex in {item:?}"))?)));
    }
    Ok(v)
}

/// The select command (0x09) of the first worker (SCV / Drone / Probe) `player` owns.
fn select_worker(st: &crate::bw::State, player: u8) -> Vec<u8> {
    let mut u = st.visible_units.head;
    while u != crate::bw::list::NIL {
        let un = &st.units[u as usize];
        if un.player == player && matches!(un.unit_type, 7 | 41 | 64) {
            let h = (st.unit_handle(u) as u16).to_le_bytes();
            return vec![0x09, 1, h[0], h[1]];
        }
        u = un.link.next;
    }
    Vec::new()
}

/// A result line: stdout natively, the console in the browser.
fn say(s: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&s.into());
    #[cfg(not(target_arch = "wasm32"))]
    println!("{s}");
}

pub struct NetGame {
    se: Session,
    tr: Transport,
    next_send: u32,
    script: Vec<(u32, Cmd)>,
    /// SCBW_NET_HEADLESS: the frame to play to.
    headless: Option<u32>,
    finished: Option<Instant>,
    host: bool,
    verbose: bool,
    last_report: Instant,
    /// Browser: play to this frame and log `frame K hash H` (the headless line, for the console).
    report: Option<u32>,
    reported: bool,
    /// Host: (frame -> first (player, hash) seen), for the desync compare.
    hashes: HashMap<u32, (u8, u64)>,
    desync: Option<u32>,
    /// Why the game cannot go on (the host left).
    over: Option<String>,
    /// Host: clean quits waiting for the quitter's turns to arrive: (player, its next frame, since).
    quitting: Vec<(u8, u32, Instant)>,
    /// SCBW_NET_REPLAY=<path>: a headless game writes its replay there at the end.
    replay_path: Option<String>,
    /// SCBW_NET_QUIT_AT=<frame>: a headless peer quits (as the in-game menu would) at that frame.
    quit_at: Option<u32>,
    /// Since when the session has been held by missing turns (`Session::waiting_on`).
    stall_since: Option<Instant>,
    /// Host of an enhanced game: players asking to join (link id, army wanted, name), and the
    /// one being let in (link id, human, army, its join command's frame once sent).
    join_queue: Vec<(u8, u8, Vec<u8>)>,
    joining: Option<(u8, u8, u8, Option<u32>)>,
    /// An enhanced game: players who hold it are dropped after ENH_DROP, no waiting dialog.
    drops_fast: bool,
    /// Joiners sent their start: when (they load it before their turns come).
    joined_at: Vec<(u8, Instant)>,
    /// Ours (campaign matches): the army a joiner of this name may take (None: refused).
    pub(crate) join_filter: Option<Box<dyn FnMut(&[u8]) -> Option<u8>>>,
    /// Links refused or kicked since the last look (the campaign host closes them).
    pub(crate) refused: Vec<u8>,
}

/// A joiner's time to take the game's state in and load it before a stall drops it.
const JOIN_GRACE: Duration = Duration::from_secs(60);

/// Enhanced games: a player whose turns hold the game this long is dropped (no waiting
/// dialog).
const ENH_DROP: Duration = Duration::from_secs(3);
/// The parts of a joiner's start (C_LATE): under the transport's 600-byte control budget.
const LATE_PART: usize = 500;

thread_local! {
    static REPORT_NEXT: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Control messages (first byte; see the module doc).
const C_LEAVE: u8 = 1;
const C_LEAVE_REQ: u8 = 2;
const C_HASH: u8 = 3;
const C_DESYNC: u8 = 4;
const C_HOST_LEFT: u8 = 5;
/// A Storm message (chat / whisper, `chat::Chat::out`): client -> host `[6, to u32, packet]`,
/// host -> client `[6, from, packet]`; the host passes each on to the humans in `to` (a bit
/// per human: ours, humans can pass 16).
const C_CHAT: u8 = 6;
/// A client pressed Drop Players: `[7]`; the host drops the players it is itself waiting for.
const C_DROP: u8 = 7;
/// Host -> an enhanced game's joiner: a part of its start (`late_game`): `idx u16`,
/// `count u16`, bytes (snet::LATE).
const C_LATE: u8 = super::snet::LATE;

/// How long the session must be held before the waiting dialog shows. Ours: 1.16.1 opens it
/// at the first missing turn, but its turns are buffered ahead (0x57f090) where a rollback
/// session runs into short holds that end on their own.
const WAIT_SHOW: Duration = Duration::from_millis(500);

fn rd32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl NetGame {
    /// Browser: the SCBW_NET_SCRIPT text (`<frame>:<hex>;...`) for this peer.
    pub(crate) fn set_script(&mut self, s: &str) -> Result<(), String> {
        self.script = parse_script(s)?;
        Ok(())
    }

    /// Stop at frame `k` and report the state hash there (see `report`).
    pub(crate) fn report_at(&mut self, k: u32) {
        self.report = Some(k);
        self.se.end = k;
    }

    /// The next game reports at frame `k` (`report_at`; the browser's ?frames= for a game the
    /// lobby starts).
    pub(crate) fn report_next(k: u32) {
        REPORT_NEXT.with(|r| r.set(Some(k)));
    }

    pub(crate) fn new(se: Session, tr: Transport, host: bool, early: Vec<(u8, u32, Vec<u8>)>) -> Result<NetGame, String> {
        let script = parse_script(&std::env::var("SCBW_NET_SCRIPT").unwrap_or_default())?;
        let headless = std::env::var("SCBW_NET_HEADLESS").ok().and_then(|v| v.parse().ok());
        let mut se = se;
        se.record = true;
        if let Some(k) = headless {
            se.end = k;
        }
        // Turns that beat the Start message here are not delivered again.
        for (p, f, b) in early {
            se.remote_turn(p, f, b);
        }
        let next_send = se.frame + se.delay;
        let mut n = NetGame::make(se, tr, next_send, script, headless, host);
        if let Some(k) = REPORT_NEXT.with(|r| r.take()) {
            n.report_at(k);
            n.verbose = true;
        }
        Ok(n)
    }

    /// Host: remove link `p` from the game (a kick), as a dropped player.
    pub(crate) fn kick(&mut self, p: u8) {
        self.join_queue.retain(|q| q.0 != p);
        if self.se.live_players().iter().any(|q| q.0 == p) {
            self.decide_leave(p, 6);
        }
        self.refused.push(p);
    }

    /// The players in the game: (link id, human).
    pub(crate) fn links(&self) -> Vec<(u8, u8)> {
        self.se.live_players().into_iter().map(|q| (q.0, q.2)).collect()
    }

    /// The session's next frame and its first frame with a turn missing.
    pub(crate) fn frames(&self) -> (u32, u32) {
        (self.se.frame, self.se.confirmed)
    }

    /// Host: `player` is gone (quit, or its connection died). Fence at `Session::fence_for`
    /// of the turns the host holds, tell everybody (reliable), apply it here.
    fn decide_leave(&mut self, player: u8, reason: u8) {
        if !self.host || self.se.is_gone(player) {
            return;
        }
        let last = self.se.contiguous(player);
        let fence = Session::fence_for(last);
        let mut msg = vec![C_LEAVE, player];
        msg.extend_from_slice(&last.to_le_bytes());
        msg.extend_from_slice(&fence.to_le_bytes());
        msg.push(reason);
        eprintln!("net: LEAVE player {player} (reason {reason}): turns below {last} kept, fence frame {fence}, host at frame {}", self.se.frame);
        self.se.apply_leave(player, last, fence, reason);
        self.tr.broadcast_ctl(&msg);
    }

    /// Host of an enhanced game, each frame: players holding the game too long are dropped;
    /// the next asking player's join command goes into my turn (the army it gets, 0x51267c
    /// / 0x512678 as `enhanced_pick` chose); once that frame is final its start state (a
    /// saved game of it, compressed, with what the save does not keep) goes to the joiner
    /// in parts and its turns flow from there.
    fn enhanced_host(&mut self, scene: &mut Scene<'_>) {
        self.drops_fast = true;
        self.joined_at.retain(|j| j.1.elapsed() < JOIN_GRACE);
        if self.stall_since.is_some_and(|t| t.elapsed() >= ENH_DROP) {
            for p in self.se.waiting_on() {
                if !self.joined_at.iter().any(|j| j.0 == p) {
                    eprintln!("net: player {p} held the game {} ms: dropped", self.stall_since.map_or(0, |t| t.elapsed().as_millis()));
                    self.decide_leave(p, 6);
                }
            }
        }
        if self.joining.is_none() && !self.join_queue.is_empty() {
            let (p, mut want, name) = self.join_queue.remove(0);
            if let Some(f) = self.join_filter.as_mut() {
                match f(&name) {
                    Some(a) => want = a,
                    None => {
                        eprintln!("net: player {p} ({}) may not join this game", String::from_utf8_lossy(&name));
                        self.refused.push(p);
                        return;
                    }
                }
            }
            match scene.st.enhanced_pick(want).filter(|&(_, n)| self.join_filter.is_none() || n == want) {
                Some((h, n)) => {
                    eprintln!("net: player {p} joins as human {h}, army {n}");
                    scene.st.send_command(&crate::bw::enhanced::join_command(h, n, p, &name));
                    self.joining = Some((p, h, n, None));
                }
                None => {
                    eprintln!("net: player {p} cannot join: no army");
                    self.refused.push(p);
                }
            }
        }
        let Some((p, h, n, Some(f))) = self.joining else { return };
        let Some(snap) = self.se.kept(f).cloned() else { return };
        let players: Vec<(u8, u8, u8)> = self.se.live_players().into_iter().filter(|q| q.0 != p).collect();
        let Some(start) = late_bytes(scene, &snap, f, p, h, n, self.se.delay, &players) else {
            eprintln!("net: the game state for player {p} could not be saved");
            self.joining = None;
            return;
        };
        let parts: Vec<&[u8]> = start.chunks(LATE_PART).collect();
        for (i, c) in parts.iter().enumerate() {
            let mut m = vec![C_LATE];
            m.extend_from_slice(&(i as u16).to_le_bytes());
            m.extend_from_slice(&(parts.len() as u16).to_le_bytes());
            m.extend_from_slice(c);
            self.tr.send_ctl(p, m);
        }
        eprintln!("net: player {p} gets the game at frame {f} ({} bytes)", start.len());
        self.tr.start_peer(p, f);
        self.joined_at.push((p, Instant::now()));
        self.se.keep = None;
        self.joining = None;
    }

    /// A control message from `from` (see the constants above).
    fn ctl(&mut self, from: u8, msg: &[u8], scene: &mut Scene<'_>) {
        match (msg.first().copied(), self.host) {
            (Some(C_LEAVE), false) if from == HOST => {
                if let (Some(&p), Some(last), Some(fence), Some(&reason)) = (msg.get(1), rd32(msg, 2), rd32(msg, 6), msg.get(10)) {
                    eprintln!("net: leave of player {p}: fence frame {fence} (at frame {})", self.se.frame);
                    self.se.apply_leave(p, last, fence, reason);
                }
            }
            (Some(C_LEAVE_REQ), true) => {
                if let Some(upto) = rd32(msg, 1) {
                    if !self.quitting.iter().any(|q| q.0 == from) {
                        self.quitting.push((from, upto, Instant::now()));
                    }
                }
            }
            (Some(C_HASH), true) => {
                if let (Some(m), Some(lo), Some(hi)) = (rd32(msg, 1), rd32(msg, 5), rd32(msg, 9)) {
                    self.on_hash(from, m, lo as u64 | (hi as u64) << 32, scene);
                }
            }
            (Some(C_DESYNC), false) if from == HOST => {
                if let Some(m) = rd32(msg, 1) {
                    self.report_desync(m, scene);
                }
            }
            (Some(C_HOST_LEFT), false) if from == HOST => self.over = Some("the host left".into()),
            // snet's NAME (a link that joined after the start): an enhanced game lets it in.
            (Some(0x83), true) if scene.st.enhanced && self.se.live_players().iter().all(|p| p.0 != from) => {
                let body = &msg[1..];
                let end = body.iter().position(|&c| c == 0).unwrap_or(body.len());
                let want = body.get(end + 1).copied().unwrap_or(0xff);
                if !self.join_queue.iter().any(|q| q.0 == from) && self.joining.is_none_or(|j| j.0 != from) {
                    eprintln!("net: player {from} asks to join the running game");
                    self.join_queue.push((from, want, body[..end.min(0x17)].to_vec()));
                }
            }
            (Some(C_DROP), true) => {
                eprintln!("net: player {from} asks to drop the players the game waits for");
                self.drop_waiting();
            }
            (Some(C_CHAT), true) => {
                if let (Some(to), Some(pkt)) = (msg.get(1..5), msg.get(5..)) {
                    self.route_chat(from, u32::from_le_bytes([to[0], to[1], to[2], to[3]]) & !(1 << from), pkt, scene);
                }
            }
            (Some(C_CHAT), false) if from == HOST => {
                if let (Some(&p), Some(pkt)) = (msg.get(1), msg.get(2..)) {
                    self.chat_in(p, pkt, scene);
                }
            }
            _ => {}
        }
    }

    /// The players still in the game (0x66fe20 == 1), a bit per player.
    fn active(&self) -> u32 {
        self.se.players().filter(|&p| !self.se.is_gone(p)).fold(0, |m, p| m | 1 << p)
    }

    /// SNetSendMessage (ord 127) to each player in `to`: Storm hands a message to oneself
    /// at once; the rest go through the host.
    fn send_chat(&mut self, to: u32, pkt: Vec<u8>, scene: &mut Scene<'_>) {
        let me = self.se.me;
        if to & 1 << me != 0 {
            self.chat_in(me, &pkt, scene);
        }
        let to = to & !(1 << me);
        if to == 0 {
            return;
        }
        if self.host {
            self.route_chat(me, to, &pkt, scene);
        } else {
            let mut msg = vec![C_CHAT];
            msg.extend_from_slice(&to.to_le_bytes());
            msg.extend_from_slice(&pkt);
            self.tr.send_ctl(HOST, msg);
        }
    }

    /// Host: a message of `from` for the players in `to`.
    fn route_chat(&mut self, from: u8, to: u32, pkt: &[u8], scene: &mut Scene<'_>) {
        let to = to & self.active();
        for p in (0..32u8).filter(|&p| to & 1 << p != 0) {
            if p == self.se.me {
                self.chat_in(from, pkt, scene);
            } else {
                let mut msg = vec![C_CHAT, from];
                msg.extend_from_slice(pkt);
                self.tr.send_ctl(p, msg);
            }
        }
    }

    /// 0x485f70 (SNetReceiveMessage, ord 121): a message of player `from` by its first byte,
    /// 0 chat (0x485f50) and 1 whisper (0x485f30); 2 (0x47d170) and others are not ours.
    /// A chat line is recorded at the frame it arrives in (0x4cde70 under the local storm
    /// id) and shown (`State::chat`, 0x48d1c0).
    fn chat_in(&mut self, from: u8, pkt: &[u8], scene: &mut Scene<'_>) {
        if self.active() & 1 << from == 0 || pkt.first().is_none_or(|&t| t > 1) {
            return;
        }
        if let Some(cmd) = crate::chat::receive(&mut scene.chat, from, pkt, pkt[0] == 1) {
            scene.st.chat.push((from, cmd[2..].to_vec()));
            self.se.record_chat(cmd);
        }
    }

    /// Host: compare a peer's hash of frame `m` with the first one seen for it.
    fn on_hash(&mut self, player: u8, m: u32, h: u64, scene: &mut Scene<'_>) {
        match self.hashes.get(&m) {
            None => {
                self.hashes.insert(m, (player, h));
                self.hashes.retain(|&f, _| f + 4096 > m);
            }
            Some(&(p0, h0)) if h0 != h && self.desync.is_none() => {
                eprintln!("net: hash mismatch at frame {m}: player {p0} {h0:016x}, player {player} {h:016x}");
                let mut msg = vec![C_DESYNC];
                msg.extend_from_slice(&m.to_le_bytes());
                self.tr.broadcast_ctl(&msg);
                self.report_desync(m, scene);
            }
            _ => {}
        }
    }

    fn report_desync(&mut self, m: u32, scene: &mut Scene<'_>) {
        if self.desync.is_some() {
            return;
        }
        self.desync = Some(m);
        let text = format!("DESYNC detected at frame {m}");
        say(&text);
        eprintln!("net: {text}");
        scene.msgs.error(text.as_bytes(), scene.clock_ms);
    }

    fn make(se: Session, tr: Transport, next_send: u32, script: Vec<(u32, Cmd)>, headless: Option<u32>, host: bool) -> NetGame {
        NetGame { se, tr, next_send, script, headless, finished: None, host, verbose: std::env::var_os("NETREP_DEBUG").is_some(), last_report: Instant::now(), report: None, reported: false, hashes: HashMap::new(), desync: None, over: None, quitting: Vec::new(), replay_path: std::env::var("SCBW_NET_REPLAY").ok(), quit_at: std::env::var("SCBW_NET_QUIT_AT").ok().and_then(|v| v.parse().ok()), stall_since: None, join_queue: Vec::new(), joining: None, drops_fast: false, joined_at: Vec::new(), join_filter: None, refused: Vec::new() }
    }
}

impl NetDriver for NetGame {
    fn headless(&self) -> bool {
        self.headless.is_some()
    }

    fn frame(&mut self, scene: &mut Scene<'_>) -> Option<Vec<(u16, i32, i32)>> {
        if self.se.frame == 0 {
            let t = &scene.st.template;
            crate::replay_rec::set_net_header(scene.st.game_seed, u16::from_le_bytes([t[0], t[1]]), u16::from_le_bytes([t[2], t[3]]));
        }
        if self.over.is_some() {
            return None;
        }
        // My turn for frame + delay: the commands the UI queued since the last one.
        if self.se.frame + self.se.delay == self.next_send && self.se.frame < self.se.end {
            self.next_send += 1;
            let f = self.se.frame;
            for (_, c) in self.script.iter().filter(|(sf, _)| *sf == f) {
                let b = match c {
                    Cmd::Raw(b) => b.clone(),
                    Cmd::SelectWorker => select_worker(&scene.st, scene.st.nation_of(self.se.me)),
                    Cmd::Say(t) => {
                        scene.chat.mode = 2;
                        scene.chat.send(&scene.st, t);
                        continue;
                    }
                };
                scene.st.send_command(&b);
            }
            let bytes = std::mem::take(&mut scene.st.turn_out);
            let (f, b) = self.se.local_turn(bytes);
            // The join command went out in this turn: its frame's start state is kept.
            if let Some(j) = self.joining.as_mut().filter(|j| j.3.is_none()) {
                if crate::replay::split_commands(&b).iter().any(|c| c[0] == crate::bw::enhanced::CMD_JOIN && c.get(3) == Some(&j.0)) {
                    j.3 = Some(f);
                    self.se.keep = Some((f, None));
                }
            }
            self.tr.send_turns(f, b);
        }
        scene.chat.active = self.active();
        for (to, pkt) in std::mem::take(&mut scene.chat.out) {
            self.send_chat(to, pkt, scene);
        }
        for e in self.tr.poll() {
            match e {
                Event::Turn { player, frame, bytes } => self.se.remote_turn(player, frame, bytes),
                Event::Ctl { from, msg } => self.ctl(from, &msg, scene),
                Event::Lost { player, why } => {
                    eprintln!("net: player {player} lost: {why}");
                    self.decide_leave(player, 6);
                }
                Event::Error(m) => {
                    say(&format!("net: {m}"));
                    // A client that already holds every turn up to its end (a test's frame
                    // count) plays them out: the host may close as soon as its turns are acked.
                    if !self.host && self.se.confirmed < self.se.end {
                        self.over = Some(format!("lost the host ({m})"));
                    }
                }
                // A link after the start (an enhanced game's joiner): no turns until it is let in.
                Event::Joined { player } => self.tr.hold_peer(player),
                Event::Start { .. } => {}
            }
        }
        // Host: a clean quitter's leave once its last turns are in (or after a grace period).
        let ready: Vec<(u8, u32, Instant)> = self.quitting.iter().copied().filter(|&(p, upto, t)| self.se.contiguous(p) >= upto || t.elapsed() > Duration::from_millis(1500)).collect();
        self.quitting.retain(|q| !ready.iter().any(|r| r.0 == q.0));
        for (p, _, _) in ready {
            self.decide_leave(p, 1);
        }
        if self.over.is_some() {
            return None;
        }
        self.drops_fast = scene.st.enhanced;
        if self.host && scene.st.enhanced {
            self.enhanced_host(scene);
        }
        if self.verbose && self.last_report.elapsed() > Duration::from_secs(2) {
            self.last_report = Instant::now();
            let se = &self.se;
            say(&format!("net player {}: frame {} confirmed {} stalled {} rollbacks {} dgrams {:?}", se.me, se.frame, se.confirmed, se.stalled, se.rollbacks, self.tr.stats));
        }
        let stepped = self.se.tick(scene);
        for (p, f) in std::mem::take(&mut self.se.added) {
            self.tr.player_from(p, f);
        }
        if self.se.waiting_on().is_empty() {
            self.stall_since = None;
        } else {
            self.stall_since.get_or_insert_with(Instant::now);
        }
        // Desync check: the state hash of every 8th confirmed frame goes to the host.
        for (m, h) in self.se.due_hashes(&scene.st) {
            if self.host {
                self.on_hash(self.se.me, m, h, scene);
            } else {
                let mut msg = vec![C_HASH];
                msg.extend_from_slice(&m.to_le_bytes());
                msg.extend_from_slice(&h.to_le_bytes());
                self.tr.send_ctl(HOST, msg);
            }
        }
        let se = &mut self.se;
        if let Some(k) = self.report.filter(|&k| !self.reported && se.frame >= k && se.confirmed >= k) {
            self.reported = true;
            say(&format!("frame {k} hash {:016x}", scene.st.state_hash()));
            say(&format!("player {} rollbacks {} resimulated {} stalled {}", se.me, se.rollbacks, se.resimulated, se.stalled));
        }
        stepped.then(|| std::mem::take(&mut se.sounds))
    }

    fn over(&self) -> Option<String> {
        self.over.clone()
    }

    fn waiting(&self) -> Vec<u8> {
        if self.drops_fast {
            return Vec::new();
        }
        match self.stall_since {
            Some(t) if t.elapsed() >= WAIT_SHOW => self.se.waiting_on().into_iter().filter(|&p| p != self.se.me).collect(),
            _ => Vec::new(),
        }
    }

    /// The host drops whom it waits for (reason 6, as a lost connection); a client asks the
    /// host. ponytail: 0x4a3010's "more players lagging than responding: leave yourself"
    /// is not done; the host decides every drop.
    fn drop_waiting(&mut self) {
        if self.host {
            for p in self.se.waiting_on() {
                self.decide_leave(p, 6);
            }
        } else {
            self.tr.send_ctl(HOST, vec![C_DROP]);
        }
    }

    fn pump_headless(&mut self, scene: &mut Scene<'_>) -> bool {
        let k = self.headless.unwrap_or(0);
        let nap = || std::thread::sleep(Duration::from_millis(1));
        let Some(t0) = self.finished else {
            if self.frame(scene).is_none() {
                nap();
            } else if let Some(us) = std::env::var("SCBW_NET_PACE_US").ok().and_then(|v| v.parse().ok()) {
                // Tests: a slow peer (a browser) can keep up with a paced headless one.
                std::thread::sleep(Duration::from_micros(us));
            }
            if self.over.is_some() {
                println!("game over: {}", self.over.as_deref().unwrap_or(""));
                return false;
            }
            if self.quit_at.is_some_and(|q| self.se.frame >= q) {
                println!("quit at frame {}", self.se.frame);
                return false; // dropping the game announces the leave
            }
            if self.se.frame >= k && self.se.confirmed >= k {
                println!("frame {k} hash {:016x}", scene.st.state_hash());
                println!("player {} rollbacks {} resimulated {} stalled {}", self.se.me, self.se.rollbacks, self.se.resimulated, self.se.stalled);
                if let Some(p) = self.replay_path.take() {
                    match scene.replay_file() {
                        Some(b) => std::fs::write(&p, b).unwrap_or_else(|e| eprintln!("net: {p}: {e}")),
                        None => eprintln!("net: no replay recorded"),
                    }
                }
                self.finished = Some(Instant::now());
            }
            return true;
        };
        // Keep serving until the peers have the last turns (a client: the host acked mine).
        for e in self.tr.poll() {
            match e {
                Event::Ctl { from, msg } => self.ctl(from, &msg, scene),
                Event::Lost { player, .. } => self.decide_leave(player, 6),
                _ => {}
            }
        }
        nap();
        if self.over.is_some() {
            return false;
        }
        !(t0.elapsed() > LINGER || (self.tr.fully_acked(k + self.se.delay) && (self.host || t0.elapsed() > Duration::from_millis(300))))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn lobby_wait(tr: &mut Transport) -> Result<(u8, u32, Vec<u8>, Vec<(u8, u32, Vec<u8>)>), String> {
    let mut early = Vec::new();
    loop {
        for e in tr.poll() {
            match e {
                Event::Start { slot, first, setup } => return Ok((slot, first, setup, early)),
                Event::Error(m) => return Err(m),
                Event::Turn { player, frame, bytes } => early.push((player, frame, bytes)),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn play(mut a: view::Assets, mut g: GameSetup, se: Session, tr: Transport, host: bool, early: Vec<(u8, u32, Vec<u8>)>) -> std::io::Result<()> {
    let net = NetGame::new(se, tr, host, early).map_err(std::io::Error::other)?;
    view::set_net_driver(Box::new(net));
    g.multiplayer = true;
    view::run_setup(&mut a, g)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
/// `scbw nethost <map> [port] [players] [delay]`: print the join code, wait for
/// players - 1 joins, start.
pub fn nethost(files: crate::storage::Storage, map: &str, port: u16, players: usize, delay: u32) -> std::io::Result<()> {
    let delay = delay.max(1);
    let a = view::Assets::load(files)?;
    let seed = std::env::var("SCBW_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |t| t.as_secs() as u32));
    let (mut g, slots) = melee_setup(&a, load_chk(map)?, players.clamp(1, 32), seed).map_err(std::io::Error::other)?;
    let (tr, code) = Transport::host(port, slots[0], &slots[1..], delay, encode_setup(&g, &slots)).map_err(std::io::Error::other)?;
    println!("LISTEN {code}");
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut tr = tr;
    let (_, _, _, early) = lobby_wait(&mut tr).map_err(std::io::Error::other)?;
    g.local = g.humans.get(slots[0] as usize).copied().unwrap_or(slots[0]);
    g.local_human = slots[0];
    g.mpq = crate::mpq::Mpq::open(map.as_ref()).ok();
    let table: Vec<(u8, u8)> = slots.iter().enumerate().map(|(i, &p)| (p, i as u8)).collect();
    play(a, g, Session::new(slots[0], &table, delay), tr, true, early)
}

#[cfg(not(target_arch = "wasm32"))]
/// `scbw netjoin <code> [delay]`: join a host (its delay wins) and play the slot it gives.
pub fn netjoin(files: crate::storage::Storage, code: &str, _delay: u32) -> std::io::Result<()> {
    let a = view::Assets::load(files)?;
    let mut tr = Transport::join(code, ANY, 0).map_err(std::io::Error::other)?;
    let (slot, delay, setup, early) = lobby_wait(&mut tr).map_err(std::io::Error::other)?;
    let (g, table) = decode_setup(&setup, slot).ok_or_else(|| std::io::Error::other("bad game setup"))?;
    play(a, g, Session::new(slot, &table, delay), tr, false, early)
}

/// Leaving a game announces itself: a client tells the host which turn it sent last (the host
/// fences it as soon as those arrived), the host tells every client the game is over. Not after
/// a finished headless game or when the host already left.
impl Drop for NetGame {
    fn drop(&mut self) {
        if self.finished.is_some() || self.over.is_some() {
            return;
        }
        if self.host {
            self.tr.broadcast_ctl(&[C_HOST_LEFT]);
        } else {
            let mut msg = vec![C_LEAVE_REQ];
            msg.extend_from_slice(&self.next_send.to_le_bytes());
            self.tr.send_ctl(HOST, msg);
        }
        // The browser cannot block: send once and let the closing link (web_transport.rs)
        // tell the host the rest.
        #[cfg(target_arch = "wasm32")]
        {
            self.tr.poll();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let t0 = Instant::now();
        #[cfg(not(target_arch = "wasm32"))]
        while t0.elapsed() < Duration::from_millis(1000) {
            self.tr.poll();
            if !self.tr.ctl_pending() && (self.host || self.tr.fully_acked(self.next_send)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// An enhanced game's joiner start (C_LATE): the frame `f` it starts at, the joiner (link id,
/// human, army), the delay, the players (link id, storm order, human), what the saved game
/// does not keep (the humans' armies, names, the AI's armies, the slot names), the saved
/// game of `snap` (zlib).
#[allow(clippy::too_many_arguments)]
fn late_bytes(scene: &mut Scene<'_>, snap: &crate::bw::State, f: u32, p: u8, h: u8, n: u8, delay: u32, players: &[(u8, u8, u8)]) -> Option<Vec<u8>> {
    use std::io::Write;
    let live = std::mem::replace(&mut scene.st, snap.clone());
    let save = scene.save_file(b"enhanced", true);
    scene.st = live;
    let save = save?;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&save).ok()?;
    let save = z.finish().ok()?;
    let mut b = Vec::new();
    b.extend_from_slice(&f.to_le_bytes());
    b.extend_from_slice(&[p, h, n]);
    b.extend_from_slice(&delay.to_le_bytes());
    b.push(players.len() as u8);
    for q in players {
        b.extend_from_slice(&[q.0, q.1, q.2]);
    }
    crate::bw::enhanced::Extras::of(snap).encode(&mut b);
    b.extend_from_slice(&(save.len() as u32).to_le_bytes());
    b.extend_from_slice(&save);
    Some(b)
}

/// The joiner of an enhanced game: its scene from the host's start (`late_bytes`, which came
/// through snet: `snet::late`) and its net game over the link it joined with, from that frame.
pub fn late_game<'a>(a: &'a view::Assets, b: &[u8]) -> std::io::Result<(Scene<'a>, NetGame)> {
    use std::io::Read;
    let bad = || std::io::Error::other("bad game start from the host");
    let mut i = In(b);
    let mut parse = || -> Option<_> {
        let f = i.u32()?;
        let (p, h, n) = (i.u8()?, i.u8()?, i.u8()?);
        let delay = i.u32()?;
        let mut players = Vec::new();
        for _ in 0..i.u8()? {
            players.push((i.u8()?, i.u8()?, i.u8()?));
        }
        let mut at = b.len() - i.0.len();
        let ex = crate::bw::enhanced::Extras::decode(b, &mut at)?;
        i = In(&b[at..]);
        let zl = i.u32()? as usize;
        let z = i.take(zl)?.to_vec();
        Some((f, p, h, n, delay, players, ex, z))
    };
    let (f, p, h, n, delay, players, ex, z) = parse().ok_or_else(bad)?;
    let mut save = Vec::new();
    flate2::read::ZlibDecoder::new(&z[..]).read_to_end(&mut save)?;
    if let Ok(path) = std::env::var("SCBW_LATE_DUMP") {
        let _ = std::fs::write(path, &save);
    }
    let mut scene = Scene::from_save_steps(a, &save, false)?;
    let st = &mut scene.st;
    ex.apply(st);
    st.local_player = n;
    st.local_human = h;
    scene.local = n;
    let (mut tr, _, _, early) = super::snet::take_transport(false).ok_or_else(|| std::io::Error::other("no link to the game"))?;
    tr.set_first(f);
    let table: Vec<(u8, u8)> = players.iter().map(|q| (q.0, q.1)).collect();
    let mut se = Session::new_at(p, &table, delay, f);
    se.map_humans(&players.iter().map(|q| (q.0, q.2)).collect::<Vec<_>>());
    se.add_player(p, p, h, f + super::JOIN_LAG);
    let mut net = NetGame::new(se, tr, false, early).map_err(std::io::Error::other)?;
    // The replay of a game joined late would lack its start.
    net.se.record = false;
    // The link counts my turns from the join frame: the ones before my first are empty.
    for fr in f..f + delay {
        net.tr.send_turns(fr, Vec::new());
    }
    eprintln!("net: joined the running game at frame {f} as human {h}, army {n}");
    Ok((scene, net))
}
