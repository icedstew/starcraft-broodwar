//! Recording and writing replays (1.16.1 net_record.cpp and replay.cpp, legacy format).
//!
//! The game's recorder [0x596bbc] (a 0x20-byte object, created by 0x4ce280 with 0x4cdf50)
//! lives here as a thread-local `Recorder`; `record` is what 0x4865d0 does after each
//! command (0x4cde70).
#![allow(dead_code)]

use crate::bw::save::Out;
use crate::bw::State;
use crate::todo_port;
use std::cell::RefCell;

/// 0x502860: the command ids 0x4cde70 does not record (0x37 sync, 5 keep alive, 6 / 7 save /
/// load, 8 restart, 0x10 / 0x11 pause / resume, 0x38 .. 0x3b, 0x55 latency).
const NOT_RECORDED: [u8; 12] = [0x37, 5, 6, 7, 8, 0x10, 0x11, 0x38, 0x39, 0x3a, 0x3b, 0x55];

/// The 0x20-byte recorder 0x4cdf50 sets up, plus the header image 0x6d0f30 it is saved with.
pub struct Recorder {
    /// +8 / +0xc: the command buffer and its used part. Frame blocks in it are the frame
    /// (u32), the length (u8) and the commands, each a player (storm) id and the command.
    pub buf: Vec<u8>,
    /// +0x14: where the open frame block's commands start (its length byte is just before).
    block: Option<usize>,
    /// +0x18: the frame of the open block (-1 at first).
    frame: u32,
    /// 0x6d0f30: the 0x279-byte replay header.
    pub header: Vec<u8>,
    /// A game being played (not the playback of a replay): the 0x48 command (seed and the
    /// players' colors) is copied into the header (0x472110).
    live: bool,
}

thread_local! {
    /// [0x596bbc].
    static REC: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

/// 0x4ce280 + 0x4cdf50: a fresh recorder (the old one freed) over the header image `header`
/// (None: a game that starts, its header is made at the end).
pub fn start(header: Option<&[u8]>) {
    let r = Recorder { buf: Vec::with_capacity(50000), block: None, frame: u32::MAX, header: header.map_or(vec![0; 0x279], |h| h.to_vec()), live: header.is_none() };
    REC.with(|c| *c.borrow_mut() = Some(r));
}

/// [0x596bbc] = 0: nothing is recorded any more.
pub fn stop() -> Option<Recorder> {
    REC.with(|c| c.borrow_mut().take())
}

pub fn recording() -> bool {
    REC.with(|c| c.borrow().is_some())
}

/// 0x4865d0 (at 0x486d7a): after a command of `player` (a slot), record it with the player's
/// storm id (0x512680) at the frame 0x57f23c.
pub fn record(st: &State, player: u8, cmd: &[u8]) {
    if SUPPRESS.with(|s| s.get()) {
        return;
    }
    record_at(st.frame, player, cmd);
}

thread_local! {
    /// A net game records confirmed frames only (`record_at`): execution does not.
    static SUPPRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Stop (true) or resume `record`'s recording at execution.
pub fn suppress(on: bool) {
    SUPPRESS.with(|s| s.set(on));
}

/// `record` for a command of `player` that runs at `frame` (a net session's confirmed frame).
pub fn record_at(frame: u32, player: u8, cmd: &[u8]) {
    REC.with(|c| {
        if let Some(r) = c.borrow_mut().as_mut() {
            // The State keeps no storm ids: `set_storm_ids` gives them (a single player
            // game's human is storm id 0).
            let storm = STORM.with(|s| s.borrow().get(player as usize).copied()).unwrap_or(0);
            r.record(frame, storm, cmd);
        }
    });
}

thread_local! {
    /// The storm id of each slot (the slot table's +4 byte).
    static STORM: RefCell<[u8; 12]> = const { RefCell::new([0; 12]) };
}

/// The storm id of each slot for `record` (0x57eee4 of each slot of the table).
pub fn set_storm_ids(ids: [u8; 12]) {
    STORM.with(|s| *s.borrow_mut() = ids);
}

impl Recorder {
    /// 0x4cddc0: close the open frame block (an empty one is dropped again) and open a block for
    /// the frame `frame` (0x57f23c).
    fn new_block(&mut self, frame: u32) {
        if let Some(start) = self.block.take() {
            let n = self.buf.len() - start;
            if n == 0 {
                self.buf.truncate(start - 5);
            } else {
                self.buf[start - 1] = n as u8;
            }
        }
        self.frame = frame;
        self.buf.extend_from_slice(&frame.to_le_bytes());
        self.buf.push(0);
        self.block = Some(self.buf.len());
    }

    /// 0x4cde70: record the command `cmd` of the player (storm id) `storm` at `frame`: skipped
    /// when it is 0x100 bytes or longer or one of `NOT_RECORDED`; a new block when the open
    /// one would pass 0xfe bytes or is of another frame.
    pub fn record(&mut self, frame: u32, storm: u8, cmd: &[u8]) {
        if cmd.len() >= 0x100 || cmd.is_empty() || NOT_RECORDED.contains(&cmd[0]) {
            return;
        }
        if self.live && cmd[0] == 0x48 && cmd.len() == 13 {
            // 0x472110: 0x6d0f37.. = the seed command.
            self.header[7..20].copy_from_slice(cmd);
        }
        if let Some(start) = self.block {
            if (self.buf.len() - start) as i32 + 1 + cmd.len() as i32 >= 0xff {
                self.new_block(frame);
            }
        }
        if self.frame != frame {
            self.new_block(frame);
        }
        self.buf.push(storm);
        self.buf.extend_from_slice(cmd);
    }

    /// 0x4ce1c0: close the open block; the buffer's size (u32) and bytes, each packed
    /// (0x4c3450). The bytes are left out when empty.
    fn write_commands(&mut self, o: &mut Out) -> bool {
        if let Some(start) = self.block.take() {
            self.buf[start - 1] = (self.buf.len() - start) as u8;
        }
        if !o.packed(&(self.buf.len() as u32).to_le_bytes()) {
            return false;
        }
        self.buf.is_empty() || o.packed(&self.buf)
    }

    /// 0x4dfab0 (the part after the file is created): the replay file. "reRS" and the header
    /// (0x279 bytes), the commands (0x4ce1c0) and the map's scenario.chk (0x4cc6e0), the
    /// last as its size and its bytes. `header` has byte 0x46 set (a replay with a map) and
    /// the game's campaign mission (0x57f244) at 5.
    pub fn file(&mut self, chk: &[u8]) -> Option<Vec<u8>> {
        let mut o = Out::default();
        self.header[0x46] = 1;
        let ok = o.packed(b"reRS") && o.packed(&self.header) && self.write_commands(&mut o) && !chk.is_empty() && o.packed(&(chk.len() as u32).to_le_bytes()) && o.packed(chk);
        ok.then_some(o.buf)
    }
}

/// 0x4def80: the path of the replay `name` ("<exe dir>maps\replays\<name>.rep"), the
/// directories created. None when that fails.
pub fn replay_path(exe_dir: &str, name: &str) -> Option<String> {
    let dir = crate::saveload::save_dir(exe_dir, b"", false, true, 0x104)?;
    crate::saveload::create_dirs(&dir).then(|| format!("{dir}{name}.rep"))
}

/// 0x4dea10 (the slot table and colors part): the player table 0x57eee0 into the header's
/// 12 slots (+0xa1, 0x24 bytes each; the storm id at +4 is the recorded game's) and the 8 color
/// slots 0x57f21c into +0x251. The game info part is todo_port in `live_header`.
fn copy_to_header(st: &State, h: &mut [u8]) {
    let storm = STORM.with(|s| *s.borrow());
    for (i, t) in st.player_table().chunks_exact(0x24).enumerate() {
        let o = 0xa1 + i * 0x24;
        h[o..o + 0x24].copy_from_slice(t);
        h[o + 4..o + 8].copy_from_slice(&(storm[i] as u32).to_le_bytes());
    }
    for (i, c) in st.color_slot.iter().enumerate() {
        h[0x251 + i * 4..0x255 + i * 4].copy_from_slice(&c.to_le_bytes());
    }
}

/// The header image 0x6d0f30 of the game that ended: the engine (0x58f440), the frame count
/// (0x6d0f31), the game info 0x5967f8 (+0x14: the map size, the tileset and the template; the
/// rest is not kept), the slot table (+0xa1) and the colors (+0x251).
pub fn live_header(st: &State, recorded: &[u8]) -> Vec<u8> {
    let mut h = recorded.to_vec();
    h[0] = st.is_expansion as u8;
    h[1..5].copy_from_slice(&st.frame.to_le_bytes());
    todo_port!("0x4dea10", "the game info 0x5967f8 (title, host, map name, speed, game type)");
    let info = 0x14;
    h[info + 0x20..info + 0x22].copy_from_slice(&(st.map_tiles.0 as u16).to_le_bytes());
    h[info + 0x22..info + 0x24].copy_from_slice(&(st.map_tiles.1 as u16).to_le_bytes());
    h[info + 0x30..info + 0x32].copy_from_slice(&st.tileset.to_le_bytes());
    h[info + 0x6d..info + 0x8d].copy_from_slice(&st.template);
    copy_to_header(st, &mut h);
    h
}

/// 0x4dea10, called by 0x4e0710 right after the game init 0x4ef100: a new game's header
/// image takes the game info, slot table and colors as they are at the start (a player who
/// leaves later stays a human in the header).
pub fn capture_header(st: &State) {
    REC.with(|c| {
        if let Some(r) = c.borrow_mut().as_mut().filter(|r| r.live) {
            r.header = live_header(st, &r.header);
        }
    });
}

/// The replay file's bytes of the recorded game (0x4dfab0's body); None without a recorder.
/// A new game's header is the one `capture_header` took, with the engine, frame count and
/// storm ids now.
pub fn replay_bytes(st: &State, chk: &[u8]) -> Option<Vec<u8>> {
    REC.with(|c| {
        let mut c = c.borrow_mut();
        let r = c.as_mut()?;
        let recorded = r.header.clone();
        if r.live {
            r.header[0] = st.is_expansion as u8;
            r.header[1..5].copy_from_slice(&st.frame.to_le_bytes());
            // The storm ids (+4 of each slot) are the recorded game's (`set_storm_ids`).
            let storm = STORM.with(|s| *s.borrow());
            for (i, id) in storm.iter().enumerate() {
                let o = 0xa1 + i * 0x24 + 4;
                r.header[o..o + 4].copy_from_slice(&(*id as u32).to_le_bytes());
            }
        } else {
            r.header = live_header(st, &recorded);
        }
        let f = r.file(chk);
        r.header = recorded;
        f
    })
}

/// 0x4dfab0: the recorded game as the file `<exe dir>maps\replays\<name>.rep`. With `fresh`
/// (the argument 1) an existing file is deleted first; one that stays is -1 (the game shows
/// the "can't overwrite" popup 0x4f5ee0, not modelled). Returns 1 written, 0 failed, -1 locked.
pub fn write_replay(exe_dir: &str, name: &str, fresh: bool, st: &State, chk: &[u8]) -> i32 {
    let Some(path) = replay_path(exe_dir, name) else { return 0 };
    let p = crate::saveload::host(&path);
    if fresh && std::fs::remove_file(&p).is_err() && p.exists() {
        todo_port!("0x4f5ee0", "the popup for a replay file that cannot be deleted");
        return -1;
    }
    match replay_bytes(st, chk) {
        Some(b) if std::fs::write(&p, &b).is_ok() => 1,
        _ => {
            let _ = std::fs::remove_file(&p);
            0
        }
    }
}

/// 0x4ded30: copy `<exe dir>maps\replays\LastReplay.rep` to `<name>.rep`. False when either
/// file cannot be opened.
pub fn copy_last_replay(exe_dir: &str, name: &str) -> bool {
    let (Some(from), Some(to)) = (replay_path(exe_dir, "LastReplay"), replay_path(exe_dir, name)) else { return false };
    let Ok(d) = std::fs::read(crate::saveload::host(&from)) else { return false };
    std::fs::write(crate::saveload::host(&to), d).is_ok()
}

/// 0x4dfd70: save the last replay as `name` (the dialog's edit text 0x51bfb8). 0 when the path
/// cannot be made, -1 when the file exists and `overwrite` (the confirmation 0x4dfc80 with
/// string 3) refuses, else 0x4ded30's result (1 copied).
pub fn save_named(exe_dir: &str, name: &str, overwrite: impl FnOnce() -> bool) -> i32 {
    let Some(path) = replay_path(exe_dir, name) else { return 0 };
    if crate::saveload::host(&path).exists() && !overwrite() {
        return -1;
    }
    copy_last_replay(exe_dir, name) as i32
}

/// A net game's header seed and game type (melee 2 / 1, as `melee_setup` loads the template): the real game copies the 0x48 command (id, seed, ...) into the
/// header (0x472110); the net session sends none, so its seed goes in directly (+8, 0x6d0f38).
pub fn set_net_header(seed: u32, game_type: u16, sub_type: u16) {
    REC.with(|c| {
        if let Some(r) = c.borrow_mut().as_mut() {
            r.header[7] = 0x48;
            r.header[8..12].copy_from_slice(&seed.to_le_bytes());
            r.header[0x3c..0x3e].copy_from_slice(&game_type.to_le_bytes());
            r.header[0x3e..0x40].copy_from_slice(&sub_type.to_le_bytes());
        }
    });
}
