//! Pathfinder's grappling hook (the user's 2026-10-09 ask: "keep Apex's feel"), Q's other ability
//! (the weapon wheel picks it: `select`), with no cooldown. Apex's numbers, from the retail
//! settings (apex-data/export: `mp_ability_grapple.txt`, the player class `pilot_survival_grapple`;
//! Apex units, inches):
//!
//! - the hook flies at `grapple_shootVel` 2000 u/s, out to `grapple_maxLength` 850; a miss reels back;
//! - attached, the pull's speed ramps from `grapple_speedRampMin` 50 to `grapple_speedRampMax` 800
//!   over `grapple_speedRampTime` 1.5 s, reached at `grapple_accel` 1500 u/s² along the cable;
//!   across it the speed decays at `grapple_decel` 425 u/s² but for what goes the move keys' way
//!   and what falls; gravity `grapple_gravityFracMin`..`Max` 0.25..0.7 of itself (whole with the
//!   hook steeply below); the move keys push at `grapple_airAccel` 650 u/s² up to
//!   `grapple_airSpeedMax` 420; `grapple_attachVerticalBoost` 200 up off the ground;
//! - it lets go within `grapple_detachLengthMax` 50 of the hook, on a jump, crouch or Q, with the
//!   view turned past ini `grapple_detach_angle` (90°) from the hook or the hook out of sight, after
//!   `grapple_detachLowSpeedTime` 1.5 s under `grapple_detachLowSpeedThreshold` 250 u/s, or after
//!   MAX_ATTACHED; letting go adds `grapple_detachVerticalBoost` 200 up to
//!   `grapple_detachVerticalMaxSpeed` 200 and, above `grapple_detachSpeedLossMin` 460 u/s, takes
//!   `grapple_detachSpeedLoss` 300 off the horizontal speed (not below 460).
//!
//! The pull itself follows Season 3's r5apex.exe (the R5Reloaded build: the code that reads these
//! settings, 0x1406441B0 / 0x1406444C0 / 0x140A04401, read 2026-10-10; see `update`). The sounds are Apex's (`--set grapple`); hud/beam.rs draws the cable.

use std::sync::Mutex;

use glam::Vec3;

use super::kcc::UNITS_PER_METRE as UPM;
use crate::log;

const SHOOT_SPEED: f32 = 2000.0;
const MAX_LENGTH: f32 = 850.0;
const RAMP_MIN: f32 = 50.0;
const RAMP_MAX: f32 = 800.0;
const RAMP_TIME: f32 = 1.5;
const ACCEL: f32 = 1500.0;
const DECEL: f32 = 425.0;
/// The hook this far below the line of the body (the sine of its angle): gravity whole (推断:
/// a convar the code reads, its value not in the data).
const STEEP_BELOW: f32 = 0.5;
const AIR_ACCEL: f32 = 650.0;
const AIR_SPEED_MAX: f32 = 420.0;
const GRAVITY_MIN: f32 = 0.25;
const GRAVITY_MAX: f32 = 0.7;
const ATTACH_BOOST: f32 = 200.0;
const DETACH_LENGTH: f32 = 50.0;
const DETACH_BOOST: f32 = 200.0;
const DETACH_VERTICAL_MAX: f32 = 200.0;
const SPEED_LOSS: f32 = 300.0;
const SPEED_LOSS_MIN: f32 = 460.0;
const LOW_SPEED: f32 = 250.0;
const LOW_SPEED_TIME: f32 = 1.5;
/// Seconds attached at most (推断: Apex's hook holds while the power lasts).
const MAX_ATTACHED: f32 = 4.0;
/// The reel back after a miss or a detach (s).
const RETRACT: f32 = 0.25;

/// Q's ability (the weapon wheel's choice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QAbility {
    Stim,
    Grapple,
}

static Q: Mutex<QAbility> = Mutex::new(QAbility::Stim);

pub fn q_ability() -> QAbility {
    *Q.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn select(a: QAbility, by: &str) -> String {
    *Q.lock().unwrap_or_else(|e| e.into_inner()) = a;
    if a != QAbility::Grapple {
        release("ability changed");
    }
    format!("q ability ({by}): {a:?}")
}

#[derive(Clone, Copy, Debug)]
enum State {
    Idle,
    /// the hook flying: from, direction, how far it has gone, where it will catch (None: a miss)
    Shooting { from: Vec3, dir: Vec3, gone: f32, hit: Option<f32> },
    /// `blocked`: seconds the line from the eye to the hook has been cut by the map
    Attached { anchor: Vec3, t: f32, slow: f32, blocked: f32 },
    /// reeling back from `at` over RETRACT
    Retracting { at: Vec3, t: f32 },
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static KEYS_WERE: Mutex<bool> = Mutex::new(false);
static CROUCH_WERE: Mutex<bool> = Mutex::new(false);

/// The line to the hook cut this long lets go (推断: Apex's exact rule not read).
const BLOCKED_TIME: f32 = 0.15;
/// The view turned this many degrees from the hook lets go (ini `grapple_detach_angle`; 推断: the
/// value Apex uses not found in its code).
const DETACH_ANGLE: f32 = 90.0;

fn detach_angle() -> f32 {
    crate::paths::config("grapple_detach_angle").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(DETACH_ANGLE).clamp(10.0, 180.0)
}

fn eye() -> Option<(Vec3, Vec3)> {
    let (eye, fwd, _, _) = crate::camera::view()?;
    Some((eye, fwd.normalize_or_zero()))
}

/// Q with the grapple: fire the hook, or let go of it.
pub fn press(by: &str) -> String {
    let state = *STATE.lock().unwrap_or_else(|e| e.into_inner());
    match state {
        State::Attached { anchor, .. } => {
            detach(anchor);
            format!("grapple ({by}): let go")
        }
        State::Shooting { .. } | State::Retracting { .. } => format!("grapple ({by}): busy"),
        State::Idle => {
            let Some((eye, fwd)) = eye() else { return format!("grapple ({by}): no view") };
            let reach = MAX_LENGTH / UPM;
            let hit = super::gun::map_ray(eye, fwd * reach).map(|p| (p - eye).length());
            *STATE.lock().unwrap_or_else(|e| e.into_inner()) = State::Shooting { from: eye, dir: fwd, gone: 0.0, hit };
            crate::audio::play("pilot_grapple_fire", 0.6);
            format!("grapple ({by}): fired, {}", hit.map_or("nothing in reach".into(), |d| format!("catches {d:.1} m out")))
        }
    }
}

/// Lets go without the detach's boost (an ability change, leaving the world).
pub fn release(why: &str) {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if !matches!(*s, State::Idle) {
        *s = State::Idle;
        crate::audio::stop("pilot_grapple_traverse_1p");
        log(format!("grapple: released ({why})"));
    }
}

fn detach(anchor: Vec3) {
    crate::audio::stop("pilot_grapple_traverse_1p");
    crate::audio::play("pilot_grapple_retract_1p", 0.6);
    if let Some((v, _)) = super::kcc::velocity() {
        let mut v = v * UPM;
        let h = Vec3::new(v.x, 0.0, v.z);
        let speed = h.length();
        if speed > SPEED_LOSS_MIN {
            let k = (speed - SPEED_LOSS).max(SPEED_LOSS_MIN) / speed;
            v.x *= k;
            v.z *= k;
        }
        if v.y < DETACH_VERTICAL_MAX {
            v.y = (v.y + DETACH_BOOST).min(DETACH_VERTICAL_MAX);
        }
        super::kcc::pull(v / UPM, 1.0);
    }
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = State::Retracting { at: anchor, t: 0.0 };
}

fn jump_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    crate::cursor::game_in_front() && unsafe { GetAsyncKeyState(0x20) } as u16 & 0x8000 != 0
}

/// Once a frame (octane.rs, before the controller steps): the hook's flight, the pull, letting go.
pub fn update(dt: f32) {
    let jump = jump_down();
    let jumped = jump && !std::mem::replace(&mut *KEYS_WERE.lock().unwrap_or_else(|e| e.into_inner()), jump);
    let crouch = crate::input::move_keys().is_some_and(|k| k.crouch || k.crouch_toggle);
    let crouched = crouch && !std::mem::replace(&mut *CROUCH_WERE.lock().unwrap_or_else(|e| e.into_inner()), crouch);
    let state = *STATE.lock().unwrap_or_else(|e| e.into_inner());
    let next = match state {
        State::Idle => State::Idle,
        State::Shooting { from, dir, gone, hit } => {
            let gone = gone + SHOOT_SPEED / UPM * dt;
            match hit {
                Some(d) if gone >= d => {
                    let anchor = from + dir * d;
                    crate::audio::play("default_grapple_impact_1p_vs_3p", 0.6);
                    crate::audio::play("pilot_grapple_traverse_1p", 0.45);
                    if let Some((v, true)) = super::kcc::velocity() {
                        let mut v = v * UPM;
                        v.y = v.y.max(ATTACH_BOOST);
                        super::kcc::pull(v / UPM, GRAVITY_MAX);
                    }
                    log(format!("grapple: attached {d:.1} m out"));
                    State::Attached { anchor, t: 0.0, slow: 0.0, blocked: 0.0 }
                }
                None if gone >= MAX_LENGTH / UPM => {
                    crate::audio::play("pilot_grapple_retract_1p", 0.6);
                    State::Retracting { at: from + dir * gone, t: 0.0 }
                }
                _ => State::Shooting { from, dir, gone, hit },
            }
        }
        State::Attached { anchor, t, slow, blocked } => {
            let (Some(feet), Some((v, _))) = (super::kcc::feet(), super::kcc::velocity()) else {
                release("no controller");
                return;
            };
            // the pull goes from the body's middle (about a metre up)
            let me = feet + Vec3::Y;
            let to = (anchor - me) * UPM;
            let dist = to.length();
            let v = v * UPM;
            let slow = if v.length() < LOW_SPEED { slow + dt } else { 0.0 };
            // Apex's other ways to let go (the user, 2026-10-10): crouch, the view turned too far
            // from the hook (ini `grapple_detach_angle`, degrees), the hook out of sight (the map
            // between the eye and it for BLOCKED_TIME)
            let mut blocked = blocked;
            let mut away = false;
            if let Some((eye, fwd)) = eye() {
                let to_hook = anchor - eye;
                let d = to_hook.length();
                if d > 0.5 {
                    away = fwd.dot(to_hook / d) < detach_angle().to_radians().cos();
                    let cut = super::gun::map_ray(eye, to_hook * ((d - 0.5) / d)).is_some();
                    blocked = if cut { blocked + dt } else { 0.0 };
                }
            }
            if dist < DETACH_LENGTH || jumped || crouched || away || blocked > BLOCKED_TIME || slow > LOW_SPEED_TIME || t > MAX_ATTACHED {
                detach(anchor);
                return;
            }
            let dir = to / dist;
            // r5apex.exe's pull (Season 3, 0x1406444C0), per frame:
            // - along the cable only speeding up, at ACCEL, to the ramped speed (0x1406441B0); never
            //   braked by the cable when faster;
            // - across it the speed decays at DECEL, except what goes the way of the move keys and
            //   what falls (kept whole: the swing, gravity not fought);
            // - the move keys' own push is the air acceleration (AIR_ACCEL up to AIR_SPEED_MAX).
            let speed = RAMP_MIN + (RAMP_MAX - RAMP_MIN) * (t / RAMP_TIME).min(1.0);
            let along = v.dot(dir);
            let mut across = v - dir * along;
            let along = if along < speed { (along + ACCEL * dt).min(speed) } else { along };
            let mut kept = Vec3::ZERO;
            if across.y < 0.0 {
                kept.y = across.y;
                across.y = 0.0;
            }
            let wish = super::kcc::wish();
            let wish = wish - dir * wish.dot(dir);
            if wish.length() > 1e-3 {
                let w = wish.normalize();
                let d = across.dot(w);
                if d > 0.0 {
                    kept += w * d;
                    across -= w * d;
                }
                // the keys' push, Quake-style: up to AIR_SPEED_MAX their way
                let room = AIR_SPEED_MAX * wish.length().min(1.0) - (across + kept).dot(w);
                if room > 0.0 {
                    kept += w * (AIR_ACCEL * dt).min(room);
                }
            }
            let left = across.length();
            if left > 1e-3 {
                across *= (left - DECEL * dt).max(0.0) / left;
            }
            let v = dir * along + across + kept;
            // gravity (0x140A04401): all of it when the hook is steeply below; else the least
            // (GRAVITY_MIN) towards GRAVITY_MAX as the body falls along the swing or moves away
            // from the hook (推断: the scale of those speeds, STEEP_BELOW)
            let gravity = if dir.y < -STEEP_BELOW {
                1.0
            } else {
                let flat = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();
                let away = (-v.dot(flat) / 100.0).clamp(0.0, 1.0);
                let falling = if v.y < 0.0 { (-v.y / 100.0).clamp(0.0, 1.0) } else { 0.0 };
                GRAVITY_MIN + (GRAVITY_MAX - GRAVITY_MIN) * away.max(falling)
            };
            super::kcc::pull(v / UPM, gravity);
            State::Attached { anchor, t: t + dt, slow, blocked }
        }
        State::Retracting { at, t } => {
            if t + dt >= RETRACT {
                State::Idle
            } else {
                State::Retracting { at, t: t + dt }
            }
        }
    };
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = next;
}

/// The cable for the HUD (hud/beam.rs): the hook's end (world), and how far reeled back (0..1).
pub fn cable() -> Option<(Vec3, f32)> {
    match *STATE.lock().unwrap_or_else(|e| e.into_inner()) {
        State::Idle => None,
        State::Shooting { from, dir, gone, .. } => Some((from + dir * gone, 0.0)),
        State::Attached { anchor, .. } => Some((anchor, 0.0)),
        State::Retracting { at, t } => Some((at, (t / RETRACT).clamp(0.0, 1.0))),
    }
}

/// Whether the hook holds (the HUD's tactical slot looks in use).
pub fn attached() -> bool {
    matches!(*STATE.lock().unwrap_or_else(|e| e.into_inner()), State::Attached { .. })
}
