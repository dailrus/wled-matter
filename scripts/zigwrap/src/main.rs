//! Linker wrapper: `zig cc -target aarch64-linux-musl` minus the Cortex-A53 erratum flag that
//! rustc adds for aarch64 and zig rejects. Response files (@file) are expanded and filtered too.
use std::process::{exit, Command};

const DROP: &str = "-Wl,--fix-cortex-a53-843419";

fn main() {
    let mut args = Vec::new();
    for a in std::env::args().skip(1) {
        match a.strip_prefix('@') {
            Some(file) => {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                args.extend(text.lines().map(|l| l.trim_matches('"').to_string()));
            }
            None => args.push(a),
        }
    }
    args.retain(|a| a != DROP && !a.is_empty());
    let zig = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/zig-windows-x86_64-0.13.0/zig.exe");
    let status = Command::new(zig)
        .args(["cc", "-target", "aarch64-linux-musl"])
        .args(&args)
        .status()
        .expect("run zig");
    exit(status.code().unwrap_or(1));
}
