//! The campaign host (ours): `scbw campaign host <world> [dir]`. Owns the world, runs a
//! match (runner.rs, a thread each) on every border between two factions, all behind one
//! endpoint (`Hub`), applies each result (world.rs) and keeps going until one faction owns
//! everything. `dir` (default `campaign/`) holds `world.txt` (the world as it stands),
//! `players.txt` (names, factions, bans) and a saved game per live match, written every two
//! minutes and at exit; a host started on a `dir` that has them goes on from there.
//!
//! Commands on stdin (the web map's admin view sends the same, web.rs): `status`, `pause`,
//! `resume`, `kick <name>`, `ban <name>`, `unban <name>`, `rename <old> <new>`,
//! `end <border> <faction | draw>`, `save`, `quit`.

use super::runner::{self, End, Order, Report, Resume, Side, Spec, Stats};
use super::world::World;
use crate::net::transport::Hub;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SAVE_EVERY: Duration = Duration::from_secs(120);
/// A border whose match failed to start this often in a row gets no more.
const MAX_FAILS: u32 = 3;

/// Who plays for which faction: names are unique per campaign; a name's first join (or the
/// web map) gives it a faction for good.
#[derive(Default)]
pub struct Players {
    pub faction: Vec<(String, u8)>,
    pub banned: Vec<String>,
}

impl Players {
    pub fn of(&self, name: &str) -> Option<u8> {
        self.faction.iter().find(|n| n.0.eq_ignore_ascii_case(name)).map(|n| n.1)
    }

    pub fn is_banned(&self, name: &str) -> bool {
        self.banned.iter().any(|b| b.eq_ignore_ascii_case(name))
    }

    /// The faction `name` plays in a match of `sides` (registering a new name with the side
    /// that has fewer players); None when banned or of another faction.
    pub fn allow(&mut self, name: &str, sides: [u8; 2]) -> Option<u8> {
        if name.is_empty() || self.is_banned(name) {
            return None;
        }
        let f = match self.of(name) {
            Some(f) => f,
            None => {
                let count = |f: u8| self.faction.iter().filter(|n| n.1 == f).count();
                let f = if count(sides[1]) < count(sides[0]) { sides[1] } else { sides[0] };
                self.faction.push((name.to_string(), f));
                f
            }
        };
        sides.contains(&f).then_some(f)
    }

    fn parse(text: &str) -> Players {
        let mut p = Players::default();
        for l in text.lines() {
            match l.split_once(' ') {
                Some(("player", r)) => {
                    if let Some((f, n)) = r.split_once(' ').and_then(|(f, n)| Some((f.parse().ok()?, n))) {
                        p.faction.push((n.to_string(), f));
                    }
                }
                Some(("ban", n)) => p.banned.push(n.to_string()),
                _ => {}
            }
        }
        p
    }

    fn write(&self) -> String {
        let mut s = String::new();
        for (n, f) in &self.faction {
            s += &format!("player {f} {n}\n");
        }
        for b in &self.banned {
            s += &format!("ban {b}\n");
        }
        s
    }
}

/// A running match.
pub struct Live {
    pub game: u16,
    pub spec: Spec,
    pub orders: Sender<Order>,
    pub code: Option<String>,
    pub stats: Stats,
}

/// What the web map reads (shared with its server thread).
pub struct Shared {
    pub world: World,
    pub live: HashMap<u16, Live>,
    pub players: Players,
    pub paused: bool,
    pub winner: Option<u8>,
    /// Faction chat lines (faction, name, text), newest last.
    pub chat: Vec<(u8, String, String)>,
}

pub struct Host {
    pub shared: Arc<Mutex<Shared>>,
    hub: Hub,
    dir: PathBuf,
    reports: (Sender<(u16, Report)>, Receiver<(u16, Report)>),
    next_game: u16,
    seed: u32,
    /// Per border: failed starts in a row, and when to try again.
    fails: HashMap<u16, (u32, Option<Instant>)>,
    /// Pool maps that did not load.
    bad_maps: Vec<String>,
    last_save: Instant,
}

impl Host {
    /// The campaign of `world_file` (or the one saved in `dir`), its matches behind `port`.
    pub fn new(world_file: &str, dir: &Path, port: u16) -> Result<Host, String> {
        let saved = dir.join("world.txt");
        let resume = saved.exists();
        let text = std::fs::read_to_string(if resume { saved.as_path() } else { Path::new(world_file) }).map_err(|e| format!("{world_file}: {e}"))?;
        let mut world = World::parse(&text)?;
        world.deal()?;
        let players = Players::parse(&std::fs::read_to_string(dir.join("players.txt")).unwrap_or_default());
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let hub = Hub::new(port)?;
        let seed = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(1, |t| t.as_secs() as u32) | 1;
        let shared = Shared { world, live: HashMap::new(), players, paused: false, winner: None, chat: Vec::new() };
        let mut h = Host { shared: Arc::new(Mutex::new(shared)), hub, dir: dir.to_path_buf(), reports: mpsc::channel(), next_game: 0, seed, fails: HashMap::new(), bad_maps: Vec::new(), last_save: Instant::now() };
        let live = h.shared.lock().unwrap().world.live_borders();
        for b in live {
            let r = if resume { Resume::read(&h.save_path(b)) } else { None };
            h.start(b, r);
        }
        println!("campaign: {} ({})", if resume { "resumed" } else { "started" }, h.hub.code(0).trim_end_matches("#0"));
        print!("{}", h.shared.lock().unwrap().world.summary());
        Ok(h)
    }

    fn save_path(&self, border: u16) -> PathBuf {
        self.dir.join(format!("border-{border}.sav"))
    }

    pub fn rand(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }

    /// The map files of the pool (directories: every .scm / .scx under them, sorted), less the
    /// bad ones.
    fn pool_maps(&self, pool: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for p in pool {
            let path = Path::new(p);
            if path.is_dir() {
                let mut v = Vec::new();
                let mut dirs = vec![path.to_path_buf()];
                while let Some(d) = dirs.pop() {
                    for f in std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()) {
                        if f.is_dir() {
                            dirs.push(f);
                        } else if f.extension().is_some_and(|x| x.eq_ignore_ascii_case("scm") || x.eq_ignore_ascii_case("scx")) {
                            v.push(f.to_string_lossy().into_owned());
                        }
                    }
                }
                v.sort();
                out.extend(v);
            } else {
                out.push(p.clone());
            }
        }
        out.retain(|m| !self.bad_maps.contains(m));
        out
    }

    /// Start border `b`'s match (from `resume` when given).
    fn start(&mut self, b: u16, resume: Option<Resume>) {
        let (map, sides, pool) = {
            let sh = self.shared.lock().unwrap();
            let w = &sh.world;
            let br = &w.borders[b as usize];
            let side = |a: u16| {
                let f = w.areas[a as usize].owner.unwrap_or(0);
                let fa = &w.factions[f as usize];
                Side { faction: f, race: fa.race, colour: fa.colour, name: fa.name.clone() }
            };
            (br.map.clone(), [side(br.a), side(br.b)], w.pool.clone())
        };
        let map = match map {
            Some(m) => m,
            None => {
                let maps = self.pool_maps(&pool);
                if maps.is_empty() {
                    eprintln!("campaign: border {b}: no map in the pool loads");
                    return;
                }
                let i = self.rand() as usize % maps.len();
                maps[i].clone()
            }
        };
        let game = self.next_game;
        self.next_game = self.next_game.wrapping_add(1);
        let spec = Spec { border: b, game, map, sides, seed: self.rand(), resume };
        let (otx, orx) = mpsc::channel();
        let (rtx, rrx) = mpsc::channel();
        let all = self.reports.0.clone();
        // The runner's reports, tagged with its game id.
        std::thread::spawn(move || {
            for r in rrx {
                if all.send((game, r)).is_err() {
                    break;
                }
            }
        });
        let shared = self.shared.clone();
        let allow: runner::Allow = Box::new(move |name, sides| shared.lock().unwrap().players.allow(name, sides));
        let (s, hub) = (spec.clone(), self.hub.clone());
        std::thread::Builder::new().name(format!("border {b}")).spawn(move || runner::run(s, hub, rtx, orx, Some(allow), None)).ok();
        if self.shared.lock().unwrap().paused {
            let _ = otx.send(Order::Pause(true));
        }
        self.shared.lock().unwrap().live.insert(b, Live { game, spec, orders: otx, code: None, stats: Stats::default() });
    }

    fn stop(&mut self, b: u16) {
        if let Some(l) = self.shared.lock().unwrap().live.remove(&b) {
            let _ = l.orders.send(Order::Stop);
        }
    }

    /// One match ended: the world changes, matches end and start.
    fn ended(&mut self, b: u16, end: End) {
        let Some(l) = self.shared.lock().unwrap().live.remove(&b) else { return };
        let sides = l.spec.sides.clone();
        match end {
            End::Stopped => {}
            End::Failed => {
                let f = self.fails.entry(b).or_insert((0, None));
                f.0 += 1;
                if l.spec.resume.is_none() && self.shared.lock().unwrap().world.borders[b as usize].map.is_none() {
                    self.bad_maps.push(l.spec.map.clone());
                }
                if f.0 < MAX_FAILS {
                    f.1 = Some(Instant::now() + Duration::from_secs(5));
                } else {
                    eprintln!("campaign: border {b}: failed {MAX_FAILS} times, no more matches there");
                }
            }
            End::Draw => {
                println!("campaign: border {b} ({} - {}): draw, a new match starts", sides[0].name, sides[1].name);
                self.fails.remove(&b);
                self.start(b, None);
            }
            End::Won(i) => {
                self.fails.remove(&b);
                self.apply(b, Some(sides[i].faction), [sides[0].faction, sides[1].faction]);
            }
        }
    }

    /// `winner` takes border `b` (None: a draw) between the factions `sides`; stops and
    /// starts what that changes.
    fn apply(&mut self, b: u16, winner: Option<u8>, sides: [u8; 2]) {
        let r = self.shared.lock().unwrap().world.apply(b, winner);
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                eprintln!("campaign: {e}");
                return;
            }
        };
        let (news, winner_all) = {
            let sh = self.shared.lock().unwrap();
            let w = &sh.world;
            let mut s = String::new();
            if let (Some(a), Some(f)) = (o.captured, winner) {
                s += &format!("campaign: {} takes {}", w.factions[f as usize].name, w.areas[a as usize].name);
                for f in sides.iter().filter(|&&f| w.areas_of(f) == 0) {
                    s += &format!("; {} is eliminated", w.factions[*f as usize].name);
                }
            }
            (s, w.winner())
        };
        if !news.is_empty() {
            println!("{news}");
        }
        for c in o.cancel {
            self.stop(c);
        }
        if let Some(f) = winner_all {
            let name = self.shared.lock().unwrap().world.factions[f as usize].name.clone();
            println!("campaign: {name} wins the campaign");
            self.shared.lock().unwrap().winner = Some(f);
            return;
        }
        for s in o.start {
            self.start(s, None);
        }
    }

    /// Write the world, the players and every live match's save.
    pub fn save(&mut self) {
        let sh = self.shared.lock().unwrap();
        let _ = std::fs::write(self.dir.join("world.txt"), sh.world.write());
        let _ = std::fs::write(self.dir.join("players.txt"), sh.players.write());
        // Saves of matches that are no longer live must not come back on a resume.
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(b) = n.strip_prefix("border-").and_then(|r| r.split('.').next()).and_then(|b| b.parse::<u16>().ok()) {
                if !sh.live.contains_key(&b) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        for (&b, l) in &sh.live {
            let _ = l.orders.send(Order::Save(self.save_path(b)));
        }
        self.last_save = Instant::now();
    }

    /// A command line (stdin or the web admin view); the answer to print.
    pub fn command(&mut self, line: &str) -> String {
        let mut w = line.split_whitespace();
        let (cmd, arg) = (w.next().unwrap_or(""), w.next().unwrap_or(""));
        let rest: Vec<&str> = w.collect();
        let send_all = |sh: &Shared, o: &dyn Fn() -> Order| {
            for l in sh.live.values() {
                let _ = l.orders.send(o());
            }
        };
        match cmd {
            "status" => {
                let sh = self.shared.lock().unwrap();
                let mut s = sh.world.summary();
                for (b, l) in &sh.live {
                    let hum: Vec<String> = l.stats.sides.iter().map(|x| x.humans.join(",")).collect();
                    s += &format!("border {b}: game {} frame {} idle {} humans [{}] [{}] {}\n", l.game, l.stats.frame, l.stats.idle_frames, hum[0], hum[1], l.code.as_deref().unwrap_or(""));
                }
                s
            }
            "pause" | "resume" => {
                let mut sh = self.shared.lock().unwrap();
                sh.paused = cmd == "pause";
                let on = sh.paused;
                send_all(&sh, &|| Order::Pause(on));
                format!("campaign {cmd}d")
            }
            "kick" | "ban" if !arg.is_empty() => {
                let mut sh = self.shared.lock().unwrap();
                if cmd == "ban" && !sh.players.is_banned(arg) {
                    sh.players.banned.push(arg.to_string());
                }
                send_all(&sh, &|| Order::Kick(arg.to_string()));
                format!("{arg} {}", if cmd == "ban" { "banned" } else { "kicked" })
            }
            "unban" => {
                self.shared.lock().unwrap().players.banned.retain(|b| !b.eq_ignore_ascii_case(arg));
                format!("{arg} unbanned")
            }
            "rename" if rest.len() == 1 => {
                let mut sh = self.shared.lock().unwrap();
                let new = rest[0];
                if sh.players.of(new).is_some() {
                    return format!("{new} is taken");
                }
                match sh.players.faction.iter_mut().find(|n| n.0.eq_ignore_ascii_case(arg)) {
                    Some(n) => {
                        n.0 = new.to_string();
                        // In a match the old name stays until the player joins again.
                        send_all(&sh, &|| Order::Kick(arg.to_string()));
                        format!("{arg} is now {new}")
                    }
                    None => format!("no player {arg}"),
                }
            }
            "end" if !rest.is_empty() => {
                let Ok(b) = arg.parse::<u16>() else { return "end <border> <faction | draw>".into() };
                let Some(l) = self.shared.lock().unwrap().live.get(&b).map(|l| l.spec.sides.clone()) else { return format!("border {b} has no match") };
                let end = match rest[0] {
                    "draw" => End::Draw,
                    f => match l.iter().position(|s| s.faction.to_string() == f || s.name.eq_ignore_ascii_case(f)) {
                        Some(i) => End::Won(i),
                        None => return format!("{f} is not on border {b}"),
                    },
                };
                self.stop_keep(b);
                self.ended(b, end);
                format!("border {b} ended: {end:?}")
            }
            "save" => {
                self.save();
                "saved".into()
            }
            _ => "commands: status, pause, resume, kick <name>, ban <name>, unban <name>, rename <old> <new>, end <border> <faction | draw>, save, quit".into(),
        }
    }

    /// Stop border `b`'s match but keep its entry (its result is decided here).
    fn stop_keep(&mut self, b: u16) {
        if let Some(l) = self.shared.lock().unwrap().live.get_mut(&b) {
            let _ = l.orders.send(Order::Stop);
            // Its reports from now on are stale.
            l.game = u16::MAX;
        }
    }

    /// One pass: reports, retries, the periodic save. False once the campaign is over.
    pub fn tick(&mut self) -> bool {
        while let Ok((game, r)) = self.reports.1.try_recv() {
            let b = {
                let sh = self.shared.lock().unwrap();
                sh.live.iter().find(|(_, l)| l.game == game).map(|(&b, _)| b)
            };
            let Some(b) = b else { continue };
            match r {
                Report::Started(code) => {
                    if let Some(l) = self.shared.lock().unwrap().live.get_mut(&b) {
                        l.code = Some(code);
                    }
                }
                Report::Stats(s) => {
                    if let Some(l) = self.shared.lock().unwrap().live.get_mut(&b) {
                        l.stats = s;
                    }
                }
                Report::Chat(f, name, text) => {
                    let mut sh = self.shared.lock().unwrap();
                    sh.chat.push((f, name.clone(), text.clone()));
                    if sh.chat.len() > 200 {
                        sh.chat.remove(0);
                    }
                    for (&o, l) in &sh.live {
                        if o != b && l.spec.sides.iter().any(|s| s.faction == f) {
                            let _ = l.orders.send(Order::Say(f, format!("{name}: {text}")));
                        }
                    }
                }
                Report::Ended(e) => self.ended(b, e),
            }
        }
        let due: Vec<u16> = self.fails.iter().filter(|f| f.1 .1.is_some_and(|t| Instant::now() >= t)).map(|f| *f.0).collect();
        for b in due {
            self.fails.get_mut(&b).unwrap().1 = None;
            if self.shared.lock().unwrap().world.is_live(b) {
                self.start(b, None);
            }
        }
        if self.last_save.elapsed() >= SAVE_EVERY {
            self.save();
        }
        self.shared.lock().unwrap().winner.is_none()
    }
}

/// `scbw campaign host <world> [dir] [port] [web port]`: the matches behind `port`, the web
/// map on `web port` (default 8780; SCBW_CAMPAIGN_PLAY = the browser build's directory,
/// default `web`; SCBW_CAMPAIGN_WEB = the URL the page's Join opens, default its own
/// `/play/index.html`, which a browser only runs from localhost or https).
pub fn main(world: &str, dir: &str, port: u16, web_port: u16) -> Result<(), String> {
    let mut h = Host::new(world, Path::new(dir), port)?;
    let (atx, arx) = mpsc::channel::<super::web::Admin>();
    let token = format!("{:08x}{:08x}", h.rand(), h.rand());
    let web = super::web::Web {
        shared: h.shared.clone(),
        admin: atx,
        token: token.clone(),
        play_dir: std::env::var("SCBW_CAMPAIGN_PLAY").unwrap_or_else(|_| "web".into()).into(),
        play_url: std::env::var("SCBW_CAMPAIGN_WEB").unwrap_or_else(|_| "/play/index.html".into()),
    };
    let wp = super::web::serve(web, web_port)?;
    println!("campaign: web map http://127.0.0.1:{wp}/ (admin: http://127.0.0.1:{wp}/?token={token})");
    let (ltx, lrx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in std::io::stdin().lines().map_while(Result::ok) {
            if ltx.send(l).is_err() {
                break;
            }
        }
    });
    loop {
        if !h.tick() {
            break;
        }
        match lrx.try_recv() {
            Ok(l) if l.trim() == "quit" => break,
            Ok(l) => println!("{}", h.command(&l)),
            Err(_) => {}
        }
        while let Ok((cmd, back)) = arx.try_recv() {
            let _ = back.send(h.command(&cmd));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    h.save();
    // The matches write their saves on their own threads.
    std::thread::sleep(Duration::from_millis(500));
    Ok(())
}

/// A plain HTTP GET of `url` (`http://host:port/path?query`): the body.
pub(crate) fn http_get(url: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let rest = url.strip_prefix("http://").ok_or("only http:// web maps")?;
    let (host, path) = rest.split_once('/').map_or((rest, String::from("/")), |(h, p)| (h, format!("/{p}")));
    let mut s = std::net::TcpStream::connect(host).map_err(|e| format!("{host}: {e}"))?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").map_err(|e| e.to_string())?;
    let mut b = String::new();
    s.read_to_string(&mut b).map_err(|e| e.to_string())?;
    let (head, body) = b.split_once("\r\n\r\n").ok_or("bad answer")?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(body.trim().to_string());
    }
    Ok(body.to_string())
}

/// `scbw campaign join <web map url> <name> [faction]`: register the name (with `faction`
/// when new); the glue then opens on the Join Game screen listing the faction's matches
/// (net/snet.rs `set_campaign`).
pub fn register(url: &str, name: &str, faction: Option<u8>) -> Result<(), String> {
    let base = url.trim_end_matches('/');
    let enc: String = name.bytes().map(|c| if c.is_ascii_alphanumeric() { (c as char).to_string() } else { format!("%{c:02x}") }).collect();
    if let Some(f) = faction {
        http_get(&format!("{base}/join?name={enc}&faction={f}"))?;
    }
    crate::net::snet::set_campaign(base, name);
    Ok(())
}
