//! Link to WLED Animation (yam package dailrus/wled-anim, light music on the same Station):
//! the strip IP configured here is handed over to it, so a strip added for «Дом с Алисой» also
//! becomes the light-music target without retyping the address.
//!
//! Its own API (`POST /api/settings`, form `wled_ip=…&wled_leds=…`, port 8098) is preferred,
//! as it validates and applies at once; when its web daemon is not running, its settings file
//! (`key=value` lines, re-read by the animation every second) is edited the same way.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use log::{info, warn};
use serde_json::Value;

use crate::config::Config;
use crate::wled;

const PORT: u16 = 8098;

fn yam_root() -> PathBuf {
    PathBuf::from(std::env::var("YAM_ROOT").unwrap_or_else(|_| "/data/yam".into()))
}

fn settings_file() -> PathBuf {
    yam_root().join("var/wled_anim/settings")
}

pub fn installed() -> bool {
    yam_root().join("share/dailrus.wled-anim").is_dir()
}

/// The strip WLED Animation currently plays on ("" if none)
pub fn current_ip() -> String {
    let via_api = wled::http_to("127.0.0.1", PORT, "GET", "/api/settings", "text/plain", "")
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.pointer("/settings/wled_ip").and_then(Value::as_str).map(str::to_string));
    via_api.unwrap_or_else(|| {
        fs::read_to_string(settings_file())
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.trim().strip_prefix("wled_ip=").map(|v| v.trim().to_string()))
            .unwrap_or_default()
    })
}

/// Point WLED Animation at `host`, with the LED count read from the strip itself
pub fn set_strip(host: &str) -> Result<(), String> {
    if !installed() {
        return Err("WLED Animation не установлена".into());
    }
    let leds = wled::http(host, "GET", "/json/info", "")
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.pointer("/leds/count").and_then(Value::as_u64))
        .filter(|n| (1..=490).contains(n));
    let mut upd = vec![("wled_ip", host.to_string())];
    if let Some(n) = leds {
        upd.push(("wled_leds", n.to_string()));
    }

    let form: Vec<String> = upd.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let api = wled::http_to(
        "127.0.0.1",
        PORT,
        "POST",
        "/api/settings",
        "application/x-www-form-urlencoded",
        &form.join("&"),
    );
    match api {
        Ok(_) => {}
        Err(_) => write_settings(&upd).map_err(|e| format!("не удалось записать настройки: {e}"))?,
    }
    info!("WLED Animation now plays on {host} ({} LEDs)", leds.map_or("?".into(), |n| n.to_string()));
    Ok(())
}

/// Same edit as WLED Animation's own web.lua: replace `key=value` lines in place, append new keys
fn write_settings(upd: &[(&str, String)]) -> std::io::Result<()> {
    let path = settings_file();
    let old = fs::read_to_string(&path).unwrap_or_default();
    let mut seen = Vec::new();
    let mut lines: Vec<String> = old
        .lines()
        .map(|line| {
            let key = line.split('=').next().unwrap_or("").trim();
            match upd.iter().find(|(k, _)| *k == key) {
                Some((k, v)) => {
                    seen.push(*k);
                    format!("{k}={v}")
                }
                None => line.to_string(),
            }
        })
        .collect();
    for (k, v) in upd.iter().filter(|(k, _)| !seen.contains(k)) {
        lines.push(format!("{k}={v}"));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, lines.join("\n") + "\n")?;
    fs::rename(tmp, path)
}

/// After the strip list changed: a newly added strip becomes the light-music target, a strip
/// that was the target follows its new address, and if the target strip was deleted the first
/// remaining one takes over.
pub fn on_config_change(old: &Config, new: &Config) {
    if !installed() {
        return;
    }
    let (old, new) = (old.clone(), new.clone());
    thread::spawn(move || {
        let current = current_ip();
        let moved = new.strips.iter().find(|s| {
            old.strip(&s.id).is_some_and(|o| o.host == current && o.host != s.host)
        });
        let added = new.strips.iter().rev().find(|s| old.strip(&s.id).is_none());
        let target_deleted = old.strips.iter().any(|o| o.host == current)
            && !new.strips.iter().any(|s| s.host == current);
        let fallback = if target_deleted { new.strips.first() } else { None };
        if let Some(s) = moved.or(added).or(fallback) {
            if s.host != current {
                if let Err(e) = set_strip(&s.host) {
                    warn!("WLED Animation: {e}");
                }
            }
        }
    });
}

/// WLED Animation installed later, or never configured: give it the first strip
pub fn start_watch(load: impl Fn() -> Config + Send + 'static) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(60));
        if installed() && current_ip().is_empty() {
            if let Some(s) = load().strips.first() {
                if let Err(e) = set_strip(&s.host) {
                    warn!("WLED Animation: {e}");
                }
            }
        }
    });
}
