//! The game screen as 1.16.1 keeps it (0x4bd580): the map area lives in a persistent 8-bit
//! buffer and a frame redraws only the 16-pixel cells flagged in the dirty array 0x6ceff8
//! (40 x 25 cells for the map). A scroll (flag 0x6cefb5 & 1, set by 0x49bfd0 when the
//! position changed) redraws all of it; otherwise a cell is redrawn when its terrain or
//! sprites changed (0x4983a0 / 0x42d280 mark the old and new image rects) or when the fog
//! levels around it did (0x4804d0). The cells the game does not flag keep what they showed,
//! so a fog change it misses stays on the screen as a stale block. scbw draws whole frames
//! (`Scene::draw`); this keeps the previous screen and takes the redrawn cells from it.
//!
//! Not modelled: the sprite marks are taken from the drawn pixels (a cell whose pre-fog
//! pixels changed), not from the images' redraw flags, and a sprite that overlaps a flagged
//! cell is not drawn over unflagged cells (0x497ce0): double-darkened shadows are missing.

use crate::fog::{LH, LW};

/// Map cells: 640 x 400 pixels in 16 pixel cells.
const CW: usize = 40;
const CH: usize = 25;
/// 0x6ceff8 holds 30 rows (the whole 640 x 480 screen); marks can run past the map's 25.
const ROWS: usize = 30;

pub struct Screen {
    /// The map area as shown: 640 x 400.
    pub pix: Vec<u8>,
    /// A draw has happened.
    started: bool,
    /// 0x6d5c10: the fog levels the last compare saw.
    prev_levels: Vec<u8>,
    /// 0x628448 / 0x628470: the position of the last draw.
    cam: Option<(i32, i32)>,
    /// Per image (+0x20 / +0x22 / +0x28 / +0x2a): the screen rectangle (x, y, w, h) it had at the
    /// last update, which 0x4983a0 flags before the new one.
    pub rects: Vec<[i16; 4]>,
    /// The images whose REDRAW `Scene::clear_redraw` clears after the draw.
    pub clear: Vec<u32>,
}

impl Screen {
    pub fn new() -> Screen {
        Screen { pix: vec![0; 640 * 400], started: false, prev_levels: vec![0; LW * LH], cam: None, rects: Vec::new(), clear: Vec::new() }
    }

    /// Whether a draw at (`vx`, `vy`) is a scroll (0x6cefb5 & 1, set by 0x49bfd0): the first
    /// draw, or the position moved.
    pub fn is_scroll(&self, vx: i32, vy: i32) -> bool {
        self.cam != Some((vx, vy)) || !self.started
    }

    /// 0x4bd580's flags for a draw at (`vx`, `vy`): all cells on a scroll, else the fog marks
    /// (0x4804d0) and the sprites' (`sprites`, from `Scene::sprite_marks`). `levels` is
    /// 0x47fc50's blurred tile levels.
    pub fn flags(&mut self, vx: i32, vy: i32, levels: &[u8], sprites: &[u8; CW * ROWS]) -> [u8; CW * ROWS] {
        let scrolled = self.is_scroll(vx, vy);
        let mut dirty = [0u8; CW * ROWS];
        if scrolled {
            // 0x47fc50's levels become the compare's old ones (the 0x66 dwords copy) and the
            // whole ring is copied and shaded (0x40c253, 0x4808e0 with eax != 0).
            self.prev_levels.copy_from_slice(levels);
            dirty.fill(1);
        } else {
            self.fog_marks(levels, vx & 0x1f, vy & 0x1f, &mut dirty);
            for c in 0..CW * ROWS {
                dirty[c] |= sprites[c];
            }
        }
        self.started = true;
        self.cam = Some((vx, vy));
        dirty
    }

    /// The terrain copy (0x4bcdc0 / 0x40c253): the flagged cells of `terrain` (the map without
    /// sprites or fog) replace the screen's.
    pub fn copy_terrain(&mut self, dirty: &[u8; CW * ROWS], terrain: &[u8]) {
        self.copy_cells(dirty, terrain);
    }

    /// The fog shading (0x4808e0): the flagged cells of `shaded` (the screen shaded whole) replace
    /// the screen's.
    pub fn copy_shaded(&mut self, dirty: &[u8; CW * ROWS], shaded: &[u8]) {
        self.copy_cells(dirty, shaded);
    }

    fn copy_cells(&mut self, dirty: &[u8; CW * ROWS], src: &[u8]) {
        for c in 0..CW * CH {
            if dirty[c] != 0 {
                let (x0, y0) = ((c % CW) * 16, (c / CW) * 16);
                for y in 0..16 {
                    let o = (y0 + y) * 640 + x0;
                    self.pix[o..o + 16].copy_from_slice(&src[o..o + 16]);
                }
            }
        }
    }

    /// 0x4804d0: compares the blurred levels (`cur`, 0x6d5c0c) with the old ones (0x6d5c10)
    /// tile by tile over the 22 x 15 tiles that can show, copying what differs, and flags
    /// each run of changed tiles with a border of one tile around it (0x41e0d0).
    fn fog_marks(&mut self, cur: &[u8], x_off: i32, y_off: i32, dirty: &mut [u8; CW * ROWS]) {
        let right = 0x2c0 - x_off;
        let bottom = 0x1e0 - y_off;
        let (mut pc, mut pp) = (0x19usize, 0x19usize);
        let mut y = -y_off;
        while y < bottom {
            let mut x = -x_off;
            while x < right {
                let (a, b) = (cur[pc], self.prev_levels[pp]);
                pc += 1;
                pp += 1;
                if a != b {
                    self.prev_levels[pp - 1] = cur[pc - 1];
                    let mut run = 0x20;
                    let mut e = x + 0x20;
                    while e < right {
                        let c = cur[pc];
                        if self.prev_levels[pp] == c {
                            break;
                        }
                        self.prev_levels[pp] = c;
                        run += 0x20;
                        e += 0x20;
                        pc += 1;
                        pp += 1;
                    }
                    mark(dirty, x - 0x20, y - 0x20, y + 0x3f, x + run + 0x1f);
                    x += run - 0x20;
                }
                x += 0x20;
            }
            pc += 2;
            pp += 2;
            y += 0x20;
        }
    }
}

/// 0x41e0d0 (eax left, ecx top, edx bottom, right on the stack): flags the cells of the
/// pixel rectangle, clamped to the 640 x 480 screen (a rectangle wholly outside, or whose
/// right / bottom is negative, flags nothing). Only the map's 25 rows are kept here.
fn mark(dirty: &mut [u8; CW * ROWS], left: i32, top: i32, bottom: i32, right: i32) {
    if left > 0x27f || right < 0 || top > 0x1df || bottom < 0 {
        return;
    }
    let (l, t) = (left.max(0), top.max(0));
    let (r, b) = (right.min(0x27f), bottom.min(0x1df));
    for cy in (t >> 4)..=(b >> 4) {
        for cx in (l >> 4)..=(r >> 4) {
            if (cy as usize) < CH {
                dirty[cy as usize * CW + cx as usize] = 1;
            }
        }
    }
}

/// 0x42d280 (a rectangle of the image at screen (x, y), w x h pixels, from 0x4983a0): fills the
/// cells from `x / 16` for `(x + w + 15) / 16 - x / 16 + 1` bytes on each row from `y / 16`
/// to `(y + h + 15) / 16`, one more cell right and below than the pixels cover, and a row
/// of 40 bytes that runs on into the next row.
pub fn mark_rect(dirty: &mut [u8; CW * ROWS], x: i32, y: i32, w: i32, h: i32) {
    let (l, t) = (x / 16, y / 16);
    let (r, b) = ((x + w + 15) / 16, (y + h + 15) / 16);
    for row in t..=b {
        for k in 0..(r - l + 1) {
            if let Some(c) = dirty.get_mut((row * CW as i32 + l + k) as usize) {
                *c = 1;
            }
        }
    }
}

/// 0x4d57b0 (the part after the frame's map position): the image's rectangle on the 640 x 400
/// screen for the frame at map (`mx`, `my`) of size `w` x `h`, the left / top clipped at 0 and
/// the right / bottom at the screen edge. (x, y, w, h); w or h <= 0 means nothing shows.
pub fn clip_rect(mx: i32, my: i32, w: i32, h: i32, vx: i32, vy: i32) -> [i16; 4] {
    let (mut x, mut y, mut w, mut h) = (mx - vx, my - vy, w, h);
    if x < 0 {
        w += x;
        x = 0;
    }
    w = w.min(640 - x);
    if y < 0 {
        h += y;
        y = 0;
    }
    h = h.min(400 - y);
    [x as i16, y as i16, w as i16, h as i16]
}
