//! Minimal WLED JSON API client (plain HTTP over std TcpStream, no deps).
//!
//! Commands are queued to a worker thread so a slow/offline WLED never blocks the Matter loop.
//! A poller thread keeps `snapshot()` up to date with the real device state.
//! One client per WLED host, kept across node restarts (see `Wled::get`).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use log::{debug, info, warn};

const TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Poll results this close to one of our commands may predate it and are dropped
const POLL_QUIET: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WledState {
    pub on: bool,
    pub bri: u8,
}

pub struct Wled {
    host: String,
    tx: Mutex<Sender<(&'static str, String)>>,
    /// Last state known to be on the device (polled, or sent by us); used to skip no-op commands.
    state: Mutex<Option<WledState>>,
    /// Last state actually read back from the device. Mirrored into Matter; must not include our
    /// own in-flight writes, or a level transition would be cut short by its own echo.
    polled: Mutex<Option<WledState>>,
    last_cmd: Mutex<Option<Instant>>,
}

static CLIENTS: Mutex<Vec<&'static Wled>> = Mutex::new(Vec::new());

impl Wled {
    /// The client for `host`; started on first use and kept for the process lifetime, so node
    /// restarts after a config change don't pile up poller threads.
    pub fn get(host: &str) -> &'static Wled {
        let mut clients = CLIENTS.lock().unwrap();
        if let Some(w) = clients.iter().find(|w| w.host == host) {
            return w;
        }
        let w = Self::start(host.to_string());
        clients.push(w);
        w
    }

    fn start(host: String) -> &'static Wled {
        let (tx, rx) = channel::<(&'static str, String)>();
        let wled: &'static Wled = Box::leak(Box::new(Wled {
            host,
            tx: Mutex::new(tx),
            state: Mutex::new(None),
            polled: Mutex::new(None),
            last_cmd: Mutex::new(None),
        }));

        thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                // Coalesce bursts (e.g. slider drags): only the newest value per key matters
                // Give related commands (e.g. Off + its level drop) a moment to arrive together
                thread::sleep(Duration::from_millis(50));
                let mut fields = vec![first];
                while let Ok((k, v)) = rx.try_recv() {
                    fields.retain(|(ek, _)| *ek != k);
                    fields.push((k, v));
                }
                // Matter's Off moves the level to minimum; WLED keeps brightness while off,
                // so dropping it avoids the strip coming back at 1% when switched on elsewhere
                if fields.iter().any(|(k, v)| *k == "on" && v == "false") {
                    fields.retain(|(k, _)| *k != "bri");
                }
                let body: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
                let body = format!("{{{}}}", body.join(","));
                match wled.request("POST", &body) {
                    Ok(_) => info!("WLED <- {body}"),
                    Err(e) => warn!("WLED POST {body} failed: {e}"),
                }
            }
        });

        thread::spawn(move || loop {
            let started = Instant::now();
            match wled.request("GET", "").map(|r| parse_state(&r)) {
                Ok(Some(_)) if wled.recent_cmd(started) => debug!("WLED poll skipped: command in flight"),
                Ok(Some(s)) => {
                    let mut polled = wled.polled.lock().unwrap();
                    if *polled != Some(s) {
                        debug!("WLED -> {s:?}");
                        *polled = Some(s);
                    }
                    *wled.state.lock().unwrap() = Some(s);
                }
                Ok(None) => warn!("WLED: unparsable state"),
                Err(e) => debug!("WLED poll failed: {e}"),
            }
            thread::sleep(POLL_INTERVAL);
        });

        wled
    }

    pub fn snapshot(&self) -> Option<WledState> {
        *self.polled.lock().unwrap()
    }

    pub fn set_on(&self, on: bool) {
        if self.update(|s| s.on = on) {
            self.send("on", on.to_string());
        }
    }

    pub fn set_bri(&self, bri: u8) {
        if self.update(|s| s.bri = bri) {
            self.send("bri", bri.to_string());
        }
    }

    /// Picking a colour means a plain colour, so this also leaves any running effect
    pub fn set_rgb(&self, (r, g, b): (u8, u8, u8)) {
        self.send("seg", format!("{{\"col\":[[{r},{g},{b}]],\"fx\":0}}"));
    }


    /// Apply a change to the known state; returns false if the device already is in that state.
    fn update(&self, f: impl FnOnce(&mut WledState)) -> bool {
        let mut cur = self.state.lock().unwrap();
        let mut next = cur.unwrap_or_default();
        f(&mut next);
        if *cur == Some(next) {
            return false;
        }
        *cur = Some(next);
        true
    }

    fn recent_cmd(&self, poll_started: Instant) -> bool {
        self.last_cmd
            .lock()
            .unwrap()
            .is_some_and(|t| t + POLL_QUIET > poll_started)
    }

    fn send(&self, key: &'static str, value: String) {
        *self.last_cmd.lock().unwrap() = Some(Instant::now());
        let _ = self.tx.lock().unwrap().send((key, value));
    }

    fn request(&self, method: &str, body: &str) -> std::io::Result<String> {
        http(&self.host, method, "/json/state", body)
    }
}

/// One plain HTTP/1.1 request to a WLED device; returns the response body.
pub fn http(host: &str, method: &str, path: &str, body: &str) -> std::io::Result<String> {
    http_to(host, 80, method, path, "application/json", body)
}

/// One plain HTTP/1.1 request; returns the response body of a 200 reply.
pub fn http_to(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    ctype: &str,
    body: &str,
) -> std::io::Result<String> {
    let addr = (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other("WLED host not resolvable"))?;
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))?;
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    // WLED keeps the socket open despite `Connection: close`, so read by Content-Length
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let body_start = loop {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break Some(pos + 4);
        }
    }
    .ok_or_else(|| std::io::Error::other("WLED: truncated response"))?;
    let head = String::from_utf8_lossy(&buf[..body_start]).to_string();
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Err(std::io::Error::other(head.lines().next().unwrap_or("").to_string()));
    }
    let len = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while buf.len() < body_start + len {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(String::from_utf8_lossy(&buf[body_start..]).to_string())
}

/// WLED returns top-level `on`/`bri` first, before the nested segment objects.
fn parse_state(json: &str) -> Option<WledState> {
    let value = |key: &str| {
        let rest = &json[json.find(key)? + key.len()..];
        let end = rest.find([',', '}'])?;
        Some(rest[..end].trim().to_string())
    };
    Some(WledState {
        on: value("\"on\":")? == "true",
        bri: value("\"bri\":")?.parse().ok()?,
    })
}
