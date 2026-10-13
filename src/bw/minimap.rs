//! The minimap (minimap.cpp): an 8-bit image of the map, one pixel per tile (two
//! for 64-tile maps, half for 192/256), built from the terrain, darkened by fog and
//! overdrawn with one box per unit.

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// 0x51a0f0: the 26 interface colors (0x00bbggrr), matched to the palette as 0x6ceb20.
const UI_RGB: [u32; 26] = [
    0xffffff, 0xe0e0e0, 0xd0d0d0, 0xc0c0c0, 0xb0b0b0, 0xa0a0a0, 0x909090, 0x808080, 0x707070, 0x606060, 0x505050,
    0x404040, 0x303030, 0x202020, 0x101010, 0x000000, 0xe34b00, 0x4fffff, 0x2be300, 0x157100, 0x0000cf, 0x1892ff,
    0xba185a, 0x243c5c, 0xae54dc, 0xffff00,
];

/// 0x5127dc: palette ranges each tileset color-cycles (inclusive).
fn cycling(tileset: u16) -> &'static [(u8, u8)] {
    match tileset {
        0 | 4 => &[(1, 6), (7, 13), (248, 254)],
        3 => &[(1, 4), (5, 8), (9, 12)],
        5..=7 => &[(1, 13), (248, 254)],
        _ => &[],
    }
}

/// 0x4bdb30: nearest palette index to an 0x00bbggrr color, skipping cycling entries
/// (0x6d125c, filled by 0x4bdd60) and index 0.
fn nearest(palette: &[[u8; 3]; 256], excluded: &[bool; 256], rgb: u32) -> u8 {
    let (r, g, b) = ((rgb & 0xff) as i32, (rgb >> 8 & 0xff) as i32, (rgb >> 16 & 0xff) as i32);
    let mut best = (u32::MAX, 0u8);
    for i in 1..256 {
        if excluded[i] {
            continue;
        }
        let [pr, pg, pb] = palette[i].map(|c| c as i32);
        let d = ((b - pb) * (b - pb) + (g - pg) * (g - pg) + (r - pr) * (r - pr)) as u32;
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

pub struct Minimap {
    /// 0x59cc74 / 0x59cc64: image size.
    pub w: u16,
    pub h: u16,
    /// 0x59c1b0: 1 when a tile is two pixels wide.
    shift: u32,
    /// 0x59cc68: tiles per pixel.
    scale: u32,
    /// 0x59cc6c: map pixels per minimap pixel.
    pub tile_px: u16,
    /// 0x59c190: terrain colors.
    terrain: Vec<u8>,
    /// 0x59c198: the finished image.
    pub pixels: Vec<u8>,
    /// 0x59cb60: palette remap moving color-cycling entries to a fixed color.
    remap: [u8; 256],
    /// 0x59c2c0: per fog state (unexplored, fogged, visible, visible) color tables.
    fog: [[u8; 256]; 4],
    /// 0x6ceb20: interface colors (UI_RGB matched to the palette).
    pub colors: [u8; 26],
    /// 0x581dd6: each player's minimap color (tminimap.pcx).
    pub player_colors: [u8; 12],
    vx4: Vec<u16>,
    vr4: Vec<u8>,
    /// 0x6d5bbe: draw units in ally / enemy colors.
    pub ally_colors: bool,
    /// 0x6d5bbf: terrain hidden (black background).
    pub hide_terrain: bool,
    /// 0x6d5bc0: blink phase for units under attack; 0x6d5bc1: some unit is blinking.
    pub blink_phase: bool,
    pub blinking: bool,
    /// 0x59c1a8 / 0x59c2b8: visibility checksum and box count of the last unit pass.
    pub checksum: u32,
    pub boxes: u32,
}

impl Minimap {
    /// 0x4a4400 + 0x4a3720 (size by map size), the remap of 0x4a5a80, 0x41e450 and the
    /// fog tables of 0x4a3a00. `dark` is the tileset's dark.pcx (0x5973a0); vx4 holds
    /// the low 16 bits of each minitile reference.
    pub fn new(
        map_w: u16, map_h: u16, tileset: u16, palette: &[[u8; 3]; 256], dark: &[u8], vx4: Vec<u16>, vr4: Vec<u8>,
        player_colors: [u8; 12],
    ) -> Minimap {
        let (mut w, mut h, mut shift, mut scale, mut tile_px) = (0x80, 0x80, 0, 1, 0x20);
        match map_w.max(map_h) {
            0x40 => (w, h, shift, scale, tile_px) = (map_w * 2, map_h * 2, 1, 1, 0x10),
            0x60 | 0x80 => (w, h, scale, tile_px) = (map_w, map_h, 1, 0x20),
            0xc0 | 0x100 => (w, h, scale, tile_px) = (map_w >> 1, map_h >> 1, 2, 0x40),
            _ => todo_port!("0x4a4400", "minimap for a non-standard map size"),
        }
        let mut excluded = [false; 256];
        for &(lo, hi) in cycling(tileset) {
            excluded[lo as usize..=hi as usize].iter_mut().for_each(|e| *e = true);
        }
        let mut remap = [0u8; 256];
        for (i, r) in remap.iter_mut().enumerate() {
            *r = if excluded[i] { nearest(palette, &excluded, u32::from_le_bytes([palette[i][0], palette[i][1], palette[i][2], 0])) } else { i as u8 };
        }
        let colors = UI_RGB.map(|c| nearest(palette, &excluded, c));
        let mut fog = [[0u8; 256]; 4];
        for i in 0..256 {
            fog[0][i] = colors[0xf];
            fog[1][i] = dark.get(0xf00 + i).copied().unwrap_or(0);
            fog[2][i] = i as u8;
            fog[3][i] = i as u8;
        }
        let n = w as usize * h as usize;
        Minimap {
            w, h, shift, scale, tile_px, terrain: vec![0; n], pixels: vec![0; n], remap, fog, colors, player_colors, vx4, vr4,
            ally_colors: false, hide_terrain: false, blink_phase: false, blinking: false, checksum: 0, boxes: 0,
        }
    }

    /// Color of one minitile's pixel (7, 6) for a megatile id.
    fn tile_color(&self, mega: u16, mini: usize) -> u8 {
        let v = self.vx4.get((mega as usize & 0x7fff) * 16 + mini).copied().unwrap_or(0);
        self.remap[self.vr4.get((v & 0xfffe) as usize * 0x20 + 0x37).copied().unwrap_or(0) as usize]
    }

    /// 0x4a41b0 / 0x4a4240 / 0x4a42b0: terrain colors.
    pub fn draw_terrain(&mut self, st: &State) {
        let t = &st.terrain;
        let mw = t.width as usize;
        let (w, h) = (self.w as usize, self.h as usize);
        match self.scale {
            2 => {
                for y in 0..h {
                    for x in 0..w {
                        self.terrain[y * w + x] = self.tile_color(t.megatiles[y * 2 * mw + x * 2], 0);
                    }
                }
            }
            _ if self.shift == 1 => {
                for ty in 0..h / 2 {
                    for tx in 0..w / 2 {
                        let m = t.megatiles[ty * mw + tx];
                        let o = ty * 2 * w + tx * 2;
                        self.terrain[o] = self.tile_color(m, 0);
                        self.terrain[o + w] = self.tile_color(m, 4);
                        self.terrain[o + 1] = self.tile_color(m, 1);
                        self.terrain[o + w + 1] = self.tile_color(m, 5);
                    }
                }
            }
            _ => {
                for i in 0..w * h {
                    self.terrain[i] = self.tile_color(t.megatiles[i], 0);
                }
            }
        }
    }

    /// Fog table for a tile's flags (bits 0-7 hidden, 8-15 unexplored per player).
    fn fog_table(&self, flags: u32, vision: u32) -> &[u8; 256] {
        // ponytail: replay vision (0x6d0f14 / 0x6d0f18 / 0x6d0f1c) not handled.
        let visible = flags & vision == 0;
        let explored = flags & (vision << 8) == 0;
        &self.fog[(visible as usize) << 1 | explored as usize]
    }

    /// 0x4a4150 with 0x4a3a40 / 0x4a3b30 / 0x4a3c00: fog over the terrain.
    pub fn draw_fog(&mut self, st: &State) {
        if self.hide_terrain {
            self.pixels.fill(0);
            return;
        }
        let t = &st.terrain;
        let mw = t.width as usize;
        let vision = st.local_vision_mask as u32;
        let (w, h) = (self.w as usize, self.h as usize);
        for y in 0..h {
            for x in 0..w {
                let tile = match (self.scale, self.shift) {
                    (2, _) => y * 2 * mw + x * 2,
                    (_, 1) => y / 2 * mw + x / 2,
                    _ => y * mw + x,
                };
                let i = y * w + x;
                self.pixels[i] = self.fog_table(t.tile_flags[tile], vision)[self.terrain[i] as usize];
            }
        }
    }

    /// 0x4a3fd0: one unit's box, at least 2x2 pixels (buildings up to 4x4).
    fn draw_box(&mut self, color: u8, x: i32, y: i32, bw: u8, bh: u8, building: bool) {
        let (hw, hh) = ((bw >> 1) as u32, (bh >> 1) as u32);
        let x = (x as u32).max(hw);
        let y = (y as u32).max(hh);
        let mut left = ((((x - hw) << self.shift) / self.scale) >> 5) as i16;
        let mut top = ((((y - hh) << self.shift) / self.scale) >> 5) as i16;
        let size = |v: u8| {
            let s = (((v as u32) << self.shift) as i32 / self.scale as i32 >> 5) as i16;
            if s < 2 || !building && s > 2 {
                2
            } else {
                s.min(4)
            }
        };
        let (mut w, mut h) = (size(bw), size(bh));
        let right = w + left - 1;
        let bottom = h + top - 1;
        if left < 0 {
            left = 0;
        } else if right as i32 >= self.w as i32 {
            w = self.w as i16 - 1 - left + 1;
        }
        if top < 0 {
            top = 0;
        } else if bottom as i32 >= self.h as i32 {
            h = self.h as i16 - 1 - top + 1;
        }
        // 0x4e1d20
        let pitch = self.w as usize;
        for r in 0..h as u16 {
            let o = (top + r as i16) as usize * pitch + left as usize;
            self.pixels[o..o + w as u16 as usize].fill(color);
        }
        self.boxes += 1;
    }

    /// The vision mask of a unit as the minimap sees it.
    fn unit_vis(st: &State, u: u32) -> u32 {
        let un = &st.units[u as usize];
        let mut v = st.sprites[un.sprite as usize].visibility as u32;
        if un.status & 0x300 != 0 {
            v &= un.visibility;
        }
        v
    }

    fn skip(d: &Data, t: u16) -> bool {
        d.dat.units.flags[t as usize] & 0x10 != 0 || (0xca..=0xd5).contains(&t) || t == 0x69
    }

    fn box_for(&mut self, d: &Data, st: &State, u: u32, color: u8) {
        let un = &st.units[u as usize];
        let t = un.unit_type as usize;
        let sp = &st.sprites[un.sprite as usize];
        let pb = d.dat.units.placement_box[t];
        let building = d.dat.units.flags[t] & 1 != 0;
        self.draw_box(color, sp.x as i32, sp.y as i32, pb as u8, (pb >> 16) as u8, building);
    }

    fn player_color(&self, st: &State, p: usize) -> u8 {
        if !self.ally_colors {
            self.player_colors[p]
        } else if st.alliances[st.local_player as usize][p] == 0 {
            self.colors[0x14]
        } else {
            self.colors[0x11]
        }
    }

    /// 0x4a4ac0: unit boxes, neutral players first, the local player last (on top).
    pub fn draw_units(&mut self, d: &Data, st: &mut State) {
        self.boxes = 0;
        self.checksum = 0;
        let local = st.local_player as usize;
        let vision = st.local_vision_mask as u32;
        for p in (0..12).rev() {
            if p == local {
                continue;
            }
            let base = self.player_color(st, p);
            let mut u = st.player_units[p];
            while u != NIL {
                let t = st.units[u as usize].unit_type;
                if !Self::skip(d, t) {
                    let v = Self::unit_vis(st, u);
                    self.checksum ^= v;
                    if v & vision != 0 {
                        // 0x4a4650: neutral resources are drawn in one color.
                        let res = p >= 8 && ((0xb0..=0xb2).contains(&t) || t == 0xbc);
                        self.box_for(d, st, u, if res { self.colors[0x19] } else { base });
                    }
                }
                u = st.units[u as usize].player_next;
            }
        }
        // 0x4a48e0: the local player's units, blinking while under attack.
        self.blinking = false;
        let mut u = st.player_units[local];
        while u != NIL {
            let t = st.units[u as usize].unit_type;
            if !Self::skip(d, t) {
                let v = Self::unit_vis(st, u);
                self.checksum ^= v;
                if v & vision != 0 {
                    let mut color = self.colors[0x12];
                    let un = &mut st.units[u as usize];
                    if un.last_event_timer != 0 {
                        un.last_event_timer -= 1;
                        self.blinking = true;
                        if self.blink_phase {
                            color = un.last_event_color;
                        }
                    }
                    self.box_for(d, st, u, color);
                }
            }
            u = st.units[u as usize].player_next;
        }
        // Fog sprites (0x654868) are drawn last; they are not ported (0x488350).
    }

    /// 0x4a5200 (each game frame): fog and units over the current terrain.
    pub fn update(&mut self, d: &Data, st: &mut State) {
        self.draw_fog(st);
        self.draw_units(d, st);
        // 0x4a3870: minimap pings, not ported.
    }

    /// 0x4a4ce0: redraw terrain, fog and units.
    pub fn refresh(&mut self, d: &Data, st: &mut State) {
        self.draw_terrain(st);
        self.draw_fog(st);
        self.draw_units(d, st);
    }
}
