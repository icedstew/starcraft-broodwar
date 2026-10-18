//! Pylon power (CUnitProtoss.cpp): the psi provider list (head 0x63ff54, linked through
//! unit +0xf8 / +0xfc), the field shape (0x6d5bd8, built by 0x493360 from the frame of
//! images.dat image 584), power checks (0x4936b0) and the per-frame power update of
//! Protoss buildings (0x493790).

use super::list::NIL;
use super::{Data, State};

/// images.dat id whose first frame is the psi field shape (0x51ced0 + 584 * 4).
pub const PSI_FIELD_IMAGE: u16 = 584;

#[derive(Clone, Debug, Default)]
pub struct Psi {
    /// 0x63ff54: first provider.
    pub providers: u32,
    /// 0x6d5bd8: 2h x 2w cells of 32 px, nonzero = powered.
    pub shape: Vec<u8>,
    /// 0x63ff4c / 0x63ff58: half field size in pixels.
    pub w: i32,
    pub h: i32,
    /// 0x63ff44: provider set changed; recompute power.
    pub dirty: bool,
    /// 0x63ff48: fields are being shown (UI).
    pub shown: bool,
    /// UI images made by the placement highlight (0x4d6f90), freed by 0x4d5b60.
    pub hl_images: Vec<(u32, u16)>,
    /// 0x6d5be4: some highlight image exists.
    pub hl_flag: bool,
}

/// 0x4d4db0: the frame pixel at (x, y) is opaque. `frame` is the GRP frame header
/// (x, y, w, h, offset) and `grp` the whole file.
fn frame_pixel(grp: &[u8], frame: usize, x: u32, y: u32) -> bool {
    let h = &grp[frame..frame + 8];
    let (fw, fh) = (h[2] as u32, h[3] as u32);
    if x >= fw || y >= fh {
        return false;
    }
    let data = u32::from_le_bytes([h[4], h[5], h[6], h[7]]) as usize;
    let row = data + u16::from_le_bytes([grp[data + y as usize * 2], grp[data + y as usize * 2 + 1]]) as usize;
    let mut p = row;
    let mut left = x as i32;
    let mut b;
    loop {
        b = grp[p];
        if b & 0x80 != 0 {
            left -= (b & 0x7f) as i32;
            p += 1;
        } else if b & 0x40 != 0 {
            left -= (b & 0xbf) as i32;
            p += 2;
        } else {
            left -= b as i32;
            p += 1 + b as usize;
        }
        if left <= 0 {
            break;
        }
    }
    b & 0x80 == 0
}

impl State {
    /// 0x493360: build the field shape from the psi field image's GRP file.
    pub fn init_psi(&mut self, grp: &[u8]) {
        let frame = 6;
        let ws = ((grp[frame + 2] as i32 + 0xf) / 32) as usize;
        let hs = ((grp[frame + 3] as i32 + 0xf) / 32) as usize;
        let mut shape = vec![0u8; ws * hs * 4];
        for r in 0..hs {
            let right = r * 2 * ws + ws;
            let mirror = (4 * hs - 2 * r - 1) * ws;
            for c in 0..ws {
                let v = frame_pixel(grp, frame, 0x10 + 32 * c as u32, 0x10 + 32 * r as u32) as u8;
                shape[right + c] = v;
                shape[right - 1 - c] = v;
                shape[mirror + c] = v;
                shape[mirror - c - 1] = v;
            }
        }
        self.psi = Psi { providers: NIL, shape, w: ws as i32 * 32, h: hs as i32 * 32, dirty: false, shown: false, ..Default::default() };
    }

    /// 0x4936b0: a unit type `t` placed at (x, y) by `player` is powered (types that
    /// don't need power always are).
    pub fn psi_powered(&self, d: &Data, t: u16, x: i32, y: i32, player: u8) -> bool {
        if d.dat.units.flags[t as usize] & 0x8_0000 == 0 {
            return true;
        }
        let ps = &self.psi;
        let mut p = ps.providers;
        while p != NIL {
            let pu = &self.units[p as usize];
            if pu.player == player {
                let (px, py) = self.unit_sprite_pos(p);
                let (px, py) = (px as i32, py as i32);
                if (px - x).abs() < ps.w && (py - y).abs() < ps.h {
                    let cx = (ps.w - px + x) / 32;
                    let cy = (ps.h - py + y) / 32;
                    if ps.shape[(cx + cy * (ps.w / 32) * 2) as usize] != 0 {
                        return true;
                    }
                }
            }
            p = pu.psi_next;
        }
        false
    }

    /// 0x493f70: InitializePsiProvider: join the provider list and make the field
    /// sprite; then go idle either way.
    pub fn order_initialize_psi_provider(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if self.psi.providers != u && un.psi_next == NIL && un.psi_prev == NIL {
            let head = self.psi.providers;
            self.units[u as usize].psi_prev = NIL;
            self.units[u as usize].psi_next = head;
            if head != NIL {
                self.units[head as usize].psi_prev = u;
            }
            self.psi.providers = u;
            self.create_psi_sprite(d, u);
            self.psi.dirty = true;
        }
        let idle = d.dat.units.ai_return_to_idle[self.units[u as usize].unit_type as usize];
        self.order_now(d, u, idle);
        self.order_next_or_idle(d, u);
    }

    /// 0x493bf0: the provider's field sprite (0x141, hidden unless fields are shown),
    /// kept in unit +0xd0.
    fn create_psi_sprite(&mut self, d: &Data, u: u32) -> bool {
        let (x, y) = self.unit_sprite_pos(u);
        let Some(s) = self.create_sprite(d, 0x141, x, y, 0) else {
            self.units[u as usize].set_ptr_at(0xd0, NIL);
            return false;
        };
        self.units[u as usize].set_ptr_at(0xd0, s);
        self.sprites[s as usize].elevation = 1;
        // 0x497480: only the local player sees fields, while they are shown.
        if self.units[u as usize].player == self.local_player && !self.replay && self.psi.shown {
            let m = self.local_vision_mask;
            self.sprite_set_visibility(s, m);
        } else {
            self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
            self.sprite_set_visibility(s, 0);
        }
        true
    }

    /// 0x493640: show the field sprite of every own provider (not while watching a
    /// replay), making the sprite first if it has none.
    pub fn psi_show_fields(&mut self, d: &Data) {
        let mut p = self.psi.providers;
        while p != NIL {
            let next = self.units[p as usize].psi_next;
            if !self.replay && self.units[p as usize].player == self.local_player {
                let mut s = self.units[p as usize].ptr_at(0xd0);
                if s == NIL && self.create_psi_sprite(d, p) {
                    s = self.units[p as usize].ptr_at(0xd0);
                }
                if s != NIL {
                    self.sprites[s as usize].flags &= !super::sprite::sflag::HIDDEN;
                    let m = self.local_vision_mask;
                    self.sprite_set_visibility(s, m);
                }
            }
            p = next;
        }
        self.psi.shown = true;
    }

    /// 0x494100: hide every provider's field sprite.
    pub fn psi_hide_fields(&mut self, d: &Data) {
        let mut p = self.psi.providers;
        while p != NIL {
            let next = self.units[p as usize].psi_next;
            let mut s = self.units[p as usize].ptr_at(0xd0);
            if s == NIL && self.create_psi_sprite(d, p) {
                s = self.units[p as usize].ptr_at(0xd0);
            }
            if s != NIL {
                self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
                self.sprite_set_visibility(s, 0);
            }
            p = next;
        }
        self.psi.shown = false;
    }

    /// 0x493100: leave the provider list.
    pub fn remove_psi_provider(&mut self, u: u32) {
        let (prev, next) = (self.units[u as usize].psi_prev, self.units[u as usize].psi_next);
        if prev != NIL {
            self.units[prev as usize].psi_next = next;
        }
        if next != NIL {
            self.units[next as usize].psi_prev = prev;
        }
        self.units[u as usize].psi_prev = NIL;
        if self.psi.providers == u {
            self.psi.providers = next;
        }
        self.units[u as usize].psi_next = NIL;
    }

    /// 0x493790: after the provider set changed, power or unpower every Protoss building.
    pub fn update_power(&mut self, d: &Data) {
        if !self.psi.dirty {
            return;
        }
        let mut u = self.visible_units.head;
        while u != NIL {
            let next = self.units[u as usize].link.next;
            let un = &self.units[u as usize];
            let g = d.dat.units.staredit_group_flags[un.unit_type as usize];
            if un.status & 2 != 0 && g & 1 == 0 && g & 4 != 0 && self.player_type(un.player) != 3 {
                let st = un.status;
                let was_powered = st & 0x400 == 0;
                let (x, y) = self.unit_sprite_pos(u);
                if !self.psi_powered(d, un.unit_type, x as i32, y as i32, un.player) {
                    self.units[u as usize].status = st | 0x400;
                    self.order_start(d, u);
                    if st & 1 != 0 && was_powered {
                        let s = self.units[u as usize].sprite;
                        self.sprite_images_run_anim(d, s, 0x18);
                    }
                } else {
                    self.units[u as usize].status = st & !0x400;
                    if st & 1 != 0 && !was_powered {
                        let s = self.units[u as usize].sprite;
                        self.sprite_images_run_anim(d, s, 0x1b);
                        let un = &self.units[u as usize];
                        let busy = un.order == 0x1f && un.status & 2 != 0 && un.order_target_unit != NIL && self.units[un.order_target_unit as usize].status & 1 == 0;
                        if busy || un.u8_at(0xc9) != 0x3d {
                            self.sprite_images_run_anim(d, s, 0x13);
                        }
                    }
                }
            }
            u = next;
        }
        self.psi.dirty = false;
    }
}
