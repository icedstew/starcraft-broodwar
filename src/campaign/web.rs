//! The campaign's web map (ours): a small HTTP server on the campaign host (std only, a
//! thread per request). `/` is the map page (campaign.html); it polls `/state?name=` (JSON:
//! the world, the live borders, the stats and join codes of the viewer's faction only,
//! filtered here and not in the page), shows `/minimap?border=&name=` (a PNG of the match
//! as the viewer's side sees it), registers a name with `/join?name=&faction=` and sends
//! faction chat with `/chat?name=` (the text as the body). `/admin?token=&cmd=` runs a host
//! command (host.rs) with the token the host printed. `/play/...` serves the browser build
//! (`web/`, or SCBW_CAMPAIGN_PLAY) so the page's Join can open it.

use super::host::Shared;
use super::runner::Order;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A host command from the admin view and where its answer goes.
pub type Admin = (String, Sender<String>);

/// Most chat lines kept.
const CHAT_KEEP: usize = 200;

#[derive(Clone)]
pub struct Web {
    pub shared: Arc<Mutex<Shared>>,
    pub admin: Sender<Admin>,
    pub token: String,
    /// The browser build's directory, and the URL its join links start with.
    pub play_dir: PathBuf,
    pub play_url: String,
}

/// Serve on `port` (bound to SCBW_NET_BIND as the hub, default 127.0.0.1); returns the port.
pub fn serve(web: Web, port: u16) -> Result<u16, String> {
    let bind = std::env::var("SCBW_NET_BIND").unwrap_or_else(|_| "127.0.0.1".into());
    let l = TcpListener::bind((bind.as_str(), port)).map_err(|e| format!("web map: {e}"))?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::Builder::new()
        .name("web map".into())
        .spawn(move || {
            for s in l.incoming().flatten() {
                let w = web.clone();
                std::thread::spawn(move || {
                    let _ = w.handle(s);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// `%xx` and `+` decoded.
fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A JSON string.
fn js(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Names: printable, up to 0x17 bytes (the join command's name field), no spaces.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 0x17 && n.chars().all(|c| c.is_ascii_graphic())
}

impl Web {
    fn handle(&self, s: TcpStream) -> std::io::Result<()> {
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut r = BufReader::new(s.try_clone()?);
        let mut line = String::new();
        r.read_line(&mut line)?;
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0).min(4096);
            }
        }
        let mut body = vec![0u8; len];
        r.read_exact(&mut body)?;
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let q = |k: &str| query.split('&').filter_map(|p| p.split_once('=')).find(|p| p.0 == k).map(|p| unquote(p.1)).unwrap_or_default();
        let (status, ty, out): (&str, &str, Vec<u8>) = match path {
            "/" => ("200 OK", "text/html; charset=utf-8", include_str!("campaign.html").replace("PLAY_URL", &js(&self.play_url)).into_bytes()),
            "/state" => ("200 OK", "application/json", self.state(&q("name")).into_bytes()),
            "/matches" => ("200 OK", "text/plain; charset=utf-8", self.matches(&q("name")).into_bytes()),
            "/minimap" => match self.minimap(q("border").parse().unwrap_or(u16::MAX), &q("name")) {
                Some(p) => ("200 OK", "image/png", p),
                None => ("404 Not Found", "text/plain", b"no minimap".to_vec()),
            },
            "/join" => self.text(self.register(&q("name"), q("faction").parse().unwrap_or(255))),
            "/chat" => self.text(self.chat(&q("name"), String::from_utf8_lossy(&body).trim())),
            "/admin" if !self.token.is_empty() && q("token") == self.token => {
                let (tx, rx) = mpsc::channel();
                let _ = self.admin.send((q("cmd"), tx));
                self.text(rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "no answer".to_string()))
            }
            p if p.starts_with("/play/") => self.file(&p["/play/".len()..]),
            _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
        };
        let mut s = s;
        write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", out.len())?;
        s.write_all(&out)
    }

    fn text(&self, r: Result<String, String>) -> (&'static str, &'static str, Vec<u8>) {
        match r {
            Ok(t) => ("200 OK", "text/plain; charset=utf-8", t.into_bytes()),
            Err(t) => ("400 Bad Request", "text/plain; charset=utf-8", t.into_bytes()),
        }
    }

    /// A file of the browser build (no `..`, no absolute paths).
    fn file(&self, rel: &str) -> (&'static str, &'static str, Vec<u8>) {
        let rel = if rel.is_empty() { "index.html" } else { rel };
        if rel.split('/').any(|c| c == ".." || c.is_empty()) || rel.contains('\\') {
            return ("404 Not Found", "text/plain", b"not found".to_vec());
        }
        let ty = match rel.rsplit('.').next() {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "text/javascript",
            Some("wasm") => "application/wasm",
            Some("json") => "application/json",
            _ => "application/octet-stream",
        };
        match std::fs::read(self.play_dir.join(rel)) {
            Ok(b) => ("200 OK", ty, b),
            Err(_) => ("404 Not Found", "text/plain", b"not found".to_vec()),
        }
    }

    /// `name` plays `faction` from now on: a new name, or one whose faction is out while
    /// the host lets eliminated players pick again.
    fn register(&self, name: &str, faction: u8) -> Result<String, String> {
        if !valid_name(name) {
            return Err("a name is 1 to 23 letters, digits or signs, no spaces".into());
        }
        let mut sh = self.shared.lock().unwrap();
        if sh.players.is_banned(name) {
            return Err("banned".into());
        }
        if faction as usize >= sh.world.factions.len() || sh.world.areas_of(faction) == 0 {
            return Err("no such faction in the campaign".into());
        }
        match sh.players.of(name) {
            Some(f) if f == faction => Ok("ok".into()),
            Some(f) if sh.world.areas_of(f) == 0 && sh.world.eliminated_pick => {
                sh.players.faction.retain(|n| !n.0.eq_ignore_ascii_case(name));
                sh.players.faction.push((name.to_string(), faction));
                Ok("ok".into())
            }
            Some(_) => Err("that name plays for another faction".into()),
            None => {
                sh.players.faction.push((name.to_string(), faction));
                Ok("ok".into())
            }
        }
    }

    /// A faction chat line: shown on the map to the faction, and in its matches.
    fn chat(&self, name: &str, text: &str) -> Result<String, String> {
        let mut sh = self.shared.lock().unwrap();
        let f = sh.players.of(name).filter(|_| !sh.players.is_banned(name)).ok_or("join a faction first")?;
        let text: String = text.chars().filter(|c| !c.is_control()).take(120).collect();
        if text.is_empty() {
            return Ok("".into());
        }
        sh.chat.push((f, name.to_string(), text.clone()));
        if sh.chat.len() > CHAT_KEEP {
            sh.chat.remove(0);
        }
        for l in sh.live.values().filter(|l| l.spec.sides.iter().any(|s| s.faction == f)) {
            let _ = l.orders.send(Order::Say(f, format!("{name}: {text}")));
        }
        Ok("ok".into())
    }

    /// The viewer's faction's live matches, a line each: `border code humans frame supply
    /// most workers map<TAB>area - area` (the .exe's join list, net/snet.rs `campaign_games`).
    fn matches(&self, name: &str) -> String {
        let sh = self.shared.lock().unwrap();
        let Some(v) = Self::viewer(&sh, name) else { return String::new() };
        let mut out = String::new();
        let mut live: Vec<_> = sh.live.iter().collect();
        live.sort_by_key(|l| l.0);
        for (&b, l) in live {
            let (Some(i), Some(code)) = (l.spec.sides.iter().position(|s| s.faction == v), &l.code) else { continue };
            let br = &sh.world.borders[b as usize];
            let x = &l.stats.sides[i];
            let map = l.spec.map.rsplit(['/', '\\']).next().unwrap_or("").replace(['\t', '\n'], " ");
            let (a, z) = (&sh.world.areas[br.a as usize].name, &sh.world.areas[br.b as usize].name);
            out += &format!("{b} {code} {} {} {} {} {} {map}\t{a} - {z}\n", x.humans.len(), l.stats.frame, x.supply.0, x.supply.1, x.workers);
        }
        out
    }

    /// The viewer's faction (a registered, not banned name).
    fn viewer(sh: &Shared, name: &str) -> Option<u8> {
        sh.players.of(name).filter(|_| !sh.players.is_banned(name))
    }

    fn minimap(&self, border: u16, name: &str) -> Option<Vec<u8>> {
        let rx = {
            let sh = self.shared.lock().unwrap();
            let v = Self::viewer(&sh, name)?;
            let l = sh.live.get(&border)?;
            let side = l.spec.sides.iter().position(|s| s.faction == v)?;
            let (tx, rx) = mpsc::channel();
            l.orders.send(Order::Minimap(side, tx)).ok()?;
            rx
        };
        rx.recv_timeout(Duration::from_secs(3)).ok()
    }

    /// The JSON the page draws; what the viewer's faction may not see is left out.
    fn state(&self, name: &str) -> String {
        let sh = self.shared.lock().unwrap();
        let w = &sh.world;
        let v = Self::viewer(&sh, name);
        let factions: Vec<String> = w.factions.iter().enumerate().map(|(i, f)| format!("{{\"name\":{},\"race\":{},\"colour\":{},\"areas\":{}}}", js(&f.name), f.race, f.colour, w.areas_of(i as u8))).collect();
        let areas: Vec<String> = w.areas.iter().map(|a| format!("{{\"name\":{},\"x\":{},\"y\":{},\"owner\":{}}}", js(&a.name), a.x, a.y, a.owner.map_or(-1, |o| o as i32))).collect();
        let borders: Vec<String> = w
            .borders
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let mut s = format!("{{\"a\":{},\"b\":{}", b.a, b.b);
                if let Some(l) = sh.live.get(&(i as u16)) {
                    let st = &l.stats;
                    s += &format!(",\"live\":true,\"map\":{},\"frame\":{},\"idle\":{}", js(l.spec.map.rsplit(['/', '\\']).next().unwrap_or("")), st.frame, st.idle_frames);
                    let sides: Vec<String> = l
                        .spec
                        .sides
                        .iter()
                        .zip(&st.sides)
                        .map(|(sd, x)| {
                            let mut o = format!("{{\"faction\":{}", sd.faction);
                            if v == Some(sd.faction) {
                                let hum: Vec<String> = x.humans.iter().map(|h| js(h)).collect();
                                o += &format!(
                                    ",\"humans\":[{}],\"supply\":[{},{}],\"workers\":{},\"minerals\":{},\"gas\":{},\"mined\":[{},{}]",
                                    hum.join(","),
                                    x.supply.0,
                                    x.supply.1,
                                    x.workers,
                                    x.minerals,
                                    x.gas,
                                    x.mined.0,
                                    x.mined.1
                                );
                            }
                            o + "}"
                        })
                        .collect();
                    s += &format!(",\"sides\":[{}]", sides.join(","));
                    if v.is_some_and(|f| l.spec.sides.iter().any(|x| x.faction == f)) {
                        if let Some(c) = &l.code {
                            s += &format!(",\"code\":{}", js(c));
                        }
                    }
                }
                s + "}"
            })
            .collect();
        let chat: Vec<String> = sh.chat.iter().filter(|c| Some(c.0) == v).map(|c| format!("[{},{}]", js(&c.1), js(&c.2))).collect();
        format!(
            "{{\"factions\":[{}],\"areas\":[{}],\"borders\":[{}],\"me\":{},\"faction\":{},\"paused\":{},\"winner\":{},\"pick\":{},\"chat\":[{}]}}",
            factions.join(","),
            areas.join(","),
            borders.join(","),
            js(name),
            v.map_or(-1, |f| f as i32),
            sh.paused,
            sh.winner.map_or(-1, |f| f as i32),
            w.eliminated_pick,
            chat.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_and_quote() {
        assert_eq!(unquote("a+b%21%2"), "a b!%2");
        assert_eq!(js("a\"b\\\n"), "\"a\\\"b\\\\\\u000a\"");
        assert!(valid_name("three") && !valid_name("a b") && !valid_name(""));
    }
}
