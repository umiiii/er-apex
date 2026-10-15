//! First-person camera effects (plan-gun-motion-hud.md 2.4, gun-motion spec §4): what Apex adds to
//! the eye's view for the render camera.
//!
//! - The view punch (§4.2): a spring per axis (`punch_pilot`: stiffness 65, damping 9) kicked in
//!   angular velocity when he jumps and lands (landing by the fall speed, a fifth crouched).
//! - The slide's roll (§4.3): a horizontal vector that eases (5/s up, 2.5/s down) towards the
//!   velocity turned a quarter left, scaled by speed between `slideStopSpeed` 125 and 400 units/s
//!   while sliding; the roll is 15° times its part along the view.
//! - The sprint tilt (§4.3): a fraction that eases, with limited acceleration, towards the speed's
//!   part between walk and sprint speed times the turn rate's part of 120°/s; the roll adds 2° times
//!   it (into the turn).
//! - The slide's field of view (§4.4): x1.1 while sliding with `slideLongJumpAllowed`, linear over
//!   0.25 s in and out.
//! - The view model's camera bone (§2.4): its turn relative to `jx_c_pov` in the pose being drawn.
//!
//! The sprint view offset (§4.1) is not here: it is part of the eye, so the view model goes down
//! with it (kcc.rs adds it to the view height). Stepped once a frame (`step`, firstperson.rs);
//! camera.rs turns the eye's view by `turn` into the render camera and scales the field of view by
//! `fov_scale`. Fuse moves by the eye's view and the view model is placed in it; the gun's ray and
//! the HUD use the render camera, so a punch moves the shots with the crosshair (D-023).
//!
//! Angles are Source's view angles: pitch down, yaw left and roll with the left side up positive
//! (Source's AngleVectors: roll turns the right axis down). tools/r5/vmparse.py's roll, which the
//! spec's measurements use, is the opposite.

use std::sync::Mutex;

use glam::Vec3;

/// `punch_pilot` (apex-data/export/weapon/springs.txt): stiffness and damping, every axis.
const PUNCH_K: f32 = 65.0;
const PUNCH_C: f32 = 9.0;
/// Jump: angular velocity += JUMP_KICK x (2 + U, U, 0.9 + 0.5 U) degrees/s, U in [-1, 1].
const JUMP_KICK: f32 = 20.0;
/// Landing (Apex units): gravity 750; viewkickFallDistMin / Max 10 / 70 feet; MaxScale 12;
/// crouched x0.2.
const GRAVITY: f32 = 750.0;
const FALL_MIN_FT: f32 = 10.0;
const FALL_MAX_FT: f32 = 70.0;
const FALL_MAX_SCALE: f32 = 12.0;
const LAND_CROUCHED: f32 = 0.2;
/// The slide's roll: slide_viewTiltSide 15°, slideStopSpeed 125, slide_viewTiltPlayerSpeed 400,
/// slide_viewTiltIncrease / DecreaseSpeed 5 / 2.5 per second.
const TILT_SIDE: f32 = 15.0;
const TILT_STOP: f32 = 125.0;
const TILT_FULL: f32 = 400.0;
const TILT_UP: f32 = 5.0;
const TILT_DOWN: f32 = 2.5;
/// The slide's field of view: slideFOVScale 1.1 over slideFOVLerpIn / OutTime 0.25 s.
const SLIDE_FOV: f32 = 1.1;
const SLIDE_FOV_TIME: f32 = 0.25;
/// The sprint tilt: Fuse's `sprinttiltMaxRoll` 2°; the S3 client's convars `sprinttilt_accel` 35/s²,
/// `sprinttilt_maxvel` 2/s, `sprinttilt_turnrange` 120°/s (defaults, R5R's cfg leaves them).
const SPRINT_TILT_ROLL: f32 = 2.0;
const SPRINT_TILT_ACCEL: f32 = 35.0;
const SPRINT_TILT_MAXVEL: f32 = 2.0;
const SPRINT_TILT_TURN: f32 = 120.0;
/// Fuse's poseSettings speed and sprintspeed (units/s): standing 173.5 and 260, crouched 80 and 0
/// (crouched the speed's part is never above 0: no tilt).
const WALK_SPEED: f32 = 173.5;
const SPRINT_SPEED: f32 = 260.0;
const CROUCH_SPEED: f32 = 80.0;
const CROUCH_SPRINT_SPEED: f32 = 0.0;

/// The sprint tilt's state: the fraction (Apex's player +0x1EB8, -1..1, turning right positive), its
/// rate (`m_sprintTiltVel`) and the flat view axes it last measured the turn from
/// (`m_prevMoveYaw`).
#[derive(Clone, Copy, Debug, Default)]
pub struct SprintTilt {
    pub x: f32,
    v: f32,
    last: Option<(Vec3, Vec3)>,
}

impl SprintTilt {
    /// The target (0x140826A60, each movement step): 0 off the ground, and 0 below 1.05 times the
    /// walk speed (the speed is the full velocity's), where the last view is not updated either;
    /// else the speed's part between walk and sprint speed times the turn rate since the last
    /// update over `sprinttilt_turnrange`, both clamped. Apex also gives 0 while the player's
    /// `m_upDir` is not the world's up (never in ER). The first update has no last view: no turn.
    fn target(&mut self, dt: f32, i: &Inputs) -> f32 {
        if !i.grounded {
            return 0.0;
        }
        let (walk, sprint) = if i.crouched { (CROUCH_SPEED, CROUCH_SPRINT_SPEED) } else { (WALK_SPEED, SPRINT_SPEED) };
        let speed = i.velocity.length();
        if walk * 1.05 > speed {
            return 0.0;
        }
        // AngleDiff(m_prevMoveYaw, yaw): Source's yaw grows to the left, so turning right is positive
        let right_turn = self.last.map_or(0.0, |(f, r)| i.forward.dot(r).atan2(i.forward.dot(f)).to_degrees());
        self.last = Some((i.forward, i.right));
        let turn = (right_turn / dt / SPRINT_TILT_TURN).clamp(-1.0, 1.0);
        ((speed - walk) / (sprint - walk)).clamp(0.0, 1.0) * turn
    }

    /// One step towards the target (0x140826C90): the rate goes towards the fastest that can still
    /// stop at the target, sqrt(2 accel (distance - |rate dt|)), at most `sprinttilt_maxvel`,
    /// changing by at most `sprinttilt_accel` dt; the fraction moves by the mean of the old and new
    /// rates. Reaching or passing the target slower than 0.1/s, it stops there.
    fn step(&mut self, dt: f32, target: f32) {
        let diff = target - self.x;
        let room = (diff.abs() - (self.v * dt).abs()).max(0.0);
        let want = (1f32.copysign(diff) * (2.0 * room * SPRINT_TILT_ACCEL).sqrt()).clamp(-SPRINT_TILT_MAXVEL, SPRINT_TILT_MAXVEL);
        let dv = SPRINT_TILT_ACCEL * dt;
        let (x0, v0) = (self.x, self.v);
        self.v = v0 + (want - v0).clamp(-dv, dv);
        self.x = x0 + (v0 + self.v) * 0.5 * dt;
        if (x0 - target) * (self.x - target) <= 0.0 && self.v.abs() < 0.1 {
            (self.x, self.v) = (target, 0.0);
        }
        self.x = self.x.clamp(-1.0, 1.0);
    }
}

/// One axis of a damped spring x'' = -k x - c x', stepped exactly by `dt`: Apex's 0x1408060D0
/// (the client's punch integrator, by the frame time) solves the over-, critically and
/// under-damped cases in closed form the same way.
fn spring_axis(x: f32, v: f32, k: f32, c: f32, dt: f32) -> (f32, f32) {
    let d = c * c - 4.0 * k;
    let a = c / 2.0;
    let e = (-a * dt).exp();
    if d > 1e-6 {
        // over-damped: e^(-a t) (p e^(s t) + q e^(-s t))
        let s = d.sqrt() / 2.0;
        let p = (v + (a + s) * x) / (2.0 * s);
        let q = x - p;
        let (up, down) = ((s * dt).exp(), (-s * dt).exp());
        (e * (p * up + q * down), e * (p * (s - a) * up - q * (s + a) * down))
    } else if d < -1e-6 {
        let w = (-d).sqrt() / 2.0;
        let (cs, sn) = ((w * dt).cos(), (w * dt).sin());
        (e * (x * cs + (v + a * x) / w * sn), e * (v * cs - (a * v + k * x) / w * sn))
    } else {
        // critically damped
        let b = v + a * x;
        (e * (x + b * dt), e * (b - a * (x + b * dt)))
    }
}

/// A damped spring per axis (pitch, yaw, roll), stepped exactly (any frame rate gives the same
/// motion).
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    /// degrees
    pub x: Vec3,
    /// degrees/s
    pub v: Vec3,
}

impl Spring {
    /// Stiffness `k` and damping `c` per axis.
    pub fn step(&mut self, dt: f32, k: Vec3, c: Vec3) {
        for i in 0..3 {
            (self.x[i], self.v[i]) = spring_axis(self.x[i], self.v[i], k[i], c[i], dt);
        }
    }

    pub fn kick(&mut self, dv: Vec3) {
        self.v += dv;
    }
}

/// Octane's launch pad: the trigger's `SetViewPunchValues(15, 4, 0)` (soft, hard, random; the
/// pad's script `_jump_pads.gnut`).
const PAD_PUNCH_SOFT: f32 = 15.0;
const PAD_PUNCH_HARD: f32 = 4.0;

/// The pitch part of S3's punch from a source at the feet (0x140B8CB10, which the pad's launch
/// 0x140B8CEB0 calls for each value with the player's own origin as the source): the eye's view
/// pitched 10° further down gives forward and up; the vector from the box's centre to the source,
/// here `half_height` units straight down, loses its part along that up (what is left is
/// half_height x sin(pitch + 10°) along that forward) and is normalized if its squared length is
/// over 0.1; the punch is (-forward . that, right . that = 0, 0) times the value. So the view kicks
/// up unless it looks more than 10° up. `pitch` in degrees, down positive (Source's).
fn pad_punch_pitch(pitch: f32, half_height: f32) -> f32 {
    let along = half_height * (pitch + 10.0).to_radians().sin();
    -(if along * along > 0.1 { along.signum() } else { along })
}

/// The landing kick's scale for a fall speed (units/s, spec §4.2).
pub fn land_scale(speed: f32, crouched: bool) -> f32 {
    let lo = (2.0 * GRAVITY * 12.0 * FALL_MIN_FT).sqrt();
    let hi = (2.0 * GRAVITY * 12.0 * FALL_MAX_FT).sqrt();
    let s = if speed < lo {
        speed / lo
    } else if speed <= hi {
        1.0 + (speed - lo) / (hi - lo) * (FALL_MAX_SCALE - 1.0)
    } else {
        FALL_MAX_SCALE
    };
    if crouched { s * LAND_CROUCHED } else { s }
}

/// What drives the effects this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    /// Velocity, units/s (world; the slide's roll takes its horizontal part), and the eye's flat
    /// forward and right (world, unit length).
    pub velocity: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub grounded: bool,
    /// Crouched or sliding (Apex's FL_DUCKING: the crouched speeds).
    pub crouched: bool,
    pub sliding: bool,
    pub slide_long_jump: bool,
    pub jumped: bool,
    /// Fall speed (units/s) and whether he was crouched.
    pub landed: Option<(f32, bool)>,
    /// The view model's camera bone turn (pitch, yaw, roll, degrees).
    pub bone: Vec3,
    /// The zoom, 0..1 (the view model's punch spring goes from its hip to its ADS constants).
    pub ads: f32,
}

/// The view model's own punch: kicked with 0.2 of the camera's impulses (0x140807080: the jump
/// adds them to the player's +8392; spec §3).
const VM_KICK: f32 = 0.2;
/// ... and sprung by the weapon's viewkick spring (0x1408067D0 -> 0x140A57130: `viewkick_spring`,
/// towards `viewkick_spring_hot` by the firing heat, which moving alone leaves at 0), hip to ADS by
/// the zoom: the R-301's `rspn101_vkp` (springs.txt), stiffness and damping (pitch, yaw, roll).
/// Pitch and yaw are over-damped: the gun comes back slower than the camera (R5R: "回摆更大更慢").
const VM_K_HIP: Vec3 = Vec3::new(40.0, 45.0, 20000.0);
const VM_C_HIP: Vec3 = Vec3::new(20.0, 20.0, 22.0);
const VM_K_ADS: Vec3 = Vec3::new(115.0, 95.0, 20000.0);
const VM_C_ADS: Vec3 = Vec3::new(20.0, 15.0, 20.0);

pub struct Fx {
    pub punch: Spring,
    /// the view model's own (on top of the camera's, which it follows)
    pub vm_punch: Spring,
    /// U3: a weapon's view kick, Apex's second punch spring (S3 player +27532 angle, +27544
    /// velocity; 0x140BE7F90 kicks it, 0x140B8B380 springs it by the weapon's `viewkick_spring`):
    /// only the Charge Rifle kicks it (chargerifle.rs), so it is `titan_arc`'s; the view shows
    /// (1 - weaponFraction) of it (0x140BE1360), the shot goes along all of it (0x140BF4FC0)
    pub kick: Spring,
    /// the zoom last step (the kick spring and its weapon fraction go from hip to aimed by it)
    ads: f32,
    /// The slide's roll vector (world, horizontal; its length up to 1).
    pub tilt: Vec3,
    pub sprint_tilt: SprintTilt,
    pub fov_scale: f32,
    pub bone: Vec3,
    rng: u64,
}

impl Fx {
    pub fn new(seed: u64) -> Fx {
        Fx {
            punch: Spring::default(),
            vm_punch: Spring::default(),
            kick: Spring::default(),
            ads: 0.0,
            tilt: Vec3::ZERO,
            sprint_tilt: SprintTilt::default(),
            fov_scale: 1.0,
            bone: Vec3::ZERO,
            rng: seed | 1,
        }
    }

    /// A uniform number in [-1, 1] (xorshift).
    fn u(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }

    pub fn step(&mut self, dt: f32, i: &Inputs) {
        let dt = dt.clamp(0.0, 0.1);
        let mut kick = Vec3::ZERO;
        if i.jumped {
            let (u1, u2, u3) = (self.u(), self.u(), self.u());
            kick += JUMP_KICK * Vec3::new(2.0 + u1, u2, 0.9 + 0.5 * u3);
        }
        if let Some((speed, crouched)) = i.landed {
            let s = land_scale(speed, crouched);
            let (u1, u3) = (self.u(), self.u());
            kick += JUMP_KICK * s * Vec3::new(2.0 + 0.75 * u1, 0.0, u3);
        }
        self.punch.kick(kick);
        self.vm_punch.kick(VM_KICK * kick);
        self.punch.step(dt, Vec3::splat(PUNCH_K), Vec3::splat(PUNCH_C));
        let ads = i.ads.clamp(0.0, 1.0);
        self.vm_punch.step(dt, VM_K_HIP.lerp(VM_K_ADS, ads), VM_C_HIP.lerp(VM_C_ADS, ads));
        self.ads = ads;
        if self.kick.x != Vec3::ZERO || self.kick.v != Vec3::ZERO {
            // the weapon in hand's spring (the Wingman's `wingman`, the Charge Rifle's `titan_arc`)
            let ([k_hip, c_hip, k_ads, c_ads], _) = crate::spike::weapons::kick_spring();
            self.kick.step(dt, k_hip.lerp(k_ads, ads), c_hip.lerp(c_ads, ads));
        }

        // the slide's roll: towards the velocity turned a quarter left (the eye's right turns to its
        // forward), by speed
        let v = Vec3::new(i.velocity.x, 0.0, i.velocity.z);
        let speed = v.length();
        let target = if i.sliding && speed > 1e-3 {
            let along = (speed - TILT_STOP) / (TILT_FULL - TILT_STOP);
            let left = i.forward * v.dot(i.right) - i.right * v.dot(i.forward);
            left / speed * along.clamp(0.0, 1.0)
        } else {
            Vec3::ZERO
        };
        let rate = if target.length() <= self.tilt.length() { TILT_DOWN } else { TILT_UP };
        let d = target - self.tilt;
        self.tilt += d.clamp_length_max(rate * dt);

        if dt > 0.0 {
            let t = self.sprint_tilt.target(dt, i);
            self.sprint_tilt.step(dt, t);
        }

        let want = if i.sliding && i.slide_long_jump { SLIDE_FOV } else { 1.0 };
        let step = (SLIDE_FOV - 1.0) / SLIDE_FOV_TIME * dt;
        self.fov_scale += (want - self.fov_scale).clamp(-step, step);
        self.bone = i.bone;
    }

    /// The turn of the render camera from the eye: pitch, yaw, roll (degrees, Source's).
    pub fn turn(&self, forward: Vec3) -> Vec3 {
        let f = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        // roll = -slide_viewTiltSide x (forward . s): sliding with the velocity right of the view,
        // the left side goes down (R5R T6: -3.48°; the horizon high on the left);
        // + sprinttiltMaxRoll x the sprint tilt (0x14085FFC0): turning right the left side goes up
        let roll = SPRINT_TILT_ROLL * self.sprint_tilt.x - TILT_SIDE * f.dot(self.tilt);
        self.punch.x + self.bone + Vec3::new(0.0, 0.0, roll) + self.kick.x * (1.0 - self.weapon_fraction())
    }

    /// The part of the weapon kick the view does not show (S3 0x140BE0BC0: the hip fraction to the
    /// aimed one by the zoom).
    fn weapon_fraction(&self) -> f32 {
        let (_, (hip, ads)) = crate::spike::weapons::kick_spring();
        hip + (ads - hip) * self.ads
    }
}

/// Turns a camera's axes (right, up, forward) by pitch (down), yaw (left) and roll (left side up),
/// degrees, about its own axes; works whatever the handedness of the game's matrix.
pub fn turned(right: Vec3, up: Vec3, fwd: Vec3, t: Vec3) -> [Vec3; 3] {
    let (sp, cp) = t.x.to_radians().sin_cos();
    let (f, u) = (fwd * cp - up * sp, up * cp + fwd * sp);
    let (sy, cy) = t.y.to_radians().sin_cos();
    let (f, r) = (f * cy - right * sy, right * cy + f * sy);
    let (sr, cr) = t.z.to_radians().sin_cos();
    let (r, u) = (r * cr - u * sr, u * cr + r * sr);
    [r.normalize_or_zero(), u.normalize_or_zero(), f.normalize_or_zero()]
}

static FX: Mutex<Option<Fx>> = Mutex::new(None);

/// Once a frame.
pub fn step(dt: f32, i: &Inputs) {
    let mut g = FX.lock().unwrap_or_else(|e| e.into_inner());
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    g.get_or_insert_with(|| Fx::new(seed)).step(dt, i);
}

/// Octane's launch pad launched the player (0x140B8CEB0): the soft value's punch x20 goes into the
/// punch's angular velocity (0x140BE7EF0), the hard value's straight into its angle. The view model
/// gets nothing of its own (the pad does not touch the player's +8392); it follows the punched view.
/// `pitch`: the eye's, degrees down; `half_height`: the player's box, units.
pub fn pad_launch(pitch: f32, half_height: f32) {
    let f = pad_punch_pitch(pitch, half_height);
    let mut g = FX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(fx) = g.as_mut() {
        fx.punch.kick(Vec3::new(JUMP_KICK * PAD_PUNCH_SOFT * f, 0.0, 0.0));
        fx.punch.x += Vec3::new(PAD_PUNCH_HARD * f, 0.0, 0.0);
    }
}

/// U3: a weapon's shot kicks the view (S3 0x140BE7F90): the soft part x20 into the kick spring's
/// angular velocity, the hard part straight into its angle (pitch down, yaw left, roll; degrees).
pub fn weapon_kick(soft: Vec3, hard: Vec3) {
    let mut g = FX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(fx) = g.as_mut() {
        fx.kick.kick(JUMP_KICK * soft);
        fx.kick.x += hard;
    }
}

/// U3: the weapon kick's part the view does not show, which the shot's direction adds (pitch, yaw,
/// roll; degrees).
pub fn weapon_aim_offset() -> Vec3 {
    FX.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(Vec3::ZERO, |f| f.kick.x * f.weapon_fraction())
}

/// The render camera's turn from the eye (degrees) and the field of view's scale.
pub fn current(forward: Vec3) -> (Vec3, f32) {
    FX.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or((Vec3::ZERO, 1.0), |f| (f.turn(forward), f.fov_scale))
}

/// The view model's turn from the eye: the punch and its own on top. Apex draws the view model with
/// the punched view (R5R: its angle peaks about 18 % above the camera's; the rest is its own 0.2x
/// spring), but not with the slide's roll (it rolls against it: -2.5° measured, spec §3) nor the
/// camera bone (on screen the gun turns against that: spec §2.4).
pub fn view_model_turn() -> Vec3 {
    FX.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or(Vec3::ZERO, |f| f.punch.x + f.vm_punch.x)
}

/// For `fp trace`: punch, slide roll vector's part, bone turn, field of view scale.
pub fn describe(forward: Vec3) -> String {
    let g = FX.lock().unwrap_or_else(|e| e.into_inner());
    let Some(f) = g.as_ref() else { return "fx -".into() };
    let t = f.turn(forward);
    format!(
        "fx turn {:.3} {:.3} {:.3} (punch {:.3} {:.3} {:.3}, bone {:.3} {:.3} {:.3}, sprint tilt {:.3}) fov x{:.4} | view model's own punch {:.3} {:.3} {:.3} | weapon kick {:.3} {:.3} {:.3}",
        t.x, t.y, t.z, f.punch.x.x, f.punch.x.y, f.punch.x.z, f.bone.x, f.bone.y, f.bone.z, f.sprint_tilt.x, f.fov_scale, f.vm_punch.x.x, f.vm_punch.x.y, f.vm_punch.x.z, f.kick.x.x, f.kick.x.y, f.kick.x.z
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak_with(dv: f32, k: f32, c: f32) -> (f32, f32) {
        let mut s = Spring::default();
        s.kick(Vec3::new(dv, 0.0, 0.0));
        let (mut best, mut at) = (0.0f32, 0.0f32);
        for i in 1..=2000 {
            s.step(0.0005, Vec3::splat(k), Vec3::splat(c));
            if s.x.x > best {
                (best, at) = (s.x.x, i as f32 * 0.0005);
            }
        }
        (best, at)
    }

    fn peak(dv: f32) -> (f32, f32) {
        peak_with(dv, PUNCH_K, PUNCH_C)
    }

    /// The pad's punch kicks the view up (negative pitch) unless it looks more than 10° up, where it
    /// kicks down; within half a degree of that the short vector is not normalized.
    #[test]
    fn pad_punch_direction() {
        assert_eq!(pad_punch_pitch(0.0, 36.0), -1.0);
        assert_eq!(pad_punch_pitch(80.0, 36.0), -1.0);
        assert_eq!(pad_punch_pitch(-9.0, 36.0), -1.0);
        assert_eq!(pad_punch_pitch(-11.0, 36.0), 1.0);
        assert_eq!(pad_punch_pitch(-60.0, 36.0), 1.0);
        let near = pad_punch_pitch(-9.8, 36.0);
        assert!((near + 36.0 * 0.2f32.to_radians().sin()).abs() < 1e-5, "{near}");
        // the angle starts at -4° and the impulse adds -300°/s: the view rises further first
        let mut s = Spring::default();
        let f = pad_punch_pitch(0.0, 36.0);
        s.kick(Vec3::new(JUMP_KICK * PAD_PUNCH_SOFT * f, 0.0, 0.0));
        s.x += Vec3::new(PAD_PUNCH_HARD * f, 0.0, 0.0);
        let mut low = 0.0f32;
        for _ in 0..600 {
            s.step(1.0 / 600.0, Vec3::splat(PUNCH_K), Vec3::splat(PUNCH_C));
            low = low.min(s.x.x);
        }
        assert!(low < -4.0 && s.x.x.abs() < 1.0, "lowest {low}, after 1 s {}", s.x.x);
    }

    /// A7: a unit impulse peaks at 0.0642 at 0.146 s (spec §4.2), whatever the frame rate.
    #[test]
    fn spring_unit_impulse() {
        let (p, t) = peak(1.0);
        assert!((p - 0.0642).abs() < 0.003 && (t - 0.146).abs() < 0.01, "{p} at {t}");
        let run = |dt: f32| {
            let mut s = Spring::default();
            s.kick(Vec3::ONE);
            for _ in 0..(0.5 / dt).round() as usize {
                s.step(dt, Vec3::splat(PUNCH_K), Vec3::splat(PUNCH_C));
            }
            s.x
        };
        assert!((run(1.0 / 30.0) - run(1.0 / 144.0)).length() < 1e-4);
    }

    /// The closed forms against a fine numerical integration, for every case 0x1408060D0 has:
    /// under-damped (punch_pilot), over-damped (rspn101_vkp pitch), critical, and the stiff roll.
    #[test]
    fn spring_cases_match_integration() {
        for (k, c) in [(65.0f32, 9.0f32), (40.0, 20.0), (45.0, 20.0), (100.0, 20.0), (20000.0, 22.0)] {
            let (mut x, mut v) = (0.3f64, 5.0f64);
            let (mut xs, mut vs) = (0.3f32, 5.0f32);
            let h = 1e-6;
            for _ in 0..30 {
                for _ in 0..(1.0 / 60.0 / h) as usize {
                    // semi-implicit Euler at 1 µs
                    v += (-(k as f64) * x - c as f64 * v) * h;
                    x += v * h;
                }
                (xs, vs) = spring_axis(xs, vs, k, c, 1.0 / 60.0);
            }
            assert!((xs as f64 - x).abs() < 2e-3 && (vs as f64 - v).abs() < 2e-2 * (1.0 + v.abs()), "k {k} c {c}: {xs} {vs} vs {x} {v}");
        }
    }

    /// A7: the pitch peaks for the extremes of U: jump 1.3-3.9°, landing at 612 units/s 6.4-14.0°,
    /// at 861 12.7-27.9° (R5R's 2.24/3.30, 10.67, 23.13 inside).
    #[test]
    fn punch_ranges() {
        let unit = peak(1.0).0;
        let jump = (JUMP_KICK * 1.0 * unit, JUMP_KICK * 3.0 * unit);
        assert!((jump.0 - 1.3).abs() < 0.05 && (jump.1 - 3.9).abs() < 0.05, "{jump:?}");
        for (v, lo, hi) in [(612.0, 6.4, 14.0), (861.0, 12.7, 27.9)] {
            let s = land_scale(v, false);
            let (a, b) = (JUMP_KICK * s * 1.25 * unit, JUMP_KICK * s * 2.75 * unit);
            assert!((a - lo).abs() < 0.15 && (b - hi).abs() < 0.25, "{v}: {a} {b}");
        }
        assert!((land_scale(424.26, false) - 1.0).abs() < 1e-3);
        assert_eq!(land_scale(5000.0, false), 12.0);
        assert!((land_scale(612.0, true) - 0.2 * land_scale(612.0, false)).abs() < 1e-6);
        // R5R's jump-landing (260 units/s, s about 0.61)
        assert!((land_scale(260.0, false) - 0.613).abs() < 0.01);
    }

    /// The view model follows the camera's punch and adds its own 0.2x through the R-301's
    /// rspn101_vkp spring (over-damped in pitch: a unit impulse peaks at 0.0418 at 0.133 s): its
    /// angle peaks about 13 % above the camera's (R5R: about 18 %) and comes back slower (R5R:
    /// "回摆更大更慢"): when the camera has swung past zero, the gun is still up.
    #[test]
    fn view_model_punch() {
        let (p, t) = peak_with(1.0, VM_K_HIP.x, VM_C_HIP.x);
        assert!((p - 0.0418).abs() < 0.001 && (t - 0.133).abs() < 0.005, "{p} at {t}");
        let mut f = Fx::new(7);
        let (mut cam, mut vm, mut cam_low, mut vm_at_cam_low) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for i in 0..60 {
            f.step(1.0 / 60.0, &Inputs { jumped: i == 0, ..Default::default() });
            cam = cam.max(f.punch.x.x);
            vm = vm.max(f.punch.x.x + f.vm_punch.x.x);
            if f.punch.x.x < cam_low {
                (cam_low, vm_at_cam_low) = (f.punch.x.x, f.vm_punch.x.x);
            }
        }
        assert!(cam > 1.0 && vm / cam > 1.1 && vm / cam < 1.2, "{cam} {vm}");
        assert!(cam_low < 0.0 && vm_at_cam_low > 0.0, "{cam_low} {vm_at_cam_low}");
    }

    #[test]
    fn random_in_range() {
        let mut f = Fx::new(12345);
        let u: Vec<f32> = (0..10000).map(|_| f.u()).collect();
        assert!(u.iter().all(|x| (-1.0..=1.0).contains(x)));
        let mean = u.iter().sum::<f32>() / u.len() as f32;
        assert!(mean.abs() < 0.05 && u.iter().any(|x| *x < -0.95) && u.iter().any(|x| *x > 0.95));
    }

    fn sliding(speed: f32, right_of_view_deg: f32) -> Inputs {
        let a = right_of_view_deg.to_radians();
        // the eye looks along +z with +x on its right
        let (forward, right) = (Vec3::Z, Vec3::X);
        Inputs { velocity: (forward * a.cos() + right * a.sin()) * speed, forward, right, sliding: true, ..Default::default() }
    }

    /// A8: the roll's two R5R points (spec §4.3), once the vector has eased in; velocity right of
    /// the view gives Source roll negative (the left side down).
    #[test]
    fn slide_roll_points() {
        for (speed, deg, want) in [(208.2, 50.1, -3.48), (176.5, 31.2, -1.44)] {
            let mut f = Fx::new(1);
            for _ in 0..120 {
                f.step(1.0 / 60.0, &sliding(speed, deg));
            }
            let r = f.turn(Vec3::Z).z;
            assert!((r - want).abs() < 0.05, "{speed} {deg}: {r}");
            let l = f.turn(Vec3::Z).z;
            let mut g = Fx::new(1);
            for _ in 0..120 {
                g.step(1.0 / 60.0, &sliding(speed, -deg));
            }
            assert!((g.turn(Vec3::Z).z + l).abs() < 1e-4, "mirror");
        }
        // straight ahead: no roll
        let mut f = Fx::new(1);
        for _ in 0..60 {
            f.step(1.0 / 60.0, &sliding(300.0, 0.0));
        }
        assert!(f.turn(Vec3::Z).z.abs() < 1e-4);
    }

    /// A8: the roll vector eases at 5/s towards a bigger target and 2.5/s back.
    #[test]
    fn slide_roll_rates() {
        let mut f = Fx::new(1);
        // full speed sideways: target length 1
        f.step(0.1, &sliding(400.0, 90.0));
        assert!((f.tilt.length() - 0.5).abs() < 1e-4);
        f.step(0.1, &Inputs { forward: Vec3::Z, right: Vec3::X, ..Default::default() });
        assert!((f.tilt.length() - 0.25).abs() < 1e-4);
    }

    /// A8: the slide FOV x1.1 over 0.25 s in and out, only with slideLongJumpAllowed.
    #[test]
    fn slide_fov() {
        let mut f = Fx::new(1);
        let slide = Inputs { sliding: true, slide_long_jump: true, ..Default::default() };
        for _ in 0..15 {
            f.step(1.0 / 60.0, &slide);
        }
        assert!((f.fov_scale - 1.1).abs() < 1e-4, "{}", f.fov_scale);
        // half the way out in 0.125 s (steps are capped at 0.1 s)
        f.step(0.0625, &Inputs::default());
        f.step(0.0625, &Inputs::default());
        assert!((f.fov_scale - 1.05).abs() < 1e-4, "{}", f.fov_scale);
        for _ in 0..2 {
            f.step(0.1, &Inputs { sliding: true, ..Default::default() });
        }
        assert!((f.fov_scale - 1.0).abs() < 1e-4, "no boost left: no FOV");
    }

    /// Sprinting at `speed` on the ground, the view turning right at `rate` °/s (the eye looks along
    /// +z with +x on its right at first; negative rates turn left).
    fn sprint_turning(f: &mut Fx, frames: usize, speed: f32, rate: f32, crouched: bool) -> Vec<f32> {
        let mut yaw = 0.0f32;
        (0..frames)
            .map(|_| {
                yaw += rate / 60.0;
                let (s, c) = yaw.to_radians().sin_cos();
                let (forward, right) = (Vec3::new(s, 0.0, c), Vec3::new(c, 0.0, -s));
                f.step(1.0 / 60.0, &Inputs { velocity: forward * speed, forward, right, grounded: true, crouched, ..Default::default() });
                f.turn(forward).z
            })
            .collect()
    }

    /// The sprint tilt (spec §4.3): sprinting (260) and turning right at the turn range 120°/s, the
    /// fraction goes to 1 in the fastest way (35/s² up to 2/s, then down: about 0.56 s) and the roll
    /// to +2° (the left side up: leaning into the turn); turning left the mirror; going straight,
    /// walking, crouched or in the air nothing; half way between walk and sprint speed, half.
    #[test]
    fn sprint_tilt() {
        let mut f = Fx::new(1);
        let r = sprint_turning(&mut f, 60, 260.0, 120.0, false);
        let at = r.iter().position(|x| (x - 2.0).abs() < 1e-4).expect("reaches 2°");
        assert!((32..=36).contains(&at), "at frame {at}: {:?}", &r[..40]);
        // (the measured turn has float noise: the target wobbles by about 1e-6)
        assert!(r.windows(2).all(|w| w[1] >= w[0] - 1e-4) && r.iter().all(|x| *x <= 2.0), "no overshoot: {r:?}");
        assert!(f.sprint_tilt.v.abs() < 0.01, "{}", f.sprint_tilt.v);
        let mut f = Fx::new(1);
        let l = sprint_turning(&mut f, 60, 260.0, -120.0, false);
        assert!(r.iter().zip(&l).all(|(a, b)| (a + b).abs() < 1e-5), "mirror");
        // a faster turn is clamped to the same
        let mut f = Fx::new(1);
        assert!((sprint_turning(&mut f, 60, 260.0, 300.0, false)[59] - 2.0).abs() < 1e-4);
        // half way between walk and sprint speed, half the turn range: a quarter
        let mut f = Fx::new(1);
        let q = sprint_turning(&mut f, 120, (173.5 + 260.0) / 2.0, 60.0, false);
        assert!((q[119] - 0.5).abs() < 1e-3, "{}", q[119]);
        for (speed, rate, crouched) in [(260.0, 0.0, false), (175.0, 120.0, false), (260.0, 120.0, true)] {
            let mut f = Fx::new(1);
            let r = sprint_turning(&mut f, 60, speed, rate, crouched);
            assert!(r.iter().all(|x| x.abs() < 1e-6), "{speed} {rate} {crouched}: {:?}", &r[..5]);
        }
        // in the air: back to 0
        let mut f = Fx::new(1);
        sprint_turning(&mut f, 60, 260.0, 120.0, false);
        for _ in 0..60 {
            f.step(1.0 / 60.0, &Inputs { velocity: Vec3::Z * 260.0, forward: Vec3::Z, right: Vec3::X, ..Default::default() });
        }
        assert_eq!(f.sprint_tilt.x, 0.0);
    }

    /// The camera's axes turned: pitch down tips the forward down, yaw left turns it to the left,
    /// roll lifts the left side; in either handedness.
    #[test]
    fn turned_axes() {
        for handed in [1.0f32, -1.0] {
            let (r, u, f) = (Vec3::X * handed, Vec3::Y, Vec3::Z);
            let [_, _, f2] = turned(r, u, f, Vec3::new(10.0, 0.0, 0.0));
            assert!(f2.y < 0.0 && (f2.angle_between(f).to_degrees() - 10.0).abs() < 1e-3);
            let [_, _, f3] = turned(r, u, f, Vec3::new(0.0, 10.0, 0.0));
            assert!(f3.dot(r) < 0.0, "left is away from the right");
            let [r4, u4, _] = turned(r, u, f, Vec3::new(0.0, 0.0, 10.0));
            assert!((-r4).dot(Vec3::Y) > 0.0 && u4.dot(r) > 0.0, "left side up");
        }
    }
}
