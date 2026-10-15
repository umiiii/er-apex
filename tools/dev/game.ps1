<#
Dev harness for ER-Apex test runs (ER-Fuse until D-027). The test install lives in scratch\mod (not in git).

  pwsh tools/dev/game.ps1 build                     cargo build --release (er-apex)
  pwsh tools/dev/game.ps1 install [-Set k=v,...] [-NoArena] [-Legend fuse|octane]   DLL + profile +
                                                    dev ini into scratch\mod, and the test arena's two
                                                    map files (D-026, T013: er-data\test_arena\package)
                                                    unless -NoArena; -Legend also installs that legend's
                                                    armour parts (999 body, 998 first-person arms), their
                                                    materials and the first-person animation pack from
                                                    the converters' outputs (without it they stay)
  pwsh tools/dev/game.ps1 start                     launch the game through me3 (returns at once)
  pwsh tools/dev/game.ps1 spawn                     mob tests (D-025): three soldiers (npc 30001014) by
                                                    The First Step's grace (qb_place = first_step); out
                                                    of the world until then, again once all are dead
  pwsh tools/dev/game.ps1 wait [-InWorld] [-Timeout s]   wait for the mod's dev\state.json
  pwsh tools/dev/game.ps1 cmd "<command>"           send a dev command, print what it logged
  pwsh tools/dev/game.ps1 shot <name>               screenshot of the game -> scratch\shots\<name>.png
  pwsh tools/dev/game.ps1 burst <name> [-Count n] [-Cmd "<command>"]   optional dev command, then n
                                                    screenshots ~0.1 s apart (<name>-1.png ...)
  pwsh tools/dev/game.ps1 lure [npc] [-Walk ms] [-Timeout s]   mob tests (D-014): shoot the nearest
                                                    regular enemy to draw it until an npc (default the
                                                    soldier 30001014) is within 5 m; -Walk: walk forward
                                                    once it is within 16 m
  pwsh tools/dev/game.ps1 lock                      lock on (right stick), turning the camera until it takes
  pwsh tools/dev/game.ps1 keys "<keys>"             send key presses to the game window (SendKeys syntax)
  pwsh tools/dev/game.ps1 stop                      quit through the dev channel, kill after 15 s
  pwsh tools/dev/game.ps1 log [-Tail n]             the mod's log
  pwsh tools/dev/game.ps1 status                    is the game running, last state
#>
param(
    [Parameter(Position = 0)][string]$Action,
    [Parameter(Position = 1)][string]$Arg,
    [string[]]$Set = @(),
    [switch]$InWorld,
    [int]$Timeout = 300,
    [int]$Tail = 60,
    [int]$Count = 6,
    [string]$Cmd = '',
    [int]$Walk = 0,
    [switch]$NoArena,
    [ValidateSet('', 'fuse', 'octane')][string]$Legend = ''
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path "$PSScriptRoot\..\..").Path
$Crate = $Root
$Mod = "$Root\scratch\mod"
$Me3 = "$Root\tools\bin\me3\bin\me3.exe"
$Shots = "$Root\scratch\shots"
$Log = "$Mod\logs\er_apex.log"
$State = "$Mod\dev\state.json"

function Game { Get-Process eldenring -ErrorAction SilentlyContinue | Select-Object -First 1 }

function Build {
    Push-Location $Crate
    try {
        cargo build --release 2>&1 | Select-String -Pattern '^(warning: unused|error|\s+-->|\s+Finished)' | ForEach-Object { $_.Line }
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally { Pop-Location }
}

function Install {
    New-Item -ItemType Directory -Force "$Mod\package", "$Mod\logs", "$Mod\dev" | Out-Null
    $dll = "$Crate\target\x86_64-pc-windows-msvc\release\er_apex.dll"
    # a running game keeps the DLL locked against overwriting but allows renaming it (er-mario)
    Get-ChildItem "$Mod\er_apex.dll.old*" -ErrorAction SilentlyContinue | ForEach-Object { try { Remove-Item $_ -Force } catch {} }
    # until D-027 the mod was er-fuse: drop its old-named files (a running game keeps its DLL locked)
    Remove-Item "$Mod\er_fuse.ini", "$Mod\er-fuse.me3" -ErrorAction SilentlyContinue
    Get-ChildItem "$Mod\er_fuse.dll*" -ErrorAction SilentlyContinue | ForEach-Object { try { Remove-Item $_ -Force } catch {} }
    if (Test-Path "$Mod\er_apex.dll") { Move-Item "$Mod\er_apex.dll" "$Mod\er_apex.dll.old$(Get-Date -Format HHmmss)" -Force }
    Copy-Item $dll "$Mod\er_apex.dll"
    Copy-Item "$Root\tools\dev\er-apex.dev.me3" "$Mod\er-apex.me3" -Force
    $ini = Get-Content "$Root\tools\dev\er_apex.dev.ini" -Raw
    # -Set "a = 1;b = 2" (through pwsh -File an array arrives as one string: split on ';')
    foreach ($kv in $Set) { foreach ($one in ($kv -split ';')) { if ($one.Trim()) { $ini += "`n$($one.Trim())" } } }
    Set-Content "$Mod\er_apex.ini" $ini -NoNewline
    Remove-Item "$Mod\dev\cmd.txt", $State -ErrorAction SilentlyContinue
    # the test arena (D-026, T013): The First Step's map with an enemy generator and its event; the
    # soldiers stay out of the world until `spawn`
    $arena = "$Root\er-data\test_arena\package"
    foreach ($f in 'map\mapstudio\m60_42_36_00.msb.dcx', 'event\m60_42_36_00.emevd.dcx') {
        $to = "$Mod\package\$f"
        if ($NoArena) {
            Remove-Item $to -ErrorAction SilentlyContinue
        } elseif (Test-Path "$arena\$f") {
            New-Item -ItemType Directory -Force (Split-Path $to) | Out-Null
            Copy-Item "$arena\$f" $to -Force
        } else {
            "no test arena file $arena\$f (python tools/testarena/run.py build): spawn will do nothing"
        }
    }
    if ($Legend) { Install-Legend }
    "installed $((Get-Item "$Mod\er_apex.dll").Length) bytes into $Mod$(if ($NoArena) { ' (no test arena)' })"
}

# The legend's armour parts and materials (Fuse: T005/T008/T011, Octane: T015/T019/T016, in er-data\s3) and its
# first-person animation pack (tools/apexpov) into the mod folder. The material bundle must hold both
# models' entries: the first-person build's output, when it was built on the body's bundle.
function Install-Legend {
    $parts = "$Mod\package\parts"
    New-Item -ItemType Directory -Force $parts, "$Mod\package\material" | Out-Null
    # The body with the R-301 in the hand (T008/T019) when it is complete, else the plain body.
    if ($Legend -eq 'fuse') {
        $bodies = "$Root\er-data\s3\fuse_gun\package", "$Root\er-data\s3\fuse\package"; $pov = "$Root\er-data\s3\fuse_pov\package"; $anim = "$Root\apex-data\pov\fuse_pov.anim"
    } else {
        $bodies = "$Root\er-data\s3\octane_gun\package", "$Root\er-data\s3\octane\package"; $pov = "$Root\er-data\s3\octane_pov\package"; $anim = "$Root\apex-data\pov\octane\fuse_pov.anim"
        # the abilities' props in 998 (T020) go only with their FPOV v2 pack: the old pack leaves the
        # props' carriers to Elden Ring's animation, which would draw them on the body
        $abilityPov = "$Root\er-data\s3\octane_pov_ability\package"; $abilityAnim = "$Root\apex-data\pov\octane_ability\fuse_pov.anim"
        if ((Test-Path $abilityAnim) -and @(Get-ChildItem "$abilityPov\parts" -Filter '*_m_0998*' -ErrorAction SilentlyContinue).Count -eq 8) {
            $pov = $abilityPov; $anim = $abilityAnim
        }
        # the shield battery (T021, D-032): its prop joins the injector in 998's head piece, its
        # clips join T020's in the pack (group 4)
        $batteryPov = "$Root\er-data\s3\octane_pov_battery\package"; $batteryAnim = "$Root\apex-data\pov\octane_battery\fuse_pov.anim"
        if ((Test-Path $batteryAnim) -and @(Get-ChildItem "$batteryPov\parts" -Filter '*_m_0998*' -ErrorAction SilentlyContinue).Count -eq 8) {
            $pov = $batteryPov; $anim = $batteryAnim
        }
        # the Charge Rifle and the frag grenade (T022, U3/U9): the rifle in 998's body piece, the
        # grenade and two thrown ones in its head piece, their clips in the pack (groups 5-8)
        $weaponsPov = "$Root\er-data\s3\octane_pov_weapons\package"; $weaponsAnim = "$Root\apex-data\pov\octane_weapons\fuse_pov.anim"
        if ((Test-Path $weaponsAnim) -and @(Get-ChildItem "$weaponsPov\parts" -Filter '*_m_0998*' -ErrorAction SilentlyContinue).Count -eq 8) {
            $pov = $weaponsPov; $anim = $weaponsAnim
        }
        # the Wingman in the R-301's place (tools/apexpov/bake_wingman.py, tools/fusepov/build_wingman.py):
        # the pistol in 998's body piece, its clips in the pack (group 9)
        $wingmanPov = "$Root\er-data\s3\octane_pov_wingman\package"; $wingmanAnim = "$Root\apex-data\pov\octane_wingman\fuse_pov.anim"
        if ((Test-Path $wingmanAnim) -and @(Get-ChildItem "$wingmanPov\parts" -Filter '*_m_0998*' -ErrorAction SilentlyContinue).Count -eq 8) {
            $pov = $wingmanPov; $anim = $wingmanAnim
        }
    }
    $body = $bodies | Where-Object { @(Get-ChildItem "$_\parts" -Filter '*_m_0999*' -ErrorAction SilentlyContinue).Count -eq 8 } | Select-Object -First 1
    if (-not $body) { $body = $bodies[-1] }
    $used = @()
    foreach ($set in @(@($body, '*_m_0999*'), @($pov, '*_m_0998*'))) {
        $files = @(Get-ChildItem "$($set[0])\parts" -Filter $set[1] -ErrorAction SilentlyContinue)
        if ($files.Count -eq 8) {
            $files | Copy-Item -Destination $parts -Force
            $used += "$($set[1]) from $($set[0])"
        } else {
            "legend ${Legend}: no complete $($set[1]) parts in $($set[0]) ($($files.Count) of 8): left as installed"
        }
    }
    $material = @("$pov\material\allmaterial.matbinbnd.dcx", "$body\material\allmaterial.matbinbnd.dcx") | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($material) {
        Copy-Item $material "$Mod\package\material\allmaterial.matbinbnd.dcx" -Force
        $used += "materials from $material"
    }
    if (Test-Path $anim) {
        Copy-Item $anim "$Mod\fuse_pov.anim" -Force
        $used += "first-person pack $anim"
    } else {
        "legend ${Legend}: no $anim, first-person pack left as installed"
    }
    # The base pose first person plays under the view model (firstperson.rs: fuse_idle_rifle_ADS through
    # spike/pose.rs, which writes the carriers): tools/retarget/bake_er_anim.py
    $erAnim = "$Root\er-data\s4\fuse_er.anim"
    if (Test-Path $erAnim) {
        Copy-Item $erAnim "$Mod\fuse_er.anim" -Force
        $used += "base pose $erAnim"
    } else {
        "legend ${Legend}: no $erAnim, first person cannot pose the view model"
    }
    # Octane's jump pad on the ground (R4, tools/apexpov/bake_padworld.py): the world prop's
    # sequences for model 998's pad; without it the mod rings the pad on the HUD
    $padworld = "$Root\apex-data\pov\octane_padworld\padworld.json"
    if ($Legend -eq 'octane' -and (Test-Path $padworld)) {
        Copy-Item $padworld "$Mod\padworld.json" -Force
        $used += "world pad $padworld"
    } elseif ($Legend -eq 'fuse') {
        Remove-Item "$Mod\padworld.json" -ErrorAction SilentlyContinue
    }
    "legend ${Legend}: " + ($used -join '; ')
}

function Start-Game {
    if (Game) { throw "the game is already running" }
    # D-010: each dev run is 1920x1080 in a window. The game can save BORDERLESS after a run;
    # read its UTF-16 XML explicitly (the file can have no BOM) before launching it again.
    $graphics = Join-Path $env:APPDATA 'EldenRing\GraphicsConfig.xml'
    if (Test-Path -LiteralPath $graphics) {
        $settings = [IO.File]::ReadAllText($graphics, [Text.Encoding]::Unicode)
        $windowed = $settings -replace '<ScreenMode>[^<]+</ScreenMode>', '<ScreenMode>WINDOW</ScreenMode>'
        $windowed = $windowed -replace '<Resolution-WindowScreenWidth>[^<]+</Resolution-WindowScreenWidth>', '<Resolution-WindowScreenWidth>1920</Resolution-WindowScreenWidth>'
        $windowed = $windowed -replace '<Resolution-WindowScreenHeight>[^<]+</Resolution-WindowScreenHeight>', '<Resolution-WindowScreenHeight>1080</Resolution-WindowScreenHeight>'
        if ($windowed -ne $settings) {
            New-Item -ItemType Directory -Path "$Root\scratch\saves" -Force | Out-Null
            $backup = Join-Path "$Root\scratch\saves" "GraphicsConfig.xml.before-dev-$(Get-Date -Format yyyyMMdd-HHmmss)"
            Copy-Item -LiteralPath $graphics -Destination $backup
            [IO.File]::WriteAllText($graphics, $windowed, [Text.UnicodeEncoding]::new($false, $false))
            "dev display: WINDOW 1920x1080 (backup $backup)"
        }
    }
    Remove-Item $State -ErrorAction SilentlyContinue
    # a fresh log per run: waits on log lines must not see the previous run's
    Remove-Item "$Mod\logs\er_apex.log" -ErrorAction SilentlyContinue
    # no output redirection: redirecting makes me3 (and through it the game) inherit our handles,
    # and a caller reading our output then waits until the game exits (me3 keeps its own logs in
    # %LOCALAPPDATA%\garyttierney\me3\data\logs)
    $p = Start-Process $Me3 -ArgumentList @('launch', '-g', 'er', '-p', "$Mod\er-apex.me3") -PassThru -WindowStyle Hidden
    # the game window takes the focus when it opens: hand it back to whatever had it (D-010)
    if (-not ('DevFg' -as [type])) {
        Add-Type 'using System; using System.Runtime.InteropServices; public static class DevFg { [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); }'
    }
    $prev = [DevFg]::GetForegroundWindow().ToInt64()
    if ($prev -ne 0) {
        Start-Process pwsh -ArgumentList @('-NoProfile', '-File', "$Root\tools\dev\focus-back.ps1", '-Prev', $prev) -WindowStyle Hidden | Out-Null
    }
    "me3 started (pid $($p.Id))"
}

function Wait-State {
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt $Timeout) {
        if (Test-Path $State) {
            try { $s = Get-Content $State -Raw | ConvertFrom-Json } catch { $s = $null }
            if ($s -and (-not $InWorld -or $s.in_world)) { return "after $([int]((Get-Date) - $t0).TotalSeconds) s: $(Get-Content $State -Raw)" }
        }
        if (-not (Game) -and ((Get-Date) - $t0).TotalSeconds -gt 60) { throw "the game is not running" }
        Start-Sleep -Milliseconds 500
    }
    throw "timeout after $Timeout s; last state: $(if (Test-Path $State) { Get-Content $State -Raw })"
}

function Send-Cmd([string]$command) {
    $before = if (Test-Path $Log) { (Get-Content $Log).Count } else { 0 }
    Set-Content "$Mod\dev\cmd.txt" $command
    $t0 = Get-Date
    while (((Get-Date) - $t0).TotalSeconds -lt 20) {
        Start-Sleep -Milliseconds 300
        if (-not (Test-Path "$Mod\dev\cmd.txt")) {
            Start-Sleep -Milliseconds 500
            return (Get-Content $Log | Select-Object -Skip $before) -join "`n"
        }
    }
    throw "the game did not take the command (dev = 1? in a frame?)"
}

# distance (m) to the nearest living npc of this param id, from the dev command `chrs`
function Nearest([string]$npc) {
    $d = (Send-Cmd 'chrs') -split "`n" | Where-Object { $_ -match "npc $npc hp [1-9]" } |
        ForEach-Object { [double](($_.Trim() -split '\s+')[9]) }
    if ($d) { ($d | Measure-Object -Minimum).Minimum } else { [double]::NaN }
}

function Shot([string]$name) {
    $g = Game
    if (-not $g) { throw "the game is not running" }
    Add-Type -AssemblyName System.Drawing
    if (-not ('DevWin' -as [type])) {
        Add-Type @'
using System; using System.Runtime.InteropServices;
public static class DevWin {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
    }
    [DevWin]::SetProcessDPIAware() | Out-Null
    $r = New-Object DevWin+RECT
    [DevWin]::GetClientRect($g.MainWindowHandle, [ref]$r) | Out-Null
    $w = $r.R - $r.L; $h = $r.B - $r.T
    if ($w -le 0 -or $h -le 0) { throw "the game window has no client area (minimised?)" }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $gr = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $gr.GetHdc()
    # PW_CLIENTONLY | PW_RENDERFULLCONTENT: the window's own content (DX12 included), even when
    # other windows cover it
    $ok = [DevWin]::PrintWindow($g.MainWindowHandle, $hdc, 3)
    $gr.ReleaseHdc($hdc)
    New-Item -ItemType Directory -Force $Shots | Out-Null
    # a 1280-wide copy to look at (the full size one for details)
    $sw = [Math]::Min(1280, $w); $sh = [int]($h * $sw / $w)
    $small = New-Object System.Drawing.Bitmap $bmp, $sw, $sh
    $path = "$Shots\$name.png"
    $small.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Save("$Shots\$name.full.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $gr.Dispose(); $bmp.Dispose(); $small.Dispose()
    "$path (client ${w}x${h}, PrintWindow $ok)"
}

function Send-Keys([string]$keys) {
    $g = Game
    if (-not $g) { throw "the game is not running" }
    $ws = New-Object -ComObject WScript.Shell
    $ws.AppActivate($g.Id) | Out-Null
    Start-Sleep -Milliseconds 300
    $ws.SendKeys($keys)
    "sent $keys"
}

function Stop-Game {
    if (-not (Game)) { return "not running" }
    try { Set-Content "$Mod\dev\cmd.txt" 'quit' } catch {}
    $t0 = Get-Date
    while ((Game) -and ((Get-Date) - $t0).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 500 }
    if (Game) { Get-Process eldenring | Stop-Process -Force; return "killed" }
    "quit"
}

switch ($Action) {
    'build' { Build }
    'install' { Install }
    'start' { Start-Game }
    'wait' { Wait-State }
    'cmd' { Send-Cmd $Arg }
    'shot' { Shot $(if ($Arg) { $Arg } else { Get-Date -Format 'yyyyMMdd-HHmmss' }) }
    'burst' {
        # the command goes in without waiting for its log (Send-Cmd polls), so the first frames
        # come right after the game picks it up (next frame)
        if ($Cmd) { Set-Content "$Mod\dev\cmd.txt" $Cmd; while (Test-Path "$Mod\dev\cmd.txt") { Start-Sleep -Milliseconds 20 } }
        for ($i = 1; $i -le $Count; $i++) { Shot "$Arg-$i" | Out-Null; Start-Sleep -Milliseconds 60 }
        "$Count shots: $Shots\$Arg-1..$Count.png"
    }
    'lure' {
        # at the "deep small room" grace the soldier stops at the corridor until Fuse walks in (-Walk 1800)
        $npc = if ($Arg) { $Arg } else { '30001014' }
        $t0 = Get-Date; $shot = [datetime]::MinValue; $walked = $false
        while (((Get-Date) - $t0).TotalSeconds -lt $Timeout) {
            $d = Nearest $npc
            '{0,5:N1} s  nearest {1}: {2:N1} m' -f ((Get-Date) - $t0).TotalSeconds, $npc, $d
            if ($d -le 5) { break }
            if ($d -le 16 -and $Walk -gt 0 -and -not $walked) { Send-Cmd "pad LY=1 $Walk" | Out-Null; $walked = $true }
            elseif ($d -gt 16 -and ((Get-Date) - $shot).TotalSeconds -gt 12) { Send-Cmd 'burstnear 1 1 1' | Out-Null; $shot = Get-Date }
            Start-Sleep -Milliseconds 700
        }
    }
    'spawn' {
        # the test arena's generator event clears the flag once it has called the generator
        (Send-Cmd 'setflag 1042360990 1') -split "`n" | Select-Object -Last 1
        Start-Sleep -Seconds 3
        (Send-Cmd 'chrs 30') -split "`n" | Where-Object { $_ -match 'npc 30001014' }
    }
    'lock' {
        for ($i = 1; $i -le 6; $i++) {
            Send-Cmd 'pad RS 120' | Out-Null; Start-Sleep -Milliseconds 400
            $s = Send-Cmd 'cam'
            if ($s -match 'is_locked_on true') { "locked (try $i)"; break }
            Send-Cmd 'pad RX=1 450' | Out-Null; Start-Sleep -Milliseconds 300
        }
        ($s -split "`n" | Select-Object -Last 1) -replace '.*\| lock:', 'lock:'
    }
    'keys' { Send-Keys $Arg }
    'stop' { Stop-Game }
    'log' { if (Test-Path $Log) { Get-Content $Log -Tail $Tail } }
    'status' { "running: $([bool](Game))"; if (Test-Path $State) { Get-Content $State -Raw } }
    default { Get-Content $PSCommandPath -TotalCount 15 | Select-Object -Skip 1 }
}
