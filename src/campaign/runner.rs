//! A campaign match (ours): one border's melee game, run headless on its own thread by the
//! campaign host. Both armies start as computers (melee scripts, 0x4ab230 through the
//! game start) and count as taken by the AI, so a joiner takes one over (enhanced games,
//! bw/enhanced.rs); the host plays an empty slot of its own and sends nothing but join
//! commands. The game runs at Fastest (42 ms a frame) whether humans are in it or not.

use crate::bw::State;
use crate::net::live::NetGame;
use crate::net::transport::Hub;
use crate::net::Session;
use crate::view::{self, NetDriver, Scene};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// One side of a match: its faction and how its army plays.
#[derive(Clone)]
pub struct Side {
    pub faction: u8,
    /// 1.16.1 race (6: random, dealt from the seed).
    pub race: u8,
    /// tunit.pcx colour row.
    pub colour: u8,
    pub name: String,
}

#[derive(Clone)]
pub struct Spec {
    pub border: u16,
    /// The match id on the hub (a new one per match: a border's next match never meets
    /// the last one's links).
    pub game: u16,
    pub map: String,
    pub sides: [Side; 2],
    pub seed: u32,
    /// Go on from a saved match (the campaign host restarted).
    pub resume: Option<Resume>,
}

/// A match saved by `Order::Save`: the 1.16.1 saved game, its seats, its draw timer, its
/// match id (join links stay valid after a restart).
#[derive(Clone)]
pub struct Resume {
    pub save: Vec<u8>,
    pub armies: [u8; 2],
    pub host: u8,
    pub idle_frames: u32,
    pub game: Option<u16>,
}

impl Resume {
    /// `<path>` (the save) and `<path>.txt` (`armies A B host H idle N [game G]`).
    pub fn read(path: &std::path::Path) -> Option<Resume> {
        let save = std::fs::read(path).ok()?;
        let t = std::fs::read_to_string(path.with_extension("txt")).ok()?;
        let v: Vec<u32> = t.split_whitespace().filter_map(|w| w.parse().ok()).collect();
        let (&[a, b, host, idle_frames], game) = (&v[..v.len().min(4)], v.get(4).map(|&g| g as u16)) else { return None };
        Some(Resume { save, armies: [a as u8, b as u8], host: host as u8, idle_frames, game })
    }
}

/// What a side has, for the campaign map.
#[derive(Clone, Default, Debug)]
pub struct SideStats {
    pub humans: Vec<String>,
    /// Supply used and provided (whole units, every race).
    pub supply: (i32, i32),
    pub workers: i32,
    pub minerals: i32,
    pub gas: i32,
    /// Minerals and gas gathered so far.
    pub mined: (i32, i32),
}

#[derive(Clone, Default, Debug)]
pub struct Stats {
    pub frame: u32,
    pub idle_frames: u32,
    pub sides: [SideStats; 2],
}

/// How a match ended: the winning side (0 / 1), or none (a draw, or it was stopped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    Won(usize),
    Draw,
    Stopped,
    /// The game could not start or went wrong.
    Failed,
}

pub enum Report {
    /// The join code of the match.
    Started(String),
    Stats(Stats),
    /// A faction chat line typed in the match: (faction, name, text).
    Chat(u8, String, String),
    Ended(End),
}

pub enum Order {
    Stop,
    /// Write the match's saved game to the path (and its seats next to it, `Resume::read`).
    Save(std::path::PathBuf),
    /// Pause (true) or resume the game (commands 0x10 / 0x11 in the host's turn).
    Pause(bool),
    /// Remove the humans of this name, telling them why.
    Kick(String, String),
    /// The minimap of side 0 / 1 as that side sees it (its vision), as a PNG.
    Minimap(usize, Sender<Vec<u8>>),
    /// A faction chat line for that faction's humans in this match.
    Say(u8, String),
}

/// In-game chat lines starting with this go to the whole faction (the web map and its
/// other matches, never the other side: net/live.rs `route_chat`); the rest stay in the match.
pub const FACTION_CHAT: char = crate::bw::enhanced::FACTION_CHAT as char;

/// Which faction a joiner of this name plays, given the match's two factions, or why it may
/// not. The campaign host's name registry.
pub type Allow = Box<dyn FnMut(&str, [u8; 2]) -> Result<u8, String> + Send>;

/// The two armies' slots and the host's own (an empty slot).
pub struct Seats {
    pub armies: [u8; 2],
    pub host: u8,
}

/// The melee setup of `spec`: the map's first two start slots are the computers of the
/// two sides (faction name, race and colour); the host takes the first empty slot.
pub fn setup(a: &view::Assets, spec: &Spec) -> Result<(view::GameSetup, Seats), String> {
    let chk = crate::net::live::load_chk(&spec.map).map_err(|e| format!("{}: {e}", spec.map))?;
    let (mut g, slots) = crate::net::live::melee_setup(a, chk, 2, spec.seed)?;
    let host = (0..8u8).find(|&p| g.types[p as usize] == 0).ok_or("no free slot for the host")?;
    for (i, s) in spec.sides.iter().enumerate() {
        let p = slots[i] as usize;
        g.types[p] = 1;
        g.names[p] = s.name.clone();
        g.races[p] = if s.race == 6 { ((spec.seed >> (8 * i)) % 3) as u8 } else { s.race };
        g.colors[p] = s.colour;
        // Own force each (the map's FORC may put both start slots in one: allies).
        g.forces[p] = i as u8 + 1;
    }
    // Alliances not allowed (template +0xf, 0x596874): 0x4c3ed0 would ally the two
    // computers, and sides must not ally across a border.
    g.template[0xf] = 0;
    g.local = host;
    g.local_human = host;
    g.names[host as usize] = "Faction".into();
    g.enhanced = true;
    g.multiplayer = true;
    g.mpq = crate::mpq::Mpq::open(spec.map.as_ref()).ok();
    Ok((g, Seats { armies: [slots[0], slots[1]], host }))
}

/// Link ids the joiners get (the host is 0): 30, or SCBW_CAMPAIGN_SLOTS (tests).
pub fn join_ids() -> Vec<u8> {
    let n = std::env::var("SCBW_CAMPAIGN_SLOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(30u8).clamp(1, 30);
    (1..=n).collect()
}

/// `scbw campaign match <map> [frames] [count]`: `count` matches (default 2) on `map` in
/// this process, each its own thread and lobby transport; prints `MATCH <border> <code>`
/// per match, `border B frame K hash H` at `frames` and how each ended.
pub fn test_matches(map: &str, k: Option<u32>, count: u16) -> Result<(), String> {
    crate::view::forbid_windows();
    let (tx, rx) = std::sync::mpsc::channel();
    let hub = Hub::new(0, None)?.0;
    let mut stops = Vec::new();
    for b in 0..count {
        let side = |f: u8| Side { faction: f, race: 6, colour: f, name: format!("Faction {f}") };
        let spec = Spec { border: b, game: b, map: map.to_string(), sides: [side(0), side(1)], seed: 1 + b as u32, resume: None };
        let (otx, orx) = std::sync::mpsc::channel();
        stops.push(otx);
        let tx = tx.clone();
        let (rtx, rrx) = std::sync::mpsc::channel();
        let hub = hub.clone();
        std::thread::spawn(move || {
            let r = run(spec, hub, rtx, orx, None, k);
            let _ = tx.send((b, r));
        });
        std::thread::spawn(move || {
            for r in rrx {
                match r {
                    Report::Started(code) => println!("MATCH {b} {code}"),
                    Report::Stats(s) if std::env::var_os("CAMPAIGN_STATS").is_some() => println!("border {b} {s:?}"),
                    Report::Chat(f, name, text) => println!("border {b} faction {f} chat {name}: {text}"),
                    _ => {}
                }
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
        });
    }
    for _ in 0..count {
        let (b, r) = rx.recv().map_err(|e| e.to_string())?;
        println!("border {b} ended: {r:?}");
        if k.is_some() {
            for s in &stops {
                let _ = s.send(Order::Stop);
            }
        }
    }
    Ok(())
}
const DELAY: u32 = 2;
/// A frame at Fastest; SCBW_NET_PACE_US paces slower (tests: a headless browser keeps up).
fn frame_time() -> Duration {
    std::env::var("SCBW_NET_PACE_US").ok().and_then(|v| v.parse().ok()).map_or(Duration::from_millis(42), Duration::from_micros)
}
const STATS_EVERY: Duration = Duration::from_secs(1);

/// Play `spec` until it ends or is stopped, its joiners coming through `hub` (the match's
/// code goes to `report` first); `report` gets its stats every second and the end. `hash_at`: print `border B frame K
/// hash H` at that frame (tests).
pub fn run(spec: Spec, hub: Hub, report: Sender<Report>, orders: Receiver<Order>, allow: Option<Allow>, hash_at: Option<u32>) -> End {
    // A panic in the game (a map our port mishandles) is a failed start, not a stuck border.
    let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play(&spec, &hub, &report, &orders, allow, hash_at)));
    let end = match played {
        Ok(Ok(e)) => e,
        Ok(Err(e)) => {
            eprintln!("campaign: border {}: {}: {e}", spec.border, spec.map);
            End::Failed
        }
        Err(p) => {
            let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            eprintln!("campaign: border {}: {}: the game panicked: {why}", spec.border, spec.map);
            End::Failed
        }
    };
    hub.close(spec.game);
    let _ = report.send(Report::Ended(end));
    end
}

fn play(spec: &Spec, hub: &Hub, report: &Sender<Report>, orders: &Receiver<Order>, allow: Option<Allow>, hash_at: Option<u32>) -> Result<End, String> {
    let files = crate::storage::Storage::open(&crate::install_dir()).map_err(|e| e.to_string())?;
    let mut a = view::Assets::load(files).map_err(|e| e.to_string())?;
    let (mut scene, seats) = match &spec.resume {
        Some(r) => {
            let a: &view::Assets = &a;
            let mut s = Scene::from_save_steps(a, &r.save, false).map_err(|e| e.to_string())?;
            view::with_data(a, |d| s.st.campaign_resume(d, r.armies, r.idle_frames));
            (s.st.local_player, s.st.local_human, s.local) = (r.host, r.host, r.host);
            s.st.player_names[r.host as usize] = "Faction".into();
            (s, Seats { armies: r.armies, host: r.host })
        }
        None => {
            let (mut g, seats) = setup(&a, spec)?;
            view::prepare_setup(&mut a, &mut g);
            let a: &view::Assets = &a;
            let mut s = Scene::from_setup(a, &g).map_err(|e| e.to_string())?;
            s.st.campaign = true;
            // Both armies count as the AI's: a joiner takes one over (State::enhanced_pick).
            s.st.ai_taken = 1 << seats.armies[0] | 1 << seats.armies[1];
            (s, seats)
        }
    };
    scene.live_net = true;
    let mut tr = hub.lobby(spec.game, 0, &join_ids());
    let _ = report.send(Report::Started(hub.code(spec.game)));
    let start = scene.st.frame;
    tr.set_first(start.max(DELAY));
    // The host plays from `start` (a resumed match: past 0); its own turns below the first it
    // sends (start + delay) are known empty, as a joiner's (`add_player`); else it waits on
    // itself forever.
    let mut se = Session::new_at(0, &[], DELAY, start);
    se.add_player(0, 0, seats.host, start + DELAY);
    let mut net = NetGame::new(se, tr, true, Vec::new())?;
    if let Some(mut allow) = allow {
        let (factions, armies) = (spec.sides.clone().map(|s| s.faction), seats.armies);
        net.join_filter = Some(Box::new(move |name: &[u8]| {
            let f = allow(&String::from_utf8_lossy(name), factions)?;
            factions.iter().position(|&x| x == f).map(|i| armies[i]).ok_or_else(|| "your faction is not in this match".to_string())
        }));
    }
    let frame_len = frame_time();
    let mut next = Instant::now();
    let mut last_stats = Instant::now() - STATS_EVERY;
    let mut last_humans: [Vec<String>; 2] = Default::default();
    let mut hashed = false;
    loop {
        match orders.try_recv() {
            Ok(Order::Stop) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(End::Stopped),
            Ok(Order::Save(path)) => save(&mut scene, &seats, spec.game, &path),
            Ok(Order::Pause(on)) => scene.st.send_command(&[if on { 0x10 } else { 0x11 }]),
            Ok(Order::Minimap(side, back)) => {
                let _ = back.send(minimap_png(&mut scene, seats.armies[side.min(1)]));
            }
            Ok(Order::Say(f, text)) => {
                // Storm messages to the faction's humans in this match (chat.rs 0x4c2420).
                if let Some(i) = spec.sides.iter().position(|s| s.faction == f) {
                    let to = net.links().iter().filter(|&&(_, h)| scene.st.nation_of(h) == seats.armies[i]).fold(0u32, |m, &(p, _)| m | 1 << p);
                    if to != 0 {
                        scene.chat.out.push((to, crate::chat::packet(0, text.as_bytes())));
                    }
                }
            }
            Ok(Order::Kick(name, why)) => {
                for (p, h) in net.links() {
                    if scene.st.human_name(h).eq_ignore_ascii_case(&name) {
                        eprintln!("campaign: border {}: {name} (link {p}) removed", spec.border);
                        net.kick(p, &why);
                    }
                }
            }
            Err(_) => {}
        }
        if let Some(why) = net.over() {
            return Err(why);
        }
        net.frame(&mut scene);
        // Chat the host got (no window shows it): faction lines go out.
        for (from, text) in std::mem::take(&mut scene.st.chat) {
            let Some(&(_, h)) = net.links().iter().find(|q| q.0 == from) else { continue };
            let text = String::from_utf8_lossy(text.split(|&c| c == 0).next().unwrap_or(&[])).into_owned();
            let army = scene.st.nation_of(h);
            if let (Some(t), Some(i)) = (text.strip_prefix(FACTION_CHAT), seats.armies.iter().position(|&a| a == army)) {
                let _ = report.send(Report::Chat(spec.sides[i].faction, scene.st.human_name(h).to_string(), t.trim().to_string()));
            }
        }
        for p in net.take_refused() {
            hub.drop_link(spec.game, p);
        }
        let (frame, confirmed) = net.frames();
        // The host runs past frame k: a multiple of 8 takes the confirmed frame's hash (the
        // desync check's); another the state's when confirmed reaches it (frame may be later).
        let known = hash_at.and_then(|k| if k % 8 == 0 { net.confirmed_hash(k) } else { (frame >= k && confirmed >= k).then(|| scene.st.state_hash()) });
        if let (Some(k), Some(h), false) = (hash_at, known, hashed) {
            hashed = true;
            println!("border {} frame {k} hash {h:016x}", spec.border);
            // SCBW_CAMPAIGN_STOP_AT_HASH (the pool sweep): the match is done there.
            if std::env::var_os("SCBW_CAMPAIGN_STOP_AT_HASH").is_some() {
                return Ok(End::Stopped);
            }
        }
        // Every second, and at once when a side's humans change (a leave frees the name for
        // another match: the host's "in use" check reads these).
        let s = stats(&scene.st, &seats);
        let humans = [s.sides[0].humans.clone(), s.sides[1].humans.clone()];
        if last_stats.elapsed() >= STATS_EVERY || humans != last_humans {
            last_stats = Instant::now();
            last_humans = humans;
            let _ = report.send(Report::Stats(s));
        }
        if let Some(e) = result(&scene.st, &seats) {
            eprintln!("campaign: border {} ends at frame {frame}: {e:?}", spec.border);
            return Ok(e);
        }
        next += frame_len;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > frame_len * 25 {
            next = now; // fell behind (a stall): no catching up in a burst
        }
    }
}

/// The minimap (0x4a4ce0's terrain, fog and units) of a copy of the state seen as `army`
/// (its vision mask 0x57f1ec), in the tileset's colours, as a PNG. Only the minimap's own
/// pixels change; the game does not.
fn minimap_png(scene: &mut Scene<'_>, army: u8) -> Vec<u8> {
    let mut st = scene.st.clone();
    st.local_player = army;
    st.local_vision_mask = st.player_vision[army as usize] as u8;
    let a = scene.a;
    let mm = &mut scene.minimap;
    view::with_data(a, |d| mm.refresh(d, &mut st));
    let name = crate::map::TILESETS[scene.st.tileset as usize % crate::map::TILESETS.len()];
    let wpe = a.files.read(&format!("tileset/{name}.wpe")).unwrap_or_default();
    let rgb: Vec<u8> = mm.pixels.iter().flat_map(|&i| wpe.get(i as usize * 4..i as usize * 4 + 3).unwrap_or(&[0, 0, 0]).to_vec()).collect();
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, mm.w as u32, mm.h as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = e.write_header() {
        let _ = w.write_image_data(&rgb);
    }
    out
}

/// `Order::Save`: a saved game of a copy of the state (saving changes a game, 0x4e3410: the
/// live one must stay as the joiners have it).
fn save(scene: &mut Scene<'_>, seats: &Seats, game: u16, path: &std::path::Path) {
    let live = scene.st.clone();
    let f = scene.save_file(b"campaign", true);
    let idle = live.idle_frames;
    scene.st = live;
    let ok = f.is_some_and(|f| std::fs::write(path, f).is_ok())
        && std::fs::write(path.with_extension("txt"), format!("armies {} {} host {} idle {idle} game {game}\n", seats.armies[0], seats.armies[1], seats.host)).is_ok();
    if !ok {
        eprintln!("campaign: could not save {}", path.display());
    }
}

/// The match's end from the armies' results (0x58d700): a victory, or both drawn, or one
/// defeated.
pub fn result(st: &State, seats: &Seats) -> Option<End> {
    let [a, b] = seats.armies.map(|p| st.player_left[p as usize]);
    match (a, b) {
        (5, 5) | (2, 2) => Some(End::Draw),
        (3, _) | (_, 2) => Some(End::Won(0)),
        (_, 3) | (2, _) => Some(End::Won(1)),
        _ => None,
    }
}

/// The workers' unit ids: SCV, drone, probe.
const WORKERS: [usize; 3] = [7, 41, 64];

pub fn stats(st: &State, seats: &Seats) -> Stats {
    let p = &st.players;
    let side = |n: u8| {
        let i = n as usize;
        let humans = (0..st.human_nation.len().min(32) as u8)
            .filter(|&h| st.nation_of(h) == n && st.humans_gone & 1 << h == 0 && (h >= 8 || st.player_types[h as usize] == 2))
            .map(|h| st.human_name(h).to_string())
            .collect();
        let half = |t: &[[i32; 12]; 3]| (0..3).map(|r| t[r][i]).sum::<i32>() / 2;
        SideStats {
            humans,
            supply: (half(&p.supply_used), half(&p.supply_provided).min(200)),
            workers: WORKERS.iter().map(|&u| p.unit_counts[u][i]).sum(),
            minerals: p.minerals[i],
            gas: p.gas[i],
            mined: (p.minerals_gathered[i], p.gas_gathered[i]),
        }
    };
    Stats { frame: st.frame, idle_frames: st.idle_frames, sides: seats.armies.map(side) }
}

/// `scbw campaign handoff <map> [at] [more]` (experiment): the AI's army and what a join does to
/// it. Three runs of one match: untouched; saved and loaded at frame `at` (what every peer does
/// when someone joins); saved, loaded and army 0 handed to a human. Per 120 frames: the army's
/// workers by order, and its buildings' build time left.
pub fn handoff_test(map: &str, at: u32, more: u32) -> Result<(), String> {
    crate::view::forbid_windows();
    let files = crate::storage::Storage::open(&crate::install_dir()).map_err(|e| e.to_string())?;
    let mut a = view::Assets::load(files).map_err(|e| e.to_string())?;
    let side = |f: u8| Side { faction: f, race: 6, colour: f, name: format!("Faction {f}") };
    let spec = Spec { border: 0, game: 0, map: map.to_string(), sides: [side(0), side(1)], seed: std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1), resume: None };
    let (mut g, seats) = setup(&a, &spec)?;
    view::prepare_setup(&mut a, &mut g);
    let a: &view::Assets = &a;
    for variant in 0..4 {
        let mut s = Scene::from_setup(a, &g).map_err(|e| e.to_string())?;
        s.st.campaign = true;
        s.st.ai_taken = 1 << seats.armies[0] | 1 << seats.armies[1];
        let n = seats.armies[0];
        println!("== variant {variant}: {}", ["untouched", "save+load", "save+load+join", "join, leave at +600, rejoin at +1000"][variant]);
        for f in 0..at + more {
            if f == at && variant >= 1 {
                println!("reload: {}", s.reload_through_save());
            }
            if variant == 3 && f == at + 600 {
                view::with_data(a, |d| {
                    s.st.cmd_player = Some(1);
                    s.st.leave_notice(d, None, n, 0x4000_0006);
                    s.st.cmd_player = None;
                });
            }
            if variant == 3 && f == at + 1000 {
                println!("reload: {}", s.reload_through_save());
                view::with_data(a, |d| s.st.enhanced_join(d, &crate::bw::enhanced::join_command(2, n, 2, b"joiner2")));
            }
            if f == at && variant >= 2 {
                view::with_data(a, |d| s.st.enhanced_join(d, &crate::bw::enhanced::join_command(1, n, 1, b"joiner")));
            }
            s.step();
            if f % 120 == 0 && f + 60 >= at {
                let (mut orders, mut left, mut bldg) = (std::collections::BTreeMap::<(u16, u8), u32>::new(), 0u32, 0u32);
                for un in s.st.units.iter().filter(|u| u.player == n && u.unit_type < 0xe4 && u.sprite != crate::bw::list::NIL) {
                    if matches!(un.unit_type, 0x07 | 0x29 | 0x40) {
                        *orders.entry((un.unit_type, un.order)).or_default() += 1;
                    }
                    if un.status & 1 == 0 {
                        bldg += 1;
                        left += un.remaining_build_time as u32;
                    }
                }
                for (i, un) in s.st.units.iter().enumerate().filter(|(_, u)| matches!(u.unit_type, 0x24 | 0x3b | 0x61) && u.sprite != crate::bw::list::NIL) {
                    println!("  f{f} morph unit {i} type {:#x} player {} slot {} queue {:x?}", un.unit_type, un.player, un.build_queue_slot, un.build_queue);
                }
                println!("f{f}: mined {} incomplete {bldg} left {left} workers {orders:?}", s.st.players.minerals_gathered[n as usize]);
            }
        }
    }
    Ok(())
}

/// `scbw campaign morph <map>` (experiment): a Creep Colony morphs into a Sunken Colony; its
/// queue, status and shown wireframe type every 20 frames.
pub fn morph_test(map: &str) -> Result<(), String> {
    crate::view::forbid_windows();
    let files = crate::storage::Storage::open(&crate::install_dir()).map_err(|e| e.to_string())?;
    let mut a = view::Assets::load(files).map_err(|e| e.to_string())?;
    let side = |f: u8| Side { faction: f, race: 0, colour: f, name: format!("Faction {f}") };
    let spec = Spec { border: 0, game: 0, map: map.to_string(), sides: [side(0), side(1)], seed: 3, resume: None };
    let (mut g, seats) = setup(&a, &spec)?;
    view::prepare_setup(&mut a, &mut g);
    let a: &view::Assets = &a;
    let mut s = Scene::from_setup(a, &g).map_err(|e| e.to_string())?;
    for _ in 0..200 {
        s.step();
    }
    let n = seats.armies[0];
    let (x, y) = s.st.start_locations[n as usize];
    let u = view::with_data(a, |d| {
        s.st.players.minerals[n as usize] = 5000;
        let u = s.st.create_unit(d, 0x8f, x as i16 + 160, y as i16, n).expect("colony");
        s.st.finish_unit_pre(d, u);
        s.st.finish_unit_post(d, u);
        let pool = s.st.create_unit(d, 0x8e, x as i16 + 160, y as i16 + 120, n).expect("pool");
        s.st.finish_unit_pre(d, pool);
        s.st.finish_unit_post(d, pool);
        u
    });
    for _ in 0..30 {
        s.step();
    }
    for f in 0..260 {
        if f == 10 {
            view::with_data(a, |d| s.st.command_building_morph(d, n, &[u], 0x92));
        }
        if f == 100 {
            println!("reload: {}", s.reload_through_save());
        }
        if f == 150 || f == 5 {
            // The status panel as the army's player sees it.
            s.st.local_player = n;
            s.local = n;
            s.st.selected = vec![u];
            let ui = crate::ui::Ui::load_with(&a.files, "zerg", true).map_err(|e| e.to_string())?;
            let (mut idx, mut out) = (vec![0u8; 640 * 480], vec![0u32; 640 * 480]);
            let (cx, cy) = (((x as i32 + 160 - 320).max(0)), ((y as i32 - 200).max(0)));
            s.st.screen_tile = ((cx / 32) as u16, (cy / 32) as u16);
            view::compose_with_cursor(&s, Some(&ui), cx, cy, None, None, &mut idx, &mut out);
            crate::write_png_rgb(&format!("/tmp/claude-1000/morph{f}.png"), 640, 480, &out).map_err(|e| e.to_string())?;
        }
        s.step();
        if f % 20 == 0 {
            let un = &s.st.units[u as usize];
            println!("f{f}: button_set {:#x} type {:#x} status {:#x} order {:#x} slot {} queue {:x?} wire {:#x} left {}", un.button_set, un.unit_type, un.status, un.order, un.build_queue_slot, un.build_queue, crate::ui::wire_type(&s.st, u), un.remaining_build_time);
        }
    }
    Ok(())
}

/// `scbw campaign switch <map> [cycles]` (experiment): army 0 goes AI -> human -> AI ... `cycles`
/// times, through the same commands a live game runs (join with its save and load, the human's
/// commands, the leave 0x57). The human stops two workers and sends two other units away; a line
/// per 240 frames, and a `BUG:` line for each check that fails at the end of a phase: a human's
/// army runs no AI script, builds nothing by itself and keeps its stopped workers idle; an AI
/// army runs its scripts, re-tasks the stopped workers, keeps building and spends, and has
/// taken every unit of the army (one frame after the take-over); a save and load that changes
/// the state hash is noted. Options (env):
/// - SWITCH_HUMAN / SWITCH_AI: phase lengths (default 1200 / 1800); the build and spend checks
///   need an AI phase of 1200 or more.
/// - SWITCH_SHARED=1: two humans join; the first leaves halfway (the army stays human), the
///   second at the end.
/// - SWITCH_BUSY=1: the human also trains a worker every 200 frames (a larva morphs for Zerg).
/// - SWITCH_RB=1: every join and leave is also replayed from a copy of the state taken before
///   it (as a rollback does); 60 frames later both must have the same state hash.
pub fn switch_test(map: &str, cycles: u32) -> Result<(), String> {
    use crate::bw::list::NIL;
    crate::view::forbid_windows();
    let files = crate::storage::Storage::open(&crate::install_dir()).map_err(|e| e.to_string())?;
    let mut a = view::Assets::load(files).map_err(|e| e.to_string())?;
    let side = |f: u8| Side { faction: f, race: 6, colour: f, name: format!("Faction {f}") };
    let spec = Spec { border: 0, game: 0, map: map.to_string(), sides: [side(0), side(1)], seed: std::env::var("SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1), resume: None };
    let (mut g, seats) = setup(&a, &spec)?;
    view::prepare_setup(&mut a, &mut g);
    let a: &view::Assets = &a;
    let mut s = Scene::from_setup(a, &g).map_err(|e| e.to_string())?;
    s.st.campaign = true;
    s.st.ai_taken = 1 << seats.armies[0] | 1 << seats.armies[1];
    let n = seats.armies[0];
    let start = std::env::var("SWITCH_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(3000u32);
    let env = |k: &str, v: u32| std::env::var(k).ok().and_then(|x| x.parse().ok()).unwrap_or(v);
    let (ai_len, human_len) = (env("SWITCH_AI", 1800), env("SWITCH_HUMAN", 1200));
    let (shared, busy, rb) = (env("SWITCH_SHARED", 0) != 0, env("SWITCH_BUSY", 0) != 0, env("SWITCH_RB", 0) != 0);
    let alive = |st: &State, u: usize| st.units[u].sprite != NIL && st.units[u].unit_type < 0xe4;
    let worker = |t: u16| matches!(t, 0x07 | 0x29 | 0x40);
    let harvesting = |o: u8| (0x4f..=0x5b).contains(&o);
    let scripts = |st: &State| {
        let (mut c, mut i) = (0, st.ai.active_scripts);
        while i != NIL {
            let sc = &st.ai.scripts.items[i as usize];
            c += (sc.player == n as u32 && sc.flags & 4 == 0) as u32;
            i = sc.link.next;
        }
        c
    };
    let towns = |st: &State| {
        let (mut c, mut i) = (0, st.ai.town_lists[n as usize]);
        while i != NIL {
            c += 1;
            i = st.ai.towns.items[i as usize].link.next;
        }
        c
    };
    // Buildings, finished or not (units.dat flag 1).
    let buildings = |st: &State| view::with_data(a, |d| (0..st.units.len()).filter(|&u| alive(st, u) && st.units[u].player == n && d.dat.units.flags[st.units[u].unit_type as usize] & 1 != 0).count());
    let bldg_ids = |st: &State| view::with_data(a, |d| (0..st.units.len()).filter(|&u| alive(st, u) && st.units[u].player == n && d.dat.units.flags[st.units[u].unit_type as usize] & 1 != 0).collect::<Vec<_>>());
    let mut ids0 = Vec::new();
    let units = |st: &State| (0..st.units.len()).filter(|&u| alive(st, u) && st.units[u].player == n).count();
    let mut bugs = 0;
    let mut bug = |f: u32, what: String| {
        println!("BUG: f{f}: {what}");
        bugs += 1;
    };
    let reload = |s: &mut Scene, f: u32, bug: &mut dyn FnMut(u32, String)| {
        let h0 = s.st.state_hash();
        let before = if std::env::var_os("SWITCH_DEBUG").is_some() { Some(s.st.clone()) } else { None };
        if !s.reload_through_save() {
            bug(f, "save and load failed".into());
        }
        let h1 = s.st.state_hash();
        // Not a desync (every peer loads the same save at a join), but the save leaves
        // something out.
        if h0 != h1 {
            println!("  f{f}: note: save and load changed the state hash {h0:x} -> {h1:x}");
            if let Some(old) = &before {
                let list = |st: &State, mask: u32| {
                    let mut v = Vec::new();
                    for first in [st.visible_units.head, st.hidden_units.head] {
                        let mut u = first;
                        while u != NIL {
                            let un = &st.units[u as usize];
                            v.push(format!("{u} t{:x} p{} hp{} sh{} en{} st{:x} o{:x}/{}/{} so{:x}/{} cd{}/{}/{} bt{} q{:x?} tp{:?} tu{:x} sub{:x} pos{:?} uid{}", un.unit_type, un.player, un.hp, un.shields, un.energy, un.status & mask, un.order, un.order_state, un.main_order_timer, un.secondary_order, un.secondary_order_state, un.ground_cooldown, un.air_cooldown, un.spell_cooldown, un.remaining_build_time, un.build_queue, un.order_target_pos, un.order_target_unit, un.subunit, (un.fl.exact, un.fl.heading, un.fl.speed, un.fl.velocity, un.fl.move_target, un.fl.next_waypoint, un.fl.flags, un.move_target_unit), un.unique_id));
                            u = un.link.next;
                        }
                        v.push("--".into());
                    }
                    v
                };
                // 0x4e2e20 (the load) refreshes the overlap status 0x40000 (0x42d9a0) of every
                // unit it puts back in the finder: expected, so left out of the comparison.
                let mask = !0x40000u32;
                let (x, y) = (list(old, mask), list(&s.st, mask));
                let diffs = x.iter().zip(&y).filter(|(p, q)| p != q).count();
                println!("    units {} -> {}; rng {} -> {}; {diffs} differ other than status 0x40000", x.len(), y.len(), old.rng.seed, s.st.rng.seed);
                for (i, (p, q)) in x.iter().zip(&y).enumerate().filter(|(_, (p, q))| p != q).take(6) {
                    println!("    #{i}\n      {p}\n      {q}");
                }
            }
        }
    };
    // (frame, human) of each phase start; human = None for an AI phase.
    let mut stopped: Vec<u32> = Vec::new();
    let mut sent: Vec<u32> = Vec::new();
    let (mut phase_end, mut human, mut cycle, mut b0) = (start, None::<u8>, 0u32, 0usize);
    let mut u0 = 0usize;
    let mut taken_at = None::<u32>;
    let mut m0 = 0i32;
    // Shared mode: (frame to check, A's two workers then B's two, where B sent them).
    let mut pair_check = None::<(u32, [u32; 4])>;
    // Over the window: which of the four took the Move order (6).
    let mut pair_moved = [false; 4];
    let mut f = 0u32;
    loop {
        if s.st.player_left[n as usize] != 0 {
            println!("  f{f}: army {n} is out of the game ({})", s.st.player_left[n as usize]);
            break;
        }
        if f == phase_end {
            let st = &s.st;
            match human {
                None if f > start && buildings(st) == 0 => println!("  f{f}: army {n} has no buildings left"),
                None if f > start => {
                    // End of an AI phase.
                    if scripts(st) == 0 {
                        bug(f, "AI army runs no script".into());
                    }
                    for &u in &stopped {
                        if alive(st, u as usize) && worker(st.units[u as usize].unit_type) && st.units[u as usize].order == 3 {
                            bug(f, format!("worker {u} the human stopped is not re-tasked (order {:#x}, ai {:x})", st.units[u as usize].order, st.units[u as usize].ai));
                        }
                    }
                    // An army losing units may only fight; one near the supply cap may only bank.
                    let r = st.player_races[n as usize].min(2) as usize;
                    let capped = st.players.supply_used[r][n as usize] >= 360;
                    if ai_len >= 1200 && !capped && buildings(st) == b0 && units(st) == u0 {
                        bug(f, format!("AI made nothing ({b0} -> {} buildings, {u0} -> {} units)", buildings(st), units(st)));
                    }
                    if ai_len >= 1200 && !capped && st.players.minerals[n as usize] > 1500 && st.players.minerals[n as usize] > m0 + 300 {
                        bug(f, format!("AI hoards {} minerals", st.players.minerals[n as usize]));
                    }
                }
                Some(h) => {
                    if scripts(st) != 0 {
                        bug(f, format!("human army still runs {} AI scripts", scripts(st)));
                    }
                    if buildings(st) > b0 && !busy {
                        let new: Vec<_> = bldg_ids(st).into_iter().filter(|u| !ids0.contains(u)).map(|u| (u, st.units[u].unit_type, st.units[u].status & 1)).collect();
                        bug(f, format!("human army built by itself ({b0} -> {} buildings: new (unit, type, done) {new:x?})", buildings(st)));
                    }
                    for &u in &stopped {
                        if alive(st, u as usize) && harvesting(st.units[u as usize].order) {
                            bug(f, format!("worker {u} the human stopped went back to work (order {:#x})", st.units[u as usize].order));
                        }
                    }
                    for &u in &sent {
                        let un = &st.units[u as usize];
                        if alive(st, u as usize) && un.order != 0x03 && un.order != 0x0a {
                            println!("  f{f}: unit {u} the human sent has order {:#x}", un.order);
                        }
                    }
                    let _ = h;
                }
                None => {}
            }
            if cycle == cycles && human.is_none() && f > start {
                break;
            }
            b0 = buildings(&s.st);
            u0 = units(&s.st);
            m0 = s.st.players.minerals[n as usize];
            ids0 = bldg_ids(&s.st);
            if std::env::var_os("SWITCH_DEBUG").is_some() {
                for u in 0..s.st.units.len() {
                    let un = &s.st.units[u];
                    if alive(&s.st, u) && un.player == n && worker(un.unit_type) && !harvesting(un.order) {
                        let mut q = Vec::new();
                        let mut o = un.order_queue_head;
                        while o != NIL {
                            q.push(s.st.orders[o as usize].order);
                            o = s.st.orders[o as usize].link.next;
                        }
                        println!("  f{f}: worker {u:x} order {:#x} queue {q:x?} target {:?} status {:x}", un.order, un.order_target_pos, un.status);
                    }
                }
            }
            // The join or leave; with SWITCH_RB run twice from the same copy (a rollback).
            let joining = human.is_none();
            if joining {
                cycle += 1;
            }
            let h = if joining { 8 + cycle as u8 } else { human.unwrap() };
            let transition = |s: &mut Scene, bug: &mut dyn FnMut(u32, String)| {
                if joining {
                    // A human joins (every peer saves and loads first).
                    reload(s, f, bug);
                    s.human_command(h, &crate::bw::enhanced::join_command(h, n, h, b"joiner"));
                    if shared {
                        s.human_command(h + 16, &crate::bw::enhanced::join_command(h + 16, n, h + 16, b"second"));
                    }
                } else {
                    s.human_command(h, &[0x57, 1]);
                }
            };
            if rb {
                let copy = s.st.clone();
                let mut quiet = |_: u32, _: String| {};
                transition(&mut s, &mut quiet);
                for _ in 0..60 {
                    s.step();
                }
                let first = s.st.state_hash();
                s.st = copy;
                transition(&mut s, &mut bug);
                for _ in 0..60 {
                    s.step();
                }
                if s.st.state_hash() != first {
                    bug(f, format!("{} replayed from a copy differs 60 frames on", if joining { "the join" } else { "the leave" }));
                }
                // The 60 frames are part of the next phase.
                f += 60;
            } else {
                transition(&mut s, &mut bug);
            }
            println!("== f{f}: human {h} {} army {n} (types {:?}, taken {:#x})", if joining { "joins" } else { "leaves" }, &s.st.player_types[..8], s.st.ai_taken);
            if joining {
                human = Some(h);
                phase_end = f + human_len;
            } else {
                human = None;
                phase_end = f + ai_len;
                taken_at = Some(f + 1);
            }
        }
        if taken_at == Some(f) {
            // One frame after a take-over every unit of the army is the AI's, but for units in
            // training (they join when finished, 0x4a2830), dying, in a transport, and subunits.
            let st = &s.st;
            let flag = |u: usize, f: u32| view::with_data(a, |d| d.dat.units.flags[st.units[u].unit_type as usize] & f != 0);
            let free: Vec<_> = (0..st.units.len()).filter(|&u| alive(st, u) && st.units[u].player == n && st.units[u].ai == NIL && st.units[u].order != 0 && st.units[u].status & 0x40 == 0 && !flag(u, 0x10) && (st.units[u].status & 1 != 0 || flag(u, 1))).map(|u| (u, st.units[u].unit_type, st.units[u].status & 1)).collect();
            if !free.is_empty() {
                bug(f, format!("units without AI after the take-over (unit, type, done): {free:x?}"));
            }
            if std::env::var_os("SWITCH_DEBUG").is_some() {
                view::with_data(a, |d| {
                    for u in 0..st.units.len() {
                        let un = &st.units[u];
                        if alive(st, u) && un.player == n && worker(un.unit_type) && !harvesting(un.order) {
                            let t = un.unit_type as usize;
                            println!("  f{f}: worker {u} order {:#x} queue {:x} idle human {:#x} return {:#x} computer {:#x} ai {:x}", un.order, un.order_queue_head, d.dat.units.ai_human_idle[t], d.dat.units.ai_return_to_idle[t], d.dat.units.ai_computer_idle[t], un.ai);
                        }
                    }
                });
            }
            taken_at = None;
        }
        if shared && human.is_some() && phase_end - f == human_len / 2 {
            // The first human leaves: the army keeps its second human.
            let h = human.unwrap();
            s.human_command(h, &[0x57, 1]);
            println!("== f{f}: human {h} leaves, human {} stays (types {:?}, taken {:#x})", h + 16, &s.st.player_types[..8], s.st.ai_taken);
            human = Some(h + 16);
            if s.st.player_types[n as usize] != 2 || s.st.ai_taken & 1 << n != 0 {
                bug(f, "the AI took an army that still has a human".into());
            }
        }
        if busy && human.is_some() && (phase_end - f) % 200 == 100 {
            // The human trains a worker: a town hall trains one, a Zerg larva morphs.
            let h = human.unwrap();
            let st = &s.st;
            let race = st.player_races[n as usize];
            let pick = (0..st.units.len() as u32).find(|&u| alive(st, u as usize) && st.units[u as usize].player == n && st.units[u as usize].status & 1 != 0 && if race == 0 { st.units[u as usize].unit_type == 0x23 } else { matches!(st.units[u as usize].unit_type, 0x6a | 0x9a) });
            if let Some(u) = pick {
                let mut c = vec![0x09, 1];
                c.extend_from_slice(&(st.unit_handle(u) as u16).to_le_bytes());
                s.human_command(h, &c);
                let t: u16 = [0x29, 7, 0x40][race.min(2) as usize];
                let mut c = vec![if race == 0 { 0x23 } else { 0x1f }];
                c.extend_from_slice(&t.to_le_bytes());
                s.human_command(h, &c);
            }
        }
        if let Some((_, w)) = pair_check {
            for (k, &u) in w.iter().enumerate() {
                pair_moved[k] |= s.st.units[u as usize].order == 6;
            }
        }
        if let Some((at, w)) = pair_check.filter(|q| q.0 == f) {
            pair_check = None;
            let st = &s.st;
            let live = |u: u32| alive(st, u as usize) && st.units[u as usize].order != 0;
            let o = |u: u32| st.units[u as usize].order;
            // A's: stopped (PlayerGuard), never moved; B's: took the move, not mining after.
            for (k, &u) in w[..2].iter().enumerate() {
                if live(u) && (o(u) != 3 || pair_moved[k]) {
                    bug(at, format!("worker {u} human A stopped has order {:#x} (moved: {})", o(u), pair_moved[k]));
                }
            }
            for (k, &u) in w.iter().enumerate().skip(2) {
                if live(u) && (harvesting(o(u)) || !pair_moved[k]) {
                    bug(at, format!("worker {u} human B moved has order {:#x} (moved: {})", o(u), pair_moved[k]));
                }
            }
        }
        if let (true, Some(h)) = (shared, human.filter(|_| phase_end - f == human_len - 60)) {
            // Two humans, one army, each with its own selection: A selects two mining workers
            // and B two others; A's Stop stops A's only, then B's move moves B's only.
            let (ha, hb) = (h, h + 16);
            let st = &s.st;
            let w: Vec<u32> = (0..st.units.len() as u32).filter(|&u| alive(st, u as usize) && st.units[u as usize].player == n && worker(st.units[u as usize].unit_type) && matches!(st.units[u as usize].order, 0x55..=0x57) && !stopped.contains(&u)).take(4).collect();
            if w.len() == 4 {
                let sel = |l: &[u32]| {
                    let mut c = vec![0x09, l.len() as u8];
                    for &u in l {
                        c.extend_from_slice(&(st.unit_handle(u) as u16).to_le_bytes());
                    }
                    c
                };
                let (ca, cb) = (sel(&w[..2]), sel(&w[2..]));
                // B sends its pair to the army's start location (always reachable from its base).
                let (mx, my) = st.start_locations[n as usize];
                s.human_command(ha, &ca);
                s.human_command(hb, &cb);
                s.human_command(ha, &[0x1a, 0]);
                let mut c = vec![0x14];
                for v in [mx, my, 0, 0xe4] {
                    c.extend_from_slice(&v.to_le_bytes());
                }
                c.push(0);
                s.human_command(hb, &c);
                // Mining steps finish first (not interruptible): checked 150 frames on.
                pair_check = Some((f + 150, [w[0], w[1], w[2], w[3]]));
                pair_moved = [false; 4];
                println!("  f{f}: human {ha} stopped {:?}, human {hb} moved {:?}", &w[..2], &w[2..]);
                stopped.extend_from_slice(&w[..2]);
            }
        }
        if let Some(h) = human {
            if phase_end - f == human_len - 24 {
                // The human's commands: two workers stopped, two non-workers sent to the map centre.
                let st = &s.st;
                let mine: Vec<u32> = (0..st.units.len() as u32).filter(|&u| alive(st, u as usize) && st.units[u as usize].player == n && st.units[u as usize].status & 1 != 0).collect();
                stopped = mine.iter().copied().filter(|&u| worker(st.units[u as usize].unit_type) && matches!(st.units[u as usize].order, 0x55..=0x57)).take(2).collect();
                sent = mine.iter().copied().filter(|&u| st.units[u as usize].status & 2 == 0 && !worker(st.units[u as usize].unit_type) && !matches!(st.units[u as usize].unit_type, 0x23 | 0x24)).take(2).collect();
                let sel = |l: &[u32]| {
                    let mut c = vec![0x09, l.len() as u8];
                    for &u in l {
                        c.extend_from_slice(&(st.unit_handle(u) as u16).to_le_bytes());
                    }
                    c
                };
                let (cs, cm) = (sel(&stopped), sel(&sent));
                let (mx, my) = (st.map_px.0 / 2, st.map_px.1 / 2);
                println!("  f{f}: human stops workers {stopped:?}, sends {sent:?} to ({mx}, {my})");
                if !stopped.is_empty() {
                    s.human_command(h, &cs);
                    s.human_command(h, &[0x1a, 0]);
                }
                if !sent.is_empty() {
                    s.human_command(h, &cm);
                    let mut c = vec![0x14];
                    for v in [mx, my, 0, 0xe4] {
                        c.extend_from_slice(&v.to_le_bytes());
                    }
                    c.push(0);
                    s.human_command(h, &c);
                }
            }
        }
        s.step();
        f += 1;
        // A unit that dies (order 0) leaves the lists: its slot may hold a new unit later.
        for l in [&mut stopped, &mut sent] {
            l.retain(|&u| alive(&s.st, u as usize) && s.st.units[u as usize].order != 0);
        }
        if let Some(w) = std::env::var("SWITCH_WATCH").ok().and_then(|v| v.parse::<usize>().ok()) {
            let un = &s.st.units[w];
            println!("  f{f}: watch {w} type {:x} player {} order {:x}/{} status {:x} ai {:x} sprite {:x}", un.unit_type, un.player, un.order, un.order_state, un.status, un.ai, un.sprite);
        }
        if human.is_some() && std::env::var_os("SWITCH_TRACE").is_some() {
            for &u in &stopped {
                let un = &s.st.units[u as usize];
                if f % 8 == 0 || un.order != 3 {
                    if un.order != 3 {
                        println!("  f{f}: stopped worker {u} order {:#x}/{} queue {:x} ai {:x} status {:x}", un.order, un.order_state, un.order_queue_head, un.ai, un.status);
                    }
                }
            }
        }
        if f % 240 == 0 && f >= start - 240 {
            let st = &s.st;
            let mut orders = std::collections::BTreeMap::<u8, u32>::new();
            for u in 0..st.units.len() {
                if alive(st, u) && st.units[u].player == n && worker(st.units[u].unit_type) {
                    *orders.entry(st.units[u].order).or_default() += 1;
                }
            }
            let units = units(st);
            let no_ai = (0..st.units.len()).filter(|&u| alive(st, u) && st.units[u].player == n && st.units[u].ai == NIL && st.units[u].status & 1 != 0).count();
            if std::env::var_os("SWITCH_DEBUG").is_some() {
                let mut i = st.ai.active_scripts;
                while i != NIL {
                    let sc = &st.ai.scripts.items[i as usize];
                    if sc.player == n as u32 {
                        println!("  script {i}: pos {:#x} wait {} town {:x} flags {:#x}", sc.pos, sc.wait, sc.town, sc.flags);
                    }
                    i = sc.link.next;
                }
                let mut t = st.ai.town_lists[n as usize];
                while t != NIL {
                    let tw = &st.ai.towns.items[t as usize];
                    println!("  town {t}: inited {} workers {} limit {} main {:x} builder {:x} pos {:?} area {} (pos {:?} geysers {}) gas {:x?}", tw.inited, tw.worker_count, tw.worker_limit, tw.main_building, tw.building_scv, tw.position, tw.resource_area, st.areas[tw.resource_area as usize].pos, st.areas[tw.resource_area as usize].geysers, tw.gas_buildings);
                    let used: Vec<_> = tw.town_units.iter().take_while(|e| e.0 & 0xf8 != 0).map(|e| (e.0 >> 3, e.1, e.2)).collect();
                    println!("    town units (count, priority, id): {used:x?}");
                    t = tw.link.next;
                }
                for u in 0..st.units.len() {
                    let un = &st.units[u];
                    if alive(st, u) && un.player == n && un.status & 2 != 0 {
                        println!("  bldg {u} type {:#x} order {:#x} sec {:#x} ai {:x} queue {:x?} timer {} pos {:?} status {:x}", un.unit_type, un.order, un.secondary_order, un.ai, un.build_queue, un.main_order_timer, un.fl.pos, un.status);
                    }
                }
                let pa = &st.ai.players[n as usize];
                println!("  requests {} {:?} flags {:#x}", pa.request_count, &pa.requests[..pa.request_count as usize], pa.flags);
            }
            println!("f{f}: {} type {} scripts {} towns {} units {units} (no AI {no_ai}) buildings {} minerals {} supply {}/{} worker orders {orders:x?}", if human.is_some() { "human" } else { "AI   " }, st.player_types[n as usize], scripts(st), towns(st), buildings(st), st.players.minerals[n as usize], st.players.supply_used[st.player_races[n as usize].min(2) as usize][n as usize], st.players.supply_provided[st.player_races[n as usize].min(2) as usize][n as usize]);
        }
    }
    println!("{bugs} bugs");
    Ok(())
}
