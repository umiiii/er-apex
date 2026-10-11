<#
Start the game with the mod: Octane in first person at Limgrave's The First Step grace.
Needs build.ps1 and export-assets.ps1 run first. The game runs offline through me3, on the
test save ER0000_fuse.sl2 (backed up to scratch\saves before each start).

  pwsh play.ps1             gun damage x3 and no Jump Pad cooldown (play-test values)
  pwsh play.ps1 -Season3    Season 3 values
  pwsh play.ps1 -NoSpawn    no soldiers at the grace
  pwsh play.ps1 -Pad        play with a controller (by default the game sees no controller: keyboard and mouse prompts)

Quit: pwsh tools/dev/game.ps1 stop
#>
param([switch]$Season3, [switch]$NoSpawn, [switch]$Pad)
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Game = "$Root\tools\dev\game.ps1"

$need = [ordered]@{
    "$Root\target\x86_64-pc-windows-msvc\release\er_apex.dll" = 'pwsh build.ps1'
    "$Root\tools\bin\me3\bin\me3.exe"                          = 'me3 (see README, Building)'
    "$Root\apex-data\hud\octane"                               = 'pwsh export-assets.ps1'
    "$Root\apex-data\audio"                                    = 'pwsh export-assets.ps1'
    "$Root\er-data\json\NpcName_zhocn.json"                    = 'pwsh export-assets.ps1'
    "$Root\er-data\s4\fuse_er.anim"                            = 'pwsh export-assets.ps1'
}
foreach ($path in $need.Keys) {
    if (-not (Test-Path $path)) { throw "missing ${path}: run $($need[$path])" }
}
if (Get-Process eldenring -ErrorAction SilentlyContinue) {
    throw 'the game is already running: quit it first (pwsh tools/dev/game.ps1 stop)'
}

# the quick start writes to the test save; me3 copies it from the normal save on its first start
$save = Get-ChildItem "$env:APPDATA\EldenRing\*\ER0000_fuse.sl2" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($save) {
    $bak = "$Root\scratch\saves\before-play-$(Get-Date -Format yyyyMMdd-HHmmss)"
    New-Item -ItemType Directory -Force $bak | Out-Null
    Copy-Item "$($save.FullName)*" $bak
    "test save backed up to $bak"
}

$set = "quickboot = 1;qb_place = first_step;kcc = 1;fuse_model = 1;lethal_guard = 1;gun = 1;first_person = 1;camera_shoulder = 1;hud_hide = 1;virtual_pad = 0" +
    ";audio_dir = $Root\apex-data\audio;hud_dir = $Root\apex-data\hud\octane;npc_names = $Root\er-data\json\NpcName_zhocn.json"
if (-not $Season3) { $set += ';gun_damage_mult = 3;pad_cooldown = 0' }
if (-not $Pad) { $set += ';keyboard_only = 1' }
$arena = Test-Path "$Root\er-data\test_arena\package"
if ($arena) { & $Game install -Legend octane -Set $set } else { & $Game install -Legend octane -Set $set -NoArena }
& $Game start

# the quick start skips the title screen and continues the test save's last character (about 26 s)
$log = "$Root\scratch\mod\logs\er_apex.log"
$t0 = Get-Date
while (-not ((Test-Path $log) -and (Select-String -Quiet 'quickboot: done' $log))) {
    if (((Get-Date) - $t0).TotalSeconds -gt 300) { throw "the quick start did not finish in 300 s: see $log" }
    if (((Get-Date) - $t0).TotalSeconds -gt 60 -and -not (Get-Process eldenring -ErrorAction SilentlyContinue)) {
        throw "the game quit during the quick start: see $log"
    }
    Start-Sleep 1
}
"in the world after $([int]((Get-Date) - $t0).TotalSeconds) s"

if ($arena -and -not $NoSpawn) {
    & $Game spawn | Out-Null
    'three soldiers at the grace (again: pwsh tools/dev/game.ps1 spawn); do not shoot the friendly NPC about 10.5 m from the grace'
}
'click the game window to play. Back to the grace: pwsh tools/dev/game.ps1 cmd "warp 1042361951". Quit: pwsh tools/dev/game.ps1 stop'
