<#
Make the game assets (apex-data\, er-data\) from your own Apex Legends and ELDEN RING installs.
Asks for the two game folders (found through Steam when it can), checks the tool chain, then runs
the seven steps in order (about an hour, about 45 GB). Step 4 starts the game once.

  pwsh export-assets.ps1                              ask for the folders, run all steps
  pwsh export-assets.ps1 -ApexDir <dir> -EldenRingDir <dir>   no questions
  pwsh export-assets.ps1 -NoArena                     no test area (game.ps1 spawn then does nothing)
  pwsh export-assets.ps1 -Force                       make everything again, even what is already there
  pwsh export-assets.ps1 -From 5 -Force               make steps 5 to 7 again

A command whose outputs are already there is skipped, so after a failure just run it again.

The game folders are only read, never written.
#>
param(
    [string]$ApexDir,
    [string]$EldenRingDir,
    [ValidateRange(1, 8)][int]$From = 1,
    [ValidateRange(1, 8)][int]$To = 7,
    [switch]$NoArena,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'
$Root = $PSScriptRoot
$Game = "$Root\tools\dev\game.ps1"
Set-Location $Root

# ---- game folders ----

function Find-SteamGame([string]$sub) {
    $steam = (Get-ItemProperty 'HKCU:\Software\Valve\Steam' -ErrorAction SilentlyContinue).SteamPath
    if (-not $steam) { return $null }
    $libs = @($steam)
    $vdf = Join-Path $steam 'steamapps\libraryfolders.vdf'
    if (Test-Path $vdf) {
        $libs += [regex]::Matches((Get-Content $vdf -Raw), '"path"\s+"([^"]+)"') | ForEach-Object { $_.Groups[1].Value -replace '\\\\', '\' }
    }
    foreach ($lib in $libs | Select-Object -Unique) {
        $dir = Join-Path $lib "steamapps\common\$sub"
        if (Test-Path $dir) { return $dir }
    }
}

function Get-GameDir([string]$given, [string]$what, [string]$default, [string]$mark) {
    $ok = { param($d) $d -and (Test-Path (Join-Path $d $mark)) }
    if ($given) {
        if (& $ok $given) { return (Resolve-Path $given).Path }
        throw "$given is not $what (no $mark in it)"
    }
    while ($true) {
        $in = Read-Host "$what$(if ($default) { " [$default]" })"
        $dir = if ($in.Trim()) { $in.Trim().Trim('"') } else { $default }
        if (& $ok $dir) { return (Resolve-Path $dir).Path }
        Write-Host "  no $mark in '$dir': give the folder that has it" -ForegroundColor Yellow
    }
}

$apexDefault = @($env:APEX_LEGENDS_DIR, (Find-SteamGame 'Apex Legends')) | Where-Object { $_ } | Select-Object -First 1
$erDefault = @($env:ELDEN_RING_DIR, (Find-SteamGame 'ELDEN RING\Game')) | Where-Object { $_ } | Select-Object -First 1
$env:APEX_LEGENDS_DIR = Get-GameDir $ApexDir 'Apex Legends install folder' $apexDefault 'paks\Win64'
$env:ELDEN_RING_DIR = Get-GameDir $EldenRingDir 'ELDEN RING Game folder' $erDefault 'eldenring.exe'
"Apex Legends: $env:APEX_LEGENDS_DIR"
"ELDEN RING:   $env:ELDEN_RING_DIR"

# ---- tool chain ----

. "$Root\tools\dev\toolchain.ps1"

Write-Host ''
Check 'python >= 3.11' {
    # no double quotes in the code: Windows PowerShell drops them on the way to python
    $v = Py "import sys; print('.'.join(map(str, sys.version_info[:3])))"
    if ($v -and [version]$v -ge [version]'3.11') { $v }
} 'install Python 3.11 or later and put it on PATH'
Check 'numpy' { Py 'import numpy; print(numpy.__version__)' } 'pip install numpy'
Check 'pillow' { Py 'import PIL; print(PIL.__version__)' } 'pip install pillow'
Check 'matplotlib' { Py 'import matplotlib; print(matplotlib.__version__)' } 'pip install matplotlib'
Test-Rust
Check 'MSBuild (Visual Studio)' { Get-VisualStudio 'Microsoft.Component.MSBuild' } 'install Visual Studio 2022 with "Desktop development with C++" (build_rsx.py builds RSX with it)'
Check '.NET SDK 8.0.4xx >= 8.0.425' {
    # global.json pins 8.0.425 with rollForward latestPatch: the same feature band, that patch or later
    & dotnet --list-sdks 2>$null | ForEach-Object { ($_ -split ' ')[0] } |
        Where-Object { $_ -match '^8\.0\.4(\d\d)$' -and [int]$Matches[1] -ge 25 } | Select-Object -Last 1
} 'install .NET SDK 8.0.425 or a later 8.0.4xx'
Check 'me3' { $v = & "$Root\tools\bin\me3\bin\me3.exe" --version 2>$null; if ($LASTEXITCODE -eq 0) { ("$v" -split ' ')[-1] } } 'unzip the me3 Windows release into tools\bin\me3\ (see README)'
Check 'texconv' { Have 'tools\bin\texconv\texconv.exe' } 'put texconv.exe from DirectXTex into tools\bin\texconv\ (see README)'
Check 'rsx 2.3.0' { Have 'tools\rsx-2.3.0\rsx_nogui.exe' } 'unzip the RSX 2.3.0 release into tools\rsx-2.3.0\ (see README)'
Assert-Toolchain

# ---- steps ----

# A command is skipped when its outputs are already there: made by an earlier run of this script
# (scratch\export-assets\<command>.done), or by hand before this script existed. A command that
# started and did not finish (<command>.started) runs again even when some of its outputs exist.
$Stamps = "$Root\scratch\export-assets"
$script:made = 0; $script:skipped = 0

function Make([string[]]$Out, [string]$Key, [scriptblock]$Body) {
    $stamp = Join-Path $Stamps ($Key -replace '[^A-Za-z0-9.-]+', '_').Trim('_')
    $have = -not ($Out | Where-Object { -not (Test-Path (Join-Path $Root $_)) })
    if (-not $Force -and $have -and ((Test-Path "$stamp.done") -or -not (Test-Path "$stamp.started"))) {
        Write-Host "skip $Key (have $($Out[0]))" -ForegroundColor DarkGray
        New-Item -ItemType Directory -Force $Stamps | Out-Null
        if (-not (Test-Path "$stamp.done")) { Set-Content "$stamp.done" "found $(Get-Date -Format s)" }
        $script:skipped++
        return
    }
    New-Item -ItemType Directory -Force $Stamps | Out-Null
    Remove-Item "$stamp.done" -ErrorAction SilentlyContinue
    Set-Content "$stamp.started" (Get-Date -Format s)
    & $Body
    Move-Item -Force "$stamp.started" "$stamp.done"
    $script:made++
}

# Run <outputs> <command...>: the outputs are relative to the repository root
function Run {
    $out = @($args[0])
    # an array even for one argument: Windows PowerShell splats a lone string character by character
    $cmd = @($args | Select-Object -Skip 1)
    $line = $cmd -join ' '
    Make $out $line {
        Write-Host "> $line" -ForegroundColor DarkGray
        $rest = @($cmd | Select-Object -Skip 1)
        & $cmd[0] @rest
        if ($LASTEXITCODE -ne 0) { throw "failed ($LASTEXITCODE): $line" }
    }
}

function Step([int]$n, [string]$name, [scriptblock]$body) {
    if ($n -lt $From -or $n -gt $To) { return }
    $t0 = Get-Date
    Write-Host "`n== step $n/8: $name ==" -ForegroundColor Cyan
    try { & $body } catch { Write-Host "step $n failed; fix the cause and run export-assets.ps1 again" -ForegroundColor Red; throw }
    Write-Host "== step $n done in $([int]((Get-Date) - $t0).TotalMinutes) min ==" -ForegroundColor Cyan
}

Step 1 'Apex exports (about 31 min)' {
    Run 'tools\apexassets\rsx_source\bin\Release_NoGui\rsx.exe' python tools/apexassets/build_rsx.py
    Run 'apex-data\export\weapon\mp_weapon_frag_grenade.txt', 'apex-data\export\localization' python tools/apexdata/rsx_export.py
    Run 'apex-data\fuse_data.json', 'apex-data\fuse_names.md' python tools/apexdata/extract_fuse.py
    Run 'apex-data\assets\verification.json', 'apex-data\assets\fuse_skeleton.json' python tools/apexassets/export_assets.py
    Run 'apex-data\assets\octane\verification.json' python tools/apexassets/export_assets.py --legend octane
    Run 'apex-data\pov\smd\mdl', 'apex-data\pov\smd\animrig' python tools/apexpov/export_pov.py
    Run 'apex-data\weapons\defender\pov\smd\mdl', 'apex-data\weapons\defender\pov\smd\animrig' python tools/apexpov/export_defender_pov.py
    Run 'apex-data\assets\frag_grenade\verification.json' python tools/apexassets/frag_grenade_assets.py
    Run 'apex-data\assets\battery\verification.json' python tools/apexassets/battery_assets.py
    Run 'apex-data\assets\defender\verification.json' python tools/apexassets/defender_assets.py
    Run 'apex-data\assets\frag\verification.json' python tools/apexassets/frag_assets.py
}

Step 2 'HUD and sounds (about 5 min)' {
    Run 'apex-data\hud\octane\hud_pack.json' python tools/apexhud/export_hud.py --legend octane
    Run 'apex-data\hud\octane\extra_images.json' python tools/apexhud/export_extra.py --legend octane
    Run 'apex-data\audio\manifest.json' python tools/fuseaudio/export_audio.py
    Run 'apex-data\audio\octane\manifest.json' python tools/fuseaudio/export_audio.py --set octane
    Run 'apex-data\audio\defender\manifest.json' python tools/fuseaudio/export_audio.py --set defender
    Run 'apex-data\audio\frag_grenade\manifest.json' python tools/fuseaudio/export_audio.py --set frag
}

Step 3 'ELDEN RING extraction (about 2 min)' {
    Push-Location tools/erdata
    try {
        Run 'tools\third_party\SoulsFormatsNEXT', 'tools\third_party\HKLib', 'tools\third_party\ERDATA_PATCHES.md' python scripts/setup_dependencies.py
        Run 'tools\erdata\erextract\target\x86_64-pc-windows-msvc\release\erextract.exe' cargo build --release --manifest-path erextract/Cargo.toml
        Run 'tools\erdata\ertool\bin\Release\net8.0\ertool.exe' dotnet build ertool/ertool.csproj -c Release
        Run 'er-data\json\s2a_summary.md', 'er-data\skeleton' python scripts/run_s2a.py
        Run 'er-data\s3\roundtrip_comparison.json', 'er-data\s3\inputs\material\allmaterial.matbinbnd.dcx' python scripts/s3a_roundtrip.py
        Run 'er-data\s3\material_evidence\provenance.json' python scripts/s3a_material_evidence.py
    } finally { Pop-Location }
    # NPC names for the kill feed
    Make 'er-data\json\NpcName_zhocn.json' 'ertool fmg NpcName' {
        $fmg = Get-ChildItem er-data\extract\msg\zhocn\item.msgbnd -Recurse -Filter NpcName.fmg | Select-Object -First 1
        if (-not $fmg) { throw 'no NpcName.fmg in er-data\extract\msg\zhocn\item.msgbnd' }
        & "$Root\tools\erdata\ertool\bin\Release\net8.0\ertool.exe" fmg $fmg.FullName --json --out er-data\json\NpcName_zhocn.json
        if ($LASTEXITCODE -ne 0) { throw "ertool fmg failed ($LASTEXITCODE)" }
    }
    if (-not $NoArena) {
        Run 'er-data\test_arena\package\map\mapstudio\m60_42_36_00.msb.dcx', 'er-data\test_arena\package\event\m60_42_36_00.emevd.dcx' python tools/testarena/run.py build
    }
}

Step 4 'the skeleton from the running game (about 1 min)' {
    Make 'er-data\runtime\c0000_runtime.json', 'er-data\skeleton\c0000_live_skeleton.json' 'game skeleton c0000_runtime.json' {
        if (Get-Process eldenring -ErrorAction SilentlyContinue) { throw 'the game is running: quit it first (pwsh tools/dev/game.ps1 stop)' }
        if (-not (Test-Path "$Root\target\x86_64-pc-windows-msvc\release\er_apex.dll")) { & "$Root\build.ps1" }
        $save = Get-ChildItem "$env:APPDATA\EldenRing\*\ER0000_fuse.sl2" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($save) {
            $bak = "$Root\scratch\saves\before-export-$(Get-Date -Format yyyyMMdd-HHmmss)"
            New-Item -ItemType Directory -Force $bak | Out-Null
            Copy-Item "$($save.FullName)*" $bak
        }
        # no Octane models yet: first person stays off
        & $Game install -Set 'quickboot = 1;qb_place = first_step' -NoArena
        & $Game start
        try {
            $log = "$Root\scratch\mod\logs\er_apex.log"
            $t0 = Get-Date
            while (-not ((Test-Path $log) -and (Select-String -Quiet 'quickboot: done' $log))) {
                if (((Get-Date) - $t0).TotalSeconds -gt 300) { throw "the quick start did not finish in 300 s: see $log" }
                Start-Sleep 1
            }
            & $Game cmd 'skeleton c0000_runtime.json' | Out-Null
        } finally { & $Game stop | Out-Null }
        $dump = "$Root\scratch\mod\dev\c0000_runtime.json"
        if (-not (Test-Path $dump)) { throw "the game wrote no $dump" }
        New-Item -ItemType Directory -Force er-data\runtime, er-data\skeleton | Out-Null
        Copy-Item $dump er-data\runtime\c0000_runtime.json
        Copy-Item $dump er-data\skeleton\c0000_live_skeleton.json
    }
    Run 'er-data\s2b\mapping-v0.json' python tools/retarget/skeleton_map.py
}

Step 5 'body model 999 (about 2.5 min)' {
    Run 'er-data\s3\octane\package\build-manifest.json' python tools/octanemesh/convert_octane.py --matbin-bnd er-data\s3\inputs\material\allmaterial.matbinbnd.dcx
}

Step 6 'first-person model 998 and the animation packs (about 17 min)' {
    # each step starts from the previous one's material bundle and animation pack: keep the order
    Run 'apex-data\pov\fuse_pov.anim' python tools/apexpov/bake_pov.py
    Run 'er-data\s3\octane_pov\package\build-manifest.json' python tools/fusepov/build_pov.py --legend octane --matbin-bnd er-data\s3\octane\package\material\allmaterial.matbinbnd.dcx
    Run 'apex-data\pov\octane\fuse_pov.anim' python tools/apexpov/bake_pov.py --legend octane
    Run 'er-data\s3\octane_gun\package\build-manifest.json' python tools/fusegun/build_gun.py --legend octane
    Run 'apex-data\pov\octane_ability\fuse_pov.anim' python tools/apexpov/bake_ability.py
    Run 'er-data\s3\octane_pov_ability\package\build-manifest.json' python tools/fusepov/build_ability.py
    Run 'apex-data\pov\octane_battery\fuse_pov.anim' python tools/apexpov/bake_battery.py
    Run 'er-data\s3\octane_pov_battery\package\build-manifest.json' python tools/fusepov/build_battery.py
    Run 'apex-data\pov\octane_weapons\fuse_pov.anim' python tools/apexpov/bake_weapons.py
    Run 'er-data\s3\octane_pov_weapons\package\build-manifest.json' python tools/fusepov/build_weapons.py
    Run 'apex-data\pov\octane_padworld\padworld.json' python tools/apexpov/bake_padworld.py
}

Step 7 'first-person base pose (about 1 min)' {
    Run 'apex-data\anim\fuse.anim' python tools/fuseanim/export_anim.py
    Run 'er-data\s3\fuse\align.json' python tools/fusemesh/convert_fuse.py --geometry-only
    Run 'er-data\s4\fuse_er.anim' python tools/retarget/bake_er_anim.py fuse_idle_rifle_ADS
}

Step 8 'the Wingman and the R-99 (about 8 min)' {
    Run 'apex-data\assets\wingman\verification.json' python tools/apexassets/wingman_assets.py
    Run 'apex-data\assets\r99\verification.json' python tools/apexassets/r99_assets.py
    Run 'apex-data\assets\r99_ascension\verification.json' python tools/apexassets/r99_ascension_assets.py
    Run 'apex-data\pov\octane_wingman\fuse_pov.anim' python tools/apexpov/bake_wingman.py
    Run 'er-data\s3\octane_pov_wingman\wingman-verification.json' python tools/fusepov/build_wingman.py
    Run 'apex-data\audio\wingman\manifest.json' python tools/fuseaudio/export_audio.py --set wingman
    Run 'apex-data\audio\r99\manifest.json' python tools/fuseaudio/export_audio.py --set r99
    Run 'apex-data\hud\octane\extra\rui\weapon_icons\r5\weapon_wingman.png' python tools/apexhud/export_wingman.py --legend octane
    Run 'apex-data\hud\octane\extra\rui\weapon_icons\r5\weapon_r97.png' python tools/apexhud/export_wingman.py --legend octane --weapon r99
}

"`nall assets ready ($made made, $skipped already there). Start the game: pwsh play.ps1"

