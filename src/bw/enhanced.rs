//! Enhanced games (ours, no 1.16.1 counterpart): players may join a game that is running.
//! A joiner takes an army: alone when the army has no human (the AI that held it stops), or
//! shared with its humans as in a team melee (0x512678 / 0x51267c: its own selection and
//! groups, the army's units). When the last human of an army leaves, the AI takes the army
//! over (its race's melee script, as 0x4ab230 starts a computer) and the game goes on.
//!
//! The join is a command in the host's turn, so every peer applies it at the same frame:
//! `CMD_JOIN`, the human id, the army, the name (0x18 bytes, NUL padded).

use super::{Data, State};

/// The join command's id (outside 1.16.1's 0x05 .. 0x5c).
/// Notice id (not a network.tbl line): the computer took army `p`.
pub const NOTICE_AI: u16 = 0x1000;
/// Notice id (not a network.tbl line): human `p` joined the army `nation_of(p)`.
pub const NOTICE_JOIN: u16 = 0x1001;
/// Notice id (campaign matches): a draw in `p` minutes if nothing dies and nobody mines.
pub const NOTICE_DRAW: u16 = 0x1002;

/// Campaign matches: a chat line starting with this is for the sender's faction only (the
/// host passes it to the humans of the sender's army, the web map and the faction's other
/// matches; never to the other side).
pub const FACTION_CHAT: u8 = b'!';

/// Campaign matches: a minute at Fastest (42 ms frames), the warnings from 5 idle minutes
/// and the draw at 10.
pub const MINUTE: u32 = 60_000 / 42;
pub const DRAW_WARN: u32 = 5 * MINUTE;
pub const DRAW_AT: u32 = 10 * MINUTE;

pub const CMD_JOIN: u8 = 0xe0;
pub const JOIN_LEN: usize = 4 + 0x18;

/// The join command for human `h` (link id `p`) taking army `n` as `name`.
pub fn join_command(h: u8, n: u8, p: u8, name: &[u8]) -> Vec<u8> {
    let mut c = vec![CMD_JOIN, h, n, p];
    let mut nm = [0u8; 0x18];
    let k = name.len().min(0x17);
    nm[..k].copy_from_slice(&name[..k]);
    c.extend_from_slice(&nm);
    c
}

/// What a saved game does not hold of an enhanced game: the humans' armies, the gone humans,
/// the armies the AI took, the names. A joiner's state is a saved game of the join frame
/// with these, and every peer goes through the same save and load there (view.rs
/// `reload_through_save`): a load rebuilds pools and lists, so only a game that went through
/// it equals the joiner's.
pub struct Extras {
    pub nation: Vec<u8>,
    pub gone: u32,
    pub ai_taken: u8,
    pub expand: [u16; 8],
    pub parked: Vec<Vec<Parked>>,
    pub human_names: Vec<String>,
    pub names: Vec<String>,
    /// Campaign matches: the draw timer (`State::campaign_tick`).
    pub campaign: bool,
    pub idle_frames: u32,
}

/// Ours: an AI script thread put aside while a human has its army (`State::ai_parked`): where
/// it was, its flags and area, and its town.
#[derive(Clone, Debug, PartialEq)]
pub struct Parked {
    pub pos: u32,
    pub flags: u32,
    pub area: [i32; 4],
    pub town: Option<ParkedTown>,
}

/// What a parked thread's town had that its units do not bring back: its place, resource
/// area, worker limit and the units its script asked for (`AiTown::town_units`, the entries
/// up to the last one in use).
#[derive(Clone, Debug, PartialEq)]
pub struct ParkedTown {
    pub pos: (i16, i16),
    pub res: u8,
    pub limit: u8,
    pub units: Vec<(u8, u8, u16)>,
}

/// A cursor over received bytes.
struct Rd<'a>(&'a [u8], usize);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let r = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(r)
    }

    fn text(&mut self) -> Option<String> {
        let k = self.take(1)?[0] as usize;
        Some(String::from_utf8_lossy(self.take(k)?).into_owned())
    }
}

impl Extras {
    pub fn of(st: &State) -> Extras {
        Extras { nation: st.human_nation.clone(), gone: st.humans_gone, ai_taken: st.ai_taken, expand: st.ai_expand_script, parked: st.ai_parked.to_vec(), human_names: st.human_names.clone(), names: st.player_names.iter().take(8).cloned().collect(), campaign: st.campaign, idle_frames: st.idle_frames }
    }

    pub fn apply(&self, st: &mut State) {
        st.multiplayer = true;
        st.enhanced = true;
        st.set_humans(&self.nation);
        st.humans_gone = self.gone;
        st.ai_taken = self.ai_taken;
        st.ai_expand_script = self.expand;
        for (k, v) in self.parked.iter().enumerate().take(8) {
            st.ai_parked[k] = v.clone();
        }
        st.human_names = self.human_names.clone();
        for (k, t) in self.names.iter().enumerate() {
            st.player_names[k] = t.clone();
        }
        st.campaign = self.campaign;
        st.idle_frames = self.idle_frames;
        st.activity = st.activity_count();
    }

    pub fn encode(&self, b: &mut Vec<u8>) {
        let s = |b: &mut Vec<u8>, t: &str| {
            let t = &t.as_bytes()[..t.len().min(0x40)];
            b.push(t.len() as u8);
            b.extend_from_slice(t);
        };
        b.push(self.nation.len() as u8);
        b.extend_from_slice(&self.nation);
        b.extend_from_slice(&self.gone.to_le_bytes());
        b.push(self.ai_taken);
        for v in self.expand {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for k in 0..8 {
            let l = self.parked.get(k).map_or(&[][..], |v| &v[..]);
            b.push(l.len().min(0xff) as u8);
            for t in l.iter().take(0xff) {
                b.extend_from_slice(&t.pos.to_le_bytes());
                b.extend_from_slice(&t.flags.to_le_bytes());
                for v in t.area {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                b.push(t.town.is_some() as u8);
                if let Some(tw) = &t.town {
                    b.extend_from_slice(&tw.pos.0.to_le_bytes());
                    b.extend_from_slice(&tw.pos.1.to_le_bytes());
                    b.push(tw.res);
                    b.push(tw.limit);
                    b.push(tw.units.len() as u8);
                    for &(f, pr, id) in &tw.units {
                        b.push(f);
                        b.push(pr);
                        b.extend_from_slice(&id.to_le_bytes());
                    }
                }
            }
        }
        b.push(self.human_names.len() as u8);
        for t in &self.human_names {
            s(b, t);
        }
        for t in &self.names {
            s(b, t);
        }
        b.push(self.campaign as u8);
        b.extend_from_slice(&self.idle_frames.to_le_bytes());
    }

    /// `encode`'s inverse from `b[*at..]` (`at` moves past it).
    pub fn decode(b: &[u8], at: &mut usize) -> Option<Extras> {
        let mut r = Rd(b, *at);
        let nn = r.take(1)?[0] as usize;
        let nation = r.take(nn)?.to_vec();
        let gone = u32::from_le_bytes(r.take(4)?.try_into().ok()?);
        let ai_taken = r.take(1)?[0];
        let mut expand = [0u16; 8];
        for v in expand.iter_mut() {
            *v = u16::from_le_bytes(r.take(2)?.try_into().ok()?);
        }
        let mut parked = Vec::new();
        for _ in 0..8 {
            let k = r.take(1)?[0];
            let mut l = Vec::new();
            for _ in 0..k {
                let u32_ = |r: &mut Rd| Some(u32::from_le_bytes(r.take(4)?.try_into().ok()?));
                let pos = u32_(&mut r)?;
                let flags = u32_(&mut r)?;
                let mut area = [0i32; 4];
                for v in area.iter_mut() {
                    *v = u32_(&mut r)? as i32;
                }
                let town = if r.take(1)?[0] != 0 {
                    let x = i16::from_le_bytes(r.take(2)?.try_into().ok()?);
                    let y = i16::from_le_bytes(r.take(2)?.try_into().ok()?);
                    let h = r.take(3)?;
                    let (res, limit, k) = (h[0], h[1], h[2].min(0x64));
                    let mut units = Vec::new();
                    for _ in 0..k {
                        let e = r.take(4)?;
                        units.push((e[0], e[1], u16::from_le_bytes([e[2], e[3]])));
                    }
                    Some(ParkedTown { pos: (x, y), res, limit, units })
                } else {
                    None
                };
                l.push(Parked { pos, flags, area, town });
            }
            parked.push(l);
        }
        let nh = r.take(1)?[0];
        let human_names = (0..nh).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
        let names = (0..8).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
        let campaign = r.take(1)?[0] != 0;
        let idle_frames = u32::from_le_bytes(r.take(4)?.try_into().ok()?);
        *at = r.1;
        Some(Extras { nation, gone, ai_taken, expand, parked, human_names, names, campaign, idle_frames })
    }
}

impl State {
    /// A human still in the game controls army `n` (not counting `except`).
    pub fn army_has_human(&self, n: u8, except: Option<u8>) -> bool {
        (0..self.human_nation.len().min(32) as u8).any(|h| {
            Some(h) != except && self.nation_of(h) == n && self.humans_gone & 1 << h == 0 && (h >= 8 || self.player_types[h as usize] == 2)
        })
    }

    /// The host's choice for a joiner who asks for army `want` (0xff: any): (the human id, the
    /// army). An army the AI took over first, else the human army with the fewest humans; the
    /// joiner takes the army's own human id (its slot) when that one left, else the next free
    /// id from 8.
    pub fn enhanced_pick(&self, want: u8) -> Option<(u8, u8)> {
        let alive = |n: u8| (n as usize) < 8 && self.player_left[n as usize] == 0 && matches!(self.player_types[n as usize], 1 | 2);
        let humans = |n: u8| (0..self.human_nation.len().min(32) as u8).filter(|&h| self.nation_of(h) == n && self.humans_gone & 1 << h == 0 && (h >= 8 || self.player_types[h as usize] == 2)).count();
        let n = if want < 8 && alive(want) && (self.player_types[want as usize] == 2 || self.ai_taken & 1 << want != 0) {
            want
        } else if let Some(n) = (0..8u8).find(|&n| alive(n) && self.ai_taken & 1 << n != 0) {
            n
        } else {
            (0..8u8).filter(|&n| alive(n) && self.player_types[n as usize] == 2).min_by_key(|&n| humans(n))?
        };
        let own_free = self.player_types[n as usize] != 2 || self.humans_gone & 1 << n != 0 || self.ai_taken & 1 << n != 0;
        let h = if own_free && self.nation_of(n) == n {
            n
        } else {
            (8..32u8).find(|&h| h as usize >= self.human_nation.len() || self.humans_gone & 1 << h != 0)?
        };
        Some((h, n))
    }

    /// `CMD_JOIN`: human `h` controls army `n` from now on (its selection row and groups made
    /// if new); an army the AI held is the humans' again.
    pub fn enhanced_join(&mut self, d: &Data, p: &[u8]) {
        if !self.enhanced || p.len() < JOIN_LEN {
            return;
        }
        let (h, n) = (p[1], p[2]);
        if h >= 32 || n >= 8 {
            return;
        }
        let nm = &p[4..JOIN_LEN];
        let name = String::from_utf8_lossy(&nm[..nm.iter().position(|&c| c == 0).unwrap_or(nm.len())]).into_owned();
        let mut nation = self.human_nation.clone();
        if nation.len() <= h as usize {
            nation.extend((nation.len()..=h as usize).map(|i| i as u8));
        }
        nation[h as usize] = n;
        self.set_humans(&nation);
        if h < 8 {
            self.player_names[h as usize] = name;
        } else {
            if self.human_names.len() < h as usize - 7 {
                self.human_names.resize(h as usize - 7, String::new());
            }
            self.human_names[h as usize - 8] = name;
        }
        self.humans_gone &= !(1 << h);
        if let Some(sel) = self.selections.get_mut(h as usize) {
            *sel = [super::NIL; 12];
        }
        if self.ai_taken & 1 << n != 0 {
            self.ai_release(d, n);
            // The army's own slot is also a human index (< 8). A computer army never had a
            // human there, so it never leaves: without this its last human leaving finds
            // "another human" on the army and the AI never takes it back.
            if h != n && n < 8 {
                self.humans_gone |= 1 << n;
            }
        }
        // Ours: "<name> has joined <owner>'s army." in the army's colour (the view builds it).
        self.notices.push((NOTICE_JOIN, h));
    }

    /// The last human of army `n` left: the army is the AI's (a computer, 0x57eee8 = 1). Its AI
    /// goes on where it stopped when a human took the army (`ai_release` parked its script
    /// threads): the threads' towns are made again, then the threads resume. An army that was
    /// never the AI's gets its race's melee script around its start location (as 0x4ab230).
    pub fn ai_take_over(&mut self, d: &Data, n: u8) {
        let p = n as usize;
        if p >= 8 || self.player_left[p] != 0 {
            return;
        }
        self.player_types[p] = 1;
        self.ai_taken |= 1 << n;
        self.notices.push((NOTICE_AI, n));
        // What the AI knew of its regions is stale (0x437a70 for this player): their military
        // left with the units.
        let regions = self.ai.regions[p].len();
        self.ai.regions[p] = (0..regions).map(|i| super::ai::AiRegion::new(i as u16, n)).collect();
        let parked = std::mem::take(&mut self.ai_parked[p]);
        let bw = if parked.is_empty() {
            // A fresh computer as at the game's start (0x447680), but for its expansion script
            // (the melee script's `panic` block, 0x690102).
            let ai = &mut self.ai;
            let panic = ai.players[p].panic_script_pos;
            ai.players[p] = super::ai::PlayerAi { minerals_available: self.players.minerals[p] as u32, gas_available: self.players.gas[p] as u32, difficulty: 1, default_min_strength_for_regions: 100, panic_script_pos: panic, ..Default::default() };
            let head = self.ai.active_scripts;
            let bw = self.start_melee_ai_for(d, p);
            // Its first step (start_town) now: the towns must exist below.
            if self.ai.active_scripts != head {
                self.run_script(d, self.ai.active_scripts);
            }
            bw
        } else {
            // The player's AI data stays (what the script set up: build limits, flags, ...);
            // its spending requests start over (as 0x448890).
            let pa = &mut self.ai.players[p];
            pa.request_count = 0;
            pa.mineral_need = 0;
            pa.gas_need = 0;
            pa.supply_need = 0;
            // The towns, the one nearest the start location first: its start_town (0x434220)
            // takes every finished unit; the others take the units around them.
            let (sx, sy) = self.start_locations[p];
            let mut keys: Vec<&ParkedTown> = Vec::new();
            for k in parked.iter().filter_map(|t| t.town.as_ref()) {
                if !keys.iter().any(|t| (t.pos, t.res) == (k.pos, k.res)) {
                    keys.push(k);
                }
            }
            keys.sort_by_key(|t| super::path::approx_distance(t.pos.0 as i32, sx as i32, t.pos.1 as i32, sy as i32));
            let mut towns = Vec::new();
            for (i, k) in keys.iter().enumerate() {
                let (x, y) = k.pos;
                let Some(t) = self.start_town(d, n, x, y, k.res) else { continue };
                if i > 0 {
                    self.town_take_near(d, n, t, (x, y));
                }
                let tw = &mut self.ai.towns.items[t as usize];
                tw.worker_limit = k.limit;
                tw.town_units[..k.units.len()].copy_from_slice(&k.units);
                towns.push((k.pos, k.res, t));
            }
            // The threads, last first (each goes first in the list), with their flags and town.
            for pk in parked.iter().rev() {
                let Some(sc) = self.start_ai_script(n as u32, pk.pos, pk.flags & 1 != 0, Some(pk.area)) else { continue };
                let town = pk.town.as_ref().and_then(|k| towns.iter().find(|t| (t.0, t.1) == (k.pos, k.res))).map_or(super::NIL, |t| t.2);
                let it = &mut self.ai.scripts.items[sc as usize];
                it.flags = pk.flags & !4;
                it.town = town;
            }
            Some(parked[0].flags & 1 != 0)
        };
        // A town hall far from every town (a human's new base, or one the AI was still making
        // when the human took the army) gets the expansion script its last `expand` started
        // (0x45b440 over the hall's resource area); its start_town finds nothing left, so the
        // units around the hall move to its town. Without it a far base belongs to a town that
        // places buildings around itself only.
        let expand = self.ai_expand_script[p];
        if let (Some(bw), true) = (bw, expand != 0) {
            let halls: Vec<(i16, i16)> = (0..self.units.len())
                .filter(|&u| {
                    let un = &self.units[u];
                    un.player == n && un.sprite != super::NIL && matches!(un.unit_type, 0x6a | 0x83 | 0x84 | 0x85 | 0x9a)
                })
                .map(|u| self.unit_sprite_pos(u as u32))
                .collect();
            for (x, y) in halls {
                let mut t = self.ai.town_lists[p];
                let mut far = true;
                while t != super::NIL {
                    let (tx, ty) = self.ai.towns.items[t as usize].position;
                    far &= super::path::approx_distance(x as i32, tx as i32, y as i32, ty as i32) > 0x200;
                    t = self.ai.towns.items[t as usize].link.next;
                }
                if !far {
                    continue;
                }
                let near = (1..=self.area_count as usize)
                    .filter(|&a| self.areas[a].flags & 2 == 0)
                    .map(|a| (super::path::approx_distance(self.areas[a].pos.0 as i32, x as i32, self.areas[a].pos.1 as i32, y as i32), a))
                    .min();
                let Some((_, a)) = near.filter(|&(dist, _)| dist < 0x180) else { continue };
                self.areas[a].flags |= 2;
                let head = self.ai.active_scripts;
                self.start_area_script(d, n, bw, a as u32, expand);
                let s = self.ai.active_scripts;
                if s == head {
                    continue;
                }
                self.run_script(d, s);
                let town = self.ai.scripts.items[s as usize].town;
                if town != super::NIL {
                    self.town_take_near(d, n, town, (x, y));
                }
            }
        }
        // The start town goes first in the town list: requests of equal priority are taken in
        // that order (0x436320's pass over the towns), and the release stopped the workers on
        // their way to place a building. Left behind a new town's request whose requirement
        // the start town was about to build (a cannon at an expansion before the Forge), the
        // AI waits for good.
        let (sx, sy) = self.start_locations[p];
        let mut best = (super::NIL, u32::MAX);
        let mut t = self.ai.town_lists[p];
        while t != super::NIL {
            let (tx, ty) = self.ai.towns.items[t as usize].position;
            let dist = super::path::approx_distance(tx as i32, sx as i32, ty as i32, sy as i32);
            if dist < best.1 {
                best = (t, dist);
            }
            t = self.ai.towns.items[t as usize].link.next;
        }
        let head = self.ai.town_lists[p];
        if best.0 != super::NIL && best.0 != head {
            let towns = &mut self.ai.towns.items;
            let super::ai::AiLink { next, prev } = towns[best.0 as usize].link;
            towns[prev as usize].link.next = next;
            if next != super::NIL {
                towns[next as usize].link.prev = prev;
            }
            towns[best.0 as usize].link = super::ai::AiLink { next: head, prev: super::NIL };
            towns[head as usize].link.prev = best.0;
            self.ai.town_lists[p] = best.0;
        }
        // A town whose main building moved away (start_town made the first town hall it took
        // its main) takes the next of its own town halls (0x433dd0 does so for the first).
        for u in 0..self.units.len() as u32 {
            let r = self.units[u as usize].ai;
            if self.units[u as usize].player != n || r == super::NIL || super::ai_town::ai_kind(r) != super::ai_town::AI_BUILDING || !self.is_drop_off(d, u) {
                continue;
            }
            let town = self.ai.buildings.items[super::ai_town::ai_index(r) as usize].town;
            if town != super::NIL && self.ai.towns.items[town as usize].main_building == super::NIL {
                self.ai.towns.items[town as usize].main_building = u;
            }
        }
        // The human's units belong to no AI list (0x463040 runs when a computer's unit is
        // created; `ai_release` took them out): the AI takes them now (start_town takes
        // finished units only), workers and unfinished buildings into the nearest town. A unit
        // a building still trains is left alone: it joins when it is finished (0x4a2830). An
        // idle one gets the computer's idle order, as a computer's unit gets when it is
        // finished (0x49fa40, units.dat): with the human's a building never trains what the
        // AI asks (only order 0x9c serves the requests). Idle is units.dat's idle orders or
        // PlayerGuard (3), where a Stop ends.
        let trained: Vec<u32> = (0..self.units.len()).filter(|&u| self.units[u].player == n && self.units[u].sprite != super::NIL).map(|u| self.units[u].current_build_unit).filter(|&c| c != super::NIL).collect();
        for u in 0..self.units.len() as u32 {
            let un = &self.units[u as usize];
            let training = un.status & 1 == 0 && trained.contains(&u);
            // A dying unit (order 0) is past the AI's cleanup: 0x434220 skips it too.
            if un.player == n && un.sprite != super::NIL && un.order != 0 && un.ai == super::NIL && un.unit_type < 0xe4 && !training {
                let (ux, uy) = self.unit_sprite_pos(u);
                let mut town = (super::NIL, u32::MAX);
                let mut t = self.ai.town_lists[p];
                while t != super::NIL {
                    let (tx, ty) = self.ai.towns.items[t as usize].position;
                    let dist = super::path::approx_distance(ux as i32, tx as i32, uy as i32, ty as i32);
                    if dist < town.1 {
                        town = (t, dist);
                    }
                    t = self.ai.towns.items[t as usize].link.next;
                }
                if town.0 != super::NIL && d.dat.units.ai_internal[un.unit_type as usize] & 2 != 0 {
                    self.town_add_unit(d, u, town.0);
                } else {
                    self.ai_take_unit(d, u);
                }
            }
            let un = &self.units[u as usize];
            let t = un.unit_type as usize;
            let idle = un.order == d.dat.units.ai_human_idle[t] || un.order == d.dat.units.ai_return_to_idle[t] || un.order == 3;
            if un.player == n && un.sprite != super::NIL && t < 0xe4 && un.status & 1 != 0 && un.order_queue_head == super::NIL && idle {
                self.order_now(d, u, d.dat.units.ai_computer_idle[t]);
            }
        }
    }

    /// Army `n`'s town units (workers, buildings) within 0x200 of `at` move to `town`.
    fn town_take_near(&mut self, d: &Data, n: u8, town: u32, at: (i16, i16)) {
        for u in 0..self.units.len() as u32 {
            let un = &self.units[u as usize];
            if un.player != n || un.sprite == super::NIL || un.ai == super::NIL || d.dat.units.ai_internal[un.unit_type as usize] & 2 == 0 {
                continue;
            }
            let (ux, uy) = self.unit_sprite_pos(u);
            if super::path::approx_distance(ux as i32, at.0 as i32, uy as i32, at.1 as i32) < 0x200 && self.town_remove_unit(d, u) {
                self.town_add_unit(d, u, town);
            }
        }
    }

    /// A human took army `n` back from the AI: it is a human's again, its AI scripts end and
    /// its units leave the AI (0x4a1e50 each, as when a unit changes owner: towns, guard
    /// posts, regions), so no town or guard post orders them about for the human. The scripts
    /// go back to the pool at once with their flags cleared: 0x45aef0 keeps a reused entry's
    /// flags & 7, so one flagged 4 (killed) would end the next script started in it, the melee
    /// script of a later take-over.
    fn ai_release(&mut self, d: &Data, n: u8) {
        self.player_types[n as usize] = 2;
        self.ai_taken &= !(1 << n);
        // The threads are parked (with their town's place and resource area) for a take-over,
        // before the towns go with their units.
        let mut parked = Vec::new();
        let mut s = self.ai.active_scripts;
        while s != super::NIL {
            let sc = &self.ai.scripts.items[s as usize];
            if sc.player == n as u32 && sc.flags & 4 == 0 {
                let town = (sc.town != super::NIL).then(|| {
                    let t = &self.ai.towns.items[sc.town as usize];
                    let used = t.town_units.iter().rposition(|e| *e != (0, 0, 0)).map_or(0, |i| i + 1);
                    ParkedTown { pos: t.position, res: t.resource_area, limit: t.worker_limit, units: t.town_units[..used].to_vec() }
                });
                parked.push(Parked { pos: sc.pos, flags: sc.flags, area: sc.area, town });
            }
            s = sc.link.next;
        }
        self.ai_parked[n as usize] = parked;
        // A worker the AI sent to place a building (orders 0x19 / 0x1a / 0x1e / 0x1f, now or
        // queued) is stopped as the human's Stop command does: the human did not ask for it.
        let builds = |o: u8| matches!(o, 0x19 | 0x1a | 0x1e | 0x1f);
        for u in 0..self.units.len() as u32 {
            let un = &self.units[u as usize];
            if un.player != n || un.sprite == super::NIL {
                continue;
            }
            if un.ai != super::NIL {
                self.ai_release_unit(d, u, false);
            }
            let un = &self.units[u as usize];
            let mut building = builds(un.order);
            let mut q = un.order_queue_head;
            while q != super::NIL && !building {
                building = builds(self.orders[q as usize].order);
                q = self.orders[q as usize].link.next;
            }
            if building && d.dat.units.flags[un.unit_type as usize] & 8 != 0 {
                self.command_order(d, u, 1, false, 1, super::NIL, 0xe4, (0, 0));
            }
        }
        let mut s = self.ai.active_scripts;
        while s != super::NIL {
            let next = self.ai.scripts.items[s as usize].link.next;
            if self.ai.scripts.items[s as usize].player == n as u32 {
                let mut head = self.ai.active_scripts;
                self.ai.scripts.release(&mut head, s);
                self.ai.active_scripts = head;
                self.ai.scripts.items[s as usize].flags = 0;
            }
            s = next;
        }
    }

    /// A campaign match loaded from its save (the host restarted): nobody is connected, so
    /// every army is the AI's (a human one taken over as when its last human leaves).
    pub fn campaign_resume(&mut self, d: &Data, armies: [u8; 2], idle_frames: u32) {
        self.multiplayer = true;
        self.enhanced = true;
        self.campaign = true;
        self.idle_frames = idle_frames;
        self.activity = self.activity_count();
        for n in armies {
            if self.player_types[n as usize] == 2 {
                self.ai_take_over(d, n);
            } else {
                self.ai_taken |= 1 << n;
            }
        }
    }

    /// Deaths (men and buildings lost, 0x581e74 / 0x581fc4) and minerals gathered (0x57f180),
    /// every player: the campaign draw timer's activity. Not gas: a depleted geyser still
    /// gives some forever, and the user's rule is "no minerals mined".
    pub fn activity_count(&self) -> i64 {
        let p = &self.players;
        (0..12).map(|i| p.men_lost[i] as i64 + p.buildings_lost[i] as i64 + p.minerals_gathered[i] as i64).sum()
    }

    /// Campaign matches, after each frame's trigger pass: a death or minerals mined restart
    /// the idle count; from 5 idle minutes every player is warned each minute; at 10 every
    /// player still in the game gets a draw (0x58d700 = 5, the end screen as a trigger's
    /// draw 0x4c5070 then 0x48a200 settle it).
    pub fn campaign_tick(&mut self) {
        if !self.campaign || self.trig.paused {
            return;
        }
        let a = self.activity_count();
        if a != self.activity {
            self.activity = a;
            self.idle_frames = 0;
            return;
        }
        self.idle_frames += 1;
        let t = self.idle_frames;
        if (DRAW_WARN..DRAW_AT).contains(&t) && t % MINUTE == 0 {
            self.notices.push((NOTICE_DRAW, ((DRAW_AT - t) / MINUTE) as u8));
        }
        if t == DRAW_AT {
            for p in 0..8 {
                if self.trig.in_game[p] != 0 && self.player_left[p] == 0 && matches!(self.player_types[p], 1 | 2) {
                    self.trig.result[p] = 5;
                    self.player_left[p] = 5;
                }
            }
            self.trig.end_screen_timer = 0x2d;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extras_round_trip() {
        let mut st = State::new((64, 64), 0, 1700);
        st.human_nation = vec![0, 1, 2, 3, 4, 5, 6, 7, 0, 1];
        st.humans_gone = 1 << 8;
        st.ai_taken = 2;
        st.ai_expand_script = [0x1234, 0, 0, 0, 0, 0, 0, 7];
        st.human_names = vec!["a".into(), "bb".into()];
        st.campaign = true;
        st.idle_frames = 99;
        st.ai_parked[1] = vec![
            Parked { pos: 0xac7b, flags: 0x41, area: [1, 2, 3, -4], town: Some(ParkedTown { pos: (288, 2288), res: 1, limit: 17, units: vec![(9, 0x96, 0x9a), (0x21, 0x50, 0x40)] }) },
            Parked { pos: 0xade9, flags: 0, area: [0; 4], town: None },
        ];
        let ex = Extras::of(&st);
        let mut b = vec![0xaa];
        ex.encode(&mut b);
        b.push(0x55);
        let mut at = 1;
        let back = Extras::decode(&b, &mut at).expect("decodes");
        assert_eq!(at, b.len() - 1);
        let mut st2 = State::new((64, 64), 0, 1700);
        back.apply(&mut st2);
        assert_eq!(st2.ai_parked, st.ai_parked);
        assert_eq!(st2.ai_expand_script, st.ai_expand_script);
        assert_eq!((st2.ai_taken, st2.humans_gone, st2.idle_frames), (2, 1 << 8, 99));
        assert_eq!(st2.human_names, st.human_names);
    }

    #[test]
    fn draw_timer() {
        let mut st = State::new((64, 64), 0, 1700);
        st.campaign = true;
        st.trig.in_game = [1, 1, 0, 0, 0, 0, 0, 0];
        st.player_types[..2].copy_from_slice(&[2, 1]);
        // Mining at frame 100 restarts the count.
        for f in 0..DRAW_AT + 100 {
            if f == 100 {
                st.players.minerals_gathered[0] += 8;
            }
            st.campaign_tick();
        }
        let warn: Vec<u8> = st.notices.iter().filter(|n| n.0 == NOTICE_DRAW).map(|n| n.1).collect();
        assert_eq!(warn, vec![5, 4, 3, 2, 1]);
        assert_eq!(st.player_left[..3], [0, 0, 0]);
        // A death restarts it again; then 10 minutes with only gas coming in (a depleted
        // geyser) end the game in a draw.
        st.players.men_lost[1] += 1;
        for _ in 0..=DRAW_AT {
            st.players.gas_gathered[0] += 2;
            st.campaign_tick();
        }
        assert_eq!(st.player_left[..3], [5, 5, 0]);
        // Off: nothing happens.
        let mut st = State::new((64, 64), 0, 1700);
        for _ in 0..=DRAW_AT {
            st.campaign_tick();
        }
        assert!(st.notices.is_empty());
    }
}
