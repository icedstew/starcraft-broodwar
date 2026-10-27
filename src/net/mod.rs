//! The rollback session (agent/areas/net.md, phase 2): one peer's engine, transport
//! agnostic. After ShieldBattery's rollback netcode (MIT, Travis Collins; docs/rollback.md,
//! game/src/netcode_v2/input_table.rs, game/src/rollback/tick.rs).
//!
//! Turn index = frame: a player's turn for frame f holds the commands run at frame counter
//! f, before that frame's step. A peer sends its turn for frame f when it is at frame
//! f - delay. A missing remote turn is predicted empty; a late non-empty one rolls back to
//! the state before its frame and plays forward again.

use crate::{bw, replay, view};

pub mod live;
#[cfg(not(target_arch = "wasm32"))]
pub mod netrep;
pub mod snet;
pub mod statstr;
pub mod transport;
#[cfg(target_arch = "wasm32")]
pub mod web_transport;

/// Frames a peer may simulate past the first frame with a missing turn.
pub const PRED_LIMIT: u32 = 8;

pub struct Session {
    /// This peer's map player.
    pub me: u8,
    /// (map player, storm id), highest storm id first: 0x486f50's dispatch order.
    players: Vec<(u8, u8)>,
    pub delay: u32,
    /// Per player (as `players`), the turn of each frame; frames < delay are known empty.
    table: Vec<Vec<Option<Vec<u8>>>>,
    /// The next frame to simulate.
    pub frame: u32,
    /// The first frame some player's turn is still missing.
    pub confirmed: u32,
    /// Do not simulate frames from here on (tests: the replay's end).
    pub end: u32,
    /// snaps[f % (PRED_LIMIT + 1)]: the state before frame f's commands.
    snaps: Vec<Option<bw::State>>,
    /// The lowest frame that got a late non-empty turn.
    rollback: Option<u32>,
    pub rollbacks: u32,
    pub resimulated: u32,
    pub stalled: u32,
    /// The sounds the last first-run step returned.
    pub sounds: Vec<(u16, i32, i32)>,
    /// Per player (as `players`): `(last, fence)` once it left: its turns below `last` are
    /// real, `last..fence` empty, `fence` the leave command, after it empty (`apply_leave`).
    gone: Vec<Option<(u32, u32)>>,
    /// Record confirmed frames into the replay recorder (live games); `recorded` = the next
    /// frame to record.
    pub record: bool,
    recorded: u32,
    /// Chat lines (0x5c commands) to record at the frame they arrived in, before its turns.
    chat_rec: Vec<(u32, Vec<u8>)>,
    /// The next multiple of 8 whose state hash is due.
    hash_next: u32,
    /// Test: SCBW_NET_DEBUG_DESYNC=<f>: skip the first non-empty frame at or after f.
    debug_desync: Option<u32>,
    skipped: Option<u32>,
}

impl Session {
    /// `players`: (map player, storm id) of every player in the game.
    pub fn new(me: u8, players: &[(u8, u8)], delay: u32) -> Session {
        let mut players = players.to_vec();
        players.sort_by_key(|p| std::cmp::Reverse(p.1));
        let gone = vec![None; players.len()];
        let table = players.iter().map(|_| (0..delay).map(|_| Some(Vec::new())).collect()).collect();
        Session {
            me,
            players,
            delay: delay.max(1),
            table,
            frame: 0,
            confirmed: 0,
            end: u32::MAX,
            snaps: vec![None; PRED_LIMIT as usize + 1],
            rollback: None,
            rollbacks: 0,
            resimulated: 0,
            stalled: 0,
            sounds: Vec::new(),
            gone,
            record: false,
            recorded: 0,
            chat_rec: Vec::new(),
            hash_next: 8,
            debug_desync: std::env::var("SCBW_NET_DEBUG_DESYNC").ok().and_then(|v| v.parse().ok()),
            skipped: None,
        }
    }

    fn index(&self, player: u8) -> Option<usize> {
        self.players.iter().position(|p| p.0 == player)
    }

    /// Is player `i`'s turn at frame `f` known (stored, or past a leave's fence)?
    fn known(&self, i: usize, f: u32) -> bool {
        self.table[i].get(f as usize).is_some_and(|x| x.is_some()) || self.gone[i].is_some_and(|(_, fence)| f > fence)
    }

    fn advance_confirmed(&mut self) {
        while (0..self.table.len()).all(|i| self.known(i, self.confirmed)) {
            self.confirmed += 1;
        }
    }

    fn store(&mut self, i: usize, frame: u32, bytes: Vec<u8>) -> bool {
        if self.gone[i].is_some_and(|(last, _)| frame >= last) {
            return false; // the fence replaced everything from `last` on
        }
        let t = &mut self.table[i];
        if t.len() <= frame as usize {
            t.resize(frame as usize + 1, None);
        }
        let slot = &mut t[frame as usize];
        if slot.is_some() {
            return false;
        }
        *slot = Some(bytes);
        self.advance_confirmed();
        true
    }

    /// The first frame of `player` whose turn is missing (every turn below it is stored).
    pub fn contiguous(&self, player: u8) -> u32 {
        let Some(i) = self.index(player) else { return 0 };
        self.table[i].iter().position(|x| x.is_none()).unwrap_or(self.table[i].len()) as u32
    }

    /// The frame a leave must be fenced at when the leaver's turns below `last` are all that
    /// exist: no peer can have simulated it. A peer's `frame` is at most `confirmed + PRED_LIMIT
    /// + 1`, its `confirmed` at most `last` (it only gets the leaver's turns through the host,
    /// which holds exactly the turns below `last` contiguously).
    pub fn fence_for(last: u32) -> u32 {
        last + PRED_LIMIT + 1
    }

    /// The players whose missing turn holds the game once the prediction limit is reached
    /// (their turn at `confirmed` is not known); empty while frames can still be simulated.
    pub fn waiting_on(&self) -> Vec<u8> {
        if self.frame <= self.confirmed + PRED_LIMIT || self.frame >= self.end {
            return Vec::new();
        }
        (0..self.players.len()).filter(|&i| !self.known(i, self.confirmed)).map(|i| self.players[i].0).collect()
    }

    pub fn is_gone(&self, player: u8) -> bool {
        self.index(player).is_some_and(|i| self.gone[i].is_some())
    }

    /// `player` leaves: its turns below `last` stay, `last..fence` become empty, the turn at
    /// `fence` the leave command (0x57, `reason`: 1 quit, 6 dropped), every later one empty.
    /// `fence` must not be simulated yet (the host picks `fence_for(last)`); a violation is
    /// logged and rolled back if the ring still has it.
    pub fn apply_leave(&mut self, player: u8, last: u32, fence: u32, reason: u8) {
        let Some(i) = self.index(player) else { return };
        if self.gone[i].is_some() || fence < last {
            return;
        }
        if fence < self.frame {
            eprintln!("net: FENCE VIOLATION: player {player} leaves at frame {fence}, already simulated to {}", self.frame);
        }
        self.gone[i] = Some((last, fence));
        let frame = self.frame;
        let t = &mut self.table[i];
        if t.len() <= fence as usize {
            t.resize(fence as usize + 1, None);
        }
        let mut rb = None;
        for f in last..=fence {
            let new = if f == fence { vec![0x57, reason] } else { Vec::new() };
            let old = t[f as usize].replace(new.clone());
            if f < frame && old.is_none_or(|o| o != new) && frame - f <= PRED_LIMIT + 1 {
                rb = Some(rb.map_or(f, |r: u32| r.min(f)));
            }
        }
        if let Some(f) = rb {
            self.rollback = Some(self.rollback.map_or(f, |r| r.min(f)));
        }
        self.advance_confirmed();
    }

    /// My turn, at the current frame: stored for frame `self.frame + delay`; the transport
    /// sends the returned (frame, bytes) to everyone.
    pub fn local_turn(&mut self, bytes: Vec<u8>) -> (u32, Vec<u8>) {
        let f = self.frame + self.delay;
        let i = self.index(self.me).expect("me is a player");
        self.store(i, f, bytes.clone());
        (f, bytes)
    }

    /// `player`'s turn for `frame` arrived (duplicates are ignored).
    pub fn remote_turn(&mut self, player: u8, frame: u32, bytes: Vec<u8>) {
        let Some(i) = self.index(player) else { return };
        let empty = bytes.is_empty();
        if self.store(i, frame, bytes) && frame < self.frame && !empty {
            self.rollback = Some(self.rollback.map_or(frame, |r| r.min(frame)));
        }
    }

    /// Frame `f`'s turns, run in 0x486f50's order (storm ids 7 down to 0); a missing turn
    /// is empty. Then the step. A net game records at confirmation (`record_confirmed`).
    fn run_frame(&mut self, scene: &mut view::Scene, f: u32) -> Vec<(u16, i32, i32)> {
        if self.record {
            crate::replay_rec::suppress(true);
        }
        // Test hook: one peer drops the first non-empty frame at or after the given one.
        let mut skip = self.skipped == Some(f);
        if let (Some(d), None) = (self.debug_desync, self.skipped) {
            if f >= d && self.table.iter().any(|t| t.get(f as usize).is_some_and(|x| x.as_ref().is_some_and(|b| !b.is_empty()))) {
                self.skipped = Some(f);
                skip = true;
                eprintln!("net: DEBUG_DESYNC: skipping the commands of frame {f}");
            }
        }
        if !skip {
            for (i, &(player, _)) in self.players.iter().enumerate() {
                if let Some(Some(turn)) = self.table[i].get(f as usize) {
                    for c in replay::split_commands(turn) {
                        scene.replay_command(player, c);
                    }
                }
            }
        }
        let r = scene.step();
        if self.record {
            crate::replay_rec::suppress(false);
        }
        r
    }

    /// A received chat line's 0x5c command (`chat::receive`): 0x485d10 records it at the
    /// frame counter 0x57f23c, the frame about to run.
    pub fn record_chat(&mut self, cmd: Vec<u8>) {
        if self.record {
            self.chat_rec.push((self.frame.max(self.recorded), cmd));
        }
    }

    /// The players in the game.
    pub fn players(&self) -> impl Iterator<Item = u8> + '_ {
        self.players.iter().map(|p| p.0)
    }

    /// Record the commands of every simulated, confirmed frame once (storm id order).
    fn record_confirmed(&mut self) {
        if !self.record {
            return;
        }
        let mut ids = [0u8; 12];
        for &(p, s) in &self.players {
            ids[p as usize] = s;
        }
        crate::replay_rec::set_storm_ids(ids);
        while self.recorded < self.confirmed.min(self.frame) {
            let f = self.recorded as usize;
            let me = self.me;
            self.chat_rec.retain(|(cf, c)| {
                if *cf as usize != f {
                    return true;
                }
                crate::replay_rec::record_at(*cf, me, c);
                false
            });
            for (i, &(player, _)) in self.players.iter().enumerate() {
                if let Some(Some(turn)) = self.table[i].get(f) {
                    for c in replay::split_commands(turn) {
                        crate::replay_rec::record_at(f as u32, player, c);
                    }
                }
            }
            self.recorded += 1;
        }
    }

    /// The (frame, hash) pairs now due: every multiple of 8 the confirmed frame has reached,
    /// hashed from the snapshot of the state before that frame (the live state when it is the
    /// next frame). Nothing while a rollback is pending (`tick` clears it first).
    pub fn due_hashes(&mut self, st: &bw::State) -> Vec<(u32, u64)> {
        let mut out = Vec::new();
        if self.rollback.is_some() {
            return out;
        }
        while self.hash_next <= self.confirmed && self.hash_next <= self.frame {
            let m = self.hash_next;
            self.hash_next += 8;
            if m == self.frame {
                out.push((m, st.state_hash()));
            } else if let (Some(s), true) = (&self.snaps[(m % (PRED_LIMIT + 1)) as usize], self.frame - m <= PRED_LIMIT + 1) {
                out.push((m, s.state_hash()));
            } else {
                eprintln!("net: no snapshot for the hash of frame {m}");
            }
        }
        out
    }

    /// One game frame of wall time: a pending rollback is played first, then (unless the
    /// prediction limit stalls it) the next frame. Returns whether it stepped.
    pub fn tick(&mut self, scene: &mut view::Scene) -> bool {
        let stepped = self.tick_inner(scene);
        self.record_confirmed();
        stepped
    }

    fn tick_inner(&mut self, scene: &mut view::Scene) -> bool {
        let ring = PRED_LIMIT + 1;
        if let Some(g) = self.rollback.take() {
            let snap = self.snaps[(g % ring) as usize].take().expect("snapshot for the rollback");
            scene.st.restore_sim(&snap);
            self.snaps[(g % ring) as usize] = Some(snap);
            self.rollbacks += 1;
            // ponytail: what the re-simulated frames make for the player is dropped, the
            // State queues (out_lens) and the sounds. Scene-side per-frame effects still run
            // twice (voices.tick, minimap pings, show_error); upgrade path: SB matches the
            // re-run's effects against the first run's.
            let outs = scene.st.out_lens();
            for h in g..self.frame {
                if h > g {
                    self.snaps[(h % ring) as usize] = Some(scene.st.clone());
                }
                self.run_frame(scene, h);
                self.resimulated += 1;
            }
            scene.st.truncate_outs(&outs);
        }
        if self.frame >= self.end {
            return false;
        }
        if self.frame > self.confirmed + PRED_LIMIT {
            self.stalled += 1;
            return false;
        }
        self.snaps[(self.frame % ring) as usize] = Some(scene.st.clone());
        self.sounds = self.run_frame(scene, self.frame);
        self.frame += 1;
        true
    }
}

/// What a replay-driven net test needs: the players, each one's recorded turn per frame
/// and the straight playback's final hash.
pub struct Plan {
    pub end: u32,
    /// (map player, storm id) of every player.
    pub players: Vec<(u8, u8)>,
    /// Per player (as `players`), the commands of each frame, concatenated.
    pub turns: Vec<Vec<Vec<u8>>>,
    /// Commands of owners that are not players, or past the end.
    pub dropped: usize,
    /// The state before frame 0.
    pub start: bw::State,
    /// The straight playback's final state hash.
    pub want: u64,
}

/// Build the `Plan`; `Err` is an INVALID message (a command before `delay`).
pub fn plan(a: &view::Assets, r: &replay::Replay, delay: u32) -> std::io::Result<Result<Plan, String>> {
    let end = r.header.frames;
    let players: Vec<(u8, u8)> =
        r.header.slots.iter().filter(|s| s.kind == 1 || s.kind == 2).map(|s| (s.slot_id as u8, s.player_id)).collect();
    // Each player's turn per frame: its commands, concatenated.
    let mut turns: Vec<Vec<Vec<u8>>> = vec![vec![Vec::new(); end as usize]; players.len()];
    let mut dropped = 0;
    for c in &r.commands {
        let (Some(i), true) = (players.iter().position(|p| p.0 == c.owner), c.frame < end) else {
            dropped += 1;
            continue;
        };
        if c.frame < delay {
            return Ok(Err(format!("INVALID: a command of player {} at frame {} < delay {delay}", c.owner, c.frame)));
        }
        turns[i][c.frame as usize].extend_from_slice(&c.data);
    }
    let mut straight = view::Scene::from_replay(a, r)?;
    let start = straight.st.clone();
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); end as usize];
    for (i, c) in r.commands.iter().enumerate() {
        if let Some(v) = at.get_mut(c.frame as usize) {
            v.push(i);
        }
    }
    for f in 0..end {
        for &i in &at[f as usize] {
            straight.replay_command(r.commands[i].owner, &r.commands[i].data);
        }
        straight.step();
    }
    let want = straight.st.state_hash();
    Ok(Ok(Plan { end, players, turns, dropped, start, want }))
}

/// `scbw nettest`: one `Session` per replay player in one process, each with the others'
/// turns arriving over a simulated link (`latency` ticks plus a pseudo-random 0..=jitter);
/// every final state must equal a straight playback's. Returns the result lines.
pub fn nettest(a: &view::Assets, r: &replay::Replay, delay: u32, latency: u32, jitter: u32) -> std::io::Result<String> {
    let Plan { end, players, turns, dropped, start, want } = match plan(a, r, delay)? {
        Ok(p) => p,
        Err(m) => return Ok(m),
    };
    let mut scenes = Vec::new();
    let mut sessions = Vec::new();
    for &(p, _) in &players {
        let mut s = view::Scene::from_replay(a, r)?;
        s.st = start.clone();
        s.local = p;
        s.st.local_player = p;
        scenes.push(s);
        let mut se = Session::new(p, &players, delay);
        se.end = end;
        sessions.push(se);
    }
    let n = players.len();
    let mut next_send = vec![delay.max(1); n];
    // In flight: (arrival tick, to, from player, frame, bytes).
    let mut link: Vec<(u32, usize, u8, u32, Vec<u8>)> = Vec::new();
    let mut rng = 0x2545f491u32;
    let mut now = 0u32;
    let done = |ss: &[Session]| ss.iter().all(|s| s.frame >= end && s.confirmed >= end);
    while !done(&sessions) {
        for i in 0..n {
            let s = &mut sessions[i];
            if s.frame + s.delay == next_send[i] && s.frame < end {
                next_send[i] += 1;
                let bytes = turns[i].get(s.frame as usize + s.delay as usize).cloned().unwrap_or_default();
                let (f, b) = s.local_turn(bytes);
                for j in (0..n).filter(|&j| j != i) {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    let extra = if jitter == 0 { 0 } else { rng % (jitter + 1) };
                    link.push((now + latency + extra, j, players[i].0, f, b.clone()));
                }
            }
        }
        // Deliver what is due (in flight order: jitter reorders per link).
        let mut k = 0;
        while k < link.len() {
            if link[k].0 <= now {
                let (_, to, from, f, b) = link.remove(k);
                sessions[to].remote_turn(from, f, b);
            } else {
                k += 1;
            }
        }
        for i in 0..n {
            sessions[i].tick(&mut scenes[i]);
        }
        now += 1;
        if now > end * 40 + 1000 {
            return Ok(format!("STUCK at tick {now}: frames {:?}", sessions.iter().map(|s| (s.frame, s.confirmed)).collect::<Vec<_>>()));
        }
    }
    let mut ok = true;
    let mut out = String::new();
    for (i, s) in sessions.iter().enumerate() {
        let h = scenes[i].st.state_hash();
        ok &= h == want;
        out += &format!(
            "\n  player {}: rollbacks {}, re-simulated {}, stalled ticks {}, hash {h:016x}",
            s.me, s.rollbacks, s.resimulated, s.stalled
        );
    }
    Ok(format!(
        "{} f{end} delay {delay} latency {latency} jitter {jitter}, {n} players, {now} ticks ({dropped} commands without a session player), straight hash {want:016x}{out}",
        if ok { "MATCH" } else { "DIFF" }
    ))
}
