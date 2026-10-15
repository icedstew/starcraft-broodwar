//! Classic (SD) GRP sprite sheets: u16 frames, u16 w, u16 h, then per frame
//! {u8 x, u8 y, u8 w, u8 h, u32 offset} -> u16 row offsets -> RLE rows.

pub struct Grp {
    pub w: i32,
    pub h: i32,
    data: Vec<u8>,
    frames: Vec<Frame>,
    /// Uncompressed variant (UI grps): each frame is w*h raw pixels, 0 = transparent.
    raw: bool,
}

#[derive(Clone, Copy)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    off: usize,
}

/// How a pixel is combined with the destination.
#[derive(Clone, Copy)]
pub enum Blend<'a> {
    Normal,
    /// Palette indices 8..=15 replaced by this player's colors.
    Player(&'a [u8; 8]),
    /// Shadow: darken via lookup table.
    Remap(&'a [u8; 256]),
    /// Source colors replaced through this table (selection circles).
    Colors(&'a [u8; 256]),
    /// Translucent remap (ofire, bexpl, ...): out = table[(src - 1) * 256 + dst].
    Table(&'a [u8]),
}

impl Grp {
    pub fn parse(data: Vec<u8>) -> Grp {
        let n = u16::from_le_bytes([data[0], data[1]]) as usize;
        let w = u16::from_le_bytes([data[2], data[3]]) as i32;
        let h = u16::from_le_bytes([data[4], data[5]]) as i32;
        let frames: Vec<Frame> = (0..n)
            .map(|i| {
                let f = &data[6 + i * 8..14 + i * 8];
                let off = u32::from_le_bytes(f[4..8].try_into().unwrap());
                // 0x41d160: the offset's top bit adds 256 to the width.
                Frame {
                    x: f[0] as i32,
                    y: f[1] as i32,
                    w: f[2] as i32 + if off & 0x8000_0000 != 0 { 0x100 } else { 0 },
                    h: f[3] as i32,
                    off: (off & 0x7fff_ffff) as usize,
                }
            })
            .collect();
        // Raw frames are packed back to back with no row-offset table.
        let raw = frames.len() > 1 && frames.windows(2).all(|p| p[1].off == p[0].off + (p[0].w * p[0].h) as usize || p[1].off == p[0].off);
        Grp { w, h, data, frames, raw }
    }

    /// A grp whose frames are uncompressed w*h bitmaps (dialog grps: 0x4ef600 blits
    /// them as bitmaps), whatever their offsets look like.
    pub fn parse_raw(data: Vec<u8>) -> Grp {
        Grp { raw: true, ..Grp::parse(data) }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn frame(&self, i: usize) -> Option<&Frame> {
        self.frames.get(i)
    }

    /// Draw frame `i` with its grp-box top-left at (x, y) in `dst` (w*h indexed pixels).
    pub fn draw(&self, i: usize, flip: bool, x: i32, y: i32, dst: &mut [u8], dw: i32, dh: i32, blend: Blend) {
        let Some(f) = self.frames.get(i) else { return };
        let fx = if flip { x + self.w - f.x - f.w } else { x + f.x };
        let fy = y + f.y;
        if self.raw {
            for row in 0..f.h {
                for col in 0..f.w {
                    let (px, py) = (if flip { fx + f.w - 1 - col } else { fx + col }, fy + row);
                    let c = self.data.get(f.off + (row * f.w + col) as usize).copied().unwrap_or(0);
                    if c != 0 && px >= 0 && py >= 0 && px < dw && py < dh {
                        dst[(py * dw + px) as usize] = c;
                    }
                }
            }
            return;
        }
        for row in 0..f.h {
            let py = fy + row;
            if py < 0 || py >= dh {
                continue;
            }
            let ro = f.off + row as usize * 2;
            let Some(b) = self.data.get(ro..ro + 2) else { return };
            let mut p = f.off + u16::from_le_bytes([b[0], b[1]]) as usize;
            let mut col = 0;
            let line = (py * dw) as usize;
            let mut put = |col: i32, c: u8| {
                let px = if flip { fx + f.w - 1 - col } else { fx + col };
                if px >= 0 && px < dw {
                    let d = &mut dst[line + px as usize];
                    *d = match blend {
                        Blend::Normal => c,
                        Blend::Player(pc) if (8..16).contains(&c) => pc[c as usize - 8],
                        Blend::Player(_) => c,
                        Blend::Remap(t) => t[*d as usize],
                        Blend::Colors(t) => t[c as usize],
                        Blend::Table(t) => t.get((c as usize).wrapping_sub(1) * 256 + *d as usize).copied().unwrap_or(c),
                    };
                }
            };
            while col < f.w {
                let Some(&op) = self.data.get(p) else { break };
                p += 1;
                if op & 0x80 != 0 {
                    col += (op & 0x7F) as i32;
                } else if op & 0x40 != 0 {
                    let c = self.data.get(p).copied().unwrap_or(0);
                    p += 1;
                    for _ in 0..(op & 0x3F) {
                        put(col, c);
                        col += 1;
                    }
                } else {
                    for _ in 0..op {
                        put(col, self.data.get(p).copied().unwrap_or(0));
                        p += 1;
                        col += 1;
                    }
                }
            }
        }
    }
}
