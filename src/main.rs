#![allow(dead_code)] // dat fields are consumed incrementally as game systems land

mod audio;
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
        Some("view") => {
            let a = view::Assets::load(casc)?;
            view::run(&a, load_chk(&args[1])?)?;
        }
        Some("shot") => {
            let a = view::Assets::load(casc)?;
            let mut scene = view::Scene::new(&a, load_chk(&args[1])?)?;
            let frames: usize = args.get(5).and_then(|f| f.parse().ok()).unwrap_or(24);
            let (x, y) = (args[3].parse().unwrap_or(0), args[4].parse().unwrap_or(0));
            if let Some(n) = args.get(6).and_then(|v| v.parse::<usize>().ok()) {
                // Debug: select up to n units in view.
                let mut sel = scene.units_in_rect(x, y, x + 640, y + 360);
                sel.truncate(n);
                scene.selected = sel;
            }
            // Debug: SCBW_MOVE=x,y orders the selection to move there before stepping.
            if let Some((mx, my)) = std::env::var("SCBW_MOVE").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.parse::<i16>().ok()?, b.parse::<i16>().ok()?))
            }) {
                for &u in &scene.selected.clone() {
                    scene.units[u].flingy.set_move_target(mx, my);
                }
            }
            for _ in 0..frames {
                scene.step();
            }
            let ui = ui::Ui::load(&a.casc, ["zerg", "terran", "protoss"][scene.local_race()]).ok();
            let (w, h) = (view::SCREEN_W as usize, view::SCREEN_H as usize);
            let (mut idx, mut rgb) = (vec![0u8; w * h], vec![0u32; w * h]);
            view::compose(&scene, ui.as_ref(), x, y, &mut idx, &mut rgb);
            write_png_rgb(&args[2], w, h, &rgb)?;
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
