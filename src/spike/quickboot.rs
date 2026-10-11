//! Spike S7: from launch straight to Godrick the Grafted's fog gate, without input
//! (fuse-mod-plan.md 3.11, MVP criterion 5).
//!
//!   title "press any button" -> main menu "Continue" (synthetic pad, input.rs)
//!   -> in the world: set the test save's flags (grace unlocked, Godrick reset)
//!   -> the game's own grace warp (the function the map's "travel to grace" uses) to Secluded Cell
//!   -> the player put at the fog gate.
//!
//! Every id comes from the game data (docs/m0/S7-quickboot.md has the sources):
//! - Secluded Cell grace: BonfireWarpParam row 100007, bonfireEntityId 10001957, eventflagId 71007
//! - Godrick: MSB m10_00_00_00 enemy c4750_9000, entity 10000800; EMEVD event 10002800 ends if
//!   flag 10000800 is on and on his death sets 10000800, 9101, 61101 and (first time) 10000802
//! - the fog gate: MSB asset AEG099_002_9000, entity 10001800, at (-204.370, 81.858, 315.139)
//! - where the player stands: MSB player point c0000_9013, (-202.71, 81.864, 314.077), 1.97 m from
//!   the fog gate, moved 1.2 m closer into the gate's interaction range (see STAND)
//!
//! ini `qb_place` picks another end (`Target`): `grace` stops at the Secluded Cell grace (D-014);
//! `first_step` warps to Limgrave's The First Step grace instead, for the normal tests (D-025):
//! BonfireWarpParam row 61423601 `[Limgrave] The First Step`, bonfireEntityId 1042361951,
//! eventflagId 76101, m60_42_36_00, PlaceName 610019「引导之始」(er-data/json/smoke).
//!
//! Only ever touches the mod's own save file (me3 `savefile`), never the normal save.

use std::sync::Mutex;

use eldenring::cs::{CSEventFlagMan, CSLuaEventManImp, WorldChrMan};
use eldenring::position::HavokPosition;
use eldenring::rotation::Quaternion;
use fromsoftware_shared::FromStatic;

use crate::{input, log, paths, state};

/// Stormveil Castle, the block the fight is in.
const BLOCK: i32 = 0x0A00_0000; // m10_00_00_00
const GRACE_ENTITY: i32 = 10001957;
const GRACE_FLAG: u32 = 71007;
/// Godrick's flags the reset clears (see the module docs).
const BOSS_FLAGS: [u32; 4] = [10000800, 9101, 61101, 10000802];
/// Where the player is put: MSB player point c0000_9013 ((-202.71, 81.864, 314.077), 1.97 m from
/// the fog) moved 1.2 m towards the fog gate, into its interaction range (measured: the "enter the
/// fog" prompt shows from there, 0.78 m from the gate, and not from c0000_9013 itself); the yaw is
/// c0000_9013's (degrees), which faces the fog.
const STAND: [f32; 3] = [-203.71, 81.864, 314.727];
const STAND_YAW_DEG: f32 = 141.154;
/// The fog gate (MSB AEG099_002_9000), for the distance check and the log.
const FOG: [f32; 3] = [-204.370, 81.858, 315.139];
/// Limgrave, The First Step (see the module docs).
const FIRST_STEP_ENTITY: i32 = 1042361951;
const FIRST_STEP_FLAG: u32 = 76101;
/// The open world: area 60 (m60_42_36_00 for The First Step; a grace's spawn point near a tile's
/// edge can be in the next tile, so arrival only checks the area).
const OPEN_WORLD_AREA: u32 = 60;
/// A normal boot presses A five times at the title: "press any button", the notice about the last
/// quit (the dev channel's quit), the main menu's first item Continue, and once more during the load.
/// More means a screen the quick boot does not know: on 2026-10-05 the main menu had its fourth item,
/// the add-on content store, selected, and every press opened the Steam store's DLC page in the
/// overlay. So it stops, and it stops at once when the Steam overlay opens (`steam_overlay_open`).
const TITLE_PRESSES: u32 = 6;

/// Where the quick boot ends (ini `qb_place`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    /// Godrick's fog gate (the default, MVP criterion 5).
    FogGate,
    /// `grace`: the Secluded Cell grace (D-014).
    Grace,
    /// `first_step`: Limgrave's The First Step grace (D-025).
    FirstStep,
}

fn target() -> Target {
    match paths::config("qb_place").map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("grace") => Target::Grace,
        Some("first_step") => Target::FirstStep,
        _ => Target::FogGate,
    }
}

impl Target {
    /// The grace warped to: its entity id and the flag that unlocks it.
    fn grace(self) -> (i32, u32) {
        match self {
            Target::FirstStep => (FIRST_STEP_ENTITY, FIRST_STEP_FLAG),
            _ => (GRACE_ENTITY, GRACE_FLAG),
        }
    }

    fn arrived(self, block: i32) -> bool {
        match self {
            Target::FirstStep => (block as u32) >> 24 == OPEN_WORLD_AREA,
            _ => block == BLOCK,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Target::FogGate => "title -> Continue -> Secluded Cell -> Godrick's fog gate",
            Target::Grace => "title -> Continue -> Secluded Cell grace (qb_place = grace)",
            Target::FirstStep => "title -> Continue -> The First Step grace (qb_place = first_step)",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Stage {
    Off,
    /// press A every few seconds until the character is in the world (at most TITLE_PRESSES times)
    Title { next_press: f32, presses: u32 },
    /// in the world: wait for the load to settle
    Loading { since: f32 },
    /// waiting for the warp's loading screen (`seen_load`), then for the arrival to settle
    Warping { since: f32, started: f32, seen_load: bool },
    Place { frames: u32 },
    Done,
    Failed(String),
}

static STAGE: Mutex<Stage> = Mutex::new(Stage::Off);

pub fn enabled() -> bool {
    paths::flag("quickboot")
}

/// The grace warp: CSLuaEventScriptImitation::WarpToBonfire-like function (TGA table's "Warp";
/// `fn(imitation, proxy, bonfire entity id)`), found by its bytes.
type WarpFn = unsafe extern "C" fn(usize, usize, i32);

fn warp_fn() -> Option<WarpFn> {
    static FN: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let addr = *FN.get_or_init(|| {
        // C3 ?? ?? ?? ?? ?? ?? 57 48 83 EC ?? 48 8B FA 44, the function starts 2 bytes in
        let pat: [Option<u8>; 16] = [
            Some(0xC3), None, None, None, None, None, None, Some(0x57), Some(0x48), Some(0x83), Some(0xEC), None, Some(0x48),
            Some(0x8B), Some(0xFA), Some(0x44),
        ];
        let found = crate::scan::text(&pat).map(|a| a + 2);
        match found {
            Some(a) => log(format!("quickboot: grace warp function at +{:#x}", a - crate::scan::base())),
            None => log("quickboot: grace warp function not found"),
        }
        found
    });
    addr.map(|a| unsafe { std::mem::transmute::<usize, WarpFn>(a) })
}

/// Sets the test save's flags. Returns what changed and whether a boss flag was on: the map
/// was then loaded with Godrick defeated, and only a reload (the warp) brings him back.
fn set_flags(target: Target) -> Result<(String, bool), String> {
    let man = unsafe { CSEventFlagMan::instance_mut() }.map_err(|e| format!("no event flags: {e:?}"))?;
    let f = &mut man.virtual_memory_flag;
    let mut out = Vec::new();
    let mut boss_was_on = false;
    let (_, grace_flag) = target.grace();
    if !f.get_flag(grace_flag) {
        f.set_flag(grace_flag, true);
        out.push(format!("grace flag {grace_flag} on"));
    }
    // Godrick only matters in Stormveil
    if target != Target::FirstStep && paths::config("mvp_reset_boss").is_none_or(|v| v != "0") {
        for flag in BOSS_FLAGS {
            let was = f.get_flag(flag);
            boss_was_on |= was;
            f.set_flag(flag, false);
            out.push(format!("{flag} {}->off", if was { "on" } else { "off" }));
        }
    }
    Ok((out.join(", "), boss_was_on))
}

fn warp(target: Target) -> Result<(), String> {
    warp_to(target.grace().0)
}

/// The game's own grace warp to the grace (bonfire) with this entity id (also the dev channel's
/// `warp`; the grace must be one the save has unlocked).
pub fn warp_to(grace: i32) -> Result<(), String> {
    let man = unsafe { CSLuaEventManImp::instance_mut() }.map_err(|e| format!("no lua event manager: {e:?}"))?;
    let proxy = man.lua_event_proxy.as_ptr() as usize;
    let imitation = man.lua_event_script_imitation.as_ref().ok_or("no script imitation")?.as_ptr() as usize;
    let f = warp_fn().ok_or("warp function not found")?;
    log(format!("warp to grace {grace} (imitation {imitation:#x}, proxy {proxy:#x})"));
    unsafe { f(imitation, proxy, grace) };
    Ok(())
}

/// Puts the player at STAND facing STAND_YAW_DEG. Block and Havok space differ by a translation:
/// the player's own two positions give it.
fn place() -> Result<f32, String> {
    let wcm = unsafe { WorldChrMan::instance_mut() }.map_err(|e| format!("{e:?}"))?;
    let p = wcm.main_player.as_mut().ok_or("no player")?;
    let b = p.block_position;
    let physics = &mut p.chr_ins.modules.physics;
    let h = physics.position;
    let (ox, oy, oz) = (h.0 - b.x, h.1 - b.y, h.2 - b.z);
    let target = HavokPosition(STAND[0] + ox, STAND[1] + oy, STAND[2] + oz, 0.0);
    let yaw = STAND_YAW_DEG.to_radians();
    let q = Quaternion(0.0, (yaw / 2.0).sin(), 0.0, (yaw / 2.0).cos());
    physics.position = target;
    physics.orientation = q;
    physics.interpolated_orientation = q;
    physics.chr_proxy_pos_update_requested = true;
    let d = ((b.x - FOG[0]).powi(2) + (b.y - FOG[1]).powi(2) + (b.z - FOG[2]).powi(2)).sqrt();
    Ok(d)
}

/// Once per frame, from the frame task.
pub fn update() {
    let mut stage = STAGE.lock().unwrap_or_else(|e| e.into_inner());
    let now = crate::log::uptime();
    if *stage == Stage::Off {
        if !enabled() {
            return;
        }
        input::VIRTUAL.store(true, std::sync::atomic::Ordering::Relaxed);
        log(format!("quickboot: on ({})", target().describe()));
        *stage = Stage::Title { next_press: now + 6.0, presses: 0 };
    }
    let target = target();
    let s = state::LAST.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default();
    let next = match stage.clone() {
        Stage::Off | Stage::Done | Stage::Failed(_) => return,
        Stage::Title { .. } if s.in_world => {
            log("quickboot: in the world, waiting for the load to finish");
            Stage::Loading { since: now }
        }
        Stage::Title { presses, .. } if steam_overlay_open() => Stage::Failed(format!(
            "the Steam overlay opened at the title after {presses} presses of A (a store page?); nothing more is pressed"
        )),
        Stage::Title { next_press, presses } if now < next_press => Stage::Title { next_press, presses },
        Stage::Title { presses, .. } if presses >= TITLE_PRESSES => Stage::Failed(format!(
            "still at the title after {presses} presses of A, an unknown screen (take a screenshot); nothing more is pressed"
        )),
        Stage::Title { presses, .. } => {
            // the title's "press any button", then the main menu's first item, Continue
            input::hold(input::Synthetic { buttons: 0x1000, ..Default::default() }, 150);
            log(format!("quickboot: press A ({}{})", presses + 1, mouse_note()));
            Stage::Title { next_press: now + 2.5, presses: presses + 1 }
        }
        Stage::Loading { since } => {
            if s.anim == -1 || !s.in_world {
                Stage::Loading { since: now }
            } else if now - since > 2.0 {
                let reload = match set_flags(target) {
                    Ok((changes, boss_was_on)) => {
                        log(format!("quickboot: flags: {changes}"));
                        boss_was_on
                    }
                    Err(e) => {
                        log(format!("quickboot: flags failed: {e}"));
                        false
                    }
                };
                if reload {
                    log("quickboot: Godrick was defeated in the loaded map: warping to reload it");
                }
                if !reload && target == Target::FogGate && s.block == BLOCK && distance(s.block_pos, STAND) < 30.0 {
                    log("quickboot: already near the fog gate, no warp");
                    Stage::Place { frames: 0 }
                } else {
                    match warp(target) {
                        Ok(()) => Stage::Warping { since: now, started: now, seen_load: false },
                        Err(e) => Stage::Failed(e),
                    }
                }
            } else {
                Stage::Loading { since }
            }
        }
        Stage::Warping { since, started, seen_load } => {
            // a warp within the same map looks settled until its loading screen starts: only
            // count arrival after a load was seen (or after 30 s, in case none shows)
            let loading = s.anim == -1 || !s.in_world;
            let seen_load = seen_load || loading || now - started > 30.0;
            if now - started > 120.0 {
                Stage::Failed(format!("no arrival ({:?}) after 120 s (block {})", target, state::block_name(s.block)))
            } else if seen_load && target.arrived(s.block) && !loading {
                if now - since > 2.0 {
                    log(format!("quickboot: arrived in {} at {:.2} {:.2} {:.2}", state::block_name(s.block), s.block_pos[0], s.block_pos[1], s.block_pos[2]));
                    Stage::Place { frames: 0 }
                } else {
                    Stage::Warping { since, started, seen_load }
                }
            } else {
                Stage::Warping { since: now, started, seen_load }
            }
        }
        Stage::Place { .. } if target != Target::FogGate => {
            log(format!(
                "quickboot: done at the grace ({target:?}, {} at {:.2} {:.2} {:.2}), {:.1} s after launch",
                state::block_name(s.block),
                s.block_pos[0],
                s.block_pos[1],
                s.block_pos[2],
                now
            ));
            if !paths::flag("virtual_pad") {
                input::set_virtual(false);
            }
            Stage::Done
        }
        Stage::Place { frames } => match place() {
            Ok(d) if frames >= 10 => {
                // the lock-on button with nothing to lock on puts the camera behind the player
                input::hold(input::Synthetic { buttons: 0x0080, ..Default::default() }, 150);
                log(format!("quickboot: done, {d:.2} m from the fog gate; {:.1} s after launch", now));
                if !paths::flag("virtual_pad") {
                    input::set_virtual(false);
                }
                Stage::Done
            }
            Ok(_) => Stage::Place { frames: frames + 1 },
            Err(e) => Stage::Failed(format!("placing the player: {e}")),
        },
    };
    if let Stage::Failed(e) = &next {
        log(format!("quickboot: FAILED: {e}"));
    }
    *stage = next;
}

/// Whether the Steam overlay is open: its renderer's exports `SteamOverlayIsUsing{Mouse,Keyboard,
/// Gamepad}` (C:\Program Files (x86)\Steam\GameOverlayRenderer64.dll), false without the overlay.
fn steam_overlay_open() -> bool {
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::core::s;
    let Ok(dll) = (unsafe { GetModuleHandleA(s!("GameOverlayRenderer64.dll")) }) else {
        return false;
    };
    [s!("SteamOverlayIsUsingMouse"), s!("SteamOverlayIsUsingKeyboard"), s!("SteamOverlayIsUsingGamepad")]
        .into_iter()
        .filter_map(|name| unsafe { GetProcAddress(dll, name) })
        // bool fn(); only the low byte is read
        .any(|f| unsafe { std::mem::transmute::<_, unsafe extern "C" fn() -> u8>(f)() } != 0)
}

/// The menus follow the mouse even without the focus (suspected on 2026-10-05, not proven): note
/// where the mouse is at each press, for the log.
fn mouse_note() -> &'static str {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId, WindowFromPoint};
    let ours = |w| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(w, Some(&mut pid)) };
        pid == unsafe { GetCurrentProcessId() }
    };
    let mut p = POINT::default();
    let over = unsafe { GetCursorPos(&mut p) }.is_ok() && ours(unsafe { WindowFromPoint(p) });
    match (ours(unsafe { GetForegroundWindow() }), over) {
        (true, true) => "; focused, mouse over the game",
        (true, false) => "; focused",
        (false, true) => "; mouse over the game",
        (false, false) => "",
    }
}

fn distance(a: [f32; 4], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Whether the quick boot has finished (or failed: nothing more will move the player).
pub fn settled() -> bool {
    matches!(*STAGE.lock().unwrap_or_else(|e| e.into_inner()), Stage::Done | Stage::Failed(_))
}

/// For the dev channel: the current stage.
pub fn status() -> String {
    let stage = STAGE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    format!("{stage:?}, Steam overlay open {}{}", steam_overlay_open(), mouse_note())
}
