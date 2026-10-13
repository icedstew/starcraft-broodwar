#![allow(dead_code)] // dat fields are consumed incrementally as game systems land

mod audio;
mod bw;
mod casc;
mod dat;
mod game;
mod grp;
mod hpbar;
mod iscript;
mod map;
mod mpq;
mod replay;
mod ui;
mod view;

use std::path::PathBuf;

fn install_dir() -> PathBuf {
    std::env::var_os("SCBW_INSTALL").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Games/battlenet/drive_c/Program Files (x86)/StarCraft")
    })
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mpqls") {
        let m = mpq::Mpq::open(args[1].as_ref())?;
        let list = m.read("(listfile)")?;
        print!("{}", String::from_utf8_lossy(&list));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("mpqx") {
        let m = mpq::Mpq::open(args[1].as_ref())?;
        std::fs::write(&args[3], m.read(&args[2])?)?;
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("rep") {
        let r = replay::Replay::parse(&std::fs::read(&args[1])?)?;
        let h = &r.header;
        println!("map {:?} {}x{}, {} frames, title {:?}, host {:?}", h.map_name, h.map_width, h.map_height, h.frames, h.title, h.host);
        for s in h.slots.iter().filter(|s| !s.name.is_empty()) {
            println!("  slot {} player {} race {} team {} {:?}", s.slot_id, s.player_id, s.race, s.team, s.name);
        }
        let mut hist = std::collections::BTreeMap::new();
        for c in &r.commands {
            *hist.entry(c.data[0]).or_insert(0) += 1;
        }
        println!("{} commands, by id: {:x?}", r.commands.len(), hist);
        println!("chk {} bytes, tileset {}", r.chk.len(), map::Chk::parse(&r.chk).tileset());
        return Ok(());
    }
    let casc = casc::Casc::open(&install_dir())?;
    match args.first().map(String::as_str) {
        Some("ls") => {
            let mut v: Vec<_> = casc.files().collect();
            v.sort();
            v.iter().for_each(|f| println!("{f}"));
        }
        Some("extract") => {
            let data = casc.read(&args[1])?;
            std::fs::write(&args[2], data)?;
        }
        Some("export") => {
            // export <dir> <prefix>...: copy matching CASC files (for the emulator harness).
            let dir = std::path::Path::new(&args[1]);
            let files: Vec<String> = casc.files().filter(|f| args[2..].iter().any(|p| f.starts_with(p.as_str()))).map(|f| f.to_string()).collect();
            for f in &files {
                let out = dir.join(f);
                std::fs::create_dir_all(out.parent().unwrap())?;
                std::fs::write(out, casc.read(f)?)?;
            }
            println!("{} files", files.len());
        }
        Some("dat") => {
            let dat = dat::Dat::load(&casc)?;
            let names = dat::parse_tbl(&casc.read("rez/stat_txt.tbl")?);
            let u = &dat.units;
            for id in 0..dat::Units::COUNT {
                println!(
                    "{id:3} {:32} hp={:4} min={:3} gas={:3} gw={:3} flingy={:3} sprite={}",
                    names.get(id).map_or("", String::as_str),
                    u.hit_points[id] >> 8,
                    u.mineral_cost[id],
                    u.gas_cost[id],
                    u.ground_weapon[id],
                    u.flingy[id],
                    dat.flingy.sprite[u.flingy[id] as usize],
                );
            }
        }
        Some("bwsim") => {
            // Faithful-engine harness: SCBW_UNITS="type,x,y,player;..." SCBW_MOVE="x,y" bwsim <frames>
            let l = bw::load::Loaded::load(&casc)?;
            let g = |i: u16| l.grp_size(i);
            let d = bw::load::data(&l, &g);
            let chk = std::env::var("SCBW_MAP").ok().map(|p| load_chk(&p)).transpose()?;
            let dims = chk.as_ref().map_or((64, 64), |c| c.dims());
            let mut st = bw::State::new(dims, 0, 1700);
            st.search.init(&d);
            st.init_tech_defaults(&d);
            st.init_acquisition_ranges(&d);
            st.init_psi(&l.psi_grp);
            st.init_thingies(&d);
            if let Some(chk) = &chk {
                let name = map::TILESETS[chk.tileset()];
                let vf4 = casc.read(&format!("tileset/{name}.vf4"))?;
                let ts = bw::map::TilesetData {
                    vf4: vf4.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
                    cv5: casc.read(&format!("tileset/{name}.cv5"))?,
                };
                let (w, h) = chk.dims();
                let ok = st.load_terrain(&ts, w as u16, h as u16, chk.section(b"MTXM").unwrap_or(&[]));
                println!("map {w}x{h}: {} regions (ok {ok})", st.sai.region_count);
                if let Some(doodads) = chk.section(b"THG2") {
                    st.chk_doodads(&d, doodads, Default::default());
                }
                if let Some(units) = chk.section(b"UNIT") {
                    st.chk_units(&d, units, Default::default());
                    println!("map units: {}", st.unit_count);
                }
            }
            st.player_types = [2; 12];
            st.init_alliances(Default::default());
            for p in 0..8 {
                st.players.minerals[p] = 10000;
                st.players.gas[p] = 10000;
            }
            if let Ok(m) = std::env::var("SCBW_MELEE") {
                // "race,race,...": those players start a melee game (0x49da40, mode 2).
                let races: Vec<u8> = m.split(',').filter_map(|r| r.parse().ok()).collect();
                for p in 0..8 {
                    st.player_types[p] = if p < races.len() { 2 } else { 0 };
                    st.player_races[p] = races.get(p).copied().unwrap_or(0);
                }
                st.rng.enabled = true;
                st.melee_start_units(&d, 2);
                st.rng.enabled = false;
                if std::env::var("SCBW_RNGDBG").is_ok() {
                    let sites: Vec<_> = st.rng.per_site.iter().enumerate().filter(|(_, c)| **c != 0).collect();
                    eprintln!("melee rng sites {sites:?} total {}", st.rng.total);
                }
            }
            let mut made = Vec::new();
            for e in std::env::var("SCBW_UNITS").unwrap_or("0,320,320,0".into()).split(';').filter(|e| !e.is_empty()) {
                let v: Vec<i32> = e.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                if let [t, x, y, p] = v[..] {
                    st.rng.enabled = true;
                    let u = st.create_unit(&d, t as u16, x as i16, y as i16, p as u8);
                    if let Some(u) = u {
                        st.finish_unit_pre(&d, u);
                        if st.place_unit(&d, u) {
                            st.finish_unit_post(&d, u);
                        }
                        made.push(u);
                    }
                    st.rng.enabled = false;
                    println!("create {t} at ({x},{y}) p{p} -> {u:?}");
                }
            }
            if let Ok(m) = std::env::var("SCBW_MOVE") {
                apply_moves(&mut st, &d, &made, &m);
            }
            // SCBW_MOVE2 is applied the same way before frame SCBW_AT.
            let move2 = std::env::var("SCBW_MOVE2").ok();
            let at: u32 = std::env::var("SCBW_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            let frames: u32 = args.get(1).and_then(|f| f.parse().ok()).unwrap_or(48);
            let trace = std::env::var("SCBW_TRACE").ok().map(std::fs::File::create).transpose()?;
            let mut trace = trace.map(std::io::BufWriter::new);
            // SCBW_MINIMAP: also trace the minimap image every 50 frames.
            let mut minimap = match (&chk, std::env::var("SCBW_MINIMAP").is_ok()) {
                (Some(chk), true) => Some(view::load_minimap(&casc, chk)?),
                _ => None,
            };
            for f in 0..frames {
                if let (Some(m), true) = (&move2, f == at) {
                    apply_moves(&mut st, &d, &made, m);
                }
                // 0x4d9670: the frame counter (0x57f23c) advances before each step.
                st.frame += 1;
                st.game_step(&d);
                if let Some(t) = trace.as_mut() {
                    use std::io::Write;
                    for &u in &made {
                        writeln!(t, "{}", unit_trace(&st, f, u))?;
                    }
                    // Other units on the map (map-placed critters etc.).
                    let mut u = st.visible_units.head;
                    while u != bw::list::NIL {
                        if !made.contains(&u) {
                            writeln!(t, "{}", unit_trace(&st, f, u))?;
                        }
                        u = st.units[u as usize].link.next;
                    }
                    writeln!(t, "{}", thingy_trace(&st, f))?;
                    writeln!(t, "{}", fog_trace(&st, f))?;
                    if f % 25 == 0 {
                        writeln!(t, "{}", tile_trace(&st, f))?;
                    }
                    if let Ok(p) = std::env::var("SCBW_TILEDUMP") {
                        if f.to_string() == std::env::var("SCBW_TILEDUMP_AT").unwrap_or("0".into()) {
                            let m = &st.terrain;
                            let n = m.width as usize * m.height as usize;
                            let mut out: Vec<u8> = m.mtxm[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
                            out.extend(m.tile_flags[..n].iter().flat_map(|v| v.to_le_bytes()));
                            std::fs::write(p + ".rust", out)?;
                        }
                    }
                    writeln!(t, "{}", bullet_trace(&st, f))?;
                    let r: Vec<String> = (0..2).map(|p| format!("{},{}", st.players.minerals[p], st.players.gas[p])).collect();
                    writeln!(t, "f{f} res {}", r.join(" "))?;
                    writeln!(t, "f{f} rng {:#x} {}", st.rng.seed, st.rng.total)?;
                    if std::env::var("SCBW_SELTRACE").is_ok() {
                        let row: Vec<String> = bw::select::selection_units(&st.selections[0]).iter().map(|u| u.to_string()).collect();
                        writeln!(t, "f{f} psel {}", row.join(" "))?;
                        for g in 0..18 {
                            let hk: Vec<String> = st.hotkeys[g].iter().filter(|&&h| h != 0).map(|h| format!("{h:#x}")).collect();
                            if !hk.is_empty() {
                                writeln!(t, "f{f} hk{g} {}", hk.join(" "))?;
                            }
                        }
                    }
                    if let (Ok(tests), 0) = (std::env::var("SCBW_CLICKTEST"), f % 50) {
                        for pt in tests.split(';').filter(|p| !p.is_empty()) {
                            let v: Vec<i16> = pt.split(',').map(|n| n.parse().unwrap_or(0)).collect();
                            let u = st.unit_at_point(&d, v[0], v[1]);
                            let u = if u == bw::list::NIL { "-".to_string() } else { u.to_string() };
                            writeln!(t, "f{f} click {pt} -> {u}")?;
                        }
                    }
                    if let (Ok(tests), 0) = (std::env::var("SCBW_SELTEST"), f % 50) {
                        // "x0,y0,x1,y1[,s];...": drag boxes in turn (s = shift), from no selection.
                        let mut sel: Vec<u32> = Vec::new();
                        for b in tests.split(';').filter(|b| !b.is_empty()) {
                            let v: Vec<&str> = b.split(',').collect();
                            let r: Vec<i16> = v[..4].iter().map(|n| n.parse().unwrap_or(0)).collect();
                            if let Some(n) = st.box_select(&d, [r[0], r[1], r[2], r[3]], v.get(4) == Some(&"s"), &sel) {
                                sel = n;
                            }
                            let ids: Vec<String> = sel.iter().map(|u| u.to_string()).collect();
                            writeln!(t, "f{f} sel {b} -> {}", ids.join(" "))?;
                        }
                    }
                    if let (Some(m), 0) = (minimap.as_mut(), f % 50) {
                        m.refresh(&d, &mut st);
                        let hex: String = m.pixels.iter().map(|b| format!("{b:02x}")).collect();
                        writeln!(t, "f{f} minimap {}x{} {} {}", m.w, m.h, m.boxes, hex)?;
                    }
                }
                if f % 8 == 0 || f + 1 == frames {
                    for &u in &made {
                        let un = &st.units[u as usize];
                        if un.sprite == bw::list::NIL {
                            continue;
                        }
                        let s = &st.sprites[un.sprite as usize];
                        println!(
                            "f{f:4} u{u} order {:#x} st {} mv {:#x} pos ({},{}) tgt {:?} spd {} anim {} top {} next {} fl {:#x}",
                            un.order, un.order_state, un.movement_state, s.x, s.y, un.fl.move_target, un.fl.speed,
                            st.images[s.main_image as usize].iscript.anim,
                            un.fl.top_speed, un.fl.next_speed, un.fl.flags
                        );
                    }
                }
            }
            println!("rng seed {:#x} draws {}", st.rng.seed, st.rng.total);
        }
        Some("view") => {
            let a = view::Assets::load(casc)?;
            view::run(&a, load_chk(&args[1])?)?;
        }
        Some("shot") => {
            let a = view::Assets::load(casc)?;
            let mut scene = view::Scene::new(&a, load_chk(&args[1])?)?;
            eprintln!("local player {} start {:?}", scene.local_player(), scene.st.start_locations[scene.local_player() as usize]);
            let frames: usize = args.get(5).and_then(|f| f.parse().ok()).unwrap_or(24);
            let (x, y) = (args[3].parse().unwrap_or(0), args[4].parse().unwrap_or(0));
            // Debug: SCBW_SPAWN="kind,owner,x,y;..." creates units before stepping.
            if let Ok(spec) = std::env::var("SCBW_SPAWN") {
                for e in spec.split(';').filter(|e| !e.is_empty()) {
                    let v: Vec<i32> = e.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                    if let [k, o, sx, sy] = v[..] {
                        scene.spawn(k as u16, o as u8, sx as i16, sy as i16);
                    }
                }
            }
            if let Some(m) = std::env::var("SCBW_MINERALS").ok().and_then(|v| v.parse().ok()) {
                let me = scene.local_player() as usize;
                scene.st.players.minerals[me] = m;
            }
            // Vision is computed by the first frame; selection and commands see only
            // visible units.
            scene.step();
            if let Some(n) = args.get(6).and_then(|v| v.parse::<usize>().ok()) {
                // Debug: select up to n of the local player's units in view.
                let me = scene.local_player();
                let mut sel: Vec<u32> = scene.units_in_rect(x, y, x + 640, y + 360).into_iter().filter(|&u| scene.st.units[u as usize].player == me).collect();
                sel.truncate(n.min(12));
                scene.selected = sel;
            }
            // Debug: SCBW_MOVE=x,y right-clicks there with the selection before stepping.
            if let Some((mx, my)) = std::env::var("SCBW_MOVE").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
            }) {
                scene.right_click((mx, my), false);
            }
            // Debug: SCBW_SELECT=x,y selects the unit at that map point.
            if let Some((sx, sy)) = std::env::var("SCBW_SELECT").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
            }) {
                scene.selected = scene.unit_at(sx, sy).into_iter().collect();
            }
            // Debug: SCBW_KEYS="sm..." presses command card hotkeys with the selection.
            if let Ok(keys) = std::env::var("SCBW_KEYS") {
                let ui = ui::Ui::load(&a.casc, ["zerg", "terran", "protoss"][scene.local_race()]).ok();
                for c in keys.chars() {
                    let card = scene.card();
                    let hit = card.iter().flatten().find(|(b, en)| {
                        *en && ui.as_ref().and_then(|u| u.stat_txt.get((b.enabled_str as usize).wrapping_sub(1))).and_then(|s| s.chars().next()).map(|h| h.to_ascii_lowercase()) == Some(c)
                    });
                    eprintln!("key {c}: {:?}", hit.map(|(b, _)| (b.pos, b.act)));
                    if let Some((b, _)) = hit {
                        let b = *b;
                        scene.press(&b, false);
                    }
                }
            }
            // Debug: SCBW_CLICK=x,y clicks the map while targeting (after SCBW_KEYS).
            if let Some((px, py)) = std::env::var("SCBW_CLICK").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
            }) {
                eprintln!("targeting {:?}", scene.targeting);
                scene.target_click((px, py), false);
                eprintln!("click error {}", scene.error);
            }
            // Debug: SCBW_PLACE=x,y places the building chosen with SCBW_KEYS.
            if let Some((px, py)) = std::env::var("SCBW_PLACE").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
            }) {
                scene.check_placement((px, py));
                eprintln!("placing {:?} ok {}", scene.placing, scene.place_ok);
                scene.build((px, py));
            }
            // Debug: SCBW_TRAIN=type trains with the first selected unit; SCBW_BUILD=type,x,y
            // builds with it.
            if let Some(t) = std::env::var("SCBW_TRAIN").ok().and_then(|v| v.parse::<u16>().ok()) {
                scene.selected.retain(|&u| scene.st.units[u as usize].status & 2 != 0);
                scene.command(|st, d, me, sel| {
                    if let Some(&u) = sel.first() {
                        eprintln!("train check {:?}", st.can_make_unit(d, u, t, me));
                    }
                    st.command_train_sel(d, me, sel, t)
                });
            }
            if let Some((t, bx, by)) = std::env::var("SCBW_BUILD").ok().and_then(|v| {
                let n: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                Some((*n.first()? as u16, *n.get(1)?, *n.get(2)?))
            }) {
                scene.selected.retain(|&u| scene.st.units[u as usize].unit_type == 7);
                scene.selected.truncate(1);
                let (tx, ty) = scene.build_tile(t, (bx, by));
                scene.command(|st, d, me, sel| {
                    if let Some(&u) = sel.first() {
                        eprintln!("build {tx},{ty} make {:?} order {:?} place {}", st.can_make_unit(d, u, t, me), st.can_issue_order(d, u, 0x1e, me), st.placement_check(d, u, me, tx, ty, t, 1, false, false, false));
                    }
                });
                scene.placing = Some((0x1e, t));
                scene.build((bx, by));
            }
            for f in 0..frames {
                scene.step();
                if std::env::var("SCBW_DEBUG").is_ok() && f % 50 == 0 {
                    for &u in &scene.selected {
                        let un = &scene.st.units[u as usize];
                        let s = &scene.st.sprites[un.sprite as usize];
                        eprintln!("f{f} u{u} order {:#x} st {} pos ({},{}) tgt {} res {}", un.order, un.order_state, s.x, s.y, un.order_target_unit as i32, un.resource_type);
                    }
                }
            }
            if std::env::var("SCBW_DEBUG").is_ok() {
                for (i, slot) in scene.card().iter().enumerate() {
                    if let Some((b, en)) = slot {
                        eprintln!("card {} icon {} cond {:#x} act {:#x} enabled {en}", i + 1, b.icon, b.cond, b.act);
                    }
                }
            }
            let ui = ui::Ui::load(&a.casc, ["zerg", "terran", "protoss"][scene.local_race()]).ok();
            let (w, h) = (view::SCREEN_W as usize, view::SCREEN_H as usize);
            let (mut idx, mut rgb) = (vec![0u8; w * h], vec![0u32; w * h]);
            // Debug: SCBW_HOVER=type,sx,sy shows the placement box of `type` with the
            // cursor at screen point (sx, sy).
            let hover = std::env::var("SCBW_HOVER").ok().and_then(|v| {
                let n: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                Some((*n.first()? as u16, *n.get(1)?, *n.get(2)?))
            });
            // Debug: SCBW_ERROR=id shows stat_txt line id as an error.
            if let Some(e) = std::env::var("SCBW_ERROR").ok().and_then(|v| v.parse().ok()) {
                scene.show_error(e);
            }
            if let Some((t, sx, sy)) = hover {
                scene.placing = Some((0x1e, t));
                scene.check_placement((sx + x, sy + y));
            }
            // Debug: SCBW_CURSOR=sx,sy draws the cursor the game would show there.
            let at = std::env::var("SCBW_CURSOR").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))
            });
            let cursor = match at {
                Some((sx, sy)) => Some((scene.cursor_kind(sx, sy, x, y, false), 0, sx, sy)),
                None => hover.map(|(_, sx, sy)| (0, 0, sx, sy)),
            };
            view::compose_with_cursor(&scene, ui.as_ref(), x, y, cursor, &mut idx, &mut rgb);
            write_png_rgb(&args[2], w, h, &rgb)?;
        }
        Some("path") => {
            let chk = load_chk(&args[1])?;
            let ts = map::Tileset::load(&casc, chk.tileset())?;
            let (w, h, walk) = map::walk_grid(&chk, &ts);
            let g = game::path::WalkGrid { w, h, walk };
            let n: Vec<i32> = args[2..6].iter().map(|v| v.parse().unwrap_or(0)).collect();
            let t = std::time::Instant::now();
            println!("{:?} in {:?}", g.find((n[0], n[1]), (n[2], n[3])), t.elapsed());
        }
        Some("units") => {
            let chk = load_chk(&args[1])?;
            for r in chk.section(b"UNIT").unwrap_or(&[]).chunks_exact(36) {
                let r16 = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
                let res = u32::from_le_bytes(r[20..24].try_into().unwrap());
                println!("kind {:3} owner {:2} at ({:4},{:4}) resources {res}", r16(8), r[16], r16(4), r16(6));
            }
        }
        Some("walk") => {
            let chk = load_chk(&args[1])?;
            let ts = map::Tileset::load(&casc, chk.tileset())?;
            let (w, h, g) = map::walk_grid(&chk, &ts);
            let mut pal = [[0u8; 3]; 256];
            pal[1] = [255, 255, 255];
            let img: Vec<u8> = g.iter().map(|&b| b as u8).collect();
            write_png(&args[2], w, h, &img, &pal)?;
        }
        Some("mapdump") => {
            // Differential-test inputs/outputs for re/emu_map.py: mapdump <map> <dir>
            let chk = load_chk(&args[1])?;
            let dir = std::path::Path::new(&args[2]);
            std::fs::create_dir_all(dir)?;
            let name = map::TILESETS[chk.tileset()];
            let vf4 = casc.read(&format!("tileset/{name}.vf4"))?;
            let cv5 = casc.read(&format!("tileset/{name}.cv5"))?;
            let ts = bw::map::TilesetData {
                vf4: vf4.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
                cv5: cv5.clone(),
            };
            let (w, h) = chk.dims();
            let mtxm = chk.section(b"MTXM").unwrap_or(&[]).to_vec();
            let mut m = bw::map::GameMap::new(w as u16, h as u16, &ts);
            m.load_mtxm(&ts, &mtxm);
            m.build_minitiles(&ts);
            let le16 = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
            let le32 = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
            std::fs::write(dir.join("dims"), le16(&[w as u16, h as u16]))?;
            std::fs::write(dir.join("mtxm"), &mtxm)?;
            std::fs::write(dir.join("chk"), load_chk_raw(&args[1])?)?;
            std::fs::write(dir.join("tileset"), [chk.tileset() as u8])?;
            std::fs::write(dir.join("vf4"), &vf4)?;
            std::fs::write(dir.join("cv5"), &cv5)?;
            std::fs::write(dir.join("tile_flags"), le32(&m.tile_flags[..w * h]))?;
            std::fs::write(dir.join("megatiles"), le16(&m.megatiles[..w * h]))?;
            std::fs::write(dir.join("minitiles"), &m.minitiles)?;
            if let Some(mut sai) = bw::sai::create(&mut m, &ts) {
                // Region path searches between pseudo-random points.
                let mut seed = 12345u32;
                let mut rnd = |n: u32| {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                    (seed >> 8) % n
                };
                let (pw, ph) = (w as u32 * 32, h as u32 * 32);
                let mut tests = Vec::new();
                let mut out = Vec::new();
                for _ in 0..200 {
                    let mut ctx = bw::path::PathCtx::default();
                    ctx.start = (rnd(pw) as i16, rnd(ph) as i16);
                    ctx.target = (rnd(pw) as i16, rnd(ph) as i16);
                    tests.extend_from_slice(&le16(&[ctx.start.0 as u16, ctx.start.1 as u16, ctx.target.0 as u16, ctx.target.1 as u16]));
                    bw::path::find_path_regions(&mut sai, &mut ctx, [0; 4]);
                    out.extend_from_slice(&ctx.layout());
                }
                std::fs::write(dir.join("path_tests"), &tests)?;
                std::fs::write(dir.join("path_regions"), out)?;
                // Full searches for a marine.
                let dat = dat::Dat::load(&casc)?;
                let dims = |t: u16| {
                    let d = dat.units.dimensions[t as usize];
                    [d as i16, (d >> 16) as i16, (d >> 32) as i16, (d >> 48) as i16]
                };
                let unit_type = 0u16;
                let mut out = Vec::new();
                let mut walker = bw::short_path::Walker::default();
                for t in tests.chunks_exact(8) {
                    let r = |i: usize| i16::from_le_bytes([t[i], t[i + 1]]);
                    let mut ctx = bw::path::PathCtx { start: (r(0), r(2)), target: (r(4), r(6)), ..Default::default() };
                    let mut env = bw::short_path::Env { sai: &mut sai, st: None, unit: None, search: None, map_w: w as u16 * 32, map_h: h as u16 * 32, dims: &dims, unit_type };
                    bw::path::find_path(&mut env, &mut walker, &mut ctx);
                    out.extend_from_slice(&ctx.layout());
                }
                let d = dims(unit_type);
                std::fs::write(dir.join("path_unit"), le16(&[unit_type, d[0] as u16, d[1] as u16, d[2] as u16, d[3] as u16]))?;
                std::fs::write(dir.join("path_full"), out)?;
                std::fs::write(dir.join("sai"), bw::sai::layout(&sai))?;
                std::fs::write(dir.join("contours"), bw::sai::contours_layout(sai.contours.as_ref().unwrap()))?;
            }
        }
        Some("render") => {
            let chk = load_chk(&args[1])?;
            let ts = map::Tileset::load(&casc, chk.tileset())?;
            let (w, h, img) = map::render_terrain(&chk, &ts);
            write_png(&args[2], w, h, &img, &ts.palette)?;
        }
        _ => eprintln!(
            "usage: scbw ls | extract <path> <out> | dat | render <map> <out.png> | view <map> | shot <map> <out.png> <x> <y> [frames]"
        ),
    }
    Ok(())
}

/// One line of unit state, matching re/emu_game.py's trace of 1.16.1 memory.
fn bullet_trace(st: &bw::State, f: u32) -> String {
    let mut out = format!("f{f} bul");
    let mut b = st.bullets.active.head;
    while b != bw::list::NIL {
        let bu = &st.bullets.pool[b as usize];
        let t = if bu.target == bw::list::NIL { "-".to_string() } else { bu.target.to_string() };
        out += &format!(
            " w{},st{},{},{},t{},tm{},fl{},s{}",
            bu.weapon, bu.state, bu.fl.pos.0, bu.fl.pos.1, t, bu.remaining_time, bu.flags, (bu.sprite != bw::list::NIL) as u8
        );
        b = bu.link.next;
    }
    out
}

/// FNV-1a over the per-tile fog bytes (low byte of each tile flag).
fn fog_trace(st: &bw::State, f: u32) -> String {
    let (w, h) = (st.terrain.width as usize, st.terrain.height as usize);
    let mut x = 0x811c9dc5u32;
    for &t in &st.terrain.tile_flags[..w * h] {
        x = (x ^ (t & 0xff)).wrapping_mul(0x01000193);
    }
    format!("f{f} fog {x:#x}")
}

fn tile_trace(st: &bw::State, f: u32) -> String {
    let m = &st.terrain;
    let mut x = 0x811c9dc5u32;
    for i in 0..m.width as usize * m.height as usize {
        let [a, b] = m.mtxm[i].to_le_bytes();
        let [_, _, c, e] = m.tile_flags[i].to_le_bytes();
        for v in [a, b, c, e] {
            x = (x ^ v as u32).wrapping_mul(0x01000193);
        }
    }
    format!("f{f} tile {x:#x}")
}

fn thingy_trace(st: &bw::State, f: u32) -> String {
    let mut out = format!("f{f} th");
    let mut t = st.active_thingies.head;
    while t != bw::list::NIL {
        let s = &st.sprites[st.thingies[t as usize].sprite as usize];
        let im = &st.images[s.main_image as usize];
        out += &format!(" {},{},{},{},{},{}", s.sprite_id, s.x, s.y, im.iscript.pc, im.iscript.wait, im.frameset);
        t = st.thingies[t as usize].link.next;
    }
    out
}

fn unit_trace(st: &bw::State, f: u32, u: u32) -> String {
    let un = &st.units[u as usize];
    if un.sprite == bw::list::NIL {
        return format!("f{f} u{u} gone");
    }
    let s = &st.sprites[un.sprite as usize];
    let fl = &un.fl;
    format!(
        "f{f} u{u} pos {},{} order {:#x} ost {} mv {:#x} mt {},{} nw {},{} ntw {},{} fl {:#x} dir {} vdir {} ddir {} spd {} nspd {} top {} exact {},{} vel {},{} status {:#x} path {} ot {} mot {} hp {} sh {} gcd {} tgt {} vis {}",
        s.x, s.y, un.order, un.order_state, un.movement_state, fl.move_target.0, fl.move_target.1,
        fl.next_waypoint.0, fl.next_waypoint.1, fl.next_target_waypoint.0, fl.next_target_waypoint.1,
        fl.flags, fl.heading, fl.velocity_dir_prev, fl.desired_dir, fl.speed, fl.next_speed, fl.top_speed,
        fl.exact.0, fl.exact.1, fl.velocity.0, fl.velocity.1, un.status, (un.path != bw::list::NIL) as u8, un.order_process_timer, un.main_order_timer,
        un.hp, un.shields, un.ground_cooldown,
        if un.order_target_unit == bw::list::NIL { "-".to_string() } else { un.order_target_unit.to_string() },
        s.visibility
    )
}

fn load_chk(path: &str) -> std::io::Result<map::Chk> {
    Ok(map::Chk::parse(&load_chk_raw(path)?))
}

fn load_chk_raw(path: &str) -> std::io::Result<Vec<u8>> {
    if path.to_ascii_lowercase().ends_with(".rep") {
        return Ok(replay::Replay::parse(&std::fs::read(path)?)?.chk);
    }
    if path.to_ascii_lowercase().ends_with(".chk") {
        return std::fs::read(path);
    }
    mpq::Mpq::open(path.as_ref())?.read("staredit\\scenario.chk")
}

fn write_png(path: &str, w: usize, h: usize, idx: &[u8], pal: &[[u8; 3]; 256]) -> std::io::Result<()> {
    let mut enc = png::Encoder::new(std::fs::File::create(path)?, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    let rgb: Vec<u8> = idx.iter().flat_map(|&i| pal[i as usize]).collect();
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

fn write_png_rgb(path: &str, w: usize, h: usize, px: &[u32]) -> std::io::Result<()> {
    let mut enc = png::Encoder::new(std::fs::File::create(path)?, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    let rgb: Vec<u8> = px.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

/// Apply a bwsim SCBW_MOVE spec to the created units (see `bwsim`).
fn apply_moves(st: &mut bw::State, d: &bw::Data, made: &[u32], m: &str) {
    // "x,y" for every unit, or "x,y;x,y;..." cycled over the units.
    // "@k" attacks the k-th created unit (AttackUnit, order 0xa).
    let v: Vec<&str> = m.split(';').filter(|p| !p.is_empty()).collect();
    if v.is_empty() {
        return;
    }
    // Group commands: "R<x>,<y>[q]" / "R@k[q]" right-click, "T<order>:<x>,<y>[q]" /
    // "T<order>:@k[q]" targeted order, with every created player-0 unit selected.
    let mut sel: Vec<u32> = made.iter().copied().filter(|&u| st.units[u as usize].player == 0).take(12).collect();
    if made.is_empty() {
        // Melee start: player 0's workers.
        sel = (0..st.units.len() as u32)
            .filter(|&u| {
                let un = &st.units[u as usize];
                un.sprite != bw::list::NIL && un.player == 0 && matches!(un.unit_type, 7 | 0x29 | 0x40)
            })
            .take(12)
            .collect();
    }
    for p in v.iter().filter(|p| p.starts_with(['S', 'A', 'D', 'H'])) {
        // "S<i>,<j>,..." select / add / remove created units (0x09-0x0b); "H<action>:<group>" hotkey 0x13.
        let units = if made.is_empty() { &sel[..] } else { made };
        let pkt: Vec<u8> = if let Some(b) = p.strip_prefix('H') {
            let (a, g) = b.split_once(':').unwrap_or(("0", "0"));
            vec![0x13, a.parse().unwrap_or(0), g.parse().unwrap_or(0)]
        } else {
            // "#n" is unit array index n rather than the n-th created unit.
            let ids: Vec<u32> = p[1..]
                .split(',')
                .filter_map(|n| match n.strip_prefix('#') {
                    Some(r) => r.parse().ok(),
                    None => n.parse::<usize>().ok().map(|i| units[i]),
                })
                .collect();
            let mut pkt = vec![[9, 10, 11]["SAD".find(&p[..1]).unwrap_or(0)], ids.len() as u8];
            for u in ids {
                pkt.extend((st.unit_handle(u) as u16).to_le_bytes());
            }
            pkt
        };
        st.execute_command(d, 0, &[], &pkt);
    }
    for p in v.iter().filter(|p| p.starts_with('K')) {
        // "K<op>[:<u16>]": cancel command 0x18 / 0x19 / 0x20 with player 0's selection.
        let (op, arg) = p[1..].split_once(':').unwrap_or((&p[1..], "0"));
        let num = |s: &str| s.strip_prefix("0x").map_or_else(|| s.parse().unwrap_or(0), |h| u16::from_str_radix(h, 16).unwrap_or(0));
        let mut pkt = vec![num(op) as u8];
        pkt.extend(num(arg).to_le_bytes());
        let sel = bw::select::selection_units(&st.selections[0]).to_vec();
        st.execute_command(d, 0, &sel, &pkt);
    }
    for p in v.iter().filter(|p| p.starts_with('M')) {
        // "M<type>": every player-0 larva morphs into <type> (0x4c1990).
        let t = p[1..].strip_prefix("0x").map_or_else(|| p[1..].parse().unwrap_or(0), |h| u16::from_str_radix(h, 16).unwrap_or(0));
        let larvae: Vec<u32> = (0..st.units.len() as u32)
            .filter(|&u| {
                let un = &st.units[u as usize];
                un.sprite != bw::list::NIL && un.player == 0 && un.unit_type == 0x23
            })
            .take(12)
            .collect();
        st.command_unit_morph(&d, 0, &larvae, t);
    }
    for p in v.iter().filter(|p| p.starts_with(['R', 'T'])) {
        let q = p.ends_with('q');
        let mut body = p[1..].trim_end_matches('q');
        let mut order = 0u8;
        if p.starts_with('T') {
            let (o, b) = body.split_once(':').unwrap_or(("0", body));
            order = o.strip_prefix("0x").map_or_else(|| o.parse().unwrap_or(0), |h| u8::from_str_radix(h, 16).unwrap_or(0));
            body = b;
        }
        let (pos, t) = if let Some(k) = body.strip_prefix('@') {
            let t = made[k.parse::<usize>().unwrap_or(0)];
            let s = &st.sprites[st.units[t as usize].sprite as usize];
            ((s.x, s.y), t)
        } else {
            let xy: Vec<i16> = body.split(',').filter_map(|n| n.parse().ok()).collect();
            ((xy[0], xy[1]), bw::list::NIL)
        };
        if p.starts_with('R') {
            st.command_right_click(&d, 0, &sel, pos, t, 0xe4, q);
        } else {
            st.command_targeted_order(&d, 0, &sel, pos, t, 0xe4, order, q);
        }
    }
    let units = if made.is_empty() { sel.clone() } else { made.to_vec() };
    for (i, &u) in units.iter().enumerate() {
        let p = v[i % v.len()];
        if p.starts_with(['R', 'T', 'M', 'S', 'A', 'D', 'H', 'K']) {
            continue;
        }
        if let Some(t) = p.strip_prefix('t') {
            // "t<type>[x<count>]"
            let (t, n) = t.split_once('x').map_or((t, 1), |(a, b)| (a, b.parse().unwrap_or(1)));
            let t: u16 = t.parse().unwrap_or(0);
            for _ in 0..n {
                st.command_train(&d, u, t);
            }
            continue;
        }
        if let Some(rest) = p.strip_prefix('b') {
            // "b<order>:<type>:<tx>,<ty>"
            let f: Vec<i32> = rest.split([':', ',']).map(|n| n.strip_prefix("0x").map_or_else(|| n.parse().unwrap_or(0), |h| i32::from_str_radix(h, 16).unwrap_or(0))).collect();
            if let [o, t, tx, ty] = f[..] {
                st.command_build(&d, u, o as u8, tx as i16, ty as i16, t as u16);
            }
            continue;
        }
        if let Some(rest) = p.strip_prefix('o') {
            let (o, xy) = rest.split_once(':').unwrap_or(("0", "0,0"));
            let o = if let Some(h) = o.strip_prefix("0x") { u8::from_str_radix(h, 16).unwrap_or(0) } else { o.parse().unwrap_or(0) };
            if let Some(k) = xy.strip_prefix('@') {
                let t = made[k.parse::<usize>().unwrap_or(0)];
                let s = &st.sprites[st.units[t as usize].sprite as usize];
                let pos = (s.x, s.y);
                st.order_target(&d, u, o, pos, t);
                continue;
            }
            let v: Vec<i16> = xy.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            let mut t = bw::list::NIL;
            let mut a = st.visible_units.head;
            while a != bw::list::NIL {
                let s = &st.sprites[st.units[a as usize].sprite as usize];
                if (s.x, s.y) == (v[0], v[1]) {
                    t = a;
                }
                a = st.units[a as usize].link.next;
            }
            let pos = (v[0], v[1]);
            st.order_target(&d, u, o, pos, t);
            continue;
        }
        if let Some(k) = p.strip_prefix('@') {
            let t = made[k.parse::<usize>().unwrap_or(0)];
            let s = &st.sprites[st.units[t as usize].sprite as usize];
            let pos = (s.x, s.y);
            st.order_target(&d, u, 0xa, pos, t);
            continue;
        }
        let t: Vec<i16> = p.split(',').filter_map(|n| n.trim().parse().ok()).collect();
        st.order_target(&d, u, 6, (t[0], t[1]), bw::list::NIL);
    }
}
