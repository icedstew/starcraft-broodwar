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

impl ScenarioModes {
    /// The template fields the loaders test (0x59686d, 0x596871, 0x596877).
    pub fn of_template(t: &[u8; 0x20]) -> ScenarioModes {
        ScenarioModes { flag_59686d: t[8] != 0, flag_596871: t[0xc] != 0, flag_596877: t[0x12] != 0 }
    }
}

fn u16_at(e: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([e[o], e[o + 1]])
}

impl State {
    /// 0x4c3ed0: alliances (0x58d634) and shared vision (0x57f1ec) at game start.
    pub fn init_alliances(&mut self, modes: ScenarioModes) {
        let ty = self.player_types;
        let a = &mut self.alliances;
        *a = [[0; 12]; 12];
        for i in 0..8 {
            a[i][i] = 1;
        }
        for p in (0..12).rev() {
            a[11][p] = 1;
            a[p][11] = 1;
            if self.template[0xf] == 1 && !modes.flag_596877 {
                for q in (0..8).rev() {
                    if q != p && ty[p] == 1 && ty[q] == 1 {
                        a[p][q] = 2;
                        a[q][p] = 2;
                    }
                }
            }
        }
        for p in 0..12 {
            self.player_vision[p] = 1 << p;
            if ty[p] == 3 || ty[p] == 7 {
                for q in 0..12 {
                    self.alliances[p][q] = 1;
                    self.alliances[q][p] = 1;
                }
            }
        }
        if !(modes.flag_59686d || modes.flag_596871 || modes.flag_596877) {
            for f in 1..=4u8 {
                let props = self.force_flags[f as usize - 1];
                let v = if props & 2 == 0 { 0 } else { 1 + (props & 4 != 0) as u8 };
                self.force_alliance(f, v);
                self.force_vision(f, props & 8 != 0);
            }
        }
        if self.template[0] == 0xf {
            for p in 0..11 {
                for q in 0..11 {
                    if ty[p] != 0 && ty[q] != 0 {
                        let same = self.player_force[p] == self.player_force[q];
                        self.alliances[p][q] = if same { 2 } else { 0 };
                        if same {
                            self.player_vision[p] |= 1 << q;
                        }
                    }
                }
            }
        }
    }

    /// 0x45ab20: set alliances between the human players of force `f`.
    fn force_alliance(&mut self, f: u8, v: u8) {
        for p in 0..8 {
            if self.player_force[p] != f || self.player_types[p] != 2 {
                continue;
            }
            for q in 0..8 {
                if (v != 0 || p != q) && self.player_force[q] == f && self.player_types[q] == 2 {
                    self.alliances[p][q] = v;
                }
            }
        }
    }

    /// 0x45a9b0: share (or stop sharing) vision between the human players of force `f`.
    fn force_vision(&mut self, f: u8, on: bool) {
        for p in 0..8 {
            if self.player_force[p] != f || self.player_types[p] != 2 {
                continue;
            }
            for q in 0..8 {
                if p != q && self.player_force[q] == f && self.player_types[q] == 2 {
                    if on {
                        self.player_vision[p] |= 1 << q;
                    } else {
                        self.player_vision[p] &= !(1 << q);
                    }
                }
            }
        }
    }

    /// 0x4cd600: THG2 section: map sprites and doodad units.
    pub fn chk_doodads(&mut self, d: &Data, data: &[u8], modes: ScenarioModes) -> bool {
        if data.len() % 10 != 0 {
            return false;
        }
        for e in data.chunks_exact(10) {
            let (id, x, y) = (u16_at(e, 0), u16_at(e, 2) as i16, u16_at(e, 4) as i16);
            let mut player = e[6];
            let flags = e[9];
            if flags & 0x10 != 0 {
                // 0x488210
                self.create_thingy(d, id, x, y, player);
                continue;
            }
            if matches!(id, 0xcd..=0xd0) {
                player = 0xb;
            }
            if (modes.flag_59686d || modes.flag_596871 || modes.flag_596877) && player != 0xb {
                continue;
            }
            if let Some(u) = self.chk_place_unit(d, id, x, y, player) {
                if flags & 0x80 != 0 {
                    self.disable_doodad(d, u);
                }
            }
        }
        true
    }

    /// 0x47b4b0: disable a map-placed doodad unit (THG2 flag 0x8000).
    fn disable_doodad(&mut self, d: &Data, u: u32) {
        let un = &self.units[u as usize];
        if un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0 {
            return;
        }
        self.units[u as usize].status |= 0x400;
        let s = self.units[u as usize].sprite;
        self.sprite_images_run_anim(d, s, 0xf);
        let un = &mut self.units[u as usize];
        un.status |= 0x20_0000;
        let t = un.unit_type;
        if matches!(t, 0xcb | 0xd1 | 0xcd..=0xd0) {
            self.sprites[s as usize].elevation = 1;
        }
        if matches!(t, 0xcb | 0xd1..=0xd5) {
            let un = &mut self.units[u as usize];
            un.status |= 0x300;
            un.visibility = 0x8000_0000;
            un.ai_action_flag = 0;
        }
    }

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
            self.update_unit_visibility(d, u);
            u = self.units[u as usize].link.next;
        }
        true
    }

    /// 0x49e010 (game start, from 0x49f380): each unit type's acquisition range is at
    /// least the range of its (or its subunit's) weapons.
    pub fn init_acquisition_ranges(&mut self, d: &Data) {
        let (u, w) = (&d.dat.units, &d.dat.weapons);
        let mut acq = u.target_acquisition_range.clone();
        for i in 0..acq.len() {
            let s = match u.subunit1[i] {
                0xe4 => i,
                s => s as usize,
            };
            let mut r = acq[s] as u32;
            for wp in [u.ground_weapon[s], u.air_weapon[s]] {
                if wp < 0x82 {
                    r = r.max(w.max_range[wp as usize] >> 5);
                }
            }
            acq[i] = r as u8;
        }
        self.acquisition = acq;
    }

    /// 0x4ccc80 (defaults part): every unit and tech available, nothing researched,
    /// upgrade caps from upgrades.dat.
    pub fn init_tech_defaults(&mut self, d: &Data) {
        let pl = &mut self.players;
        pl.unit_available = [[1; 228]; 12];
        pl.tech_available = [[1; 44]; 12];
        pl.tech_researched = [[0; 44]; 12];
        pl.research_in_progress = [[0; 6]; 12];
        for p in 0..12 {
            for u in 0..61 {
                pl.upgrade_max[p][u] = d.dat.upgrades.max_level[u];
            }
        }
    }

    /// Create a unit and finish it as the start-of-game code does (0x4a09d0, 0x4a01f0,
    /// 0x49ec30, 0x49fa40).
    fn start_unit(&mut self, d: &Data, t: u16, x: i16, y: i16, p: u8) -> Option<u32> {
        let u = self.create_unit(d, t, x, y, p)?;
        self.finish_unit_pre(d, u);
        if self.place_unit(d, u) {
            self.finish_unit_post(d, u);
        }
        Some(u)
    }

    /// 0x49da40: starting units (`mode` = 0x596871: 1 workers only, 2 town hall and
    /// workers) for every human and computer player at its start location. With map
    /// settings (template +8 / +0xc / +0x12 all 0), players flagged in 0x59bda8 get a
    /// town hall and workers.
    pub fn melee_start_units(&mut self, d: &Data, template_mode: u8) {
        for p in (0..8u8).rev() {
            let ty = self.player_type(p);
            if ty != 2 && ty != 1 {
                continue;
            }
            let t = &self.template;
            let mode = if t[8] == 0 && template_mode == 0 && t[0x12] == 0 && self.melee_map_units[p as usize] != 0 { 2 } else { template_mode };
            let race = self.player_races[p as usize];
            if mode == 2 {
                self.start_town_hall(d, race, p);
                if race == 0 {
                    self.start_overlord(d, p);
                }
            } else if mode != 1 {
                continue;
            }
            let worker = match race {
                0 => 0x29,
                1 => 7,
                2 => 0x40,
                _ => 0xe4,
            };
            let (x, y) = self.start_locations[p as usize];
            for _ in 0..4 {
                self.start_unit(d, worker, x, y, p);
            }
        }
    }

    /// 0x49d7c0: the town hall on the start location's tile grid; units in its way die.
    fn start_town_hall(&mut self, d: &Data, race: u8, p: u8) {
        let t: u16 = match race {
            0 => 0x83,
            1 => 0x6a,
            2 => 0x9a,
            _ => 0xe4,
        };
        let v = d.dat.units.placement_box[t as usize];
        let (w2, h2) = ((v as i16 as i32 / 2) as u16, ((v >> 16) as i16 as i32 / 2) as u16);
        let (sx, sy) = self.start_locations[p as usize];
        let x = ((sx as u16).wrapping_sub(w2) & 0xffe0).wrapping_add(w2);
        let y = ((sy as u16).wrapping_sub(h2) & 0xffe0).wrapping_add(h2);
        let b = [x.wrapping_sub(w2) as i16, y.wrapping_sub(h2) as i16, w2.wrapping_add(x).wrapping_sub(1) as i16, h2.wrapping_add(y).wrapping_sub(1) as i16];
        // 0x4e8830 with 0x49d640: kill everything overlapping the town hall.
        let start = self.units_in_box(d, b);
        let found = self.search.results(start);
        self.search.pop();
        for u in found {
            self.units[u as usize].order_flags |= 4;
            self.kill_unit(d, u);
        }
        let Some(u) = self.start_unit(d, t, x as i16, y as i16, p) else { return };
        let ut = self.units[u as usize].unit_type;
        if matches!(ut, 0x84 | 0x85 | 0x90 | 0x92 | 0x83 | 0x8f) || d.dat.units.flags[ut as usize] & 0x2_0000 != 0 {
            let (x, y) = self.unit_sprite_pos(u);
            self.spread_creep_instantly(d, ut, x as i32, y as i32);
            self.spawn_larva(d, u);
            self.spawn_larva(d, u);
        }
    }

    /// 0x49d6c0: a zerg player's overlord, one hatchery size toward the map centre.
    fn start_overlord(&mut self, d: &Data, p: u8) {
        let v = d.dat.units.dimensions[0x83];
        let [l, u, r, dn] = [v as i16, (v >> 16) as i16, (v >> 32) as i16, (v >> 48) as i16];
        let dx = l.wrapping_add(r).wrapping_add(1) as i32;
        let dy = dn.wrapping_add(u).wrapping_add(1) as i32;
        let (sx, sy) = self.start_locations[p as usize];
        let (sx, sy) = (sx as i32, sy as i32);
        let x = if sx < (self.map_px.0 >> 1) as i32 { sx + dx } else { sx - dx };
        let y = if sy < (self.map_px.1 >> 1) as i32 { sy + dy } else { sy - dy };
        self.start_unit(d, 0x2a, x as i16, y as i16, p);
    }

    /// 0x4cd360: create and finish a map-placed unit.
    fn chk_place_unit(&mut self, d: &Data, ty: u16, x: i16, y: i16, player: u8) -> Option<u32> {
        if d.dat.units.staredit_availability[ty as usize] & 0x200 != 0 && !self.is_expansion {
            return None;
        }
        let Some(u) = self.create_unit(d, ty, x, y, player) else {
            self.show_last_error(player);
            return None;
        };
        let t = self.units[u as usize].unit_type;
        if matches!(t, 0x84 | 0x85 | 0x90 | 0x92 | 0x83 | 0x8f) || d.dat.units.flags[t as usize] & 0x20000 != 0 {
            let (x, y) = self.unit_sprite_pos(u);
            self.spread_creep_instantly(d, t, x as i32, y as i32);
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
    pub(super) fn chk_unit_props(&mut self, d: &Data, u: u32, p: &[u8]) {
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
            self.hp_changed(d, u);
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
        if valid & 0x200000 != 0 {
            self.fill_hangar(d, u, p[0xc]);
        }
        if valid & 1 != 0 && state & 1 != 0 {
            // 0x491a50: personnel cloaking (10) for ghosts and their heroes, cloaking
            // field (9) for wraiths, none (0x2c) otherwise.
            let tech = match ut {
                1 | 0x10 | 100 | 99 | 0x68 | 0x33 => 10,
                8 | 0x15 => 9,
                _ => 0x2c,
            };
            let pl = self.units[u as usize].player;
            if self.can_use_tech(d, u, tech, pl) == Ok(1) {
                self.burrowed_cloak(d, u);
            }
        }
        // 0x4ccf20
        if valid & 2 != 0 && state & 2 != 0 {
            let pl = self.units[u as usize].player;
            if self.can_use_tech(d, u, 0xb, pl) == Ok(1) {
                let s = self.units[u as usize].sprite;
                self.sprite_images_run_anim(d, s, 0xe);
                self.go_underground(u);
                self.burrowed_cloak(d, u);
                self.units[u as usize].status |= 0x20_0000;
                self.order_now(d, u, 0x75);
            }
        }
        // 0x4cc470: a lifted building, if its owner could lift it off (0x46dc20 inline,
        // its error code into 0x66ff60).
        if valid & 4 != 0 && state & 4 != 0 {
            let pl = self.units[u as usize].player;
            if self.can_issue_order(d, u, 0x48, pl).is_ok() {
                let s = self.units[u as usize].sprite;
                let tail = self.sprites[s as usize].images.tail;
                if tail != NIL && self.images[tail as usize].modifier == 10 && self.images[tail as usize].y != 0x2a {
                    self.images[tail as usize].flags |= 1;
                    self.images[tail as usize].y = 0x2a;
                }
                self.sprite_images_run_anim(d, s, 0xd);
                self.unit_leave_map_grids(d, u);
                // 0x463470
                {
                    let t = self.units[u as usize].unit_type;
                    let un = &mut self.units[u as usize];
                    un.status = (un.status & !2) | 0x1_0000;
                    if d.dat.units.flags[t as usize] & 0x10 == 0 {
                        un.status |= 0x2_0000;
                    } else {
                        un.status &= !0x2_0000;
                    }
                }
                self.path_free(u);
                {
                    let elev = self.sprites[s as usize].elevation;
                    let un = &mut self.units[u as usize];
                    un.movement_state = 0;
                    un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
                }
                self.sprites[s as usize].elevation = 0xc;
                self.set_air(u, true);
                self.unit_enter_map_grids(d, u);
            }
        }
        if valid & 8 != 0 && state & 8 != 0 {
            self.make_hallucination(d, u);
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

    /// 0x4f6420: turn `u` (and its subunit) into a hallucination: out of the unit counts,
    /// hallucination buttons, a 0x546-frame life, drawn with the hallucination draw function.
    pub fn make_hallucination(&mut self, d: &Data, u: u32) {
        self.modify_unit_counters(d, u, -1);
        self.modify_completed_counters(d, u, -1, true);
        let t = self.units[u as usize].unit_type as usize;
        let building = d.dat.units.flags[t] & 1 != 0;
        let un = &mut self.units[u as usize];
        let disabled = un.status & 0x400 != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0;
        if !disabled || building {
            un.button_set = 0xf4;
        }
        un.status |= crate::bw::unit::status::HALLUCINATION;
        self.modify_unit_counters(d, u, 1);
        self.modify_completed_counters(d, u, 1, false);
        if self.replay || self.units[u as usize].player == self.local_player {
            // 0x497dc0: the sprite's images switch to the hallucination draw function.
            let mut i = self.sprites[self.units[u as usize].sprite as usize].images.head;
            while i != NIL {
                let img = &mut self.images[i as usize];
                if img.modifier == 0 || img.modifier == 0x10 {
                    img.modifier = 0x10;
                    img.flags |= 1;
                    img.coloring = 0;
                }
                i = img.link.next;
            }
        }
        if d.dat.units.flags[t] & 0x10 != 0 {
            return;
        }
        let un = &mut self.units[u as usize];
        if un.remove_timer == 0 || 0x546 < un.remove_timer {
            un.remove_timer = 0x546;
        }
        self.refresh_strength(d, u);
        let un = &mut self.units[u as usize];
        if un.secondary_order != 0x95 {
            un.secondary_order = 0x95;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
        let sub = un.subunit;
        if sub != NIL {
            self.make_hallucination(d, sub);
        }
        if matches!(t, 0x48 | 0x52 | 0x53 | 0x51) {
            self.kill_fighters(d, u);
        }
    }
}

/// Use Map Settings sections that rewrite the dat tables (as the game's memory copies of
/// units.dat / weapons.dat / upgrades.dat / techdata.dat are): UNIx (0x4cacd0), UPGx
/// (0x4ca9f0), TECx (0x4ca7d0). Only Use Map Settings games read them (no melee flag).
pub fn apply_dat_settings(dat: &mut crate::dat::Dat, unix: Option<&[u8]>, upgx: Option<&[u8]>, tecx: Option<&[u8]>) {
    let u16s = |b: &[u8], o: usize, i: usize| u16::from_le_bytes([b[o + i * 2], b[o + i * 2 + 1]]);
    if let Some(b) = unix.filter(|b| b.len() == 0x1048) {
        // use default [228], hp u32 [228], shields, armor u8, time, minerals, gas,
        // string id (u16 [228] each), weapon damage and bonus (u16 [130] each).
        let (hp, sh, ar, tm, mi, ga, st, dm, bo) = (0xe4, 0x474, 0x63c, 0x720, 0x8e8, 0xab0, 0xc78, 0xe40, 0xf44);
        let u = &mut dat.units;
        for i in (0..0xe4usize).rev() {
            if b[i] != 0 {
                continue;
            }
            u.hit_points[i] = u32::from_le_bytes(b[hp + i * 4..hp + i * 4 + 4].try_into().unwrap());
            u.shields[i] = u16s(b, sh, i);
            u.armor[i] = b[ar + i];
            u.build_time[i] = u16s(b, tm, i);
            u.mineral_cost[i] = u16s(b, mi, i);
            u.gas_cost[i] = u16s(b, ga, i);
            u.name_string[i] = u16s(b, st, i);
            // The weapons of the unit's turret when it has one.
            let s = u.subunit1[i];
            let w_of = if s != 0xe4 { s as usize } else { i };
            for w in [u.ground_weapon[w_of], u.air_weapon[w_of]] {
                if w != 0x82 {
                    dat.weapons.damage[w as usize] = u16s(b, dm, w as usize);
                    dat.weapons.damage_bonus[w as usize] = u16s(b, bo, w as usize);
                }
            }
        }
    }
    if let Some(b) = upgx.filter(|b| b.len() == 0x31a) {
        // use default [61] + pad, then minerals, mineral factor, gas, gas factor, time,
        // time factor (u16 [61] each).
        let g = &mut dat.upgrades;
        for i in (0..61usize).rev() {
            if b[i] == 0 {
                g.mineral_base[i] = u16s(b, 0x3e, i);
                g.mineral_factor[i] = u16s(b, 0xb8, i);
                g.gas_base[i] = u16s(b, 0x132, i);
                g.gas_factor[i] = u16s(b, 0x1ac, i);
                g.time_base[i] = u16s(b, 0x226, i);
                g.time_factor[i] = u16s(b, 0x2a0, i);
            }
        }
    }
    if let Some(b) = tecx.filter(|b| b.len() == 0x18c) {
        // use default [44], minerals, gas, time, energy (u16 [44] each).
        let t = &mut dat.techdata;
        for i in (0..44usize).rev() {
            if b[i] == 0 {
                t.mineral_cost[i] = u16s(b, 0x2c, i);
                t.gas_cost[i] = u16s(b, 0x84, i);
                t.research_time[i] = u16s(b, 0xdc, i);
                t.energy_cost[i] = u16s(b, 0x134, i);
            }
        }
    }
}

impl State {
    /// 0x4caf90: MASK, the starting fog: each tile's flags get the mask byte as both the
    /// visibility and the exploration bits (low 16 bits replaced).
    pub fn chk_mask(&mut self, data: &[u8]) {
        for (f, &m) in self.terrain.tile_flags.iter_mut().zip(data) {
            *f = (*f & 0xffff_0000) | (m as u32) << 8 | m as u32;
        }
    }

    /// 0x4ca600: PUNI, which units each player may build.
    pub fn chk_puni(&mut self, b: &[u8]) {
        if b.len() != 0x1644 {
            return;
        }
        // per player [12][228], global [228], use global [12][228]
        for p in (0..12usize).rev() {
            for t in (0..228usize).rev() {
                let v = if b[0xb94 + p * 228 + t] == 0 { b[p * 228 + t] } else { b[0xab0 + t] };
                self.players.unit_available[p][t] = v;
            }
        }
    }

    /// 0x4cbac0: PUPx, upgrade levels and limits per player.
    pub fn chk_pupx(&mut self, b: &[u8]) {
        if b.len() != 0x90e {
            return;
        }
        // max [12][61], start [12][61], default max [61], default start [61], use default [12][61]
        for p in (0..12usize).rev() {
            for u in (0..61usize).rev() {
                let (level, max) = if b[0x5b8 + 0x7a + p * 61 + u] == 0 {
                    (b[0x2dc + p * 61 + u], b[p * 61 + u])
                } else {
                    (b[0x5b8 + 61 + u], b[0x5b8 + u])
                };
                if u < 46 {
                    self.players.upgrade_sc[p][u] = level;
                } else if self.is_expansion {
                    self.players.upgrade_bw[p][u - 46] = level;
                }
                self.players.upgrade_max[p][u] = max;
            }
        }
    }

    /// 0x4cb7d0: PTEx, techs available and researched per player.
    pub fn chk_ptex(&mut self, b: &[u8]) {
        if b.len() != 0x688 {
            return;
        }
        // available [12][44], researched [12][44], default available [44], default
        // researched [44], use default [12][44]
        for p in (0..12usize).rev() {
            for t in (0..44usize).rev() {
                let (avail, done) = if b[0x478 + p * 44 + t] == 0 {
                    (b[p * 44 + t], b[0x210 + p * 44 + t])
                } else {
                    (b[0x420 + t], b[0x44c + t])
                };
                self.players.tech_available[p][t] = avail;
                if t < 0x18 || self.is_expansion {
                    self.players.tech_researched[p][t] = done;
                }
            }
        }
    }

    /// 0x4cb250: UPRP, the unit property slots of CreateUnitWithProperties (0x596cd8).
    pub fn chk_uprp(&mut self, b: &[u8]) {
        if b.len() == 0x500 {
            self.trig.unit_properties = b.chunks_exact(0x14).map(|c| c.try_into().unwrap()).collect();
        }
    }
}

impl State {
    /// 0x4653d0: how many interceptors / scarabs `u`'s hangar holds (0 for others).
    pub(super) fn hangar_capacity(&self, d: &Data, u: u32) -> u8 {
        let un = &self.units[u as usize];
        let t = un.unit_type;
        let hero = d.dat.units.flags[t as usize] & 0x40 != 0;
        let up = |i: usize| self.players.upgrade_sc[un.player as usize][i] != 0;
        match t {
            0x48 | 0x52 => if hero { 8 } else if up(0x2b) { 8 } else { 4 },
            0x53 | 0x51 => if hero { 10 } else if up(0x24) { 10 } else { 5 },
            _ => 0,
        }
    }

    /// 0x4cd260: `u` starts with up to `n` interceptors / scarabs docked.
    pub(super) fn fill_hangar(&mut self, d: &Data, u: u32, n: u8) {
        let ht = match self.units[u as usize].unit_type {
            0x48 | 0x52 => 0x49,
            0x53 | 0x51 => 0x55,
            _ => return,
        };
        let n = n.min(self.hangar_capacity(d, u));
        for _ in 0..n {
            let (x, y) = self.unit_sprite_pos(u);
            let pl = self.units[u as usize].player;
            let Some(c) = self.create_unit(d, ht, x, y, pl) else { continue };
            self.finish_unit_pre(d, c);
            self.finish_unit_post(d, c);
            self.units[c as usize].set_ptr_at(0xc0, u);
            // 0x466300 inline
            self.dock_fighter(d, u, c);
        }
    }
}
