//! The network lobby's protocol (net_chat_cmds.cpp 0x470000 .. 0x472ec0, net_misc.cpp
//! 0x4c4160 .. 0x4c4980) and the turn exchange the lobby and the ready room share (0x485aa0 /
//! 0x486580 / 0x486530 / 0x487070).
//!
//! Turns: every 250 ms (0x4f40f0) each player sends one turn (Storm's SNetSendTurn, Ordinal
//! 128) and runs the commands of every player's turn once Storm has them all (Ordinal 122,
//! 0x486530 -> 0x486040: the lobby commands 0x3c .. 0x5b). The single player provider hands a
//! turn straight back (`Net::turns`); a network provider goes through snet.rs.
//!
//! Lobby state 0x66fbfa: 1 started (0x4d3240), 2 a joiner waiting for the game's data ('J'),
//! 3 a creator / joiner with it, 4 in the lobby (the first 0x3e), 5 .. 9 the start countdown
//! (0x472060 / 0x4719d0: 6 the dialog stops taking input, 7 / 8 the creator sends 0x48, 9
//! the game starts).
//!
//! Storm messages (Ordinal 127, received by 0x470a30): 'I' a player joined (shown), 'J' the
//! game's slots and map (creator -> joiner), 'K' the force names, 'L' / 'M' chat, 'N' a
//! refusal (creator -> joiner, then dropped), 'O' the map transfer (0x4eafe0), 'P' nothing.
//!
//! A child module of `glue`: it extends `Glue` and uses its private state.

use super::{screen, Glue};
use crate::net::snet;
use crate::setup::slot;
use crate::todo_port;
use std::collections::VecDeque;

/// The turn exchange 0x485aa0 / 0x486580 / 0x486530: the single player provider's turns come
/// straight back (latency 1); a network provider's through Storm (snet.rs).
#[derive(Default)]
pub(super) struct Net {
    /// 0x654880: the commands queued for the next turn (its length is Setup::g654aa0).
    pub buf: Vec<u8>,
    /// The single player provider's turns sent and not yet received.
    pub turns: VecDeque<Vec<u8>>,
    /// 0x57f0b8: per player 0x10000 in the game, 0x20000 a turn arrived; 0x654a80 its data.
    pub flags: [u32; 8],
    pub data: [Vec<u8>; 8],
    /// 0x6556d8: per player, the ready command (0x54) arrived.
    pub ready: [bool; 8],
    /// 0x57ee78: the last receive got a turn.
    pub rx: bool,
    /// 0x6d63d4: the clock of the last turn received.
    pub last_rx: u32,
}

/// 0x66fe20 (0x22 bytes per Storm id, 0x470d10): a player of the game as its join command
/// (0x40) described it.
#[derive(Clone, Default)]
pub(super) struct PlayerInfo {
    /// +0: set.
    pub on: bool,
    /// +2: the join command's flags (& 0xff03, 0x100 with a version hash).
    pub flags: u16,
    /// +4 / +6.
    pub a: u16,
    pub b: u16,
    /// +8: the Storm name (0x19 bytes).
    pub name: Vec<u8>,
}

/// The lobby's globals.
#[derive(Default)]
pub(super) struct Lobby {
    pub net: Net,
    /// 0x66fbfc: the start countdown in turns (0x472060).
    pub countdown: u32,
    /// 0x66fbf9: the download percent last sent (0x3d; 0xff none).
    pub dl_sent: u8,
    /// 0x66ff48: the clock of the last lobby turn (0x4f40f0).
    pub last_turn: u32,
    /// The commands waiting for the next turn (0x471a50 sends them in this order):
    /// 0x6d5c70 start (0x3c), 0x6d5c74 the download percent (0x3d), 0x6d5c30 per slot
    /// (0x3e), 0x6d5c50 per Storm id (0x3f), 0x6d5c7c .. 0x6d5c94 (0x41, 0x42, 0x43, 0x44,
    /// 0x45, 0x48, 0x5b), 0x6d5c78 the join (0x40).
    pub cmd_start: Option<Vec<u8>>,
    pub cmd_dl: Option<Vec<u8>>,
    pub cmd_slot: [Option<Vec<u8>>; 8],
    pub cmd_info: [Option<Vec<u8>>; 8],
    pub cmd_misc: [Option<Vec<u8>>; 7],
    pub cmd_join: Option<Vec<u8>>,
    pub info: [PlayerInfo; 8],
    /// 0x68f4fc: each Storm id's map download percent.
    pub dl_pct: [u32; 8],
    /// 0x5999d4: the slots changed (the dialog's idle proc redraws them, 0x452250).
    pub dirty: bool,
    /// 0x5999e8 / 0x5999e0: the lobby failed with this gluAll string (0x4b8870).
    pub error: Option<usize>,
    /// 0x5999cc.
    pub g5999cc: bool,
    /// 0x6d1204: the creator pressed Start (0x452460).
    pub starting: bool,
    /// 0x596890: the map transfer group (the creator offers the map to the players).
    pub transfer: Option<Transfer>,
    /// 0x57f1d8: whose chat is shown (bit per Storm id; /squelch clears one).
    pub accept: u16,
    /// Lines for the chat list (0x4b8f10: text, color kind 0 own / 1 player / 2 / 3 system).
    pub chat_out: Vec<(Vec<u8>, u8)>,
    /// 0x6d5bc8: the game timer runs (0x472e00 at the start).
    pub timer_on: bool,
    /// 0x44fd30 ran: the dialog takes no input (the countdown's last turns).
    pub locked: bool,
    /// 0x51ca10 from the creator's 0x48: the game's seed.
    pub seed: Option<u32>,
    /// 0x596904 = 1: the game itself is next (no ready room).
    pub start_game: bool,
}

/// 0x596890 (0x472c90): the map file the creator offers (net misc's file transfer group).
#[derive(Default)]
pub(super) struct Transfer {
    /// +0xc: bit per Storm id that joined and has no slot yet (0x4c4980 sets, 0x40 clears).
    pub waiting: u32,
    /// +0x124: the players taking part (0x472ab0) and whether each has the map (+9 == 1).
    pub members: Vec<(u8, bool)>,
    /// The file (its name without the directory, its bytes).
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    /// Per Storm id: the offset sent up to (0x459e00 sends from there).
    pub sent: [u32; 8],
}

/// Transfer chunk size: 0x66fe18 = the turn size 0x57f0d8 - 4 - 9 (the 'O' and data headers).
const CHUNK: usize = 0x200 - 4 - 9;
/// Chunks in flight per player (0x45a230 sends 8 at its start, 0x45a190 one per ack).
const WINDOW: u32 = 8;

/// A map arriving (0x51a30c records, fd_user.cpp): +8 the transfer id, +9 the sender,
/// +0xc the size, +0x10 the CRC, +0x40 bytes so far, +0x44 the buffer; +0x34 the state
/// (1 started, 8 failed).
pub(super) struct Download {
    pub id: u8,
    pub from: u8,
    pub size: u32,
    pub crc: u32,
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub failed: bool,
}

impl Transfer {
    /// 0x472a70: everyone has the map (no joiner pending, every member done).
    pub fn done(&self) -> bool {
        self.waiting == 0 && self.members.iter().all(|m| m.1)
    }
}

impl Download {
    pub fn percent(&self) -> u8 {
        if self.size == 0 {
            return 100;
        }
        (self.data.len() as u64 * 100 / self.size as u64) as u8
    }

    pub fn done(&self) -> bool {
        self.data.len() as u32 == self.size
    }
}

impl Glue {
    // ---- the turn exchange ----

    /// 0x485bd0 (ECX = the bytes): a command joins the next turn.
    pub(super) fn net_485bd0(&mut self, cmd: &[u8]) {
        if (self.setup.g654aa0 as usize) + cmd.len() > 0x200 {
            todo_port!("0x485a40", "the command buffer is full: send it first");
        }
        self.lobby.net.buf.extend_from_slice(cmd);
        self.setup.g654aa0 += cmd.len() as u32;
    }

    /// 0x4c3d20 (`leave`): the turn exchange ends: the tables clear and the game is left
    /// (Ordinal 119; a network game waits a second first so the leave gets out).
    pub(super) fn net_4c3d20(&mut self, _leave: u32) {
        self.setup.g654a80 = [0; 8];
        self.setup.g6556e4 = 0;
        self.setup.g6554b0 = 0;
        self.setup.g6556e0 = 0;
        self.setup.g57eebc = 0;
        self.setup.g654aa0 = 0;
        self.lobby.net.buf.clear();
        if self.setup.multiplayer && self.setup.net_provider != 0 {
            // Sleep(1000): ours flushes the link instead (the transport closes when dropped).
            snet::leave_game();
        }
    }

    /// 0x485aa0: turns are sent while fewer than the provider's in flight (0x57f090 +
    /// 0x6556e4; 1 for the single player provider); an empty buffer sends the no-op command 5.
    pub(super) fn net_485aa0(&mut self) {
        if self.setup.net_provider != 0 {
            let most = self.setup.turns_in_flight + self.setup.g6556e4;
            while snet::turns_in_flight() < most {
                if self.setup.g654aa0 == 0 {
                    self.lobby.net.buf = vec![5];
                    self.setup.g654aa0 = 1;
                }
                let turn = std::mem::take(&mut self.lobby.net.buf);
                snet::send_turn(&turn);
                self.setup.g654aa0 = 0;
            }
            return;
        }
        while self.lobby.net.turns.is_empty() {
            if self.setup.g654aa0 == 0 {
                self.lobby.net.buf = vec![5];
                self.setup.g654aa0 = 1;
            }
            let turn = std::mem::take(&mut self.lobby.net.buf);
            self.lobby.net.turns.push_back(turn);
            self.setup.g654aa0 = 0;
        }
    }

    /// 0x4bb530 (Ordinal_122, SNetReceiveTurns): the oldest turn of every player; flags 0x10000
    /// for a player in the game, 0x20000 with a turn. The single player provider has only ours.
    fn net_4bb530(&mut self) -> bool {
        if self.setup.net_provider != 0 {
            let Some(r) = snet::receive_turns() else { return false };
            for (p, (f, d)) in r.into_iter().enumerate() {
                self.lobby.net.flags[p] = f;
                self.lobby.net.data[p] = d;
            }
            return true;
        }
        let me = (self.setup.local_storm_id as usize).min(7);
        self.lobby.net.flags[me] = 0x10000;
        match self.lobby.net.turns.pop_front() {
            Some(t) => {
                self.lobby.net.data[me] = t;
                self.lobby.net.flags[me] |= 0x20000;
                true
            }
            None => false,
        }
    }

    /// 0x486580: receive; true when a turn came or the last one was less than 2.5 s ago.
    fn net_486580(&mut self) -> bool {
        let now = self.clock_ms;
        if self.net_4bb530() {
            self.lobby.net.last_rx = now;
            self.lobby.net.rx = true;
            return true;
        }
        self.lobby.net.rx = false;
        now.wrapping_sub(self.lobby.net.last_rx) < 0x9c4
    }

    /// 0x486530: the commands of every player whose turn arrived run, Storm ids 7 .. 0
    /// (0x486040, 0x512680 = the id).
    fn net_486530(&mut self) {
        for p in (0..8).rev() {
            if self.lobby.net.flags[p] & 0x20000 == 0 {
                continue;
            }
            let data = self.lobby.net.data[p].clone();
            self.lobby_486040(&data, p as u32);
        }
    }

    /// 0x487070: receive, then (a turn came) send the next one and run the commands. True when
    /// a turn ran.
    pub(super) fn net_487070(&mut self) -> bool {
        if self.screen == screen::EXIT {
            return false;
        }
        let got = self.net_486580();
        if !got && self.setup.g6556e0 != 0 {
            todo_port!("0x4a3380", "network timeout handling");
        }
        if self.lobby.net.rx && self.screen != screen::EXIT {
            self.net_485aa0();
            if self.screen != screen::EXIT {
                if self.setup.g6556e0 == 0 {
                    self.net_486530();
                } else {
                    todo_port!("0x486f50", "replay / observer turns");
                }
                if self.screen != screen::EXIT {
                    self.setup.g57eebc = self.setup.g57eebc.wrapping_add(1);
                    if self.setup.g6556e0 != 0 {
                        todo_port!("0x47cfc0", "turn bookkeeping of a network game");
                    }
                    return true;
                }
            }
        }
        false
    }

    // ---- the lobby commands ----

    /// 0x486040 (EAX = the bytes, `id` the sender's Storm id): the lobby commands of one turn.
    /// A command too short for its length ends the turn.
    fn lobby_486040(&mut self, d: &[u8], id: u32) {
        let mut i = 0;
        while i < d.len() {
            let left = d.len() - i;
            let c = &d[i..];
            let n = match c[0] {
                5 => 1,
                0x3a | 0x3b => 2,
                0x3c => {
                    self.lobby_472060(id);
                    1
                }
                0x3d if left >= 2 => {
                    let p = if c[1] > 100 { 0 } else { c[1] };
                    if let Some(x) = self.lobby.dl_pct.get_mut(id as usize).filter(|x| **x != p as u32) {
                        *x = p as u32;
                        // 0x450210: the slots show it.
                        self.lobby.dirty = true;
                    }
                    2
                }
                0x3e if left >= 6 => {
                    self.lobby_471460(&c[..6], id);
                    6
                }
                0x3f if left >= 8 => {
                    self.lobby_4713e0(&c[..8], id);
                    8
                }
                0x40 if left >= 0x12 => {
                    self.lobby_join_cmd(&c[..0x12], id);
                    0x12
                }
                0x41 if left >= 3 => {
                    self.lobby_471300(&c[..3], id);
                    3
                }
                0x42 if left >= 2 => {
                    self.lobby_471750(c[1], id);
                    2
                }
                0x43 if left >= 2 => {
                    self.lobby_471670(c[1], id);
                    2
                }
                0x44 if left >= 3 => {
                    self.lobby_471570(c[1], c[2], id);
                    3
                }
                0x45 if left >= 3 => {
                    self.lobby_471860(c[1], c[2], id);
                    3
                }
                0x48 if left >= 0xd => {
                    self.lobby_472110(&c[..0xd], id);
                    0xd
                }
                0x54 => {
                    if let Some(r) = self.lobby.net.ready.get_mut(id as usize) {
                        *r = true;
                    }
                    1
                }
                0x5b => {
                    if id == 0 {
                        self.lobby_4c46e0();
                        self.lobby.g5999cc = true;
                    }
                    1
                }
                _ => return,
            };
            i += n;
        }
    }

    /// The creator (0x596888 set: it made the game's description).
    fn is_creator(&self) -> bool {
        self.lobby_host
    }

    /// The melee game types (0x59686d / 0x596871 / 0x596877).
    pub(super) fn melee(&self) -> bool {
        let t = &self.setup.template;
        t[8] != 0 || t[0xc] != 0 || t[0x12] != 0
    }

    /// 0x484f50: a team game (0x57f0b4 and 0x596875).
    pub(super) fn team_game(&self) -> bool {
        self.setup.multiplayer && self.setup.template[0x10] != 0
    }

    /// 0x472060 (0x3c, `id`): the creator started the game: the map is checked (0x4c4870,
    /// Battle.net only), the countdown starts (0x66fbfc = max(24, 0x57f090 * 2 + 4) turns,
    /// state 5) and the game timer's start (0x472e00, 0x59cc7c ..). Else the lobby fails
    /// (0x4b8870).
    fn lobby_472060(&mut self, id: u32) {
        if self.setup.g66fbfa != 4 {
            self.lobby_4b8870(0);
            return;
        }
        if id != 0 {
            return;
        }
        todo_port!("0x472060", "a saved game keeps the slot table (0x640970)");
        self.setup.g66fbfa = 5;
        self.lobby.countdown = (self.setup.turns_in_flight * 2 + 4).max(0x18);
        self.lobby.timer_on = true;
    }

    /// 0x471460 (0x3e from `id`; the creator's only, read by the joiners): a slot's index,
    /// Storm id (0xff none), type, race and force. The first one takes a joiner into the
    /// lobby (state 3 -> 4).
    fn lobby_471460(&mut self, c: &[u8], id: u32) {
        if id != 0 || c[1] >= 8 || (c[2] >= 8 && c[2] != 0xff) || self.is_creator() {
            return;
        }
        let storm = if c[2] == 0xff { u32::MAX } else { c[2] as u32 };
        // 0x470cb0: a human's Storm id must name a player.
        if c[3] == 2 && snet::player_name(storm).is_none() {
            return;
        }
        if self.setup.g66fbfa == 3 {
            self.setup.g66fbfa = 4;
        } else if self.setup.g66fbfa < 4 {
            return;
        }
        let k = c[1] as usize;
        let s = &mut self.setup.slots[k];
        s.fill(0);
        s[..4].copy_from_slice(&(k as u32).to_le_bytes());
        s[slot::STORM..slot::STORM + 4].copy_from_slice(&storm.to_le_bytes());
        s[slot::TYPE] = c[3];
        s[slot::RACE] = c[4];
        s[slot::FORCE] = c[5];
        if c[3] == 2 {
            let name = snet::player_name(storm).unwrap_or_default();
            let n = name.len().min(0x18);
            s[slot::NAME..slot::NAME + n].copy_from_slice(&name[..n]);
        }
        self.setup.index_human_slots();
        self.lobby.dirty = true;
    }

    /// 0x4713e0 (0x3f from the creator): a player's join flags (0x470c00 / 0x470d10).
    fn lobby_4713e0(&mut self, c: &[u8], id: u32) {
        if self.setup.g66fbfa < 4 || id != 0 || c[1] >= 8 || self.is_creator() {
            return;
        }
        let w = |o: usize| u16::from_le_bytes([c[o], c[o + 1]]);
        self.lobby_470d10(c[1] as u32, w(2) & 0xff03, w(4), w(6));
    }

    /// 0x470d10: Storm id `id`'s player info (its name from Storm, Ordinal 113).
    fn lobby_470d10(&mut self, id: u32, flags: u16, a: u16, b: u16) {
        let Some(name) = snet::player_name(id).or_else(|| (self.setup.net_provider == 0 && id == self.setup.local_storm_id).then(|| cz(&self.setup.local_name).to_vec())) else { return };
        if let Some(p) = self.lobby.info.get_mut(id as usize) {
            *p = PlayerInfo { on: true, flags, a, b, name: name[..name.len().min(0x18)].to_vec() };
        }
    }

    /// The 0x40 case of 0x486040 (the creator only): a player asks to join (the creator's own
    /// comes from 0x471fb0 directly). Refusals (0x470480: 'N' + the reason, then the player is
    /// dropped): 4 not in the lobby, 7 another version (+0xb != 5), 8 another build (+0xd /
    /// +0xe, 0x4d9cd0), 2 no open slot. Taken: the slot (Use Map Settings 0x45ade0, melee
    /// 0x4a99c0, a saved game the asked one) becomes the player's (type 2, its Storm name),
    /// its info is set and every slot and player goes out again (0x3e / 0x3f), 'I' tells
    /// everyone, the game is full or not (0x4c4160).
    fn lobby_join_cmd(&mut self, c: &[u8], id: u32) {
        self.lobby.dl_sent = 0xff;
        if !self.is_creator() {
            return;
        }
        let w = |o: usize| u16::from_le_bytes([c[o], c[o + 1]]);
        if self.setup.g66fbfa != 4 {
            self.lobby_470480(id, 4);
            return;
        }
        if w(0xb) != 5 {
            self.lobby_470480(id, 7);
            return;
        }
        if c[0xd] != 0 && version_hash() != u32::from_le_bytes(c[0xe..0x12].try_into().unwrap()) {
            self.lobby_470480(id, 8);
            return;
        }
        if self.save.is_some() {
            todo_port!("0x486040", "joining a saved network game (0x6d1218: the asked slot, 0x66ff34, 0x57f1c0)");
            self.lobby_470480(id, 5);
            return;
        }
        let mut k = if !self.melee() {
            crate::saveload::first_slot_of_largest_force(&self.setup.slots) as u32
        } else {
            self.setup.open_slot()
        };
        // A player already in the table gets its slot back (opened first).
        for i in (0..8).rev() {
            let st = u32::from_le_bytes(self.setup.slots[i][slot::STORM..slot::STORM + 4].try_into().unwrap());
            if st == id {
                self.setup.slot_order[i] = 8;
                self.setup.slots[i][slot::TYPE] = 6;
                k = i as u32;
            }
        }
        if k > 7 || self.setup.slots[k as usize][slot::TYPE] != 6 {
            self.lobby_470480(id, 2);
            return;
        }
        let s = &mut self.setup.slots[k as usize];
        s[..4].copy_from_slice(&k.to_le_bytes());
        s[slot::STORM..slot::STORM + 4].copy_from_slice(&id.to_le_bytes());
        s[slot::TYPE] = 2;
        s[slot::NAME..slot::NAME + 0x19].fill(0);
        let name = snet::player_name(id).or_else(|| (id == self.setup.local_storm_id).then(|| cz(&self.setup.local_name).to_vec())).unwrap_or_default();
        let n = name.len().min(0x18);
        s[slot::NAME..slot::NAME + n].copy_from_slice(&name[..n]);
        self.setup.index_human_slots();
        self.lobby.dirty = true;
        let mut flags = w(7) & 0xff03;
        if c[0xd] != 0 {
            flags |= 0x100;
        }
        self.lobby_470d10(id, flags, w(9), w(0xb));
        for i in (0..8).rev() {
            self.lobby_470f90(i);
        }
        for i in (0..8).rev() {
            self.lobby_470ef0(i);
        }
        self.lobby_470250(id);
        if let Some(t) = self.lobby.transfer.as_mut() {
            if id != self.setup.local_storm_id && !t.members.iter().any(|m| m.0 == id as u8) {
                t.members.push((id as u8, false));
            }
            t.waiting &= !(1 << id);
        }
        let open = self.setup.slots[..8].iter().filter(|s| s[slot::TYPE] == 6).count();
        self.lobby_4c4160(open);
    }

    /// 0x470f90 (EDI = slot `k`): a 0x3e for the slot waits for the next turn.
    fn lobby_470f90(&mut self, k: usize) {
        let s = &self.setup.slots[k];
        self.lobby.cmd_slot[k & 7] = Some(vec![0x3e, s[0], s[slot::STORM], s[slot::TYPE], s[slot::RACE], s[slot::FORCE]]);
    }

    /// 0x470ef0 (EAX = slot `k`): a 0x3f with the info of the slot's Storm id.
    fn lobby_470ef0(&mut self, k: usize) {
        let id = u32::from_le_bytes(self.setup.slots[k][slot::STORM..slot::STORM + 4].try_into().unwrap());
        if id >= 8 {
            return;
        }
        let p = &self.lobby.info[id as usize];
        let (f, a, b) = if p.on { (p.flags, p.a, p.b) } else { (0, 0, 0) };
        let mut m = vec![0x3f, id as u8];
        for v in [f, a, b] {
            m.extend_from_slice(&v.to_le_bytes());
        }
        self.lobby.cmd_info[id as usize] = Some(m);
    }

    /// 0x471300 (0x41 from `id`, creator): slot c[1] (8: the sender's own) takes race c[2]
    /// if 0x4aaab0 allows; in a team game with a computer in the force, the whole force does.
    fn lobby_471300(&mut self, c: &[u8], id: u32) {
        if !self.is_creator() || self.setup.g66fbfa < 4 {
            return;
        }
        let k = if c[1] == 8 { self.setup.slot_by_storm.get(id as usize).copied().unwrap_or(8) as u8 } else { c[1] };
        let mine = self.setup.slot_by_storm.get(id as usize).copied().unwrap_or(8);
        if !self.lobby_4aaab0(mine, k, c[2]) {
            return;
        }
        let f = self.setup.slots[k as usize][slot::FORCE];
        if self.team_game() && self.force_has_computer(f) {
            for i in 0..8 {
                if self.setup.slots[i][slot::FORCE] == f {
                    self.setup.slots[i][slot::RACE] = c[2];
                    self.lobby_470f90(i);
                }
            }
        } else {
            self.setup.slots[k as usize][slot::RACE] = c[2];
            self.lobby_470f90(k as usize);
        }
        self.lobby.dirty = true;
    }

    /// 0x4aaab0 (ESI = the asker's slot, BL = the slot, AL = the race): the race may be set:
    /// Zerg / Terran / Protoss / random, in a melee game or a user selectable slot, with no
    /// saved game; a team member only its own force's slots (a force with a computer: any of
    /// them), else only its own slot or a computer's.
    fn lobby_4aaab0(&self, asker: u32, k: u8, race: u8) -> bool {
        if asker >= 8 || k >= 8 || !matches!(race, 0 | 1 | 2 | 6) {
            return false;
        }
        if !(self.melee() || self.setup.user_selectable[k as usize] != 0) || self.save.is_some() {
            return false;
        }
        let force = |i: usize| self.setup.slots[i][slot::FORCE];
        if asker == 0 {
            if self.team_game() && force(asker as usize) != force(k as usize) && !self.force_has_computer(force(k as usize)) {
                return false;
            }
            return self.team_game() || k as u32 == asker || self.setup.slots[k as usize][slot::TYPE] == 5;
        }
        (self.team_game() && force(asker as usize) == force(k as usize)) || (!self.team_game() && k as u32 == asker)
    }

    /// 0x484df0: a slot of force `f` is a computer (1 or 5).
    fn force_has_computer(&self, f: u8) -> bool {
        self.setup.slots[..8].iter().any(|s| s[slot::FORCE] == f && matches!(s[slot::TYPE], 1 | 5))
    }

    /// 0x484dc0 (BL = `f`): the first slot of force `f` that is open (6) or 9; 8 none.
    fn force_open_slot(&self, f: u8) -> u8 {
        (0..8).find(|&i| self.setup.slots[i][slot::FORCE] == f && matches!(self.setup.slots[i][slot::TYPE], 6 | 9)).map_or(8, |i| i as u8)
    }

    /// 0x471750 (0x42 from `id`, creator, a team game): the sender moves to team `t`'s open
    /// slot (0x484dc0) if the team has no computer and 0x44f6d0 allows it.
    fn lobby_471750(&mut self, t: u8, id: u32) {
        if !self.is_creator() || self.setup.g66fbfa < 4 || !self.team_game() || t == 0 {
            return;
        }
        let teams = if self.setup.template[0] == 0xf { 2 } else { self.setup.template[0x10] };
        if t > teams || self.force_has_computer(t) {
            return;
        }
        let k = self.setup.slot_by_storm.get(id as usize).copied().unwrap_or(8);
        if k >= 8 || self.setup.slots[k as usize][slot::FORCE] == t || self.setup.slots[k as usize][slot::TYPE] != 2 {
            return;
        }
        let to = self.force_open_slot(t);
        if to >= 8 || !self.lobby_44f6d0(k as u8, to) {
            return;
        }
        self.lobby_4a9130(k as u8, to);
        self.lobby_470f90(k as usize);
        self.lobby_470f90(to as usize);
        self.lobby.dirty = true;
    }

    /// 0x44f6d0: a move from slot `a` to `b` is allowed: in the lobby (state 4) always;
    /// else a team change needs 0x484ef0 != 1.
    fn lobby_44f6d0(&self, a: u8, b: u8) -> bool {
        if self.setup.g66fbfa == 4 {
            return true;
        }
        if self.team_game() && self.setup.slots[a as usize][slot::FORCE] != self.setup.slots[b as usize][slot::FORCE] {
            todo_port!("0x484ef0", "the slots of a race of type 1 or 2");
        }
        true
    }

    /// 0x471670 (0x43 from `id`, creator, Use Map Settings): the sender moves to force `f`'s
    /// next open slot (its own force: the one after it, 0x45a8b0; else the first, 0x45a900),
    /// unless the force has a computer (0x45a920).
    fn lobby_471670(&mut self, f: u8, id: u32) {
        if !self.is_creator() || self.setup.g66fbfa < 4 || self.melee() || f == 0 || f >= 5 {
            return;
        }
        if self.setup.slots[..8].iter().any(|s| s[slot::FORCE] == f && s[slot::TYPE] == 5) {
            return;
        }
        let k = self.setup.slot_by_storm.get(id as usize).copied().unwrap_or(8) as usize;
        if k >= 8 || self.setup.slots[k][slot::TYPE] != 2 {
            return;
        }
        let open = |i: usize| self.setup.slots[i][slot::FORCE] == f && self.setup.slots[i][slot::TYPE] == 6;
        let to = if self.setup.slots[k][slot::FORCE] == f {
            (1..=8).map(|d| (k + d) & 7).find(|&i| open(i)).unwrap_or(8)
        } else {
            (0..8).find(|&i| open(i)).unwrap_or(8)
        };
        if to < 8 {
            self.lobby_4a9130(k as u8, to as u8);
            self.lobby_470f90(k);
            self.lobby_470f90(to);
            self.lobby.dirty = true;
        }
    }

    /// 0x4a9130 (EBX = `a`, `b`): the player of slot `a` goes to slot `b`; `a` takes the lobby
    /// copy of `b`. Each keeps its own race and force.
    fn lobby_4a9130(&mut self, a: u8, b: u8) {
        let (a, b) = (a as usize, b as usize);
        let (ra, fa) = (self.setup.slots[a][slot::RACE], self.setup.slots[a][slot::FORCE]);
        let (rb, fb) = (self.setup.slots[b][slot::RACE], self.setup.slots[b][slot::FORCE]);
        self.setup.slots[a] = self.setup.slots[b];
        self.setup.slots[a][..4].copy_from_slice(&(a as u32).to_le_bytes());
        self.setup.slots[a][slot::RACE] = ra;
        self.setup.slots[a][slot::FORCE] = fa;
        self.setup.slots[b] = self.setup.lobby[b];
        self.setup.slots[b][..4].copy_from_slice(&(b as u32).to_le_bytes());
        self.setup.slots[b][slot::RACE] = rb;
        self.setup.slots[b][slot::FORCE] = fb;
        self.setup.index_human_slots();
    }

    /// 0x471570 (0x44 slot `k`, `what` 0 computer / 1 open / 2 closed; the creator's own): a
    /// slot changes (0x4a9010 / 0x4a8f30 / 0x4a8e50); a team game sends the whole force.
    fn lobby_471570(&mut self, k: u8, what: u8, id: u32) {
        if !self.is_creator() || id != 0 || self.setup.g66fbfa < 4 || k >= 8 || what >= 3 {
            return;
        }
        let me = self.setup.slot_by_storm[0];
        if me >= 8 {
            return;
        }
        match what {
            0 => self.lobby_4a9010(me as u8, k),
            1 => self.lobby_4a8f30(k, me as u8, 6),
            _ => self.lobby_4a8f30(k, me as u8, 8),
        }
        if self.team_game() {
            let f = self.setup.slots[k as usize][slot::FORCE];
            for i in (0..8).rev() {
                if self.setup.slots[i][slot::FORCE] == f {
                    self.lobby_470f90(i);
                }
            }
        } else {
            self.lobby_470f90(k as usize);
        }
        self.lobby.dirty = true;
        let open = self.setup.slots[..8].iter().filter(|s| s[slot::TYPE] == 6).count();
        self.lobby_4c4160(open);
    }

    /// 0x4a8f30 (`ty` 6) / 0x4a8e50 (`ty` 8) (EAX = slot `k`, `me` the creator's slot): an open
    /// / closed slot (from the lobby copy, the race kept); not a human's; in a team game with a
    /// computer in the force, the whole force.
    fn lobby_4a8f30(&mut self, k: u8, me: u8, ty: u8) {
        let k = k as usize;
        if self.setup.slots[k][slot::TYPE] == ty || k == me as usize {
            return;
        }
        let f = self.setup.slots[k][slot::FORCE];
        if self.team_game() && self.force_has_computer(f) {
            for i in 0..8 {
                if self.setup.slots[i][slot::FORCE] == f {
                    let r = self.setup.slots[i][slot::RACE];
                    self.setup.slots[i] = self.setup.lobby[i];
                    self.setup.slots[i][slot::TYPE] = ty;
                    self.setup.slots[i][slot::RACE] = r;
                }
            }
            return;
        }
        if self.setup.slots[k][slot::TYPE] != 2 {
            let r = self.setup.slots[k][slot::RACE];
            self.setup.slots[k] = self.setup.lobby[k];
            self.setup.slots[k][slot::TYPE] = ty;
            self.setup.slots[k][slot::RACE] = r;
        }
    }

    /// 0x4a9010 (EAX = `me`, `k`): slot `k` becomes a computer (5, from the lobby copy); not a
    /// computer / reserved slot already, not one's own; in a team game not one's own force,
    /// and only a force without humans (all of it).
    fn lobby_4a9010(&mut self, me: u8, k: u8) {
        let k = k as usize;
        let ty = self.setup.slots[k][slot::TYPE];
        if matches!(ty, 5 | 3 | 4 | 7) {
            return;
        }
        let f = self.setup.slots[k][slot::FORCE];
        if self.team_game() {
            if self.setup.slots[me as usize][slot::FORCE] == f {
                return;
            }
            if self.setup.slots[..8].iter().any(|s| s[slot::FORCE] == f && s[slot::TYPE] == 2) {
                return;
            }
            for i in 0..8 {
                if self.setup.slots[i][slot::FORCE] == f {
                    self.setup.slots[i] = self.setup.lobby[i];
                    self.setup.slots[i][slot::TYPE] = 5;
                }
            }
            return;
        }
        if me as usize == k {
            return;
        }
        if ty != 2 {
            self.setup.slots[k] = self.setup.lobby[k];
            self.setup.slots[k][slot::TYPE] = 5;
        }
    }

    /// 0x471860 (0x45 slots `a`, `b`; the creator's own): the two slots swap (each keeps its
    /// index and force). An open combo of either local slot closes (0x41a530).
    fn lobby_471860(&mut self, a: u8, b: u8, id: u32) {
        if !self.is_creator() || id != 0 || self.setup.g66fbfa < 4 || a == 0 || a >= 8 || b == 0 || b >= 8 || a == b {
            return;
        }
        let (a, b) = (a as usize, b as usize);
        let (sa, sb) = (self.setup.slots[a], self.setup.slots[b]);
        self.setup.slots[a] = sb;
        self.setup.slots[a][..4].copy_from_slice(&(a as u32).to_le_bytes());
        self.setup.slots[a][slot::FORCE] = sa[slot::FORCE];
        self.setup.slots[b] = sa;
        self.setup.slots[b][..4].copy_from_slice(&(b as u32).to_le_bytes());
        self.setup.slots[b][slot::FORCE] = sb[slot::FORCE];
        self.setup.index_human_slots();
        self.lobby_470f90(a);
        self.lobby_470f90(b);
        self.lobby.dirty = true;
    }

    /// 0x472110 (0x48 from the creator, state 8): the game's seed (0x51ca10) and each lobby
    /// position's slot (0x66ff34); a position past 8 fails the start (0x4c3d20, network.tbl
    /// 0x60).
    fn lobby_472110(&mut self, c: &[u8], id: u32) {
        if id != 0 || self.setup.g66fbfa != 8 {
            return;
        }
        self.lobby.seed = Some(u32::from_le_bytes(c[1..5].try_into().unwrap()));
        for k in (0..8).rev() {
            let v = c[5 + k];
            if v > 8 {
                self.net_4c3d20(3);
                self.lobby.error = Some(0x60);
                return;
            }
            self.setup.slot_order[k] = v;
        }
    }

    /// 0x4c46e0 (0x5b from the creator): the game mode's bit 1 set (Ordinal 107 / 130) and
    /// the full flag from the open slots (0x4c4160).
    fn lobby_4c46e0(&mut self) {
        if !self.is_creator() {
            return;
        }
        snet::set_game_mode(snet::game_mode() | 1);
        let open = self.setup.slots[..8].iter().filter(|s| s[slot::TYPE] == 6).count();
        self.lobby_4c4160(open);
    }

    /// 0x4c4160 (`open` slots): the game is listed as joinable or not (Ordinal 130 bit 2).
    fn lobby_4c4160(&mut self, open: usize) {
        let m = snet::game_mode();
        snet::set_game_mode(if open == 0 { m | 2 } else { m & !2 });
    }

    // ---- the pending commands ----

    /// 0x471fb0 (the creator's own join: 0x4d4130, run at once by 0x471d00 there) / 0x470840
    /// (a joiner's, after 'J'): the 0x40 command.
    fn lobby_471fb0(&mut self, b1: u8, b2: u8, save_id: u32) -> Vec<u8> {
        let mut m = vec![0x40, b1, b2];
        m.extend_from_slice(&save_id.to_le_bytes());
        m.extend_from_slice(&(self.lobby_info_flags()).to_le_bytes());
        m.extend_from_slice(&1u16.to_le_bytes());
        m.extend_from_slice(&5u16.to_le_bytes());
        // +0xd: 0x5124d0 (the build has a version hash), +0xe the hash (0x4d9cd0).
        m.push(0);
        m.extend_from_slice(&version_hash().to_le_bytes());
        m
    }

    /// +7 of the join command: 0x66ff30 (the local player's flags; 0 here).
    fn lobby_info_flags(&self) -> u16 {
        0
    }

    /// 0x471a50 (each lobby turn): the pending commands join the turn (0x485bd0) in their
    /// order; state 7 becomes 8 (the 0x48 is out); from state 7 nothing more goes.
    fn lobby_471a50(&mut self) {
        if self.setup.g66fbfa == 7 {
            self.setup.g66fbfa = 8;
        } else if self.setup.g66fbfa > 6 {
            return;
        }
        let mut out = Vec::new();
        out.extend(self.lobby.cmd_start.take());
        out.extend(self.lobby.cmd_dl.take());
        for k in (0..8).rev() {
            out.extend(self.lobby.cmd_slot[k].take());
        }
        for k in (0..8).rev() {
            out.extend(self.lobby.cmd_info[k].take());
        }
        for c in self.lobby.cmd_misc.iter_mut() {
            out.extend(c.take());
        }
        out.extend(self.lobby.cmd_join.take());
        for c in out {
            self.net_485bd0(&c);
        }
    }

    /// 0x4710b0: the pending commands are dropped.
    pub(super) fn lobby_4710b0(&mut self) {
        let l = &mut self.lobby;
        l.cmd_start = None;
        l.cmd_dl = None;
        l.cmd_join = None;
        l.cmd_slot = Default::default();
        l.cmd_info = Default::default();
        l.cmd_misc = Default::default();
    }

    /// 0x4719d0 (each lobby turn): the countdown; at 0x57f090 * 2 + 4 left the dialog takes no
    /// input (0x44fd30, state 6), at 0x57f090 + 2 the creator sends 0x48 (state 7, 0x471270;
    /// a joiner: 8), at 0 the game starts (9).
    fn lobby_4719d0(&mut self) {
        if self.lobby.countdown == 0 {
            return;
        }
        self.lobby.countdown -= 1;
        let n = self.lobby.countdown;
        if n == self.setup.turns_in_flight * 2 + 4 {
            self.lobby.locked = true;
            self.setup.g66fbfa = 6;
        }
        if n == self.setup.turns_in_flight + 2 {
            if self.is_creator() {
                self.setup.g66fbfa = 7;
                self.lobby_471270();
            } else {
                self.setup.g66fbfa = 8;
            }
        }
        if n == 0 {
            self.setup.g66fbfa = 9;
        }
    }

    /// 0x471270: the 0x48: the seed (time()) and each lobby position's slot (0x66ff34).
    fn lobby_471270(&mut self) {
        let seed = super::create::create_seed();
        let mut m = vec![0x48];
        m.extend_from_slice(&seed.to_le_bytes());
        m.extend_from_slice(&self.setup.slot_order);
        self.lobby.cmd_misc[5] = Some(m);
    }

    /// 0x471010 (BL = the percent): a 0x3d when it changed.
    fn lobby_471010(&mut self, pct: u8) {
        if self.lobby.dl_sent != pct {
            self.lobby.dl_sent = pct;
            self.lobby.cmd_dl = Some(vec![0x3d, pct]);
        }
    }

    /// 0x452460 (Ok, the creator): the map transfer must be done (0x472a70), the game is no
    /// longer joinable (Ordinal 130 bit 8) and the start (0x3c) goes out (0x6d1204).
    pub(super) fn lobby_452460(&mut self) -> bool {
        if self.lobby.transfer.as_ref().is_some_and(|t| !t.done()) {
            return false;
        }
        snet::set_game_mode(snet::game_mode() | 8);
        self.lobby.starting = true;
        self.lobby.cmd_start = Some(vec![0x3c]);
        true
    }

    /// 0x452370 / 0x470ea0 (AL = slot, `race`): a 0x41 (the local slot as 8).
    pub(super) fn lobby_452370(&mut self, k: u8, race: u8) {
        let k = if k as u32 == self.setup.local_slot[0] { 8 } else { k };
        self.lobby.cmd_misc[0] = Some(vec![0x41, k, race]);
    }

    /// 0x470df0: a 0x44 (slot `k`, 0 computer / 1 open / 2 closed).
    pub(super) fn lobby_470df0(&mut self, k: u8, what: u8) {
        self.lobby.cmd_misc[3] = Some(vec![0x44, k, what]);
    }

    /// 0x452350 / 0x470db0: a 0x45 (swap `a` and `b`).
    pub(super) fn lobby_452350(&mut self, a: u8, b: u8) -> bool {
        if a < 8 && b < 8 && a != b {
            self.lobby.cmd_misc[4] = Some(vec![0x45, a, b]);
            return true;
        }
        false
    }

    /// The team (0x42) / force (0x43) a player asks for.
    pub(super) fn lobby_team(&mut self, t: u8) {
        if self.melee() {
            self.lobby.cmd_misc[1] = Some(vec![0x42, t]);
        } else {
            self.lobby.cmd_misc[2] = Some(vec![0x43, t]);
        }
    }

    // ---- the lobby ----

    /// 0x4d4130 (gluChat's first idle pass): the lobby starts. A saved game is read first
    /// (0x4cfe40 .., not ported). The slot tables and player infos clear; the creator fills
    /// the slots from the lobby copy (0x4a8de0 for melee, 0x470150, 0x4aa4b0, 0x4a91e0); the
    /// turns start (0x4d3240, 0x485aa0), the map transfer (0x4c4c50) and the state: a joiner
    /// 2, the creator 3 with its own join run at once (0x471fb0 -> 0x471d00). False: the
    /// lobby failed (0x4b8870 or network.tbl 100).
    pub(super) fn lobby_4d4130(&mut self, files: &crate::storage::Storage) -> bool {
        self.lobby.starting = false;
        if self.setup.local_storm_id == 0 && !self.lobby_host {
            self.lobby_4b8870(0);
            return false;
        }
        self.setup.g596bb0 = 0;
        self.setup.slot_order = [8; 8];
        if self.save.is_some() {
            todo_port!("0x4d4130", "a saved network game (0x4cfe40 / 0x4cf950 / 0x4cf5f0 / 0x4aa620)");
        }
        for s in self.setup.slots.iter_mut() {
            s.fill(0);
        }
        self.setup.index_human_slots();
        self.lobby.info = Default::default();
        self.lobby.dl_pct = [0; 8];
        if self.lobby_host {
            if self.melee() {
                self.lobby_4a8de0();
            }
            self.setup.snapshot_types();
            self.lobby_4aa4b0();
            self.setup.copy_lobby_to_slots();
        }
        self.setup.local_slot = [u32::MAX; 2];
        self.lobby.accept = 0xffff;
        self.setup.reset_game_tables();
        if !self.lobby_4c4c50(files) {
            self.lobby_4d3860();
            self.error_popup(files, 100);
            return false;
        }
        self.lobby.dl_sent = 0xff;
        self.lobby.last_turn = self.clock_ms;
        self.net_485aa0();
        self.lobby.countdown = 0;
        self.setup.g66fbfa = 2;
        if self.lobby_host {
            self.setup.g66fbfa = 3;
            let c = self.lobby_471fb0(0, 0, 0);
            self.lobby_join_own(&c);
        }
        true
    }

    /// 0x471d00 for the creator's own 0x40 (0x471fb0 with its last argument set): it takes
    /// its slot like any joiner, then the lobby is open (state 4 comes with its first 0x3e).
    fn lobby_join_own(&mut self, c: &[u8]) {
        self.setup.g66fbfa = 4;
        self.lobby_join_cmd(c, self.setup.local_storm_id);
    }

    /// 0x4a8de0: melee: the lobby copy's computer / reserved slots (5, 3, 4, 7) are open and
    /// no race is fixed (+9 = 0).
    fn lobby_4a8de0(&mut self) {
        for s in self.setup.lobby[..8].iter_mut().rev() {
            if matches!(s[slot::TYPE], 5 | 3 | 4 | 7) {
                s[slot::TYPE] = 6;
            }
            s[slot::RACE] = 0;
        }
    }

    /// 0x4aa4b0: a team game's lobby copy: `team_size` open slots per team (force 1..), the
    /// first slot's race; Team CTF (game type 0xf) puts the first 0x596867 open / computer
    /// slots on force 1, the rest on 2.
    fn lobby_4aa4b0(&mut self) {
        let t = self.setup.template;
        if !(self.setup.multiplayer && t[0x10] != 0) && t[0] != 0xf {
            return;
        }
        if self.setup.multiplayer && t[0x10] != 0 {
            let teams = if t[0] == 0xf { 2 } else { t[0x10] };
            let size = crate::bw::players::team_size(t[0x10]) as usize;
            let race0 = self.setup.lobby[0][slot::RACE];
            let mut k = 0;
            for team in 0..teams {
                for _ in 0..size {
                    if k < 8 {
                        self.setup.lobby[k][slot::TYPE] = 6;
                        self.setup.lobby[k][slot::RACE] = race0;
                        self.setup.lobby[k][slot::FORCE] = team + 1;
                        k += 1;
                    }
                }
            }
        }
        if t[0] != 0xf {
            return;
        }
        let mut force = [0u8; 8];
        let usable = |s: &crate::setup::Slot| matches!(s[slot::TYPE], 6 | 5 | 3 | 4 | 7);
        let (mut i, mut n) = (0, 0);
        while n < t[2] as usize && i < 8 {
            if usable(&self.setup.lobby[i]) {
                force[i] = 1;
                n += 1;
            }
            i += 1;
        }
        if i <= 7 || t[2] == 0 {
            for j in i..8 {
                if usable(&self.setup.lobby[j]) {
                    force[j] = 2;
                }
            }
        }
        let race0 = self.setup.lobby[0][slot::RACE];
        for (s, f) in self.setup.lobby[..8].iter_mut().zip(force) {
            if s[slot::TYPE] != 0 {
                s[slot::TYPE] = 6;
                s[slot::RACE] = race0;
                s[slot::FORCE] = f;
            }
        }
    }

    /// 0x4c4c50: the map transfer starts (0x4eaf70: maps\download\ for what arrives); the
    /// creator offers its map (0x472c90 -> 0x596890: a .scm / .scx / .rep under 0x20
    /// characters). False when that fails. Ours keeps what arrives in memory
    /// (`Setup::map_data`), so a browser can take it too.
    fn lobby_4c4c50(&mut self, _files: &crate::storage::Storage) -> bool {
        self.lobby.transfer = None;
        self.map_rx = None;
        if !self.lobby_host {
            self.setup.map_data = None;
        }
        if !self.lobby_host || self.save.is_some() {
            return true;
        }
        let path = String::from_utf8_lossy(cz(&self.setup.map_path)).into_owned();
        let name = path.rsplit(['\\', '/']).next().unwrap_or("").as_bytes().to_vec();
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        if name.len() >= 0x20 || !matches!(ext.as_str(), "scx" | "scm" | "rep") {
            return false;
        }
        let data = match self.setup.map_data.clone() {
            Some(d) => d,
            None => match std::fs::read(&path) {
                Ok(d) => d,
                Err(_) => return false,
            },
        };
        self.lobby.transfer = Some(Transfer { name, data, ..Transfer::default() });
        true
    }

    /// 0x472900 (to the joiner `id`): the offer, 'O' 1: the transfer id, size, CRC and name
    /// (0x45a600 makes the joiner's record).
    fn lobby_offer(&mut self, id: u32) {
        let Some(t) = self.lobby.transfer.as_ref() else { return };
        let mut m = vec![b'O', 1, 0];
        m.extend_from_slice(&(t.data.len() as u32).to_le_bytes());
        m.extend_from_slice(&crate::bw::save::crc32(&t.data).to_le_bytes());
        m.extend_from_slice(&t.name);
        m.push(0);
        snet::send_message(id as u8, &m);
    }

    /// 0x459e00 (the creator, to `id`): the next chunks while fewer than `WINDOW` are unacked
    /// ('O' 4: id, 0, offset u32, length u16, the bytes).
    fn lobby_459e00(&mut self, id: u32, acked: u32) {
        let Some(t) = self.lobby.transfer.as_mut() else { return };
        let k = id as usize & 7;
        while (t.sent[k] as usize) < t.data.len() && t.sent[k] < acked + WINDOW * CHUNK as u32 {
            let at = t.sent[k] as usize;
            let n = CHUNK.min(t.data.len() - at);
            let mut m = vec![b'O', 4, 0, 0];
            m.extend_from_slice(&(at as u32).to_le_bytes());
            m.extend_from_slice(&(n as u16).to_le_bytes());
            m.extend_from_slice(&t.data[at..at + n]);
            snet::send_message(id as u8, &m);
            t.sent[k] += n as u32;
        }
    }

    /// 0x4eafe0 ('O' from `from`, the transfer's sub-message): 0 a player's state (0x472a00), 1
    /// an offer (0x45a600), 2 a request from an offset (0x45a230), 3 a cancel (0x45a1f0), 4 data
    /// (0x45a390), 5 an ack (0x45a190), 6 the end (0x45a160).
    fn lobby_4eafe0(&mut self, _files: &crate::storage::Storage, m: &[u8], from: u32) {
        let Some(&k) = m.first() else { return };
        let u32_at = |o: usize| m.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        match k {
            // A joiner's state: 1 it has the map.
            0 if m.len() == 8 && self.lobby_host => {
                if m[3] == 1 {
                    if let Some(t) = self.lobby.transfer.as_mut() {
                        if let Some(x) = t.members.iter_mut().find(|x| x.0 as u32 == from) {
                            x.1 = true;
                        } else {
                            t.members.push((from as u8, true));
                        }
                    }
                    self.lobby.dirty = true;
                }
            }
            1 if m.len() >= 10 && from == 0 && !self.lobby_host => {
                let (Some(size), Some(crc)) = (u32_at(2), u32_at(6)) else { return };
                let name = cz(&m[10..]).to_vec();
                if name.len() >= 0x20 {
                    return;
                }
                // 0x45a570: a map of that name and CRC here already.
                self.setup.map_path.fill(0);
                self.setup.map_path[..name.len()].copy_from_slice(&name);
                if let Some(d) = local_map(&name).filter(|d| d.len() as u32 == size && crate::bw::save::crc32(d) == crc) {
                    self.setup.map_data = Some(d);
                    snet::send_message(0, &[b'O', 0, m[1], 0, 1, 0, 0, 0, 0]);
                    return;
                }
                self.map_rx = Some(Download { id: m[1], from: from as u8, size, crc, name, data: Vec::with_capacity(size as usize), failed: false });
                let mut r = vec![b'O', 2, m[1], 0];
                r.extend_from_slice(&0u32.to_le_bytes());
                snet::send_message(0, &r);
            }
            2 if m.len() == 7 && self.lobby_host => {
                if let Some(t) = self.lobby.transfer.as_mut() {
                    t.sent[from as usize & 7] = u32_at(3).unwrap_or(0);
                }
                self.lobby_459e00(from, u32_at(3).unwrap_or(0));
            }
            5 if m.len() == 7 && self.lobby_host => {
                self.lobby_459e00(from, u32_at(3).unwrap_or(0));
            }
            3 | 6 => {
                if !self.lobby_host {
                    self.map_rx = None;
                }
            }
            4 if m.len() >= 9 && !self.lobby_host => {
                let Some(d) = self.map_rx.as_mut().filter(|d| d.id == m[1] && d.from as u32 == from && !d.failed) else { return };
                let (at, n) = (u32_at(3).unwrap_or(0), u16::from_le_bytes([m[7], m[8]]) as usize);
                if m.len() != 9 + n || at as usize != d.data.len() || at as usize + n > d.size as usize {
                    return;
                }
                d.data.extend_from_slice(&m[9..]);
                let mut ack = vec![b'O', 5, d.id, 0];
                ack.extend_from_slice(&(d.data.len() as u32).to_le_bytes());
                snet::send_message(0, &ack);
                if d.done() {
                    if crate::bw::save::crc32(&d.data) != d.crc {
                        // 0x45a390: a bad CRC starts over.
                        d.data.clear();
                        let mut r = vec![b'O', 2, d.id, 0];
                        r.extend_from_slice(&0u32.to_le_bytes());
                        snet::send_message(0, &r);
                        return;
                    }
                    // 0x45a290 writes maps\download\<name>; ours keeps it in memory.
                    self.setup.map_data = Some(d.data.clone());
                    snet::send_message(0, &[b'O', 0, d.id, 0, 1, 0, 0, 0, 0]);
                }
                self.lobby.dirty = true;
            }
            _ => {}
        }
    }

    /// 0x4d3860: the lobby ends: the transfer stops, the pending commands go, the game is left
    /// (0x4c3d20 0x40000001), the slots clear, 0x596888 = 0.
    pub(super) fn lobby_4d3860(&mut self) {
        self.lobby.transfer = None;
        self.lobby_471a50_drop();
        self.lobby_4710b0();
        self.net_4c3d20(0x4000_0001);
        self.lobby.timer_on = false;
        self.setup.local_slot = [u32::MAX; 2];
        self.setup.local_storm_id = u32::MAX;
        for s in self.setup.slots.iter_mut() {
            s.fill(0);
        }
        self.lobby_host = false;
    }

    /// 0x471a50's frees without the sends (0x4d3860 calls it before 0x4710b0: in state > 6 it
    /// sends nothing; ours drops the turn buffer with the game).
    fn lobby_471a50_drop(&mut self) {
        self.lobby.net.buf.clear();
    }

    /// 0x4b8870 (AL = the refusal): the lobby fails with gluAll 0x74 .. 0x7d (0x5999e8).
    pub(super) fn lobby_4b8870(&mut self, why: u8) {
        self.lobby.error = Some(match why {
            1 => 0x75,
            2 => 0x76,
            3 => 0x77,
            4 => 0x78,
            5 => 0x7a,
            6 => 0x7d,
            7 => 0x79,
            8 => 0x7b,
            9 => 0x7c,
            _ => 0x74,
        });
    }

    /// 0x4f40f0 (the lobby's idle pass): every 250 ms a turn (0x487070): the download percent
    /// (0x4f4080), the pending commands (0x471a50), the next turn (0x485aa0), the slots
    /// redrawn (0x5999d4), the countdown (0x4719d0); true when the game starts (state 9). No
    /// turn for 20 s: the players are dropped (0x4a3010).
    pub(super) fn lobby_4f40f0(&mut self) -> bool {
        let now = self.clock_ms;
        if now.wrapping_sub(self.lobby.last_turn) < 0xfa {
            return false;
        }
        if self.net_487070() {
            self.lobby_4f4080();
            self.lobby_471a50();
            self.lobby.last_turn = now;
            self.net_485aa0();
            self.lobby.dirty = true;
            self.lobby_4719d0();
            return self.setup.g66fbfa == 9;
        }
        if now.wrapping_sub(self.lobby.last_turn) > 19_999 {
            todo_port!("0x4a3010", "the lobby's players stopped answering: drop them");
            self.lobby_4b8870(0);
        }
        false
    }

    /// 0x4f4080: in the lobby, the map download percent goes out (0x45a050 / 0x471010); a
    /// failed download fails the lobby.
    fn lobby_4f4080(&mut self) {
        if self.setup.g66fbfa != 4 {
            return;
        }
        let pct = self.download_percent();
        self.lobby_471010(pct);
    }

    /// 0x45a050: how much of the map arrived (100 with nothing to get).
    fn download_percent(&self) -> u8 {
        self.map_rx.as_ref().map_or(100, |d| d.percent())
    }

    /// 0x4d4340 (gluChat's idle proc 0x4b9b10 each pass): the chat commands (0x47f590:
    /// Battle.net), the lobby turn (0x4f40f0); without a start the Storm messages (0x470a30:
    /// their letter, 0x51 when the game is gone); at the start the screen after it: melee / a
    /// saved game the game itself (0x596904 = 1), else the ready room of the local race (Zerg
    /// 8, Terran 7, Protoss 9), 0x53.
    pub(super) fn lobby_4d4340(&mut self, files: &crate::storage::Storage) -> u32 {
        if !self.lobby_4f40f0() {
            return self.lobby_470a30(files);
        }
        self.lobby_4710b0();
        if self.melee() || self.save.is_some() {
            self.lobby.start_game = true;
            return 0x53;
        }
        let me = self.setup.local_slot[0] as usize;
        self.screen = match self.setup.slots.get(me).map_or(0, |s| s[slot::RACE]) {
            0 => screen::READY_Z,
            2 => screen::READY_P,
            _ => screen::READY_T,
        };
        0x53
    }

    /// Ours: the game the lobby started runs on our rollback session (net/mod.rs) over the
    /// lobby's link instead of Storm's turns (0x486f50): every Storm id plays the slot the
    /// lobby gave it (0x57ee7c), the input delay is fixed.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn lobby_net_game(&mut self) -> Option<crate::net::live::NetGame> {
        const DELAY: u32 = 2;
        let (mut tr, host, ids) = snet::take_transport()?;
        tr.set_first(DELAY);
        let me = self.setup.local_storm_id as u8;
        let table: Vec<(u8, u8)> = ids.iter().map(|&i| (i, i)).collect();
        let mut se = crate::net::Session::new(me, &table, DELAY);
        let humans: Vec<(u8, u8)> = ids.iter().map(|&i| (i, self.setup.slot_by_storm2[i as usize] as u8)).collect();
        se.map_humans(&humans);
        crate::net::live::NetGame::new(se, tr, host, Vec::new()).map_err(|e| eprintln!("net: {e}")).ok()
    }

    // ---- Storm messages ----

    /// 0x470a30: one Storm message (Ordinal 121) by its first byte; 0x51 when the game is gone,
    /// 0x52 an unknown one, 0 none.
    fn lobby_470a30(&mut self, files: &crate::storage::Storage) -> u32 {
        snet::pump();
        for e in snet::events() {
            match e {
                snet::NetEvent::Joined(p) => self.lobby_4c4980(p as u32),
                snet::NetEvent::Left(p, why) => self.lobby_4c3c90(p as u32, why),
            }
        }
        let Some((from, m)) = snet::receive_message() else { return 0 };
        let from = from as u32;
        match (m.first().copied().unwrap_or(0), m.len()) {
            (b'I', 5) => {
                self.lobby_470720(&m, from);
                0x49
            }
            (b'J', 0x3f) => {
                self.lobby_470840(&m, from);
                0x4a
            }
            (b'K', 0x79) => {
                self.lobby_4701a0(&m, from);
                0x4b
            }
            (k @ (b'L' | b'M'), n) if n > 1 && valid_text(&m[1..]) => {
                if k == b'L' {
                    self.lobby_4706d0(&m, from);
                } else {
                    self.lobby_470520(&m, from);
                }
                0x4c
            }
            (b'N', 2) => {
                if from == 0 {
                    self.lobby_4b8870(m[1]);
                }
                0x4e
            }
            (b'O', _) => {
                self.lobby_4eafe0(files, &m[1..], from);
                0x4f
            }
            (b'P', _) => 0x50,
            _ => 0x52,
        }
    }

    /// 0x4c4980 (Storm event 2, a player joined): in the lobby the creator (not started) sends
    /// it the force names ('K', melee only) and the game ('J'); a banned name (0x472800) or a
    /// game past the lobby refuses it (0x470480). Other screens drop it (Ordinal 106).
    fn lobby_4c4980(&mut self, id: u32) {
        if id >= 8 {
            return;
        }
        self.setup.g5971e4[id as usize] = 0;
        if !self.lobby_host {
            return;
        }
        if self.screen != screen::CHAT {
            snet::drop_player(id, 3);
            return;
        }
        if self.setup.g66fbfa == 4 && !self.lobby.starting {
            if id == 0 {
                self.lobby_4b8870(0);
                return;
            }
            if let Some(t) = self.lobby.transfer.as_mut() {
                t.waiting |= 1 << id;
            }
            if !self.melee() {
                self.lobby_470290(id);
            }
            self.lobby_470310(id);
            self.lobby_offer(id);
            return;
        }
        self.lobby_470480(id, 4);
    }

    /// 0x4c3c90 (Storm event 3, a player left): its reason (3 unless given) in 0x5971e4.
    /// Ours also clears its slot (the creator's 0x3e tells the others) and info.
    fn lobby_4c3c90(&mut self, id: u32, why: u32) {
        if id >= 8 {
            return;
        }
        self.setup.g5971e4[id as usize] = if why == 0 { 3 } else { why };
        // The creator gone: the game is.
        if id == 0 && !self.lobby_host {
            self.lobby_4b8870(0);
            return;
        }
        self.lobby.info[id as usize] = PlayerInfo::default();
        if self.lobby_host {
            for k in 0..8 {
                let st = u32::from_le_bytes(self.setup.slots[k][slot::STORM..slot::STORM + 4].try_into().unwrap());
                if self.setup.slots[k][slot::TYPE] == 2 && st == id {
                    let r = self.setup.slots[k][slot::RACE];
                    self.setup.slots[k] = self.setup.lobby[k];
                    self.setup.slots[k][slot::TYPE] = 6;
                    self.setup.slots[k][slot::RACE] = r;
                    self.lobby_470f90(k);
                }
            }
            self.setup.index_human_slots();
            if let Some(t) = self.lobby.transfer.as_mut() {
                t.members.retain(|m| m.0 as u32 != id);
                t.waiting &= !(1 << id);
            }
            let open = self.setup.slots[..8].iter().filter(|s| s[slot::TYPE] == 6).count();
            self.lobby_4c4160(open);
        }
        self.lobby.dirty = true;
    }

    /// 0x470480 (ESI = `id`, AL = `why`): 'N' + the reason three times, then the player is
    /// dropped (Ordinal 106).
    fn lobby_470480(&mut self, id: u32, why: u8) {
        if id == self.setup.local_storm_id {
            self.lobby_4b8870(why);
            return;
        }
        for _ in 0..3 {
            snet::send_message(id as u8, &[b'N', why]);
        }
        if id < 8 {
            snet::drop_player(id, 0x4000_0004);
        }
    }

    /// 0x470250: 'I' + the joiner's Storm id to everyone.
    fn lobby_470250(&mut self, id: u32) {
        let mut m = vec![b'I'];
        m.extend_from_slice(&id.to_le_bytes());
        if self.setup.net_provider != 0 {
            snet::send_message(0xff, &m);
        }
    }

    /// 0x470720 ('I' from the creator): "%s has joined the game." (network.tbl 0x4a) for
    /// another player (the /astat whisper is Battle.net's, 0x4c4200).
    fn lobby_470720(&mut self, m: &[u8], from: u32) {
        let id = u32::from_le_bytes(m[1..5].try_into().unwrap());
        if from != 0 || id >= 8 || id == self.setup.local_storm_id {
            return;
        }
        let Some(name) = snet::player_name(id).filter(|n| !n.is_empty()) else { return };
        let fmt = self.network_raw_str(0x4a);
        self.lobby_4b91c0(&super::create::fmt_s(&fmt, &name), 8);
    }

    /// 0x470290 ('K' to `id`): the force names (0x58d5bc, 4 x 0x1e).
    fn lobby_470290(&mut self, id: u32) {
        let mut m = vec![b'K'];
        for n in &self.setup.force_names {
            m.extend_from_slice(n);
        }
        snet::send_message(id as u8, &m);
    }

    /// 0x4701a0 ('K' from the creator, a joiner of a non-melee game): the force names.
    fn lobby_4701a0(&mut self, m: &[u8], from: u32) {
        if self.lobby_host || from != 0 || self.melee() {
            return;
        }
        for (k, n) in self.setup.force_names.iter_mut().enumerate() {
            n.copy_from_slice(&m[1 + k * 0x1e..1 + (k + 1) * 0x1e]);
        }
    }

    /// 0x470310 ('J' then 'P' to `id`): the map's tileset and size, each lobby slot's type,
    /// race and force, the types 0x66ff3c, the forces 0x58d5b0 / flags 0x58d5b8 and the user
    /// selectable races 0x59bda8.
    fn lobby_470310(&mut self, id: u32) {
        let s = &self.setup;
        let mut m = vec![b'J'];
        for v in [s.tileset, s.map_width, s.map_height] {
            m.extend_from_slice(&v.to_le_bytes());
        }
        m.extend(s.lobby.iter().map(|l| l[slot::TYPE]));
        m.extend(s.lobby.iter().map(|l| l[slot::RACE]));
        m.extend_from_slice(&s.net_types);
        m.extend_from_slice(&s.player_force);
        m.extend_from_slice(&s.force_flags);
        m.extend_from_slice(&s.user_selectable);
        debug_assert_eq!(m.len(), 0x3f);
        snet::send_message(id as u8, &m);
        snet::send_message(id as u8, b"P");
    }

    /// 0x470840 ('J' from the creator to a joiner in state 2): the lobby copy (index, no Storm
    /// id, type, race, force), team forces (0x485710 seats per team), the types / forces /
    /// races tables; the joiner's own 0x40 waits (state 3).
    fn lobby_470840(&mut self, m: &[u8], from: u32) {
        if self.lobby_host || from != 0 {
            return;
        }
        let w = |o: usize| u16::from_le_bytes([m[o], m[o + 1]]);
        let s = &mut self.setup;
        s.tileset = w(1);
        s.map_width = w(3);
        s.map_height = w(5);
        for (k, l) in s.lobby.iter_mut().enumerate() {
            l.fill(0);
            l[..4].copy_from_slice(&(k as u32).to_le_bytes());
            l[slot::STORM..slot::STORM + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            l[slot::TYPE] = m[7 + k];
            l[slot::RACE] = m[0x13 + k];
            l[slot::FORCE] = m[0x2b + k];
        }
        if s.multiplayer && s.template[0x10] != 0 {
            let teams = if s.template[0] == 0xf { 2 } else { s.template[0x10] };
            let size = crate::bw::players::team_size(s.template[0x10]) as usize;
            let mut k = 0;
            for t in 0..teams {
                for _ in 0..size {
                    if k < 8 {
                        s.lobby[k][slot::FORCE] = t + 1;
                        k += 1;
                    }
                }
            }
        }
        s.net_types.copy_from_slice(&m[0x1f..0x2b]);
        s.player_force.copy_from_slice(&m[0x2b..0x33]);
        s.force_flags.copy_from_slice(&m[0x33..0x37]);
        s.user_selectable.copy_from_slice(&m[0x37..0x3f]);
        let c = self.lobby_471fb0(0, 0, 0);
        self.lobby.cmd_join = Some(c);
        self.setup.g66fbfa = 3;
    }

    /// 0x4707d0 (EDI = the text): a lobby chat line: slash commands first (0x47f8f0); else 'L'
    /// + the text to everyone (under 0x79 bytes).
    pub(super) fn lobby_4707d0(&mut self, text: &[u8]) {
        if text.first() == Some(&b'/') {
            todo_port!("0x47f8f0", "slash commands in the lobby");
            return;
        }
        if text.len() + 1 >= 0x79 {
            return;
        }
        let mut m = vec![b'L'];
        m.extend_from_slice(text);
        m.push(0);
        snet::send_message(0xff, &m);
        // Storm hands a message to its sender too.
        let me = self.setup.local_storm_id;
        self.lobby_4706d0(&m, me);
    }

    /// 0x4706d0 ('L' from `from`): shown (0x4b91c0) when the player is known and not
    /// squelched (0x57f1d8).
    fn lobby_4706d0(&mut self, m: &[u8], from: u32) {
        if from < 8 && self.lobby.info[from as usize].on && self.lobby.accept & (1 << from) != 0 {
            let t = cz(&m[1..]).to_vec();
            self.lobby_4b91c0(&t, from);
        }
    }

    /// 0x470520 ('M' from `from`): network.tbl 0x50 with the name, then the text.
    fn lobby_470520(&mut self, m: &[u8], from: u32) {
        if from < 8 && self.lobby.info[from as usize].on && self.lobby.accept & (1 << from) != 0 {
            let fmt = self.network_raw_str(0x50);
            let mut t = super::create::fmt_s(&fmt, &self.lobby.info[from as usize].name.clone());
            t.extend_from_slice(cz(&m[1..]));
            self.lobby_4b91c0(&t, from);
        }
    }

    /// 0x4b91c0 (EAX = Storm id; 8 a notice, 0xffffffff the system): a chat line: "name: text"
    /// for a player (own 0, others 1), the text alone otherwise (2 / 3); 0x4b8f10 adds it.
    pub(super) fn lobby_4b91c0(&mut self, text: &[u8], id: u32) {
        let kind = if id == self.setup.local_storm_id {
            0
        } else if id < 8 {
            1
        } else if id == 8 {
            2
        } else {
            3
        };
        let mut line = Vec::new();
        if kind < 2 {
            if self.lobby.info[id as usize].on {
                line.extend_from_slice(&self.lobby.info[id as usize].name);
            }
            line.extend_from_slice(b": ");
        }
        line.extend_from_slice(&text[..text.len().min(0x78)]);
        self.lobby.chat_out.push((line, kind));
    }

    /// network.tbl string `n` as bytes (0-based, the tbl's own numbering).
    fn network_raw_str(&self, n: usize) -> Vec<u8> {
        crate::setup::tbl_str(&self.network_raw, n).to_vec()
    }
}

/// A map named `name` in the install's maps directory or its download directory (native).
fn local_map(name: &[u8]) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let n = String::from_utf8_lossy(name).into_owned();
        let base = crate::install_dir().join("maps");
        for p in [base.join("download").join(&n), base.join(&n)] {
            if let Ok(d) = std::fs::read(p) {
                return Some(d);
            }
        }
    }
    let _ = name;
    None
}

/// 0x4d9cd0: the build's version hash (Storm SStrHash of the font.gid key); 0 here: our
/// builds all match.
fn version_hash() -> u32 {
    0
}

/// 0x485980 (EBX = 0, the third argument 0, from 0x470a30): chat text ends with a NUL and
/// has no control characters (not even the color codes).
fn valid_text(b: &[u8]) -> bool {
    for &c in b {
        if c == 0 {
            return true;
        }
        if c < 0x20 {
            return false;
        }
    }
    false
}

/// The text before the first NUL.
fn cz(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}
