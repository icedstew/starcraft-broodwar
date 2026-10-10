#![allow(dead_code)] // dat fields are consumed incrementally as game systems land

mod audio;
mod bw;
mod casc;
mod dat;
mod game;
mod grp;
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
            let mut st = bw::State::new((64, 64), 0, 1700);
            st.player_types = [2; 12];
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
                let v: Vec<i16> = m.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                for &u in &made {
                    st.order_target(&d, u, 6, (v[0], v[1]), bw::list::NIL);
                }
            }
            let frames: u32 = args.get(1).and_then(|f| f.parse().ok()).unwrap_or(48);
            for f in 0..frames {
                st.game_step(&d);
                if f % 8 == 0 || f + 1 == frames {
                    for &u in &made {
                        let un = &st.units[u as usize];
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
            let frames: usize = args.get(5).and_then(|f| f.parse().ok()).unwrap_or(24);
            let (x, y) = (args[3].parse().unwrap_or(0), args[4].parse().unwrap_or(0));
            // Debug: SCBW_SPAWN="kind,owner,x,y;..." creates units before stepping.
            if let Ok(spec) = std::env::var("SCBW_SPAWN") {
                for e in spec.split(';').filter(|e| !e.is_empty()) {
                    let v: Vec<i32> = e.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                    if let [k, o, sx, sy] = v[..] {
                        scene.spawn(k as usize, o as u8, sx, sy);
                    }
                }
            }
            if let Some(n) = args.get(6).and_then(|v| v.parse::<usize>().ok()) {
                // Debug: select up to n units in view.
                let mut sel = scene.units_in_rect(x, y, x + 640, y + 360);
                sel.truncate(n);
                scene.selected = sel;
            }
            if std::env::var("SCBW_SEL_ALL").is_ok() {
                scene.selected = (0..scene.game.units.len()).filter(|&u| scene.game.units[u].owner == 0).collect();
            }
            // Debug: SCBW_MOVE=x,y orders the selection to move there before stepping.
            if let Some((mx, my)) = std::env::var("SCBW_MOVE").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i16>().ok()?, b.parse::<i16>().ok()?))
            }) {
                for &u in &scene.selected.clone() {
                    let t = game::Target { pos: (mx, my), unit: None };
                    let order = std::env::var("SCBW_ORDER").ok().and_then(|v| v.parse().ok()).unwrap_or(game::orders::id::MOVE);
                    if scene.game.units[u].owner == 0 {
                        scene.game.issue_order(u, order, t, false);
                    }
                }
            }
            if let Some(m) = std::env::var("SCBW_MINERALS").ok().and_then(|v| v.parse().ok()) {
                scene.game.players[0].minerals = m;
            }
            if let Ok(spec) = std::env::var("SCBW_BUILD") {
                let v: Vec<i32> = spec.split(',').filter_map(|n| n.parse().ok()).collect();
                let worker = (0..scene.game.units.len()).find(|&u| scene.game.units[u].owner == 0 && a.dat.units.flags[scene.game.units[u].kind] & 0x8 != 0);
                if let (Some(b), &[k, x, y]) = (worker, &v[..]) {
                    let frames = |_: u16| 0;
                    let site = game::build::snap_on(&scene.game, &scene.world(&frames), k as usize, x, y);
                    game::build::order_build(&mut scene.game, b, k as usize, site);
                }
            }
            if let Some(k) = std::env::var("SCBW_MORPH").ok().and_then(|v| v.parse::<usize>().ok()) {
                let larva = (0..scene.game.units.len()).filter(|&u| scene.game.units[u].owner == 0 && scene.game.units[u].kind == 35).collect::<Vec<_>>();
                let frames = |_: u16| 0;
                for l in larva {
                    let w = scene.world(&frames);
                    eprintln!("morph: {:?}", game::morph::morph_unit(&mut scene.game, &w, l, k));
                }
            }
            if let Some(k) = std::env::var("SCBW_TRAIN").ok().and_then(|v| v.parse::<usize>().ok()) {
                let depot = (0..scene.game.units.len()).find(|&u| scene.game.units[u].owner == 0 && a.dat.units.flags[scene.game.units[u].kind] & 0x1000 != 0);
                if let Some(b) = depot {
                    let frames = |_: u16| 0;
                    let w = scene.world(&frames);
                    eprintln!("train: {:?}", game::economy::train(&mut scene.game, &w, b, k));
                }
            }
            for _ in 0..frames {
                scene.step();
            }
            if std::env::var("SCBW_DEBUG").is_ok() {
                eprintln!("player0 {:?} frame {}", scene.game.players[0], scene.game.frame);
                for (i, u) in scene.game.units.iter().enumerate().filter(|(_, u)| u.owner < 8) {
                    eprintln!("unit {i} kind {} owner {} order {} state {} pos {:?} target {:?} path {:?} hp {} dead {}",
                        u.kind, u.owner, u.order, u.order_state, u.flingy.pos, u.flingy.move_target, u.path, u.hp, u.dead);
                }
            }
            let ui = ui::Ui::load(&a.casc, ["zerg", "terran", "protoss"][scene.local_race()]).ok();
            let (w, h) = (view::SCREEN_W as usize, view::SCREEN_H as usize);
            let (mut idx, mut rgb) = (vec![0u8; w * h], vec![0u32; w * h]);
            view::compose(&scene, ui.as_ref(), x, y, &mut idx, &mut rgb);
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
            std::fs::write(dir.join("vf4"), &vf4)?;
            std::fs::write(dir.join("cv5"), &cv5)?;
            std::fs::write(dir.join("tile_flags"), le32(&m.tile_flags[..w * h]))?;
            std::fs::write(dir.join("megatiles"), le16(&m.megatiles[..w * h]))?;
            std::fs::write(dir.join("minitiles"), &m.minitiles)?;
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

fn load_chk(path: &str) -> std::io::Result<map::Chk> {
    if path.to_ascii_lowercase().ends_with(".rep") {
        return Ok(map::Chk::parse(&replay::Replay::parse(&std::fs::read(path)?)?.chk));
    }
    if path.to_ascii_lowercase().ends_with(".chk") {
        return Ok(map::Chk::parse(&std::fs::read(path)?));
    }
    Ok(map::Chk::parse(&mpq::Mpq::open(path.as_ref())?.read("staredit\\scenario.chk")?))
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
