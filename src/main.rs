mod casc;
mod dat;
mod map;
mod mpq;

use std::path::PathBuf;

fn install_dir() -> PathBuf {
    std::env::var_os("SCBW_INSTALL").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Games/battlenet/drive_c/Program Files (x86)/StarCraft")
    })
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
        Some("render") => {
            let chk = map::Chk::parse(&mpq::Mpq::open(args[1].as_ref())?.read("staredit\\scenario.chk")?);
            let ts = map::Tileset::load(&casc, chk.tileset())?;
            let (w, h, img) = map::render_terrain(&chk, &ts);
            write_png(&args[2], w, h, &img, &ts.palette)?;
        }
        _ => eprintln!("usage: scbw ls | extract <path> <out> | render <map.scm> <out.png>"),
    }
    Ok(())
}

fn write_png(path: &str, w: usize, h: usize, idx: &[u8], pal: &[[u8; 3]; 256]) -> std::io::Result<()> {
    let mut enc = png::Encoder::new(std::fs::File::create(path)?, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    let rgb: Vec<u8> = idx.iter().flat_map(|&i| pal[i as usize]).collect();
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}
