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
}

/// A raw game command as it goes through the network/replay stream.
#[derive(Debug, Clone)]
pub struct Command {
    pub frame: u32,
    pub player: u8,
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
        Ok(Replay { modern: modern121 || d[28] == 0x78, header: parse_header(&h), commands: parse_commands(&cmds)?, chk })
    }
}

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

fn parse_commands(d: &[u8]) -> io::Result<Vec<Command>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 5 <= d.len() {
        let frame = u32::from_le_bytes(d[p..p + 4].try_into().unwrap());
        let end = (p + 5 + d[p + 4] as usize).min(d.len());
        p += 5;
        while p + 2 <= end {
            let (player, id) = (d[p], d[p + 1]);
            let Some(n) = command_len(id, &d[p + 2..end]) else {
                // Unknown command: the rest of this frame's block can't be framed.
                break;
            };
            let stop = (p + 2 + n).min(end);
            out.push(Command { frame, player, data: d[p + 1..stop].to_vec() });
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
