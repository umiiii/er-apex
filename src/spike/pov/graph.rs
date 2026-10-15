//! The view model's animation graph (plan-gun-motion-hud.md 2.3, gun-motion spec §2): which of
//! Apex's ptpov_rspn101 sequences play, at what cycle and with what weight, from what Fuse does.
//! Fixed rules after S3's view-model code and R5R's measurements, not a general animation graph;
//! it has no game or pack in it, so the rules are tested offline.
//!
//! - Base (absolute poses; a new sequence fades in over its QC fade-in on top of the ones before,
//!   which play on under it until it is in): aiming, that is `ads_in` at cycle = zoom after the zoom
//!   last went up and `ads_out` at 1 - zoom otherwise (the hip is `ads_out` at its end); `sprint`,
//!   looping from cycle 0, when sprint starts; `sprintraise` when it ends (a jump ends it: er-apex-move);
//!   `sprintslide` when a slide starts out of sprint. Both raises go back to aiming at their end.
//!   The reload (`reload` / `reload_empty` at the gun's reload progress) goes over all of it with
//!   its own fades.
//! - Additive: the idle node (the `idle` / `crouch` loops with `idle_to_crouch` / `crouch_to_idle`
//!   played between them, sequence by sequence as the base), `fire` at each shot, `jump`, `land`
//!   (one-shots with their fade-in and fade-out; a restart fades in over the one playing) and the
//!   autoplaying `wind_effect_layer` by horizontal speed.
//! - A sequence's samples blend by `ads_blend` (the zoom), `crouchFraction` and `velocity` (T012's
//!   grid: the first axis along the columns).
//!
//! Inferred, not measured in S3 (noted in the plan): zooming in or firing cuts sprint and the
//! raises short (the mod's sprint doesn't stop for them yet), sprint shows again `SHOT_HOLD` after
//! the last shot, and the idle node plays under every base sequence.

/// QC's (studiomdl's) fade where the QC gives none, seconds.
const FADE: f32 = 0.2;
/// Seconds after a shot before a sprint that goes on shows the sprint pose again (inferred).
const SHOT_HOLD: f32 = 0.3;

/// The sequences the graph plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seq {
    AdsIn,
    AdsOut,
    Idle,
    Crouch,
    IdleToCrouch,
    CrouchToIdle,
    Fire,
    Jump,
    Land,
    Sprint,
    SprintRaise,
    SprintSlide,
    Reload,
    ReloadEmpty,
    Wind,
}

/// How a sequence's samples are laid out (QC `blend`; T012's report).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axes {
    One,
    /// `crouchFraction`: sample 0 standing, 1 crouched (width-1 grids vary along the rows)
    Crouch,
    /// `ads_blend`: 0 hip, 1 aiming
    Ads,
    /// 2x2: `ads_blend` along the columns, `crouchFraction` along the rows
    AdsCrouch,
    /// `velocity` 200..500 units/s
    Velocity,
}

pub struct SeqDef {
    /// The QC name; sample k is the clip `<name>_<k>`.
    pub name: &'static str,
    /// Frames and fps of each sample (apex-data/pov/fuse_pov_sequences.json; the pack test checks
    /// them against the baked clips).
    pub samples: &'static [(u32, f32)],
    pub axes: Axes,
    pub fade_in: f32,
    pub fade_out: f32,
    pub looping: bool,
}

const fn def(name: &'static str, samples: &'static [(u32, f32)], axes: Axes, fade_in: f32, fade_out: f32, looping: bool) -> SeqDef {
    SeqDef { name, samples, axes, fade_in, fade_out, looping }
}

/// The weapon whose sequences a graph plays (its clips: the R-301's `<name>_<k>`, the Charge
/// Rifle's T022 `cr_<name>_<k>`, the Wingman's `wm_<name>_<k>`: tools/apexpov/bake_wingman.py).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weapon {
    #[default]
    R301,
    ChargeRifle,
    /// the R-301's slot's other guns (the weapon wheel: weapons.rs `Gun`)
    Wingman,
    R99,
    Flatline,
    Sentinel,
}

impl Weapon {
    /// The prefix of its clips in the pack (`<prefix><name>_<k>`).
    pub fn prefix(self) -> &'static str {
        match self {
            Weapon::R301 => "",
            Weapon::ChargeRifle => "cr_",
            Weapon::Wingman => "wm_",
            Weapon::R99 => "r9_",
            Weapon::Flatline => "fl_",
            Weapon::Sentinel => "sn_",
        }
    }
}

/// QC `ptpov_rspn101.qc` (T012): samples, blends, `fadein` / `fadeout`, `loop`; by `Seq as usize`.
static DEFS: [SeqDef; 15] = [
    def("ads_in", &[(16, 30.0), (12, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("ads_out", &[(16, 30.0), (16, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("idle", &[(191, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("crouch", &[(191, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("idle_to_crouch", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("crouch_to_idle", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("fire", &[(28, 30.0), (11, 30.0), (28, 30.0), (11, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("sprintraise", &[(10, 30.0)], Axes::One, FADE, FADE, false),
    def("sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("reload", &[(67, 30.0), (67, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("reload_empty", &[(87, 30.0), (87, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

/// The Charge Rifle's (T022: retail `chargerifle_base_v_animRig.qc` through
/// apex-data/pov/octane_weapons/defender_sequences.json; QC fades as given, else FADE): the same
/// sequences by the same rules, its own frames.
static CR_DEFS: [SeqDef; 15] = [
    def("cr_ads_in", &[(11, 30.0), (11, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("cr_ads_out", &[(16, 30.0), (16, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("cr_idle", &[(186, 30.0), (186, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("cr_crouch", &[(186, 30.0), (186, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("cr_idle_to_crouch", &[(29, 30.0), (29, 30.0)], Axes::Ads, FADE, FADE, false),
    def("cr_crouch_to_idle", &[(29, 30.0), (29, 30.0)], Axes::Ads, FADE, FADE, false),
    def("cr_fire", &[(40, 30.0), (33, 30.0), (33, 30.0), (33, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("cr_jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("cr_land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("cr_sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("cr_sprintraise", &[(11, 30.0)], Axes::One, 0.5, FADE, false),
    def("cr_sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("cr_reload", &[(137, 30.0), (137, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("cr_reload_empty", &[(174, 30.0), (174, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("cr_wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

/// The Wingman's (retail `wingman_base_v_animRig.qc` through
/// apex-data/pov/octane_wingman/wingman_sequences.json; QC fades as given, else FADE).
static WM_DEFS: [SeqDef; 15] = [
    def("wm_ads_in", &[(11, 30.0), (8, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("wm_ads_out", &[(11, 30.0), (11, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("wm_idle", &[(191, 30.0), (121, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("wm_crouch", &[(191, 30.0), (121, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("wm_idle_to_crouch", &[(29, 30.0), (121, 30.0)], Axes::Ads, FADE, FADE, false),
    def("wm_crouch_to_idle", &[(29, 30.0), (121, 30.0)], Axes::Ads, FADE, FADE, false),
    def("wm_fire", &[(20, 30.0), (16, 30.0), (20, 30.0), (16, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("wm_jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("wm_land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("wm_sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("wm_sprintraise", &[(11, 30.0)], Axes::One, FADE, FADE, false),
    def("wm_sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("wm_reload", &[(89, 30.0), (89, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("wm_reload_empty", &[(89, 30.0), (89, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("wm_wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

/// The R-99's (retail `r99_base_v_animRig.qc` through apex-data/pov/octane_wingman/r99_sequences.json;
/// its reloads are `reload_seq` / `reload_empty_seq`).
static R9_DEFS: [SeqDef; 15] = [
    def("r9_ads_in", &[(11, 30.0), (12, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("r9_ads_out", &[(16, 30.0), (16, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("r9_idle", &[(191, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("r9_crouch", &[(191, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("r9_idle_to_crouch", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("r9_crouch_to_idle", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("r9_fire", &[(28, 30.0), (20, 30.0), (28, 30.0), (20, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("r9_jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("r9_land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("r9_sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("r9_sprintraise", &[(11, 30.0)], Axes::One, FADE, FADE, false),
    def("r9_sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("r9_reload_seq", &[(73, 30.0), (73, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("r9_reload_empty_seq", &[(94, 30.0), (94, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("r9_wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

/// The VK-47 Flatline's (retail `ptpov_vinson.qc` through
/// apex-data/pov/octane_wingman/flatline_sequences.json; QC fades as given, else FADE).
static FL_DEFS: [SeqDef; 15] = [
    def("fl_ads_in", &[(12, 30.0), (12, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("fl_ads_out", &[(16, 30.0), (16, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("fl_idle", &[(190, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("fl_crouch", &[(190, 30.0), (191, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("fl_idle_to_crouch", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("fl_crouch_to_idle", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("fl_fire", &[(27, 30.0), (11, 30.0), (27, 30.0), (11, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("fl_jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("fl_land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("fl_sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("fl_sprintraise", &[(11, 30.0)], Axes::One, FADE, FADE, false),
    def("fl_sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("fl_reload", &[(70, 30.0), (73, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("fl_reload_empty", &[(94, 30.0), (94, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("fl_wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

/// The Sentinel's (retail `sentinel_base_v_animRig.qc` through
/// apex-data/pov/octane_wingman/sentinel_sequences.json; QC fades as given, else FADE).
static SN_DEFS: [SeqDef; 15] = [
    def("sn_ads_in", &[(11, 30.0), (11, 30.0)], Axes::Crouch, 0.1, FADE, false),
    def("sn_ads_out", &[(11, 30.0), (11, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("sn_idle", &[(231, 30.0), (231, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("sn_crouch", &[(231, 30.0), (231, 30.0)], Axes::Ads, 0.3, 0.3, true),
    def("sn_idle_to_crouch", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("sn_crouch_to_idle", &[(29, 30.0), (191, 30.0)], Axes::Ads, FADE, FADE, false),
    def("sn_fire", &[(33, 30.0), (33, 30.0), (32, 30.0), (33, 30.0)], Axes::AdsCrouch, 0.05, FADE, false),
    def("sn_jump", &[(31, 30.0), (22, 30.0), (31, 30.0), (22, 30.0)], Axes::AdsCrouch, FADE, 0.35, false),
    def("sn_land", &[(19, 30.0), (19, 30.0), (19, 30.0), (19, 30.0)], Axes::AdsCrouch, 0.05, 0.35, false),
    def("sn_sprint", &[(21, 36.0)], Axes::One, FADE, FADE, true),
    def("sn_sprintraise", &[(11, 30.0)], Axes::One, FADE, FADE, false),
    def("sn_sprintslide", &[(14, 30.0)], Axes::One, FADE, FADE, false),
    def("sn_reload", &[(113, 30.0), (113, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("sn_reload_empty", &[(131, 30.0), (131, 30.0)], Axes::Crouch, FADE, FADE, false),
    def("sn_wind_effect_layer", &[(1, 30.0), (16, 30.0)], Axes::Velocity, FADE, FADE, false),
];

impl Seq {
    pub const ALL: [Seq; 15] = [
        Seq::AdsIn,
        Seq::AdsOut,
        Seq::Idle,
        Seq::Crouch,
        Seq::IdleToCrouch,
        Seq::CrouchToIdle,
        Seq::Fire,
        Seq::Jump,
        Seq::Land,
        Seq::Sprint,
        Seq::SprintRaise,
        Seq::SprintSlide,
        Seq::Reload,
        Seq::ReloadEmpty,
        Seq::Wind,
    ];

    /// The R-301's.
    pub fn def(self) -> &'static SeqDef {
        &DEFS[self as usize]
    }

    /// The weapon's.
    pub fn def_in(self, w: Weapon) -> &'static SeqDef {
        match w {
            Weapon::R301 => &DEFS[self as usize],
            Weapon::ChargeRifle => &CR_DEFS[self as usize],
            Weapon::Wingman => &WM_DEFS[self as usize],
            Weapon::R99 => &R9_DEFS[self as usize],
            Weapon::Flatline => &FL_DEFS[self as usize],
            Weapon::Sentinel => &SN_DEFS[self as usize],
        }
    }

    /// An additive layer (QC `delta`).
    pub fn additive(self) -> bool {
        matches!(self, Seq::Idle | Seq::Crouch | Seq::IdleToCrouch | Seq::CrouchToIdle | Seq::Fire | Seq::Jump | Seq::Land | Seq::Wind)
    }
}

/// The blend parameters (QC pose parameters).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    /// `ads_blend`: the zoom, 0..1
    pub ads: f32,
    /// `crouchFraction`
    pub crouch: f32,
    /// `velocity`: horizontal speed, units/s
    pub velocity: f32,
}

impl SeqDef {
    /// Each sample's weight; they add up to 1.
    pub fn weights(&self, p: Params) -> [f32; 4] {
        let (a, c) = (p.ads.clamp(0.0, 1.0), p.crouch.clamp(0.0, 1.0));
        match self.axes {
            Axes::One => [1.0, 0.0, 0.0, 0.0],
            Axes::Crouch => [1.0 - c, c, 0.0, 0.0],
            Axes::Ads => [1.0 - a, a, 0.0, 0.0],
            Axes::AdsCrouch => [(1.0 - a) * (1.0 - c), a * (1.0 - c), (1.0 - a) * c, a * c],
            Axes::Velocity => {
                let v = ((p.velocity - 200.0) / 300.0).clamp(0.0, 1.0);
                [1.0 - v, v, 0.0, 0.0]
            }
        }
    }

    /// Cycles per second at these weights (Source's Studio_CPS: each sample's fps / (frames - 1) by
    /// its weight; one-frame samples add nothing).
    pub fn cps(&self, p: Params) -> f32 {
        self.samples.iter().zip(self.weights(p)).filter(|((n, _), _)| *n > 1).map(|((n, fps), w)| w * fps / (*n - 1) as f32).sum()
    }
}

/// What drives the graph each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Signals {
    /// The zoom, 0 hip .. 1 sights (linear in time: camera.rs).
    pub ads: f32,
    /// A shot since the last step.
    pub shot: bool,
    /// The gun's reload progress 0..1 and whether it is the empty-magazine one.
    pub reload: Option<(f32, bool)>,
    /// The movement controller; None while it is off (then he stands still).
    pub moving: Option<Moving>,
}

/// The movement controller's state and this frame's events (spike/kcc.rs `Locomotion`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Moving {
    pub sprinting: bool,
    pub sliding: bool,
    /// Crouched or sliding.
    pub crouched: bool,
    /// Apex's crouchFraction.
    pub duck_frac: f32,
    /// Horizontal speed, units/s.
    pub speed: f32,
    pub jumped: bool,
    pub landed: bool,
    pub duck_started: bool,
    pub unduck_started: bool,
}

/// A sequence playing.
#[derive(Clone, Copy, Debug)]
struct Play {
    seq: Seq,
    cycle: f32,
    /// Seconds since it started.
    age: f32,
    w: Weapon,
}

impl Play {
    fn new(seq: Seq, w: Weapon) -> Play {
        Play { seq, cycle: 0.0, age: 0.0, w }
    }

    /// Fully in from the start (the graph's first state).
    fn settled(seq: Seq, cycle: f32, w: Weapon) -> Play {
        Play { seq, cycle, age: 10.0, w }
    }

    /// How far its fade-in has gone, 0..1.
    fn fade(&self) -> f32 {
        let f = self.seq.def_in(self.w).fade_in;
        if f > 0.0 { (self.age / f).min(1.0) } else { 1.0 }
    }

    fn advance(&mut self, dt: f32, p: Params) {
        self.age += dt;
        let d = self.seq.def_in(self.w);
        self.cycle += dt * d.cps(p);
        self.cycle = if d.looping { self.cycle.rem_euclid(1.0) } else { self.cycle.min(1.0) };
    }

    /// One-shots: the weight left near the end (fades out over its last `fade_out` seconds).
    fn tail(&self, p: Params) -> f32 {
        let d = self.seq.def_in(self.w);
        let cps = d.cps(p);
        if d.fade_out <= 0.0 || cps <= 0.0 { 1.0 } else { ((1.0 - self.cycle) / cps / d.fade_out).clamp(0.0, 1.0) }
    }
}

/// Drops what is fully covered: everything before the last sequence that has faded in.
fn prune(v: &mut Vec<Play>) {
    if let Some(i) = v.iter().rposition(|p| p.fade() >= 1.0) {
        v.drain(..i);
    }
}

/// A sequence to pose this frame: at `cycle`, by `weight` (base: how far it blends over what is
/// under it; additive: its weight).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pass {
    pub seq: Seq,
    pub cycle: f32,
    pub weight: f32,
}

/// What to pose this frame.
#[derive(Clone, Debug, Default)]
pub struct Out {
    pub params: Params,
    /// whose sequences the passes are
    pub w: Weapon,
    /// Absolute: the first, then each next one blended over by its weight.
    pub base: Vec<Pass>,
    /// Additive, each by its weight.
    pub add: Vec<Pass>,
}

/// Additive passes of a slot: the newest by its fade-in, each older one by what the newer ones
/// leave of it; `one_shot`: they start from nothing and fade out at their end.
fn layers(v: &[Play], one_shot: bool, p: Params, out: &mut Vec<Pass>) {
    let mut left = 1.0;
    for (i, s) in v.iter().enumerate().rev() {
        let fade = if i == 0 && !one_shot { 1.0 } else { s.fade() };
        let weight = left * fade * if one_shot { s.tail(p) } else { 1.0 };
        left *= 1.0 - fade;
        if weight > 0.0 {
            out.push(Pass { seq: s.seq, cycle: s.cycle, weight });
        }
    }
}

pub struct Graph {
    base: Vec<Play>,
    /// The zoom last went up.
    zoom_in: bool,
    ads: f32,
    reload: Option<(f32, bool)>,
    reload_w: f32,
    idle: Vec<Play>,
    fire: Vec<Play>,
    jump: Vec<Play>,
    land: Vec<Play>,
    wind: f32,
    /// Last step's: the sprint pose wanted, sliding.
    sprint: bool,
    sliding: bool,
    since_shot: f32,
    params: Params,
    w: Weapon,
}

impl Default for Graph {
    fn default() -> Self {
        Graph::new()
    }
}

impl Graph {
    /// The R-301's, at the hip, standing still.
    pub fn new() -> Graph {
        Graph::new_for(Weapon::R301)
    }

    /// A weapon's, at the hip, standing still.
    pub fn new_for(w: Weapon) -> Graph {
        Graph {
            base: vec![Play::settled(Seq::AdsOut, 1.0, w)],
            zoom_in: false,
            ads: 0.0,
            reload: None,
            reload_w: 0.0,
            idle: vec![Play::settled(Seq::Idle, 0.0, w)],
            fire: Vec::new(),
            jump: Vec::new(),
            land: Vec::new(),
            wind: 0.0,
            sprint: false,
            sliding: false,
            since_shot: f32::INFINITY,
            params: Params::default(),
            w,
        }
    }

    /// The weapon whose sequences it plays.
    pub fn weapon(&self) -> Weapon {
        self.w
    }

    fn aim(&self) -> Seq {
        if self.zoom_in { Seq::AdsIn } else { Seq::AdsOut }
    }

    fn top(&self) -> &Play {
        self.base.last().expect("the base is never empty")
    }

    fn push_base(&mut self, seq: Seq) {
        self.base.push(Play::new(seq, self.w));
    }

    /// One frame of `dt` seconds.
    pub fn step(&mut self, dt: f32, s: &Signals) {
        let dt = dt.clamp(0.0, 0.1);
        let m = s.moving.unwrap_or_default();
        let ads = s.ads.clamp(0.0, 1.0);
        if ads > self.ads + 1e-5 {
            self.zoom_in = true;
        } else if ads < self.ads - 1e-5 {
            self.zoom_in = false;
        }
        self.ads = ads;
        let p = Params { ads, crouch: m.duck_frac.clamp(0.0, 1.0), velocity: m.speed };
        self.params = p;
        self.since_shot = if s.shot { 0.0 } else { self.since_shot + dt };

        // base: what played on, then this frame's changes (new sequences start at cycle 0)
        for b in &mut self.base {
            match b.seq {
                Seq::AdsIn | Seq::AdsOut => b.age += dt,
                _ => b.advance(dt, p),
            }
        }
        let aiming_in = self.zoom_in && ads > 0.0;
        let sprint = m.sprinting && !aiming_in && self.since_shot > SHOT_HOLD;
        let top = self.top().seq;
        if m.sliding && !self.sliding && (self.sprint || top == Seq::Sprint) {
            self.push_base(Seq::SprintSlide);
        } else if sprint && !self.sprint {
            self.push_base(Seq::Sprint);
        } else if !sprint && top == Seq::Sprint {
            self.push_base(if aiming_in || s.shot { self.aim() } else { Seq::SprintRaise });
        }
        let top = *self.top();
        if matches!(top.seq, Seq::SprintRaise | Seq::SprintSlide) && (aiming_in || s.shot || top.cycle >= 1.0) {
            self.push_base(self.aim());
        }
        let top = self.top().seq;
        if matches!(top, Seq::AdsIn | Seq::AdsOut) && top != self.aim() {
            self.push_base(self.aim());
        }
        for b in &mut self.base {
            match b.seq {
                Seq::AdsIn => b.cycle = ads,
                Seq::AdsOut => b.cycle = 1.0 - ads,
                _ => {}
            }
        }
        prune(&mut self.base);
        self.sprint = sprint;
        self.sliding = m.sliding;

        // the reload over it
        let r = Seq::Reload.def_in(self.w);
        if let Some(reload) = s.reload {
            self.reload = Some(reload);
            self.reload_w = (self.reload_w + dt / r.fade_in).min(1.0);
        } else {
            self.reload_w = (self.reload_w - dt / r.fade_out).max(0.0);
        }

        // the idle node
        for b in &mut self.idle {
            b.advance(dt, p);
        }
        if m.duck_started {
            // a slide's duck goes straight to the crouch loop: R5R's camera shows no duck transition
            // when a slide starts (T2: the roll stays under 0.05°, without the standing duck's -0.7°
            // of T4, and the pitch is back at 0 by 0.45 s)
            self.idle.push(Play::new(if m.sliding { Seq::Crouch } else { Seq::IdleToCrouch }, self.w));
        } else if m.unduck_started {
            self.idle.push(Play::new(Seq::CrouchToIdle, self.w));
        }
        let top = *self.idle.last().expect("the idle node is never empty");
        let next = match top.seq {
            Seq::IdleToCrouch if top.cycle >= 1.0 => Some(Seq::Crouch),
            Seq::CrouchToIdle if top.cycle >= 1.0 => Some(Seq::Idle),
            // a duck missed (the graph started crouched)
            Seq::Idle if m.crouched => Some(Seq::Crouch),
            Seq::Crouch if !m.crouched => Some(Seq::Idle),
            _ => None,
        };
        if let Some(seq) = next {
            self.idle.push(Play::new(seq, self.w));
        }
        prune(&mut self.idle);

        // one-shots
        let w = self.w;
        for (v, start, seq) in [(&mut self.fire, s.shot, Seq::Fire), (&mut self.jump, m.jumped, Seq::Jump), (&mut self.land, m.landed, Seq::Land)] {
            for b in v.iter_mut() {
                b.advance(dt, p);
            }
            v.retain(|b| b.cycle < 1.0);
            if start {
                v.push(Play::new(seq, w));
            }
            prune(v);
        }
        self.wind = (self.wind + dt * Seq::Wind.def_in(self.w).cps(p)).rem_euclid(1.0);
    }

    pub fn out(&self) -> Out {
        let p = self.params;
        let mut o = Out { params: p, w: self.w, base: Vec::new(), add: Vec::new() };
        for (i, b) in self.base.iter().enumerate() {
            o.base.push(Pass { seq: b.seq, cycle: b.cycle, weight: if i == 0 { 1.0 } else { b.fade() } });
        }
        if let (Some((progress, empty)), true) = (self.reload, self.reload_w > 0.0) {
            o.base.push(Pass { seq: if empty { Seq::ReloadEmpty } else { Seq::Reload }, cycle: progress, weight: self.reload_w });
        }
        layers(&self.idle, false, p, &mut o.add);
        for v in [&self.fire, &self.jump, &self.land] {
            layers(v, true, p, &mut o.add);
        }
        o.add.push(Pass { seq: Seq::Wind, cycle: self.wind, weight: 1.0 });
        o
    }

    /// For `fp vm`.
    pub fn describe(&self) -> String {
        let o = self.out();
        let list = |v: &[Pass]| v.iter().map(|p| format!("{} {:.2}@{:.2}", p.seq.def_in(self.w).name, p.weight, p.cycle)).collect::<Vec<_>>().join(", ");
        format!(
            "base [{}] | add [{}] | ads {:.2} ({}) crouch {:.2} vel {:.0}",
            list(&o.base),
            list(&o.add),
            self.ads,
            if self.zoom_in { "in" } else { "out" },
            self.params.crouch,
            self.params.velocity
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn frames(seconds: f32) -> usize {
        (seconds / DT).round() as usize
    }

    fn run(g: &mut Graph, s: Signals, seconds: f32) {
        for _ in 0..frames(seconds) {
            g.step(DT, &s);
        }
    }

    fn moving(f: impl FnOnce(&mut Moving)) -> Signals {
        let mut m = Moving::default();
        f(&mut m);
        Signals { moving: Some(m), ..Default::default() }
    }

    fn top(g: &Graph) -> Pass {
        *g.out().base.last().unwrap()
    }

    fn add(g: &Graph, seq: Seq) -> Option<Pass> {
        g.out().add.iter().copied().find(|p| p.seq == seq)
    }

    fn near(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn durations_from_the_qc() {
        let p = Params::default();
        assert!(near(1.0 / Seq::Sprint.def().cps(p), 0.5556, 1e-3));
        assert!(near(1.0 / Seq::SprintRaise.def().cps(p), 0.300, 1e-3));
        assert!(near(1.0 / Seq::SprintSlide.def().cps(p), 0.4333, 1e-3));
        assert!(near(1.0 / Seq::Jump.def().cps(p), 1.000, 1e-3));
        assert!(near(1.0 / Seq::Land.def().cps(p), 0.600, 1e-3));
        assert!(near(1.0 / Seq::IdleToCrouch.def().cps(p), 0.9333, 1e-3));
        assert!(near(1.0 / Seq::Fire.def().cps(p), 0.900, 1e-3));
        let aiming = Params { ads: 1.0, ..p };
        assert!(near(1.0 / Seq::Fire.def().cps(aiming), 0.3333, 1e-3));
        assert!(near(1.0 / Seq::Jump.def().cps(aiming), 0.700, 1e-3));
        // the zeroanim sample adds nothing: the wind runs at the wind_effect sample's rate
        assert!(near(Seq::Wind.def().cps(Params { velocity: 500.0, ..p }), 2.0, 1e-4));
        assert_eq!(Seq::Wind.def().cps(Params { velocity: 100.0, ..p }), 0.0);
        let w = Seq::Fire.def().weights(Params { ads: 0.25, crouch: 0.5, ..p });
        assert!(near(w.iter().sum::<f32>(), 1.0, 1e-6) && near(w[3], 0.125, 1e-6));
    }

    #[test]
    fn rest_is_the_hip() {
        let mut g = Graph::new();
        run(&mut g, Signals::default(), 1.0);
        let o = g.out();
        assert_eq!(o.base, vec![Pass { seq: Seq::AdsOut, cycle: 1.0, weight: 1.0 }]);
        assert_eq!(o.add.len(), 2, "{o:?}");
        assert_eq!(add(&g, Seq::Idle).unwrap().weight, 1.0);
        // standing still: the wind layer is its zeroanim sample
        assert_eq!(add(&g, Seq::Wind).unwrap().cycle, 0.0);
    }

    /// A1: ads_in / ads_out follow the zoom (linear over 0.27 / 0.23 s in camera.rs); ads_in fades
    /// in over 0.1 s, ads_out over 0.2 s.
    #[test]
    fn aiming_follows_the_zoom() {
        let mut g = Graph::new();
        let mut ads = 0.0f32;
        let mut i = 0;
        while ads < 1.0 {
            ads = (ads + DT / 0.27).min(1.0);
            g.step(DT, &Signals { ads, ..Default::default() });
            let t = top(&g);
            assert_eq!(t.seq, Seq::AdsIn);
            assert_eq!(t.cycle, ads);
            // pushed on the first frame (age 0): in after 0.1 s
            if i == 6 {
                assert!(near(t.weight, 1.0, 1e-4), "{t:?}");
            }
            i += 1;
        }
        assert_eq!(i, 17);
        run(&mut g, Signals { ads: 1.0, ..Default::default() }, 0.2);
        assert_eq!(g.out().base, vec![Pass { seq: Seq::AdsIn, cycle: 1.0, weight: 1.0 }]);
        while ads > 0.0 {
            ads = (ads - DT / 0.23).max(0.0);
            g.step(DT, &Signals { ads, ..Default::default() });
            assert_eq!(top(&g).seq, Seq::AdsOut);
        }
        run(&mut g, Signals::default(), 0.2);
        assert_eq!(g.out().base, vec![Pass { seq: Seq::AdsOut, cycle: 1.0, weight: 1.0 }]);
        // the idle's aiming sample is the still one: ads_blend takes it over
        assert_eq!(Seq::Idle.def().weights(Params { ads: 1.0, ..Default::default() })[1], 1.0);
    }

    /// A1: crouched or sliding, the base sequences use their crouched sample by crouchFraction.
    #[test]
    fn crouch_fraction_blends_the_samples() {
        let mut g = Graph::new();
        g.step(DT, &moving(|m| m.duck_frac = 0.4));
        let o = g.out();
        assert_eq!(o.params.crouch, 0.4);
        let w = Seq::AdsOut.def().weights(o.params);
        assert!(near(w[0], 0.6, 1e-6) && near(w[1], 0.4, 1e-6), "{w:?}");
    }

    /// A2: sprint starts on its frame with a 0.2 s cross-fade and loops every 0.556 s; ending it
    /// plays sprintraise for 0.30 s, then the hip.
    #[test]
    fn sprint_and_its_raise() {
        let mut g = Graph::new();
        let sprinting = moving(|m| m.sprinting = true);
        g.step(DT, &sprinting);
        let t = top(&g);
        assert_eq!((t.seq, t.cycle, t.weight), (Seq::Sprint, 0.0, 0.0));
        run(&mut g, sprinting, 0.2);
        let t = top(&g);
        assert!(near(t.weight, 1.0, 1e-4) && near(t.cycle, 0.2 / 0.5556, 1e-3), "{t:?}");
        g.step(DT, &sprinting);
        assert_eq!(g.out().base.len(), 1, "the hip under it is dropped once sprint is in");
        // it loops at 36 / 20 cycles a second
        let c = top(&g).cycle;
        let n = frames(0.5556);
        run(&mut g, sprinting, 0.5556);
        assert!(near(top(&g).cycle, (c + n as f32 * DT * 1.8).rem_euclid(1.0), 1e-3), "{} {c}", top(&g).cycle);
        g.step(DT, &moving(|_| {}));
        let t = top(&g);
        assert_eq!((t.seq, t.cycle), (Seq::SprintRaise, 0.0));
        run(&mut g, moving(|_| {}), 16.0 * DT);
        assert!(near(top(&g).cycle, 16.0 * DT / 0.3, 1e-3) && top(&g).seq == Seq::SprintRaise, "{:?}", top(&g));
        run(&mut g, moving(|_| {}), 4.0 * DT);
        assert_eq!(top(&g).seq, Seq::AdsOut);
        run(&mut g, moving(|_| {}), 0.25);
        assert_eq!(g.out().base, vec![Pass { seq: Seq::AdsOut, cycle: 1.0, weight: 1.0 }]);
    }

    /// A2 / A4: a jump starts the jump layer on its frame; er-apex-move ends sprint the step after, which
    /// plays sprintraise. The jump lasts 1.0 s and fades out over its last 0.35 s.
    #[test]
    fn jump_out_of_sprint() {
        let mut g = Graph::new();
        run(&mut g, moving(|m| m.sprinting = true), 0.5);
        g.step(DT, &moving(|m| {
            m.sprinting = true;
            m.jumped = true;
        }));
        // on its frame, at cycle 0 and not yet faded in
        assert_eq!((g.jump.len(), g.jump[0].cycle), (1, 0.0));
        assert!(add(&g, Seq::Jump).is_none());
        assert_eq!(top(&g).seq, Seq::Sprint);
        g.step(DT, &moving(|_| {}));
        assert_eq!(top(&g).seq, Seq::SprintRaise);
        // fade-in 0.2 s (QC default), then full until 0.35 s before the end
        run(&mut g, moving(|_| {}), 0.5 - DT);
        let j = add(&g, Seq::Jump).unwrap();
        assert!(near(j.cycle, 0.5, 1e-3) && near(j.weight, 1.0, 1e-4), "{j:?}");
        run(&mut g, moving(|_| {}), 0.3);
        let j = add(&g, Seq::Jump).unwrap();
        assert!(near(j.weight, 0.2 / 0.35, 1e-2), "{j:?}");
        run(&mut g, moving(|_| {}), 0.2 + DT);
        assert!(add(&g, Seq::Jump).is_none());
    }

    /// A3: a slide out of sprint plays sprintslide for 0.433 s, then the base is the crouched hip;
    /// the idle node goes to the crouch loop without the duck transition (R5R T2).
    #[test]
    fn slide_out_of_sprint() {
        let mut g = Graph::new();
        run(&mut g, moving(|m| m.sprinting = true), 0.4);
        let sliding = moving(|m| {
            m.sliding = true;
            m.crouched = true;
            m.duck_frac = 1.0;
        });
        // er-apex-move's frame: slide boost, sprint-, duck
        g.step(DT, &moving(|m| {
            m.sliding = true;
            m.crouched = true;
            m.duck_started = true;
        }));
        let t = top(&g);
        assert_eq!((t.seq, t.cycle), (Seq::SprintSlide, 0.0));
        assert_eq!(g.idle.last().unwrap().seq, Seq::Crouch);
        run(&mut g, sliding, 0.41);
        assert_eq!(top(&g).seq, Seq::SprintSlide);
        run(&mut g, sliding, 0.0233 + 2.0 * DT);
        assert_eq!(top(&g).seq, Seq::AdsOut);
        run(&mut g, sliding, 0.25);
        let o = g.out();
        assert_eq!(o.base, vec![Pass { seq: Seq::AdsOut, cycle: 1.0, weight: 1.0 }]);
        assert_eq!(Seq::AdsOut.def().weights(o.params), [0.0, 1.0, 0.0, 0.0]);
    }

    /// A slide without sprint (no sprint pose before it) keeps the base: only the crouch blend.
    #[test]
    fn slide_without_sprint() {
        let mut g = Graph::new();
        g.step(DT, &moving(|m| m.sliding = true));
        assert_eq!(top(&g).seq, Seq::AdsOut);
    }

    /// A4: the land layer starts on its frame, 0.05 s in, 0.6 s long, 0.35 s out.
    #[test]
    fn land() {
        let mut g = Graph::new();
        g.step(DT, &moving(|m| m.landed = true));
        assert_eq!((g.land.len(), g.land[0].cycle), (1, 0.0));
        run(&mut g, moving(|_| {}), 0.05);
        let l = add(&g, Seq::Land).unwrap();
        assert!(near(l.weight, 1.0, 1e-4) && near(l.cycle, 0.05 / 0.6, 1e-3), "{l:?}");
        // 0.35 s before the end it starts fading out
        run(&mut g, moving(|_| {}), 0.2);
        assert!(near(add(&g, Seq::Land).unwrap().weight, 1.0, 1e-3));
        run(&mut g, moving(|_| {}), 10.0 * DT);
        let w = add(&g, Seq::Land).unwrap().weight;
        assert!(near(w, (0.35 - 10.0 * DT) / 0.35, 1e-2), "{w}");
        run(&mut g, moving(|_| {}), 0.2);
        assert!(add(&g, Seq::Land).is_none());
    }

    /// A4: ducking plays idle_to_crouch (0.933 s) in the idle node, then the crouch loop; standing
    /// up crouch_to_idle, then idle.
    #[test]
    fn duck_and_stand() {
        let mut g = Graph::new();
        let crouched = moving(|m| {
            m.crouched = true;
            m.duck_frac = 1.0;
        });
        g.step(DT, &moving(|m| m.duck_started = true));
        let t = *g.idle.last().unwrap();
        assert_eq!((t.seq, t.cycle), (Seq::IdleToCrouch, 0.0));
        run(&mut g, crouched, 0.9);
        assert_eq!(g.idle.last().unwrap().seq, Seq::IdleToCrouch);
        run(&mut g, crouched, 0.0333 + 2.0 * DT);
        assert_eq!(g.idle.last().unwrap().seq, Seq::Crouch);
        run(&mut g, crouched, 0.35);
        assert_eq!(g.idle.iter().map(|p| p.seq).collect::<Vec<_>>(), [Seq::Crouch]);
        g.step(DT, &moving(|m| m.unduck_started = true));
        assert_eq!(g.idle.last().unwrap().seq, Seq::CrouchToIdle);
        run(&mut g, moving(|_| {}), 1.3);
        assert_eq!(g.idle.iter().map(|p| p.seq).collect::<Vec<_>>(), [Seq::Idle]);
    }

    /// Each shot restarts fire, fading in over the one playing (0.05 s); aiming it is the short
    /// sample (0.33 s).
    #[test]
    fn fire_restarts() {
        let mut g = Graph::new();
        let shot = Signals { shot: true, ..Default::default() };
        g.step(DT, &shot);
        run(&mut g, Signals::default(), 0.074);
        g.step(DT, &shot);
        g.step(DT, &Signals::default());
        let passes: Vec<Pass> = g.out().add.into_iter().filter(|p| p.seq == Seq::Fire).collect();
        assert_eq!(passes.len(), 2);
        assert!(near(passes.iter().map(|p| p.weight).sum::<f32>(), 1.0, 1e-4), "{passes:?}");
        run(&mut g, Signals::default(), 0.06);
        assert_eq!(g.fire.len(), 1);
        let mut g = Graph::new();
        g.step(DT, &Signals { ads: 1.0, shot: true, ..Default::default() });
        run(&mut g, Signals { ads: 1.0, ..Default::default() }, 0.32);
        assert!(add(&g, Seq::Fire).is_some());
        run(&mut g, Signals { ads: 1.0, ..Default::default() }, 0.03);
        assert!(add(&g, Seq::Fire).is_none());
    }

    /// Zooming in or firing cuts sprint short (inferred); sprint shows again `SHOT_HOLD` after the
    /// last shot.
    #[test]
    fn aiming_and_firing_cut_sprint() {
        let mut g = Graph::new();
        run(&mut g, moving(|m| m.sprinting = true), 0.3);
        g.step(DT, &Signals { ads: 0.1, ..moving(|m| m.sprinting = true) });
        assert_eq!(top(&g).seq, Seq::AdsIn);
        let mut g = Graph::new();
        run(&mut g, moving(|m| m.sprinting = true), 0.3);
        g.step(DT, &Signals { shot: true, ..moving(|m| m.sprinting = true) });
        assert_eq!(top(&g).seq, Seq::AdsOut);
        run(&mut g, moving(|m| m.sprinting = true), SHOT_HOLD + 2.0 * DT);
        assert_eq!(top(&g).seq, Seq::Sprint);
    }

    #[test]
    fn reload_over_the_base() {
        let mut g = Graph::new();
        g.step(DT, &Signals { reload: Some((0.0, true)), ..Default::default() });
        run(&mut g, Signals { reload: Some((0.5, true)), ..Default::default() }, 0.2);
        let t = top(&g);
        assert_eq!((t.seq, t.cycle), (Seq::ReloadEmpty, 0.5));
        assert!(near(t.weight, 1.0, 1e-4));
        run(&mut g, Signals::default(), 0.1);
        assert!(near(top(&g).weight, 0.5, 1e-3) && top(&g).cycle == 0.5);
        run(&mut g, Signals::default(), 0.1 + DT);
        assert_eq!(top(&g).seq, Seq::AdsOut);
    }

    #[test]
    fn wind_by_speed() {
        let mut g = Graph::new();
        run(&mut g, moving(|m| m.speed = 500.0), 0.25);
        assert!(near(add(&g, Seq::Wind).unwrap().cycle, 0.5, 1e-3));
        assert_eq!(Seq::Wind.def().weights(Params { velocity: 350.0, ..Default::default() })[1], 0.5);
    }

    /// The lists stay short whatever happens.
    #[test]
    fn bounded() {
        let mut g = Graph::new();
        for i in 0..2000 {
            let s = Signals {
                ads: if i % 50 < 20 { 1.0 } else { 0.0 },
                shot: i % 5 == 0,
                reload: (i % 300 > 250).then_some((0.5, false)),
                moving: Some(Moving { sprinting: i % 40 < 30, sliding: i % 70 > 60, crouched: i % 70 > 60, jumped: i % 33 == 0, landed: i % 33 == 20, duck_started: i % 70 == 61, unduck_started: i % 70 == 0, ..Default::default() }),
            };
            g.step(DT, &s);
            assert!(g.base.len() <= 4 && g.idle.len() <= 4 && g.fire.len() <= 4, "{}", g.describe());
        }
    }
}
