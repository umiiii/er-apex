# er-apex

English | [中文](README.zh-CN.md)

An offline fan mod for ELDEN RING: play as Octane from Apex Legends. A Rust DLL loaded by [me3](https://me3.help), built on the architecture of [er-mario](https://github.com/deltarooo/er-mario).

**This repository ships no game files and no game assets**: no models, textures, animations, sounds, HUD images or text from Apex Legends or ELDEN RING. You make them yourself from your own game installs with the converters in this repository's `tools/` (see [Building](#building)). You must give the Apex Legends install folder yourself.

## What this fork changes

Forked from [umiiii/er-apex](https://github.com/umiiii/er-apex). On top of it:

**Weapons**

- **Six guns**: the Wingman (in hand at the start), the R-99, the R-301, the VK-47 Flatline, the Sentinel and the Charge Rifle.
- **Sentinel** (changed on purpose): automatic, a shot every 0.8 s with Apex's bolt action, 7 rounds, a round back into the magazine every 0.4 s (no reload), the amped shot's sound, every hit a headshot (70 x 1.8, the kill feed's headshot mark), and **homing rounds**: with an enemy in a cone straight ahead (60 m, 30° each side; the crosshair does not move), a shot is a glowing round that flies from the muzzle, curves after the enemy and hits it when it gets there (ini `homing`, `homing_range`, `homing_angle`, `homing_height`, `homing_speed`; the defaults are at the top of `src/spike/homing.rs`). Their first-person models, animations, sounds and HUD icons are all exported from your Apex install by `export-assets.ps1` step 8.
- **Weapon wheel**: hold Tab, turn the view (or press 1-3) to pick a gun, let go to switch. Key 2 is still the Charge Rifle.
- **Default models and textures**: every gun uses its default model and textures, which every Apex install has. The R-99's Cutting Edge and the Flatline's Teal Zeal models are opt-in (`export-assets.ps1 -SkinModels`), and so are the skins below.
- **Inspect**: key 5 plays the inspect of the gun in hand, with its sounds; a shot, aiming, a reload, a switch, sprint or an ability cuts it.
- **Holstered mode**: key 3 puts the gun away and takes out Wraith's heirloom kunai (its first-person model and animations from your Apex install); you run a little faster (×1.1, ini `holster_speed`), the left mouse button swings it (30 damage up to 2 m), 5 plays its inspect (all with the kunai's sounds), and 3 again (or 1 / 2) brings the gun back. The HUD stays as it was.
- **Wingman**: semi-auto, 8 rounds, 50 a shot, head ×1.5, reload 2.1 s, zoom 60°, Apex's view kick. **Charge Rifle**: 8 rounds.
- **Custom skins (optional)**: put your own textures in `apex-data\skins\` and step 8 uses them (see [Weapon skins](#weapon-skins-optional)). No skin textures are in the repository.

**HUD fonts (optional)**

- Put your own `.ttf` / `.otf` files in `apex-data\fonts\` and the HUD draws its digits and English letters with them (everything else, Chinese included, stays in Apex's font). By default every text uses `Apex Regular` if it is there, else the first font; ini `hud_font_body`, `hud_font_numeric`, `hud_font_bold` (the file name without extension, lower case, or `off`) pick per face, `hud_font = off` turns them off. Make the atlas with `pwsh export-assets.ps1` (step 8) or `python tools/apexhud/custom_font.py`. No font files are in the repository.

**Playing**

- **F5**: back to your own ELDEN RING character, with the game's own movement and collision, camera, HUD, armour and weapons; the mod's guns, abilities and HUD are off. F5 again: Octane.
- **Keyboard and mouse prompts**: by default the game sees no controller, so its button prompts are keyboard and mouse. To play with a controller: `pwsh play.ps1 -Pad`.
- **Targets**: every character but the player's side (teams 1, 2, 8, 12; ini `friendly_teams`) can be hit, as in ELDEN RING: dogs, birds, invaders, dragons and bosses too.
- **Movement**: the ground's collision is read again every second and a fall into the ground is caught (late-loaded map tiles); on a lift, up or down, the game carries the player until it stops.
- **Pathfinder's grapple**: the weapon wheel (hold Tab) also picks Q's ability, the stim or the grapple. With the grapple, Q fires the hook (out to 850 units, about 21 m) and it pulls you in with Apex's own numbers (speed ramping 50 to 800 over 1.5 s, 1500 acceleration, lighter gravity on the way up, a boost when it lets go); Space, Q again, reaching the hook or slowing down lets go. No cooldown. Its sounds are Apex's; a cable is drawn to the hook (no first-person animation yet).
- **Kill-streak badge** (top right, where Apex shows your rank): kills in the last minute, 0-3 D1, 4-10 M1, above 10 P1. The badges are your own pictures: put `D1.png`, `M1.png` and `P1.png` in `apex-data\hud\rank\` (without them nothing shows).
- **Kill feed names**: a monster shows its kind's name from the game (its spirit ashes' name, in the game's language) where the game has one, else its Chinese community name (`tools/apexhud/monster_names_zhocn.json`, by its Paramdex name), else that English name; only then "enemy".
- **Hit sound**: the first hit that hurts an enemy plays Apex's armour-break shatter, once per enemy (`--set hits`).
- **Charge Rifle sounds** stop with the shot: no firing sound after a cancel, no loop when a beam hits flesh.
- **Settings**: `pwsh play.ps1 -Fov 90 -FpsLimit 120 -PlayerName "name" -Pad`. Field of view 90 by default; the game's 60 FPS by default; the HUD name from `-PlayerName` or `ER_APEX_PLAYER_NAME`, else the save's character.

**Setting up**

- **No DLC needed**: without Shadow of the Erdtree, `erextract` skips `DLC.bhd`.
- `export-assets.ps1` has eight steps (step 8: the Wingman, the R-99, the kunai and the skins); run it once as below.

Known limits: the third-person body still holds the R-301; the Wingman's hammer does not move on its own; emissive textures are not used.

## Fastest start

Copy this to any agent:

```markdown
Help me get https://github.com/MegaWeed/er-apex up and running
```

## Getting started

### Tool chain

**System and disk**

- Windows 10/11 x64, PowerShell 7 (`pwsh`).
- ELDEN RING (the game bindings support 2.7.1.0 / 2.7.1.1) and Apex Legends installed.
- Keep the repository path short (for example `D:\er-apex`): the exported file paths are deep, and they fail when Windows long paths are off.
- About 45 GB of disk for the generated assets.
- The first asset run needs a network connection: it downloads the RSX source and its audio decoders, `tools/erdata`'s pinned dependencies, NuGet packages and Rust crates.

**Software to install**

| Software | Version and use |
|---|---|
| Python | 3.11 or later (tested with 3.13). Only three third-party packages: `pip install numpy pillow matplotlib` |
| Rust | stable, MSVC toolchain (`x86_64-pc-windows-msvc`, see `rust-toolchain.toml`); builds the DLL, the crates in `deps/` and `tools/erdata/erextract` |
| .NET SDK | 8.0.425 or a later patch of 8.0 (pinned by `global.json` in `tools/erdata` and `tools/testarena`); builds `ertool` and a few small C# tools |
| Visual Studio 2022 | with the "Desktop development with C++" workload (MSVC and a Windows 10/11 SDK); `build_rsx.py` builds RSX with MSBuild, and Rust's MSVC linker needs it too |

**Three programs you download into `tools/`** (not in git)

| Program | Put it at | From |
|---|---|---|
| me3 | `tools\bin\me3\bin\me3.exe` | the Windows release from [me3.help](https://me3.help); unzip the whole folder into `tools\bin\me3\` |
| texconv | `tools\bin\texconv\texconv.exe` | `texconv.exe` from a [DirectXTex](https://github.com/microsoft/DirectXTex/releases) release |
| RSX 2.3.0 | `tools\rsx-2.3.0\rsx_nogui.exe` | the 2.3.0 release of [r-ex/rsx](https://github.com/r-ex/rsx/releases), unzipped into `tools\rsx-2.3.0\`. Only `rsx_export.py` uses it; the other exports use the patched RSX that `build_rsx.py` builds from source |

### 1. Build the DLL

```powershell
pwsh build.ps1          # -> target\x86_64-pc-windows-msvc\release\er_apex.dll
pwsh build.ps1 -Test    # also run the tests of the crates in deps/
```

### 2. Make the game assets

```powershell
pwsh export-assets.ps1
```

- It asks for the Apex Legends install folder (with `paks\Win64`) and ELDEN RING's `Game` folder (with `eldenring.exe`). It finds them through Steam when it can and shows them as the default: press Enter to take it. The folders are only read, never written.
- It checks the tool chain, then runs the eight steps of [Making the game assets](#making-the-game-assets-apex-data-er-data) in order: about 70 minutes. Step 4 starts the game once for about a minute to read the player skeleton; leave the game window alone until it quits.
- A command whose outputs are already there is skipped, so after a failure fix the cause and run the script again: it goes on where it stopped. A command that was cut off runs again from the start. To make everything again: `-Force`; only steps 5 to 8: `-From 5 -Force`.
- Weapon skins are optional: put them in `apex-data\skins\` before this step, or later (see [Weapon skins](#weapon-skins-optional)). No questions: `-ApexDir <folder> -EldenRingDir <folder>`. No test area (no soldiers to shoot): `-NoArena`.

### 3. Play

```powershell
pwsh play.ps1
```

- It backs up the test save to `scratch\saves`, installs the mod into `scratch\mod` and starts the game offline through me3, in a 1920×1080 window. The game skips the title screen and continues the test save's last character at the grace "The First Step" in Limgrave: Octane in first person, Wingman in hand, three soldiers next to the grace as targets.
- The test save `ER0000_fuse.sl2` needs at least one character. me3 copies it from your normal save the first time it starts the game.
- **Click the game window** to play with keyboard and mouse. The game sees no controller unless you start it with `-Pad`.
- Keys: WASD, Space jump, Shift sprint, Ctrl crouch / slide; left mouse fire, right mouse aim, R reload; hold Tab for the weapon wheel (or 1-3), 2 the Charge Rifle, 3 the holstered mode (the kunai), 5 inspect; Q stim, Z jump pad, 4 shield battery, G frag grenade; **F5** your own ELDEN RING character and back.
- Options: `-Fov 90` (70-110), `-FpsLimit 120` (default: the game's 60), `-PlayerName "name"` (the HUD's name), `-Pad` (controller).
- By default gun damage is ×3 (not the Charge Rifle's: it keeps its own) and the Jump Pad has no cooldown; `-Season3` uses the Season 3 values. `-NoSpawn`: no soldiers.
- More soldiers: `pwsh tools/dev/game.ps1 spawn`. A friendly NPC stands about 10.5 m from the grace; do not shoot it. Back to the grace: `pwsh tools/dev/game.ps1 cmd "warp 1042361951"`. Quit: `pwsh tools/dev/game.ps1 stop`.

## Building

`build.ps1` and `export-assets.ps1` run what this section describes. The commands here are for running a part by hand.

### The DLL

The three crates of this project are in `deps/`: `er-apex-move` (Apex movement controller), `er-apex-audio` (sound mixer) and `er-apex-anim` (the first-person animation pack format; step 7's `export_anim.py` runs it). A clone of this repository alone builds the DLL.

```powershell
cargo build --release
# output: target\x86_64-pc-windows-msvc\release\er_apex.dll

# tests of the three crates
cargo test --release --offline --manifest-path deps/er-apex-move/Cargo.toml
cargo test --release --offline --manifest-path deps/er-apex-anim/Cargo.toml
cargo test --release --offline --manifest-path deps/er-apex-audio/Cargo.toml
```

### Making the game assets (`apex-data/`, `er-data/`)

The converters in `tools/` make every asset the mod uses from your local game installs; none of it goes into git (`apex-data/`, `er-data/` and `scratch/` are in `.gitignore`). The commands below were run in this order in an empty copy, and the result was checked in game. For each tool's details and checks (`verify_*.py`), see the README in its folder under `tools/`.

**Game folders**

The converters read two game folders from environment variables. They only read them, never write.

| Variable | Points to | Required |
|---|---|---|
| `APEX_LEGENDS_DIR` | the Apex Legends install folder (with `paks\Win64`) | **yes**, no default |
| `ELDEN_RING_DIR` | ELDEN RING's `Game` folder (with `eldenring.exe`) | when it is not `E:\SteamLibrary\steamapps\common\ELDEN RING\Game` |

```powershell
$env:APEX_LEGENDS_DIR = 'D:\SteamLibrary\steamapps\common\Apex Legends'   # your install folder
$env:ELDEN_RING_DIR   = 'D:\SteamLibrary\steamapps\common\ELDEN RING\Game'
# To keep them: [Environment]::SetEnvironmentVariable('APEX_LEGENDS_DIR', '<folder>', 'User')
```

Without `APEX_LEGENDS_DIR`, or when the folder has no `paks\Win64`, the converters stop with a message that tells you how to set it.

You do not need R5Reloaded. The HUD, frag grenade and audio exports check which Season 3 script lines name what they export; that evidence (file names, line numbers and the short values on those lines, never the scripts) is recorded in `tools/s3_evidence.json` and comes with the repository. The assets themselves all come from your Apex Legends install. To make the record again, set `R5RELOADED_RECORD` to an R5Reloaded `LIVE` folder and run those three exports (see `tools/s3record.py`).

Run every command at the root of this repository, in this order: each step uses what the steps before it made. The times are measured.

Some steps use outputs with `fuse` in their names. The project first played Fuse and then changed to Octane; the Apex skeleton, the R-301 third-person model, the R-301 first-person animation pack and the first-person base pose were made with Fuse first, and the Octane converters still take them as input. The steps below make only these shared parts, not Fuse's own body, HUD or sounds.

**1. Apex exports (about 31 min)**

```powershell
python tools/apexassets/build_rsx.py                     # build the patched RSX (first time; needs the network)
python tools/apexdata/rsx_export.py                       # character settings, weapon definitions, localization -> apex-data\export
python tools/apexdata/extract_fuse.py                     # weapon parameter table apex-data\fuse_data.json (the audio export reads it)
python tools/apexassets/export_assets.py                  # shared input: Apex skeleton, R-301 third-person model, first-person arms (T001)
python tools/apexassets/export_assets.py --legend octane  # Octane's body, arms, injector, jump pad (T014)
python tools/apexpov/export_pov.py                        # R-301 first-person model and animations, raw RSEQ, QC/SMD -> apex-data\pov
python tools/apexpov/export_defender_pov.py               # Charge Rifle first-person QC/SMD
python tools/apexassets/frag_grenade_assets.py            # frag grenade
python tools/apexassets/battery_assets.py                 # shield battery (T021)
python tools/apexassets/defender_assets.py                # Charge Rifle (T022)
python tools/apexassets/frag_assets.py                    # grenade and thrown grenade (T022)
```

**2. HUD and sounds (about 5 min)**

```powershell
python tools/apexhud/export_hud.py --legend octane      # -> apex-data\hud\octane (T017)
python tools/apexhud/export_extra.py --legend octane    # extra images, frag grenade icon included (U9)
python tools/fuseaudio/export_audio.py                  # R-301 -> apex-data\audio
python tools/fuseaudio/export_audio.py --set octane     # abilities and voice lines (T018)
python tools/fuseaudio/export_audio.py --set defender   # Charge Rifle
python tools/fuseaudio/export_audio.py --set frag       # frag grenade
```

**3. ELDEN RING extraction (about 2 min)**

```powershell
Push-Location tools/erdata
python scripts/setup_dependencies.py                     # fetch the pinned dependency sources and patch them
cargo build --release --manifest-path erextract/Cargo.toml
dotnet build ertool/ertool.csproj -c Release
python scripts/run_s2a.py              # c0000 skeleton, armour templates, text -> er-data\extract, er-data\json (T002)
python scripts/s3a_roundtrip.py        # armour builder round trip, original meshes and material bundle -> er-data\s3 (T003)
python scripts/s3a_material_evidence.py
Pop-Location

# NPC names for the kill feed
$fmg = Get-ChildItem er-data\extract\msg\zhocn\item.msgbnd -Recurse -Filter NpcName.fmg | Select-Object -First 1
& tools\erdata\ertool\bin\Release\net8.0\ertool.exe fmg $fmg.FullName --json --out er-data\json\NpcName_zhocn.json

python tools/testarena/run.py build    # optional: the test area -> er-data\test_arena (T013, see below)
```

`testarena` is for development tests: from your own copy of the map it makes The First Step's map and event files with an enemy generator next to the grace, so `game.ps1 spawn` brings three soldiers as targets. You do not need it to play; skip this line if you like. Without it `game.ps1 install` says the test area is missing and `spawn` does nothing; or give `install` the `-NoArena` switch.

**4. The skeleton from the running game (about 1 min)**

The skeleton mapping and the first-person props need the player skeleton as the running game has it. This step starts the game once and dumps it. Octane's models do not exist yet, so leave `first_person` off.

```powershell
$root = (Get-Location).Path
pwsh tools/dev/game.ps1 build
pwsh tools/dev/game.ps1 install -Set "quickboot = 1;qb_place = first_step"
pwsh tools/dev/game.ps1 start
$log = "$root\scratch\mod\logs\er_apex.log"
while (-not ((Test-Path $log) -and (Select-String -Quiet 'quickboot: done' $log))) { Start-Sleep 1 }
pwsh tools/dev/game.ps1 cmd "skeleton c0000_runtime.json"    # written to scratch\mod\dev\
pwsh tools/dev/game.ps1 stop
New-Item -ItemType Directory -Force er-data\runtime, er-data\skeleton | Out-Null
Copy-Item scratch\mod\dev\c0000_runtime.json er-data\runtime\c0000_runtime.json
Copy-Item scratch\mod\dev\c0000_runtime.json er-data\skeleton\c0000_live_skeleton.json

python tools/retarget/skeleton_map.py    # Apex bone -> ER bone mapping -> er-data\s2b\mapping-v0.json
```

**5. Body model 999 (about 2.5 min)**

```powershell
python tools/octanemesh/convert_octane.py --matbin-bnd er-data\s3\inputs\material\allmaterial.matbinbnd.dcx   # -> er-data\s3\octane (T015)
```

**6. First-person model 998, R-301 in hand and the animation packs (about 17 min)**

Each step builds on the one before it: the material bundle (`allmaterial.matbinbnd.dcx`) and the animation pack (`fuse_pov.anim`) start from the previous step's output, so keep the order.

```powershell
python tools/apexpov/bake_pov.py                    # shared input: R-301 first-person animation pack -> apex-data\pov\fuse_pov.anim (T012)
python tools/fusepov/build_pov.py --legend octane --matbin-bnd er-data\s3\octane\package\material\allmaterial.matbinbnd.dcx   # arms + R-301 (T016)
python tools/apexpov/bake_pov.py --legend octane    # -> apex-data\pov\octane\fuse_pov.anim
python tools/fusegun/build_gun.py --legend octane   # R-301 in 999's right hand (T019)
python tools/apexpov/bake_ability.py                # injector, hand-held jump pad (T020)
python tools/fusepov/build_ability.py
python tools/apexpov/bake_battery.py                # shield battery (T021)
python tools/fusepov/build_battery.py
python tools/apexpov/bake_weapons.py                # Charge Rifle, frag grenade (T022)
python tools/fusepov/build_weapons.py               # also turns the battery blue (battery_tint.py)
python tools/apexpov/bake_padworld.py               # jump pad on the ground (R4)
```

**7. First-person base pose (about 1 min)**

Each frame, first person plays the base pose `fuse_idle_rifle_ADS` first; the view model reaches the bones through this pose hook. Without it, the first-person arms and gun are not where they should be.

```powershell
python tools/fuseanim/export_anim.py                    # Apex third-person animation pack (T006) -> apex-data\anim\fuse.anim
python tools/fusemesh/convert_fuse.py --geometry-only   # skeleton alignment only -> er-data\s3\fuse\align.json (no model)
python tools/retarget/bake_er_anim.py fuse_idle_rifle_ADS   # -> er-data\s4\fuse_er.anim
```

**8. The Wingman, the R-99, the Flatline, the Sentinel and the kunai (about 20 min; this fork)**

The Wingman, the R-99, the Flatline, the Sentinel and Wraith's heirloom kunai, from your Apex install like the steps before, with their default models. `build_wingman.py` adds them to model 998 and its animation pack, on top of step 6's `octane_pov_weapons` stage, and takes the optional skins.

```powershell
python tools/apexassets/wingman_assets.py
python tools/apexassets/r99_assets.py
python tools/apexassets/flatline_base_assets.py       # VK-47 Flatline (flatline_base_v)
python tools/apexassets/kunai_assets.py               # Wraith's heirloom kunai (heirloom_wraith_v18_kunai_v)
python tools/apexassets/sentinel_assets.py            # Sentinel (sentinel_base_v)
python tools/apexpov/bake_wingman.py                  # -> apex-data\pov\octane_wingman\fuse_pov.anim
python tools/fusepov/build_wingman.py                 # -> er-data\s3\octane_pov_wingman (model 998); skins: see below
python tools/fuseaudio/export_audio.py --set wingman
python tools/fuseaudio/export_audio.py --set r99
python tools/fuseaudio/export_audio.py --set kunai
python tools/fuseaudio/export_audio.py --set flatline
python tools/fuseaudio/export_audio.py --set sentinel
python tools/fuseaudio/export_audio.py --set hits
python tools/fuseaudio/export_audio.py --set grapple
python tools/apexhud/export_wingman.py --legend octane
python tools/apexhud/export_wingman.py --legend octane --weapon r99
python tools/apexhud/export_wingman.py --legend octane --weapon flatline
python tools/apexhud/export_wingman.py --legend octane --weapon sentinel
python tools/apexhud/export_wingman.py --legend octane --weapon kunai
python tools/apexhud/export_wingman.py --legend octane --weapon grapple
python tools/apexhud/custom_font.py                   # only with fonts in apex-data\fonts: -> apex-data\hud\custom_font
```

#### Skin models (optional)

`pwsh export-assets.ps1 -SkinModels` uses the R-99's Cutting Edge model (`r99_react_v20_ascension_v`) and the Flatline's Teal Zeal model (`flatline_v20_trshunter_v`) instead of the default ones, and makes the animation pack and model 998 again; run it without `-SkinModels` to go back. By hand: export them too and set `ERAPEX_SKIN_MODELS=1` for `bake_wingman.py` and `build_wingman.py`:

```powershell
python tools/apexassets/r99_ascension_assets.py
python tools/apexassets/flatline_assets.py
$env:ERAPEX_SKIN_MODELS = '1'
python tools/apexpov/bake_wingman.py
python tools/fusepov/build_wingman.py
```

#### Weapon skins (optional)

Each folder you put in `apex-data\skins\` replaces that gun's textures (any other folder: `export-assets.ps1 -Skins <folder>`):

| Folder | Files | Replaces |
|---|---|---|
| `apex-data\skins\wingman\` | `Wingman_Default_col.dds` (optional `_spc`, `_nml`, `_gls`; `.png` works too) | the Wingman's base material |
| `apex-data\skins\chargerifle\` | `col\`, `nml\`, `gls\`, each with `.dds` files in several sizes such as `1024.dds`, `2048.dds`; the largest is used | the Charge Rifle's main material |
| `apex-data\skins\r99\` | `<size> COL SPC.dds`, such as `2048 COL SPC.dds`; the largest is used | the R-99's albedo and specular (Cutting Edge model: with `-SkinModels` only) |
| `apex-data\skins\flatline\` | `*COL*.dds` and `*SPC*.dds` (optional `*AO*.dds`, its glow); the largest is used | the Flatline's albedo and specular (Teal Zeal model: with `-SkinModels` only) |
| `apex-data\skins\kunai\` | `*_col.dds` and `*_spc.dds` (in subfolders too, such as `1024\P2020_Default_col.dds`); the largest is used | the kunai's albedo and specular |

Run `pwsh export-assets.ps1` again after adding, changing or removing a skin folder: step 8 notices that the set of skins changed and makes model 998 again; the rest is skipped. By hand:

```powershell
python tools/fusepov/build_wingman.py --skin apex-data\skins\wingman --cr-skin apex-data\skins\chargerifle --kunai-skin apex-data\skins\kunai
```

Then start the game with `pwsh play.ps1`. `game.ps1 install -Legend octane` takes the last stage of each chain: 999 from `octane_gun`; 998, the material bundle and the animation pack from `octane_pov_wingman` (else `octane_pov_weapons` / `octane_weapons`); plus the base pose `fuse_er.anim` and the ground jump pad `padworld.json`. When a stage is incomplete, it uses the stage before it.

Most of `apex-data` and `er-data` is intermediate data. The game reads about 490 MB of it: models about 147 MB, animation pack about 61 MB, HUD about 45 MB, sounds about 235 MB.

## Credits

- [er-mario](https://github.com/deltarooo/er-mario) by Delta: the architecture of this mod. `tools/erdata/erextract`'s archive, DCX and BND4 readers are adapted from it (MIT).
- [fromsoftware-rs](https://github.com/vswarte/fromsoftware-rs): the ELDEN RING game bindings, at the same pinned commit as er-mario.
- [me3](https://me3.help): the mod loader.
- [hudhook](https://crates.io/crates/hudhook), [ilhook](https://crates.io/crates/ilhook), [pelite](https://crates.io/crates/pelite), [glam](https://crates.io/crates/glam) and the other crates in `Cargo.toml`.
- [RSX](https://github.com/r-ex/rsx) by r-ex: the Apex Legends asset exports (2.3.0 release and a patched build from source).
- [SoulsFormatsNEXT](https://github.com/soulsmods/SoulsFormatsNEXT) (originally by Joseph Anderson, GPL-3.0) and [HKLib](https://github.com/The12thAvenger/HKLib) (MIT): ELDEN RING file formats in `ertool` and `testarena`.
- [Paramdex](https://github.com/soulsmods/Paramdex) and [UXM Selective Unpack](https://github.com/Nordgaren/UXM-Selective-Unpack): param definitions and the archive file name dictionary, used locally only.
- [DirectXTex](https://github.com/microsoft/DirectXTex) (`texconv`): texture conversion.

For the licences and pinned commits of the converter dependencies, see `tools/erdata/THIRD_PARTY_NOTICES.md`, `tools/testarena/THIRD_PARTY_NOTICES.md` and `tools/erdata/dependencies.lock.json`.

Apex Legends belongs to Respawn Entertainment and Electronic Arts; ELDEN RING to FromSoftware and Bandai Namco. This is fan work, not affiliated with any of them. This repository has source code only, and no game files or game assets. The assets you make are for your own use with game copies you own; do not redistribute them.

License: MIT (see `LICENSE`). `tools/erdata/ertool` and `tools/testarena` are GPL-3.0 because they use SoulsFormatsNEXT; they are development tools and are not linked into the mod.
