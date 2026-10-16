//! Per-player game tables (1.16.1 addresses).

#[derive(Clone, Debug)]
pub struct Players {
    /// 0x57f0f0 / 0x57f120: minerals and gas; 0x57f180 / 0x57f150: gathered totals.
    pub minerals: [i32; 12],
    pub gas: [i32; 12],
    pub minerals_gathered: [i32; 12],
    pub gas_gathered: [i32; 12],
    /// 0x5821a4 / 0x582234 / 0x5822c4: supply caps (zerg, terran, protoss).
    pub supply_max: [[i32; 12]; 3],
    /// 0x58d2b0: upgrade levels 0..46, [player][upgrade].
    pub upgrade_sc: [[u8; 46]; 12],
    /// 0x58f32c: Brood War upgrade levels 46..61.
    pub upgrade_bw: [[u8; 15]; 12],
    /// 0x58f3e0: upgrade-in-progress bits, 8 bytes per player.
    pub upgrade_in_progress: [[u8; 8]; 12],
    /// 0x582324: unit counts [unit type][player].
    pub unit_counts: Vec<[i32; 12]>,
    /// 0x582174 / 0x582204 / 0x582294: supply used by zerg / terran / protoss units.
    pub supply_used: [[i32; 12]; 3],
    /// 0x582084: factory count, 0x581f34: building count, 0x581de4: men count.
    pub factories: [i32; 12],
    pub buildings: [i32; 12],
    pub men: [i32; 12],
    /// 0x584de4: completed unit counts [unit type][player].
    pub completed_counts: Vec<[i32; 12]>,
    /// 0x582144 / 0x5821d4 / 0x582264: supply provided (zerg / terran / protoss).
    pub supply_provided: [[i32; 12]; 3],
    /// 0x5820b4 / 0x581e14 / 0x581f64: completed factories, men, buildings.
    pub factories_completed: [i32; 12],
    pub men_completed: [i32; 12],
    pub buildings_completed: [i32; 12],
    /// 0x581e44 units produced, 0x581ed4 unit score, 0x581f94 buildings constructed, 0x582024 building score.
    pub units_produced: [i32; 12],
    pub unit_score: [i32; 12],
    pub buildings_constructed: [i32; 12],
    pub building_score: [i32; 12],
    /// 0x57f27c: units each player may build (map settings; all by default).
    pub unit_available: [[u8; 228]; 12],
    /// 0x58ce24 / 0x58f050: techs enabled; 0x58cf44 / 0x58f140: techs researched
    /// (indexed by tech id; Brood War techs continue the classic arrays).
    pub tech_available: [[u8; 44]; 12],
    pub tech_researched: [[u8; 44]; 12],
    /// 0x58d088 / 0x58f24a: upgrade level caps.
    pub upgrade_max: [[u8; 61]; 12],
    /// 0x58f230: research-in-progress bits, 6 bytes per player.
    pub research_in_progress: [[u8; 6]; 12],
    /// 0x581e74 / 0x581fc4 / 0x5820e4: men, buildings and factories lost.
    pub men_lost: [i32; 12],
    pub buildings_lost: [i32; 12],
    pub factories_lost: [i32; 12],
    /// 0x581ea4 men killed, 0x581f04 kill score, 0x581ff4 buildings razed,
    /// 0x582054 razing score, 0x582114 factories razed.
    pub men_killed: [i32; 12],
    pub kill_score: [i32; 12],
    pub buildings_razed: [i32; 12],
    pub razing_score: [i32; 12],
    pub factories_razed: [i32; 12],
    /// 0x58a364: deaths [unit type][player]; 0x5878a4: kills [unit type][player].
    pub deaths: Vec<[i32; 12]>,
    /// 0x5822f4: custom score (triggers).
    pub custom_score: [i32; 12],
    pub kills: Vec<[i32; 12]>,
}

impl Default for Players {
    fn default() -> Players {
        Players {
            minerals: [0; 12],
            gas: [0; 12],
            minerals_gathered: [0; 12],
            gas_gathered: [0; 12],
            supply_max: [[400; 12]; 3],
            upgrade_sc: [[0; 46]; 12],
            upgrade_bw: [[0; 15]; 12],
            upgrade_in_progress: [[0; 8]; 12],
            unit_counts: vec![[0; 12]; 228],
            supply_used: [[0; 12]; 3],
            factories: [0; 12],
            buildings: [0; 12],
            men: [0; 12],
            completed_counts: vec![[0; 12]; 228],
            supply_provided: [[0; 12]; 3],
            factories_completed: [0; 12],
            men_completed: [0; 12],
            buildings_completed: [0; 12],
            units_produced: [0; 12],
            unit_score: [0; 12],
            buildings_constructed: [0; 12],
            building_score: [0; 12],
            unit_available: [[1; 228]; 12],
            tech_available: [[1; 44]; 12],
            tech_researched: [[0; 44]; 12],
            upgrade_max: [[0; 61]; 12],
            research_in_progress: [[0; 6]; 12],
            men_lost: [0; 12],
            buildings_lost: [0; 12],
            factories_lost: [0; 12],
            men_killed: [0; 12],
            kill_score: [0; 12],
            buildings_razed: [0; 12],
            razing_score: [0; 12],
            factories_razed: [0; 12],
            deaths: vec![[0; 12]; 228],
            custom_score: [0; 12],
            kills: vec![[0; 12]; 228],
        }
    }
}

impl Players {
    /// 0x4ce7a0: level of `upgrade` for `player` (ids below 0x2e: 0x58d2b0, stride 46; others 0x58f2fe, stride 15).
    pub fn upgrade_level(&self, player: u8, upgrade: usize) -> u8 {
        let p = player as usize;
        if upgrade < 46 {
            self.upgrade_sc[p][upgrade]
        } else {
            self.upgrade_bw[p][upgrade - 46]
        }
    }

    /// 0x4ce7f0: maximum level of `upgrade` for `player` (0x58d088 / 0x58f24a; Rust keeps one 61-entry array).
    pub fn upgrade_max_level(&self, player: u8, upgrade: usize) -> u8 {
        self.upgrade_max[player as usize][upgrade]
    }
}
