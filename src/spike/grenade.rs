//! The frag grenade (U9): Season 3's `mp_weapon_frag_grenade` in Octane's hand, on G (Apex's PC
//! default `bind "g" "weaponSelectOrdnance"`, R5R cfg/config_default_pc.cfg). Endless (D-003, D-032;
//! the HUD shows ∞; our default, the user has not confirmed it). The report is
//! docs/m1/U9-grenade.md; the numbers are S3's, from R5R's `weapons/mp_weapon_frag_grenade.txt`
//! (retail's `apex-data/export/weapon/mp_weapon_frag_grenade.txt` has the same ones) and the S3
//! server `r5apex_ds.exe` where the settings leave a rule to native code.
//!
//! The hand (`Phase`, a weapon switch in S3): G puts the R-301 away (its `holster_time` 0.55 s,
//! _base_assault_rifle.txt) and draws the grenade (`deploy_time` 0.6 s; it can be thrown from
//! `draw_seq`'s AE_WPN_READYTOFIRE, frame 9 of 16). The trigger pulls the pin (`OnWeaponTossPrep`
//! plays `sound_deploy_1p` Weapon_FragGrenade_PinPull; the prep lasts `toss_pullout_time` 0.25 s,
//! 推断: which of ACT_VM_TOSS_PREP / _PREP_PULLOUT S3 plays from a drawn grenade is not traced), it
//! is held while the trigger is, and letting go throws it: underhand (`toss_time` 0.4 s, released at
//! `toss_seq`'s AE_WPN_TOSS_RELEASE, frame 7 of 17), or overhand when the eye looks up past
//! `offhandTossOverheadPitchThreshold` -1° (`toss_overhead_time` 0.7 s, frame 6 of 23; 0x1410095E0).
//! The fuse starts at the launch (`start_fuse_on_launch`, `grenade_fuse_time` 3.5 s; no cooking).
//! Then the R-301 comes back (its `deploy_time` 0.6 s, firing from AE_WPN_READYTOFIRE 12 of 25),
//! the grenade's own holster skipped after a throw (推断: the hand is empty). G again while it is
//! out does nothing (S3 `TryCycleOrdnance` with one kind of ordnance); 1/2, Q and Z put it away
//! before the throw (`OnWeaponTossCancel` = Grenade_OnWeaponTossCancelDrop: nothing is dropped);
//! after a throw the next one waits for `fire_rate` 0.83/s. The frames are retail's QC
//! (apex-data/assets/frag_grenade, tools/apexassets/frag_grenade_assets.py; S3's own view model is in
//! R5R's paks: 待对照).
//!
//! The throw (0x14100AA40, 0x141015A50, 0x141016140): from the eye, along the eye's angles with the
//! pitch raised by `projectile_launch_pitch_offset` 9° times clamp((pitch + 90) / 90, 0, 1) (full
//! level or below, none straight up), at `projectile_launch_speed` 1300 units/s, plus
//! `projectile_inherit_owner_velocity_scale` 1 times the player's horizontal speed along the view
//! (never backwards) along the view itself (the platform's base velocity is 0 here). Gravity: S3's
//! `sv_gravity` 750 times `projectile_gravity_scale` (1, the default).
//!
//! The flight: each frame a ray through the movement controller's triangles (kcc.rs, as the jump
//! pad's), else the game's own map ray when the grenade is outside that window (its normal from two
//! more rays). Bounces by `grenade_bounce_vel_frac_shallow` 0.6 / `_sharp` 0.4 /
//! `_along_normal` 0.3 / `_randomness` 0.05 / `_extra_vertical_randomness` 0.05 and rolling by
//! `grenade_roll_vel_frac_per_second` 0.05 (`grenade_can_roll` default 1): 推断, the native bounce
//! function was not found; see `bounce`. A grenade that bonks an enemy deals `damage_near_value` 10
//! (Grenade_Launch's touch damage, DF_EXPLOSION removed) once and bounces off it.
//!
//! The explosion (0x140B659A0, RadiusDamage 0x140CDCCB0): `explosion_damage` 100 in full within
//! `explosion_inner_radius` 125 units, falling linearly to 0 at `explosionradius` 350, measured to a
//! point three quarters of the way from the target's middle to its box's nearest point, and only
//! with a clear line (the map) to that point or to its eye. It hurts its thrower too
//! (`explosion_damages_owner`, native default 1; S3 _base_gametype.gnut ShouldEntTakeDamage: "no
//! suicide protection"). Enemies through the S6 damage bridge (`combat::shoot`).

use std::sync::Mutex;
use std::time::Instant;

use eldenring::cs::{CSHavokMan, ChrIns, FieldInsHandle, WorldChrMan};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::FromStatic;
use glam::Vec3;

use crate::log;
use crate::spike::{kcc, lethal};

// ---- S3 numbers (Apex units, seconds) ----

/// The throw: projectile_launch_speed, projectile_launch_pitch_offset (degrees),
/// projectile_inherit_owner_velocity_scale; sv_gravity x projectile_gravity_scale (default 1).
const LAUNCH_SPEED: f32 = 1300.0;
const PITCH_OFFSET: f32 = 9.0;
const INHERIT_SCALE: f32 = 1.0;
const GRAVITY: f32 = 750.0;
/// grenade_fuse_time, from the launch (start_fuse_on_launch 1).
pub const FUSE: f32 = 3.5;
/// grenade_bounce_vel_frac_shallow / _sharp / _along_normal, _randomness,
/// _extra_vertical_randomness, grenade_roll_vel_frac_per_second.
const BOUNCE_SHALLOW: f32 = 0.6;
const BOUNCE_SHARP: f32 = 0.4;
const BOUNCE_ALONG_NORMAL: f32 = 0.3;
const BOUNCE_RANDOMNESS: f32 = 0.05;
const BOUNCE_EXTRA_UP: f32 = 0.05;
const ROLL_FRAC_PER_SECOND: f32 = 0.05;
/// explosion_damage, explosion_inner_radius, explosionradius; damage_near_value (the bonk).
pub const EXPLOSION_DAMAGE: f32 = 100.0;
pub const INNER_RADIUS: f32 = 125.0;
pub const OUTER_RADIUS: f32 = 350.0;
const TOUCH_DAMAGE: f32 = 10.0;
/// fire_rate: throws per second at most.
const FIRE_RATE: f32 = 0.83;
/// The hand: the grenade's deploy_time and draw_seq's AE_WPN_READYTOFIRE (frame 9 of 16 intervals);
/// toss_pullout_time; toss_time and toss_seq's AE_WPN_TOSS_RELEASE (7 of 17); toss_overhead_time and
/// toss_overhead_seq's (6 of 23); offhandTossOverheadPitchThreshold (degrees, Source's pitch: down
/// positive); holster_seq (15 frames at 30, holster_time unset). The weapon in the hands goes away
/// over its holster_time and comes back over its deploy_time to its draw's AE_WPN_READYTOFIRE
/// (`Hand::weapon`: weapons.rs's timings, the R-301's 0.55 / 0.6 / 12 of 25 or the Charge Rifle's
/// 0.5 / 0.8 / 12 of 20).
const DEPLOY: f32 = 0.6;
const DRAW_READY: f32 = 9.0 / 16.0;
const PREP: f32 = 0.25;
const TOSS: f32 = 0.4;
const TOSS_RELEASE: f32 = 7.0 / 17.0;
const TOSS_OVERHEAD: f32 = 0.7;
const TOSS_OVERHEAD_RELEASE: f32 = 6.0 / 23.0;
const OVERHEAD_PITCH: f32 = -1.0;
const HOLSTER: f32 = 14.0 / 30.0;
/// The aim arc (0x141012580): shown aiming only (grenade_arc_indicator_show_from_hip 0), for
/// min(fuse, 2) s, through grenade_arc_indicator_bounce_count 1 bounce.
const ARC_SECONDS: f32 = 2.0;
const ARC_BOUNCES: u32 = 1;
/// S3's grenade indicator (cl_damage_indicator.gnut TryAddGrenadeIndicator / GrenadeArrowThink):
/// within the explosion radius + 65, after 0.4 s (0.15 s when the grenade and the player move less
/// than 500 units/s apart), with a clear line, and 0.25 s more after that is lost.
const THREAT_PADDING: f32 = 65.0;
const THREAT_DELAY: f32 = 0.4;
const THREAT_DELAY_SLOW: f32 = 0.15;
const THREAT_SLOW: f32 = 500.0;
const THREAT_LINGER: f32 = 0.25;

// ---- ours (推断) ----

/// How far the grenade's centre keeps off a surface: the projectile model
/// (mdl/weapons/grenades/m20_f_grenade_projectile.rmdl) is 4.3 x 4.3 x 5.7 units.
const SKIN: f32 = 2.0;
/// A bounce off a surface facing up at least this much that leaves less than REST_SPEED off it
/// puts the grenade on it, rolling; below STOP_SPEED it lies still. (JUMP_PAD_ANGLE_LIMIT's 0.7.)
const WALKABLE: f32 = 0.7;
const REST_SPEED: f32 = 50.0;
const STOP_SPEED: f32 = 5.0;
/// Bounce sounds only for impacts faster than this into the surface.
const BOUNCE_SOUND_SPEED: f32 = 60.0;
/// The player's box for the explosion: er-apex-move's standing radius (player settings), its height
/// from the controller.
fn player_radius() -> f32 {
    er_apex_move::MoveParams::default().standing.radius
}

/// The standing eye height (player settings' viewheight, units).
fn player_eye() -> f32 {
    er_apex_move::MoveParams::default().standing.viewheight
}

/// Where the grenade leaves from and along what: the first-person eye and its view (S3: the eye,
/// 0x140FEAAA0); over the shoulder (no eye view) the head over the feet along the camera's view.
fn eye_and_view() -> Option<(Vec3, Vec3)> {
    if let Some((eye, fwd, _, _)) = crate::camera::eye_view() {
        return Some((eye, fwd));
    }
    let (_, fwd, _, _) = crate::camera::view()?;
    Some((kcc::feet()? + Vec3::Y * m(player_eye()), fwd))
}

/// The sounds (tools/fuseaudio/export_audio.py `--set frag`, playback names of
/// apex-data/audio/frag_grenade): the draw (draw_seq AE_CL_PLAYSOUND 0), the pin (sound_deploy_1p),
/// the throw (sound_throw_1p), the voice (battle_chatter_event bc_frag), a bounce (bounce_effect_table
/// bounce_small, "C"), the explosion to its thrower (impact_effect_table exp_frag_grenade,
/// Sound_attacker "C"), the grenade put away (holster_seq AE_CL_PLAYSOUND 0). An event's play
/// actions play together (their Miles delays are 待定, as T018's); but for the pin: its second action
/// holds the throw's sources (wpn_fraggrenade_1p_throw_*), which the throw plays itself.
const SOUND_VOLUME: f32 = 0.5;
const SND_DRAW: &[&str] = &[
    "weapon_fraggrenade_draw_1p_layer0",
    "weapon_fraggrenade_draw_1p_layer1",
];
const SND_PIN: &[&str] = &["weapon_fraggrenade_pinpull_layer0"];
const SND_THROW: &[&str] = &["weapon_fraggrenade_throw"];
const SND_VOICE: &[&str] = &["diag_mp_octane_bc_frag_1p"];
const SND_BOUNCE: &[&str] = &[
    "phys_imp_fraggrenade_concrete_layer0",
    "phys_imp_fraggrenade_concrete_layer1",
];
const SND_EXPLODE: &[&str] = &["explo_fraggrenade_impact_1p"];
const SND_HOLSTER: &[&str] = &["weapon_p2011_unequip_layer0", "weapon_p2011_unequip_layer1"];

/// Apex units -> metres.
fn m(units: f32) -> f32 {
    units / kcc::UNITS_PER_METRE
}

/// Metres -> Apex units.
fn u(metres: f32) -> f32 {
    metres * kcc::UNITS_PER_METRE
}

/// Seconds on the module's clock.
fn now() -> f32 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

// ---- the hand ----

/// Where the grenade in hand is (times on the module's clock).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// the R-301 out
    Idle,
    /// G: the R-301 going away, then the grenade drawn (`out`: its draw has started)
    Draw { start: f32, out: bool },
    /// drawn, ready to throw
    Ready { since: f32 },
    /// the trigger: the pin pulled, then held; `released` once the trigger let go
    Prep { start: f32, released: bool },
    /// the throw; `launched` once it left the hand
    Toss {
        start: f32,
        overhead: bool,
        launched: bool,
    },
    /// put away without a throw: the grenade's holster, then the R-301
    Holster { start: f32 },
    /// the R-301 coming back from `start`
    Back { start: f32 },
}

/// What the player does this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct HandInput {
    /// G went down
    pub ordnance: bool,
    /// the trigger is held
    pub fire: bool,
    /// a weapon slot (1/2) went down
    pub weapon: bool,
    /// Q or Z went down: an ability takes the hands (it plays its own clips)
    pub ability: bool,
    /// the eye's pitch, degrees down
    pub pitch: f32,
    /// no ability's clips hold the hands (pov.rs `ability_playing`)
    pub hands_free: bool,
}

/// What happened to the hand this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandEvent {
    /// G taken: the R-301 goes away
    Drawn,
    /// the grenade's own draw starts (its sound)
    GrenadeOut,
    Ready,
    PinPulled,
    Launch {
        overhead: bool,
    },
    Back,
    Canceled,
    /// the grenade can't come out yet (an ability's clips; the fire rate)
    Waiting,
}

/// The hand's state machine (pure: the clock and the inputs are given).
#[derive(Clone, Copy, Debug)]
pub struct Hand {
    pub phase: Phase,
    /// G pressed while an ability held the hands: the draw starts once they are free (or never)
    pub queued: Option<f32>,
    /// when the last throw started (the fire rate)
    pub last_toss: f32,
    /// the weapon in the hands: it goes away over its holster time before the grenade's draw and
    /// comes back over its deploy time after (weapons.rs; taken while the hand is idle)
    pub weapon: super::weapons::Timing,
}

impl Default for Hand {
    fn default() -> Hand {
        Hand {
            phase: Phase::Idle,
            queued: None,
            last_toss: f32::NEG_INFINITY,
            weapon: super::weapons::R301_TIMING,
        }
    }
}

/// How long a G press waits for an ability to let go of the hands.
const QUEUE_MAX: f32 = 3.0;

impl Hand {
    /// Moves the hand on to `t` with this frame's input.
    pub fn step(&mut self, t: f32, i: HandInput, ev: &mut Vec<HandEvent>) {
        // an ability (Q/Z) takes the hands: before the throw the grenade goes away untouched; after
        // it the ability's clips bring their own R-301 (pov/ability.rs)
        if i.ability {
            self.queued = None;
            if self.phase != Phase::Idle {
                if !matches!(
                    self.phase,
                    Phase::Toss { launched: true, .. } | Phase::Back { .. }
                ) {
                    ev.push(HandEvent::Canceled);
                }
                self.phase = Phase::Idle;
                return;
            }
        }
        if i.ordnance {
            match self.phase {
                // (during the throw's end: drawn again once it is over)
                Phase::Idle | Phase::Back { .. } | Phase::Toss { launched: true, .. } => {
                    self.queued = Some(t)
                }
                // S3 TryCycleOrdnance: one kind of ordnance, nothing to cycle to
                _ => {}
            }
        }
        if let Some(q) = self.queued {
            if t - q > QUEUE_MAX {
                self.queued = None;
            } else if i.hands_free && matches!(self.phase, Phase::Idle | Phase::Back { .. }) {
                self.queued = None;
                self.phase = Phase::Draw {
                    start: t,
                    out: false,
                };
                ev.push(HandEvent::Drawn);
            } else {
                ev.push(HandEvent::Waiting);
            }
        }
        let cancel = i.weapon;
        match self.phase {
            Phase::Idle => {}
            Phase::Draw { start, out } => {
                if !out && t - start >= self.weapon.holster {
                    self.phase = Phase::Draw { start, out: true };
                    ev.push(HandEvent::GrenadeOut);
                }
                if cancel {
                    self.put_away(t, start + self.weapon.holster <= t, ev);
                } else if t - start >= self.weapon.holster + DEPLOY * DRAW_READY {
                    self.phase = Phase::Ready { since: t };
                    ev.push(HandEvent::Ready);
                    // a trigger held through the draw pulls the pin at once
                    self.trigger(t, i, ev);
                }
            }
            Phase::Ready { .. } => {
                if cancel {
                    self.put_away(t, true, ev);
                } else {
                    self.trigger(t, i, ev);
                }
            }
            Phase::Prep { start, released } => {
                let released = released || !i.fire;
                if cancel {
                    self.put_away(t, true, ev);
                } else if released && t - start >= PREP {
                    let overhead = i.pitch < OVERHEAD_PITCH;
                    self.phase = Phase::Toss {
                        start: t,
                        overhead,
                        launched: false,
                    };
                    self.last_toss = t;
                } else {
                    self.phase = Phase::Prep { start, released };
                }
            }
            Phase::Toss {
                start,
                overhead,
                launched,
            } => {
                let (length, release) = if overhead {
                    (TOSS_OVERHEAD, TOSS_OVERHEAD_RELEASE)
                } else {
                    (TOSS, TOSS_RELEASE)
                };
                if !launched && t - start >= length * release {
                    ev.push(HandEvent::Launch { overhead });
                    self.phase = Phase::Toss {
                        start,
                        overhead,
                        launched: true,
                    };
                }
                if t - start >= length {
                    self.phase = Phase::Back { start: t };
                    ev.push(HandEvent::Back);
                }
            }
            Phase::Holster { start } => {
                if t - start >= HOLSTER {
                    self.phase = Phase::Back { start: t };
                    ev.push(HandEvent::Back);
                }
            }
            Phase::Back { start } => {
                if t - start >= self.weapon.deploy {
                    self.phase = Phase::Idle;
                }
            }
        }
    }

    /// The trigger on a ready grenade: the pin, if the fire rate allows.
    fn trigger(&mut self, t: f32, i: HandInput, ev: &mut Vec<HandEvent>) {
        if !i.fire {
            return;
        }
        if t - self.last_toss < 1.0 / FIRE_RATE {
            ev.push(HandEvent::Waiting);
            return;
        }
        self.phase = Phase::Prep {
            start: t,
            released: false,
        };
        ev.push(HandEvent::PinPulled);
    }

    /// 1/2 before the throw: the grenade's holster (if it was out), then the weapon.
    fn put_away(&mut self, t: f32, grenade_out: bool, ev: &mut Vec<HandEvent>) {
        // weapons: draw the selected slot (1/2) instead of the R-301 (spike/weapons.rs, U8)
        self.phase = if grenade_out {
            Phase::Holster { start: t }
        } else {
            Phase::Back { start: t }
        };
        ev.push(HandEvent::Canceled);
    }

    /// Whether the weapon can't fire: from G until its pull-out reaches its ready frame.
    pub fn gun_away(&self, t: f32) -> bool {
        match self.phase {
            Phase::Idle => false,
            Phase::Back { start } => t - start < self.weapon.deploy * self.weapon.ready,
            _ => true,
        }
    }

    /// The grenade is in the hand (or on its way there).
    pub fn holding(&self) -> bool {
        matches!(
            self.phase,
            Phase::Draw { .. }
                | Phase::Ready { .. }
                | Phase::Prep { .. }
                | Phase::Toss {
                    launched: false,
                    ..
                }
        )
    }

    /// What the view model shows (pov/ordnance.rs).
    pub fn view(&self, t: f32) -> Option<HandView> {
        match self.phase {
            Phase::Idle => None,
            Phase::Draw { start, .. } if t - start < self.weapon.holster => Some(HandView::Away(t - start, self.weapon.holster)),
            Phase::Draw { start, .. } => Some(HandView::Out(t - start - self.weapon.holster)),
            // ready at DRAW_READY of the draw: the draw's clock goes on
            Phase::Ready { since } => Some(HandView::Out(t - since + DEPLOY * DRAW_READY)),
            Phase::Prep { start, .. } => Some(HandView::Prep(t - start)),
            Phase::Toss { start, overhead, .. } => Some(HandView::Toss(t - start, overhead)),
            Phase::Holster { start } => Some(HandView::Holster(t - start)),
            Phase::Back { start } => Some(HandView::Back(t - start, self.weapon.deploy, self.weapon.ready)),
        }
    }
}

/// The hands while the grenade is out (pov/ordnance.rs), each with its seconds: `Away` the weapon
/// going away from G (and its holster time), `Out` the grenade drawn (its draw over DEPLOY, then
/// held), `Prep` since the pin (PREP, then held), `Toss` since the throw began (overhead or not),
/// `Holster` put away unthrown (HOLSTER), `Back` the weapon coming back (and its deploy time and
/// draw's ready fraction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandView {
    Away(f32, f32),
    Out(f32),
    Prep(f32),
    Toss(f32, bool),
    Holster(f32),
    Back(f32, f32, f32),
}

/// The grenade's timings the view model plays its clips over (S3 `mp_weapon_frag_grenade.txt`:
/// `deploy_time`, `toss_pullout_time`, `toss_time`, `toss_overhead_time`; the holster its clip's
/// 14/30 s).
pub const VIEW_DEPLOY: f32 = DEPLOY;
pub const VIEW_PREP: f32 = PREP;
pub const VIEW_TOSS: f32 = TOSS;
pub const VIEW_TOSS_OVERHEAD: f32 = TOSS_OVERHEAD;
pub const VIEW_HOLSTER: f32 = HOLSTER;
pub const VIEW_RELEASE: f32 = TOSS_RELEASE;
pub const VIEW_RELEASE_OVERHEAD: f32 = TOSS_OVERHEAD_RELEASE;

// ---- the throw and the flight ----

/// Source's pitch (degrees, down positive) of a direction.
fn pitch_of(dir: Vec3) -> f32 {
    -dir.normalize_or_zero()
        .y
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

/// Where and how fast the grenade leaves the eye (world metres, m/s): 0x14100AA40 + 0x141015A50.
pub fn launch(eye: Vec3, forward: Vec3, player_velocity: Vec3) -> (Vec3, Vec3) {
    let f = forward.normalize_or(Vec3::Z);
    let flat = Vec3::new(f.x, 0.0, f.z);
    let pitch = pitch_of(f);
    let raised = pitch - ((pitch + 90.0) / 90.0).clamp(0.0, 1.0) * PITCH_OFFSET;
    let (s, c) = raised.to_radians().sin_cos();
    let heading = flat.normalize_or(Vec3::Z);
    let dir = heading * c - Vec3::Y * s;
    let mut velocity = dir * m(LAUNCH_SPEED);
    if let Some(h) = flat.try_normalize() {
        let along = Vec3::new(player_velocity.x, 0.0, player_velocity.z)
            .dot(h)
            .max(0.0);
        velocity += f * along * INHERIT_SCALE;
    }
    (eye, velocity)
}

/// A uniform number in [0, 1) (xorshift).
fn random(s: &mut u32) -> f32 {
    if *s == 0 {
        *s = 0x9e37_79b9;
    }
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// The velocity after hitting a surface of normal `n` (facing back at it) at `v` (推断 from the
/// setting names; S3's native rule not found): the part along the surface keeps between
/// `_shallow` (grazing) and `_sharp` (head on) of itself by how square the hit is, the part into
/// it comes back out times `_along_normal`; then a random push of `_randomness` and an upward one
/// of up to `_extra_vertical_randomness` of the speed (`r`: two numbers in [0, 1) for the push's
/// direction, one for the upward part; zero for the aim arc).
pub fn bounce(v: Vec3, n: Vec3, r: [f32; 3]) -> Vec3 {
    let n = n.normalize_or(Vec3::Y);
    let into = v.dot(n);
    let speed = v.length();
    if into >= 0.0 || speed < 1e-6 {
        return v;
    }
    let vn = n * into;
    let vt = v - vn;
    let square = (-into / speed).clamp(0.0, 1.0);
    let frac = BOUNCE_SHALLOW + (BOUNCE_SHARP - BOUNCE_SHALLOW) * square;
    let mut out = vt * frac - vn * BOUNCE_ALONG_NORMAL;
    let k = out.length();
    if r != [0.0; 3] && k > 0.0 {
        let (a, z) = (r[0] * std::f32::consts::TAU, r[1] * 2.0 - 1.0);
        let s = (1.0 - z * z).max(0.0).sqrt();
        out += Vec3::new(s * a.cos(), z, s * a.sin()) * (BOUNCE_RANDOMNESS * k)
            + Vec3::Y * (r[2] * BOUNCE_EXTRA_UP * k);
    }
    out
}

/// A thrown grenade (world metres, m/s; seconds since it left the hand).
#[derive(Clone, Debug)]
pub struct Live {
    pub at: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    /// on a surface (its normal), rolling or still
    pub resting: Option<Vec3>,
    pub bounces: u32,
    /// enemies it bonked (once each)
    bonked: Vec<u64>,
    rng: u32,
    /// the grenade indicator: since when it has been in range and in sight, when it last was
    threat_since: Option<f32>,
    threat_seen: Option<f32>,
    /// its orientation (world): tumbling in the air, rolling on the ground (the thrown grenade's
    /// model, pov groups 7/8; 推断: S3's physics turns it, not read)
    pub spin: glam::Quat,
}

impl Live {
    pub fn new(at: Vec3, velocity: Vec3, seed: u32) -> Live {
        Live {
            at,
            velocity,
            age: 0.0,
            resting: None,
            bounces: 0,
            bonked: Vec::new(),
            rng: seed | 1,
            threat_since: None,
            threat_seen: None,
            spin: glam::Quat::IDENTITY,
        }
    }

    /// Turns it by this step: a tumble about the axis across its flight (FLIGHT_SPIN), or rolling
    /// on a surface at its speed over its radius.
    fn turn(&mut self, dt: f32) {
        let v = self.velocity;
        if v.length() < 1e-3 {
            return;
        }
        let up = self.resting.unwrap_or(Vec3::Y);
        let axis = up.cross(v).normalize_or(Vec3::X);
        let rate = if self.resting.is_some() { (v.length() / ROLL_RADIUS).min(40.0) } else { FLIGHT_SPIN };
        self.spin = (glam::Quat::from_axis_angle(axis, rate * dt) * self.spin).normalize();
    }
}

/// The thrown grenade's turn in the air (rad/s) and its radius rolling (m: the projectile model,
/// 4.3 units across).
const FLIGHT_SPIN: f32 = 9.0;
const ROLL_RADIUS: f32 = 0.053;

/// A surface the flight met: where, its normal, how fast into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub at: Vec3,
    pub normal: Vec3,
    pub speed: f32,
}

/// One step of a thrown grenade by `dt`: gravity, then the segment through `cast` (the first
/// surface between two points: the point and its normal facing back). Rolling on a surface it
/// keeps to it (gravity along it, ROLL_FRAC_PER_SECOND), leaves it over an edge. `noise`: the
/// bounces' randomness (off for the aim arc). The surface met, if any.
pub fn fly(
    g: &mut Live,
    dt: f32,
    cast: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)>,
    noise: bool,
) -> Option<Hit> {
    let gravity = Vec3::NEG_Y * m(GRAVITY);
    g.age += dt;
    if let Some(n) = g.resting {
        let along = gravity - n * gravity.dot(n);
        let mut v = g.velocity + along * dt;
        v -= n * v.dot(n);
        v *= ROLL_FRAC_PER_SECOND.powf(dt);
        // still: lying on something flat enough that gravity can't roll it
        if v.length() < m(STOP_SPEED) && along.length() < m(GRAVITY) * 0.3 {
            g.velocity = Vec3::ZERO;
            return None;
        }
        let next = g.at + v * dt;
        if let Some((hit, wall)) = cast(g.at, next) {
            // rolled into something
            let out = bounce(v, wall, [0.0; 3]);
            g.at = hit + wall * m(SKIN);
            g.velocity = out - n * out.dot(n);
            return None;
        }
        // the ground under the new point
        let probe = m(SKIN + 4.0);
        match cast(next + n * m(1.0), next - n * probe) {
            Some((ground, gn)) if gn.y >= WALKABLE => {
                g.at = ground + gn * m(SKIN);
                g.velocity = v - gn * v.dot(gn);
                g.resting = Some(gn);
            }
            _ => {
                g.at = next;
                g.velocity = v;
                g.resting = None;
            }
        }
        return None;
    }
    let next = g.at + g.velocity * dt + gravity * (0.5 * dt * dt);
    let next_velocity = g.velocity + gravity * dt;
    let Some((hit, n)) = cast(g.at, next) else {
        g.at = next;
        g.velocity = next_velocity;
        return None;
    };
    let r = if noise {
        [random(&mut g.rng), random(&mut g.rng), random(&mut g.rng)]
    } else {
        [0.0; 3]
    };
    let speed = (-next_velocity.dot(n)).max(0.0);
    let out = bounce(next_velocity, n, r);
    g.at = hit + n * m(SKIN);
    g.bounces += 1;
    if n.y >= WALKABLE && out.dot(n) < m(REST_SPEED) {
        g.resting = Some(n);
        g.velocity = out - n * out.dot(n);
    } else {
        g.velocity = out;
    }
    Some(Hit {
        at: hit,
        normal: n,
        speed,
    })
}

/// The aim arc: the grenade's path from the hand as `fly` would take it, without the bounces'
/// randomness, for ARC_SECONDS (or the fuse) and ARC_BOUNCES bounces: points every `dt`.
pub fn arc(
    at: Vec3,
    velocity: Vec3,
    cast: &dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)>,
    dt: f32,
) -> (Vec<Vec3>, Option<Hit>) {
    let mut g = Live::new(at, velocity, 1);
    let mut points = vec![at];
    let mut end = None;
    let mut t = 0.0;
    while t < ARC_SECONDS.min(FUSE) {
        let hit = fly(&mut g, dt, cast, false);
        t += dt;
        points.push(g.at);
        if let Some(h) = hit {
            end = Some(h);
            if g.bounces > ARC_BOUNCES || g.resting.is_some() {
                break;
            }
        }
        if g.resting.is_some() {
            break;
        }
    }
    (points, end)
}

// ---- the explosion ----

/// The share of the damage at `d` units (RadiusDamage 0x140CDCCB0: full within the inner radius,
/// linearly down to none at the outer).
pub fn falloff(d: f32) -> f32 {
    if INNER_RADIUS >= OUTER_RADIUS {
        return if d < OUTER_RADIUS { 1.0 } else { 0.0 };
    }
    1.0 - ((d - INNER_RADIUS) / (OUTER_RADIUS - INNER_RADIUS)).clamp(0.0, 1.0)
}

/// Where RadiusDamage measures a target to: three quarters of the way from the middle of its box
/// (feet `base`, `height` tall, `radius` wide each way) to the box's point nearest the centre.
pub fn target_point(centre: Vec3, base: Vec3, height: f32, radius: f32) -> Vec3 {
    let nearest = Vec3::new(
        centre.x.clamp(base.x - radius, base.x + radius),
        centre.y.clamp(base.y, base.y + height),
        centre.z.clamp(base.z - radius, base.z + radius),
    );
    let middle = base + Vec3::Y * (height * 0.5);
    nearest * 0.75 + middle * 0.25
}

/// A segment against an upright cylinder (feet `base`): the first point, its normal (sideways off
/// the axis, or the caps').
pub fn segment_cylinder(
    a: Vec3,
    b: Vec3,
    base: Vec3,
    height: f32,
    radius: f32,
) -> Option<(f32, Vec3, Vec3)> {
    let d = b - a;
    let (ox, oz) = (a.x - base.x, a.z - base.z);
    let mut best: Option<(f32, Vec3)> = None;
    let mut take = |t: f32, n: Vec3| {
        if (0.0..=1.0).contains(&t) && best.is_none_or(|b| t < b.0) {
            best = Some((t, n));
        }
    };
    let qa = d.x * d.x + d.z * d.z;
    if qa > 1e-10 {
        let qb = 2.0 * (ox * d.x + oz * d.z);
        let qc = ox * ox + oz * oz - radius * radius;
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let t = (-qb - disc.sqrt()) / (2.0 * qa);
            let y = a.y + d.y * t - base.y;
            if (0.0..=height).contains(&y) {
                let p = a + d * t;
                take(
                    t,
                    Vec3::new(p.x - base.x, 0.0, p.z - base.z).normalize_or(Vec3::X),
                );
            }
        }
    }
    if d.y.abs() > 1e-8 {
        for (cap, n) in [(base.y, Vec3::NEG_Y), (base.y + height, Vec3::Y)] {
            let t = (cap - a.y) / d.y;
            let (x, z) = (ox + d.x * t, oz + d.z * t);
            if x * x + z * z <= radius * radius && d.dot(n) < 0.0 {
                take(t, n);
            }
        }
    }
    best.map(|(t, n)| (t, a + d * t, n))
}

// ---- the game ----

/// A burst for the HUD (world metres; seconds on the module's clock).
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub at: Vec3,
    pub when: f32,
}

struct State {
    hand: Hand,
    live: Vec<Live>,
    bursts: Vec<Burst>,
    /// the aim arc this frame (world metres) and where it ends
    arc: Option<(Vec<Vec3>, Option<Vec3>)>,
    /// last frame's keys (edges)
    last_g: bool,
    last_weapon: bool,
    last_ability: bool,
    seed: u32,
    /// dev: G pressed by the channel; the trigger held until; aiming until
    dev_g: bool,
    dev_fire: Option<(f32, f32)>,
    dev_aim: Option<f32>,
    /// dev `nade throw`: once the grenade is ready, the trigger for this long (and aiming); asked when
    dev_throw: Option<(f32, bool, f32)>,
    /// totals for the status line
    thrown: u32,
    exploded: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    hand: Hand {
        phase: Phase::Idle,
        queued: None,
        last_toss: f32::NEG_INFINITY,
        weapon: super::weapons::R301_TIMING,
    },
    live: Vec::new(),
    bursts: Vec::new(),
    arc: None,
    last_g: false,
    last_weapon: false,
    last_ability: false,
    seed: 0x2545_f491,
    dev_g: false,
    dev_fire: None,
    dev_aim: None,
    dev_throw: None,
    thrown: 0,
    exploded: 0,
});

/// The STATE lock is never held while calling kcc.rs (its step asks other modules for theirs) or
/// the HUD's readers.
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn sound(names: &[&'static str], delay: f32) {
    for &name in names {
        crate::audio::play_in(name, SOUND_VOLUME, delay);
    }
}

/// Like Octane's abilities: a living player in play with the movement controller.
fn can_use() -> bool {
    crate::state::in_world()
        && crate::fe::in_play_view()
        && !lethal::fuse_hp().is_some_and(|hp| hp <= 0.0)
        && kcc::feet().is_some()
}

/// Whether the grenade is part of the mod at all: the R-301 and the movement controller.
pub fn enabled() -> bool {
    super::gun::enabled() && kcc::enabled()
}

/// The R-301 can't fire (gun.rs): the grenade is out, or on its way, or the R-301 still coming back.
pub fn gun_away() -> bool {
    let s = state();
    s.hand.gun_away(now())
}

/// The R-301's place in the view model while the grenade is out (pov/ordnance.rs).
pub fn view() -> Option<HandView> {
    let s = state();
    s.hand.view(now())
}

/// For the HUD's ordnance slot: the grenade is in the hand (or coming).
pub fn holding() -> bool {
    state().hand.holding()
}

/// The grenades in the air or on the ground, for the HUD: where, seconds left on the fuse, whether
/// S3's grenade indicator shows for it.
pub fn live() -> Vec<(Vec3, f32, bool)> {
    let s = state();
    let t = now();
    s.live
        .iter()
        .map(|g| {
            (
                g.at,
                (FUSE - g.age).max(0.0),
                g.threat_seen.is_some_and(|seen| t - seen < THREAT_LINGER)
                    && g.threat_since.is_some_and(|since| t >= since),
            )
        })
        .collect()
}

/// The thrown grenades to draw as models (pov groups 7/8, firstperson.rs): where and how turned,
/// the newest first, at most two.
pub fn props() -> Vec<(Vec3, glam::Quat)> {
    let s = state();
    let mut v: Vec<(f32, Vec3, glam::Quat)> = s.live.iter().map(|g| (g.age, g.at, g.spin)).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(2).map(|(_, at, spin)| (at, spin)).collect()
}

/// Explosions of the last second (HUD flashes): where, seconds ago.
pub fn bursts() -> Vec<(Vec3, f32)> {
    let s = state();
    let t = now();
    s.bursts
        .iter()
        .filter(|b| t - b.when < 1.0)
        .map(|b| (b.at, t - b.when))
        .collect()
}

/// The aim arc while aiming with the grenade out (world metres) and its end.
pub fn aim_arc() -> Option<(Vec<Vec3>, Option<Vec3>)> {
    state().arc.clone()
}

/// The explosion's outer radius in metres (the HUD's flash).
pub fn outer_radius() -> f32 {
    m(OUTER_RADIUS)
}

/// ER re-bases Havok coordinates between outdoor tiles (kcc.rs): keep the grenades in that frame.
/// Called with KCC held; this must never call back into KCC.
pub fn world_shift(delta: Vec3) {
    let mut s = state();
    for g in s.live.iter_mut() {
        g.at += delta;
    }
    for b in s.bursts.iter_mut() {
        b.at += delta;
    }
    if let Some((points, end)) = s.arc.as_mut() {
        for p in points.iter_mut() {
            *p += delta;
        }
        if let Some(e) = end.as_mut() {
            *e += delta;
        }
    }
}

/// Puts the grenade away (another module's action that takes the hands); what happened.
pub fn cancel(by: &str) -> String {
    let mut ev = Vec::new();
    let t = now();
    let mut s = state();
    s.hand.step(
        t,
        HandInput {
            ability: true,
            ..Default::default()
        },
        &mut ev,
    );
    format!(
        "grenade ({by}): {}",
        if ev.contains(&HandEvent::Canceled) {
            "put away"
        } else {
            "not in hand"
        }
    )
}

/// The first surface between two world points: the movement controller's triangles, else the
/// game's map ray (outside the controller's window). With the point and its normal facing back.
fn cast(a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    if let Some(hit) = kcc::ray_cast(a, b) {
        return Some(hit);
    }
    map_hit(a, b)
}

/// The game's own map ray (the gun's wall check), its normal from two more rays beside it.
fn map_hit(a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = wcm.main_player.as_ref()?;
    let havok = unsafe { CSHavokMan::instance() }.ok()?;
    let ray = |from: Vec3, d: Vec3| {
        havok
            .phys_world
            .cast_ray(
                MAP_RAY,
                &HavokPosition(from.x, from.y, from.z, 0.0),
                PositionDelta(d.x, d.y, d.z),
                me,
            )
            .map(|h| Vec3::new(h.0, h.1, h.2))
    };
    let d = b - a;
    let len = d.length();
    if len < 1e-4 {
        return None;
    }
    let hit = ray(a, d)?;
    let dir = d / len;
    let side = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let (s1, s2) = (
        dir.cross(side).normalize(),
        dir.cross(dir.cross(side)).normalize(),
    );
    const BESIDE: f32 = 0.03;
    let normal = match (ray(a + s1 * BESIDE, d * 2.0), ray(a + s2 * BESIDE, d * 2.0)) {
        (Some(p1), Some(p2)) => {
            let n = (p1 - hit).cross(p2 - hit).normalize_or_zero();
            if n == Vec3::ZERO {
                -dir
            } else if n.dot(dir) > 0.0 {
                -n
            } else {
                n
            }
        }
        _ => -dir,
    };
    Some((hit, normal))
}

/// er-mario's ray filter for map geometry (gun.rs MAP_RAY).
const MAP_RAY: u32 = 0x08;

/// Whether the map stands between two points.
fn blocked(a: Vec3, b: Vec3) -> bool {
    cast(a, b).is_some_and(|(hit, _)| hit.distance(b) > 0.05)
}

/// Enemies (team 6/7, alive): handle, a key, feet, height and radius (body.rs `cylinder`).
fn enemies() -> Vec<(FieldInsHandle, u64, Vec3, f32, f32, i32)> {
    let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in wcm.chr_sets.iter().flatten().flat_map(|s| s.characters()) {
        let c: &ChrIns = c;
        if !crate::spike::body::is_enemy(c) || c.modules.data.hp <= 0 {
            continue;
        }
        let (h, r) = super::body::cylinder(c);
        let q = c.modules.physics.position;
        let key = unsafe { std::mem::transmute_copy::<FieldInsHandle, u64>(&c.field_ins_handle) };
        out.push((
            c.field_ins_handle,
            key,
            Vec3::new(q.0, q.1, q.2),
            h,
            r,
            c.npc_param_id,
        ));
    }
    out
}

/// The frag's damage on one enemy (S6 bridge) and for the HUD (damage numbers, kill credit, totals).
fn hurt_enemy(handle: FieldInsHandle, at: Vec3, damage: f32) -> String {
    let res = super::combat::shoot(handle, damage);
    super::stats::dealt(damage);
    super::gun::record_hit(super::gun::Hit {
        at: Instant::now(),
        pos: at,
        damage,
        head: false,
        target: handle,
        // the kill feed's icon: 2 the frag grenade (stats.rs, hud/apex.rs `kill_feed`)
        weapon: 2,
    });
    res
}

/// The thrower hit by his own grenade (`explosion_damages_owner`): Apex damage off his health.
fn hurt_self(damage: f32) -> String {
    // through the shield first, as any hit (D-032, lethal.rs `take`); at 0 health the game's own
    // death follows
    match lethal::take(damage) {
        None => "self: no Apex HP (lethal_guard off) or already down: none".into(),
        Some((shield, health, died)) => format!(
            "self {damage:.1}: shield -{shield:.1}, health -{health:.1}{}",
            if died { ", killed by his own grenade" } else { "" }
        ),
    }
}

/// The explosion at `centre` (world metres): every enemy and the thrower by RadiusDamage's rule.
fn explode(centre: Vec3) {
    sound(SND_EXPLODE, 0.0);
    // the explosion's look: a vanilla explosion's effect (fx.rs, 近似 for S3's particles)
    let fx = super::fx::explosion(centre);
    let mut lines = Vec::new();
    for (handle, _, base, h, r, npc) in enemies() {
        let target = target_point(u3(centre), u3(base), u(h), u(r));
        let share = falloff(u3(centre).distance(target));
        if share <= 0.0 {
            continue;
        }
        let target_m = target / kcc::UNITS_PER_METRE;
        let eye = base + Vec3::Y * (h * 0.9);
        if blocked(centre, target_m) && blocked(centre, eye) {
            lines.push(format!("npc {npc}: behind cover"));
            continue;
        }
        let damage = EXPLOSION_DAMAGE * share;
        lines.push(format!(
            "npc {npc} {:.1} m: {damage:.1} ({})",
            (target_m - centre).length(),
            hurt_enemy(handle, target_m, damage)
        ));
    }
    if let (Some(feet), Some(half)) = (kcc::feet(), kcc::hull_half_height()) {
        let target = target_point(u3(centre), u3(feet), 2.0 * half, player_radius());
        let share = falloff(u3(centre).distance(target));
        let target_m = target / kcc::UNITS_PER_METRE;
        let eye = eye_and_view().map_or(feet + Vec3::Y * m(player_eye()), |v| v.0);
        if share > 0.0 && !(blocked(centre, target_m) && blocked(centre, eye)) {
            lines.push(hurt_self(EXPLOSION_DAMAGE * share));
        }
    }
    log(format!(
        "grenade: exploded at {centre:.2}: {}; {fx}",
        if lines.is_empty() {
            "nobody in range".into()
        } else {
            lines.join("; ")
        }
    ));
}

fn u3(p: Vec3) -> Vec3 {
    p * kcc::UNITS_PER_METRE
}

/// Steps the thrown grenades by `dt`: flight, bonks, the fuse, S3's grenade indicator.
fn step_live(dt: f32) {
    let mut live = state().live.clone();
    if live.is_empty() {
        return;
    }
    let t = now();
    let enemies = enemies();
    let player = kcc::feet().zip(kcc::hull_half_height());
    let me_velocity = kcc::locomotion().map_or(Vec3::ZERO, |l| l.velocity);
    let mut done = Vec::new();
    for (i, g) in live.iter_mut().enumerate() {
        let from = g.at;
        g.turn(dt);
        if let Some(hit) = fly(g, dt, &cast, true)
            && u(hit.speed) > BOUNCE_SOUND_SPEED
        {
            sound(SND_BOUNCE, 0.0);
        }
        // a bonk: the first enemy on the way, once each, while it flies (推断: not one walking
        // over it on the ground)
        for (handle, key, base, h, r, npc) in &enemies {
            if g.bonked.contains(key) || u(g.velocity.length()) < REST_SPEED {
                continue;
            }
            if let Some((_, at, n)) = segment_cylinder(from, g.at, *base, *h, *r + m(SKIN)) {
                g.bonked.push(*key);
                g.at = at + n * m(SKIN);
                g.velocity = bounce(g.velocity, n, [0.0; 3]);
                g.resting = None;
                log(format!(
                    "grenade: bonked npc {npc}: {}",
                    hurt_enemy(*handle, at, TOUCH_DAMAGE)
                ));
                break;
            }
        }
        // S3's grenade indicator for its thrower
        if let Some((feet, half)) = player {
            let middle = feet + Vec3::Y * m(half);
            let near = u(g.at.distance(feet)) <= OUTER_RADIUS + THREAT_PADDING;
            if near && !blocked(g.at, middle) {
                if g.threat_since.is_none() {
                    let slow = u((g.velocity - me_velocity).length()) < THREAT_SLOW;
                    g.threat_since = Some(
                        t + if slow {
                            THREAT_DELAY_SLOW
                        } else {
                            THREAT_DELAY
                        },
                    );
                }
                g.threat_seen = Some(t);
            } else if g.threat_seen.is_none_or(|s| t - s >= THREAT_LINGER) {
                g.threat_since = None;
            }
        }
        if g.age >= FUSE {
            done.push(i);
        }
    }
    let mut bursts = Vec::new();
    for &i in done.iter().rev() {
        let g = live.remove(i);
        explode(g.at);
        bursts.push(Burst { at: g.at, when: t });
    }
    let mut s = state();
    s.exploded += bursts.len() as u32;
    s.live = live;
    s.bursts.retain(|b| t - b.when < 1.0);
    s.bursts.extend(bursts);
}

/// The keys and buttons this frame, read without the STATE lock: G, 1/2, Q/Z (Apex's PC defaults,
/// with the game window focused), the R-301's trigger as gun.rs reads it, the eye's pitch, whether an
/// ability's clips hold the hands.
struct Raw {
    g: bool,
    weapon: bool,
    ability: bool,
    fire: bool,
    pitch: f32,
    hands_free: bool,
}

fn read_raw() -> Raw {
    let key = |k: u8| crate::input::key_down(k).unwrap_or(false);
    let (q, z) = crate::input::ability_keys().unwrap_or((false, false));
    Raw {
        g: key(b'G'),
        weapon: key(b'1') || key(b'2'),
        ability: q || z,
        fire: super::gun::buttons().0,
        pitch: eye_and_view().map_or(0.0, |v| pitch_of(v.1)),
        hands_free: !super::pov::ability_playing(),
    }
}

/// This frame's input: the keys' edges, the dev channel's presses.
fn read_input(s: &mut State, t: f32, r: Raw) -> HandInput {
    // dev `nade throw`: the trigger once the grenade is ready (dropped after 5 s)
    if let Some((hold, aim, asked)) = s.dev_throw {
        if t - asked > 5.0 {
            s.dev_throw = None;
        } else if matches!(s.hand.phase, Phase::Ready { .. }) {
            s.dev_fire = Some((t, t + hold.max(0.02)));
            if aim {
                s.dev_aim = Some(t + hold + PREP + 0.5);
            }
            s.dev_throw = None;
        }
    }
    let input = HandInput {
        ordnance: (r.g && !s.last_g) || std::mem::take(&mut s.dev_g),
        fire: r.fire
            || s.dev_fire
                .is_some_and(|(from, until)| t >= from && t < until),
        weapon: r.weapon && !s.last_weapon,
        ability: r.ability && !s.last_ability,
        pitch: r.pitch,
        hands_free: r.hands_free,
    };
    (s.last_g, s.last_weapon, s.last_ability) = (r.g, r.weapon, r.ability);
    input
}

/// Once a frame (after octane.rs: its Q/Z start their clips first; before the movement controller's
/// step and the view model's).
pub fn update(dt: f32) {
    if !enabled() {
        return;
    }
    let dt = dt.clamp(0.0, 0.1);
    if !crate::state::in_world() {
        let mut s = state();
        s.hand = Hand::default();
        s.live.clear();
        s.bursts.clear();
        s.arc = None;
        return;
    }
    step_live(dt);
    let t = now();
    let usable = can_use();
    let stim_held = super::pov::left_busy();
    let raw = read_raw();
    // the weapon in the hands (its locks taken outside the grenade's)
    let weapon = super::weapons::timing(super::weapons::active());
    let mut ev = Vec::new();
    let (input, phase_before) = {
        let mut s = state();
        let input = read_input(&mut s, t, raw);
        if !usable {
            // dead, a menu, the controller off: the grenade goes, nothing is thrown
            if s.hand.phase != Phase::Idle {
                s.hand = Hand::default();
                log("grenade: hand cleared (not in play)");
            }
            s.arc = None;
            return;
        }
        let before = s.hand.phase;
        if before == Phase::Idle {
            s.hand.weapon = weapon;
        }
        s.hand.step(t, input, &mut ev);
        (input, before)
    };
    if input.ordnance {
        // the shield battery's use ends (D-032; S3 cl_survival_loot.nut: weaponSelectOrdnance ->
        // AttemptCancelHeal)
        super::battery::cancel("key G");
        log(format!("grenade: G ({:?})", phase_before));
    }
    for e in &ev {
        match e {
            HandEvent::Drawn => {
                let away = state().hand.weapon.holster;
                log(format!(
                    "grenade: {} away {away} s, then the grenade drawn ({DEPLOY} s, throwable at {:.2} s)",
                    super::weapons::active().name(),
                    away + DEPLOY * DRAW_READY
                ));
            }
            HandEvent::GrenadeOut => sound(SND_DRAW, 0.0),
            HandEvent::Waiting if input.ordnance && stim_held => {
                // the left hand holds the injector: thrown now, as a reload does (gun.rs)
                if super::octane::throw_injector() {
                    log("grenade: G while the injector is held: thrown first");
                }
            }
            HandEvent::Ready => log("grenade: ready"),
            HandEvent::PinPulled => {
                sound(SND_PIN, 0.0);
                log("grenade: pin pulled");
            }
            HandEvent::Launch { overhead } => release(*overhead),
            HandEvent::Canceled => {
                if matches!(phase_before, Phase::Ready { .. } | Phase::Prep { .. })
                    && !input.ability
                {
                    sound(SND_HOLSTER, 0.0);
                }
                log(format!(
                    "grenade: put away ({})",
                    if input.ability {
                        "an ability"
                    } else {
                        "a weapon key"
                    }
                ));
            }
            HandEvent::Back => {
                log(format!("grenade: the {} coming back", super::weapons::active().name()));
                // the Charge Rifle, if it is the weapon in hand, comes out again (U3)
                super::weapons::redraw_after_offhand("the grenade");
            }
            HandEvent::Waiting => {}
        }
    }
    update_arc();
}

/// The grenade leaves the hand (AE_WPN_TOSS_RELEASE): from the eye along its view.
fn release(overhead: bool) {
    let Some((eye, fwd)) = eye_and_view() else {
        log("grenade: no eye to throw from");
        return;
    };
    let carried = kcc::locomotion().map_or(Vec3::ZERO, |l| l.velocity);
    let (at, velocity) = launch(eye, fwd, carried);
    sound(SND_THROW, 0.0);
    sound(SND_VOICE, 0.0);
    let mut s = state();
    s.seed = s.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let seed = s.seed;
    s.live.push(Live::new(at, velocity, seed));
    s.thrown += 1;
    let n = s.live.len();
    drop(s);
    log(format!(
        "grenade: thrown {} at {:.0} units/s (pitch {:.1}°, carried {:.1} m/s), fuse {FUSE} s; {n} live",
        if overhead { "overhand" } else { "underhand" },
        u(velocity.length()),
        pitch_of(fwd),
        carried.length()
    ));
}

/// The aim arc (grenade_arc_indicator_show_from_hip 0: only while aiming) with the grenade out.
fn update_arc() {
    let t = now();
    let (aim_dev, showing) = {
        let s = state();
        (
            s.dev_aim.is_some_and(|until| t < until),
            matches!(s.hand.phase, Phase::Ready { .. } | Phase::Prep { .. }),
        )
    };
    let (_, aim) = super::gun::buttons();
    if !showing || !(aim || aim_dev) {
        state().arc = None;
        return;
    }
    let Some((eye, fwd)) = eye_and_view() else {
        return;
    };
    let carried = kcc::locomotion().map_or(Vec3::ZERO, |l| l.velocity);
    let (at, velocity) = launch(eye, fwd, carried);
    let (points, end) = arc(at, velocity, &cast, 1.0 / 30.0);
    state().arc = Some((points, end.map(|h| h.at)));
}

/// Dev channel `nade ...`.
pub fn dev(args: &[&str]) -> String {
    let t = now();
    match args {
        [] => status(),
        ["g"] => {
            state().dev_g = true;
            "grenade: G pressed (dev)".into()
        }
        ["throw", rest @ ..] => {
            // G (unless it is out), then the trigger once it is ready for `hold` seconds; `aim`: the
            // arc meanwhile
            let hold = rest.first().and_then(|a| a.parse::<f32>().ok()).unwrap_or(0.1).max(0.0);
            let aim = rest.contains(&"aim");
            let mut s = state();
            if !matches!(s.hand.phase, Phase::Ready { .. } | Phase::Draw { .. }) {
                s.dev_g = true;
            }
            s.dev_throw = Some((hold, aim, t));
            format!("grenade (dev): G, the trigger once ready for {hold} s{}", if aim { ", aiming" } else { "" })
        }
        ["aim", secs] => {
            let s = secs.parse::<f32>().unwrap_or(3.0);
            state().dev_aim = Some(t + s);
            format!("grenade (dev): aiming for {s} s")
        }
        ["cancel"] => cancel("dev"),
        ["boom"] => {
            let mut s = state();
            for g in s.live.iter_mut() {
                g.age = FUSE;
            }
            format!("grenade (dev): {} set to explode", s.live.len())
        }
        ["drop", rest @ ..] => {
            // a live grenade at the feet (+dx, +dz metres), its fuse running: the self-damage test
            let f = |i: usize| rest.get(i).and_then(|a| a.parse::<f32>().ok()).unwrap_or(0.0);
            let Some(feet) = kcc::feet() else { return "grenade (dev): no movement controller".into() };
            let at = feet + Vec3::new(f(0), m(SKIN) + 0.05, f(1));
            let mut s = state();
            s.seed = s.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let seed = s.seed;
            s.live.push(Live::new(at, Vec3::ZERO, seed));
            format!("grenade (dev): dropped at {at:.2}, {FUSE} s")
        }
        _ => "usage: nade | nade g | nade throw [hold s] [aim] | nade aim <s> | nade cancel | nade boom | nade drop [dx dz]".into(),
    }
}

/// Dev channel `nade`: the hand, the live grenades, the totals.
pub fn status() -> String {
    let s = state();
    let t = now();
    let live: Vec<String> = s
        .live
        .iter()
        .map(|g| {
            format!(
                "{:.2} ({:.1} m/s, {:.2} s left{}{})",
                g.at,
                g.velocity.length(),
                FUSE - g.age,
                if g.resting.is_some() {
                    ", on the ground"
                } else {
                    ""
                },
                if g.bounces > 0 {
                    format!(", {} bounces", g.bounces)
                } else {
                    String::new()
                }
            )
        })
        .collect();
    format!(
        "grenade {}: hand {:?}{}, gun away {}, {} live [{}], thrown {}, exploded {}, arc {}",
        if enabled() {
            "on"
        } else {
            "off (gun = 1 and kcc = 1)"
        },
        s.hand.phase,
        if s.hand.queued.is_some() {
            " (G queued)"
        } else {
            ""
        },
        s.hand.gun_away(t),
        s.live.len(),
        live.join(", "),
        s.thrown,
        s.exploded,
        s.arc.as_ref().map_or("off".into(), |(p, e)| format!(
            "{} points, end {}",
            p.len(),
            e.map_or("-".into(), |e| format!("{e:.2}"))
        ))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const R301_HOLSTER: f32 = crate::spike::weapons::R301_TIMING.holster;
    const R301_DEPLOY: f32 = crate::spike::weapons::R301_TIMING.deploy;
    const R301_READY: f32 = crate::spike::weapons::R301_TIMING.ready;

    /// A flat floor at y = 0 (normal up) and, optionally, a wall at x = `wall` (normal -X).
    fn world(wall: Option<f32>) -> impl Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)> {
        move |a: Vec3, b: Vec3| {
            let mut best: Option<(f32, Vec3, Vec3)> = None;
            if a.y >= 0.0 && b.y < 0.0 {
                let t = a.y / (a.y - b.y);
                best = Some((t, a + (b - a) * t, Vec3::Y));
            }
            if let Some(x) = wall
                && a.x <= x
                && b.x > x
            {
                let t = (x - a.x) / (b.x - a.x);
                if best.is_none_or(|b| t < b.0) {
                    best = Some((t, a + (b - a) * t, Vec3::NEG_X));
                }
            }
            best.map(|(_, p, n)| (p, n))
        }
    }

    fn hand_run(steps: &[(f32, HandInput)], until: f32) -> (Hand, Vec<(f32, HandEvent)>) {
        let mut h = Hand::default();
        let mut out = Vec::new();
        let mut t = 0.0;
        while t <= until {
            let mut i = steps
                .iter()
                .rev()
                .find(|(at, _)| *at <= t)
                .map(|s| s.1)
                .unwrap_or_default();
            // edges once
            if let Some((at, s)) = steps.iter().find(|(at, _)| (*at - t).abs() < DT * 0.5) {
                let _ = at;
                i.ordnance = s.ordnance;
                i.weapon = s.weapon;
                i.ability = s.ability;
            } else {
                i.ordnance = false;
                i.weapon = false;
                i.ability = false;
            }
            let mut ev = Vec::new();
            h.step(t, i, &mut ev);
            out.extend(ev.into_iter().map(|e| (t, e)));
            t += DT;
        }
        (h, out)
    }

    fn free() -> HandInput {
        HandInput {
            hands_free: true,
            ..Default::default()
        }
    }

    /// G, then a quick click once ready: the R-301 away 0.55 s, the grenade throwable 0.3375 s into
    /// its 0.6 s draw, the pin, 0.25 s of prep, the underhand toss released 7/17 of 0.4 s in, the
    /// R-301 back 0.4 s after the toss started and firing 12/25 of 0.6 s later.
    #[test]
    fn hand_g_click_throw() {
        let ready = R301_HOLSTER + DEPLOY * DRAW_READY;
        let click = ready + 0.1;
        let steps = [
            (
                0.0,
                HandInput {
                    ordnance: true,
                    ..free()
                },
            ),
            (DT, free()),
            (
                click,
                HandInput {
                    fire: true,
                    ..free()
                },
            ),
            (click + 0.05, free()),
        ];
        let (h, ev) = hand_run(&steps, 3.0);
        let at = |e: HandEvent| {
            ev.iter()
                .find(|x| x.1 == e)
                .map(|x| x.0)
                .unwrap_or(f32::NAN)
        };
        assert!(at(HandEvent::Drawn) < DT);
        assert!((at(HandEvent::Ready) - ready).abs() < 2.0 * DT, "{ev:?}");
        let pin = at(HandEvent::PinPulled);
        assert!((pin - click).abs() < 2.0 * DT);
        // (each step lands on the first frame at or after its time: up to a frame late each)
        let toss = pin + PREP;
        let launch = ev
            .iter()
            .find(|x| matches!(x.1, HandEvent::Launch { overhead: false }))
            .map(|x| x.0)
            .unwrap();
        assert!(
            launch >= toss + TOSS * TOSS_RELEASE - 1e-4
                && launch < toss + TOSS * TOSS_RELEASE + 2.5 * DT,
            "{launch} {toss}"
        );
        let back = at(HandEvent::Back);
        assert!(
            back >= toss + TOSS - 1e-4 && back < toss + TOSS + 2.5 * DT,
            "{ev:?} toss {toss}"
        );
        assert_eq!(h.phase, Phase::Idle);
        // the R-301 fires again only from its ready frame
        let mut g = Hand {
            phase: Phase::Back { start: 10.0 },
            ..Hand::default()
        };
        assert!(g.gun_away(10.0 + R301_DEPLOY * R301_READY - 0.01));
        assert!(!g.gun_away(10.0 + R301_DEPLOY * R301_READY + 0.01));
        g.phase = Phase::Idle;
        assert!(!g.gun_away(11.0));
    }

    /// Held: the pin at once, then held as long as the trigger is; looking up throws overhand
    /// (released 6/23 of 0.7 s in).
    #[test]
    fn hand_hold_and_overhead() {
        let ready = R301_HOLSTER + DEPLOY * DRAW_READY;
        let up = HandInput {
            fire: true,
            pitch: -30.0,
            ..free()
        };
        let steps = [
            (
                0.0,
                HandInput {
                    ordnance: true,
                    fire: true,
                    pitch: -30.0,
                    ..free()
                },
            ),
            (DT, up),
            (
                ready + 2.0,
                HandInput {
                    pitch: -30.0,
                    ..free()
                },
            ),
        ];
        let (_, ev) = hand_run(&steps, 5.0);
        let pin = ev.iter().find(|x| x.1 == HandEvent::PinPulled).unwrap().0;
        assert!(
            (pin - ready).abs() < 2.0 * DT,
            "the trigger held through the draw pulls the pin at the ready frame: {pin}"
        );
        let launch = ev
            .iter()
            .find(|x| matches!(x.1, HandEvent::Launch { .. }))
            .unwrap();
        assert_eq!(launch.1, HandEvent::Launch { overhead: true });
        assert!(
            (launch.0 - (ready + 2.0 + TOSS_OVERHEAD * TOSS_OVERHEAD_RELEASE)).abs() < 2.0 * DT,
            "{launch:?}"
        );
    }

    /// 1/2 before the throw put it away (no launch); an ability (Q/Z) clears the hand at once; G
    /// while it is out does nothing; G again during the R-301's return draws again.
    #[test]
    fn hand_cancel_and_regrab() {
        let ready = R301_HOLSTER + DEPLOY * DRAW_READY;
        let steps = [
            (
                0.0,
                HandInput {
                    ordnance: true,
                    ..free()
                },
            ),
            (DT, free()),
            (
                ready + 0.2,
                HandInput {
                    weapon: true,
                    ..free()
                },
            ),
            (ready + 0.2 + DT, free()),
        ];
        let (h, ev) = hand_run(&steps, 3.0);
        assert!(ev.iter().any(|x| x.1 == HandEvent::Canceled));
        assert!(!ev.iter().any(|x| matches!(x.1, HandEvent::Launch { .. })));
        assert_eq!(h.phase, Phase::Idle);
        let mut h = Hand::default();
        let mut ev = Vec::new();
        h.step(
            0.0,
            HandInput {
                ordnance: true,
                ..free()
            },
            &mut ev,
        );
        h.step(
            0.3,
            HandInput {
                ordnance: true,
                ..free()
            },
            &mut ev,
        );
        assert_eq!(
            ev.iter().filter(|e| **e == HandEvent::Drawn).count(),
            1,
            "G again while out: nothing"
        );
        h.step(
            0.4,
            HandInput {
                ability: true,
                ..free()
            },
            &mut ev,
        );
        assert_eq!(h.phase, Phase::Idle);
        assert!(ev.contains(&HandEvent::Canceled));
        // G while an ability holds the hands waits for them
        let mut h = Hand::default();
        let mut ev = Vec::new();
        h.step(
            0.0,
            HandInput {
                ordnance: true,
                ..Default::default()
            },
            &mut ev,
        );
        assert_eq!(h.phase, Phase::Idle);
        h.step(0.5, Default::default(), &mut ev);
        h.step(0.6, free(), &mut ev);
        assert!(matches!(h.phase, Phase::Draw { start, .. } if start == 0.6));
        // the fire rate after a throw
        let mut h = Hand {
            phase: Phase::Ready { since: 0.0 },
            queued: None,
            last_toss: 0.0,
            ..Hand::default()
        };
        let mut ev = Vec::new();
        h.step(
            0.5,
            HandInput {
                fire: true,
                ..free()
            },
            &mut ev,
        );
        assert!(matches!(h.phase, Phase::Ready { .. }) && ev.contains(&HandEvent::Waiting));
        h.step(
            1.0 / FIRE_RATE + 0.01,
            HandInput {
                fire: true,
                ..free()
            },
            &mut ev,
        );
        assert!(matches!(h.phase, Phase::Prep { .. }));
    }

    /// S3's throw: level, the 9° raise in full; looking straight up, none; straight down, full; the
    /// player's speed along the view only, never backwards; the launch speed 1300 units/s.
    #[test]
    fn launch_pitch_offset_and_inherit() {
        let (_, v) = launch(Vec3::ZERO, Vec3::Z, Vec3::ZERO);
        assert!((u(v.length()) - 1300.0).abs() < 0.01);
        assert!(
            (pitch_of(v) + 9.0).abs() < 1e-3,
            "level: 9° up, {}",
            pitch_of(v)
        );
        let up45 = Vec3::new(0.0, 45f32.to_radians().sin(), 45f32.to_radians().cos());
        let (_, v) = launch(Vec3::ZERO, up45, Vec3::ZERO);
        assert!(
            (pitch_of(v) - (-45.0 - 4.5)).abs() < 1e-3,
            "45° up: half the raise, {}",
            pitch_of(v)
        );
        let (_, v) = launch(Vec3::ZERO, Vec3::new(0.0, 0.999_999, 0.001), Vec3::ZERO);
        assert!(pitch_of(v) < -89.0);
        let (_, v) = launch(Vec3::ZERO, -Vec3::Y + Vec3::Z * 1e-3, Vec3::ZERO);
        assert!(
            (pitch_of(v) - (90.0 - 9.0)).abs() < 0.1,
            "straight down: 81°, {}",
            pitch_of(v)
        );
        // running forward at 7 m/s (sideways 3 m/s ignored): +7 m/s along the view
        let (_, v0) = launch(Vec3::ZERO, Vec3::Z, Vec3::ZERO);
        let (_, v1) = launch(Vec3::ZERO, Vec3::Z, Vec3::new(3.0, -2.0, 7.0));
        assert!((v1 - v0 - Vec3::Z * 7.0).length() < 1e-4, "{}", v1 - v0);
        // backwards: nothing
        let (_, v2) = launch(Vec3::ZERO, Vec3::Z, Vec3::new(0.0, 0.0, -7.0));
        assert!((v2 - v0).length() < 1e-5);
    }

    /// Head on the part into the surface comes back at 0.3; grazing the part along keeps 0.6 of
    /// itself; square in between; away from it, nothing.
    #[test]
    fn bounce_fractions() {
        let v = bounce(Vec3::new(0.0, -10.0, 0.0), Vec3::Y, [0.0; 3]);
        assert!((v - Vec3::Y * 3.0).length() < 1e-5, "{v}");
        let v = bounce(Vec3::new(10.0, -1e-4, 0.0), Vec3::Y, [0.0; 3]);
        assert!((v.x - 6.0).abs() < 1e-3, "{v}");
        let v = bounce(Vec3::new(10.0, -10.0, 0.0), Vec3::Y, [0.0; 3]);
        let square = std::f32::consts::FRAC_1_SQRT_2;
        let frac = 0.6 + (0.4 - 0.6) * square;
        assert!(
            (v - Vec3::new(10.0 * frac, 3.0, 0.0)).length() < 1e-4,
            "{v}"
        );
        assert_eq!(bounce(Vec3::Y, Vec3::Y, [0.0; 3]), Vec3::Y);
        // the noise stays within its share
        let v = bounce(Vec3::new(0.0, -10.0, 0.0), Vec3::Y, [0.3, 0.9, 1.0]);
        assert!((v - Vec3::Y * 3.0).length() <= 3.0 * (BOUNCE_RANDOMNESS + BOUNCE_EXTRA_UP) + 1e-4);
    }

    /// Dropped from 2 m onto a floor: bounces lower each time, comes to rest on it, never below it;
    /// rolling it slows by 0.05 a second and stops.
    #[test]
    fn falls_bounces_and_rests() {
        let w = world(None);
        let mut g = Live::new(Vec3::new(0.0, 2.0, 0.0), Vec3::ZERO, 7);
        let mut hits = Vec::new();
        for _ in 0..(4.0 / DT) as usize {
            if let Some(h) = fly(&mut g, DT, &w, true) {
                hits.push(h.speed);
            }
            assert!(g.at.y > 0.0, "{g:?}");
        }
        assert!(!hits.is_empty() && g.resting.is_some(), "{hits:?} {g:?}");
        // the fuse's clock: the age runs on through the flight and the rest (step_live explodes it at
        // FUSE, from the launch: start_fuse_on_launch)
        assert!((g.age - 4.0).abs() < 2.0 * DT && g.age > FUSE, "{}", g.age);
        assert!(hits.windows(2).all(|p| p[1] < p[0]), "{hits:?}");
        assert!((g.at.y - m(SKIN)).abs() < 1e-3);
        let mut r = Live::new(Vec3::new(0.0, m(SKIN), 0.0), Vec3::new(5.0, 0.0, 0.0), 1);
        r.resting = Some(Vec3::Y);
        fly(&mut r, 1.0, &w, false);
        assert!(
            (r.velocity.x - 5.0 * ROLL_FRAC_PER_SECOND).abs() < 1e-3,
            "{r:?}"
        );
        for _ in 0..120 {
            fly(&mut r, DT, &w, false);
        }
        assert_eq!(r.velocity, Vec3::ZERO);
    }

    /// Thrown at a wall it comes back off it and lands on the near side.
    #[test]
    fn bounces_off_a_wall() {
        let w = world(Some(5.0));
        let (_, v) = launch(Vec3::new(0.0, 1.5, 0.0), Vec3::X, Vec3::ZERO);
        let mut g = Live::new(Vec3::new(0.0, 1.5, 0.0), v, 3);
        let mut walls = 0;
        for _ in 0..(3.0 / DT) as usize {
            if fly(&mut g, DT, &w, true).is_some_and(|h| h.normal == Vec3::NEG_X) {
                walls += 1;
            }
        }
        assert!(walls >= 1 && g.at.x < 5.0 && g.at.y > 0.0, "{walls} {g:?}");
    }

    /// The aim arc ends at its first or second impact, without noise; thrown level from 1.5 m it
    /// lands ahead.
    #[test]
    fn aim_arc_reaches_the_floor() {
        let w = world(None);
        let (at, v) = launch(Vec3::new(0.0, 1.5, 0.0), Vec3::Z, Vec3::ZERO);
        let (points, end) = arc(at, v, &w, 1.0 / 30.0);
        let end = end.expect("hits the floor");
        assert!(end.at.y.abs() < 1e-4 && end.at.z > 5.0, "{end:?}");
        assert!(points.len() > 5 && points.iter().all(|p| p.y >= 0.0));
        // the same throw twice: the same arc (no randomness)
        let (again, _) = arc(at, v, &w, 1.0 / 30.0);
        assert_eq!(points, again);
    }

    /// G pressed while the throw ends draws again after it (via the R-301's return); Q/Z drop a
    /// queued G.
    #[test]
    fn hand_queue_during_toss() {
        let mut h = Hand {
            phase: Phase::Toss {
                start: 0.0,
                overhead: false,
                launched: true,
            },
            queued: None,
            last_toss: 0.0,
            ..Hand::default()
        };
        let mut ev = Vec::new();
        h.step(
            0.3,
            HandInput {
                ordnance: true,
                ..free()
            },
            &mut ev,
        );
        assert!(h.queued.is_some() && matches!(h.phase, Phase::Toss { .. }));
        h.step(TOSS + 0.01, free(), &mut ev);
        assert!(
            matches!(h.phase, Phase::Back { .. }) || matches!(h.phase, Phase::Draw { .. }),
            "{:?}",
            h.phase
        );
        h.step(TOSS + 0.03, free(), &mut ev);
        assert!(matches!(h.phase, Phase::Draw { .. }), "{:?}", h.phase);
        // a queued G (an ability's clips playing) dropped by Q/Z
        let mut h = Hand::default();
        h.step(
            0.0,
            HandInput {
                ordnance: true,
                ..Default::default()
            },
            &mut ev,
        );
        assert!(h.queued.is_some());
        h.step(
            0.1,
            HandInput {
                ability: true,
                ..Default::default()
            },
            &mut ev,
        );
        assert!(h.queued.is_none() && h.phase == Phase::Idle);
    }

    /// Thrown level from the eye (60 units up) it comes down where S3's numbers say: 9° up at 1300
    /// units/s under 750 units/s², about 969 units (23.8 m) ahead after 0.75 s.
    #[test]
    fn level_throw_range() {
        let w = world(None);
        let eye = Vec3::new(0.0, m(60.0), 0.0);
        let (at, v) = launch(eye, Vec3::Z, Vec3::ZERO);
        let mut g = Live::new(at, v, 1);
        let mut t = 0.0;
        let hit = loop {
            t += 1.0 / 240.0;
            if let Some(h) = fly(&mut g, 1.0 / 240.0, &w, false) {
                break h;
            }
            assert!(t < 3.0);
        };
        let (s, c) = 9f32.to_radians().sin_cos();
        let (vy, vz) = (1300.0 * s, 1300.0 * c);
        let flight = (vy + (vy * vy + 2.0 * 750.0 * 60.0).sqrt()) / 750.0;
        assert!((t - flight).abs() < 0.01, "{t} {flight}");
        assert!(
            (u(hit.at.z) - vz * flight).abs() < 0.01 * vz * flight,
            "{} {}",
            u(hit.at.z),
            vz * flight
        );
    }

    /// RadiusDamage: 100 within 125 units, 0 at 350, linear between; measured to 3/4 of the way to
    /// the box's nearest point.
    #[test]
    fn explosion_falloff_and_target_point() {
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(125.0), 1.0);
        assert!((falloff(237.5) - 0.5).abs() < 1e-6);
        assert_eq!(falloff(350.0), 0.0);
        assert_eq!(falloff(500.0), 0.0);
        // a 72-unit box at the origin, the centre 200 units away level with its middle
        let p = target_point(Vec3::new(200.0, 36.0, 0.0), Vec3::ZERO, 72.0, 16.0);
        assert!((p - Vec3::new(12.0, 36.0, 0.0)).length() < 1e-4, "{p}");
        // the centre inside the box: the point is 3/4 of the way to the centre itself
        let p = target_point(Vec3::new(4.0, 10.0, 0.0), Vec3::ZERO, 72.0, 16.0);
        assert!((p - Vec3::new(3.0, 16.5, 0.0)).length() < 1e-4, "{p}");
    }

    /// The bonk: a segment into a cylinder's side, from above onto its top, and past it.
    #[test]
    fn segment_and_cylinder() {
        let base = Vec3::new(0.0, 0.0, 5.0);
        let (t, p, n) = segment_cylinder(
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 10.0),
            base,
            2.0,
            0.5,
        )
        .unwrap();
        assert!(
            (t - 0.45).abs() < 1e-4
                && (p.z - 4.5).abs() < 1e-4
                && (n - Vec3::NEG_Z).length() < 1e-4
        );
        let (_, p, n) = segment_cylinder(
            Vec3::new(0.0, 4.0, 5.0),
            Vec3::new(0.0, 1.0, 5.0),
            base,
            2.0,
            0.5,
        )
        .unwrap();
        assert!((p.y - 2.0).abs() < 1e-4 && n == Vec3::Y);
        assert!(
            segment_cylinder(
                Vec3::new(2.0, 1.0, 0.0),
                Vec3::new(2.0, 1.0, 10.0),
                base,
                2.0,
                0.5
            )
            .is_none()
        );
        assert!(
            segment_cylinder(
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 1.0, 4.0),
                base,
                2.0,
                0.5
            )
            .is_none()
        );
    }
}
