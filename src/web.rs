//! Browser entry (web/build.sh, web/index.html): the page hands over storm.dll, the
//! archives and a map or replay; the frame loop runs from requestAnimationFrame.

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

/// Run `frame` from requestAnimationFrame until it returns false or fails.
fn run_raf(frame: Box<dyn FnMut() -> std::io::Result<bool>>) {
    // requestAnimationFrame runs at the display rate, as the native window loop: the
    // cursor is redrawn every pass, the frame loop ticks the game every 42 ms.
    let win = web_sys::window().unwrap();
    let tick: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let again = tick.clone();
    // The loop's state (a network game's `NetGame` among it) is dropped when it ends, so a
    // leave is announced and the link closes (the rAF closure itself lives on).
    let mut frame = Some(frame);
    *tick.borrow_mut() = Some(Closure::new(move || {
        let Some(f) = frame.as_mut() else { return };
        match f() {
            Ok(true) => {}
            Ok(false) => return frame = None,
            Err(e) => {
                frame = None;
                return web_sys::console::error_1(&e.to_string().into());
            }
        }
        let w = web_sys::window().unwrap();
        w.request_animation_frame(again.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
    }));
    win.request_animation_frame(tick.borrow().as_ref().unwrap().as_ref().unchecked_ref()).unwrap();
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

/// minifb's wasm backend never calls `InputCallback::add_char`, so the game's typed
/// characters (WM_CHAR: command card hotkeys, chat) come from the canvas's keydown
/// events here, as Windows would translate them.
pub fn typed_chars(typed: Rc<RefCell<Vec<u32>>>) {
    let doc = web_sys::window().unwrap().document().unwrap();
    let Some(canvas) = doc.query_selector("canvas").ok().flatten() else { return };
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.alt_key() || e.meta_key() {
            return; // WM_SYSCHAR, not WM_CHAR
        }
        let key = e.key();
        let mut it = key.chars();
        let c = match (it.next(), it.next()) {
            (Some(c), None) if e.ctrl_key() && c.is_ascii_alphabetic() => c.to_ascii_lowercase() as u32 & 0x1f,
            (Some(c), None) if !e.ctrl_key() => c as u32,
            _ => match key.as_str() {
                "Enter" => 13,
                "Backspace" => 8,
                "Escape" => 27,
                "Tab" => 9,
                _ => return,
            },
        };
        typed.borrow_mut().push(c);
    });
    canvas.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref()).unwrap();
    // The game window lives for the page's lifetime.
    cb.forget();
}

/// Resolves after `ms` milliseconds (the lobby wait polls the transport between turns of the
/// browser's event loop).
async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        web_sys::window().unwrap().set_timeout_with_callback_and_timeout_and_arguments_0(&res, ms).unwrap();
    });
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}

/// Join a native `scbw nethost` game (`code` = its `ip:port#certhash` LISTEN line): connect
/// over WebTransport, wait for the host's Start (slot, delay, game setup), then play it
/// through the same `NetGame` driver as `scbw netjoin` (src/net/live.rs). Resolves once the
/// game is running; `frames` > 0 logs `frame K hash H` to the console at that frame (the
/// headless-net check) and stops the game there.
#[wasm_bindgen]
pub async fn net_join(storm: Vec<u8>, archives: js_sys::Array, code: String, frames: u32, script: String) -> Result<(), JsError> {
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
    let mut g = live::decode_setup(&setup, slot).ok_or_else(|| err("bad game setup".into()))?;
    let table: Vec<(u8, u8)> = (0..8u8).filter(|&p| g.types[p as usize] == 2).enumerate().map(|(i, p)| (p, i as u8)).collect();
    let mut net = live::NetGame::new(Session::new(slot, &table, delay), tr, false, early).map_err(err)?;
    net.set_script(&script).map_err(err)?;
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
