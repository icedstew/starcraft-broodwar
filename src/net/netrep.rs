//! `scbw netrep` (agent/areas/net.md, phase 3a): the nettest check across processes over
//! real QUIC on localhost. Each process loads the replay, plays one replay player's
//! recorded commands as its local input in a rollback `Session`, exchanges turns through
//! `transport::Transport`, and prints `MATCH`/`DIFF` for its final state hash against a
//! straight playback's. `all` spawns the host and one join per other player.

use super::transport::{Event, Transport};
use super::{Session, plan};
use crate::{replay, view};
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

/// How long a finished peer keeps serving datagrams for the others.
const LINGER: Duration = Duration::from_secs(2);

pub enum Role<'a> {
    Host { port: u16 },
    Join { target: &'a str, player: u8 },
}

/// Play one peer to the replay's end; returns its result line.
pub fn run_peer(a: &view::Assets, r: &replay::Replay, delay: u32, role: Role) -> std::io::Result<String> {
    let delay = delay.max(1);
    let all: Vec<u8> = r.header.slots.iter().filter(|s| s.kind == 1 || s.kind == 2).map(|s| s.slot_id as u8).collect();
    let host = matches!(role, Role::Host { .. });
    let (mut tr, me) = match role {
        Role::Host { port } => {
            let me = *all.iter().min().expect("a player");
            let clients: Vec<u8> = all.iter().copied().filter(|&p| p != me).collect();
            let (tr, target) = Transport::host(port, me, &clients, delay, Vec::new()).map_err(std::io::Error::other)?;
            println!("LISTEN {target}");
            std::io::stdout().flush()?;
            (tr, me)
        }
        Role::Join { target, player } => (Transport::join(target, player, delay).map_err(std::io::Error::other)?, player),
    };
    let p = match plan(a, r, delay)? {
        Ok(p) => p,
        Err(m) => return Ok(m),
    };
    let Some(mi) = p.players.iter().position(|x| x.0 == me) else { return Ok(format!("INVALID: no player {me}")) };
    let mut scene = view::Scene::from_replay(a, r)?;
    scene.st = p.start.clone();
    scene.local = me;
    scene.st.local_player = me;
    let mut se = Session::new(me, &p.players, delay);
    se.end = p.end;
    let end = p.end;
    let nap = || std::thread::sleep(Duration::from_millis(1));
    // Lobby: the host starts everyone once all said hello.
    let mut early = Vec::new();
    let mut started = false;
    while !started {
        for e in tr.poll() {
            match e {
                Event::Start { .. } => started = true,
                Event::Error(m) => return Err(std::io::Error::other(m)),
                Event::Turn { player, frame, bytes } => early.push((player, frame, bytes)),
                _ => {}
            }
        }
        nap();
    }
    // Turns that beat the Start message here are not delivered again.
    for (player, frame, bytes) in early {
        se.remote_turn(player, frame, bytes);
    }
    let mut next_send = se.delay;
    let verbose = std::env::var_os("NETREP_DEBUG").is_some();
    let mut last_report = Instant::now();
    while !(se.frame >= end && se.confirmed >= end) {
        if se.frame + se.delay == next_send && se.frame < end {
            next_send += 1;
            let bytes = p.turns[mi].get(se.frame as usize + se.delay as usize).cloned().unwrap_or_default();
            let (f, b) = se.local_turn(bytes);
            tr.send_turns(f, b);
        }
        for e in tr.poll() {
            match e {
                Event::Turn { player, frame, bytes } => {
                    if verbose && frame < 6 {
                        eprintln!("netrep player {me}: turn p{player} f{frame} {} bytes", bytes.len());
                    }
                    se.remote_turn(player, frame, bytes)
                }
                Event::Error(m) => eprintln!("netrep player {me}: {m}"),
                _ => {}
            }
        }
        if verbose && last_report.elapsed() > Duration::from_secs(2) {
            last_report = Instant::now();
            eprintln!("netrep player {me}: frame {} confirmed {} stalled {} rollbacks {} dgrams {:?} cap {}", se.frame, se.confirmed, se.stalled, se.rollbacks, tr.stats, tr.cap());
        }
        if !se.tick(&mut scene) {
            nap();
        }
    }
    let h = scene.st.state_hash();
    // Keep serving until the peers have the last turns (a client: the host acked mine).
    let t0 = Instant::now();
    while t0.elapsed() < LINGER && !(tr.fully_acked(end + se.delay) && (host || t0.elapsed() > Duration::from_millis(300))) {
        tr.poll();
        nap();
    }
    Ok(format!(
        "{} f{end} player {me} rollbacks {} stalled {} hash {h:016x}{}",
        if h == p.want { "MATCH" } else { "DIFF" },
        se.rollbacks,
        se.stalled,
        if h == p.want { String::new() } else { format!(" want {:016x}", p.want) }
    ))
}

/// `netrep <rep> all`: spawn this executable as the host and one join per other player.
/// Returns (report, all matched).
pub fn run_all(rep: &str, port: u16, delay: u32) -> std::io::Result<(String, bool)> {
    let r = replay::Replay::parse(&std::fs::read(rep)?)?;
    let mut all: Vec<u8> = r.header.slots.iter().filter(|s| s.kind == 1 || s.kind == 2).map(|s| s.slot_id as u8).collect();
    all.sort();
    let deadline = Instant::now() + Duration::from_millis(r.header.frames as u64 * 42 * 3) + Duration::from_secs(60);
    let exe = std::env::current_exe()?;
    let mut host = std::process::Command::new(&exe)
        .args(["netrep", rep, "host", &port.to_string(), &delay.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let mut hout = std::io::BufReader::new(host.stdout.take().unwrap());
    let mut first = String::new();
    hout.read_line(&mut first)?;
    let Some(target) = first.trim().strip_prefix("LISTEN ").map(str::to_string) else {
        let _ = host.kill();
        return Ok((format!("host failed to start: {first:?}"), false));
    };
    let mut kids = vec![(all[0], host)];
    for &p in &all[1..] {
        let c = std::process::Command::new(&exe)
            .args(["netrep", rep, "join", &target, &p.to_string(), &delay.to_string()])
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        kids.push((p, c));
    }
    let mut ok = true;
    let mut out = String::new();
    let mut waiting = vec![true; kids.len()];
    while waiting.iter().any(|&w| w) {
        if Instant::now() > deadline {
            for (i, (_, k)) in kids.iter_mut().enumerate() {
                if waiting[i] {
                    let _ = k.kill();
                }
            }
            out += "\n  TIMEOUT";
            ok = false;
            break;
        }
        for (i, (_, k)) in kids.iter_mut().enumerate() {
            if waiting[i] && let Some(st) = k.try_wait()? {
                waiting[i] = false;
                ok &= st.success();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for (i, (p, k)) in kids.iter_mut().enumerate() {
        let mut s = String::new();
        if i == 0 {
            hout.read_to_string(&mut s)?;
        } else if let Some(mut o) = k.stdout.take() {
            o.read_to_string(&mut s)?;
        }
        let line = s.lines().last().unwrap_or("(no output)");
        ok &= line.starts_with("MATCH");
        out += &format!("\n  [{}] {line}", if i == 0 { format!("host {p}") } else { format!("join {p}") });
    }
    Ok((format!("{} {rep} delay {delay}{out}", if ok { "MATCH" } else { "DIFF" }), ok))
}
