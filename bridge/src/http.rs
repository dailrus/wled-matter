//! Web UI + JSON API (std TcpListener, one thread per connection). The page itself is
//! `web/index.html`, embedded at build time. The same page is also shown as a tab of the Luna
//! panel (port 8099, see the Luna plugin in the package), hence the permissive CORS headers.
//!
//!   GET  /                 the UI
//!   GET  /api/state        config + running strips (codes, pairing state)
//!   GET  /api/qr/<id>.svg  pairing QR of a strip
//!   GET  /api/wled?host=   check a WLED device: name, version, LED count
//!   GET  /api/nodes?host=  other WLED devices known to that one (WLED node discovery)
//!   POST /api/config       replace the strip list, restarts the strips
//!   POST /api/unpair       forget a strip's pairing (it gets a new code)
//!   POST /api/wled-anim    make a strip the target of WLED Animation (light music)

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use log::{info, warn};
use rs_matter::pairing::qr::Qr;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::Config;
use crate::{wled, wled_anim};
use crate::{Ctx, Request, RESTARTING, STATUS, VERSION};

const INDEX_HTML: &str = include_str!("../web/index.html");
const MAX_BODY: usize = 64 * 1024;

pub fn start(ctx: &'static Ctx, port: u16, tx: Sender<Request>) {
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            warn!("Web UI disabled, cannot listen on :{port}: {e}");
            return;
        }
    };
    info!("Web UI on http://0.0.0.0:{port}/");
    let tx: &'static Mutex<Sender<Request>> = Box::leak(Box::new(Mutex::new(tx)));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = thread::Builder::new()
                .stack_size(128 * 1024)
                .spawn(move || {
                    if let Err(e) = handle(ctx, tx, stream) {
                        log::debug!("HTTP: {e}");
                    }
                });
        }
    });
}

struct Response {
    status: &'static str,
    ctype: &'static str,
    body: Vec<u8>,
}

fn json_resp(v: Value) -> Response {
    Response { status: "200 OK", ctype: "application/json; charset=utf-8", body: v.to_string().into_bytes() }
}

fn ok() -> Response {
    json_resp(json!({ "ok": true }))
}

fn fail(msg: impl Into<String>) -> Response {
    json_resp(json!({ "ok": false, "error": msg.into() }))
}

fn not_found() -> Response {
    Response { status: "404 Not Found", ctype: "text/plain", body: b"not found".to_vec() }
}

fn handle(ctx: &Ctx, tx: &Mutex<Sender<Request>>, stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len.min(MAX_BODY)];
    reader.read_exact(&mut body)?;

    let resp = route(ctx, tx, method, path, query, &body);
    let mut out = stream;
    write!(
        out,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        resp.status,
        resp.ctype,
        resp.body.len()
    )?;
    out.write_all(&resp.body)
}

fn route(ctx: &Ctx, tx: &Mutex<Sender<Request>>, method: &str, path: &str, query: &str, body: &[u8]) -> Response {
    match (method, path) {
        ("OPTIONS", _) => Response { status: "204 No Content", ctype: "text/plain", body: Vec::new() },
        ("GET", "/") | ("GET", "/index.html") => Response {
            status: "200 OK",
            ctype: "text/html; charset=utf-8",
            body: INDEX_HTML.as_bytes().to_vec(),
        },
        ("GET", "/api/state") => state(ctx),
        ("GET", p) if p.starts_with("/api/qr/") && p.ends_with(".svg") => {
            qr_svg(&p["/api/qr/".len()..p.len() - 4])
        }
        ("GET", "/api/wled") => wled_info(&query_param(query, "host").unwrap_or_default()),
        ("GET", "/api/nodes") => wled_nodes(&query_param(query, "host").unwrap_or_default()),
        ("POST", "/api/config") => save_config(ctx, tx, body),
        ("POST", "/api/unpair") => {
            #[derive(Deserialize)]
            struct Req {
                id: String,
            }
            match serde_json::from_slice::<Req>(body) {
                Ok(r) if STATUS.lock().unwrap().iter().any(|s| s.id == r.id) => {
                    RESTARTING.store(true, Ordering::SeqCst);
                    let _ = tx.lock().unwrap().send(Request::Unpair(r.id));
                    ok()
                }
                Ok(_) => fail("нет такой ленты"),
                Err(e) => fail(e.to_string()),
            }
        }
        ("POST", "/api/wled-anim") => {
            #[derive(Deserialize)]
            struct Req {
                id: String,
            }
            let cfg = Config::load(&ctx.base, &ctx.default_host);
            match serde_json::from_slice::<Req>(body) {
                Ok(r) => match cfg.strip(&r.id) {
                    Some(s) => match wled_anim::set_strip(&s.host) {
                        Ok(()) => ok(),
                        Err(e) => fail(e),
                    },
                    None => fail("нет такой ленты"),
                },
                Err(e) => fail(e.to_string()),
            }
        }
        _ => not_found(),
    }
}

fn state(ctx: &Ctx) -> Response {
    let cfg = Config::load(&ctx.base, &ctx.default_host);
    let devices = STATUS.lock().unwrap().clone();
    let installed = wled_anim::installed();
    let anim_ip = if installed { wled_anim::current_ip() } else { String::new() };
    json_resp(json!({
        "version": VERSION,
        "strips": cfg.strips,
        "devices": devices,
        "wled_anim": { "installed": installed, "ip": anim_ip },
        "restarting": RESTARTING.load(Ordering::SeqCst),
    }))
}

fn save_config(ctx: &Ctx, tx: &Mutex<Sender<Request>>, body: &[u8]) -> Response {
    let cfg: Config = match serde_json::from_slice(body) {
        Ok(c) => c,
        Err(e) => return fail(format!("неверные данные: {e}")),
    };
    if let Err(e) = cfg.validate() {
        return fail(e);
    }
    let old = Config::load(&ctx.base, &ctx.default_host);
    if let Err(e) = cfg.save(&ctx.base) {
        return fail(format!("не удалось сохранить: {e}"));
    }
    wled_anim::on_config_change(&old, &cfg);
    info!("Config saved: {} strips", cfg.strips.len());
    RESTARTING.store(true, Ordering::SeqCst);
    let _ = tx.lock().unwrap().send(Request::Reload);
    ok()
}

/// GET a JSON document from a WLED device given by a user-supplied address
fn wled_json(host: &str, path: &str) -> Result<Value, String> {
    let host = host.trim();
    let valid = !host.is_empty()
        && host.len() <= 64
        && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c));
    if !valid {
        return Err("неверный адрес".into());
    }
    let text = wled::http(host, "GET", path, "").map_err(|_| "WLED не отвечает".to_string())?;
    serde_json::from_str(&text).map_err(|_| "WLED не отвечает".to_string())
}

fn wled_info(host: &str) -> Response {
    match wled_json(host, "/json/info") {
        Ok(info) => json_resp(json!({
            "ok": true,
            "name": info.get("name").cloned().unwrap_or(Value::Null),
            "ver": info.get("ver").cloned().unwrap_or(Value::Null),
            "leds": info.pointer("/leds/count").cloned().unwrap_or(Value::Null),
        })),
        Err(e) => fail(e),
    }
}

fn wled_nodes(host: &str) -> Response {
    match wled_json(host, "/json/nodes") {
        Ok(v) => {
            let nodes: Vec<Value> = v
                .get("nodes")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|n| json!({ "name": n.get("name"), "ip": n.get("ip") }))
                        .collect()
                })
                .unwrap_or_default();
            json_resp(json!({ "ok": true, "nodes": nodes }))
        }
        Err(e) => fail(e),
    }
}

fn qr_svg(id: &str) -> Response {
    let Some(text) = STATUS.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.qr.clone()) else {
        return not_found();
    };
    let mut tmp = [0u8; 4096];
    let mut out = [0u8; 4096];
    let Ok(qr) = Qr::compute(&text, &mut tmp, &mut out) else {
        return not_found();
    };
    let size = qr.size() as i32;
    let border = 2;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if qr.get_module(x, y) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    let dim = size + 2 * border;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {dim} {dim}\" shape-rendering=\"crispEdges\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
    );
    Response { status: "200 OK", ctype: "image/svg+xml", body: svg.into_bytes() }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}
