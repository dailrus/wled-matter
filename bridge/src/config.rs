//! Bridge configuration (`<storage>/config.json`) and per-device commissioning data.
//!
//! Layout of the storage dir:
//!   config.json          strips, edited from the web UI
//!   <device id>/         rs-matter KV store of that device (fabrics = its Yandex pairing)
//!   <device id>/comm     "<passcode> <discriminator>" of that device

use std::fs;
use std::path::{Path, PathBuf};

use log::info;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Strip {
    pub id: String,
    pub name: String,
    pub host: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub strips: Vec<Strip>,
}

impl Config {
    pub fn load(base: &Path, default_host: &str) -> Config {
        let path = base.join("config.json");
        if let Ok(text) = fs::read_to_string(&path) {
            match serde_json::from_str(&text) {
                Ok(cfg) => return cfg,
                Err(e) => log::error!("{}: {e}; starting with defaults", path.display()),
            }
        }
        let cfg = Config {
            strips: vec![Strip {
                id: "s1".into(),
                name: "WLED".into(),
                host: default_host.into(),
            }],
        };
        migrate_single_device(base);
        let _ = cfg.save(base);
        cfg
    }

    pub fn save(&self, base: &Path) -> std::io::Result<()> {
        let tmp = base.join("config.json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, base.join("config.json"))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.strips.len() > 16 {
            return Err("не больше 16 лент".into());
        }
        for (i, s) in self.strips.iter().enumerate() {
            if s.name.trim().is_empty() || s.host.trim().is_empty() {
                return Err(format!("лента {}: пустое название или адрес", s.id));
            }
            let valid = !s.id.is_empty()
                && s.id.len() <= 16
                && s.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !valid {
                return Err(format!("недопустимый id «{}»", s.id));
            }
            if self.strips[..i].iter().any(|o| o.id == s.id) {
                return Err(format!("повторяющийся id «{}»", s.id));
            }
            if self.strips[..i].iter().any(|o| o.host == s.host) {
                return Err(format!("адрес {} указан дважды", s.host));
            }
        }
        Ok(())
    }

    pub fn strip(&self, id: &str) -> Option<&Strip> {
        self.strips.iter().find(|s| s.id == id)
    }
}

/// Before multi-device support the single strip kept its KV files directly in the storage dir
/// and used the SDK test passcode; move them under `s1/` so its Yandex pairing survives.
fn migrate_single_device(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else { return };
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n != "config.json"))
        .collect();
    if files.is_empty() {
        return;
    }
    let dir = base.join("s1");
    let _ = fs::create_dir_all(&dir);
    for f in files {
        let _ = fs::rename(&f, dir.join(f.file_name().unwrap()));
    }
    let _ = fs::write(dir.join("comm"), "20202021 3840");
    info!("Migrated the existing pairing to device s1");
}

/// Passcode + discriminator of a device, generated once and kept with its pairing.
pub fn comm_data(dir: &Path) -> (u32, u16) {
    let file = dir.join("comm");
    if let Ok(text) = fs::read_to_string(&file) {
        let mut it = text.split_whitespace().map(|v| v.parse::<u32>().ok());
        if let (Some(Some(p)), Some(Some(d))) = (it.next(), it.next()) {
            return (p, d as u16);
        }
    }
    let (p, d) = random_comm();
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(&file, format!("{p} {d}"));
    (p, d)
}

fn random_comm() -> (u32, u16) {
    use rand::RngExt as _;
    // Matter core spec 5.1.7.1: these passcodes are forbidden
    const INVALID: [u32; 12] = [
        0, 11111111, 22222222, 33333333, 44444444, 55555555, 66666666, 77777777, 88888888,
        99999999, 12345678, 87654321,
    ];
    let mut rng = rand::rng();
    loop {
        let p = rng.random_range(1..=99_999_998u32);
        if !INVALID.contains(&p) {
            return (p, rng.random_range(0..4096u16));
        }
    }
}
