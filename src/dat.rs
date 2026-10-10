//! arr/*.dat tables. Layouts recovered from the load_dat descriptor tables in
//! StarCraft.exe (x64 1.23.10: load_dat = 0x140305d10); see re/datlayout.py.
//!
//! Every column is stored widened and padded to the table's full entry count;
//! partial columns (e.g. units infestation, ids 106..=201) are placed at their first id.

use crate::casc::Casc;
use std::io;

fn read_col(d: &[u8], pos: &mut usize, size: usize, count: usize, first: usize, total: usize) -> io::Result<Vec<u32>> {
    let bytes = d
        .get(*pos..*pos + size * count)
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "dat too short"))?;
    *pos += size * count;
    let mut v = vec![0u32; total.max(first + count)];
    for (i, c) in bytes.chunks_exact(size).enumerate() {
        v[first + i] = c.iter().rev().fold(0, |a, &b| (a << 8) | b as u32);
    }
    Ok(v)
}

/// `dat!(Name, "file", total; field: ty = (size, count, first_id), ...)`
macro_rules! dat {
    ($name:ident, $file:literal, $total:expr; $($field:ident : $ty:ty = ($size:expr, $count:expr, $first:expr)),* $(,)?) => {
        pub struct $name { $(pub $field: Vec<$ty>,)* }
        impl $name {
            pub const COUNT: usize = $total;
            pub fn parse(d: &[u8]) -> io::Result<Self> {
                let mut pos = 0;
                Ok($name { $($field: read_col(d, &mut pos, $size, $count, $first, $total)?.into_iter().map(|x| x as $ty).collect(),)* })
            }
            pub fn load(casc: &Casc) -> io::Result<Self> {
                Self::parse(&casc.read($file)?)
            }
        }
    };
}

dat!(Units, "arr/units.dat", 228;
    flingy: u8 = (1, 228, 0),
    subunit1: u16 = (2, 228, 0),
    subunit2: u16 = (2, 228, 0),
    infestation: u16 = (2, 96, 106),
    construction_image: u32 = (4, 228, 0),
    direction: u8 = (1, 228, 0),
    shield_enable: u8 = (1, 228, 0),
    shields: u16 = (2, 228, 0),
    hit_points: u32 = (4, 228, 0), // 24.8 fixed point
    elevation: u8 = (1, 228, 0),
    movement_flags: u8 = (1, 228, 0),
    rank: u8 = (1, 228, 0),
    ai_computer_idle: u8 = (1, 228, 0),
    ai_human_idle: u8 = (1, 228, 0),
    ai_return_to_idle: u8 = (1, 228, 0),
    ai_attack_unit: u8 = (1, 228, 0),
    ai_attack_move: u8 = (1, 228, 0),
    ground_weapon: u8 = (1, 228, 0),
    max_ground_hits: u8 = (1, 228, 0),
    air_weapon: u8 = (1, 228, 0),
    max_air_hits: u8 = (1, 228, 0),
    ai_internal: u8 = (1, 228, 0),
    flags: u32 = (4, 228, 0),
    target_acquisition_range: u8 = (1, 228, 0),
    sight_range: u8 = (1, 228, 0),
    armor_upgrade: u8 = (1, 228, 0),
    size: u8 = (1, 228, 0),
    armor: u8 = (1, 228, 0),
    right_click_action: u8 = (1, 228, 0),
    ready_sound: u16 = (2, 106, 0),
    what_sound_start: u16 = (2, 228, 0),
    what_sound_end: u16 = (2, 228, 0),
    piss_sound_start: u16 = (2, 106, 0),
    piss_sound_end: u16 = (2, 106, 0),
    yes_sound_start: u16 = (2, 106, 0),
    yes_sound_end: u16 = (2, 106, 0),
    placement_box: u32 = (4, 228, 0), // u16 w | u16 h << 16
    addon_position: u32 = (4, 96, 106), // u16 x | u16 y << 16
    dimensions: u64 = (8, 228, 0), // u16 left, up, right, down
    portrait: u16 = (2, 228, 0),
    mineral_cost: u16 = (2, 228, 0),
    gas_cost: u16 = (2, 228, 0),
    build_time: u16 = (2, 228, 0),
    requirements: u16 = (2, 228, 0),
    staredit_group_flags: u8 = (1, 228, 0),
    supply_provided: u8 = (1, 228, 0),
    supply_required: u8 = (1, 228, 0),
    space_required: u8 = (1, 228, 0),
    space_provided: u8 = (1, 228, 0),
    build_score: u16 = (2, 228, 0),
    destroy_score: u16 = (2, 228, 0),
    name_string: u16 = (2, 228, 0),
    broodwar: u8 = (1, 228, 0),
    staredit_availability: u16 = (2, 228, 0),
);

dat!(Weapons, "arr/weapons.dat", 130;
    label: u16 = (2, 130, 0),
    flingy: u32 = (4, 130, 0),
    unused: u8 = (1, 130, 0),
    target_flags: u16 = (2, 130, 0),
    min_range: u32 = (4, 130, 0),
    max_range: u32 = (4, 130, 0),
    damage_upgrade: u8 = (1, 130, 0),
    damage_type: u8 = (1, 130, 0),
    behavior: u8 = (1, 130, 0),
    remove_after: u8 = (1, 130, 0),
    explosion_type: u8 = (1, 130, 0),
    inner_splash: u16 = (2, 130, 0),
    medium_splash: u16 = (2, 130, 0),
    outer_splash: u16 = (2, 130, 0),
    damage: u16 = (2, 130, 0),
    damage_bonus: u16 = (2, 130, 0),
    cooldown: u8 = (1, 130, 0),
    damage_factor: u8 = (1, 130, 0),
    attack_angle: u8 = (1, 130, 0),
    launch_spin: u8 = (1, 130, 0),
    forward_offset: u8 = (1, 130, 0),
    upward_offset: u8 = (1, 130, 0),
    target_error_msg: u16 = (2, 130, 0),
    icon: u16 = (2, 130, 0),
);

dat!(Flingy, "arr/flingy.dat", 209;
    sprite: u16 = (2, 209, 0),
    top_speed: u32 = (4, 209, 0),
    acceleration: u16 = (2, 209, 0),
    halt_distance: u32 = (4, 209, 0),
    turn_rate: u8 = (1, 209, 0),
    unused: u8 = (1, 209, 0),
    movement_control: u8 = (1, 209, 0),
);

dat!(Sprites, "arr/sprites.dat", 517;
    image: u16 = (2, 517, 0),
    health_bar: u8 = (1, 387, 130),
    unknown: u8 = (1, 517, 0),
    visible: u8 = (1, 517, 0),
    selection_circle: u8 = (1, 387, 130),
    selection_offset: u8 = (1, 387, 130),
);

dat!(Images, "arr/images.dat", 999;
    grp: u32 = (4, 999, 0), // 1-based images.tbl index
    gfx_turns: u8 = (1, 999, 0),
    clickable: u8 = (1, 999, 0),
    full_iscript: u8 = (1, 999, 0),
    draw_if_cloaked: u8 = (1, 999, 0),
    draw_function: u8 = (1, 999, 0),
    remapping: u8 = (1, 999, 0),
    iscript: u32 = (4, 999, 0),
    shield_overlay: u32 = (4, 999, 0),
    attack_overlay: u32 = (4, 999, 0),
    damage_overlay: u32 = (4, 999, 0),
    special_overlay: u32 = (4, 999, 0),
    landing_dust: u32 = (4, 999, 0),
    liftoff_dust: u32 = (4, 999, 0),
);

dat!(Orders, "arr/orders.dat", 189;
    label: u16 = (2, 189, 0),
    use_weapon_targeting: u8 = (1, 189, 0),
    secondary: u8 = (1, 189, 0),
    non_subunit: u8 = (1, 189, 0),
    subunit_inherits: u8 = (1, 189, 0),
    subunit_can_use: u8 = (1, 189, 0),
    interruptible: u8 = (1, 189, 0),
    stop_moving: u8 = (1, 189, 0),
    queueable: u8 = (1, 189, 0),
    keep_target_while_disabled: u8 = (1, 189, 0),
    can_be_obscured: u8 = (1, 189, 0),
    unknown11: u8 = (1, 189, 0),
    unknown12: u8 = (1, 189, 0),
    weapon: u8 = (1, 189, 0),
    tech: u8 = (1, 189, 0),
    animation: u8 = (1, 189, 0),
    highlight: u16 = (2, 189, 0),
    requirements: u16 = (2, 189, 0),
    obscured_order: u8 = (1, 189, 0),
);

dat!(Upgrades, "arr/upgrades.dat", 61;
    mineral_base: u16 = (2, 61, 0),
    mineral_factor: u16 = (2, 61, 0),
    gas_base: u16 = (2, 61, 0),
    gas_factor: u16 = (2, 61, 0),
    time_base: u16 = (2, 61, 0),
    time_factor: u16 = (2, 61, 0),
    requirements: u16 = (2, 61, 0),
    icon: u16 = (2, 61, 0),
    label: u16 = (2, 61, 0),
    race: u8 = (1, 61, 0),
    max_level: u8 = (1, 61, 0),
    broodwar: u8 = (1, 61, 0),
);

dat!(Techdata, "arr/techdata.dat", 44;
    mineral_cost: u16 = (2, 44, 0),
    gas_cost: u16 = (2, 44, 0),
    research_time: u16 = (2, 44, 0),
    energy_cost: u16 = (2, 44, 0),
    research_requirements: u16 = (2, 44, 0),
    use_requirements: u16 = (2, 44, 0),
    icon: u16 = (2, 44, 0),
    label: u16 = (2, 44, 0),
    race: u8 = (1, 44, 0),
    researched: u8 = (1, 44, 0),
    broodwar: u8 = (1, 44, 0),
);

dat!(Sfxdata, "arr/sfxdata.dat", 1144;
    file: u32 = (4, 1144, 0), // 1-based sfxdata.tbl index
    priority: u8 = (1, 1144, 0),
    flags: u8 = (1, 1144, 0),
    race: u16 = (2, 1144, 0),
    volume: u8 = (1, 1144, 0),
);

pub struct Dat {
    pub units: Units,
    pub weapons: Weapons,
    pub flingy: Flingy,
    pub sprites: Sprites,
    pub images: Images,
    pub orders: Orders,
    pub upgrades: Upgrades,
    pub techdata: Techdata,
    pub sfxdata: Sfxdata,
}

impl Dat {
    pub fn load(casc: &Casc) -> io::Result<Dat> {
        Ok(Dat {
            units: Units::load(casc)?,
            weapons: Weapons::load(casc)?,
            flingy: Flingy::load(casc)?,
            sprites: Sprites::load(casc)?,
            images: Images::load(casc)?,
            orders: Orders::load(casc)?,
            upgrades: Upgrades::load(casc)?,
            techdata: Techdata::load(casc)?,
            sfxdata: Sfxdata::load(casc)?,
        })
    }
}

/// .tbl string table: u16 count, u16 offsets[count], NUL-terminated strings. Index is 0-based here.
pub fn parse_tbl(d: &[u8]) -> Vec<String> {
    let n = u16::from_le_bytes([d[0], d[1]]) as usize;
    (0..n)
        .map(|i| {
            let o = u16::from_le_bytes([d[2 + i * 2], d[3 + i * 2]]) as usize;
            let end = d[o..].iter().position(|&b| b == 0).map_or(d.len(), |e| o + e);
            String::from_utf8_lossy(&d[o..end]).into_owned()
        })
        .collect()
}
