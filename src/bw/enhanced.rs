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
    pub human_names: Vec<String>,
    pub names: Vec<String>,
    /// Campaign matches: the draw timer (`State::campaign_tick`).
    pub campaign: bool,
    pub idle_frames: u32,
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
        Extras { nation: st.human_nation.clone(), gone: st.humans_gone, ai_taken: st.ai_taken, human_names: st.human_names.clone(), names: st.player_names.iter().take(8).cloned().collect(), campaign: st.campaign, idle_frames: st.idle_frames }
    }

    pub fn apply(&self, st: &mut State) {
        st.multiplayer = true;
        st.enhanced = true;
        st.set_humans(&self.nation);
        st.humans_gone = self.gone;
        st.ai_taken = self.ai_taken;
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
        let nh = r.take(1)?[0];
        let human_names = (0..nh).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
        let names = (0..8).map(|_| r.text()).collect::<Option<Vec<_>>>()?;
        let campaign = r.take(1)?[0] != 0;
        let idle_frames = u32::from_le_bytes(r.take(4)?.try_into().ok()?);
        *at = r.1;
        Some(Extras { nation, gone, ai_taken, human_names, names, campaign, idle_frames })
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

    /// The last human of army `n` left: the army is the AI's (a computer, 0x57eee8 = 1, with
    /// its race's melee script around its start location, as 0x4ab230).
    pub fn ai_take_over(&mut self, d: &Data, n: u8) {
        let p = n as usize;
        if p >= 8 || self.player_left[p] != 0 {
            return;
        }
        self.player_types[p] = 1;
        self.ai_taken |= 1 << n;
        self.notices.push((NOTICE_AI, n));
        self.ai.players[p].flags &= !0x20;
        self.start_melee_ai_for(d, p);
        // Units made while a human had the army belong to no AI list (0x463040 runs when a
        // computer's unit is created): the AI takes them now, workers into its town, so they
        // are not left idle for good.
        for u in 0..self.units.len() as u32 {
            let un = &self.units[u as usize];
            if un.player == n && un.sprite != super::NIL && un.ai == super::NIL && un.status & 1 != 0 && un.unit_type < 0xe4 {
                self.ai_take_unit(d, u);
            }
        }
    }

    /// A human took army `n` back from the AI: it is a human's again and its AI scripts end
    /// (flag 4: the script pool takes them back, 0x45cbd0).
    fn ai_release(&mut self, _d: &Data, n: u8) {
        self.player_types[n as usize] = 2;
        self.ai_taken &= !(1 << n);
        for s in self.ai.scripts.items.iter_mut() {
            if s.player == n as u32 {
                s.flags |= 4;
            }
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
