//! Phase 3a transport (agent/areas/net.md): QUIC/WebTransport on a tokio thread, star
//! topology (the host is also a player), driven from the game loop through channels.
//!
//! Lobby: one reliable bidirectional stream per client. The client sends `Hello{player}`;
//! once every expected client said hello the host sends `Start` to all. Messages are
//! `len u16 LE` (of what follows) + `type u8` + body: Hello = 1 + `player u8`; Start = 2.
//!
//! Turns: unreliable datagrams, resent every flush until acked. One datagram:
//! `kind u8 = 1`, `nacks u8`, nacks x (`player u8`, `next u32`: the sender has every turn
//! of that player below `next`), `nruns u8`, nruns x (`player u8`, `count u16`, count x
//! (`frame u32`, `len u16`, bytes)). A run is the receiver's unacked turns of one player.
//! Client -> host: its own turns. Host -> client: every player's but that client's (the
//! host keeps all it receives and relays them). Frames under `first` (the session's delay)
//! are known empty and never sent. A datagram is capped at the connection's maximum
//! datagram size; what does not fit goes in the next flush.
//!
//! All protocol state lives in `Transport` on the game thread; the runtime thread only
//! moves bytes (`Inbound` in, `(peer, bytes)` out). The certificate is self-signed per run
//! and the client pins its SHA-256 (`server_certificate_hashes`, as a browser would).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::sync::mpsc as tmpsc;
use wtransport::endpoint::IncomingSession;
use wtransport::endpoint::endpoint_side::Server;
use wtransport::tls::{Sha256Digest, Sha256DigestFmt};
use wtransport::{ClientConfig, Connection, Endpoint, Identity, RecvStream, ServerConfig};

/// The peer id of the host, as a client sees it.
pub const HOST: u8 = 255;
const DGRAM: u8 = 1;
/// Most frames of one player in one datagram.
const MAX_RUN: usize = 64;

pub enum Event {
    /// The host sent Start: play.
    Start,
    /// A turn of `player` for `frame` arrived for the first time.
    Turn { player: u8, frame: u32, bytes: Vec<u8> },
    /// The runtime thread failed or the link closed.
    Error(String),
}

enum Inbound {
    Start,
    Dgram { from: u8, data: Vec<u8> },
    Error(String),
}

pub struct Transport {
    me: u8,
    host: bool,
    /// Peers this side exchanges datagrams with: the clients' players, or `[HOST]`.
    peers: Vec<u8>,
    /// Frames below this are known empty and never sent.
    first: u32,
    /// Per player, the turn of each frame.
    got: HashMap<u8, Vec<Option<Vec<u8>>>>,
    /// Per player, every turn below this is present.
    next: HashMap<u8, u32>,
    /// (peer, player) -> the peer has every turn of the player below this.
    acked: HashMap<(u8, u8), u32>,
    /// A datagram of the peer arrived since the last flush to it: answer with fresh acks.
    dirty: HashMap<u8, bool>,
    /// The acks last sent to a peer (a flush with nothing new and the same acks is skipped).
    sent_acks: HashMap<u8, Vec<(u8, u32)>>,
    rx: mpsc::Receiver<Inbound>,
    tx: tmpsc::UnboundedSender<(u8, Vec<u8>)>,
    max_dgram: Arc<AtomicUsize>,
    /// Datagrams (sent, received, unparsable): diagnostics.
    pub stats: (u32, u32, u32),
}

struct Reader<'a> {
    d: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(4)?.try_into().ok()?))
    }
}

fn hex(d: &Sha256Digest) -> String {
    d.fmt(Sha256DigestFmt::DottedHex).replace(':', "")
}

fn unhex(s: &str) -> Result<Sha256Digest, String> {
    let b: Result<Vec<u8>, _> = (0..s.len() / 2).map(|i| u8::from_str_radix(s.get(2 * i..2 * i + 2).unwrap_or("zz"), 16)).collect();
    let b: [u8; 32] = b.map_err(|_| "bad certificate hash")?.try_into().map_err(|_| "bad certificate hash length")?;
    Ok(Sha256Digest::new(b))
}

fn new_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())
}

/// Run `f` on a current-thread tokio runtime on its own thread; its error goes to the game.
fn spawn_runtime<Fut>(itx: mpsc::Sender<Inbound>, rt: tokio::runtime::Runtime, f: Fut) -> Result<(), String>
where
    Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    std::thread::Builder::new()
        .name("net".into())
        .spawn(move || {
            if let Err(e) = rt.block_on(f) {
                let _ = itx.send(Inbound::Error(e));
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

impl Transport {
    fn new(me: u8, host: bool, peers: Vec<u8>, first: u32, rx: mpsc::Receiver<Inbound>, tx: tmpsc::UnboundedSender<(u8, Vec<u8>)>, max_dgram: Arc<AtomicUsize>) -> Transport {
        let mut got = HashMap::new();
        got.insert(me, Vec::new());
        Transport { me, host, peers, first, got, next: HashMap::new(), acked: HashMap::new(), dirty: HashMap::new(), sent_acks: HashMap::new(), rx, tx, max_dgram, stats: (0, 0, 0) }
    }

    /// Listen on `127.0.0.1:port` (0: any free port). Returns `ip:port#certhash` for the
    /// clients. `clients`: the players that will join; `first`: see the module doc.
    pub fn host(port: u16, me: u8, clients: &[u8], first: u32) -> Result<(Transport, String), String> {
        let identity = Identity::self_signed(["localhost", "127.0.0.1"]).map_err(|e| e.to_string())?;
        let hash = identity.certificate_chain().as_slice()[0].hash();
        let config = ServerConfig::builder()
            .with_bind_address(SocketAddr::from(([127, 0, 0, 1], port)))
            .with_identity(identity)
            .keep_alive_interval(Some(Duration::from_secs(3)))
            .build();
        // quinn's socket needs a runtime context at creation.
        let rt = new_runtime()?;
        let ep = {
            let _g = rt.enter();
            Endpoint::server(config).map_err(|e| e.to_string())?
        };
        let addr = ep.local_addr().map_err(|e| e.to_string())?;
        let (itx, rx) = mpsc::channel();
        let (tx, orx) = tmpsc::unbounded_channel();
        let max_dgram = Arc::new(AtomicUsize::new(1000));
        spawn_runtime(itx.clone(), rt, host_run(ep, clients.to_vec(), itx, orx, max_dgram.clone()))?;
        Ok((Transport::new(me, true, clients.to_vec(), first, rx, tx, max_dgram), format!("{addr}#{}", hex(&hash))))
    }

    /// Connect to a host (`target` = `ip:port#certhash`) and say hello as `me`.
    pub fn join(target: &str, me: u8, first: u32) -> Result<Transport, String> {
        let (addr, hash) = target.split_once('#').ok_or("expected addr#hash")?;
        let hash = unhex(hash)?;
        let config = ClientConfig::builder()
            .with_bind_default()
            .with_server_certificate_hashes([hash])
            .keep_alive_interval(Some(Duration::from_secs(3)))
            .build();
        let (itx, rx) = mpsc::channel();
        let (tx, orx) = tmpsc::unbounded_channel();
        let max_dgram = Arc::new(AtomicUsize::new(1000));
        spawn_runtime(itx.clone(), new_runtime()?, client_run(config, format!("https://{addr}"), me, itx, orx, max_dgram.clone()))?;
        Ok(Transport::new(me, false, vec![HOST], first, rx, tx, max_dgram))
    }

    /// The datagram size cap (diagnostics).
    pub fn cap(&self) -> usize {
        self.max_dgram.load(Ordering::Relaxed)
    }

    /// My turn for `frame` (the `Session::local_turn` result); sent by the next flushes.
    pub fn send_turns(&mut self, frame: u32, bytes: Vec<u8>) {
        Self::put(&mut self.got, self.me, frame, bytes);
        self.advance();
    }

    fn put(got: &mut HashMap<u8, Vec<Option<Vec<u8>>>>, player: u8, frame: u32, bytes: Vec<u8>) -> bool {
        let t = got.entry(player).or_default();
        if t.len() <= frame as usize {
            t.resize(frame as usize + 1, None);
        }
        let slot = &mut t[frame as usize];
        if slot.is_some() {
            return false;
        }
        *slot = Some(bytes);
        true
    }

    fn advance(&mut self) {
        for (p, t) in &self.got {
            let n = self.next.entry(*p).or_insert(self.first);
            while t.get(*n as usize).is_some_and(|x| x.is_some()) {
                *n += 1;
            }
        }
    }

    /// Drain what arrived (new turns and lobby events), then flush the datagrams.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Inbound::Start => ev.push(Event::Start),
                Inbound::Error(e) => ev.push(Event::Error(e)),
                Inbound::Dgram { from, data } => self.ingest(from, &data, &mut ev),
            }
        }
        self.advance();
        self.flush();
        ev
    }

    fn ingest(&mut self, from: u8, d: &[u8], ev: &mut Vec<Event>) {
        let mut r = Reader { d, at: 0 };
        self.stats.1 += 1;
        let ok = (|| -> Option<()> {
            if r.u8()? != DGRAM {
                return None;
            }
            for _ in 0..r.u8()? {
                let (p, n) = (r.u8()?, r.u32()?);
                let a = self.acked.entry((from, p)).or_insert(self.first);
                *a = (*a).max(n);
            }
            for _ in 0..r.u8()? {
                let p = r.u8()?;
                for _ in 0..r.u16()? {
                    let (frame, len) = (r.u32()?, r.u16()? as usize);
                    let bytes = r.bytes(len)?;
                    // The host takes only a client's own turns; a client everyone's but its own.
                    if p == self.me || (self.host && p != from) || frame < self.first {
                        continue;
                    }
                    if Self::put(&mut self.got, p, frame, bytes.to_vec()) {
                        ev.push(Event::Turn { player: p, frame, bytes: bytes.to_vec() });
                    }
                }
            }
            Some(())
        })();
        if ok.is_none() {
            self.stats.2 += 1;
        }
        self.dirty.insert(from, true);
    }

    fn flush(&mut self) {
        let cap = self.max_dgram.load(Ordering::Relaxed).saturating_sub(8);
        let mut players: Vec<u8> = self.got.keys().copied().collect();
        players.sort();
        for &peer in &self.peers.clone() {
            let acks: Vec<(u8, u32)> = players.iter().map(|&p| (p, *self.next.get(&p).unwrap_or(&self.first))).collect();
            let mut out = vec![DGRAM, acks.len() as u8];
            for &(p, n) in &acks {
                out.push(p);
                out.extend_from_slice(&n.to_le_bytes());
            }
            let nruns_at = out.len();
            out.push(0);
            let mut nruns = 0u8;
            for &p in &players {
                // A client sends only its own turns; the host everyone's but the peer's.
                if (!self.host && p != self.me) || (self.host && p == peer) {
                    continue;
                }
                let from = *self.acked.get(&(peer, p)).unwrap_or(&self.first) as usize;
                let hdr = out.len();
                out.extend_from_slice(&[p, 0, 0]);
                let mut count = 0u16;
                for (f, t) in self.got[&p].iter().enumerate().skip(from) {
                    let Some(t) = t else { continue };
                    if count as usize >= MAX_RUN || out.len() + 6 + t.len() > cap {
                        break;
                    }
                    out.extend_from_slice(&(f as u32).to_le_bytes());
                    out.extend_from_slice(&(t.len() as u16).to_le_bytes());
                    out.extend_from_slice(t);
                    count += 1;
                }
                if count == 0 {
                    out.truncate(hdr);
                } else {
                    out[hdr + 1..hdr + 3].copy_from_slice(&count.to_le_bytes());
                    nruns += 1;
                }
            }
            out[nruns_at] = nruns;
            let dirty = self.dirty.insert(peer, false).unwrap_or(false);
            if nruns == 0 && !dirty && self.sent_acks.get(&peer) == Some(&acks) {
                continue;
            }
            self.sent_acks.insert(peer, acks);
            self.stats.0 += 1;
            let _ = self.tx.send((peer, out));
        }
    }

    /// Every peer has every turn below `upto` of every player it should get from me.
    pub fn fully_acked(&self, upto: u32) -> bool {
        let a = |peer: u8, p: u8| *self.acked.get(&(peer, p)).unwrap_or(&self.first) >= upto;
        if self.host {
            self.peers.iter().all(|&c| self.got.keys().filter(|&&p| p != c).all(|&p| a(c, p)))
        } else {
            a(HOST, self.me)
        }
    }
}

type Conns = Arc<Mutex<HashMap<u8, Connection>>>;

async fn read_msg(r: &mut RecvStream) -> Result<(u8, Vec<u8>), String> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len).await.map_err(|e| e.to_string())?;
    let mut body = vec![0u8; u16::from_le_bytes(len) as usize];
    r.read_exact(&mut body).await.map_err(|e| e.to_string())?;
    if body.is_empty() {
        return Err("empty lobby message".into());
    }
    Ok((body[0], body[1..].to_vec()))
}

async fn host_run(ep: Endpoint<Server>, expected: Vec<u8>, itx: mpsc::Sender<Inbound>, mut orx: tmpsc::UnboundedReceiver<(u8, Vec<u8>)>, md: Arc<AtomicUsize>) -> Result<(), String> {
    let conns: Conns = Default::default();
    let count = Arc::new(AtomicUsize::new(0));
    let (ready, _) = tokio::sync::watch::channel(false);
    let ready = Arc::new(ready);
    let accept = async {
        loop {
            let inc = ep.accept().await;
            let (expected, conns, count, ready, itx, md) = (expected.clone(), conns.clone(), count.clone(), ready.clone(), itx.clone(), md.clone());
            tokio::spawn(async move {
                if let Err(e) = host_conn(inc, expected, conns, count, ready, itx.clone(), md).await {
                    let _ = itx.send(Inbound::Error(e));
                }
            });
        }
    };
    let send = async {
        while let Some((to, data)) = orx.recv().await {
            let c = conns.lock().unwrap().get(&to).cloned();
            if let Some(c) = c {
                if let Err(e) = c.send_datagram(data) {
                    let _ = itx.send(Inbound::Error(format!("send to {to}: {e}")));
                }
            }
        }
    };
    tokio::select! {
        _ = accept => {}
        _ = send => {}
    }
    Ok(())
}

async fn host_conn(
    inc: IncomingSession,
    expected: Vec<u8>,
    conns: Conns,
    count: Arc<AtomicUsize>,
    ready: Arc<tokio::sync::watch::Sender<bool>>,
    itx: mpsc::Sender<Inbound>,
    md: Arc<AtomicUsize>,
) -> Result<(), String> {
    let conn = inc.await.map_err(|e| e.to_string())?.accept().await.map_err(|e| e.to_string())?;
    let (mut send, mut recv) = conn.accept_bi().await.map_err(|e| e.to_string())?;
    let (ty, body) = read_msg(&mut recv).await?;
    let player = match (ty, body.as_slice()) {
        (1, &[p]) if expected.contains(&p) => p,
        _ => return Err("bad hello".into()),
    };
    {
        let mut c = conns.lock().unwrap();
        if c.contains_key(&player) {
            return Err(format!("player {player} joined twice"));
        }
        c.insert(player, conn.clone());
    }
    if let Some(n) = conn.max_datagram_size() {
        md.fetch_min(n, Ordering::Relaxed);
    }
    let mut rdy = ready.subscribe();
    if count.fetch_add(1, Ordering::SeqCst) + 1 == expected.len() {
        let _ = ready.send(true);
        let _ = itx.send(Inbound::Start);
    }
    rdy.wait_for(|v| *v).await.map_err(|e| e.to_string())?;
    send.write_all(&[1, 0, 2]).await.map_err(|e| e.to_string())?;
    loop {
        let d = conn.receive_datagram().await.map_err(|e| e.to_string())?;
        let _ = itx.send(Inbound::Dgram { from: player, data: d.payload().to_vec() });
    }
}

async fn client_run(config: ClientConfig, url: String, me: u8, itx: mpsc::Sender<Inbound>, mut orx: tmpsc::UnboundedReceiver<(u8, Vec<u8>)>, md: Arc<AtomicUsize>) -> Result<(), String> {
    let ep = Endpoint::client(config).map_err(|e| e.to_string())?;
    let conn = ep.connect(url).await.map_err(|e| e.to_string())?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
    send.write_all(&[2, 0, 1, me]).await.map_err(|e| e.to_string())?;
    if read_msg(&mut recv).await?.0 != 2 {
        return Err("expected Start".into());
    }
    if let Some(n) = conn.max_datagram_size() {
        md.fetch_min(n, Ordering::Relaxed);
    }
    let _ = itx.send(Inbound::Start);
    loop {
        tokio::select! {
            d = conn.receive_datagram() => {
                let d = d.map_err(|e| e.to_string())?;
                let _ = itx.send(Inbound::Dgram { from: HOST, data: d.payload().to_vec() });
            }
            m = orx.recv() => {
                let Some((_, data)) = m else { return Ok(()) };
                if let Err(e) = conn.send_datagram(data) {
                    let _ = itx.send(Inbound::Error(format!("send: {e}")));
                }
            }
        }
    }
}
