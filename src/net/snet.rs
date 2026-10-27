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
    NET.with(|n| *n.borrow_mut() = Net::default());
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
                if &buf[..n] == QUERY {
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
pub fn create_game(name: &[u8], stat: &[u8], max: u8) -> Option<u8> {
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
    let (addr, hash) = code.split_once('#')?;
    let port: u16 = addr.rsplit_once(':')?.1.parse().ok()?;
    let advert = Arc::new(Mutex::new(Advert { port, hash: transport::unhex(hash).ok()?, players: 1, max, name: name.to_vec(), stat: stat.to_vec() }));
    advertise(&advert);
    NET.with(|n| {
        n.borrow_mut().game = Some(Game { tr, host: true, inbox: VecDeque::new(), events: VecDeque::new(), advert: Some(advert), code });
    });
    Some(0)
}

/// Ordinal 133 (0x4d37c0, the join screen every second): the LAN games heard of. Each call
/// asks again; answers arrive for the next call (as a LAN list fills). Games not heard of
/// for 3 s drop out.
pub fn enum_games() -> Vec<GameEntry> {
    NET.with(|n| {
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
pub fn join_game(id: u32, code: Option<&str>) -> Option<u8> {
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
                        NET.with(|n| n.borrow_mut().game = Some(Game { tr, host: false, inbox: VecDeque::new(), events: VecDeque::new(), advert: None, code }));
                        return Some(slot);
                    }
                    Event::Error(m) => {
                        eprintln!("net: join: {m}");
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
        let _ = code;
        None
    }
}

/// Ordinal 127: a message to Storm id `to` (Storm's 0xff = everyone else). Ours: the
/// transport's control messages, relayed by the host (clients only reach it).
pub fn send_message(to: u8, msg: &[u8]) {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut() else { return };
        let mut m = vec![to];
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
    let mut m = vec![from];
    m.extend_from_slice(msg);
    m
}

/// Drive the game's link: messages and joins / leaves arrive (call often).
pub fn pump() {
    NET.with(|n| {
        let mut n = n.borrow_mut();
        let Some(g) = n.game.as_mut() else { return };
        for e in g.tr.poll() {
            match e {
                Event::Joined { player } => g.events.push_back(NetEvent::Joined(player)),
                Event::Lost { player, .. } => g.events.push_back(NetEvent::Left(player, 0x4000_0004)),
                Event::Ctl { from, msg } if g.host => {
                    // A client's message: `to` + bytes; the host keeps or forwards it.
                    let Some((&to, body)) = msg.split_first() else { continue };
                    if to == 0 || to == 0xff {
                        g.inbox.push_back((from, body.to_vec()));
                    }
                    for p in g.tr.peers().to_vec() {
                        if p != from && (to == 0xff || to == p) {
                            g.tr.send_ctl(p, framed(from, body));
                        }
                    }
                }
                Event::Ctl { msg, .. } => {
                    if let Some((&from, body)) = msg.split_first() {
                        g.inbox.push_back((from, body.to_vec()));
                    }
                }
                Event::Error(m) => eprintln!("net: {m}"),
                _ => {}
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(a) = g.advert.as_ref() {
            a.lock().unwrap().players = 1 + g.tr.peers().len() as u8;
        }
    });
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
            assert_eq!(create_game(b"test game", b",,,,\rhost\rmap\r", 2), Some(0));
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
        assert_eq!(join_game(games[0].id, None), Some(1));
        send_message(0, b"hello host");
        let t0 = Instant::now();
        let mut got = None;
        while t0.elapsed().as_secs() < 10 && got.is_none() {
            pump();
            got = receive_message();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, Some((0, b"hello joiner".to_vec())));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), (1, b"hello host".to_vec()));
        assert_eq!(host.join().unwrap(), Some(1));
    }
}
