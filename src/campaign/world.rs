//! Campaign world (ours, not 1.16.1): factions, areas, the borders between them (a map
//! each) and the rules that change who owns what. agent/plans/campaign.md.
//!
//! World file, a line each, `#` starts a comment; factions and areas are numbered from 0
//! in file order:
//! ```text
//! faction <race Z|T|P|R> <colour> <name>
//! area <x> <y> <owner faction | -> <name>
//! border <area> <area> [map]          no map: a pool map per match
//! pool <map file or directory>
//! eliminated pick | out               what an eliminated faction's players may do
//! ```
//! Either every area has an owner or none has; with none, `deal` gives each faction a
//! connected block.

use std::collections::VecDeque;

pub struct Faction {
    pub name: String,
    /// 1.16.1 race: 0 Zerg, 1 Terran, 2 Protoss, 6 random.
    pub race: u8,
    /// Player colour used for this faction in every match.
    pub colour: u8,
}

pub struct Area {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub owner: Option<u8>,
}

pub struct Border {
    pub a: u16,
    pub b: u16,
    pub map: Option<String>,
}

#[derive(Default)]
pub struct World {
    pub factions: Vec<Faction>,
    pub areas: Vec<Area>,
    pub borders: Vec<Border>,
    pub pool: Vec<String>,
    /// Players of an eliminated faction may pick a surviving one (else they are out).
    pub eliminated_pick: bool,
}

/// What a match result changed: the area that changed hands, the live matches that end
/// with it, and the borders that need a new match.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub captured: Option<u16>,
    pub cancel: Vec<u16>,
    pub start: Vec<u16>,
}

const RACES: [(char, u8); 4] = [('Z', 0), ('T', 1), ('P', 2), ('R', 6)];

impl World {
    pub fn parse(text: &str) -> Result<World, String> {
        let mut w = World::default();
        for (i, line) in text.lines().enumerate() {
            let err = |m: &str| format!("line {}: {m}", i + 1);
            let line = line.split('#').next().unwrap_or("").trim();
            let mut f = line.split_whitespace();
            let Some(key) = f.next() else { continue };
            let mut num = |what: &str| -> Result<i64, String> {
                f.next().and_then(|v| v.parse().ok()).ok_or_else(|| err(&format!("bad {what}")))
            };
            match key {
                "faction" => {
                    let r = line.split_whitespace().nth(1).unwrap_or("");
                    let race = RACES.iter().find(|(c, _)| r.len() == 1 && r.starts_with(*c)).ok_or_else(|| err("race is Z, T, P or R"))?.1;
                    let mut f = line.split_whitespace().skip(2);
                    let colour = f.next().and_then(|v| v.parse().ok()).ok_or_else(|| err("bad colour"))?;
                    w.factions.push(Faction { name: rest(f), race, colour });
                }
                "area" => {
                    let (x, y) = (num("x")? as i32, num("y")? as i32);
                    let mut f = line.split_whitespace().skip(3);
                    let owner = match f.next() {
                        Some("-") => None,
                        Some(v) => Some(v.parse().map_err(|_| err("bad owner"))?),
                        None => return Err(err("missing owner")),
                    };
                    w.areas.push(Area { name: rest(f), x, y, owner });
                }
                "border" => {
                    let (a, b) = (num("area")? as u16, num("area")? as u16);
                    let map = line.split_whitespace().skip(3).collect::<Vec<_>>().join(" ");
                    w.borders.push(Border { a, b, map: (!map.is_empty()).then_some(map) });
                }
                "pool" => w.pool.push(line["pool".len()..].trim().to_string()),
                "eliminated" => match f.next() {
                    Some("pick") => w.eliminated_pick = true,
                    Some("out") => w.eliminated_pick = false,
                    _ => return Err(err("eliminated pick | out")),
                },
                _ => return Err(err(&format!("unknown `{key}`"))),
            }
        }
        w.check()?;
        Ok(w)
    }

    fn check(&self) -> Result<(), String> {
        if !(2..=8).contains(&self.factions.len()) {
            return Err("2 to 8 factions".into());
        }
        let n = self.areas.len() as u16;
        for (i, a) in self.areas.iter().enumerate() {
            if a.owner.is_some_and(|o| o as usize >= self.factions.len()) {
                return Err(format!("area {i}: no such faction"));
            }
        }
        let owned = self.areas.iter().filter(|a| a.owner.is_some()).count();
        if owned != 0 && owned != self.areas.len() {
            return Err("give every area an owner or none".into());
        }
        for (i, b) in self.borders.iter().enumerate() {
            if b.a >= n || b.b >= n || b.a == b.b {
                return Err(format!("border {i}: bad areas"));
            }
            if self.borders[..i].iter().any(|c| (c.a, c.b) == (b.a, b.b) || (c.a, c.b) == (b.b, b.a)) {
                return Err(format!("border {i}: twice"));
            }
            if b.map.is_none() && self.pool.is_empty() {
                return Err(format!("border {i}: no map and no pool"));
            }
        }
        Ok(())
    }

    pub fn write(&self) -> String {
        let mut s = String::new();
        for f in &self.factions {
            let r = RACES.iter().find(|(_, v)| *v == f.race).map_or('R', |r| r.0);
            s += &format!("faction {r} {} {}\n", f.colour, f.name);
        }
        for a in &self.areas {
            let o = a.owner.map_or("-".to_string(), |o| o.to_string());
            s += &format!("area {} {} {o} {}\n", a.x, a.y, a.name);
        }
        for b in &self.borders {
            let line = format!("border {} {} {}", b.a, b.b, b.map.as_deref().unwrap_or(""));
            s += line.trim_end();
            s.push('\n');
        }
        for p in &self.pool {
            s += &format!("pool {p}\n");
        }
        s += if self.eliminated_pick { "eliminated pick\n" } else { "eliminated out\n" };
        s
    }

    fn neighbours(&self, area: u16) -> impl Iterator<Item = u16> + '_ {
        self.borders.iter().filter_map(move |b| match area {
            _ if b.a == area => Some(b.b),
            _ if b.b == area => Some(b.a),
            _ => None,
        })
    }

    /// Steps from `from` to every area (u32::MAX: unreachable).
    fn distances(&self, from: &[u16]) -> Vec<u32> {
        let mut d = vec![u32::MAX; self.areas.len()];
        let mut q: VecDeque<u16> = from.iter().copied().collect();
        for &a in from {
            d[a as usize] = 0;
        }
        while let Some(a) = q.pop_front() {
            for n in self.neighbours(a) {
                if d[n as usize] == u32::MAX {
                    d[n as usize] = d[a as usize] + 1;
                    q.push_back(n);
                }
            }
        }
        d
    }

    /// Owners for a world that has none: seeds spread apart (each the area farthest from
    /// the seeds so far), then the factions take turns claiming an unowned neighbour, the
    /// one leaning most to their own seed. Not always even on small or odd graphs; the
    /// generator (or the world file) can place owners itself.
    pub fn deal(&mut self) -> Result<(), String> {
        if self.areas.iter().any(|a| a.owner.is_some()) {
            return Ok(());
        }
        let nf = self.factions.len();
        if self.areas.len() < nf {
            return Err("fewer areas than factions".into());
        }
        let mut seeds = vec![0u16];
        while seeds.len() < nf {
            let d = self.distances(&seeds);
            if d.contains(&u32::MAX) {
                return Err("the areas are not all connected".into());
            }
            let far = (0..d.len()).max_by_key(|&i| (d[i], std::cmp::Reverse(i))).unwrap();
            seeds.push(far as u16);
        }
        for (f, &s) in seeds.iter().enumerate() {
            self.areas[s as usize].owner = Some(f as u8);
        }
        // How much nearer an area is to its own seed than to the closest other one: a faction
        // claims its own side first instead of crowding a neighbour's seed.
        let dist: Vec<Vec<u32>> = seeds.iter().map(|&s| self.distances(&[s])).collect();
        let lean = |f: usize, a: usize| {
            let other = (0..nf).filter(|&g| g != f).map(|g| dist[g][a]).min().unwrap_or(0);
            dist[f][a] as i64 - other as i64
        };
        loop {
            let mut moved = false;
            for f in 0..nf as u8 {
                let take = (0..self.areas.len() as u16)
                    .filter(|&a| self.areas[a as usize].owner == Some(f))
                    .flat_map(|a| self.neighbours(a))
                    .filter(|&n| self.areas[n as usize].owner.is_none())
                    .min_by_key(|&n| (lean(f as usize, n as usize), n));
                if let Some(n) = take {
                    self.areas[n as usize].owner = Some(f);
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        if self.areas.iter().any(|a| a.owner.is_none()) {
            return Err("the areas are not all connected".into());
        }
        Ok(())
    }

    fn owners(&self, b: &Border) -> (Option<u8>, Option<u8>) {
        (self.areas[b.a as usize].owner, self.areas[b.b as usize].owner)
    }

    /// A border between two factions: it has a match running.
    pub fn is_live(&self, border: u16) -> bool {
        match self.owners(&self.borders[border as usize]) {
            (Some(x), Some(y)) => x != y,
            _ => false,
        }
    }

    pub fn live_borders(&self) -> Vec<u16> {
        (0..self.borders.len() as u16).filter(|&b| self.is_live(b)).collect()
    }

    fn live_around(&self, area: u16) -> Vec<u16> {
        (0..self.borders.len() as u16)
            .filter(|&i| {
                let b = &self.borders[i as usize];
                (b.a == area || b.b == area) && self.is_live(i)
            })
            .collect()
    }

    /// A match on `border` ended: `winner` takes the other side's area (`None`: draw, the
    /// border gets a new match). Every other live match on the captured area's borders
    /// ends; each of its borders that is live afterwards gets a new match.
    pub fn apply(&mut self, border: u16, winner: Option<u8>) -> Result<Outcome, String> {
        if !self.is_live(border) {
            return Err(format!("border {border} has no match"));
        }
        let Some(w) = winner else {
            return Ok(Outcome { captured: None, cancel: vec![], start: vec![border] });
        };
        let b = &self.borders[border as usize];
        let lost = match self.owners(b) {
            (Some(x), _) if x == w => b.b,
            (_, Some(y)) if y == w => b.a,
            _ => return Err(format!("faction {w} is not on border {border}")),
        };
        let before = self.live_around(lost);
        self.areas[lost as usize].owner = Some(w);
        Ok(Outcome {
            captured: Some(lost),
            cancel: before.into_iter().filter(|&i| i != border).collect(),
            start: self.live_around(lost),
        })
    }

    pub fn areas_of(&self, faction: u8) -> usize {
        self.areas.iter().filter(|a| a.owner == Some(faction)).count()
    }

    /// The last faction owning areas, once only one does.
    pub fn winner(&self) -> Option<u8> {
        let first = self.areas.first()?.owner;
        self.areas.iter().all(|a| a.owner == first).then_some(first).flatten()
    }

    /// One line per faction and per live border (`scbw campaign check`).
    pub fn summary(&self) -> String {
        let mut s = String::new();
        for (i, f) in self.factions.iter().enumerate() {
            s += &format!("faction {i} {}: {} areas\n", f.name, self.areas_of(i as u8));
        }
        for i in self.live_borders() {
            let b = &self.borders[i as usize];
            let (an, bn) = (&self.areas[b.a as usize].name, &self.areas[b.b as usize].name);
            s += &format!("live {i}: {an} - {bn} ({})\n", b.map.as_deref().unwrap_or("pool"));
        }
        s
    }
}

/// Area names the generator draws from (then numbered when it runs out).
const NAMES: [&str; 40] = [
    "Char", "Aiur", "Mar Sara", "Tarsonis", "Korhal", "Shakuras", "Braxis", "Chau Sara", "Umoja", "Moria", "Antiga", "Halcyon", "Brontes", "Dylar",
    "Ulnar", "Zerus", "Bel'Shir", "Redstone", "Typhon", "Haven", "Tyrador", "Meinhoff", "Agria", "Ursa", "Xil", "Endion", "Glacius", "Kaldir",
    "Avernus", "Monlyth", "Abaddon", "Castanar", "Dead Man's Rock", "Erebus", "Feronis", "Gantris", "Jarban", "Kheyvar", "Lysos", "Nemaka",
];

impl World {
    /// A world of `areas` areas for `factions` factions (races Z, T, P in turn, colours
    /// 0, 1, 2...; 3 factions are named after their race), maps from `pool`. Areas sit on a
    /// jittered grid; borders join areas whose diameter circle holds no other area (the
    /// Gabriel graph: planar, connected). Owners are dealt in connected blocks (`deal`).
    pub fn generate(areas: usize, factions: usize, pool: &[String], seed: u32) -> Result<World, String> {
        if !(2..=8).contains(&factions) || areas < factions || areas > 400 {
            return Err("2 to 8 factions, at least as many areas, at most 400".into());
        }
        let mut r = seed | 1;
        let mut rand = move |n: u32| {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            r % n
        };
        let mut w = World { pool: pool.to_vec(), eliminated_pick: true, ..World::default() };
        const RACE_NAMES: [&str; 3] = ["Zerg", "Terran", "Protoss"];
        for f in 0..factions {
            let name = if factions == 3 { RACE_NAMES[f].to_string() } else { format!("Faction {}", f + 1) };
            w.factions.push(Faction { name, race: f as u8 % 3, colour: f as u8 });
        }
        // A grid a bit wider than tall; each cell gets one area somewhere in its middle.
        let cols = ((areas as f64 * 1.5).sqrt().ceil() as usize).max(1);
        let rows = areas.div_ceil(cols);
        let mut cells: Vec<usize> = (0..cols * rows).collect();
        while cells.len() > areas {
            let i = rand(cells.len() as u32) as usize;
            cells.remove(i);
        }
        for (k, c) in cells.into_iter().enumerate() {
            let (x, y) = ((c % cols) as i32 * 100 + 20 + rand(60) as i32, (c / cols) as i32 * 100 + 20 + rand(60) as i32);
            let name = NAMES.get(k).map_or(format!("Area {}", k + 1), |n| n.to_string());
            w.areas.push(Area { name, x, y, owner: None });
        }
        let p: Vec<(i64, i64)> = w.areas.iter().map(|a| (a.x as i64, a.y as i64)).collect();
        for a in 0..p.len() {
            for b in a + 1..p.len() {
                // c inside the circle on a-b: the angle a-c-b is obtuse, (a - c) . (b - c) < 0.
                let blocked = (0..p.len()).any(|c| c != a && c != b && (p[a].0 - p[c].0) * (p[b].0 - p[c].0) + (p[a].1 - p[c].1) * (p[b].1 - p[c].1) <= 0);
                if !blocked {
                    w.borders.push(Border { a: a as u16, b: b as u16, map: None });
                }
            }
        }
        w.check()?;
        w.deal()?;
        Ok(w)
    }
}

/// The remaining words of a line, joined by single spaces.
fn rest<'a>(f: impl Iterator<Item = &'a str>) -> String {
    f.collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0 - 1 - 2 - 3, and 1 - 4 - 2.
    const LINE: &str = "faction T 0 Terran\nfaction Z 1 Zerg\nfaction P 2 Protoss\n\
        area 0 0 - A\narea 1 0 - B\narea 2 0 - C\narea 3 0 - D\narea 1 1 - E\n\
        border 0 1\nborder 1 2\nborder 2 3\nborder 1 4\nborder 4 2 maps/x.scx\npool maps\neliminated pick\n";

    fn owned(o: [u8; 5]) -> World {
        let mut w = World::parse(LINE).unwrap();
        for (a, o) in w.areas.iter_mut().zip(o) {
            a.owner = Some(o);
        }
        w
    }

    #[test]
    fn parse_write() {
        let w = World::parse(LINE).unwrap();
        assert_eq!((w.factions.len(), w.areas.len(), w.borders.len()), (3, 5, 5));
        assert_eq!(w.borders[4].map.as_deref(), Some("maps/x.scx"));
        assert_eq!(World::parse(&w.write()).unwrap().write(), w.write());
        assert!(World::parse("faction T 0 a\nfaction Q 1 b\n").is_err());
        assert!(World::parse("faction T 0 a\nfaction Z 1 b\narea 0 0 - A\narea 0 0 - B\nborder 0 1\n").is_err());
    }

    #[test]
    fn deal_connected_blocks() {
        let mut w = World::parse(LINE).unwrap();
        w.deal().unwrap();
        for f in 0..3 {
            assert!(w.areas_of(f) >= 1);
        }
        assert!(w.areas.iter().all(|a| a.owner.is_some()));
        assert_eq!(w.winner(), None);
    }

    #[test]
    fn capture_cancels_and_restarts() {
        // A B C D E owned by 0 0 1 2 1: live 1-2 (0 v 1), 2-3 (1 v 2), 1-4 (0 v 1).
        let mut w = owned([0, 0, 1, 2, 1]);
        assert_eq!(w.live_borders(), vec![1, 2, 3]);
        // Faction 0 wins on B-C: takes C; C-D (vs 2) restarts, C-E (now 1 v 0) starts.
        let o = w.apply(1, Some(0)).unwrap();
        assert_eq!(o, Outcome { captured: Some(2), cancel: vec![2], start: vec![2, 4] });
        assert_eq!(w.live_borders(), vec![2, 3, 4]);
        // Faction 1 wins on B-E: takes B; A-B and B-C (now 1 v 0) start.
        let o = w.apply(3, Some(1)).unwrap();
        assert_eq!(o, Outcome { captured: Some(1), cancel: vec![], start: vec![0, 1] });
        // Draw: same border again; a faction off the border can't win it.
        assert_eq!(w.apply(0, None).unwrap().start, vec![0]);
        assert!(w.apply(0, Some(2)).is_err());
        assert!(w.apply(3, Some(1)).is_err());
    }

    #[test]
    fn generated_worlds() {
        for (n, f, seed) in [(30, 3, 1), (12, 2, 7), (60, 8, 3), (5, 5, 9)] {
            let w = World::generate(n, f, &["maps".into()], seed).unwrap();
            assert_eq!((w.areas.len(), w.factions.len()), (n, f));
            assert!(w.distances(&[0]).iter().all(|&d| d != u32::MAX), "connected");
            assert!((0..f as u8).all(|i| w.areas_of(i) >= 1));
            assert!(!w.live_borders().is_empty());
            assert_eq!(World::parse(&w.write()).unwrap().write(), w.write());
        }
    }

    #[test]
    fn elimination_and_winner() {
        let mut w = owned([0, 0, 0, 1, 0]);
        assert_eq!(w.areas_of(2), 0);
        assert_eq!(w.winner(), None);
        w.apply(2, Some(0)).unwrap();
        assert_eq!(w.winner(), Some(0));
        assert!(w.live_borders().is_empty());
    }
}
