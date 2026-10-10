//! Fuse's R-301 Carbine, M0 prototype (D-014): fire / aim / reload from the game's own action
//! inputs (so pad and mouse both work), a hitscan shot from the camera against enemy hit
//! cylinders, and every hit goes through the S6 damage bridge (`combat::shoot`).
//!
//! Weapon numbers from the local Apex export (`apex-data/fuse_data.json`, `mp_weapon_rspn101`):
//! damage_near/far/very_far_value 15, fire_rate 13.5, ammo_clip_size 21, reload_time 2.4,
//! reloadempty_time 3.2, damage_headshot_scale 1.3, damage_leg_scale 0.75,
//! spread_stand_hip 3, spread_stand_ads 0.
//!
//! Spread (HUD v3, user to-do U2 "the crosshair should match Apex"): the R-301's own spread by how
//! Fuse moves (movement controller, spike/kcc.rs) — hip: stand 3, moving 6.6, sprint 8.4, crouch
//! 2.4, air 8.4; aimed: 0, crouch 0, air 6 — reached at spread_moving_increase_rate 3 going up and
//! spread_moving_decay_rate 30 going down, plus a kick per shot (spread_kick_on_fire_*_hip 0.2, up
//! to spread_max_kick_* 2 / 1.5 / 3) that decays at spread_decay_rate 100 after spread_decay_delay
//! 0.25 s. 推断: the rates read as degrees per second; "moving" is above 0.5 m/s; sprinting hides
//! the crosshair (crosshair_tri's isSprinting input; T009 rule).
//!
//! M0 approximations, to be replaced: the projectile (projectile_launch_speed 29000, gravity on)
//! is a hitscan ray; walls stop it through the game's own ray cast (`CSPhysWorld::cast_ray`, the
//! map filter er-mario's ground probes use); head and leg zones
//! are fixed fractions of the target's hit height; the hip spread is read as a cone half-angle in
//! degrees (its unit in Apex is to be confirmed).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use eldenring::cs::{CSCamera, CSHavokMan, ChrIns, FieldInsHandle, WorldChrMan};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::FromStatic;
use glam::Vec3;

use crate::{log, paths, state};

/// A bullet gun of slot 1 (weapons.rs `Gun`): its numbers from the local Apex export.
pub struct Spec {
    pub gun: super::weapons::Gun,
    damage: f32,
    fire_rate: f32,
    pub clip: u32,
    reload: f32,
    reload_empty: f32,
    head_scale: f32,
    leg_scale: f32,
    /// spread_*_hip / spread_*_ads: stand, moving (stand_hip_run), sprint, crouch, air
    spread_hip: [f32; 5],
    spread_ads: [f32; 5],
    spread_up: f32,
    spread_down: f32,
    /// spread_kick_on_fire_*_hip and spread_max_kick_*_hip: stand, crouch, air (the ADS ones are 0)
    kick_hip: [f32; 3],
    kick_max_hip: [f32; 3],
    kick_delay: f32,
    kick_decay: f32,
    semi_auto: bool,
    /// the shot's view kick (chargerifle.rs's rule): pitch, yaw (base, random, inner exclude, soft,
    /// hard), roll (base, random min, random max, soft, hard); None: no kick
    view_kick: Option<[[f32; 5]; 3]>,
    /// a shot's sounds (every play action together)
    fire_sounds: &'static [&'static str],
    /// The shot's sounds at this times GUN_VOLUME.
    fire_volume: f32,
    /// a burst's sounds instead (`looping_sounds`): start, loop (its length), end
    burst: Option<(&'static [&'static str], &'static [&'static str], f32, &'static [&'static str])>,
    ads_in: &'static str,
    ads_out: &'static str,
    dry: &'static str,
    /// reload sounds at their frames (30 fps) in the reload of `reload_frames` frames, stretched to
    /// the reload's length; `true`: the empty reload's only
    reload_sounds: &'static [(&'static str, f32, bool)],
    reload_frames: f32,
    /// where the magazine fills (AE_WPN_FILLAMMO, fraction): tactical, empty
    fill: (f32, f32),
    /// a round back into the magazine every so many seconds while the gun is in hand (no reload:
    /// the Sentinel, the user's 2026-10-09 ask); None: reloads as Apex's
    regen: Option<f32>,
    /// its shots home on the enemy nearest the crosshair (homing.rs)
    homing: bool,
    /// every hit is a headshot (its damage, the headshot mark: the Sentinel, the user's 2026-10-09 ask)
    head_only: bool,
}

/// The R-301 (`apex-data/fuse_data.json`, `mp_weapon_rspn101`): 15 a round, fire_rate 13.5,
/// ammo_clip_size 21, reload_time 2.4, reloadempty_time 3.2, head x1.3, legs x0.75; its spread as
/// T009 read it; its sounds the 3p reload QC `mp_pt_medium_reload_rspn101` (88 frames) and T007's
/// set; the magazine full at ptpov_rspn101's AE_WPN_FILLAMMO (reload 38 of 66, reload_empty 53 of 86).
pub const R301: Spec = Spec {
    gun: super::weapons::Gun::R301,
    damage: 15.0,
    fire_rate: 13.5,
    clip: 21,
    reload: 2.4,
    reload_empty: 3.2,
    head_scale: 1.3,
    leg_scale: 0.75,
    spread_hip: [3.0, 6.6, 8.4, 2.4, 8.4],
    spread_ads: [0.0, 0.0, 0.0, 0.0, 6.0],
    spread_up: 3.0,
    spread_down: 30.0,
    kick_hip: [0.2, 0.2, 0.2],
    kick_max_hip: [2.0, 1.5, 3.0],
    kick_delay: 0.25,
    kick_decay: 100.0,
    semi_auto: false,
    view_kick: None,
    fire_sounds: &["fire_3p"],
    fire_volume: 1.0,
    burst: None,
    ads_in: "ads_in",
    ads_out: "ads_out",
    dry: "dry_fire",
    reload_sounds: &[
        ("reload_magout", 2.0, false),
        ("reload_maggrab", 19.0, false),
        ("reload_magin_re45", 28.0, false),
        ("reload_magin", 32.0, false),
        ("reload_boltback", 45.0, true),
        ("reload_boltforward", 49.0, true),
        ("reload_handrest", 61.0, false),
    ],
    reload_frames: 88.0,
    fill: (38.0 / 66.0, 53.0 / 86.0),
    regen: None,
    homing: false,
    head_only: false,
};

/// The R-99 (`apex-data/export/weapon/mp_weapon_r97.txt`): 12 a round, fire_rate 18, ammo_clip_size
/// 18, reload_time 1.8, reloadempty_time 2.45, head x1.25, legs x0.8; spread hip 2 / 3 / 5 / 1.6 /
/// 7, aimed 0.35 (crouched 0.25, air 5), towards it at 3 / 10.5, a shot's kick 0.18 up to 2 / 1.5 /
/// 3, decaying at 12 after 0.15 s; `looping_sounds` (`--set r99`); the view model's `reload_seq`
/// (73 frames: fill 57) and `reload_empty_seq` (94: fill 74).
pub const R99: Spec = Spec {
    gun: super::weapons::Gun::R99,
    damage: 12.0,
    fire_rate: 18.0,
    clip: 18,
    reload: 1.8,
    reload_empty: 2.45,
    head_scale: 1.25,
    leg_scale: 0.8,
    spread_hip: [2.0, 3.0, 5.0, 1.6, 7.0],
    spread_ads: [0.35, 0.35, 0.35, 0.25, 5.0],
    spread_up: 3.0,
    spread_down: 10.5,
    kick_hip: [0.18, 0.18, 0.18],
    kick_max_hip: [2.0, 1.5, 3.0],
    kick_delay: 0.15,
    kick_decay: 12.0,
    semi_auto: false,
    view_kick: None,
    fire_sounds: &[],
    fire_volume: 1.0,
    burst: Some((
        &["weapon_r97_fire_first_1p", "weapon_r97_fire_first_1p_layer1"],
        &["weapon_r97_fire_loop_1p", "weapon_r97_fire_loop_1p_layer1", "weapon_r97_fire_loop_1p_layer2"],
        3.5,
        &["weapon_r97_fire_last_1p", "weapon_r97_fire_last_1p_layer1"],
    )),
    ads_in: "weapon_r97_ads_in",
    ads_out: "weapon_r97_ads_out",
    dry: "assault_rifle_dryfire",
    reload_sounds: &[
        ("wpn_r97_reload_pullmag", 5.0, false),
        ("wpn_r97_reload_insertmag", 51.0, false),
        ("wpn_r97_reload_handgrab", 65.0, false),
        ("wpn_r97_reload_chargeback", 69.0, true),
        ("wpn_r97_reload_chargeforward", 74.0, true),
    ],
    reload_frames: 73.0,
    fill: (57.0 / 72.0, 74.0 / 93.0),
    regen: None,
    homing: false,
    head_only: false,
};

/// The Wingman (`apex-data/export/weapon/mp_weapon_wingman.txt`): `is_semi_auto` 1, 50 a round,
/// fire_rate 2.8, ammo_clip_size 5 plus 3 (the user, 2026-10-09), reload_time and reloadempty_time
/// 2.1, head x1.5, legs x0.9; its view kick (`viewkick_*`); its view model's `reload` (89 frames,
/// fill 62 of 88; `--set wingman`).
pub const WINGMAN: Spec = Spec {
    gun: super::weapons::Gun::Wingman,
    damage: 50.0,
    fire_rate: 2.8,
    clip: 8,
    reload: 2.1,
    reload_empty: 2.1,
    head_scale: 1.5,
    leg_scale: 0.9,
    spread_hip: [2.4, 3.0, 4.0, 1.5, 6.0],
    spread_ads: [0.0, 0.0, 0.0, 0.0, 2.0],
    spread_up: 5.5,
    spread_down: 12.0,
    kick_hip: [2.5, 2.0, 3.0],
    kick_max_hip: [5.5, 3.0, 4.5],
    kick_delay: 0.26,
    kick_decay: 11.0,
    semi_auto: true,
    view_kick: Some([[-4.4, 0.8, 0.0, 0.65, 0.35], [-0.6, 0.5, 0.0, 0.8, 0.4], [0.1, 0.1, 0.15, 0.4, 0.2]]),
    fire_sounds: &["weapon_wingman_fire_1p", "weapon_wingman_fire_1p_layer1", "weapon_wingman_fire_1p_layer2", "weapon_wingman_fire_1p_layer3"],
    fire_volume: 1.0,
    burst: None,
    ads_in: "weapon_wingman_ads_in",
    ads_out: "weapon_wingman_ads_out",
    dry: "pistol_dryfire",
    reload_sounds: &[
        ("wpn_wingman_reload_open", 6.0, false),
        ("wpn_wingman_reload_eject", 22.0, false),
        ("wpn_wingman_reload_insertmag", 53.0, false),
        ("wpn_wingman_reload_close", 68.0, false),
        ("wpn_wingman_reload_handgrab", 75.0, false),
    ],
    reload_frames: 88.0,
    fill: (62.0 / 88.0, 62.0 / 88.0),
    regen: None,
    homing: false,
    head_only: false,
};

/// The VK-47 Flatline (`apex-data/export/weapon/mp_weapon_vinson.txt`): 20 a round, fire_rate 10,
/// ammo_clip_size 19, reload_time 2.4, reloadempty_time 3.1, head x1.3, legs x0.75; spread hip 4.5 /
/// 9.9 / 12.6 / 3.6 / 12.6, aimed 0 (air 6), a shot's kick 0.2 up to 2 / 1.5 / 3, decaying at 100
/// after 0.25 s; `looping_sounds` (`--set flatline`); the view model's `reload` (70 frames: fill 45)
/// and `reload_empty` (94: fill 74), sounds at their QC frames.
pub const FLATLINE: Spec = Spec {
    gun: super::weapons::Gun::Flatline,
    damage: 20.0,
    fire_rate: 10.0,
    clip: 19,
    reload: 2.4,
    reload_empty: 3.1,
    head_scale: 1.3,
    leg_scale: 0.75,
    spread_hip: [4.5, 9.9, 12.6, 3.6, 12.6],
    spread_ads: [0.0, 0.0, 0.0, 0.0, 6.0],
    spread_up: 3.0,
    spread_down: 44.0,
    kick_hip: [0.2, 0.2, 0.2],
    kick_max_hip: [2.0, 1.5, 3.0],
    kick_delay: 0.25,
    kick_decay: 100.0,
    semi_auto: false,
    view_kick: None,
    fire_sounds: &[],
    fire_volume: 1.0,
    burst: Some((
        &["weapon_vinson_firstshot_1p", "weapon_vinson_firstshot_1p_layer1", "weapon_vinson_firstshot_1p_layer2"],
        &[
            "weapon_vinson_loop_1p",
            "weapon_vinson_loop_1p_layer1",
            "weapon_vinson_loop_1p_layer2",
            "weapon_vinson_loop_1p_layer3",
            "weapon_vinson_loop_1p_layer4",
            "weapon_vinson_loop_1p_layer5",
            "weapon_vinson_loop_1p_layer6",
        ],
        3.07,
        &["weapon_vinson_loopend_1p", "weapon_vinson_loopend_1p_layer1", "weapon_vinson_loopend_1p_layer2"],
    )),
    ads_in: "weapon_r101_ads_in",
    ads_out: "weapon_r101_ads_out",
    dry: "weapon_vinson_trigger",
    reload_sounds: &[
        ("weapon_vinson_reload_magout", 9.0, false),
        ("weapon_vinson_reload_magin", 41.0, false),
        ("weapon_vinson_reloadempty_charge", 66.0, true),
    ],
    reload_frames: 70.0,
    fill: (45.0 / 69.0, 74.0 / 93.0),
    regen: None,
    homing: false,
    head_only: false,
};

/// The Sentinel (`apex-data/export/weapon/mp_weapon_sentinel.txt`) as the user asked on 2026-10-09:
/// automatic, a shot every 0.8 s (the user's 2026-10-10 change from 3 a second), a magazine of 7
/// that takes a round back every 0.4 s (no reload), shots
/// that home (homing.rs); 70 a round, head x1.8, legs x0.9 and its spread from the retail settings;
/// its amped shot's sound (the shield-charged mod's `weapon_sentinel_fire_alt_1p` and its outdoor
/// sub-event's own Sentinel layers, the electric crack among them, `--set sentinel`).
pub const SENTINEL: Spec = Spec {
    gun: super::weapons::Gun::Sentinel,
    damage: 70.0,
    fire_rate: 1.0 / 0.8,
    clip: 7,
    reload: 3.0,
    reload_empty: 4.0,
    head_scale: 1.8,
    leg_scale: 0.9,
    spread_hip: [8.0, 10.0, 11.0, 6.0, 10.0],
    spread_ads: [0.0, 0.0, 0.0, 0.0, 6.0],
    spread_up: 4.0,
    spread_down: 4.0,
    kick_hip: [1.0, 1.0, 1.0],
    kick_max_hip: [12.0, 10.0, 12.0],
    kick_delay: 0.1,
    kick_decay: 4.0,
    semi_auto: false,
    view_kick: None,
    fire_sounds: &["weapon_sentinel_fire_alt_1p", "weapon_sentinel_fire_alt_1p_layer1", "weapon_sentinel_fire_alt_1p_layer2", "weapon_sentinel_fire_alt_1p_layer3", "weapon_sentinel_fire_alt_1p_layer4",
        "weapon_sentinel_fire_alt_1p_extbase", "weapon_sentinel_fire_alt_1p_extbase_layer1", "weapon_sentinel_fire_alt_1p_extbase_layer2", "weapon_sentinel_fire_alt_1p_extbase_layer3", "weapon_sentinel_fire_alt_1p_extbase_layer4"],
    // 30% quieter, then 30% again (the user, 2026-10-10): 0.7 x 0.7
    fire_volume: 0.49,
    burst: None,
    ads_in: "weapon_sentinel_ads_in",
    ads_out: "weapon_sentinel_ads_out",
    dry: "rifle_dryfire",
    reload_sounds: &[],
    reload_frames: 113.0,
    fill: (71.0 / 112.0, 71.0 / 112.0),
    regen: Some(0.4),
    homing: true,
    head_only: true,
};

/// A gun's numbers.
pub fn spec_of(gun: super::weapons::Gun) -> &'static Spec {
    match gun {
        super::weapons::Gun::R301 => &R301,
        super::weapons::Gun::R99 => &R99,
        super::weapons::Gun::Wingman => &WINGMAN,
        super::weapons::Gun::Flatline => &FLATLINE,
        super::weapons::Gun::Sentinel => &SENTINEL,
    }
}

/// Slot 1's gun's numbers now.
fn spec() -> &'static Spec {
    spec_of(super::weapons::primary_gun())
}
/// Above this (m/s) Fuse counts as moving (推断).
const MOVING: f32 = 0.5;
/// Hitscan range (m); Apex's damage_very_far_distance is 5000 units, far beyond any arena here.
const RANGE: f32 = 150.0;
/// Top / bottom share of a target's hit height counted as head / legs.
const HEAD_ZONE: f32 = 0.85;
const LEG_ZONE: f32 = 0.45;
/// er-mario's ray filter for map geometry (lib.rs RAY_FILTER)
const MAP_RAY: u32 = 0x08;

// ChrActions bits (fromsoftware-rs action_request.rs)
const R1: u64 = 1 << 0;
const R2: u64 = 1 << 1;
const L1: u64 = 1 << 2;
const L2: u64 = 1 << 3;
const USE_ITEM: u64 = 1 << 7;
const MAGIC: u64 = (1 << 19) | (1 << 20) | (1 << 33) | (1 << 34);
const GUARD: u64 = 1 << 24;
const KICKS: u64 = (1 << 26) | (1 << 27);
/// What the gun takes from the Tarnished: attacks, guard, spells, kicks and items (Fuse doesn't
/// drink flasks, D-003); movement, rolls, jumps, interact and lock-on stay the game's.
const TAKEN: u64 = R1 | R2 | L1 | L2 | USE_ITEM | MAGIC | GUARD | KICKS;

/// Buttons held this frame (written by `input_task`, read by `update`): fire, aim, reload press.
static HELD: AtomicU64 = AtomicU64::new(0);

/// Sound volume of the gun (times the mixer's master `volume`).
const GUN_VOLUME: f32 = 0.5;
/// A trigger pull not yet answered by a shot (semi-auto: one shot a pull; a pull during the
/// cooldown fires when it is over).
static PULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the reload going on is the empty one (the HUD's).
static RELOAD_IS_EMPTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The aim state last frame (ADS sounds on its changes).
static WAS_AIMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The trigger last frame: a new pull cancels the shield battery (S3 `+attack`, D-032).
static WAS_FIRING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Gun {
    /// slot 1's gun these numbers are of (the magazines of the others in `stash`)
    gun: super::weapons::Gun,
    stash: [u32; 5],
    /// Movement spread now (degrees) and the kick on top, seconds since the last shot.
    spread: f32,
    kick: f32,
    since_shot: f32,
    ammo: u32,
    cooldown: f32,
    /// seconds left of a reload
    reloading: Option<f32>,
    /// a reload asked for (the key, an empty trigger pull) while an ability had the hands
    reload_wanted: bool,
    /// a burst's loop sound going on (looping_sounds guns): seconds until it is played again
    burst: Option<f32>,
    shots: u32,
    hits: u32,
    rng: u32,
}

static GUN: Mutex<Gun> = Mutex::new(Gun {
    gun: super::weapons::Gun::Wingman,
    stash: [R301.clip, R99.clip, WINGMAN.clip, FLATLINE.clip, SENTINEL.clip],
    spread: 3.0,
    kick: 0.0,
    since_shot: 1.0,
    ammo: WINGMAN.clip,
    cooldown: 0.0,
    reloading: None,
    reload_wanted: false,
    burst: None,
    shots: 0,
    hits: 0,
    rng: 0x9e37_79b9,
});

impl Gun {
    /// Slot 1's gun changed (the wheel): the old one's magazine kept, the new one's taken out, a
    /// reload or a burst over.
    fn take(&mut self, gun: super::weapons::Gun) -> bool {
        if gun == self.gun {
            return false;
        }
        self.stash[self.gun.index()] = self.ammo;
        self.ammo = self.stash[gun.index()];
        let old = spec_of(self.gun);
        self.gun = gun;
        self.reloading = None;
        self.reload_wanted = false;
        self.cooldown = 0.0;
        if self.burst.take().is_some()
            && let Some((start, looped, _, _)) = old.burst
        {
            start.iter().chain(looped).for_each(|n| crate::audio::stop(n));
        }
        true
    }
}
/// What the HUD shows: magazine, reload progress (0..1), aiming, the last hit (when, headshot).
pub struct HudState {
    pub ammo: u32,
    pub clip: u32,
    pub reload: Option<f32>,
    pub aiming: bool,
    pub spread_deg: f32,
    /// Sprinting: Apex hides the crosshair.
    pub sprinting: bool,
    pub last_hit: Option<(std::time::Instant, bool)>,
    /// Shots fired so far (a change is a new shot) and whether the reload is the empty one.
    pub shots: u32,
    pub reload_empty: bool,
    /// U3: the weapon's slot (0 the R-301, 1 the Charge Rifle: weapons.rs) and its charge (0..1)
    pub slot: u8,
    pub charge: Option<f32>,
}
static LAST_HIT: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);

/// One bullet that hit: for the HUD's damage numbers and target health bar.
#[derive(Clone)]
pub struct Hit {
    pub at: std::time::Instant,
    /// Where it hit (world, the camera's space).
    pub pos: Vec3,
    /// Apex damage (before the bridge to ER health).
    pub damage: f32,
    pub head: bool,
    pub target: FieldInsHandle,
    /// U3: the slot of the weapon that fired it (the kill feed's icon)
    pub weapon: u8,
}

/// Where a shot hit a target: its top HEAD_ZONE, its bottom LEG_ZONE, or between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Head,
    Body,
    Legs,
}

impl Zone {
    fn name(self) -> &'static str {
        match self {
            Zone::Head => "head",
            Zone::Body => "body",
            Zone::Legs => "legs",
        }
    }
}

/// Hits of the last few seconds, oldest first.
static HITS: Mutex<std::collections::VecDeque<Hit>> = Mutex::new(std::collections::VecDeque::new());
/// How long hits are kept for the HUD.
const HIT_KEEP_SECONDS: f32 = 4.0;

/// Another weapon's hit (the frag grenade, spike/grenade.rs) for the HUD's damage numbers and the
/// kill credit (stats.rs).
pub fn record_hit(hit: Hit) {
    let mut h = HITS.lock().unwrap_or_else(|e| e.into_inner());
    h.push_back(hit);
    if h.len() > 64 {
        h.pop_front();
    }
}

/// The gun's buttons as `input_task` last saw them: the trigger held, aiming (read-only; the frag
/// grenade's pin and throw, spike/grenade.rs).
pub fn buttons() -> (bool, bool) {
    let held = HELD.load(Ordering::Relaxed);
    (held & 1 != 0, held & 2 != 0)
}

/// Hits younger than HIT_KEEP_SECONDS, oldest first.
pub fn hits() -> Vec<Hit> {
    let mut h = HITS.lock().unwrap_or_else(|e| e.into_inner());
    while h.front().is_some_and(|x| x.at.elapsed().as_secs_f32() > HIT_KEEP_SECONDS) {
        h.pop_front();
    }
    h.iter().cloned().collect()
}
static RELOAD_TOTAL: Mutex<f32> = Mutex::new(2.0);

pub fn hud() -> Option<HudState> {
    if !enabled() || !state::in_world() {
        return None;
    }
    let mut g = GUN.lock().unwrap_or_else(|e| e.into_inner());
    g.take(super::weapons::primary_gun());
    let total = *RELOAD_TOTAL.lock().unwrap_or_else(|e| e.into_inner());
    // (aiming waits for a drawn weapon: weapons.rs)
    let aiming = super::weapons::aiming(aim_held());
    Some(HudState {
        ammo: g.ammo,
        clip: spec_of(g.gun).clip,
        reload: g.reloading.map(|left| (1.0 - left / total).clamp(0.0, 1.0)),
        aiming,
        spread_deg: g.spread + g.kick,
        sprinting: super::kcc::locomotion().is_some_and(|l| l.sprinting),
        last_hit: last_hit(),
        shots: g.shots,
        reload_empty: g.reloading.is_some() && RELOAD_IS_EMPTY.load(Ordering::Relaxed),
        slot: 0,
        charge: None,
    })
}

/// Whether the aim button is held (either weapon's).
pub fn aim_held() -> bool {
    HELD.load(Ordering::Relaxed) & 2 != 0
}

/// The last hit of either weapon: when, whether on the head (the hit marker).
pub fn last_hit() -> Option<(std::time::Instant, bool)> {
    *LAST_HIT.lock().unwrap_or_else(|e| e.into_inner())
}

/// U3: a switch puts the R-301 away (weapons.rs): a reload going on is off with its queued sounds
/// (`interrupt_reload`); the magazine stays as it is (full if the reload's AE_WPN_FILLAMMO had
/// come).
pub fn holster_check() {
    if !matches!(super::weapons::phase(), super::weapons::Phase::Holstering { slot: super::weapons::Slot::R301, .. }) {
        return;
    }
    interrupt_reload();
}

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| paths::flag("gun")) && crate::mode::apex()
}

/// ChrIns_PreBehaviorSafe, right after the game turned the pad / mouse into character actions:
/// note what the gun's buttons do and take them away from the Tarnished.
pub fn input_task() {
    if !enabled() || !state::in_world() {
        return;
    }
    let Some(player) = (unsafe { WorldChrMan::instance_mut() }).ok().and_then(|w| w.main_player.as_mut()) else { return };
    let req: &mut eldenring::cs::CSChrActionRequestModule = &mut player.chr_ins.modules.action_request;
    let bits = |a: &mut eldenring::cs::ChrActions| unsafe { &mut *(a as *mut _ as *mut u64) };
    let held = *bits(&mut req.action_requests);
    let pressed = *bits(&mut req.new_action_presses);
    let mut out = 0;
    if held & (R1 | R2) != 0 {
        out |= 1;
    }
    if held & (L1 | L2 | GUARD) != 0 {
        out |= 2;
    }
    if pressed & USE_ITEM != 0 {
        out |= 4;
    }
    // reload presses are edges: keep one until `update` has seen it
    HELD.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| Some(out | (old & 4))).ok();
    for a in [&mut req.action_requests, &mut req.new_action_presses, &mut req.queued_action_inputs, &mut req.cancel_ready_actions] {
        *bits(a) &= !TAKEN;
    }
    // (not `disabled_action_inputs`: with those bits set the game stops reporting the buttons in
    // `action_requests` at all, and the trigger went dead; Fuse holds fists instead, armor.rs)
}

/// Dev channel `fire <seconds>`: holds the trigger without a pad (the synthetic pad's RB works
/// too; this one skips the game's input mapping).
pub fn hold_fire(seconds: f32) -> String {
    *DEV_FIRE.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now() + std::time::Duration::from_secs_f32(seconds));
    format!("trigger held for {seconds} s")
}
static DEV_FIRE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Stops a reload in progress (the shield battery takes the hands: S3 switches weapons, which ends
/// the reload); its sounds go with it, the magazine stays as it was.
pub fn interrupt_reload() {
    let mut g = GUN.lock().unwrap_or_else(|e| e.into_inner());
    if g.reloading.take().is_some() {
        g.reload_wanted = false;
        drop(g);
        for s in [&R301, &R99, &WINGMAN, &FLATLINE, &SENTINEL] {
            for (name, _, _) in s.reload_sounds {
                crate::audio::stop(name);
            }
        }
        log("gun: reload interrupted");
    }
}

/// Every frame (ChrIns_PostPhysics): fire rate, magazine, reload, and the shots themselves.
pub fn update(dt: f32) {
    if !enabled() || !state::in_world() {
        return;
    }
    let held = HELD.fetch_and(!4, Ordering::Relaxed);
    let dev = DEV_FIRE.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| std::time::Instant::now() < t);
    let (fire, aim, reload) = (held & 1 != 0 || dev, held & 2 != 0, held & 4 != 0);
    // the shield battery (battery.rs): a new trigger pull cancels it (S3 `AttemptCancelHeal` on
    // `+attack`); while it is out the gun neither fires nor reloads
    let was_firing = WAS_FIRING.swap(fire, Ordering::Relaxed);
    let pulled = fire && !was_firing;
    if pulled {
        PULL.store(true, Ordering::Relaxed);
    }
    if pulled && super::battery::busy() {
        super::battery::cancel("fire");
    }
    // U3: the weapon slots (weapons.rs) switch on keys 1 / 2 and run the Charge Rifle while it is
    // out; the R-301 sits out while it is put away, away or coming out
    if super::weapons::update(dt, super::weapons::Trigger { fire, aim, reload }) {
        WAS_AIMING.store(false, Ordering::Relaxed);
        PULL.store(false, Ordering::Relaxed);
        let mut g = GUN.lock().unwrap_or_else(|e| e.into_inner());
        g.cooldown = g.cooldown.max(0.0);
        return;
    }
    let sp = spec();
    let mut g = GUN.lock().unwrap_or_else(|e| e.into_inner());
    if g.take(sp.gun) {
        log(format!("gun: {} in hand, {} in the magazine", sp.gun.name(), g.ammo));
    }
    // a round back every `regen` seconds (the Sentinel): no reload for it
    if let Some(every) = sp.regen {
        let mut t = REGEN.lock().unwrap_or_else(|e| e.into_inner());
        if g.ammo >= sp.clip {
            *t = 0.0;
        } else {
            *t += dt;
            while *t >= every && g.ammo < sp.clip {
                *t -= every;
                g.ammo += 1;
            }
        }
    }
    if WAS_AIMING.swap(aim, Ordering::Relaxed) != aim {
        crate::audio::play(if aim { sp.ads_in } else { sp.ads_out }, GUN_VOLUME);
    }
    let (target, kick_row) = spread_target(sp, aim);
    let step = if g.spread < target { sp.spread_up } else { sp.spread_down } * dt;
    g.spread = if g.spread < target { (g.spread + step).min(target) } else { (g.spread - step).max(target) };
    g.since_shot += dt;
    if g.since_shot > sp.kick_delay {
        g.kick = (g.kick - sp.kick_decay * dt).max(0.0);
    }
    g.cooldown = (g.cooldown - dt).max(-1.0 / sp.fire_rate);
    // a burst's sounds (looping_sounds): the loop again while the trigger holds, the end when it lets go
    let firing = fire && g.ammo > 0 && g.reloading.is_none() && !super::pov::gun_away() && !super::battery::busy() && !super::grenade::gun_away();
    if let (Some(left), Some((start, looped, every, end))) = (g.burst, sp.burst) {
        if firing {
            let left = left - dt;
            if left <= 0.0 {
                looped.iter().for_each(|n| crate::audio::play(n, GUN_VOLUME));
            }
            g.burst = Some(if left <= 0.0 { left + every } else { left });
        } else {
            start.iter().chain(looped).for_each(|n| crate::audio::stop(n));
            end.iter().for_each(|n| crate::audio::play(n, GUN_VOLUME));
            g.burst = None;
        }
    }
    if let Some(left) = g.reloading {
        let left = left - dt;
        let total = *RELOAD_TOTAL.lock().unwrap_or_else(|e| e.into_inner());
        // the magazine is full from the reload's AE_WPN_FILLAMMO on (the HUD's number jumps there,
        // S3: gun-motion spec 6.3); he can't fire before the reload time is up
        let fill = if RELOAD_IS_EMPTY.load(Ordering::Relaxed) { sp.fill.1 } else { sp.fill.0 };
        if g.ammo < sp.clip && 1.0 - left / total >= fill {
            g.ammo = sp.clip;
            log(format!("gun: magazine filled, {} rounds", sp.clip));
        }
        if left <= 0.0 {
            g.reloading = None;
            g.ammo = sp.clip;
            log(format!("gun: reloaded, {} rounds", sp.clip));
        } else {
            g.reloading = Some(left);
        }
        PULL.store(false, Ordering::Relaxed);
        return;
    }
    if reload && g.ammo < sp.clip && sp.regen.is_none() {
        g.reload_wanted = true;
    }
    // the gun is away for an ability (switching to one hand for the stim, the pad's toss:
    // pov/ability.rs); while the left hand holds the injector it fires one-handed but does not
    // reload: a reload asked for then throws the injector now (the stim goes on) and follows it
    // (or for the frag grenade in hand: G to the gun's pull-out after it, spike/grenade.rs)
    if super::pov::gun_away() || super::battery::busy() || super::grenade::gun_away() {
        g.cooldown = g.cooldown.max(0.0);
        PULL.store(false, Ordering::Relaxed);
        return;
    }
    let left_busy = super::pov::left_busy();
    if g.reload_wanted && g.ammo < sp.clip {
        if !left_busy {
            start_reload(sp, &mut g);
            return;
        }
        super::octane::throw_injector();
    }
    // semi-auto: a pull fires once, as soon as the fire rate allows; automatic: while held
    let wants = if sp.semi_auto { PULL.load(Ordering::Relaxed) } else { fire };
    if !wants {
        g.cooldown = g.cooldown.max(0.0);
        return;
    }
    while g.cooldown <= 0.0 {
        PULL.store(false, Ordering::Relaxed);
        if g.ammo == 0 {
            crate::audio::play(sp.dry, GUN_VOLUME);
            if sp.regen.is_some() {
                // the next round comes by itself: one dry click per pull's worth of shots
                g.cooldown += 1.0 / sp.fire_rate;
                return;
            }
            if left_busy {
                // once per trigger pull's worth of shots, until the injector is gone (thrown
                // next frame for the reload)
                g.cooldown += 0.5;
                g.reload_wanted = true;
            } else {
                start_reload(sp, &mut g);
            }
            return;
        }
        g.ammo -= 1;
        sp.fire_sounds.iter().for_each(|n| crate::audio::play(n, GUN_VOLUME * sp.fire_volume));
        if let (None, Some((start, looped, every, _))) = (g.burst, sp.burst) {
            start.iter().chain(looped).for_each(|n| crate::audio::play(n, GUN_VOLUME));
            g.burst = Some(every);
        }
        g.cooldown += 1.0 / sp.fire_rate;
        g.shots += 1;
        let spread = g.spread + g.kick;
        if !aim {
            g.kick = (g.kick + sp.kick_hip[kick_row]).min(sp.kick_max_hip[kick_row]);
        }
        g.since_shot = 0.0;
        let r = (next(&mut g.rng), next(&mut g.rng));
        // the shot goes along the camera with the kick the view does not show, then kicks it
        let out = shoot(sp, spread, r);
        if let Some(k) = sp.view_kick {
            let u = [next(&mut g.rng), next(&mut g.rng), next(&mut g.rng), next(&mut g.rng), next(&mut g.rng), next(&mut g.rng)];
            let (soft, hard) = view_kick(k, u);
            crate::viewfx::weapon_kick(soft, hard);
        }
        if out.is_some() {
            g.hits += 1;
        }
        if g.shots <= 30 || g.shots % 50 == 0 {
            log(format!(
                "gun: {} shot {} ({}), ammo {}, {}",
                sp.gun.name(),
                g.shots,
                if aim { "aimed" } else { "hip" },
                g.ammo,
                out.unwrap_or_else(|| "miss".into())
            ));
        }
        if sp.semi_auto {
            break;
        }
    }
}
/// The spread Fuse's movement asks for (degrees) and the kick row (stand, crouch, air).
fn spread_target(sp: &Spec, aim: bool) -> (f32, usize) {
    let table = if aim { sp.spread_ads } else { sp.spread_hip };
    match super::kcc::locomotion() {
        Some(l) if !l.grounded => (table[4], 2),
        Some(l) if l.crouched => (table[3], 1),
        Some(l) if l.sprinting => (table[2], 0),
        Some(l) if l.speed > MOVING => (table[1], 0),
        _ => (table[0], 0),
    }
}

/// One shot's view kick (chargerifle.rs `view_kick`'s rule, the gun's numbers): the soft part
/// (into the spring's velocity) and the hard part (into its angle), pitch / yaw / roll in degrees;
/// `u` uniforms in 0..1 (pitch, its sign, yaw, its sign, roll, the roll's sign).
fn view_kick(k: [[f32; 5]; 3], u: [f32; 6]) -> (Vec3, Vec3) {
    let [kp, ky, kr] = k;
    let sign = |s: f32| if s < 0.5 { 1.0 } else { -1.0 };
    let pick = |k: [f32; 5], r: f32, s: f32| {
        let (half, inner) = (k[1] * 0.5, k[2] * 0.5);
        k[0] + sign(s) * (inner + (half - inner) * r)
    };
    let pitch = pick(kp, u[0], u[1]);
    let yaw = pick(ky, u[2], u[3]);
    let roll = kr[0] + sign(u[5]) * (kr[1] + (kr[2] - kr[1]) * u[4]);
    let soft = Vec3::new(pitch * kp[3], yaw * ky[3], roll * kr[3]);
    let hard = Vec3::new(pitch * kp[4], yaw * ky[4], roll * kr[4]);
    (soft, hard)
}

fn start_reload(sp: &Spec, g: &mut Gun) {
    let empty = g.ammo == 0;
    RELOAD_IS_EMPTY.store(empty, Ordering::Relaxed);
    let t = if empty { sp.reload_empty } else { sp.reload };
    g.reloading = Some(t);
    g.reload_wanted = false;
    let stretch = t / (sp.reload_frames / 30.0);
    for &(name, frame, bolt) in sp.reload_sounds {
        if empty || !bolt {
            crate::audio::play_in(name, GUN_VOLUME, frame / 30.0 * stretch);
        }
    }
    *RELOAD_TOTAL.lock().unwrap_or_else(|e| e.into_inner()) = t;
    log(format!("gun: {} reloading ({t} s, {} left in the magazine)", sp.gun.name(), g.ammo));
}
/// Uniform 0..1 from a xorshift.
fn next(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// One shot of slot 1's gun (its damage, head and legs scales, ini `gun_damage_mult`), along the
/// camera with the part of the view kick the view does not show (viewfx.rs).
fn shoot(sp: &Spec, spread_deg: f32, r: (f32, f32)) -> Option<String> {
    let (damage, head, legs) = (sp.damage, sp.head_scale, sp.leg_scale);
    // a homing gun (the Sentinel): a round that flies to an enemy in the cone ahead (homing.rs),
    // else the shot goes as any other
    if sp.homing {
        super::homing::shot_fired();
    }
    if sp.homing
        && let Some(line) = super::homing::fire(sp.head_only, |zone| {
            let scale = match zone {
                Zone::Head => head,
                Zone::Legs => legs,
                Zone::Body => 1.0,
            };
            damage * (scale * damage_mult())
        })
    {
        return Some(line);
    }
    HEAD_ONLY.store(sp.head_only, Ordering::Relaxed);
    let out = fire_ray(spread_deg, r, crate::viewfx::weapon_aim_offset(), 0, |zone, _| {
        let scale = match zone {
            Zone::Head => head,
            Zone::Legs => legs,
            Zone::Body => 1.0,
        };
        // ini `gun_damage_mult` (read fresh; the user's 3 on 2026-10-05)
        damage * (scale * damage_mult())
    });
    HEAD_ONLY.store(false, Ordering::Relaxed);
    out
}

/// This shot's hits are headshots (a `head_only` gun: set by `shoot`, read by `fire_ray_ex`).
static HEAD_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The hit sound: the enemies it has played for (their handles), how loud.
static HIT_SOUND: Mutex<std::collections::BTreeSet<u64>> = Mutex::new(std::collections::BTreeSet::new());
const HIT_SOUND_VOLUME: f32 = 0.6;
/// Seconds towards the next round back (a `regen` gun).
static REGEN: Mutex<f32> = Mutex::new(0.0);

/// ini `gun_damage_mult`, read fresh (both weapons).
pub(super) fn damage_mult() -> f32 {
    paths::number::<f32>("gun_damage_mult").map_or(1.0, |m| m.max(0.0))
}

/// One shot along the camera's forward direction (inside the spread cone); returns what it hit.
/// `offset` turns that direction first (pitch down, yaw left, degrees: the part of a weapon's view
/// kick the view does not show, viewfx.rs); `weapon` the slot it came from; `damage` the Apex damage
/// for the zone hit at the distance (m).
pub(super) fn fire_ray(spread_deg: f32, r: (f32, f32), offset: Vec3, weapon: u8, damage: impl Fn(Zone, f32) -> f32) -> Option<String> {
    fire_ray_ex(spread_deg, r, offset, weapon, RANGE, true, damage).and_then(|o| o.line)
}

/// What a ray along the camera ended on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RayEnd {
    /// a character (hurt when the ray deals damage)
    Target,
    /// the map, or a character's hit behind the map
    Map,
    /// nothing within the range
    Nothing,
}

/// A ray along the camera: the hit's log line (None: no character hit, or not dealing), where it
/// ends (world), and what it ended on.
pub(super) struct RayOut {
    pub line: Option<String>,
    pub end: Vec3,
    pub on: RayEnd,
}

/// `fire_ray` with its end (the Charge Rifle's beam, U3): out to `range` metres; `deal` false only
/// finds the end (the beam drawn between pulses).
pub(super) fn fire_ray_ex(spread_deg: f32, r: (f32, f32), offset: Vec3, weapon: u8, range: f32, deal: bool, damage: impl Fn(Zone, f32) -> f32) -> Option<RayOut> {
    // the camera the player sees: ours over the shoulder when it's on, else the game's
    let (origin, fwd, right, up) = match crate::camera::view() {
        Some(v) => v,
        None => {
            let cam = unsafe { CSCamera::instance() }.ok()?;
            let m = &cam.pers_cam_1.matrix;
            (Vec3::new(m.3.0, m.3.1, m.3.2), Vec3::new(m.2.0, m.2.1, m.2.2), Vec3::new(m.0.0, m.0.1, m.0.2), Vec3::new(m.1.0, m.1.1, m.1.2))
        }
    };
    let (fwd, right, up) = if offset == Vec3::ZERO {
        (fwd, right, up)
    } else {
        let [r, u, f] = crate::viewfx::turned(right, up, fwd, offset);
        (f, r, u)
    };
    let fwd = fwd.normalize_or_zero();
    if fwd == Vec3::ZERO {
        return None;
    }
    // uniform in the cone's disc: angle a around the axis, radius sqrt(r) of the half-angle
    let (a, rad) = (r.0 * std::f32::consts::TAU, r.1.sqrt() * spread_deg.to_radians().tan());
    let dir = (fwd + right * (rad * a.cos()) + up * (rad * a.sin())).normalize();
    // `fp trace`: the shot goes along the render camera (the crosshair), the view punch included
    // (D-023), not along the eye's line
    if crate::spike::pov::tracing() {
        if let Some((_, eye, _, _)) = crate::camera::eye_view() {
            log(format!("gun: shot {:.3}° from the crosshair (spread {spread_deg:.2}°), {:.3}° from the eye's line", dir.angle_between(fwd).to_degrees(), dir.angle_between(eye.normalize_or_zero()).to_degrees()));
        }
    }
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = wcm.main_player.as_ref()?;
    let p = me.chr_ins.modules.physics.position;
    let me_pos = Vec3::new(p.0, p.1, p.2);
    // nothing between the camera and Fuse counts (the over-the-shoulder camera sits behind him)
    let near = (me_pos - origin).dot(dir).max(0.0);
    let mut best: Option<(f32, FieldInsHandle, f32, u32)> = None;
    for c in wcm.chr_sets.iter().flatten().flat_map(|s| s.characters()) {
        let c: &ChrIns = c;
        if c.modules.data.hp <= 0 {
            continue;
        }
        if !super::body::is_enemy_team(c.team_type) {
            // which characters the shots pass because of their team: once per character kind
            if deal && c.team_type != 1 {
                let (h, rad) = super::body::cylinder(c);
                let q = c.modules.physics.position;
                if ray_cylinder(origin, dir, Vec3::new(q.0, q.1, q.2), h, rad).is_some_and(|t| t < range) {
                    note_skipped(c.npc_param_id as u32, c.team_type);
                }
            }
            continue;
        }
        let (h, rad) = super::body::cylinder(c);
        let q = c.modules.physics.position;
        let base = Vec3::new(q.0, q.1, q.2);
        if let Some(t) = ray_cylinder(origin, dir, base, h, rad) {
            if t > near && t < range && best.as_ref().is_none_or(|b| t < b.0) {
                let y = ((origin.y + dir.y * t) - base.y) / h;
                best = Some((t, c.field_ins_handle.clone(), y, c.npc_param_id as u32));
            }
        }
    }
    // a wall between Fuse and the target stops the shot (the ray starts at Fuse, not the camera,
    // so a wall behind him that the camera is pushed against doesn't count)
    let start = origin + dir * near;
    let reach = best.as_ref().map_or(range, |b| b.0) - near;
    let wall = unsafe { CSHavokMan::instance() }.ok().and_then(|h| {
        h.phys_world.cast_ray(MAP_RAY, &HavokPosition(start.x, start.y, start.z, 0.0), PositionDelta(dir.x * reach, dir.y * reach, dir.z * reach), me)
    });
    let wall = wall.map(|w| Vec3::new(w.0, w.1, w.2)).filter(|w| (*w - start).length() < reach - if best.is_some() { 0.3 } else { 0.0 });
    let Some((t, handle, y, npc)) = best else {
        return Some(match wall {
            Some(w) => RayOut { line: None, end: w, on: RayEnd::Map },
            None => RayOut { line: None, end: origin + dir * range, on: RayEnd::Nothing },
        });
    };
    if let Some(w) = wall {
        if deal {
            log(format!("gun: shot blocked by the map {:.1} m out (target npc {npc} at {t:.1} m)", (w - start).length()));
        }
        return Some(RayOut { line: None, end: w, on: RayEnd::Map });
    }
    let end = origin + dir * t;
    if !deal {
        return Some(RayOut { line: None, end, on: RayEnd::Target });
    }
    let zone = if HEAD_ONLY.load(Ordering::Relaxed) { Zone::Head } else { zone_at(y) };
    let amount = damage(zone, t);
    let line = apply_hit(handle, amount, zone, end, weapon);
    Some(RayOut { line: Some(format!("hit npc {npc} {} at {t:.1} m for {amount:.1}: {line}", zone.name())), end, on: RayEnd::Target })
}

/// Where a ray from `start` along `delta` (world metres) first meets the map (the game's own ray
/// cast, as the shots' walls; the grapple's hook).
pub(super) fn map_ray(start: Vec3, delta: Vec3) -> Option<Vec3> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = wcm.main_player.as_ref()?;
    let h = unsafe { CSHavokMan::instance() }.ok()?;
    let w = h.phys_world.cast_ray(MAP_RAY, &HavokPosition(start.x, start.y, start.z, 0.0), PositionDelta(delta.x, delta.y, delta.z), me)?;
    Some(Vec3::new(w.0, w.1, w.2))
}

/// The zone at a height on a target (0 its feet .. 1 its top).
pub(super) fn zone_at(y: f32) -> Zone {
    if y >= HEAD_ZONE { Zone::Head } else if y < LEG_ZONE { Zone::Legs } else { Zone::Body }
}

/// A hit's damage and its HUD record (hit marker, damage number, kill feed): the ray's or a homing
/// round's (homing.rs). What the damage bridge said.
pub(super) fn apply_hit(handle: FieldInsHandle, amount: f32, zone: Zone, end: Vec3, weapon: u8) -> String {
    let res = super::combat::shoot(handle.clone(), amount);
    // the hit's sound: Apex's armour break as the attacker hears it (the user's 2026-10-09 ask;
    // `--set hits`), once per enemy: on the first hit that hurts it, as Apex's armour breaks once
    if amount > 0.0 {
        let id = unsafe { std::mem::transmute_copy::<FieldInsHandle, u64>(&handle) };
        let mut broken = HIT_SOUND.lock().unwrap_or_else(|e| e.into_inner());
        if broken.insert(id) {
            crate::audio::play("humanshield_break_1p_vs_3p", HIT_SOUND_VOLUME);
        }
    }
    super::stats::dealt(amount);
    let now = std::time::Instant::now();
    *LAST_HIT.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, zone == Zone::Head));
    {
        let mut h = HITS.lock().unwrap_or_else(|e| e.into_inner());
        h.push_back(Hit { at: now, pos: end, damage: amount, head: zone == Zone::Head, target: handle, weapon });
        if h.len() > 64 {
            h.pop_front();
        }
    }
    res
}

/// Logs a character the shots passed through because of its team (once per npc and team).
fn note_skipped(npc: u32, team: u8) {
    static SEEN: Mutex<Vec<(u32, u8)>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.len() < 256 && !seen.contains(&(npc, team)) {
        seen.push((npc, team));
        log(format!("gun: shot passed npc {npc}: team {team} is not hit (ini friendly_teams / enemy_teams): no damage"));
    }
}

/// The nearest t >= 0 where the ray meets an upright cylinder (base centre, height, radius).
fn ray_cylinder(o: Vec3, d: Vec3, base: Vec3, h: f32, r: f32) -> Option<f32> {
    let (ox, oz) = (o.x - base.x, o.z - base.z);
    let a = d.x * d.x + d.z * d.z;
    let inside = |t: f32| {
        let y = o.y + d.y * t - base.y;
        (0.0..=h).contains(&y)
    };
    let mut hits = Vec::with_capacity(4);
    if a > 1e-8 {
        let b = 2.0 * (ox * d.x + oz * d.z);
        let c = ox * ox + oz * oz - r * r;
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let s = disc.sqrt();
            for t in [(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)] {
                if t >= 0.0 && inside(t) {
                    hits.push(t);
                }
            }
        }
    }
    // the caps (shots from above or below)
    if d.y.abs() > 1e-6 {
        for cap in [base.y, base.y + h] {
            let t = (cap - o.y) / d.y;
            let (x, z) = (ox + d.x * t, oz + d.z * t);
            if t >= 0.0 && x * x + z * z <= r * r {
                hits.push(t);
            }
        }
    }
    hits.into_iter().reduce(f32::min)
}

pub fn status() -> String {
    let g = GUN.lock().unwrap_or_else(|e| e.into_inner());
    format!(
        "gun {}: {} ammo {}/{}, reloading {:?}, shots {}, hits {}",
        if enabled() { "on" } else { "off (gun = 1 in the ini)" },
        g.gun.name(),
        g.ammo,
        spec_of(g.gun).clip,
        g.reloading,
        g.shots,
        g.hits
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cylinder_side_cap_and_miss() {
        let base = Vec3::new(0.0, 0.0, 10.0);
        // straight at the side, 1 m up: enters at z = 9.5
        let t = ray_cylinder(Vec3::new(0.0, 1.0, 0.0), Vec3::Z, base, 2.0, 0.5).unwrap();
        assert!((t - 9.5).abs() < 1e-4);
        // above it: miss
        assert!(ray_cylinder(Vec3::new(0.0, 3.0, 0.0), Vec3::Z, base, 2.0, 0.5).is_none());
        // from straight above: the top cap
        let t = ray_cylinder(Vec3::new(0.0, 5.0, 10.0), -Vec3::Y, base, 2.0, 0.5).unwrap();
        assert!((t - 3.0).abs() < 1e-4);
        // behind the origin: no hit
        assert!(ray_cylinder(Vec3::new(0.0, 1.0, 20.0), Vec3::Z, base, 2.0, 0.5).is_none());
    }
}
