//! The browser client of phase 5 (agent/areas/net.md): a `Transport` over the browser's
//! WebTransport, joining a native `scbw nethost`. The protocol (turn datagrams, acks,
//! redundancy) is `Transport` itself, shared with the native build; only the link differs:
//! a small JS glue opens `new WebTransport(url, { serverCertificateHashes })`, runs the
//! lobby exchange (`hello_msg` out, Start in) and pushes datagrams into the same `Inbound`
//! channel the native runtime thread feeds. `Transport::poll` stays non-blocking, once per
//! frame. (Raw JS rather than web-sys' WebTransport bindings: those need
//! `--cfg=web_sys_unstable_apis` and the stream readers are simpler to write in JS.)

use super::transport::{HOST, Inbound, Transport, hello_msg, parse_start, split_code};
use js_sys::Uint8Array;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function wt_connect(url, hash, hello, on_start, on_dgram, on_err) {
  // Rust's slices are views of wasm memory: copy what is used after an await.
  hash = hash.slice(); hello = hello.slice();
  let dw = null, wt = null, closed = false;
  const fail = e => on_err(String((e && e.message) || e));
  (async () => {
    try {
      wt = new WebTransport(url, { serverCertificateHashes: [{ algorithm: "sha-256", value: hash }] });
      await wt.ready;
      if (closed) return wt.close();
      const bi = await wt.createBidirectionalStream();
      const w = bi.writable.getWriter();
      await w.write(hello);
      const rd = bi.readable.getReader();
      let buf = new Uint8Array(0);
      const need = async n => {
        while (buf.length < n) {
          const { value, done } = await rd.read();
          if (done) throw new Error("lobby stream closed");
          const m = new Uint8Array(buf.length + value.length);
          m.set(buf); m.set(value, buf.length); buf = m;
        }
      };
      await need(4);
      const len = new DataView(buf.buffer, buf.byteOffset).getUint32(0, true);
      await need(4 + len);
      const body = buf.slice(4, 4 + len);
      dw = wt.datagrams.writable.getWriter();
      on_start(body, wt.datagrams.maxDatagramSize);
      const dr = wt.datagrams.readable.getReader();
      for (;;) {
        const { value, done } = await dr.read();
        if (done) throw new Error("connection closed");
        on_dgram(value);
      }
    } catch (e) { fail(e); }
  })();
  return {
    // A datagram that cannot go is lost like any other (the protocol resends); it does not
    // end the session.
    send(d) { if (dw && !closed) dw.write(d).catch(e => console.warn("datagram: " + ((e && e.message) || e))); },
    // A short grace so the last datagrams (the leave) go out before the session closes.
    close() { closed = true; setTimeout(() => { try { if (wt) wt.close(); } catch (e) {} }, 200); },
  };
}
"#)]
extern "C" {
    type Link;
    fn wt_connect(url: &str, hash: &[u8], hello: &[u8], on_start: &Closure<dyn FnMut(Uint8Array, u32)>, on_dgram: &Closure<dyn FnMut(Uint8Array)>, on_err: &Closure<dyn FnMut(String)>) -> Link;
    #[wasm_bindgen(method)]
    fn send(this: &Link, d: &Uint8Array);
    #[wasm_bindgen(method)]
    fn close(this: &Link);
}

/// The link of a `Transport`: closing the session when the transport is dropped makes the
/// host see the player lost at once (instead of after its idle timeout).
struct Owned(Link);

impl Drop for Owned {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl Transport {
    /// Browser `Transport::join`: connect to `target` (`ip:port#certhash`) and say hello as
    /// `me`; the connection completes in the background and Start arrives through `poll`.
    pub fn join_web(target: &str, me: u8, first: u32) -> Result<Transport, String> {
        let (addr, hash, game) = split_code(target)?;
        let (itx, rx) = mpsc::channel();
        let max_dgram = Arc::new(AtomicUsize::new(1000));
        let (tx1, md) = (itx.clone(), max_dgram.clone());
        let on_start = Closure::<dyn FnMut(Uint8Array, u32)>::new(move |body: Uint8Array, max: u32| {
            if max > 0 {
                md.fetch_min(max as usize, Ordering::Relaxed);
            }
            let body = body.to_vec();
            let m = match body.split_first() {
                Some((&ty, rest)) => parse_start(ty, rest),
                None => Err("empty lobby message".into()),
            };
            let _ = tx1.send(m.unwrap_or_else(Inbound::Error));
        });
        let tx2 = itx.clone();
        let on_dgram = Closure::<dyn FnMut(Uint8Array)>::new(move |d: Uint8Array| {
            let _ = tx2.send(Inbound::Dgram { from: HOST, data: d.to_vec() });
        });
        let on_err = Closure::<dyn FnMut(String)>::new(move |e: String| {
            let _ = itx.send(Inbound::Error(e));
        });
        let link = wt_connect(&format!("https://{addr}/"), &hash, &hello_msg(me, game), &on_start, &on_dgram, &on_err);
        // The callbacks live as long as the page.
        on_start.forget();
        on_dgram.forget();
        on_err.forget();
        let link = Owned(link);
        let out = Box::new(move |_to: u8, d: Vec<u8>| link.0.send(&Uint8Array::from(&d[..])));
        Ok(Transport::new(me, false, vec![HOST], first, rx, out, max_dgram))
    }
}
