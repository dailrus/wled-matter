# Packs one .yampkg per yam platform (yam checks manifest.target == station profile).
# aarch64 binary for 64-bit stations; armv7 for the 32-bit ABI profiles and the first Mini
# (yam labels it aarch64, but Luna ships armv7 for it — the static armv7 binary runs either way).
# Uses yam-publish built from tag yam-v0.2.2: newer publishers emit manifest fields yam 0.2.2 rejects.
$w = Split-Path -Parent $PSScriptRoot
$ver = (Select-String -Path "$w\pkg\yam.yaml" -Pattern '^version: (.+)$').Matches[0].Groups[1].Value
$publish = "$w\tools\yam-0.2.2\target\release\yam-publish.exe"
$profiles = "$w\tools\yam-0.2.2\platforms"
$bins = @{
    aarch64 = "$w\bridge\target\aarch64-unknown-linux-musl\release\wled_matter"
    armv7   = "$w\bridge\target\armv7-unknown-linux-musleabihf\release\wled_matter"
}
$armv7 = @('mango', 'jbl_link_music', 'yandexmini')
$platforms = 'yandexmini_2', 'yandexmini', 'bergamot', 'pickle', 'yandexmicro', 'plum', 'yandexplum',
    'cucumber', 'yandexmidi', 'yandexstation', 'yandexstation_2', 'orion', 'saturn', 'mango', 'jbl_link_music'

Copy-Item "$w\bridge\web\index.html" "$w\pkg\luna\index.html" -Force
$out = "$w\dist\$ver"
New-Item -ItemType Directory -Force $out | Out-Null
Remove-Item "$out\*.yampkg" -ErrorAction SilentlyContinue
foreach ($p in $platforms) {
    $abi = if ($armv7 -contains $p) { 'armv7' } else { 'aarch64' }
    Copy-Item $bins[$abi] "$w\pkg\bin\wled_matterd" -Force
    & $publish pack --source "$w\pkg" --platform $p --profiles $profiles --output "$out\wled-matter-$p.yampkg" 2>&1
    if ($LASTEXITCODE -ne 0) { "FAILED: $p"; exit 1 }
    "{0,-16} {1,-8} {2,8:N0} B" -f $p, $abi, (Get-Item "$out\wled-matter-$p.yampkg").Length
}
# Постоянная ссылка из README и прошлых версий вела на пакет для Mini 2
Copy-Item "$out\wled-matter-yandexmini_2.yampkg" "$out\wled-matter.yampkg" -Force
