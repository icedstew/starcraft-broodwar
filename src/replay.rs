//! .rep replay reader (legacy, 1.18-1.20 and 1.21+ "seRS" formats).
//!
//! Container format and command sizes follow icza/screp (Apache-2.0):
//! https://github.com/icza/screp.

use std::io::{self, Read};

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> io::Result<u32> {
        let b = self.d.get(self.pos..self.pos + 4).ok_or_else(|| err("replay truncated"))?;
        self.pos += 4;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    fn bytes(&mut self, n: usize) -> io::Result<&[u8]> {
        let b = self.d.get(self.pos..self.pos + n).ok_or_else(|| err("replay truncated"))?;
        self.pos += n;
        Ok(b)
    }

    /// One section: u32 checksum, u32 chunk count, then chunks (u32 len + data) of up to
    /// 0x2000 bytes each, compressed when shorter than that: zlib if it has a zlib header,
    /// else PKWare implode (pre-1.18).
    fn section(&mut self, size: usize) -> io::Result<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }
        let _checksum = self.u32()?;
        let chunks = self.u32()?;
        let mut out = Vec::with_capacity(size);
        for _ in 0..chunks {
            let len = self.u32()? as usize;
            let c = self.bytes(len)?;
            let want = (size - out.len().min(size)).min(0x2000);
            if len > 4 && c[0] == 0x78 && (u16::from_be_bytes([c[0], c[1]]) % 31) == 0 {
                flate2::read::ZlibDecoder::new(c).read_to_end(&mut out)?;
            } else if len < want {
                out.extend(explode::explode(c).map_err(|e| err(format!("explode: {e:?}")))?);
            } else {
                out.extend_from_slice(c);
            }
        }
        Ok(out)
    }

    /// Variable-size section: a 4-byte section holding the size, then the section.
    fn sized_section(&mut self) -> io::Result<Vec<u8>> {
        let s = self.section(4)?;
        let n = u32::from_le_bytes(s.get(..4).ok_or_else(|| err("bad size section"))?.try_into().unwrap());
        self.section(n as usize)
    }
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub slot_id: u16,
    pub player_id: u8,
    /// 0 inactive, 1 computer, 2 human, 3 rescue, 5 computer slot, 6 open, 7 neutral, 8 closed
    pub kind: u8,
    /// 0 zerg, 1 terran, 2 protoss, 6 random
    pub race: u8,
    pub team: u8,
    pub name: String,
    pub color: u32,
}

#[derive(Debug, Clone)]
pub struct Header {
    /// 0 StarCraft, 1 Brood War.
    pub engine: u8,
    pub frames: u32,
    pub start_time: u32,
    pub title: String,
    pub map_width: u16,
    pub map_height: u16,
    pub speed: u8,
    pub game_type: u16,
    pub sub_type: u16,
    pub host: String,
    pub map_name: String,
    pub slots: Vec<Slot>,
    /// The whole 0x279-byte header as loaded to 0x6d0f30.
    pub raw: Vec<u8>,
}

/// A raw game command as it goes through the network/replay stream.
#[derive(Debug, Clone)]
pub struct Command {
    pub frame: u32,
    /// The storm id the command came with.
    pub player: u8,
    /// The player 0x487100 runs it as (0x512678): the slot whose storm id (0x57eee0 +4)
    /// matches, or the one before it in the frame's group.
    pub owner: u8,
    /// Command id byte followed by its payload.
    pub data: Vec<u8>,
}

pub struct Replay {
    pub modern: bool,
    pub header: Header,
    pub commands: Vec<Command>,
    /// Embedded scenario.chk.
    pub chk: Vec<u8>,
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

impl Replay {
    pub fn parse(d: &[u8]) -> io::Result<Replay> {
        if d.len() < 30 {
            return Err(err("not a replay"));
        }
        let modern121 = d[12] == b's';
        let mut r = Reader { d, pos: 0 };
        let id = r.section(4)?;
        if id != b"seRS" && id != b"reRS" {
            return Err(err("not a replay (bad replay id)"));
        }
        if modern121 {
            r.u32()?; // encoded length of the header section
        }
        let h = r.section(0x279)?;
        if h.len() < 0x279 {
            return Err(err("short header"));
        }
        let cmds = r.sized_section()?;
        let chk = r.sized_section()?;
        let modern = modern121 || d[28] == 0x78;
        // SC:R's files use commands 1.16.1 lacks (0x60..): framed by our own table.
        let header = parse_header(&h);
        let owners = Owners::new(&header.raw);
        let commands = if modern { parse_commands(&cmds, &owners)? } else { playback_commands(&cmds, &owners) };
        Ok(Replay { modern, header, commands, chk })
    }
}

/// Also the data 0x4de9a0 (the 12 slots at +0xa1 into the player table 0x57eee0) and 0x4de950
/// (the 8 color dwords at +0x251 into 0x57f21c) copy out of a replay's header when it plays.
fn parse_header(d: &[u8]) -> Header {
    let u16_at = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let slots = (0..12)
        .map(|i| {
            let p = &d[0xA1 + i * 36..0xA1 + i * 36 + 36];
            Slot {
                slot_id: u16::from_le_bytes([p[0], p[1]]),
                player_id: p[4],
                kind: p[8],
                race: p[9],
                team: p[10],
                name: cstr(&p[11..36]),
                color: if i < 8 { u32_at(0x251 + i * 4) } else { 0 },
            }
        })
        .collect();
    Header {
        engine: d[0],
        frames: u32_at(0x01),
        start_time: u32_at(0x08),
        title: cstr(&d[0x18..0x18 + 28]),
        map_width: u16_at(0x34),
        map_height: u16_at(0x36),
        speed: d[0x3A],
        game_type: u16_at(0x3C),
        sub_type: u16_at(0x3E),
        host: cstr(&d[0x48..0x48 + 24]),
        map_name: cstr(&d[0x61..0x61 + 26]),
        slots,
        raw: d.to_vec(),
    }
}

/// Payload size (excluding the id byte) of command `id`, given the bytes that follow it.
/// None = unknown command.
pub fn command_len(id: u8, rest: &[u8]) -> Option<usize> {
    Some(match id {
        0x05 | 0x08 | 0x10 | 0x11 | 0x18 | 0x19 | 0x1b | 0x1c | 0x1d | 0x27 | 0x2a | 0x2e | 0x31 | 0x33 | 0x34 | 0x36 | 0x38 | 0x39
        | 0x3c | 0x54 | 0x5a | 0x5b => 0,
        0x06 | 0x07 => 4 + u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize,
        0x09..=0x0b => 1 + 2 * *rest.first()? as usize,
        0x63..=0x65 => 1 + 4 * *rest.first()? as usize,
        0x0c => 7,  // build: order, x, y, unit
        0x0d => 2,  // vision
        0x0e => 4,  // alliance
        0x0f => 1,  // game speed
        0x12 => 4,  // cheat
        0x13 => 2,  // hotkey: type, group
        0x14 => 9,  // right click: x, y, tag, unit, queued
        0x15 => 10, // targeted order: x, y, tag, unit, order, queued
        0x60 => 11,
        0x61 => 12,
        0x1a | 0x1e | 0x21 | 0x22 | 0x25 | 0x26 | 0x28 | 0x2b | 0x2c | 0x2d => 1, // queued flag
        0x1f | 0x20 | 0x23 | 0x29 | 0x35 => 2,
        0x62 => 4,
        0x2f => 4,        // lift off: x, y
        0x30 | 0x32 => 1, // tech, upgrade
        0x37 => 6,        // sync
        0x3a | 0x3b | 0x3d | 0x42 | 0x43 => 1,
        0x3e => 5,
        0x3f => 7,
        0x40 => 17,
        0x41 | 0x44 | 0x45 => 2,
        0x48 => 12,
        0x55 => 1, // latency
        0x56 => 9, // replay speed
        0x57 => 1, // leave game: reason
        0x58 => 4, // minimap ping
        0x5c => 81, // chat: sender slot + 80 bytes
        _ => return None,
    })
}

/// 0x5005f8: each command's length with its id byte (0x4cdff0); -1 = not a command.
const CMD_LEN: [i8; 0x77] = [
    -1, -1, -1, -1, -1, 1, 33, 33, 1, 26, 26, 26, 8, 3, 5, 2, //
    1, 1, 5, 3, 10, 11, -1, -1, 1, 1, 2, 1, 1, 1, 2, 3, //
    3, 2, 2, 3, 1, 2, 2, 1, 2, 3, 1, 2, 2, 2, 1, 5, //
    2, 1, 2, 1, 1, 3, 1, 1, 1, 1, 2, 2, -1, -1, -1, -1, //
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, //
    -1, -1, -1, -1, -1, 2, 10, 2, 5, -1, 1, -1, 82, -1, -1, -1, //
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, //
    -1, -1, -1, -1, -1, -1, -1,
];

/// 0x4865d0: split a turn's command bytes into commands by the 0x5005f8 lengths (0x09..0x0b:
/// 2 bytes per unit + 2); the rest after a byte that is not a command is dropped.
pub fn split_commands(turn: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut p = 0;
    while let Some(&id) = turn.get(p) {
        let n = match id {
            0x09..=0x0b => turn.get(p + 1).map_or(0, |&k| k as usize * 2 + 2),
            _ => match CMD_LEN.get(id as usize) {
                Some(&n) if n > 0 => n as usize,
                _ => break,
            },
        };
        let Some(c) = turn.get(p..p + n) else { break };
        out.push(c);
        p += n;
    }
    out
}

/// 0x487100's player lookup: the 8 entries of the slot table 0x57eee0 (as the header
/// stores it at 0xa1): +4 the storm id, +0 the player. A group starts from 0x512684, the
/// local player: 0x596bac in a replay (0x4dbe50), 0 when read from the real game (tl1742).
struct Owners([(u32, u8); 8]);

impl Owners {
    fn new(raw: &[u8]) -> Owners {
        let u32_at = |o: usize| raw.get(o..o + 4).map_or(u32::MAX, |b| u32::from_le_bytes(b.try_into().unwrap()));
        Owners(std::array::from_fn(|i| (u32_at(0xa1 + i * 36 + 4), u32_at(0xa1 + i * 36) as u8)))
    }

    /// The player for storm id `id`, else `cur` (0x512678 unchanged).
    fn owner(&self, id: u8, cur: u8) -> u8 {
        self.0.iter().find(|e| e.0 == id as u32).map_or(cur, |e| e.1)
    }
}

/// 0x6552b0: the group's command bytes; 0x6554d8, right after it, the storm ids.
const GROUP_DATA: usize = 0x228;

/// The commands 1.16.1 runs when it plays a replay, at the frame and as the player it runs
/// them. 0x4cdff0 reads every block of one frame (u32 frame, u8 length, then storm id +
/// command pairs) into one group; the command lengths come from 0x5005f8 (0x09..0x0b: 2
/// bytes per unit + 2), and the bytes read so far in a block are counted in a byte, so a
/// long command (chat 0x5c, 82 bytes) can wrap the count past the block's length and the
/// reading runs on into the next block's header. The bytes go to 0x6552b0, only 0x228 long:
/// a bigger group runs over the storm ids at 0x6554d8 (each written before its command's
/// bytes), so its first commands can run as another player, and storm ids written later
/// land in earlier commands' bytes. Reaching the end of the data 0x4cdff0 returns -1
/// instead of the frame, so 0x487100 never runs that last group. 0x487100 reads one group
/// per frame and runs it once the counter reaches its frame (a group behind the counter
/// runs at once).
fn playback_commands(d: &[u8], owners: &Owners) -> Vec<Command> {
    let mut out = Vec::new();
    let (mut p, mut next, mut cur) = (0usize, -1i64, 0i64);
    let u32_at = |p: usize| d.get(p..p + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    loop {
        // 0x487100 at counter `cur`: a new group is read once the last one has run.
        if cur <= next {
            cur = next + 1;
        }
        let Some(frame) = u32_at(p) else { break };
        // ponytail: one flat buffer for 0x6552b0 and what follows; a group past 0x6554d8 +
        // its own storm ids would reach other globals, which are not modeled.
        let mut mem: Vec<u8> = vec![0; GROUP_DATA + 0x400];
        let mut cmds: Vec<(usize, usize)> = Vec::new();
        let mut off = 0usize;
        let mut done = false;
        while p < d.len() {
            if u32_at(p) != Some(frame) {
                done = true;
                break;
            }
            let Some(&len) = d.get(p + 4) else {
                p = d.len();
                break;
            };
            p += 5;
            let mut read = 0u8;
            while read < len {
                let (Some(&player), Some(&id)) = (d.get(p), d.get(p + 1)) else {
                    p = d.len();
                    break;
                };
                let n = match id {
                    0x09..=0x0b => d.get(p + 2).map_or(0, |&k| k as usize * 2 + 2),
                    _ => match CMD_LEN.get(id as usize) {
                        Some(&n) if n > 0 => n as usize,
                        // Not a command: 1.16.1 would copy (u32)-1 bytes.
                        _ => {
                            p = d.len();
                            break;
                        }
                    },
                };
                let src = &d[p + 1..(p + 1 + n).min(d.len())];
                let at = GROUP_DATA + cmds.len();
                if at >= mem.len() || off + src.len() > mem.len() {
                    p = d.len();
                    break;
                }
                mem[at] = player;
                mem[off..off + src.len()].copy_from_slice(src);
                cmds.push((off, src.len()));
                off += src.len();
                p += 1 + n;
                read = read.wrapping_add(n as u8).wrapping_add(1);
            }
        }
        if !done {
            break;
        }
        next = frame as i64;
        let run = next.max(cur);
        let mut owner = 0;
        for (i, &(o, n)) in cmds.iter().enumerate() {
            let player = mem[GROUP_DATA + i];
            owner = owners.owner(player, owner);
            out.push(Command { frame: run as u32, player, owner, data: mem[o..o + n].to_vec() });
        }
        next = run;
    }
    out
}

fn parse_commands(d: &[u8], owners: &Owners) -> io::Result<Vec<Command>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 5 <= d.len() {
        let frame = u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
        let end = (p + 5 + d[p + 4] as usize).min(d.len());
        p += 5;
        let mut owner = 0;
        while p + 2 <= end {
            let (player, id) = (d[p], d[p + 1]);
            let Some(n) = command_len(id, &d[p + 2..end]) else {
                // Unknown command: the rest of this frame's block can't be framed.
                break;
            };
            let stop = (p + 2 + n).min(end);
            owner = owners.owner(player, owner);
            out.push(Command { frame, player, owner, data: d[p + 1..stop].to_vec() });
            p = stop;
        }
        p = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_sizes() {
        assert_eq!(super::command_len(0x09, &[3]), Some(7));
        assert_eq!(super::command_len(0x63, &[2]), Some(9));
        assert_eq!(super::command_len(0x14, &[]), Some(9));
        assert_eq!(super::command_len(0xEE, &[]), None);
    }
}
