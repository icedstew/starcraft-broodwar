//! Scenario sections applied when a game starts (1.16.1 section table 0x5002a0).

use super::list::NIL;
use super::{Data, State};
use crate::todo_port;

/// Game-mode flags the UNIT loader consults (0x59686d / 0x596871 / 0x596877).
#[derive(Clone, Copy, Debug, Default)]
pub struct ScenarioModes {
    pub flag_59686d: bool,
    pub flag_596871: bool,
    pub flag_596877: bool,
}

fn u16_at(e: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([e[o], e[o + 1]])
}

impl State {
    /// 0x4cd7a0: UNIT section: start locations and map-placed units.
    pub fn chk_units(&mut self, d: &Data, data: &[u8], modes: ScenarioModes) -> bool {
        if data.len() % 0x24 != 0 {
            return false;
        }
        let mut starts = [(0i16, 0i16); 8];
        for e in data.chunks_exact(0x24) {
            let ty = u16_at(e, 8);
            let player = e[0x10];
            // 0x4cb190: start locations are recorded, not created.
            if ty == 0xd6 {
                if (player as usize) < 8 {
                    starts[player as usize] = (u16_at(e, 4) as i16, u16_at(e, 6) as i16);
                } else {
                    todo_port!("0x4cb190", "start location with player >= 8 writes past the 0x20 buffer");
                }
                continue;
            }
            if player < 8 {
                let t = self.player_type(player);
                if t == 0 || (t > 4 && t != 7) {
                    continue;
                }
            }
            // 0x4cbe20
            if modes.flag_596871 && !(player == 0xb && matches!(ty, 0xb0..=0xb2 | 0xbc | 0x59 | 0x5a | 0x5f | 0x5d | 0x5e | 0x60)) {
                continue;
            }
            if !(modes.flag_59686d || modes.flag_596871 || modes.flag_596877) {
                // 0x4a8b90: per-player "use map units" setting (0x59bda8).
                let keep_player_units = if player < 8 { self.melee_map_units[player as usize] } else { 0 };
                if keep_player_units != 0 && d.dat.units.staredit_group_flags[ty as usize] & 0x80 == 0 {
                    continue;
                }
            }
            let Some(u) = self.chk_place_unit(d, ty, u16_at(e, 4) as i16, u16_at(e, 6) as i16, player) else {
                continue;
            };
            self.chk_unit_props(d, u, &e[0xc..]);
            if ty == 0x86 || d.dat.units.staredit_group_flags[ty as usize] & 2 != 0 {
                todo_port!("0x4cbef0", "link nydus exits / addons placed by the map (0x4cbd30, 0x4ccf90)");
            }
        }
        self.start_locations = starts;
        let mut u = self.visible_units.head;
        while u != NIL {
            todo_port!("0x4ebe10", "sprite position / vision update for map units");
            u = self.units[u as usize].link.next;
        }
        true
    }

    /// 0x4cd360: create and finish a map-placed unit.
    fn chk_place_unit(&mut self, d: &Data, ty: u16, x: i16, y: i16, player: u8) -> Option<u32> {
        if d.dat.units.staredit_availability[ty as usize] & 0x200 != 0 && !self.is_expansion {
            return None;
        }
        let Some(u) = self.create_unit(d, ty, x, y, player) else {
            todo_port!("0x49e530", "show the placement error");
            return None;
        };
        let t = self.units[u as usize].unit_type;
        if matches!(t, 0x84 | 0x85 | 0x90 | 0x92 | 0x83 | 0x8f) || d.dat.units.flags[t as usize] & 0x20000 != 0 {
            todo_port!("0x4148f0", "spread creep under map-placed zerg buildings");
        }
        self.finish_unit_pre(d, u);
        if self.place_unit(d, u) {
            self.finish_unit_post(d, u);
            if self.player_type(self.units[u as usize].player) == 1 {
                todo_port!("0x462960", "register map unit with the computer player's town");
            }
            return Some(u);
        }
        todo_port!("0x4cd40e", "placement failure message and removal (0x475710)");
        self.units[u as usize].order_flags |= 4;
        None
    }

    /// 0x4cd480: apply the UNIT entry's properties (`p` = entry + 0xc).
    fn chk_unit_props(&mut self, d: &Data, u: u32, p: &[u8]) {
        let valid = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
        let state = p[0xe];
        let ut = self.units[u as usize].unit_type as usize;
        if valid & 0x20000 != 0 {
            let max = d.dat.units.hit_points[ut];
            let mut hp = ((p[5] as u32).wrapping_mul(max) as u64 / 100) as u32;
            if hp == 0 {
                hp = 1;
            }
            // 0x467340
            let un = &mut self.units[u as usize];
            un.hp = hp as i32;
            if un.hp > max as i32 {
                un.hp = max as i32;
            }
            todo_port!("0x467340", "damage overlays (0x4e6090) and AI strength (0x431800) after setting hp");
        }
        let max_sh = d.dat.units.shields[ut] as u32;
        let sh = (p[6] as u32 * max_sh) / 100;
        if valid & 0x40000 != 0 {
            let v = if (max_sh as i32) < sh as i32 { max_sh } else { sh };
            self.units[u as usize].shields = (v << 8) as i32;
        }
        // 0x4cc3b0
        let max_en = self.max_energy(d, u) as i32;
        let en = (((max_en >> 8) as u32).wrapping_mul(p[7] as u32) / 100) as u8;
        if valid & 0x80000 != 0 {
            self.units[u as usize].energy = if (max_en >> 8) < en as i32 { ((max_en >> 8) as u16 & 0xff) << 8 } else { (en as u16) << 8 };
        }
        if valid & 0x100000 != 0 && d.dat.units.flags[ut] & 0x2000 != 0 {
            self.units[u as usize].set_u16_at(0xd0, u16_at(p, 8));
            if (0xb0..=0xb2).contains(&ut) {
                self.mineral_frames(d, u);
            }
        }
        if valid & 0x200000 != 0 && matches!(ut, 0x48 | 0x52 | 0x53 | 0x51) {
            todo_port!("0x4cd260", "map-placed carrier/reaver hangar units");
        }
        if valid & 1 != 0 && state & 1 != 0 {
            todo_port!("0x491a50", "map-placed cloaked unit");
        }
        if valid & 2 != 0 && state & 2 != 0 {
            todo_port!("0x4ccf20", "map-placed burrowed unit");
        }
        if valid & 4 != 0 && state & 4 != 0 {
            todo_port!("0x4cc470", "map-placed lifted building / unit in transit");
        }
        if valid & 8 != 0 && state & 8 != 0 {
            todo_port!("0x4f6420", "map-placed hallucination");
        }
        if valid & 0x10 != 0 && state & 0x10 != 0 {
            self.units[u as usize].status |= 0x400_0000;
        }
        if ut == 0x28 {
            let un = &mut self.units[u as usize];
            let v: u16 = if un.status & 0x4000_0000 != 0 { 0x546 } else { 0x708 };
            if un.remove_timer == 0 || v < un.remove_timer {
                un.remove_timer = v;
            }
        }
    }

    /// 0x468830: mineral field frame set from its amount.
    pub fn mineral_frames(&mut self, d: &Data, u: u32) {
        let amount = self.units[u as usize].resource_amount();
        let anim: u8 = if amount < 0x2ee {
            if amount < 500 { 0xe - (amount < 0xfa) as u8 } else { 0xf }
        } else {
            0x14
        };
        if self.units[u as usize].u8_at(0xd2) != anim {
            self.units[u as usize].set_u8_at(0xd2, anim);
            let s = self.units[u as usize].sprite;
            let mut img = self.sprites[s as usize].images.head;
            while img != NIL {
                let next = self.images[img as usize].link.next;
                self.image_run_anim(d, img, anim);
                img = next;
            }
        }
    }
}
