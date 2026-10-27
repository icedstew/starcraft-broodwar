//! Phase 3b (agent/areas/net.md): a live networked game. The host builds a melee
//! `GameSetup` on a map, sends it with Start on the lobby stream, and every peer plays the
//! same game through the rollback `Session` inside `view::frame_loop_end` (`NetGame` is the
//! `view::NetDriver` it drives). `scbw nethost <map> [port] [players] [delay]` and
//! `scbw netjoin <code> [delay]`.
//!
//! SCBW_NET_HEADLESS=<K> runs without a window for K frames and prints
//! `frame K hash <state_hash>`; SCBW_NET_SCRIPT="<frame>:<hex command>;..." injects commands
//! into this peer's local turn at that frame (replay-format bytes; `selw` stands for the
//! select command of the first worker of the local player). SCBW_NET_RACES="t,z,p,r" picks
//! the host's slot races (default: random by the seed), SCBW_NET_BIND the host address.

#[cfg(not(target_arch = "wasm32"))]
use super::transport::ANY;
use super::transport::{Event, HOST, Transport};
use super::Session;
#[cfg(not(target_arch = "wasm32"))]
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

/// The wire form of a `GameSetup` (not the map archive, campaign directory or a save).
pub fn encode_setup(g: &GameSetup) -> Vec<u8> {
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
    o.0.push(g.camp_cont as u8 | (g.expansion as u8) << 1 | (g.multiplayer as u8) << 2);
    o.0
}

/// `encode_setup`'s inverse; `local` is the receiving peer's slot.
pub fn decode_setup(b: &[u8], local: u8) -> Option<GameSetup> {
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
    Some(GameSetup {
        chk,
        mpq: None,
        map_dir: None,
        types,
        races,
        names,
        forces,
        colors,
        template,
        local,
        seed,
        objectives,
        save: None,
        camp_cont: fl & 1 != 0,
        mission,
        expansion: fl & 2 != 0,
        multiplayer: fl & 4 != 0,
    })
}

#[cfg(not(target_arch = "wasm32"))]
/// The scenario.chk of a map argument (.scm / .scx archive, bare .chk, or a replay's).
fn load_chk(path: &str) -> std::io::Result<Vec<u8>> {
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
/// (local = the first human) and the human slots.
pub fn melee_setup(a: &view::Assets, chk_raw: Vec<u8>, humans: usize, seed: u32) -> Result<(GameSetup, Vec<u8>), String> {
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
        seed,
        objectives: [0; 8],
        save: None,
        camp_cont: false,
        mission: 0,
        expansion: true,
        multiplayer: true,
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

/// One scripted injection: (frame, bytes), or `selw`.
enum Cmd {
    Raw(Vec<u8>),
    SelectWorker,
}

fn parse_script(s: &str) -> Result<Vec<(u32, Cmd)>, String> {
    let mut v = Vec::new();
    for item in s.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (f, c) = item.split_once(':').ok_or_else(|| format!("bad script item {item:?}"))?;
        let f: u32 = f.trim().parse().map_err(|_| format!("bad frame in {item:?}"))?;
        let c = c.trim();
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
}

/// Control messages (first byte; see the module doc).
const C_LEAVE: u8 = 1;
const C_LEAVE_REQ: u8 = 2;
const C_HASH: u8 = 3;
const C_DESYNC: u8 = 4;
const C_HOST_LEFT: u8 = 5;

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
        let next_send = se.delay;
        Ok(NetGame::make(se, tr, next_send, script, headless, host))
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
            _ => {}
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
        NetGame { se, tr, next_send, script, headless, finished: None, host, verbose: std::env::var_os("NETREP_DEBUG").is_some(), last_report: Instant::now(), report: None, reported: false, hashes: HashMap::new(), desync: None, over: None, quitting: Vec::new(), replay_path: std::env::var("SCBW_NET_REPLAY").ok(), quit_at: std::env::var("SCBW_NET_QUIT_AT").ok().and_then(|v| v.parse().ok()) }
    }
}

impl NetDriver for NetGame {
    fn headless(&self) -> bool {
        self.headless.is_some()
    }

    fn frame(&mut self, scene: &mut Scene<'_>) -> Option<Vec<(u16, i32, i32)>> {
        if self.se.frame == 0 {
            crate::replay_rec::set_net_header(scene.st.game_seed, 2, 1);
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
                    Cmd::SelectWorker => select_worker(&scene.st, self.se.me),
                };
                scene.st.send_command(&b);
            }
            let bytes = std::mem::take(&mut scene.st.turn_out);
            let (f, b) = self.se.local_turn(bytes);
            self.tr.send_turns(f, b);
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
                    eprintln!("net: {m}");
                    // A client that already holds every turn up to its end (a test's frame
                    // count) plays them out: the host may close as soon as its turns are acked.
                    if !self.host && self.se.confirmed < self.se.end {
                        self.over = Some(format!("lost the host ({m})"));
                    }
                }
                Event::Start { .. } | Event::Joined { .. } => {}
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
        if self.verbose && self.last_report.elapsed() > Duration::from_secs(2) {
            self.last_report = Instant::now();
            let se = &self.se;
            eprintln!("net player {}: frame {} confirmed {} stalled {} rollbacks {} dgrams {:?}", se.me, se.frame, se.confirmed, se.stalled, se.rollbacks, self.tr.stats);
        }
        let stepped = self.se.tick(scene);
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

    fn pump_headless(&mut self, scene: &mut Scene<'_>) -> bool {
        let k = self.headless.unwrap_or(0);
        let nap = || std::thread::sleep(Duration::from_millis(1));
        let Some(t0) = self.finished else {
            if self.frame(scene).is_none() {
                nap();
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
    let (mut g, slots) = melee_setup(&a, load_chk(map)?, players.clamp(1, 8), seed).map_err(std::io::Error::other)?;
    let (tr, code) = Transport::host(port, slots[0], &slots[1..], delay, encode_setup(&g)).map_err(std::io::Error::other)?;
    println!("LISTEN {code}");
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut tr = tr;
    let (_, _, _, early) = lobby_wait(&mut tr).map_err(std::io::Error::other)?;
    g.local = slots[0];
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
    let g = decode_setup(&setup, slot).ok_or_else(|| std::io::Error::other("bad game setup"))?;
    let table: Vec<(u8, u8)> = (0..8u8).filter(|&p| g.types[p as usize] == 2).enumerate().map(|(i, p)| (p, i as u8)).collect();
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
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(1000) {
            self.tr.poll();
            if !self.tr.ctl_pending() && (self.host || self.tr.fully_acked(self.next_send)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
