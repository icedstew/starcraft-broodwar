//! Storm's SNet calls as the 1.16.1 glue and game make them, over our own networking
//! (agent/areas/net.md, phase 3c). Storm.dll exports them by ordinal only; each function
//! notes the ordinal and the call sites it was read from. One provider exists: 'UDPN'
//! ("Local Area Network (UDP)"), backed by the QUIC transport (transport.rs); Battle.net,
//! IPX, modem and direct cable are not offered.

use super::transport::{self, Event, Transport};
use std::cell::RefCell;
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::net::UdpSocket;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, Mutex};
use web_time::Instant;

/// 'UDPN': the provider id 0x4bade0 / 0x4d3cc0 receive (EBX).
pub const UDPN: u32 = 0x5544_504e;

/// A provider as Ordinal 105's callback (0x4ba610) gets it: `id`, its name and description
/// (both <= 0x80 bytes) and its caps (+4 flags, +0x10 the most players).
pub struct Provider {
    pub id: u32,
    pub name: &'static str,
    pub desc: &'static str,
    pub flags: u32,
    pub max_players: u32,
}

/// Storm's provider caps (Ordinal 114 fills the 0x24-byte struct 0x4d3cc0 reads).
#[derive(Clone, Copy)]
pub struct Caps {
    /// +0x08: the most bytes of a turn (0x57f0d8 = min(this, 0x200); < 0x80 is refused).
    pub max_message: u32,
    /// +0x1c: turns per second (0x4d92a0's L).
    pub turns_per_sec: u32,
    /// +0x20: turns in flight (0x57f090, clamped to 2..=8).
    pub turns_in_flight: u32,
}

const UDPN_CAPS: Caps = Caps { max_message: 0x200, turns_per_sec: 8, turns_in_flight: 2 };

/// A game Ordinal 133 (SNetEnumGames) lists, as its callback 0x4d35e0 reads it: +4 the id,
/// +8 the name, +0xc the stat string (0x472570), +0x14 / +0x18 the players in it and the most.
#[derive(Clone, Debug)]
pub struct GameEntry {
    pub id: u32,
    pub name: Vec<u8>,
    pub stat: Vec<u8>,
    pub players: u8,
    pub max: u8,
    /// Ours: how to reach it (`ip:port#certhash`, transport.rs).
    pub code: String,
    seen: Instant,
}

/// What Storm tells the game's event handlers (Ordinal 123 registrations of 0x4d3cc0).
#[derive(Debug)]
pub enum NetEvent {
    /// Type 2 (0x4c4980): `id` joined the game.
    Joined(u8),
    /// Type 3 (0x4c3c90): `id` left (`reason`: 0x40000004 dropped, else quit).
    Left(u8, u32),
}

/// The game this side created or joined.
struct Game {
    tr: Transport,
    host: bool,
    /// Storm messages (Ordinal 127) received: (from, bytes).
    inbox: VecDeque<(u8, Vec<u8>)>,
    events: VecDeque<NetEvent>,
    /// The host's LAN answer (players change as they join).
    #[cfg(not(target_arch = "wasm32"))]
    advert: Option<Arc<Mutex<Advert>>>,
    code: String,
    /// Ordinal 113's names by Storm id (the host's table, sent to every client).
    names: [Option<Vec<u8>>; 8],
    /// Host: each player's turns not yet in a round, and the players a round waits for.
    queues: [VecDeque<Vec<u8>>; 8],
    in_turns: Vec<u8>,
    /// Rounds of turns Ordinal 122 has not returned yet.
    rounds: VecDeque<Round>,
    /// Ordinal 115: my turns sent and not yet back in a round.
    in_flight: u32,
    /// Ordinal 130's game mode (bit 2: full / private, the LAN list hides it).
    mode: u32,
    /// The game's name and stat string (host: its own; a joiner: from kind::GAME).
    info: Option<(Vec<u8>, Vec<u8>)>,
    /// Game turns that came before the game took the link (a peer started first).
    early: Vec<(u8, u32, Vec<u8>)>,
    /// An enhanced game already running: the host's state for us, in parts (live.rs C_LATE).
    late: Vec<Option<Vec<u8>>>,
}

/// live.rs's C_LATE control message: `idx u16`, `count u16`, a part of the joiner's start.
pub const LATE: u8 = 8;
/// live.rs's C_REFUSED (ours, campaign matches): the join is refused; the reason follows.
pub const REFUSED: u8 = 9;

thread_local! {
    /// Why the host refused this side's join (C_REFUSED, or a refusal before the Start).
    static REFUSAL: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The reason a join was refused, once (the front end shows it).
pub fn refused() -> Option<String> {
    REFUSAL.with(|r| r.borrow_mut().take())
}

fn set_refused(why: &str) {
    eprintln!("net: {}{why}", transport::REFUSED);
    REFUSAL.with(|r| *r.borrow_mut() = Some(why.to_string()));
}

/// One turn of every player (Ordinal 122): flags (0x10000 in the game, 0x20000 with a turn)
/// and the bytes.
pub type Round = [(u32, Vec<u8>); 8];

/// Control message kinds over the transport (the first byte; from 0x80: the game's own,
/// live.rs, start at 1 on the same link once the lobby hands it over).
mod kind {
    /// A Storm message: client -> host `to` + bytes, host -> client `from` + bytes.
    pub const MSG: u8 = 0x80;
    /// A client's turn (client -> host).
    pub const TURN: u8 = 0x81;
    /// A round (host -> client): `n u8`, n x (`player u8`, `flags u32`, `len u16`, bytes).
    pub const ROUND: u8 = 0x82;
    /// A client's player name (client -> host, once after Start).
    pub const NAME: u8 = 0x83;
    /// The host's name table (host -> client): n x (`id u8`, `len u8`, bytes).
    pub const NAMES: u8 = 0x84;
    /// A player left (host -> client): `id u8`, `reason u32`.
    pub const LEFT: u8 = 0x85;
    /// The game (host -> a joiner, with its first NAMES): its name and stat string, each
    /// NUL-terminated (what the LAN list carries; a join by code has no list).
    pub const GAME: u8 = 0x86;
}

impl Game {
    fn new(tr: Transport, host: bool, code: String) -> Game {
        Game {
            tr,
            host,
            inbox: VecDeque::new(),
            events: VecDeque::new(),
            #[cfg(not(target_arch = "wasm32"))]
            advert: None,
            code,
            names: Default::default(),
            queues: Default::default(),
            in_turns: Vec::new(),
            rounds: VecDeque::new(),
            in_flight: 0,
            mode: 0,
            info: None,
            early: Vec::new(),
            late: Vec::new(),
        }
    }

    fn names_msg(&self) -> Vec<u8> {
        let mut m = vec![kind::NAMES];
        for (i, n) in self.names.iter().enumerate() {
            if let Some(n) = n {
                m.extend_from_slice(&[i as u8, n.len() as u8]);
                m.extend_from_slice(n);
            }
        }
        m
    }

    /// Host: rounds while every player in the turns has one waiting.
    fn make_rounds(&mut self) {
        while !self.in_turns.is_empty() && self.in_turns.iter().all(|&p| !self.queues[p as usize].is_empty()) {
            let mut r: Round = Default::default();
            let mut m = vec![kind::ROUND, self.in_turns.len() as u8];
            for &p in &self.in_turns {
                let t = self.queues[p as usize].pop_front().unwrap();
                m.push(p);
                m.extend_from_slice(&0x30000u32.to_le_bytes());
                m.extend_from_slice(&(t.len() as u16).to_le_bytes());
                m.extend_from_slice(&t);
                r[p as usize] = (0x30000, t);
            }
            for p in self.tr.peers().to_vec() {
                if self.in_turns.contains(&p) {
                    self.tr.send_ctl(p, m.clone());
                }
            }
            self.rounds.push_back(r);
        }
    }

    /// Host: `p` is gone (its link closed or it was dropped).
    fn drop_player(&mut self, p: u8, reason: u32) {
        if (p as usize) >= 8 {
            return;
        }
        let was = self.in_turns.contains(&p) || self.names[p as usize].is_some();
        self.in_turns.retain(|&x| x != p);
        self.queues[p as usize].clear();
        self.names[p as usize] = None;
        if !was {
            return;
        }
        self.events.push_back(NetEvent::Left(p, reason));
        let mut m = vec![kind::LEFT, p];
        m.extend_from_slice(&reason.to_le_bytes());
        // The dropped player gets it too: for it, everyone else is gone.
        for q in self.tr.peers().to_vec() {
            self.tr.send_ctl(q, m.clone());
        }
        self.make_rounds();
    }
}

/// A round's bytes (kind::ROUND without the kind).
fn parse_round(b: &[u8]) -> Option<Round> {
    let mut r: Round = Default::default();
    let n = *b.first()? as usize;
    let mut at = 1;
    for _ in 0..n {
        let p = *b.get(at)? as usize;
        let flags = u32::from_le_bytes(b.get(at + 1..at + 5)?.try_into().ok()?);
        let len = u16::from_le_bytes(b.get(at + 5..at + 7)?.try_into().ok()?) as usize;
        let t = b.get(at + 7..at + 7 + len)?.to_vec();
        at += 7 + len;
        *r.get_mut(p)? = (flags, t);
    }
    Some(r)
}

#[derive(Default)]
struct Net {
    /// The initialized provider (0 none).
    provider: u32,
    game: Option<Game>,
    /// LAN games heard of (Ordinal 133), ours: id -> entry.
    games: Vec<GameEntry>,
    next_id: u32,
    #[cfg(not(target_arch = "wasm32"))]
    disc: Option<UdpSocket>,
}

thread_local! {
    static NET: RefCell<Net> = RefCell::new(Net::default());
}

/// Ordinal 105 (0x4bab10 with the callback 0x4ba610): the providers to list.
pub fn enum_providers() -> Vec<Provider> {
    vec![Provider { id: UDPN, name: "Local Area Network (UDP)", desc: "", flags: 0, max_players: 8 }]
}

/// Ordinal 117 (0x4d3cc0): make `id` the provider; false for one we do not have.
pub fn initialize_provider(id: u32) -> bool {
    if !enum_providers().iter().any(|p| p.id == id) {
        return false;
    }
    NET.with(|n| n.borrow_mut().provider = id);
    true
}

/// Ordinal 114: the caps of the initialized provider.
pub fn provider_caps() -> Option<Caps> {
    NET.with(|n| (n.borrow().provider == UDPN).then_some(UDPN_CAPS))
}

/// Ordinal 102 (0x4bab10, 0x4d3cc0's failure path): leave any game and drop the provider.
pub fn destroy() {
    #[cfg(not(target_arch = "wasm32"))]
    KEPT_ADVERT.with(|k| *k.borrow_mut() = None);
    NET.with(|n| *n.borrow_mut() = Net::default());
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    /// The LAN advert of a running enhanced game (dropping it ends the answering thread).
    static KEPT_ADVERT: RefCell<Option<Arc<Mutex<Advert>>>> = const { RefCell::new(None) };
}

/// Storm ids of a game's players: the creator is 0, joiners get 1.. in join order.
const JOIN_IDS: [u8; 7] = [1, 2, 3, 4, 5, 6, 7];

/// LAN discovery (ours, standing in for the UDP provider's broadcasts): a joiner sends
/// `SCBWQ1` to UDP port 6112 (SCBW_LAN_PORT) as a broadcast and to localhost; a host answers
/// `SCBWG1`, `port u16`, the certificate hash (32 bytes), `players u8`, `max u8`, the name and
/// the stat string, each NUL-terminated. The joiner reaches it at the answer's source address.
const QUERY: &[u8] = b"SCBWQ1";
const ANSWER: &[u8] = b"SCBWG1";

#[cfg(not(target_arch = "wasm32"))]
fn lan_port() -> u16 {
    std::env::var("SCBW_LAN_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(6112)
}

/// The host's answer, kept current by the game thread.
#[cfg(not(target_arch = "wasm32"))]
struct Advert {
    port: u16,
    hash: [u8; 32],
    players: u8,
    max: u8,
    /// Ordinal 130's bit 2: not listed.
    full: bool,
    name: Vec<u8>,
    stat: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Advert {
    fn bytes(&self) -> Vec<u8> {
        let mut b = ANSWER.to_vec();
        b.extend_from_slice(&self.port.to_le_bytes());
        b.extend_from_slice(&self.hash);
        b.extend_from_slice(&[self.players, self.max]);
        b.extend_from_slice(&self.name);
        b.push(0);
        b.extend_from_slice(&self.stat);
        b.push(0);
        b
    }
}

/// Answer queries on the LAN port while the game is advertised (a thread; it ends when the
/// advert is dropped). One host per machine (the port is not shared).
#[cfg(not(target_arch = "wasm32"))]
fn advertise(advert: &Arc<Mutex<Advert>>) {
    let Ok(sock) = UdpSocket::bind(("0.0.0.0", lan_port())) else {
        eprintln!("net: LAN port {} busy: the game is not listed on the LAN (the join code works)", lan_port());
        return;
    };
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(500)));
    let weak = Arc::downgrade(advert);
    std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        while let Some(a) = weak.upgrade() {
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                if &buf[..n] == QUERY && !a.lock().unwrap().full {
                    let reply = a.lock().unwrap().bytes();
                    let _ = sock.send_to(&reply, from);
                }
            }
        }
    });
}

/// The `ip:port#hash` join code of `code` with its address replaced by `ip`.
fn with_ip(code: &str, ip: &str) -> String {
    let (addr, hash) = code.split_once('#').unwrap_or((code, ""));
    let port = addr.rsplit_once(':').map_or("", |p| p.1);
    format!("{ip}:{port}#{hash}")
}

/// Ordinal 138 (0x4d3910; SNetCreateGame's sibling with the game type and template):
/// create a game named `name` with the stat string `stat` for at most `max` players. The
/// creator's Storm id is 0 (0x51268c). Ours: a lobby host (transport.rs) bound to
/// SCBW_NET_BIND (default 0.0.0.0 here: a LAN game), advertised on the LAN.
#[cfg(not(target_arch = "wasm32"))]
pub fn create_game(name: &[u8], stat: &[u8], max: u8, player: &[u8]) -> Option<u8> {
    if std::env::var_os("SCBW_NET_BIND").is_none() {
        // SAFETY: the game thread sets it before the transport thread starts.
        unsafe { std::env::set_var("SCBW_NET_BIND", "0.0.0.0") };
    }
    let (tr, code) = match Transport::host_lobby(0, 0, &JOIN_IDS[..max.saturating_sub(1).min(7) as usize]) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("net: create game: {e}");
            return None;
        }
    };
    // The join code for SCBW_JOIN / a browser (ours: the LAN list needs none).
    println!("LOBBY {code}");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let (addr, hash) = code.split_once('#')?;
    let port: u16 = addr.rsplit_once(':')?.1.parse().ok()?;
    let advert = Arc::new(Mutex::new(Advert { port, hash: transport::unhex(hash).ok()?, players: 1, max, full: false, name: name.to_vec(), stat: stat.to_vec() }));
    advertise(&advert);
    let mut g = Game::new(tr, true, code);
    g.advert = Some(advert);
    g.info = Some((name.to_vec(), stat.to_vec()));
    g.names[0] = Some(player.to_vec());
    g.in_turns.push(0);
    NET.with(|n| n.borrow_mut().game = Some(g));
    Some(0)
}

/// Ours (campaign mode, `scbw campaign join <web map> <name>`): the join list shows the
/// faction's live matches from the campaign host instead of the LAN.
struct Campaign {
    url: String,
    name: String,
    asked: Option<Instant>,
    games: Vec<GameEntry>,
    /// Per game id: the stat line the join screen shows.
    lines: Vec<(u32, String)>,
}

thread_local! {
    static CAMPAIGN: RefCell<Option<Campaign>> = const { RefCell::new(None) };
}

/// Campaign mode: the web map at `url`, playing as `name`.
pub fn set_campaign(url: &str, name: &str) {
    CAMPAIGN.with(|c| *c.borrow_mut() = Some(Campaign { url: url.into(), name: name.into(), asked: None, games: Vec::new(), lines: Vec::new() }));
}

/// The campaign player's name, in campaign mode.
pub fn campaign_name() -> Option<String> {
    CAMPAIGN.with(|c| c.borrow().as_ref().map(|c| c.name.clone()))
}

/// The stat line of a listed campaign match.
pub fn campaign_line(id: u32) -> Option<String> {
    CAMPAIGN.with(|c| c.borrow().as_ref()?.lines.iter().find(|l| l.0 == id).map(|l| l.1.clone()))
}

/// The campaign's matches for the join list (asked again at most once a second); None
/// outside campaign mode.
#[cfg(not(target_arch = "wasm32"))]
fn campaign_games(n: &mut Net) -> Option<Vec<GameEntry>> {
    CAMPAIGN.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if c.asked.is_none_or(|t| t.elapsed().as_millis() >= 1000) {
            c.asked = Some(Instant::now());
            let enc: String = c.name.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02x}") }).collect();
            let text = crate::campaign::host::http_get(&format!("{}/matches?name={enc}", c.url)).unwrap_or_default();
            (c.games, c.lines) = (Vec::new(), Vec::new());
            for l in text.lines() {
                let Some((head, title)) = l.split_once('\t') else { continue };
                let f: Vec<&str> = head.splitn(8, ' ').collect();
                let [b, code, humans, frame, used, most, _workers, map] = f[..] else { continue };
                let num = |s: &str| s.parse::<u32>().unwrap_or(0);
                let id = num(b) + 1;
                // A melee description (statstr.rs) for the list to parse: 128x128, Fastest,
                // game type 2 / subtype 1, enhanced (+0x2f bit 7), host "Campaign", the map's name.
                let mut d = vec![0u8; 0x8d];
                d[0x20..0x24].copy_from_slice(&[128, 0, 128, 0]);
                (d[0x24], d[0x25], d[0x26], d[0x28], d[0x2a], d[0x2f]) = (num(humans).min(255) as u8, 8, 6, 2, 1, 0x80);
                d[0x34..0x3c].copy_from_slice(b"Campaign");
                let m = map.as_bytes();
                d[0x4d..0x4d + m.len().min(0x1f)].copy_from_slice(&m[..m.len().min(0x1f)]);
                let secs = num(frame) * 42 / 1000;
                c.lines.push((id, format!("{}:{:02}  supply {used}/{most}", secs / 60, secs % 60)));
                let e = GameEntry { id, name: title.as_bytes().to_vec(), stat: super::statstr::build(&d, 0x80), players: d[0x24], max: 8, code: code.to_string(), seen: Instant::now() };
                c.games.push(e);
            }
        }
        // Ordinal 118 finds the game by its id.
        n.games = c.games.clone();
        Some(c.games.clone())
    })
}

/// Ordinal 133 (0x4d37c0, the join screen every second): the LAN games heard of. Each call
/// asks again; answers arrive for the next call (as a LAN list fills). Games not heard of
/// for 3 s drop out.
pub fn enum_games() -> Vec<GameEntry> {
    NET.with(|n| {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(g) = campaign_games(&mut n.borrow_mut()) {
            return g;
        }
        let mut n = n.borrow_mut();
        #[cfg(not(target_arch = "wasm32"))]
        n.lan_poll();
        n.games.retain(|g| g.seen.elapsed().as_millis() < 3000);
        n.games.clone()
    })
}

impl Net {
    #[cfg(not(target_arch = "wasm32"))]
    fn lan_poll(&mut self) {
        if self.disc.is_none() {
            let Ok(s) = UdpSocket::bind(("0.0.0.0", 0)) else { return };
            let _ = s.set_broadcast(true);
            let _ = s.set_nonblocking(true);
            self.disc = Some(s);
        }
        let Some(sock) = self.disc.as_ref() else { return };
        let mut buf = [0u8; 600];
        let mut heard = Vec::new();
        while let Ok((len, from)) = sock.recv_from(&mut buf) {
            if let Some(g) = parse_answer(&buf[..len], &from.ip().to_string()) {
                heard.push(g);
            }
        }
        let port = lan_port();
        let _ = sock.send_to(QUERY, ("255.255.255.255", port));
        let _ = sock.send_to(QUERY, ("127.0.0.1", port));
        for mut g in heard {
            match self.games.iter_mut().find(|e| e.code == g.code) {
                Some(e) => {
                    g.id = e.id;
                    *e = g;
                }
                None => {
                    self.next_id += 1;
                    g.id = self.next_id;
                    self.games.push(g);
                }
            }
        }
    }
}

/// A host's `SCBWG1` answer from `ip`.
fn parse_answer(b: &[u8], ip: &str) -> Option<GameEntry> {
    let b = b.strip_prefix(ANSWER)?;
    let port = u16::from_le_bytes(b.get(0..2)?.try_into().ok()?);
    let hash = b.get(2..34)?;
    let (players, max) = (*b.get(34)?, *b.get(35)?);
    let mut rest = b.get(36..)?.split(|&c| c == 0);
    let name = rest.next()?.to_vec();
    let stat = rest.next()?.to_vec();
    let hex: String = hash.iter().map(|x| format!("{x:02x}")).collect();
    Some(GameEntry { id: 0, name, stat, players, max, code: format!("{ip}:{port}#{hex}"), seen: Instant::now() })
}

/// Ordinal 118 (0x4d3b50): join the listed game `id`; our join code entry joins by `code`
/// directly. The local Storm id once the host has answered (waits up to 5 s).
pub fn join_game(id: u32, code: Option<&str>, player: &[u8]) -> Option<u8> {
    let code = match code {
        Some(c) => c.to_string(),
        None => NET.with(|n| n.borrow().games.iter().find(|g| g.id == id).map(|g| g.code.clone()))?,
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut tr = Transport::join(&code, transport::ANY, 0).ok()?;
        let t0 = Instant::now();
        while t0.elapsed().as_secs() < 5 {
            for e in tr.poll() {
                match e {
                    Event::Start { slot, .. } => {
                        adopt(tr, slot, code, player);
                        return Some(slot);
                    }
                    Event::Error(m) => {
                        match m.strip_prefix(transport::REFUSED) {
                            Some(why) => set_refused(why),
                            None => eprintln!("net: join: {m}"),
                        }
                        return None;
                    }
                    _ => {}
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }
    #[cfg(target_arch = "wasm32")]
    {
        crate::todo_port!("snet::join_game", "the browser join (WebTransport) from the glue");
        let _ = (code, player);
        None
    }
}

/// A joined link (its Start came as `slot`): it becomes the game; our name goes to the host.
pub fn adopt(mut tr: Transport, slot: u8, code: String, player: &[u8]) {
    // Ours: an enhanced game's joiner may ask for an army (SCBW_ARMY / ?army=): after a NUL.
    let mut m = vec![kind::NAME];
    m.extend_from_slice(player);
    if let Some(a) = ARMY.with(|a| a.get()) {
        m.extend_from_slice(&[0, a]);
    }
    tr.send_ctl(transport::HOST, m);
    let mut g = Game::new(tr, false, code);
    g.names[slot as usize & 7] = Some(player.to_vec());
    NET.with(|n| n.borrow_mut().game = Some(g));
}

/// This side's Storm id in the game.
pub fn local_id() -> Option<u8> {
    NET.with(|n| n.borrow().game.as_ref().map(|g| g.tr.me()))
}

/// The running game's start for this joiner (an enhanced game), once all of it came.
pub fn late() -> Option<Vec<u8>> {
    NET.with(|n| {
        let n = n.borrow();
        let g = n.game.as_ref()?;
        if g.late.is_empty() || g.late.iter().any(|x| x.is_none()) {
            return None;
        }
        Some(g.late.iter().flatten().flatten().copied().collect())
    })
}

thread_local! {
    static ARMY: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// The army (slot) to ask for when joining a running enhanced game.
pub fn want_army(a: u8) {
    ARMY.with(|x| x.set(Some(a)));
}

/// The game's name and stat string (a joiner has them once the host answered its name).
pub fn game_info() -> Option<(Vec<u8>, Vec<u8>)> {
    NET.with(|n| n.borrow().game.as_ref()?.info.clone())
}

/// Ordinal 127: a message to Storm id `to` (Storm's 0xff = everyone else). Ours: the
/// transport's control messages, relayed by the host (clients only reach it).
pub fn send_message(to: u8, msg: &[u8]) {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut() else { return };
        let mut m = vec![kind::MSG, to];
        m.extend_from_slice(msg);
        if g.host {
            if to == 0xff {
                for p in g.tr.peers().to_vec() {
                    g.tr.send_ctl(p, framed(0, msg));
                }
            } else {
                g.tr.send_ctl(to, framed(0, msg));
            }
        } else {
            g.tr.send_ctl(transport::HOST, m);
        }
    });
}

/// A message as the host forwards it: `from u8` + the bytes.
fn framed(from: u8, msg: &[u8]) -> Vec<u8> {
    let mut m = vec![kind::MSG, from];
    m.extend_from_slice(msg);
    m
}

/// Drive the game's link: messages, turns and joins / leaves arrive (call often).
pub fn pump() {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut() else { return };
        for e in g.tr.poll() {
            match e {
                // A client counts as joined once its name came (Storm knows it at once).
                Event::Joined { .. } => {}
                Event::Lost { player, .. } => g.drop_player(player, 0x4000_0004),
                Event::Ctl { from, msg } if g.host => {
                    let Some((&k, body)) = msg.split_first() else { continue };
                    match k {
                        kind::MSG => {
                            // `to` + bytes; the host keeps or forwards it.
                            let Some((&to, body)) = body.split_first() else { continue };
                            if to == 0 || to == 0xff {
                                g.inbox.push_back((from, body.to_vec()));
                            }
                            for p in g.tr.peers().to_vec() {
                                if p != from && (to == 0xff || to == p) {
                                    g.tr.send_ctl(p, framed(from, body));
                                }
                            }
                        }
                        kind::TURN if g.in_turns.contains(&from) => {
                            g.queues[from as usize].push_back(body.to_vec());
                            g.make_rounds();
                        }
                        kind::NAME if (from as usize) < 8 && g.names[from as usize].is_none() => {
                            let end = body.iter().position(|&c| c == 0).unwrap_or(body.len()).min(0x18);
                            g.names[from as usize] = Some(body[..end].to_vec());
                            if let Some((name, stat)) = g.info.clone() {
                                let mut m = vec![kind::GAME];
                                m.extend_from_slice(&name);
                                m.push(0);
                                m.extend_from_slice(&stat);
                                m.push(0);
                                g.tr.send_ctl(from, m);
                            }
                            let m = g.names_msg();
                            for p in g.tr.peers().to_vec() {
                                g.tr.send_ctl(p, m.clone());
                            }
                            // Its turns count from the next round on.
                            g.in_turns.push(from);
                            g.events.push_back(NetEvent::Joined(from));
                        }
                        _ => {}
                    }
                }
                Event::Ctl { msg, .. } => {
                    let Some((&k, body)) = msg.split_first() else { continue };
                    match k {
                        kind::MSG => {
                            if let Some((&from, body)) = body.split_first() {
                                g.inbox.push_back((from, body.to_vec()));
                            }
                        }
                        kind::ROUND => {
                            if let Some(r) = parse_round(body) {
                                g.rounds.push_back(r);
                            }
                        }
                        kind::NAMES => {
                            let mut at = 0;
                            let mut names: [Option<Vec<u8>>; 8] = Default::default();
                            while let (Some(&id), Some(&len)) = (body.get(at), body.get(at + 1)) {
                                let Some(t) = body.get(at + 2..at + 2 + len as usize) else { break };
                                if let Some(x) = names.get_mut(id as usize) {
                                    *x = Some(t.to_vec());
                                }
                                at += 2 + len as usize;
                            }
                            for (i, x) in names.iter().enumerate() {
                                if x.is_some() && g.names[i].is_none() && i as u8 != g.tr.me() {
                                    g.events.push_back(NetEvent::Joined(i as u8));
                                }
                            }
                            g.names = names;
                        }
                        REFUSED => set_refused(&String::from_utf8_lossy(body)),
                        LATE if body.len() >= 4 => {
                            let (i, n) = (u16::from_le_bytes([body[0], body[1]]) as usize, u16::from_le_bytes([body[2], body[3]]) as usize);
                            if g.late.len() != n {
                                g.late = vec![None; n];
                            }
                            if let Some(x) = g.late.get_mut(i) {
                                *x = Some(body[4..].to_vec());
                            }
                        }
                        kind::GAME => {
                            let mut it = body.split(|&c| c == 0);
                            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                                g.info = Some((a.to_vec(), b.to_vec()));
                            }
                        }
                        kind::LEFT if body.len() >= 5 => {
                            let reason = u32::from_le_bytes(body[1..5].try_into().unwrap());
                            let gone: Vec<u8> = if body[0] == g.tr.me() { (0..8).filter(|&i| i != body[0]).collect() } else { vec![body[0]] };
                            for p in gone {
                                if g.names[p as usize].take().is_some() {
                                    g.events.push_back(NetEvent::Left(p, reason));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Event::Turn { player, frame, bytes } => g.early.push((player, frame, bytes)),
                Event::Error(m) => {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::warn_1(&format!("net: {m}").into());
                    eprintln!("net: {m}");
                    // A client's link to the host closed: the host and everyone with it are gone.
                    if !g.host {
                        for i in 0..8u8 {
                            if i != g.tr.me() && g.names[i as usize].take().is_some() {
                                g.events.push_back(NetEvent::Left(i, 0x4000_0004));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(a) = g.advert.as_ref() {
            let mut a = a.lock().unwrap();
            a.players = 1 + g.tr.peers().len() as u8;
            a.full = g.mode & 2 != 0;
        }
    });
}

/// Ordinal 128 (SNetSendTurn, from 0x485aa0): my next turn.
pub fn send_turn(t: &[u8]) {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut() else { return };
        g.in_flight += 1;
        if g.host {
            let me = g.tr.me();
            g.queues[me as usize].push_back(t.to_vec());
            g.make_rounds();
        } else {
            let mut m = vec![kind::TURN];
            m.extend_from_slice(t);
            g.tr.send_ctl(transport::HOST, m);
        }
    });
}

/// Ordinal 115 (0x485aa0): my turns not yet back in a round.
pub fn turns_in_flight() -> u32 {
    NET.with(|n| n.borrow().game.as_ref().map_or(0, |g| g.in_flight))
}

/// Ordinal 122 (SNetReceiveTurns, 0x4bb530): the next round of every player's turn, if
/// it is complete.
pub fn receive_turns() -> Option<Round> {
    pump();
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let g = n.game.as_mut()?;
        let r = g.rounds.pop_front()?;
        if r[g.tr.me() as usize & 7].0 & 0x20000 != 0 {
            g.in_flight = g.in_flight.saturating_sub(1);
        }
        Some(r)
    })
}

/// Ordinal 113 (SNetGetPlayerName): the name of Storm id `id`.
pub fn player_name(id: u32) -> Option<Vec<u8>> {
    NET.with(|n| n.borrow().game.as_ref()?.names.get(id as usize)?.clone())
}

/// Ordinal 106 (SNetDropPlayer, host): drop `id` (its link closes; everyone gets the leave).
pub fn drop_player(id: u32, reason: u32) {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut().filter(|g| g.host) else { return };
        if id < 8 {
            g.drop_player(id as u8, reason);
        }
    });
}

/// Ordinal 107 type 4 / Ordinal 130 (0x4c4160): the game mode bits (2: no open slot, the
/// game is not listed as joinable).
pub fn game_mode() -> u32 {
    NET.with(|n| n.borrow().game.as_ref().map_or(0, |g| g.mode))
}

pub fn set_game_mode(mode: u32) {
    NET.with(|n| {
        if let Some(g) = n.borrow_mut().game.as_mut() {
            g.mode = mode;
        }
    });
}

/// The transport of the game, for the in-game session (live.rs) once the lobby starts it:
/// (it, this side created the game, the players still in it, the game turns that came
/// already).
pub fn take_transport(keep_advert: bool) -> Option<(Transport, bool, Vec<u8>, Vec<(u8, u32, Vec<u8>)>)> {
    pump();
    NET.with(|n| {
        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut g = n.borrow_mut().game.take()?;
        // An enhanced game stays on the LAN list: joiners may come while it runs.
        #[cfg(not(target_arch = "wasm32"))]
        let kept = if keep_advert { g.advert.take() } else { None };
        #[cfg(target_arch = "wasm32")]
        let _ = keep_advert;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(a) = kept.as_ref() {
            // The lobby closed the game to start it; a running enhanced game takes joiners.
            let mut a = a.lock().unwrap();
            a.full = false;
            a.players = 1 + g.tr.peers().len() as u8;
        }
        #[cfg(not(target_arch = "wasm32"))]
        KEPT_ADVERT.with(|k| *k.borrow_mut() = kept);
        let ids = (0..8u8).filter(|&i| g.names[i as usize].is_some()).collect();
        Some((g.tr, g.host, ids, g.early))
    })
}

/// Ordinal 121: the next message (from, bytes).
pub fn receive_message() -> Option<(u8, Vec<u8>)> {
    NET.with(|n| n.borrow_mut().game.as_mut()?.inbox.pop_front())
}

/// The Storm events since the last call (the handlers of Ordinal 123).
pub fn events() -> Vec<NetEvent> {
    NET.with(|n| n.borrow_mut().game.as_mut().map(|g| g.events.drain(..).collect()).unwrap_or_default())
}

/// Ours (the custom join URL UI): the game's join code with the given address.
pub fn join_code(ip: &str) -> Option<String> {
    NET.with(|n| n.borrow().game.as_ref().map(|g| with_ip(&g.code, ip)))
}

/// Ordinal 119: leave the game (the link closes).
pub fn leave_game() {
    #[cfg(not(target_arch = "wasm32"))]
    KEPT_ADVERT.with(|k| *k.borrow_mut() = None);
    NET.with(|n| n.borrow_mut().game = None);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A host and a joiner (two threads, so two Storm states): the joiner finds the game on
    /// the LAN, joins as id 1, and a message goes each way.
    #[test]
    fn lan_create_list_join_message() {
        // SAFETY: set before any thread reads it.
        unsafe { std::env::set_var("SCBW_LAN_PORT", "16112") };
        let (tx, rx) = std::sync::mpsc::channel();
        let host = std::thread::spawn(move || {
            assert!(initialize_provider(UDPN));
            assert_eq!(create_game(b"test game", b",,,,\rhost\rmap\r", 2, b"host"), Some(0));
            let t0 = Instant::now();
            let mut joined = None;
            while t0.elapsed().as_secs() < 10 {
                pump();
                for e in events() {
                    if let NetEvent::Joined(p) = e {
                        joined = Some(p);
                        send_message(p, b"hello joiner");
                    }
                }
                if let Some(m) = receive_message() {
                    tx.send(m).unwrap();
                    // One round of turns: the host's and the joiner's.
                    send_turn(b"h");
                    let t1 = Instant::now();
                    let mut r = None;
                    while t1.elapsed().as_secs() < 10 && r.is_none() {
                        r = receive_turns();
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    let r = r.expect("host round");
                    assert_eq!((r[0].0, r[0].1.as_slice(), r[1].0, r[1].1.as_slice()), (0x30000, &b"h"[..], 0x30000, &b"j"[..]));
                    assert_eq!(player_name(1), Some(b"joiner".to_vec()));
                    // Keep the link up until the joiner has had time to ack our message.
                    let t1 = Instant::now();
                    while t1.elapsed().as_secs() < 3 {
                        pump();
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            joined
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(initialize_provider(UDPN));
        let mut games = Vec::new();
        for _ in 0..20 {
            games = enum_games();
            if !games.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(games.len(), 1, "the game is listed");
        assert_eq!(games[0].name, b"test game");
        assert_eq!((games[0].players, games[0].max), (1, 2));
        assert_eq!(join_game(games[0].id, None, b"joiner"), Some(1));
        send_message(0, b"hello host");
        send_turn(b"j");
        let t0 = Instant::now();
        let mut got = None;
        while t0.elapsed().as_secs() < 10 && got.is_none() {
            pump();
            got = receive_message();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, Some((0, b"hello joiner".to_vec())));
        let t0 = Instant::now();
        let mut r = None;
        while t0.elapsed().as_secs() < 10 && r.is_none() {
            r = receive_turns();
            std::thread::sleep(Duration::from_millis(5));
        }
        let r = r.expect("joiner round");
        assert_eq!((r[0].1.as_slice(), r[1].1.as_slice()), (&b"h"[..], &b"j"[..]));
        assert_eq!(turns_in_flight(), 0);
        assert_eq!(player_name(0), Some(b"host".to_vec()));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), (1, b"hello host".to_vec()));
        assert_eq!(host.join().unwrap(), Some(1));
    }
}
