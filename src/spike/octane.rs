//! Octane's abilities (D-027..D-029; plan docs/plan/plan-octane.md A2/A3), by Season 3's rules
//! (docs/research/s3-octane-reference.md).
//!
//! The stim (S3 `mp_ability_heal`, A2): Q on the keyboard (dev `stim`). It costs 20 HP, never
//! below 1 (S3 `octane_health_cost`), and gives the `speed_boost` status 0.205 for 6 s plus a
//! 0.5 s fade (`StatusEffect_AddTimed(.., duration + 0.5, 0.5)`, sh_stim.gnut). Apex keeps a
//! severity as a byte (52/255) and fades it out over the last 0.5 s as (1 - cos(pi r / 0.5)) / 2 of
//! it, r the time left (0x14089D640). The movement controller scales its speeds by 1 + 2 x the
//! severity and slows the slide's decay (er-apex-move `MoveInput::speed_boost`). No new stim while one
//! lasts (`OnWeaponAttemptOffhandSwitch_ability_heal`). The cooldown (plan R2, S3's native charge
//! rules): see `STIM_CHARGE_COOLDOWN`.
//!
//! The launch pad (S3 `mp_weapon_jump_pad`, A3): Z on the keyboard (dev `jumppad`). Tossed from 15 units
//! ahead of the eye at 1000 units/s, pitched up 2°, with the player's velocity on top
//! (`grenade_inherit_owner_velocity`), under gravity (`sv_gravity` 750); each frame a ray through
//! the movement controller's triangles finds what it hits: a surface facing up at least 0.7
//! (`JUMP_PAD_ANGLE_LIMIT`) takes it, a wall throws it back at 0.3 of its speed
//! (`grenade_bounce_vel_frac_sharp`; S3's bounce rules simplified). 90 s cooldown from the toss
//! (ammo 90, refill 1/s). One pad at a time, the newest (D-029). Stepping into its cylinder (radius
//! 45, 32 above, 16 below; `_jump_pads.gnut`, R5R's rewrite) launches at 1000 units/s along
//! (horizontal velocity's direction, 1.7) normalized, straight up when standing (S3 0x140B8CEB0),
//! with one double jump (er-apex-move) and the trigger's view punch (15, 4, 0; viewfx.rs). Until its
//! model is in, the HUD draws a ring for it (D-029).
//!
//! The passive (Swift Mend): S3's server script is missing, so this is R5R's community rewrite
//! (`_health_regen.gnut`), marked 非原版 (D-029). See `REGEN_TICK`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use glam::Vec3;

use crate::log;
use crate::spike::{kcc, lethal};

/// S3 `STIM_DURATION`, `STIM_HEALTH_COST`, `STIM_EFFECT_DEFAULT_SEVERITY` (as Apex stores it).
const STIM_SECONDS: f32 = 6.0;
const STIM_FADE: f32 = 0.5;
const STIM_HEALTH_COST: f32 = 20.0;
const STIM_SEVERITY: f32 = 52.0 / 255.0;
/// The stim is a charge weapon (`mp_ability_heal.txt`): its 6 s charge (`charge_time`) is the stim;
/// the charge's end forces the fire (`charge_end_forces_fire`) and the charge stays full when fired
/// (`charge_remain_full_when_fired`; 0x14100F060 clears it only without that), then drains over
/// `charge_cooldown_time` 2 s after `charge_cooldown_delay` 0 (0x141013470), and a new charge may
/// start only once it is empty (`charge_allow_midway_charge` 0; 0x141008E50). So the stim comes back
/// 2 s after it ends; the ammo (200, refill 400/s) is full 0.5 s after the fire and does not hold it.
const STIM_CHARGE_COOLDOWN: f32 = 2.0;
/// From one stim to the next.
const STIM_CYCLE: f32 = STIM_SECONDS + STIM_CHARGE_COOLDOWN;

/// The launch pad, in Apex units: `mp_weapon_jump_pad` (projectile_launch_speed,
/// projectile_launch_pitch_offset, grenade_view_launch_offset, ammo 90 / regen 1), sv_gravity,
/// `SetLaunchScaleValues(1000, 1.7)`, the trigger cylinder, JUMP_PAD_ANGLE_LIMIT,
/// grenade_bounce_vel_frac_sharp. FLIGHT_MAX is ours: a toss that hits nothing in the controller's
/// window by then is dropped.
const PAD_SPEED: f32 = 1000.0;
const PAD_PITCH_UP_DEG: f32 = 2.0;
const PAD_RELEASE_SECONDS: f32 = 0.3;
const PAD_AHEAD: f32 = 15.0;
const GRAVITY: f32 = 750.0;
const PAD_COOLDOWN: f32 = 90.0;

/// The pad's cooldown: ini `pad_cooldown` (seconds, read fresh; 0: no cooldown, for tests and
/// play), else S3's `PAD_COOLDOWN`.
fn pad_cooldown() -> f32 {
    crate::paths::number::<f32>("pad_cooldown").map_or(PAD_COOLDOWN, |s| s.max(0.0))
}
const LAUNCH_SPEED: f32 = 1000.0;
const LAUNCH_UP: f32 = 1.7;
const PAD_RADIUS: f32 = 45.0;
const PAD_ABOVE: f32 = 32.0;
const PAD_BELOW: f32 = 16.0;
const PAD_ANGLE_LIMIT: f32 = 0.70;
const PAD_BOUNCE: f32 = 0.3;
const FLIGHT_MAX: f32 = 6.0;
/// 非原版 (D-029): R5R's community `_jump_pads.gnut` sets the player's gravity to 0.75 on the pad
/// ("seems to be a thing in retail") and back to 1 on the ground; Season 3's native launch has no
/// such scale (plan R1).
const PAD_GRAVITY_SCALE: f32 = 0.75;

/// 非原版 (D-029): the passive by R5R's community `_health_regen.gnut` ("values that seems to fit
/// with retail"): a tick every 0.6 s; nothing at full health, while the stim's visual effect lasts
/// (6 s) or within 5 s of damage (the stim's cost counts); else of every three ticks the first
/// shows the target, HP + int(2 x 1.85), for three ticks and the other two heal int(1.85) = 1 HP
/// each (Apex's health is a whole number): 2 HP per 1.8 s.
const REGEN_TICK: f32 = 0.6;
const REGEN_DELAY: f32 = 5.0;
const REGEN_RATE: f32 = 1.85;

/// The abilities' sounds (T018, play names of `apex-data/audio/octane`): the events S3's settings
/// and scripts emit (mp_ability_heal.txt/.nut, mp_weapon_jump_pad.txt, _jump_pads.gnut,
/// cl_jump_pads.gnut). An event with several play actions plays all its layers together (their
/// Miles delays are 待定, T018).
const SOUND_VOLUME: f32 = 0.5;
const STIM_START: &[&str] = &["octane_stimpack_activate_1p", "octane_stimpack_loop_1p"];
/// `StimEnd` emits `octane_stimpack_deactivate_1P` 2 s before the stim ends. Its first play action
/// (the warning) plays then; the other two (the deactivation, the canister) 2 s later, as the stim
/// ends: the user's R5R video (2026-10-05 16:53; stim at 24.60 s) has the warning at 28.60 s and
/// both others at 30.60–30.70 s. Where Miles keeps that delay in the event is 待定.
const STIM_ENDING: &[&str] = &["octane_stimpack_deactivate_1p_layer0"];
const STIM_ENDED: &[&str] = &["octane_stimpack_deactivate_1p_layer1", "octane_stimpack_deactivate_1p_layer2"];
/// The injector's throw, by flourish (pov/ability.rs `FLOURISHES`): each `holster_*` sequence of its
/// QC plays `Octane_Stim_Release_<flourish>_1P` at frame 0 (the user's R5R video: Release_Throw at
/// 30.94 s, the throw). The QC's common `Octane_Stim_Release_1P` is left out (export_audio.py).
const STIM_RELEASE: [&[&str]; 6] = [
    &["octane_stim_release_backtoss_1p"],
    &["octane_stim_release_micdrop_1p"],
    &["octane_stim_release_rocker_1p"],
    &["octane_stim_release_spin_1p"],
    &["octane_stim_release_throw_1p"],
    &["octane_stim_release_throw_akimbo_1p"],
];
/// S3 `OnWeaponChargeBegin_ability_heal`: `PlayBattleChatterLineToSpeakerAndTeam( player,
/// "bc_tactical" )` when the stim starts, then not again for `RandomFloatRange( 20.0, 40.0 )` s
/// (`lastStimChatterTime`). The line comes 0.30 s after the stim in the user's R5R video
/// (`diag_mp_octane_bc_tactical_01_03_1p` from 24.90 s); English, the video's language.
const STIM_VOICE: &[&str] = &["diag_mp_octane_bc_tactical_1p"];
const STIM_VOICE_DELAY: f32 = 0.30;
const STIM_VOICE_GAP: (f32, f32) = (20.0, 40.0);
const NOT_READY: &[&str] = &["survival_ui_ability_notready"];
const TACTICAL_READY: &[&str] = &["survival_ui_tactical_ready"];
const ULTIMATE_READY: &[&str] = &["survival_ui_ultimate_ready"];
const PAD_THROW: &[&str] = &["jumppad_throw_layer0", "jumppad_throw_layer1"];
const PAD_DEPLOY: &[&str] = &["jumppad_deploy_unpack"];
const PAD_LAUNCH: &[&str] = &[
    "jumppad_launchplayer_1p_layer0",
    "jumppad_launchplayer_1p_layer1",
    "jumppad_ascent_windrush",
];
const PAD_DOUBLE_JUMP: &[&str] = &[
    "jumppad_doublejump_1p_layer0",
    "jumppad_doublejump_1p_layer1",
    "jumppad_doublejump_1p_layer2",
];

fn sound(names: &[&'static str], delay: f32) {
    for &name in names {
        crate::audio::play_in(name, SOUND_VOLUME, delay);
    }
}

#[derive(Clone, Copy, Debug)]
enum Pad {
    None,
    Throwing {
        since: Instant,
    },
    /// world metres and metres/s; the heading it was tossed along (radians from +Z towards +X)
    Flying {
        at: Vec3,
        velocity: Vec3,
        since: Instant,
        yaw: f32,
    },
    /// where it stands, its up (the surface's), the heading it was tossed along, since when
    Planted {
        at: Vec3,
        up: Vec3,
        yaw: f32,
        since: Instant,
    },
}

struct Octane {
    /// When the stim (the charge) starts: STIM_DEPLOY after the key, while the left hand pulls
    /// the injector out; whether its start (the cost, the sounds) has been done; when the voice
    /// line may come again; the voice gap's random state.
    stim_started: Option<Instant>,
    stim_begun: bool,
    next_voice: Option<Instant>,
    rng: u32,
    last_tactical: bool,
    last_ultimate: bool,
    pad: Pad,
    tossed: Option<Instant>,
    /// the player's feet were in the pad's cylinder last frame (it launches on the way in)
    on_pad: bool,
    /// when the pad last launched someone (its bounce, R4)
    launched: Option<Instant>,
    /// the passive: seconds into the current tick, the tick in its cycle of three, the target the
    /// HUD shows and until when, whether it is healing (for the log)
    regen_clock: f32,
    regen_tick: u8,
    regen_target: Option<(f32, Instant)>,
    regen_on: bool,
    /// whether each ability was ready last frame (the ready sounds), the movement frame last
    /// looked at (the double jump's sound)
    tactical_was_ready: bool,
    ultimate_was_ready: bool,
    move_frame: u64,
    /// the injector's throw sound: when it is due and the flourish (moved up when a reload throws
    /// the injector early)
    release_due: Option<(Instant, usize)>,
}

static OCTANE: Mutex<Octane> = Mutex::new(Octane {
    stim_started: None,
    stim_begun: false,
    next_voice: None,
    rng: 0,
    last_tactical: false,
    last_ultimate: false,
    pad: Pad::None,
    tossed: None,
    on_pad: false,
    launched: None,
    regen_clock: 0.0,
    regen_tick: 0,
    regen_target: None,
    regen_on: false,
    tactical_was_ready: true,
    ultimate_was_ready: true,
    move_frame: 0,
    release_due: None,
});

/// Apex units -> metres.
fn m(units: f32) -> f32 {
    units / kcc::UNITS_PER_METRE
}

/// Seconds since `i`, negative while it is still ahead (the stim's start during the pull-out).
fn since(i: Instant) -> f32 {
    let now = Instant::now();
    match now.checked_duration_since(i) {
        Some(d) => d.as_secs_f32(),
        None => -(i - now).as_secs_f32(),
    }
}

/// Uniform 0..1 (xorshift, seeded from the clock).
fn next_random(s: &mut u32) -> f32 {
    if *s == 0 {
        *s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos()) | 1;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// The stim's `speed_boost` severity `t` seconds after it started.
fn stim_severity(t: f32) -> f32 {
    let end = STIM_SECONDS + STIM_FADE;
    if !(0.0..end).contains(&t) {
        return 0.0;
    }
    let left = end - t;
    if left > STIM_FADE {
        STIM_SEVERITY
    } else {
        STIM_SEVERITY * (1.0 - (std::f32::consts::PI * left / STIM_FADE).cos()) * 0.5
    }
}

/// Once a frame (before the movement controller steps): the ability keys, the tossed pad's flight,
/// the pad's launch.
pub fn update(dt: f32) {
    if !crate::state::in_world() || !crate::mode::apex() || lethal::fuse_hp().is_some_and(|hp| hp <= 0.0) {
        let mut o = octane();
        o.stim_started = None;
        o.stim_begun = false;
        o.pad = Pad::None;
        o.tossed = None;
        o.on_pad = false;
        o.last_tactical = false;
        o.last_ultimate = false;
        o.regen_clock = 0.0;
        o.regen_tick = 0;
        o.regen_target = None;
        o.regen_on = false;
        drop(o);
        super::grapple::release("out of play");
        return;
    }
    begin_stim();
    regen(dt.clamp(0.0, 0.1));
    if !crate::fe::in_play_view() || kcc::feet().is_none() {
        let mut o = octane();
        o.last_tactical = false;
        o.last_ultimate = false;
        o.on_pad = false;
        return;
    }
    let (tactical, ultimate) = crate::input::ability_keys().unwrap_or((false, false));
    let (q, z) = {
        let mut o = OCTANE.lock().unwrap_or_else(|e| e.into_inner());
        let pressed = (tactical && !o.last_tactical, ultimate && !o.last_ultimate);
        (o.last_tactical, o.last_ultimate) = (tactical, ultimate);
        pressed
    };
    // Q: the stim, or Pathfinder's grapple when the weapon wheel picked it (grapple.rs)
    if q {
        match super::grapple::q_ability() {
            super::grapple::QAbility::Stim => log(format!("octane: {}", stim("key Q"))),
            super::grapple::QAbility::Grapple => log(super::grapple::press("key Q")),
        }
    }
    super::grapple::update(dt.clamp(0.0, 0.1));
    if z {
        log(format!("octane: {}", toss("key Z")));
    }
    fly(dt.clamp(0.0, 0.1));
    launch_check();
    cue_sounds();
}

/// The ready sounds when an ability comes back, the double jump's.
fn cue_sounds() {
    let release = {
        let mut o = octane();
        match o.release_due {
            Some((at, flourish)) if Instant::now() >= at => {
                o.release_due = None;
                Some(flourish)
            }
            _ => None,
        }
    };
    if let Some(flourish) = release {
        sound(STIM_RELEASE[flourish], 0.0);
    }
    let state = abilities();
    let double_jumped = kcc::locomotion()
        .filter(|l| l.events.double_jumped)
        .map(|l| l.frame);
    let mut o = octane();
    let (tactical, ultimate) = (state.tactical_left <= 0.0, state.ultimate_left <= 0.0);
    if tactical && !o.tactical_was_ready {
        sound(TACTICAL_READY, 0.0);
    }
    // (no ready cue after every toss when the pad has no cooldown to speak of)
    if ultimate && !o.ultimate_was_ready && pad_cooldown() >= 1.0 {
        sound(ULTIMATE_READY, 0.0);
    }
    (o.tactical_was_ready, o.ultimate_was_ready) = (tactical, ultimate);
    if let Some(frame) = double_jumped
        && frame != o.move_frame
    {
        o.move_frame = frame;
        sound(PAD_DOUBLE_JUMP, 0.0);
    }
}

/// The OCTANE lock is never held while calling into kcc.rs: the movement controller's step, which
/// holds its own lock, asks `speed_boost` for this lock.
fn octane() -> std::sync::MutexGuard<'static, Octane> {
    OCTANE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tosses the launch pad if it can (the key or the dev channel); what happened.
pub fn toss(by: &str) -> String {
    if !can_use() {
        return format!("pad ({by}): needs a living player in play with kcc = 1");
    }
    let cooldown = pad_cooldown();
    if let Some(t) = octane().tossed.map(|t| t.elapsed().as_secs_f32())
        && t < cooldown
    {
        sound(NOT_READY, 0.0);
        return format!("pad ({by}): cooling down, {:.0} s left", cooldown - t);
    }
    let now = Instant::now();
    let mut o = octane();
    o.pad = Pad::Throwing { since: now };
    o.tossed = Some(now);
    o.ultimate_was_ready = false;
    o.on_pad = false;
    drop(o);
    // another offhand: the shield battery's use ends (S3 switches weapons, D-032)
    super::battery::cancel(by);
    // the first-person toss, its release frame on PAD_RELEASE_SECONDS (pov/ability.rs)
    let vm = super::pov::start_ability(super::pov::AbilityKind::Pad).unwrap_or_else(|| "pov: no view model".into());
    format!("pad ({by}): preparing, release in {PAD_RELEASE_SECONDS} s, cooldown {cooldown} s; {vm}")
}

fn can_use() -> bool {
    crate::state::in_world()
        && crate::fe::in_play_view()
        && !lethal::fuse_hp().is_some_and(|hp| hp <= 0.0)
        && kcc::feet().is_some()
}

/// S3's launch direction uses the horizontal velocity's direction, or straight up at rest.
fn launch_velocity(horizontal: Vec3) -> Vec3 {
    let h = Vec3::new(horizontal.x, 0.0, horizontal.z).normalize_or_zero();
    (h + Vec3::Y * LAUNCH_UP).normalize() * m(LAUNCH_SPEED)
}

fn in_pad(feet: Vec3, at: Vec3) -> bool {
    let d = feet - at;
    Vec3::new(d.x, 0.0, d.z).length_squared() <= m(PAD_RADIUS).powi(2)
        && d.y >= -m(PAD_BELOW)
        && d.y <= m(PAD_ABOVE)
}

/// Read the current eye and owner velocity at the release event, after the 0.3 s windup.
fn release_pad() -> Pad {
    let Some((eye, fwd, _, _)) = crate::camera::eye_view() else {
        return Pad::None;
    };
    // the view's pitch raised by 2°, along the view's heading
    let fwd = fwd.normalize_or_zero();
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
    let pitch = fwd.y.clamp(-1.0, 1.0).asin() + PAD_PITCH_UP_DEG.to_radians();
    let dir = flat * pitch.cos() + Vec3::Y * pitch.sin();
    let carried = kcc::locomotion().map_or(Vec3::ZERO, |l| l.velocity);
    let at = eye + fwd * m(PAD_AHEAD);
    let velocity = dir * m(PAD_SPEED) + carried;
    log(format!(
        "octane: pad released at {at:.2}, {:.1} m/s (carried {:.1})",
        velocity.length(),
        carried.length()
    ));
    Pad::Flying {
        at,
        velocity,
        since: Instant::now(),
        yaw: flat.x.atan2(flat.z),
    }
}

/// The tossed pad's flight: gravity, then a ray through the controller's triangles for this frame.
fn fly(dt: f32) {
    let pad = octane().pad;
    if let Pad::Throwing { since } = pad {
        if since.elapsed().as_secs_f32() >= PAD_RELEASE_SECONDS {
            let released = release_pad();
            if matches!(released, Pad::Flying { .. }) {
                sound(PAD_THROW, 0.0);
            }
            octane().pad = released;
        }
        return;
    }
    let Pad::Flying {
        at,
        velocity,
        since,
        yaw,
    } = pad
    else {
        return;
    };
    if since.elapsed().as_secs_f32() > FLIGHT_MAX {
        octane().pad = Pad::None;
        log("octane: pad fell without a surface to land on (outside the controller's window?)");
        return;
    }
    let g = Vec3::Y * m(GRAVITY);
    let next = at + velocity * dt - g * (0.5 * dt * dt);
    let next_velocity = velocity - g * dt;
    let pad = match kcc::ray_cast(at, next) {
        Some((hit, normal)) if normal.y >= PAD_ANGLE_LIMIT => {
            log(format!(
                "octane: pad planted at {hit:.2} (surface up {:.2}) after {:.2} s",
                normal.y,
                since.elapsed().as_secs_f32()
            ));
            sound(PAD_DEPLOY, 0.0);
            Pad::Planted {
                at: hit,
                up: normal,
                yaw,
                since: Instant::now(),
            }
        }
        Some((hit, normal)) => {
            let v = next_velocity - normal * (2.0 * next_velocity.dot(normal));
            Pad::Flying {
                at: hit + normal * 0.02,
                velocity: v * PAD_BOUNCE,
                since,
                yaw,
            }
        }
        None => Pad::Flying {
            at: next,
            velocity: next_velocity,
            since,
            yaw,
        },
    };
    octane().pad = pad;
}

/// Launches the player stepping into the planted pad's cylinder (on the way in).
fn launch_check() {
    let pad = octane().pad;
    let Pad::Planted { at, .. } = pad else {
        octane().on_pad = false;
        return;
    };
    let Some(feet) = kcc::feet() else { return };
    let inside = in_pad(feet, at);
    let entered = {
        let mut o = octane();
        let entered = inside && !o.on_pad;
        o.on_pad = inside;
        entered
    };
    if !entered {
        return;
    }
    let h = kcc::locomotion().map_or(Vec3::ZERO, |l| l.velocity);
    let velocity = launch_velocity(h);
    let half_height = kcc::hull_half_height();
    let ok = kcc::launch(velocity, true, PAD_GRAVITY_SCALE);
    if ok {
        sound(PAD_LAUNCH, 0.0);
        octane().launched = Some(Instant::now());
    }
    // the trigger's view punch (S3 0x140B8CB10), by the eye's pitch before the launch
    let pitch =
        crate::camera::eye_view().map(|(_, fwd, _, _)| -fwd.y.clamp(-1.0, 1.0).asin().to_degrees());
    if let (Some(pitch), Some(half_height)) = (pitch, half_height) {
        crate::viewfx::pad_launch(pitch, half_height);
    }
    log(format!(
        "octane: launched at {velocity:.2} m/s ({}) with a double jump, gravity x{PAD_GRAVITY_SCALE} until landing (community value), view punch at pitch {}",
        if ok { "ok" } else { "no controller" },
        pitch.map_or("-".into(), |p| format!("{p:.1}°"))
    ));
}

/// What the passive does on a tick (see `REGEN_TICK`).
#[derive(Debug, PartialEq)]
enum Regen {
    /// full health: nothing (the cycle goes on where it was)
    Full,
    /// the stim or recent damage: the cycle starts over
    Wait,
    /// first tick of the cycle: show this target
    Target(f32),
    /// heal this much
    Heal(f32),
}

/// One tick of the passive at `hp`; `tick` is the tick in its cycle and moves on.
fn regen_step(hp: f32, stimmed: bool, since_damage: Option<f32>, tick: &mut u8) -> Regen {
    if hp >= lethal::FUSE_MAX_HP {
        return Regen::Full;
    }
    if stimmed || since_damage.is_some_and(|t| t < REGEN_DELAY) {
        *tick = 0;
        return Regen::Wait;
    }
    match *tick {
        0 => {
            *tick = 1;
            Regen::Target(hp + (2.0 * REGEN_RATE).trunc())
        }
        t => {
            *tick = if t >= 2 { 0 } else { t + 1 };
            Regen::Heal(REGEN_RATE.trunc())
        }
    }
}

/// The passive, once a frame while in the world and alive (not only in play: the game runs on
/// behind its menus).
fn regen(dt: f32) {
    let Some(hp) = lethal::fuse_hp() else { return };
    let since_damage = lethal::since_damage();
    let mut o = octane();
    o.regen_clock += dt;
    if o.regen_clock < REGEN_TICK {
        return;
    }
    o.regen_clock = (o.regen_clock - REGEN_TICK).min(REGEN_TICK);
    let stimmed = o
        .stim_started
        .is_some_and(|s| since(s) < STIM_SECONDS);
    let mut tick = o.regen_tick;
    let step = regen_step(hp, stimmed, since_damage, &mut tick);
    o.regen_tick = tick;
    match step {
        Regen::Full => {
            o.regen_target = None;
            if std::mem::take(&mut o.regen_on) {
                log(format!("octane: passive: full health ({hp:.1})"));
            }
        }
        Regen::Wait => {
            o.regen_target = None;
            if std::mem::take(&mut o.regen_on) {
                log(format!(
                    "octane: passive: stopped at {hp:.1} (stim or damage)"
                ));
            }
        }
        Regen::Target(target) => {
            let until = Instant::now() + Duration::from_secs_f32(3.0 * REGEN_TICK);
            o.regen_target = Some((target.min(lethal::FUSE_MAX_HP), until));
            if !std::mem::replace(&mut o.regen_on, true) {
                log(format!(
                    "octane: passive: healing from {hp:.1} (community values)"
                ));
            }
        }
        Regen::Heal(amount) => {
            drop(o);
            lethal::heal(amount);
        }
    }
}

/// How long the stim's `stim_visual_effect` lasts (S3: its duration, no fade time added).
pub const STIM_VISUAL_SECONDS: f32 = STIM_SECONDS;

/// Seconds since the stim started, for its screen effects, until a second after the visual effect
/// ends (None before or after).
pub fn stim_age() -> Option<f32> {
    octane()
        .stim_started
        .map(since)
        .filter(|&t| (0.0..STIM_VISUAL_SECONDS + 1.0).contains(&t))
}

/// The ability slots for the HUD: how ready each is (1 ready) and the seconds until it is.
#[derive(Clone, Copy, Debug)]
pub struct Abilities {
    pub tactical_left: f32,
    /// The stim is on (or being pulled out): S3's slot looks ready meanwhile.
    pub tactical_active: bool,
    pub ultimate_ready: f32,
    pub ultimate_left: f32,
}

/// The stim is ready again 2 s after it ends (`STIM_CHARGE_COOLDOWN`); the pad 90 s after the toss.
pub fn abilities() -> Abilities {
    let o = octane();
    let part = |since: Option<Instant>, total: f32| {
        if total <= 0.0 {
            return (1.0, 0.0);
        }
        let t = since.map_or(total, |s| s.elapsed().as_secs_f32().min(total));
        (t / total, total - t)
    };
    let (_, tactical_left) = part(o.stim_started, STIM_CYCLE);
    let (ultimate_ready, ultimate_left) = part(o.tossed, pad_cooldown());
    let tactical_active = o.stim_started.is_some_and(|s| since(s) < STIM_SECONDS);
    Abilities {
        tactical_left,
        tactical_active,
        ultimate_ready,
        ultimate_left,
    }
}

/// The passive's target health for the HUD while it shows (None: not healing).
pub fn regen_target() -> Option<f32> {
    let o = octane();
    o.regen_target
        .filter(|(_, until)| Instant::now() < *until)
        .map(|(hp, _)| hp)
}

/// ER re-bases Havok coordinates while moving between outdoor tiles. Keep the pad in that frame.
/// Called with KCC held; this function must never call back into KCC.
pub fn world_shift(delta: Vec3) {
    let mut o = octane();
    match &mut o.pad {
        Pad::Flying { at, .. } | Pad::Planted { at, .. } => *at += delta,
        Pad::None | Pad::Throwing { .. } => {}
    }
}

/// For the HUD: the pad and whether it has landed (world metres; its surface's up).
pub fn pad() -> Option<(Vec3, Vec3, bool)> {
    match OCTANE.lock().unwrap_or_else(|e| e.into_inner()).pad {
        Pad::None | Pad::Throwing { .. } => None,
        Pad::Flying { at, .. } => Some((at, Vec3::Y, false)),
        Pad::Planted { at, up, .. } => Some((at, up, true)),
    }
}

/// The pad in the air or standing on the ground, for its model (R4, pov/padworld.rs): where (world
/// metres), its up, the heading it was tossed along (radians from +Z towards +X), seconds since it
/// landed (0 in the air) and since it last launched someone (if since it landed).
#[derive(Clone, Copy, Debug)]
pub struct PadWorld {
    pub at: Vec3,
    pub up: Vec3,
    pub yaw: f32,
    pub age: f32,
    pub bounce: Option<f32>,
}

pub fn pad_world() -> Option<PadWorld> {
    let o = OCTANE.lock().unwrap_or_else(|e| e.into_inner());
    match o.pad {
        // in the air: closed (the deploy's first frame), upright
        Pad::Flying { at, yaw, .. } => Some(PadWorld { at, up: Vec3::Y, yaw, age: 0.0, bounce: None }),
        Pad::Planted { at, up, yaw, since } => {
            let bounce = o.launched.filter(|l| *l >= since).map(|l| l.elapsed().as_secs_f32());
            Some(PadWorld { at, up, yaw, age: since.elapsed().as_secs_f32(), bounce })
        }
        Pad::None | Pad::Throwing { .. } => None,
    }
}

/// The pad's trigger radius in metres (the HUD's ring).
pub fn pad_radius() -> f32 {
    m(PAD_RADIUS)
}

/// Uses the stim if it can (the key or the dev channel); what happened.
pub fn stim(by: &str) -> String {
    if !can_use() {
        return format!("stim ({by}): needs a living player in play with kcc = 1");
    }
    let mut o = OCTANE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = o.stim_started.map(since)
        && t < STIM_CYCLE
    {
        sound(NOT_READY, 0.0);
        return if t < STIM_SECONDS {
            format!("stim ({by}): still active, {:.1} s left", STIM_SECONDS - t)
        } else {
            format!(
                "stim ({by}): charge cooling down, {:.1} s left",
                STIM_CYCLE - t
            )
        };
    }
    // the stim itself starts once the left hand has the injector out (`begin_stim`)
    o.stim_started = Some(Instant::now() + Duration::from_secs_f32(super::pov::STIM_DEPLOY));
    o.stim_begun = false;
    o.tactical_was_ready = false;
    let flourish = super::pov::pick_flourish(next_random(&mut o.rng));
    drop(o);
    // another offhand: the shield battery's use ends (S3 switches weapons, D-032)
    super::battery::cancel(by);
    // the first-person injector (pov/ability.rs; nothing in third person), and its throw's sound
    let vm = match super::pov::start_ability(super::pov::AbilityKind::Stim(flourish)) {
        Some(vm) => {
            octane().release_due = Some((Instant::now() + Duration::from_secs_f32(super::pov::stim_throw_at()), flourish));
            vm
        }
        None => "pov: no view model".into(),
    };
    format!("stim ({by}): the injector out, the stim in {} s; {vm}", super::pov::STIM_DEPLOY)
}

/// A reload wants the left hand while it holds the injector (gun.rs): the injector goes now, its
/// release sound with it; the stim goes on. Whether it was held.
/// The shield battery took the hands while the left one held the injector (battery.rs): the
/// injector is gone without its throw, so its release sound does not come (the stim goes on).
pub fn drop_injector() {
    if octane().release_due.take().is_some() {
        log("octane: injector put away for the shield battery");
    }
}

pub fn throw_injector() -> bool {
    let Some((flourish, delay)) = super::pov::throw_stim() else { return false };
    octane().release_due = Some((Instant::now() + Duration::from_secs_f32(delay), flourish));
    log(format!("octane: injector thrown early for a reload, in {delay:.2} s"));
    true
}

/// The stim's start, STIM_DEPLOY after the key (S3 `OnWeaponChargeBegin_ability_heal`): the cost,
/// its sounds, the voice line if it is due.
fn begin_stim() {
    let mut o = octane();
    let Some(start) = o.stim_started else { return };
    if o.stim_begun || since(start) < 0.0 {
        return;
    }
    o.stim_begun = true;
    let now = Instant::now();
    let voice = o.next_voice.map_or(true, |t| now >= t);
    if voice {
        let r = next_random(&mut o.rng);
        let gap = STIM_VOICE_GAP.0 + (STIM_VOICE_GAP.1 - STIM_VOICE_GAP.0) * r;
        o.next_voice = Some(now + Duration::from_secs_f32(gap));
    }
    drop(o);
    sound(STIM_START, 0.0);
    if voice {
        sound(STIM_VOICE, STIM_VOICE_DELAY);
    }
    sound(STIM_ENDING, STIM_SECONDS - 2.0);
    sound(STIM_ENDED, STIM_SECONDS);
    lethal::mark_damage();
    let hp = match lethal::fuse_hp() {
        Some(hp) => {
            let after = (hp - STIM_HEALTH_COST).max(1.0);
            lethal::set_fuse_hp(after);
            format!("HP {hp:.0} -> {after:.0}")
        }
        None => "no Apex HP (lethal_guard off): no cost".into(),
    };
    log(format!(
        "octane: stim starts: {hp}, speed x{:.3} for {STIM_SECONDS} s + {STIM_FADE} s fade{}",
        1.0 + 2.0 * STIM_SEVERITY,
        if voice { ", voice line" } else { "" }
    ));
}

/// The `speed_boost` severity now (er-apex-move `MoveInput::speed_boost`).
pub fn speed_boost() -> f32 {
    let o = OCTANE.lock().unwrap_or_else(|e| e.into_inner());
    o.stim_started
        .map_or(0.0, |s| stim_severity(since(s)))
}

/// Dev channel `octane`.
pub fn status() -> String {
    let o = OCTANE.lock().unwrap_or_else(|e| e.into_inner());
    let stim = match o.stim_started.map(since) {
        Some(t) => format!("stim {t:.2} s ago, speed_boost {:.4}", stim_severity(t)),
        None => "stim not used yet".into(),
    };
    let cooldown = o
        .tossed
        .map_or(0.0, |t| (pad_cooldown() - t.elapsed().as_secs_f32()).max(0.0));
    let pad = match o.pad {
        Pad::None => "none".into(),
        Pad::Throwing { since } => format!("preparing ({:.2} s)", since.elapsed().as_secs_f32()),
        Pad::Flying { at, velocity, .. } => {
            format!("flying at {at:.2} ({:.1} m/s)", velocity.length())
        }
        Pad::Planted { at, up, .. } => {
            format!("planted at {at:.2} (up {up:.2}), player on it {}", o.on_pad)
        }
    };
    let since = lethal::since_damage().map_or("never".into(), |t| format!("{t:.1} s ago"));
    format!(
        "{stim} | pad {pad}, cooldown {cooldown:.0} s | passive tick {} {}, damage {since}",
        o.regen_tick,
        if o.regen_on { "healing" } else { "idle" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_launch_preserves_the_s3_angle_and_total_speed() {
        let rest = launch_velocity(Vec3::ZERO) * kcc::UNITS_PER_METRE;
        assert!((rest - Vec3::Y * 1000.0).length() < 0.001);
        let moving = launch_velocity(Vec3::new(3.0, -20.0, 4.0)) * kcc::UNITS_PER_METRE;
        assert!((moving.length() - 1000.0).abs() < 0.001);
        let horizontal = Vec3::new(moving.x, 0.0, moving.z);
        assert!((moving.y / horizontal.length() - 1.7).abs() < 0.0001);
        assert!((horizontal.normalize() - Vec3::new(0.6, 0.0, 0.8)).length() < 0.0001);
        // S3 has no 0.1 m/s threshold: a slow approach still gives the angled launch.
        assert!(
            (launch_velocity(Vec3::X * 0.01) - launch_velocity(Vec3::X * 10.0)).length() < 0.001
        );
    }

    #[test]
    fn pad_trigger_has_horizontal_and_vertical_limits() {
        let at = Vec3::new(2.0, 8.0, -4.0);
        assert!(in_pad(at, at));
        assert!(in_pad(at + Vec3::X * m(44.9), at));
        assert!(!in_pad(at + Vec3::X * m(45.1), at));
        assert!(!in_pad(at + Vec3::Y * m(32.1), at));
        assert!(!in_pad(at - Vec3::Y * m(16.1), at));
    }

    /// The community passive: nothing at full health, a restart on the stim or damage within 5 s,
    /// then target, +1, +1 over and over.
    #[test]
    fn regen_cycles_target_heal_heal() {
        let mut tick = 0;
        assert_eq!(regen_step(100.0, false, None, &mut tick), Regen::Full);
        assert_eq!(regen_step(50.0, true, None, &mut tick), Regen::Wait);
        assert_eq!(regen_step(50.0, false, Some(4.9), &mut tick), Regen::Wait);
        assert_eq!(tick, 0);
        assert_eq!(
            regen_step(50.0, false, Some(5.0), &mut tick),
            Regen::Target(53.0)
        );
        assert_eq!(regen_step(50.0, false, None, &mut tick), Regen::Heal(1.0));
        assert_eq!(regen_step(51.0, false, None, &mut tick), Regen::Heal(1.0));
        assert_eq!(
            regen_step(52.0, false, None, &mut tick),
            Regen::Target(55.0)
        );
        // full health keeps the cycle where it was; damage starts it over
        assert_eq!(regen_step(100.0, false, None, &mut tick), Regen::Full);
        assert_eq!(tick, 1);
        assert_eq!(regen_step(60.0, false, Some(1.0), &mut tick), Regen::Wait);
        assert_eq!(tick, 0);
    }

    /// S3's charge rules give 6 s of stim and 2 s of charge cooldown before the next.
    #[test]
    fn stim_cycle_is_charge_plus_cooldown() {
        assert_eq!(STIM_CYCLE, 8.0);
        assert!(
            STIM_CHARGE_COOLDOWN > 0.5,
            "the 0.5 s ammo refill does not hold it"
        );
    }

    /// Full for 6 s, then a cosine fade to 0 at 6.5 s (half way at 6.25 s), nothing before or after.
    #[test]
    fn stim_severity_fades_like_apex() {
        assert_eq!(stim_severity(-0.1), 0.0);
        assert_eq!(stim_severity(0.0), 52.0 / 255.0);
        assert_eq!(stim_severity(5.99), 52.0 / 255.0);
        assert!((stim_severity(6.25) - 26.0 / 255.0).abs() < 1e-5);
        assert!(stim_severity(6.49) > 0.0 && stim_severity(6.49) < 0.001);
        assert_eq!(stim_severity(6.5), 0.0);
        assert!((1.0 + 2.0 * stim_severity(1.0) - 1.408).abs() < 0.001);
    }
}
