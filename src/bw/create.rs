//! Unit creation: create_unit (1.16.1 0x4a09d0) and the initialization chain.

use super::list::{self, NIL};
use super::unit::status;
use super::{Data, State};
use crate::game::flingy::mflags;

/// 0x6a4: unit limit checked by 0x4a06c0.
pub const UNIT_LIMIT: u32 = 0x6a4;

impl Data<'_> {
    /// 0x66a210 non-null: the image has a damage overlay LO file.
    pub fn has_damage_overlays(&self, image_id: u16) -> bool {
        matches!(self.lo.get(1).and_then(|k| k.get(image_id as usize)), Some(Some(_)))
    }

    /// 0x4d5c20 (table 0x5240d0): 2 * number of used damage-overlay slots in frame 0 of the
    /// image's damage LO file (slots of (0x7f, 0x7f) are unused).
    pub fn damage_overlay_count(&self, image_id: u16) -> u8 {
        let Some(Some(lo)) = self.lo.get(1).and_then(|k| k.get(image_id as usize)) else {
            return 0;
        };
        let Some(off) = lo.get(8..12).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else {
            return 0;
        };
        let mut n = 0u8;
        for i in (0..0x2c).step_by(2).rev() {
            let a = lo.get(off + i).copied().unwrap_or(0x7f);
            let b = lo.get(off + i + 1).copied().unwrap_or(0x7f);
            if a != 0x7f || b != 0x7f {
                n += 1;
            }
        }
        n << 1
    }
}

fn dims(d: &Data, ut: u16) -> (i32, i32, i32, i32) {
    let dim = d.dat.units.dimensions[ut as usize];
    (dim as i16 as i32, (dim >> 16) as i16 as i32, (dim >> 32) as i16 as i32, (dim >> 48) as i16 as i32)
}

impl State {
    /// 0x4a0990: undo a unit whose creation failed: its subunit too, then out of the
    /// game and its sprite freed. The unit itself is not returned to the free list.
    fn destroy_partial_unit(&mut self, d: &Data, u: u32) {
        let sub = self.units[u as usize].subunit;
        if sub != NIL && d.dat.units.flags[self.units[sub as usize].unit_type as usize] & 0x10 != 0 {
            self.destroy_partial_unit(d, sub);
            self.units[u as usize].subunit = NIL;
        }
        self.remove_unit(d, u);
        self.free_unit_sprite(u);
    }

    /// 0x4a09d0 create_unit (ECX = type, EAX = x, stack: y, player).
    pub fn create_unit(&mut self, d: &Data, unit_type: u16, x: i16, y: i16, player: u8) -> Option<u32> {
        // Drawn and discarded (site 14).
        let _ = self.rng.rand(14);
        let u = self.alloc_unit(d, unit_type, x, y, player)?;
        if std::env::var_os("SCBW_UNITLOG").is_some() {
            eprintln!("f{} create u{u} type {unit_type:#x} at {x},{y} p{player}", self.frame);
        }
        if self.units[u as usize].status & status::GROUNDED_BUILDING != 0 {
            let mut l = self.visible_units;
            list::insert_after_head(&mut self.units, u, &mut l);
            self.visible_units = l;
        } else {
            let mut l = self.hidden_units;
            list::insert_after_head(&mut self.units, u, &mut l);
            self.hidden_units = l;
        }
        let sub_type = d.dat.units.subunit1.get(unit_type as usize).copied().unwrap_or(0xe4);
        if unit_type > 0x69 || sub_type == 0xe4 {
            self.units[u as usize].subunit = NIL;
            return Some(u);
        }
        let Some(sub) = self.alloc_unit(d, sub_type, x, y, player) else {
            self.destroy_partial_unit(d, u);
            return None;
        };
        self.units[u as usize].subunit = sub;
        self.units[sub as usize].subunit = u;
        // 0x401df0(2) on the parent's main image, then 0x401e40 on the subunit's main image.
        let ps = self.units[u as usize].sprite;
        let (ox, oy) = self.image_lo_offset(d, ps, 2, 0);
        let ss = self.units[sub as usize].sprite;
        self.sprite_set_main_offset(ss, ox, oy);
        Some(u)
    }

    /// 0x4a06c0: take a unit from the free list and initialize it.
    /// 0x49de00: unlink from the free units (0x628438 / 0x62843c).
    fn alloc_unit(&mut self, d: &Data, ut: u16, x: i16, y: i16, player: u8) -> Option<u32> {
        if self.unit_count == UNIT_LIMIT {
            self.last_error = 0x3d;
            return None;
        }
        if !self.unit_fits_in_map(d, ut, x as i32, y as i32) {
            self.last_error = 0;
            return None;
        }
        let u = self.free_units.head;
        if u != NIL && self.initialize_unit(d, u, ut, x, y, player) {
            self.unit_count += 1;
            let mut l = self.free_units;
            list::remove(&mut self.units, u, &mut l);
            self.free_units = l;
            return Some(u);
        }
        self.last_error = 0x3e;
        None
    }

    /// 0x49df10: the unit's box at (x, y) lies inside the map.
    pub(super) fn unit_fits_in_map(&self, d: &Data, ut: u16, x: i32, y: i32) -> bool {
        let (l, t, r, b) = dims(d, ut);
        x - l >= 0 && r + x < self.map_px.0 as i32 && y - t >= 0 && b + y < self.map_px.1 as i32
    }

    /// 0x4a0320 initialize_unit.
    fn initialize_unit(&mut self, d: &Data, u: u32, ut: u16, x: i16, y: i16, player: u8) -> bool {
        {
            let un = &mut self.units[u as usize];
            un.union_c0[0x10..0x1c].fill(0);
            un.rally_pos = (0, 0);
            un.rally_unit = NIL;
            un.prev_burrowed = NIL;
            un.next_burrowed = NIL;
            // 0x74..0x8f
            un.order_queue_head = NIL;
            un.order_queue_tail = NIL;
            un.auto_target = NIL;
            un.connected_unit = NIL;
            un.order_queue_count = 0;
            un.order_process_timer = 0;
            un.unknown_86 = 0;
            un.attack_notify_timer = 0;
            un.previous_unit_type = 0;
            un.last_event_timer = 0;
            un.last_event_color = 0;
            un.rank_increase = 0;
            un.unknown_8e = 0;
            un.kill_count = 0;
            // 0x110..0x12f
            un.remove_timer = 0;
            un.defense_matrix_damage = 0;
            un.defense_matrix_timer = 0;
            un.stim_timer = 0;
            un.ensnare_timer = 0;
            un.lockdown_timer = 0;
            un.irradiate_timer = 0;
            un.stasis_timer = 0;
            un.plague_timer = 0;
            un.storm_timer = 0;
            un.irradiated_by = NIL;
            un.irradiate_player = 0;
            un.parasite_flags = 0;
            un.cycle_counter = 0;
            un.is_blind = 0;
            un.maelstrom_timer = 0;
            un.acid_spore_count = 0;
            un.acid_spore_time = [0; 9];
            un.status = 0;
            un.order_flags = 0;
            un.pathing_flags = 0;
            un.previous_hp = 1;
            un.ai = NIL;
        }
        if !self.initialize_unit_base(d, u, ut, x, y, player) {
            return false;
        }
        let un = &mut self.units[u as usize];
        un.build_queue = [0xe4; 5];
        un.build_queue_slot = 0;
        un.unique_id = un.unique_id.wrapping_add(1) & 0x1f;
        let t = un.unit_type;
        let disabled = un.status & status::DOODAD_DISABLED != 0 || un.lockdown_timer != 0 || un.stasis_timer != 0 || un.maelstrom_timer != 0;
        let building = d.dat.units.flags[t as usize] & 1 != 0;
        if matches!(t, 0x6a | 0x6f | 0x71 | 0x72 | 0x82 | 0x83 | 0x84 | 0x85 | 0x9a | 0xa0 | 0xa7 | 0x9b) {
            if !disabled || building {
                un.button_set = 0xe8;
            }
        } else if !disabled || building {
            un.button_set = 0xe7;
        }
        let r = self.rng.rand(15) as u8;
        let un = &mut self.units[u as usize];
        un.wireframe_seed = r;
        un.hp = if d.dat.units.flags[t as usize] & 0x10 == 0 { (d.dat.units.hit_points[ut as usize] / 10) as i32 } else { 1 };
        un.order = if un.status & status::GROUNDED_BUILDING == 0 { 0x17 } else { d.dat.units.ai_human_idle[ut as usize] };
        un.order_target_unit = NIL;
        un.order_target_pos = (0, 0);
        un.order_state = 0;
        un.order_unit_type = 0xe4;
        if un.secondary_order != 0x17 {
            un.secondary_order = 0x17;
            un.secondary_order_pos = (0, 0);
            un.current_build_unit = NIL;
            un.secondary_order_state = 0;
        }
        un.finder_index = [-1; 4];
        // Player unit list 0x6283f8 (prev +0x68, next +0x6c).
        un.player_prev = NIL;
        let head = self.player_units[player as usize];
        self.units[u as usize].player_next = head;
        if head != NIL {
            self.units[head as usize].player_prev = u;
        }
        self.player_units[player as usize] = u;
        self.modify_unit_counters(d, u, 1);
        let s = self.units[u as usize].sprite;
        if self.units[u as usize].status & status::GROUNDED_BUILDING == 0 {
            if matches!(self.units[u as usize].unit_type, 2 | 0x13) {
                self.units[u as usize].set_u8_at(0xc0, 3); // spider mines
            }
            self.sprites[s as usize].flags |= super::sprite::sflag::HIDDEN;
            self.sprite_set_visibility(s, 0);
        } else {
            self.finder_insert(d, u);
            if self.units[u as usize].status & status::GROUNDED_BUILDING != 0 {
                let p = (self.sprites[s as usize].x, self.sprites[s as usize].y);
                self.mark_building_tiles(d, u, p, true);
            }
            self.update_unit_overlaps(d, u);
            if self.units[u as usize].status & status::IN_AIR != 0 {
                self.repulse_init(d, u);
            }
            self.reset_unit_image(d, u, true);
            self.sprite_set_visibility(s, 0);
        }
        self.units[u as usize].visibility = 0xffff_ffff;
        let t = self.units[u as usize].unit_type as usize;
        if d.dat.units.flags[t] & 0x10 == 0 {
            if self.sprites[s as usize].flags & super::sprite::sflag::HIDDEN == 0 {
                self.unit_reveal(d, u);
            }
            return true;
        }
        self.sprites[s as usize].flags |= 0x10;
        true
    }

    /// 0x49ecf0: flingy, sprite, hp/shields/energy, status from units.dat.
    pub(super) fn initialize_unit_base(&mut self, d: &Data, u: u32, ut: u16, x: i16, y: i16, player: u8) -> bool {
        let saved = self.iscript_unit;
        self.iscript_unit = Some(u);
        let fid = d.dat.units.flingy[ut as usize] as u16;
        if !self.initialize_unit_flingy(d, u, fid, x, y, player, 0) {
            self.iscript_unit = saved;
            return false;
        }
        {
            let un = &mut self.units[u as usize];
            un.player = player;
            un.order = 0xbc;
            un.order_state = 0;
            un.order_signal = 0;
            un.order_unit_type = 0xe4;
            un.main_order_timer = 0;
            un.ground_cooldown = 0;
            un.air_cooldown = 0;
            un.spell_cooldown = 0;
            un.order_target_unit = NIL;
            un.order_target_pos = (0, 0);
            un.union_c0[..0x10].fill(0);
            un.unit_type = ut;
            un.resource_type = 0;
            un.ai_action_flag = 0;
        }
        let s = self.units[u as usize].sprite;
        if s != NIL {
            self.update_unit_sprite(d, s);
            if self.sprites[s as usize].images.head == NIL {
                self.units[u as usize].sprite = NIL;
            }
        }
        self.iscript_unit = saved;
        let t = self.units[u as usize].unit_type as usize;
        let un = &mut self.units[u as usize];
        un.last_attacking_player = 8;
        un.shields = (d.dat.units.shields[t] as i32) << 8;
        let e = if ut == 0xac { 0x6400 } else { (self.max_energy(d, u) / 4) as u16 };
        self.units[u as usize].energy = e;
        let un = &mut self.units[u as usize];
        un.user_action_flags = 0;
        let s = un.sprite;
        let elev = d.dat.units.elevation[ut as usize];
        self.sprites[s as usize].elevation = elev;
        let f = d.dat.units.flags[ut as usize];
        let ft = d.dat.units.flags[t];
        let un = &mut self.units[u as usize];
        let mut st = un.status;
        st = (st & !2) | ((f << 1) & 2);
        st = (st & !4) | (f & 4);
        st = (st & !0x10000) | ((f >> 12) & 0x10000);
        st = (st & !0x20000) | ((ft >> 10) & 0x20000);
        st = (st & !0x100000) | (!((st >> 2) << 20) & 0x100000);
        un.status = st;
        un.pathing_flags = (un.pathing_flags & !1) | (elev < 0xc) as u8;
        let t16 = un.unit_type;
        if (0xc2..200).contains(&t16) || t16 == 0xcc {
            un.status |= status::NO_COLLIDE;
        }
        if ft & 1 != 0 {
            un.set_u8_at(0xc8, 0x2c);
            un.set_u8_at(0xc9, 0x3d);
        }
        un.path = NIL;
        un.movement_state = 0;
        un.recent_order_timer = 0;
        if f & 0x2000_0000 != 0 {
            un.status |= status::INVINCIBLE;
        } else if ft & 0x2000_0000 == 0 {
            un.status &= !status::INVINCIBLE;
        }
        let mi = self.sprites[s as usize].main_image;
        let img = self.images[mi as usize].image_id;
        let un = &mut self.units[u as usize];
        un.building_overlay_state = d.damage_overlay_count(img);
        let bt = d.dat.units.build_time[t];
        un.remaining_build_time = if bt == 0 { 1 } else { bt };
        self.set_hp_gain(d, u);
        self.set_shield_gain(d, u);
        self.apply_speed_upgrades(d, u);
        self.update_speed(d, u);
        true
    }

    /// 0x496360 for a unit: flingy fields and the sprite.
    fn initialize_unit_flingy(&mut self, d: &Data, u: u32, fid: u16, x: i16, y: i16, player: u8, dir: u8) -> bool {
        let fl = &d.dat.flingy;
        let i = fid as usize;
        let un = &mut self.units[u as usize];
        let f = &mut un.fl;
        f.flingy_id = fid;
        f.flags = 0;
        f.next_speed = 0;
        f.top_speed = fl.top_speed[i] as i32;
        f.acceleration = fl.acceleration[i];
        f.turn_rate = fl.turn_rate[i];
        f.movement_type = fl.movement_control[i];
        f.pos = (x, y);
        f.exact = ((x as i32) << 8, (y as i32) << 8);
        if f.move_target != (x, y) {
            f.move_target = (x, y);
            un.move_target_unit = NIL;
            f.flags = mflags::TURNING;
            f.next_waypoint = f.move_target;
        }
        if f.next_target_waypoint != (x, y) {
            f.next_target_waypoint = (x, y);
        }
        f.heading = dir;
        f.velocity_dir_prev = dir;
        un.hp = 1;
        let s = self.create_sprite(d, fl.sprite[i], x, y, player);
        self.units[u as usize].sprite = s.unwrap_or(NIL);
        let Some(s) = s else {
            return false;
        };
        let h = self.units[u as usize].fl.heading;
        self.sprite_images_set_direction(d, s, h);
        true
    }

    /// 0x491870: max energy (fp8): 250 with the unit's energy upgrade, else 200.
    pub fn max_energy(&self, d: &Data, u: u32) -> u32 {
        let un = &self.units[u as usize];
        if d.dat.units.flags[un.unit_type as usize] & 0x40 != 0 {
            return 64000;
        }
        let up = match un.unit_type {
            1 => 21,
            8 => 22,
            9 => 19,
            0xc => 23,
            0x22 => 51,
            0x2d => 31,
            0x2e => 32,
            0x3c => 47,
            0x3f => 49,
            0x43 => 40,
            0x47 => 44,
            _ => return 0xc800,
        };
        if self.players.upgrade_level(un.player, up) != 0 {
            64000
        } else {
            0xc800
        }
    }

    /// 0x47b180: hp regained per frame while building.
    fn set_hp_gain(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type as usize;
        let bt = d.dat.units.build_time[t] as u32;
        let un = &mut self.units[u as usize];
        if bt != 0 {
            let hp = d.dat.units.hit_points[t];
            let g = ((hp - hp / 10).wrapping_sub(1).wrapping_add(bt) / bt) as u16;
            un.hp_gain = g;
            if g != 0 {
                return;
            }
        }
        un.hp_gain = 1;
    }

    /// 0x47b6a0: shields regained per frame while building (buildings only).
    fn set_shield_gain(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type as usize;
        if d.dat.units.shield_enable[t] == 0 || self.units[u as usize].status & status::GROUNDED_BUILDING == 0 {
            return;
        }
        let max = (d.dat.units.shields[t] as u32) * 0x100;
        let start = max / 10;
        let un = &mut self.units[u as usize];
        un.shields = start as i32;
        let bt = d.dat.units.build_time[t] as u32;
        if bt != 0 {
            let g = ((max - start) / bt) as u16;
            un.shield_gain = g;
            if g != 0 {
                return;
            }
        }
        un.shield_gain = 1;
    }

    /// 0x454370: speed/cooldown upgrade status bits from the owner's upgrades.
    pub fn apply_speed_upgrades(&mut self, d: &Data, u: u32) {
        let (t, p) = (self.units[u as usize].unit_type, self.units[u as usize].player);
        let up = |id: usize| self.players.upgrade_level(p, id) != 0;
        let has = match t {
            2 | 0x13 => up(17),
            0x25 => {
                if up(27) {
                    self.units[u as usize].status |= status::SPEED_UPGRADE;
                    self.update_speed(d, u);
                }
                if self.players.upgrade_level(p, 28) != 0 {
                    self.units[u as usize].status |= status::COOLDOWN_UPGRADE;
                }
                self.update_speed(d, u);
                return;
            }
            0x26 => up(29),
            0x27 => up(53),
            0x2a => up(26),
            0x30 => self.is_expansion,
            0x35 | 0x39 | 0x4d | 0x50 | 0x58 | 0x67 => true,
            0x36 => {
                self.units[u as usize].status |= status::SPEED_UPGRADE;
                self.update_speed(d, u);
                self.units[u as usize].status |= status::COOLDOWN_UPGRADE;
                self.update_speed(d, u);
                return;
            }
            0x41 => up(34),
            0x45 => up(37),
            0x46 => up(42),
            0x54 => up(39),
            _ => return,
        };
        if has {
            self.units[u as usize].status |= status::SPEED_UPGRADE;
            self.update_speed(d, u);
        }
        self.update_speed(d, u);
    }

    /// Speed modifier count used by 0x47b5f0/0x47b8a0/0x47b850/0x42d8c0: stim + speed upgrade - ensnare.
    fn speed_mod(&self, u: u32) -> i32 {
        let un = &self.units[u as usize];
        (un.stim_timer != 0) as i32 + (un.status & status::SPEED_UPGRADE != 0) as i32 - (un.ensnare_timer != 0) as i32
    }

    /// 0x454310: recompute top speed, acceleration and turn rate.
    pub fn update_speed(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type as usize;
        let fid = d.dat.units.flingy[t] as usize;
        let mt = d.dat.flingy.movement_control[fid];
        if mt != 0 && mt != 1 {
            self.iscript_speed(d, u);
            return;
        }
        let top = self.adjust_top_speed(u, d.dat.flingy.top_speed[fid]);
        let acc = self.unit_acceleration(d, u);
        let turn = self.unit_turn_rate(d, u);
        let f = &mut self.units[u as usize].fl;
        f.top_speed = top as i32;
        f.acceleration = acc as u16;
        f.turn_rate = turn as u8;
    }

    /// 0x47b8f0: `u`'s flingy top speed with its speed modifiers (0x47b5f0).
    pub fn unit_top_speed(&self, d: &Data, u: u32) -> u32 {
        let fid = d.dat.units.flingy[self.units[u as usize].unit_type as usize] as usize;
        self.adjust_top_speed(u, d.dat.flingy.top_speed[fid])
    }

    /// 0x47b5f0: `base` top speed with the unit's speed modifiers.
    pub fn adjust_top_speed(&self, u: u32, base: u32) -> u32 {
        let m = self.speed_mod(u);
        if m > 0 {
            if matches!(self.units[u as usize].unit_type, 0x46 | 0x50 | 0x58) {
                0x6ab
            } else {
                let v = base.wrapping_add(base >> 1);
                if v < 0x355 { 0x355 } else { v }
            }
        } else if m < 0 {
            base >> 1
        } else {
            base
        }
    }

    /// 0x47b8a0: flingy acceleration with speed modifiers.
    pub fn unit_acceleration(&self, d: &Data, u: u32) -> u32 {
        let fid = d.dat.units.flingy[self.units[u as usize].unit_type as usize] as usize;
        let a = d.dat.flingy.acceleration[fid] as u32;
        let m = self.speed_mod(u);
        if m > 0 { a * 2 } else if m < 0 { a - (a >> 2) } else { a }
    }

    /// 0x47b850: flingy turn rate with speed modifiers.
    pub fn unit_turn_rate(&self, d: &Data, u: u32) -> u32 {
        let fid = d.dat.units.flingy[self.units[u as usize].unit_type as usize] as usize;
        let tr = d.dat.flingy.turn_rate[fid] as u32;
        let m = self.speed_mod(u);
        if m > 0 { tr * 2 } else if m < 0 { tr - (tr >> 2) } else { tr }
    }

    /// 0x42d8c0: iscript-driven units: average `move` distance of 32 dry runs of Walking.
    fn iscript_speed(&mut self, d: &Data, u: u32) {
        if self.units[u as usize].fl.movement_type != 2 {
            return;
        }
        let s = self.units[u as usize].sprite;
        let mi = self.sprites[s as usize].main_image;
        let saved = self.images[mi as usize].clone();
        // Start anim 0xb (Walking) on a copy of the iscript state.
        if !self.images[mi as usize].iscript.start_anim(d, 0xb) {
            self.images[mi as usize] = saved;
            return;
        }
        let mut total = 0i32;
        for _ in 0..0x20 {
            let mut dist = 0u32;
            let saved_unit = self.iscript_unit;
            self.iscript_unit = Some(u);
            super::iscript::run(self, d, mi, true, Some(&mut dist));
            self.iscript_unit = saved_unit;
            let m = self.speed_mod(u);
            if m > 0 {
                dist *= 2;
            } else if m < 0 {
                dist -= dist >> 2;
            }
            total = total.wrapping_add(dist as i32);
        }
        self.images[mi as usize] = saved;
        self.units[u as usize].fl.top_speed = (total + ((total >> 31) & 0x1f)) >> 5;
    }

    /// 0x488bf0: adjust unit counts, supply and category counters by `delta`.
    pub fn modify_unit_counters(&mut self, d: &Data, u: u32, delta: i32) {
        let un = &self.units[u as usize];
        if un.status & status::HALLUCINATION != 0 {
            return;
        }
        let t = un.unit_type as usize;
        if d.dat.units.flags[t] & 0x10 != 0 {
            return;
        }
        let p = un.player as usize;
        let pl = &mut self.players;
        pl.unit_counts[t][p] += delta;
        let g = d.dat.units.staredit_group_flags[t];
        let supply = d.dat.units.supply_required[t] as i32;
        if g & 1 != 0 {
            let s = if matches!(t, 0x24 | 0x3b | 0x61) {
                // Eggs / cocoons: count the unit being morphed into.
                let inner = un.build_queue[un.build_queue_slot as usize] as usize;
                let s = d.dat.units.supply_required[inner] as i32;
                if d.dat.units.flags[inner] & 0x400 != 0 { s << 1 } else { s }
            } else if d.dat.units.flags[t] & 0x400 != 0 && un.status & status::COMPLETED == 0 {
                supply << 1
            } else {
                supply
            };
            pl.supply_used[0][p] += s * delta;
        } else if g & 2 != 0 {
            pl.supply_used[1][p] += supply * delta;
        } else if g & 4 != 0 {
            pl.supply_used[2][p] += supply * delta;
        }
        if g & 0x20 != 0 {
            pl.factories[p] += delta;
        }
        if g & 8 != 0 || (g & 0x10 == 0 && matches!(t, 0x24 | 0x3b | 0x61)) {
            pl.men[p] += delta;
        } else if g & 0x10 != 0 {
            pl.buildings[p] += delta;
        }
        if pl.unit_counts[t][p] < 0 {
            pl.unit_counts[t][p] = 0;
        }
    }

    /// 0x401170: unit `u` belongs to the local player (never during a replay).
    pub fn is_local_unit(&self, u: u32) -> bool {
        !self.replay && self.units[u as usize].player == self.local_player
    }

    /// 0x48ed50 (EBX = sound `id`, ESI = unit `u`, stack: enable 1, minimum volume `min_vol`):
    /// the sim's part of the gate. Id 0 plays nothing; a unit that is not the local
    /// player's (always in a replay) must be visible to the local vision mask (0x57f0b0, in
    /// a replay 0x6d0f18). The digital volume check, distance volume and pan, the sfxdata
    /// minimum and 0x4bc270 are the front end's (view.rs); this never changes sim state.
    pub fn unit_sound(&mut self, id: u16, u: u32, min_vol: i32) {
        if u == NIL {
            // No unit: full volume, no pan (the view).
            if id != 0 {
                self.unit_sounds.push((id, NIL, min_vol, 0, 0));
            }
            return;
        }
        let s = self.units[u as usize].sprite;
        if id == 0 || s == NIL {
            return;
        }
        let sp = &self.sprites[s as usize];
        if (self.replay || !self.is_local_unit(u)) && sp.visibility & self.local_vision_mask == 0 {
            return;
        }
        self.unit_sounds.push((id, u, min_vol, sp.x, sp.y));
    }

    /// 0x48d140 (with the sprintf 0x41f1b0 of network.tbl line 0x11 and the name 0x47b090 done
    /// by the front end): the line "WARNING: Unit unplaceable! (name) (x,y)" in color 3 for 7 s
    /// and sound 2.
    pub fn unit_unplaceable(&mut self, t: u16, x: i16, y: i16) {
        self.unplaced.push((t, x, y));
    }

    /// 0x48ee30: an error for `player`: advisor sound `sound` (sfxdata id) and stat_txt
    /// line `msg`, both for the local player only.
    pub fn local_error(&mut self, player: u8, sound: u16, msg: u16) {
        if player != self.local_player || self.replay {
            return;
        }
        if sound != 0 {
            self.ui_sounds.push(sound);
        }
        if msg != 0 {
            self.shown_error = msg;
        }
    }

    /// 0x48cf00 (and 0x48ccb0 behind the same check): stat_txt line `msg` (1-based) on
    /// the error line when `player` is the local player and the command running is theirs
    /// (0x51267c == 0x512688: never while a replay plays).
    pub fn local_error_line(&mut self, player: u8, msg: u16) {
        if player == self.local_player && !self.replay {
            self.shown_error = msg;
        }
    }

    /// Minerals then gas against `m` / `g`; short, `player` gets 0x48ee30's "Not enough
    /// minerals." (0x352, advisor 0x93 + local race) or "...gas." (0x353, 0x96 + race).
    pub fn afford_or_error(&mut self, player: u8, m: i32, g: i32) -> bool {
        let race = self.player_races[self.local_player as usize] as u16;
        if self.players.minerals[player as usize] < m {
            self.local_error(player, 0x93 + race, 0x352);
            return false;
        }
        if self.players.gas[player as usize] < g {
            self.local_error(player, 0x96 + race, 0x353);
            return false;
        }
        true
    }

    /// 0x48ef30: `player`'s refused action: stat_txt line `msg` (1-based, 0 for none)
    /// and the race error beep (sfx 1 + race of unit `u`, or the local race for NIL),
    /// for the local player outside a replay.
    // ponytail: the beep's volume gate (0x68c449) is not ported (as in 0x48f230).
    pub fn race_error(&mut self, d: &Data, player: u8, u: u32, msg: u16) {
        if player != self.local_player || self.replay {
            return;
        }
        if msg != 0 {
            self.shown_error = msg;
        }
        let race = if u == NIL { self.player_races[player as usize] as u16 } else { Self::supply_race_of(d, self.units[u as usize].unit_type) };
        self.ui_sounds.push(race + 1);
    }

    /// 0x48f6c0: unit `u`'s refused placement: its race's advisor line (0xa2 + race)
    /// and stat_txt line `msg` for its owner, the local player.
    pub fn advisor_error(&mut self, d: &Data, u: u32, msg: u16) {
        let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
        self.local_error(self.units[u as usize].player, 0, msg);
        if self.is_local_unit(u) {
            self.speech.push((0xa2 + race, 0, u, 0x63));
            self.unit_talk(u, 0xa2 + race);
        }
    }

    /// 0x48f5a0: the local player's unit `u` cannot land / build here: advisor line
    /// (0x90 + race) and the unit speaks.
    pub fn land_blocked_sound(&mut self, d: &Data, u: u32) {
        let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
        if self.is_local_unit(u) {
            self.speech.push((0x90 + race, 0, u, 0x63));
            self.unit_talk(u, 0x90 + race);
        }
    }

    /// 0x48f630: the local player's unit `u` cannot land here: advisor line (0x8d + race)
    /// and the unit speaks (0x48f5a0 with base 0x90 is the build twin).
    pub fn land_refused_sound(&mut self, d: &Data, u: u32) {
        let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
        if self.is_local_unit(u) {
            self.speech.push((0x8d + race, 0, u, 0x63));
            self.unit_talk(u, 0x8d + race);
        }
    }

    /// 0x48f7b0: the local player's unit `u` cannot be placed: advisor line (0x9f + race),
    /// the unit speaks, then stat_txt line `msg` (1-based) on the error line.
    pub fn placement_blocked_sound(&mut self, d: &Data, u: u32, msg: u16) {
        let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
        let local = self.is_local_unit(u);
        if local {
            self.speech.push((0x9f + race, 0, u, 0x63));
            self.unit_talk(u, 0x9f + race);
        }
        if local && msg != 0 {
            self.shown_error = msg;
        }
    }

    /// "Not enough energy." (stat_txt 0x360 + race of `u`, 0x4c36f0) on the error line
    /// for `u`'s owner (0x48ccb0 behind the 0x48cf00 check).
    pub fn energy_error(&mut self, d: &Data, u: u32) {
        let race = Self::supply_race_of(d, self.units[u as usize].unit_type);
        self.local_error_line(self.units[u as usize].player, 0x360 + race);
    }

    /// 0x45efe0's sim side: unit `u` speaks sound `sfx`; it blinks on the minimap (+0x8a /
    /// +0x8b) and the UI (`talked`) shows its talking portrait for the sound's length.
    /// 0x4a34c0: the ping at the unit's sprite (type 0x11) is queued for the minimap.
    fn unit_talk(&mut self, u: u32, sfx: u16) {
        self.talked.push((u, sfx));
        let un = &mut self.units[u as usize];
        un.last_event_timer = 8;
        un.last_event_color = 0x11;
        let sp = &self.sprites[un.sprite as usize];
        self.minimap_pings.push((sp.x as i32, sp.y as i32, 0x11));
    }

    /// 0x48f310: the local player's construction or morph is complete: zerg 0x87,
    /// terran 0x7e (0x88 when `u` is the SCV that built it), protoss silent.
    pub fn construction_sound(&mut self, d: &Data, u: u32) {
        if !self.is_local_unit(u) {
            return;
        }
        let t = self.units[u as usize].unit_type;
        let sfx = match Self::supply_race_of(d, t) {
            0 => 0x87,
            1 if t == 7 => 0x88,
            1 => 0x7e,
            _ => 0,
        };
        if sfx != 0 {
            self.speech.push((sfx, 1, u, 0x63));
        }
        self.unit_talk(u, sfx);
    }

    /// 0x48f440: the local player's unit `u` is ready: its units.dat ready sound.
    pub fn unit_ready_sound(&mut self, d: &Data, u: u32) {
        let t = self.units[u as usize].unit_type;
        if !self.is_local_unit(u) || t == 0x49 || t == 0x55 {
            return;
        }
        // units.dat has ready sounds for the first 106 types only.
        let s = d.dat.units.ready_sound.get(t as usize).copied().unwrap_or(0);
        if s != 0 {
            self.speech.push((s, 1, u, 0x63));
        }
        self.unit_talk(u, s);
    }

    /// 0x48f8a0: the local player's addon `u` came off its building (0x82).
    pub fn addon_detached_sound(&mut self, u: u32) {
        if !self.is_local_unit(u) {
            return;
        }
        self.speech.push((0x82, 4, u, 0x63));
        self.unit_talk(u, 0x82);
    }

    /// 0x48f230: the local player's unit `u` was hit: it blinks red on the minimap and,
    /// unless an alert played lately (0x640870) or nearby, or the unit is on screen
    /// (`unit_off_screen`), the advisor warns (building or forces sound by the local race).
    pub fn under_attack_alert(&mut self, u: u32) {
        if !self.is_local_unit(u) {
            return;
        }
        let red = self.interface_colors[0x14];
        let un = &mut self.units[u as usize];
        un.last_event_color = red;
        un.last_event_timer = 0x14;
        if self.alert_cooldown != 0 {
            return;
        }
        if !self.unit_off_screen(u) || self.units[u as usize].attack_notify_timer != 0 {
            return;
        }
        self.alert_nearby_units(u);
        self.alert_cooldown = 100;
        let race = self.player_races[self.local_player as usize] as u16;
        let building = self.units[u as usize].status & 2 != 0;
        self.speech.push((if building { 0x75 } else { 0x78 } + race, 1, NIL, 0));
    }

    /// 0x4e5ad0 (EAX = `u`): false when the unit's tile is on screen (with a margin of 2
    /// tiles left / up, 0x16 wide, 0xf high), else true.
    fn unit_off_screen(&self, u: u32) -> bool {
        let (x, y) = self.unit_sprite_pos(u);
        let (tx, ty) = (x as i32 / 32, y as i32 / 32);
        let (sx, sy) = (self.screen_tile.0 as i32, self.screen_tile.1 as i32);
        !(sx <= tx + 2 && sy <= ty + 2 && tx <= sx + 0x16 && ty <= sy + 0xf)
    }

    /// 0x48eae0 (EAX = `u`): every unit within 0x160 of `u`'s sprite (finder 0x4e8280, callback
    /// 0x48e9f0) is quiet for 100 ticks.
    fn alert_nearby_units(&mut self, u: u32) {
        let (x, y) = self.unit_sprite_pos(u);
        let b = [x.wrapping_sub(0x160), y.wrapping_sub(0x160), x.wrapping_add(0x160), y.wrapping_add(0x160)];
        let mut search = std::mem::replace(&mut self.search, super::collide::UnitSearch::empty());
        let start = search.units_in_finder_box(self, b);
        let found = search.results(start);
        search.pop();
        self.search = search;
        for n in found {
            self.units[n as usize].attack_notify_timer = 100;
        }
    }

    /// 0x48e980 (once per frame, front end): alert cooldowns, and every other frame the
    /// units' quiet timers.
    pub fn alert_tick(&mut self) {
        self.alert_cooldown = self.alert_cooldown.saturating_sub(1);
        let t = self.alert_tick_counter;
        self.alert_tick_counter = t.wrapping_sub(1);
        if t != 0 {
            return;
        }
        self.alert_tick_counter = 1;
        let mut u = self.visible_units.head;
        while u != NIL {
            let un = &mut self.units[u as usize];
            un.attack_notify_timer = un.attack_notify_timer.saturating_sub(1);
            u = un.link.next;
        }
    }

    /// 0x49e530: the last error (0x59cca0) goes to the screen when `player` is the
    /// local player (0x48ccb0, the front end's); either way it is cleared.
    pub fn show_last_error(&mut self, player: u8) {
        if self.last_error != 0 {
            if player == self.local_player {
                self.shown_error = self.last_error;
            }
            self.last_error = 0;
        }
    }

    pub(crate) fn repulse_eligible(&self, d: &Data, u: u32) -> bool {
        let t = self.units[u as usize].unit_type;
        self.units[u as usize].status & 0x20000 != 0 && d.dat.units.flags[t as usize] & 1 == 0 && t != 0x49
    }

    /// 0x4533d0: take a flyer out of its repulse grid cell.
    pub(crate) fn repulse_remove(&mut self, d: &Data, u: u32) {
        if !self.repulse_eligible(d, u) {
            return;
        }
        let un = &self.units[u as usize];
        let i = un.repulse_x as usize + un.repulse_y as usize * 0xab;
        if let Some(c) = self.repulse.get_mut(i) {
            *c = c.saturating_sub(1);
        }
    }

    /// 0x453300: register a flyer in the repulse grid (48px cells, 0xab per row).
    pub(crate) fn repulse_init(&mut self, d: &Data, u: u32) {
        if !self.repulse_eligible(d, u) {
            return;
        }
        if self.units[u as usize].repulse_unknown & 0xf0 == 0 {
            let r = self.rng.rand(0x25);
            let un = &mut self.units[u as usize];
            un.repulse_angle = r as u8;
            let hi = ((r >> 8) & 0xf0) as u8;
            un.repulse_unknown |= if hi == 0 { 0xf0 } else { hi };
        }
        let s = self.units[u as usize].sprite;
        let (x, y) = (self.sprites[s as usize].x as i32, self.sprites[s as usize].y as i32);
        let un = &mut self.units[u as usize];
        un.repulse_unknown &= 0xf8;
        un.repulse_x = (x / 0x30) as u8;
        un.repulse_y = (y / 0x30) as u8;
        let i = un.repulse_x as usize + un.repulse_y as usize * 0xab;
        if let Some(c) = self.repulse.get_mut(i) {
            if *c != 0xff {
                *c += 1;
            }
        }
    }

    /// 0x497480: set a sprite's visibility mask; images become redrawable when it turns visible locally.
    pub fn sprite_set_visibility(&mut self, s: u32, mask: u8) {
        let lm = self.local_vision_mask;
        let sp = &self.sprites[s as usize];
        if sp.visibility & lm != mask & lm && mask & lm != 0 {
            let mut i = sp.images.head;
            while i != NIL {
                self.images[i as usize].flags |= super::sprite::iflag::REDRAW;
                i = self.images[i as usize].link.next;
            }
        }
        self.sprites[s as usize].visibility = mask;
    }
}
