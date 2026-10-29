# Cross-builds the bridge for both station ABIs (aarch64 and armv7, static musl).
# Expects portable toolchains in ../tools: rustup/ + cargo/ (RUSTUP_HOME/CARGO_HOME), zig-windows-x86_64-0.13.0/.
$w = Split-Path -Parent $PSScriptRoot
$env:RUSTUP_HOME = "$w\tools\rustup"; $env:CARGO_HOME = "$w\tools\cargo"
$env:PATH = "$w\tools\cargo\bin;$w\tools\zig-windows-x86_64-0.13.0;$env:PATH"
$flags = "-C link-self-contained=no -C target-feature=+crt-static"
$env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER = "$w\scripts\zcc.bat"
$env:CC_armv7_unknown_linux_musleabihf = "$w\scripts\zcc.bat"
$env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS = $flags
# aarch64: rustc passes -Wl,--fix-cortex-a53-843419, which zig rejects; zigwrap drops it
cargo build --release --quiet --manifest-path "$w\scripts\zigwrap\Cargo.toml" --target-dir "$w\tools\zigwrap-target"
$zigwrap = "$w\tools\zigwrap-target\release\zigwrap.exe"
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = $zigwrap
$env:CC_aarch64_unknown_linux_musl = $zigwrap
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS = $flags
Set-Location "$w\bridge"
foreach ($t in 'aarch64-unknown-linux-musl', 'armv7-unknown-linux-musleabihf') {
    "== $t"
    cargo build --release --target $t 2>&1 | Select-String -Pattern "^error|-->|Finished" -Context 0, 6
    if ($LASTEXITCODE -ne 0) { exit 1 }
}
