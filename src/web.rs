//! Browser entry (web/build.sh, web/index.html): the page hands over storm.dll, the
//! archives and a map or replay. It all runs in a Web Worker (web/worker.js, started by
//! web/game.js): the page forwards input to vendor/minifb's worker backend and plays the sound.

use crate::{map, mpq, replay, storage, view};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

fn js(e: std::io::Error) -> JsError {
    JsError::new(&e.to_string())
}

/// `archives`: Uint8Arrays in storage::ARCHIVES order. `name` picks how `file` is read
/// (.rep replay, .chk, else a .scm/.scx archive).
#[wasm_bindgen]
pub fn start(storm: &[u8], archives: js_sys::Array, name: &str, file: &[u8]) -> Result<(), JsError> {
    console_error_panic_hook::set_once();
    let archives = archives.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let files = storage::Storage::from_mem(storm, archives).map_err(js)?;
    // The frame loop outlives this call, so the assets live for the page's lifetime.
    let a: &'static mut view::Assets = Box::leak(Box::new(view::Assets::load(files).map_err(js)?));
    let name = name.to_ascii_lowercase();
    let frame = if name.ends_with(".rep") {
        let rep = replay::Replay::parse(file).map_err(js)?;
        view::prepare_replay(a, &rep);
        let a: &'static view::Assets = a;
        let scene = view::Scene::from_replay(a, &rep).map_err(js)?;
        Box::new(view::frame_loop(a, scene, Some(view::ReplayPlay::new(rep))).map_err(js)?) as Box<dyn FnMut() -> std::io::Result<bool>>
    } else {
        let chk = if name.ends_with(".chk") { file.to_vec() } else { mpq::Mpq::new(file.to_vec()).and_then(|m| m.read("staredit\\scenario.chk")).map_err(js)? };
        let chk = map::Chk::parse(&chk);
        view::prepare_map(a, &chk);
        let a: &'static view::Assets = a;
        Box::new(view::frame_loop(a, view::Scene::new(a, chk).map_err(js)?, None).map_err(js)?)
    };
    run_raf(frame);
    Ok(())
}

/// `setTimeout` of the global scope (the page's window, or the Web Worker the game runs in).
#[wasm_bindgen(inline_js = "export function after(ms, f) { setTimeout(f, ms); }")]
extern "C" {
    fn after(ms: i32, f: &js_sys::Function);
}

/// Run `frame` until it returns false or fails.
fn run_raf(frame: Box<dyn FnMut() -> std::io::Result<bool>>) {
    run_raf_then(frame, None);
}

/// `run_raf`, calling `done` once the loop ended (the game's frame returned false or failed).
/// The game runs in a Web Worker (web/worker.js): its timers keep running in a background
/// tab, where the page's requestAnimationFrame stops (a stopped network game is dropped).
/// Every 16 ms, about the display rate the native window loop has: the cursor is redrawn
/// every pass, the frame loop ticks the game every 42 ms.
fn run_raf_then(frame: Box<dyn FnMut() -> std::io::Result<bool>>, done: Option<js_sys::Function>) {
    let tick: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let again = tick.clone();
    // The loop's state (a network game's `NetGame` among it) is dropped when it ends, so a
    // leave is announced and the link closes (the closure itself lives on).
    let mut frame = Some(frame);
    *tick.borrow_mut() = Some(Closure::new(move || {
        let Some(f) = frame.as_mut() else { return };
        let end = |frame: &mut Option<_>| {
            *frame = None;
            if let Some(d) = done.as_ref() {
                let _ = d.call0(&JsValue::NULL);
            }
        };
        let t0 = web_time::Instant::now();
        match f() {
            Ok(true) => {}
            Ok(false) => return end(&mut frame),
            Err(e) => {
                web_sys::console::error_1(&e.to_string().into());
                return end(&mut frame);
            }
        }
        let spent = t0.elapsed().as_millis() as i32;
        after((16 - spent).max(0), again.borrow().as_ref().unwrap().as_ref().unchecked_ref());
    }));
    after(0, tick.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}

/// The rollback oracle (src/rbtest.rs) on a replay, for its timing in the browser
/// (index.html `?auto=<rep>&rbtest=<delay>`).
#[wasm_bindgen]
pub fn rbtest(storm: &[u8], archives: js_sys::Array, file: &[u8], delay: u32, frames: u32) -> Result<String, JsError> {
    console_error_panic_hook::set_once();
    let archives = archives.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let files = storage::Storage::from_mem(storm, archives).map_err(js)?;
    let mut a = view::Assets::load(files).map_err(js)?;
    let rep = replay::Replay::parse(file).map_err(js)?;
    view::prepare_replay(&mut a, &rep);
    crate::rbtest::run(&a, &rep, delay, frames).map_err(js)
}

/// Resolves after `ms` milliseconds (the lobby wait polls the transport between turns of the
/// browser's event loop).
async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        after(ms, &res);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

/// Join a native `scbw nethost` game (`code` = its `ip:port#certhash` LISTEN line): connect
/// over WebTransport, wait for the host's Start (slot, delay, game setup), then play it
/// through the same `NetGame` driver as `scbw netjoin` (src/net/live.rs). Resolves once the
/// game is running; `frames` > 0 logs `frame K hash H` to the console at that frame (the
/// headless-net check) and stops the game there.
#[wasm_bindgen]
pub async fn net_join(storm: Vec<u8>, archives: js_sys::Array, code: String, frames: u32, script: String, name: String, army: u8, quit: u32) -> Result<(), JsError> {
    use crate::net::{Session, live, transport};
    console_error_panic_hook::set_once();
    let archives = archives.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let files = storage::Storage::from_mem(&storm, archives).map_err(js)?;
    let a: &'static mut view::Assets = Box::leak(Box::new(view::Assets::load(files).map_err(js)?));
    let err = |m: String| JsError::new(&m);
    let mut tr = transport::Transport::join_web(&code, transport::ANY, 0).map_err(err)?;
    // Turns that beat the Start message are not delivered again.
    let mut early = Vec::new();
    let (slot, delay, setup) = 'lobby: loop {
        for e in tr.poll() {
            match e {
                transport::Event::Start { slot, first, setup } => break 'lobby (slot, first, setup),
                transport::Event::Error(m) => return Err(err(m)),
                transport::Event::Turn { player, frame, bytes } => early.push((player, frame, bytes)),
                _ => {}
            }
        }
        sleep(5).await;
    };
    // A lobby's Start has no setup (the glue made the game): its front end takes over.
    if setup.is_empty() {
        // An enhanced game's army to ask for (255: any).
        if army < 8 {
            crate::net::snet::want_army(army);
        }
        if frames > 0 {
            live::NetGame::report_next(frames);
        }
        if !script.is_empty() || quit > 0 {
            live::NetGame::script_next(&script, (quit > 0).then_some(quit));
        }
        return glue_lobby(a, tr, slot, code, name).await;
    }
    let (mut g, table) = live::decode_setup(&setup, slot).ok_or_else(|| err("bad game setup".into()))?;
    let mut net = live::NetGame::new(Session::new(slot, &table, delay), tr, false, early).map_err(err)?;
    net.set_script(&script).map_err(err)?;
    if quit > 0 {
        net.set_quit(quit);
    }
    if frames > 0 {
        net.report_at(frames);
    }
    view::set_net_driver(Box::new(net));
    g.multiplayer = true;
    view::prepare_setup(a, &mut g);
    let a: &'static view::Assets = a;
    let frame = view::frame_loop_end(a, view::Scene::for_setup(a, &g).map_err(js)?, None, Default::default()).map_err(js)?;
    run_raf(Box::new(frame));
    Ok(())
}

/// The lobby of a game made in the glue (`net_join` found it): the link becomes the Storm
/// game (snet.rs) and the front end runs from the lobby (gluChat) as `name`: the map comes
/// from the creator if needed, the game starts from there.
async fn glue_lobby(a: &'static mut view::Assets, tr: crate::net::transport::Transport, slot: u8, code: String, name: String) -> Result<(), JsError> {
    use crate::net::snet;
    let err = |m: String| JsError::new(&m);
    snet::initialize_provider(snet::UDPN);
    snet::adopt(tr, slot, code, name.as_bytes());
    let mut glue = crate::glue::Glue::new(&a.files);
    let t0 = web_time::Instant::now();
    while !glue.join_by_code(&a.files, name.as_bytes()) {
        // A running enhanced game sends its state instead: the game starts from it.
        if let Some(late) = snet::late() {
            glue.late = Some(late);
            glue.state = crate::glue::state::START;
            break;
        }
        if let Some(why) = snet::refused() {
            return Err(err(why));
        }
        if t0.elapsed().as_secs() > 60 {
            return Err(err("no answer from the game".into()));
        }
        snet::pump();
        sleep(5).await;
    }
    let frame = crate::glue::web_frame(a as *mut view::Assets, glue).map_err(js)?;
    run_raf(Box::new(frame));
    Ok(())
}

thread_local! {
    /// The campaign map page's game files (`campaign_load`), for every match it joins.
    static CAMPAIGN_ASSETS: RefCell<Option<&'static view::Assets>> = const { RefCell::new(None) };
}

/// Ours (the campaign map page, src/campaign/campaign.html): the player's install, loaded once
/// for the page (storm.dll and the archives, as `start`).
#[wasm_bindgen]
pub fn campaign_load(storm: Vec<u8>, archives: js_sys::Array) -> Result<(), JsError> {
    console_error_panic_hook::set_once();
    let archives = archives.iter().map(|a| js_sys::Uint8Array::new(&a).to_vec()).collect();
    let files = storage::Storage::from_mem(&storm, archives).map_err(js)?;
    let a: &'static view::Assets = Box::leak(Box::new(view::Assets::load(files).map_err(js)?));
    CAMPAIGN_ASSETS.with(|c| *c.borrow_mut() = Some(a));
    Ok(())
}

/// Ours: join the campaign match `code` as `name` with the files `campaign_load` took and play
/// it in the page's #scbw; resolves when the game ends (left with the menu's Leave, won,
/// lost, drawn, or the match ended): the page shows its map again. No glue screens (no score,
/// no replay to save). Fails with the host's reason when refused. `frames` > 0 logs
/// `frame K hash H` there (tests).
#[wasm_bindgen]
pub async fn campaign_play(code: String, name: String, frames: u32) -> Result<(), JsError> {
    use crate::net::{live, snet, transport};
    let err = |m: String| JsError::new(&m);
    let a = CAMPAIGN_ASSETS.with(|c| *c.borrow()).ok_or_else(|| err("load the game files first".into()))?;
    let mut tr = transport::Transport::join_web(&code, transport::ANY, 0).map_err(err)?;
    let slot = 'lobby: loop {
        for e in tr.poll() {
            match e {
                transport::Event::Start { slot, .. } => break 'lobby slot,
                transport::Event::Error(m) => return Err(err(m)),
                _ => {}
            }
        }
        sleep(5).await;
    };
    let log = |m: &str| web_sys::console::log_1(&format!("campaign: {m}").into());
    log(&format!("linked as {slot}"));
    snet::initialize_provider(snet::UDPN);
    snet::adopt(tr, slot, code, name.as_bytes());
    let t0 = web_time::Instant::now();
    let late = loop {
        snet::pump();
        if let Some(why) = snet::refused() {
            snet::leave_game();
            return Err(err(format!("{}{why}", transport::REFUSED)));
        }
        if let Some(l) = snet::late() {
            break l;
        }
        if t0.elapsed().as_secs() > 60 {
            snet::leave_game();
            return Err(err("no answer from the game".into()));
        }
        sleep(5).await;
    };
    log(&format!("the game's state came ({} bytes)", late.len()));
    if frames > 0 {
        live::NetGame::report_next(frames);
    }
    let (scene, net) = live::late_game(a, &late).map_err(js)?;
    view::set_net_driver(Box::new(net));
    let frame: Box<dyn FnMut() -> std::io::Result<bool>> = Box::new(view::frame_loop_end(a, scene, None, Default::default()).map_err(js)?);
    let mut frame = Some(frame);
    let ended = js_sys::Promise::new(&mut |done, _| {
        if let Some(f) = frame.take() {
            run_raf_then(f, Some(done));
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(ended).await;
    log("the game ended");
    // The window the game parked is not used again (a reused one took no keys): the page
    // removes its canvas, the next game makes a new one.
    drop(view::take_window());
    snet::leave_game();
    Ok(())
}

