//! U3: the Charge Rifle in weapon slot 2 (weapons.rs), the old beam version (user 2026-10-05 23:00:
//! "use the old Charge Rifle's firing logic"): `mp_weapon_defender_sustained`, the sustained-discharge
//! Charge Rifle, by its weapon settings (R5Reloaded `platform/scripts/weapons/mp_weapon_defender_sustained.txt`
//! with its #bases `_base_sniper.txt`, `_base_sniper_optics.txt`, `_base_stocks_sniper.txt`; retail's
//! `apex-data/export/weapon/mp_weapon_defender_sustained.txt` has the same numbers), its script
//! (`vscripts/weapons/mp_weapon_defender_sustained.nut`) and what the S3 engine does with them, read
//! in the private copy of R5R's `r5apex_ds.exe` IDA database (D-020; docs/m1/U3-charge-rifle.md 9).
//! The file is marked "TEMP sustained discharge testing" and R5R precaches it as a dev weapon; the
//! Season 3 release's settings are not in the local data (待定). The railgun version (the bolt after a
//! 0.85 s charge, U3's first take) is what R5R's `mp_weapon_defender.txt` is.
//!
//! The release's own numbers come from the user's video of it (2026-10-06: S3's HUD, World's Edge;
//! frames and measurements in scratch/video/cr1006) and replace the TEMP file's where they differ
//! (视频实测): the laser runs 0.5 s (four discharges, each 15 frames at 29.97 fps from the laser's
//! first frame to the shot's flash); 14 pulses of 3 (the damage numbers count 6, 12, 15 ... 39, then
//! 87 with the shot: 14 x 3 + 45); the shot 45 (another discharge: 6, then 51); the next discharge
//! began 0.90 s after a shot (aimed), sooner than the retail `fire` sequence allows (1.07 s).
//!
//! The discharge (S3 native code; weapon entity offsets: duration +0x1D58, pulse frequency +0x1D60):
//! - The trigger with the next attack due starts it (0x141009AB0 -> 0x14100B460: a weapon with
//!   `sustained_discharge_duration` > 0 begins a discharge instead of attacking): the script's
//!   `OnWeaponSustainedDischargeBegin` (`OnAbilityStart_Defender_Sustained`) returns 1, the rounds
//!   it takes from the magazine (0x14100B5B0 -> 0x14101CD40); state 7, the view model's
//!   ACT_VM_SUSTAINED_DISCHARGE; it ends `sustained_discharge_duration` later (the video: 0.5 s).
//! - Each frame of it (0x141004200: state 7): the end first (0x14100D440: the time is up, or
//!   `sustained_discharge_require_input` with the trigger let go: it is 0, so it always runs its
//!   time), else a pulse when due (0x14100BB00: the first right away, then every
//!   `sustained_discharge_pulse_frequency`, on the clock, not from the frame): at 0.036 s (the
//!   video's 14 in 0.5 s; the TEMP file's 0.104) the 15th would be due after the end.
//! - A pulse with `sustained_laser_enabled` (0x14100BD50 -> 0x14100C1B0): one ray
//!   (`sustained_laser_radial_iterations` 1: the centre only) from the eye along the shot's direction
//!   with all of the view kick (0x140BF4FC0), no spread (no `sustained_laser_spread_pattern`), out to
//!   `sustained_laser_range` 19685 units (500 m); the first character it meets takes the weapon's
//!   damage at its distance (the falloff 0x140C38490) times `sustained_laser_damage_scale`: the
//!   video's 3 a pulse of the shot's 45 (the TEMP file: 1.0 of 5), by the hit's body part as any
//!   bullet's (推断: the zone scales are the damage system's, not read here).
//! - The end (0x14100B7C0): with `sustained_discharge_ends_in_primary_attack` 1 and the time run out,
//!   the weapon attacks once (0x141009AB0 with +5488 set): `FireDefenderSustained` fires one bullet
//!   (`FireWeaponBullet`: hitscan, the weapon's spread, the same damage table: 45) and takes no
//!   round (0x14100ABE0 skips the ammo with +5488 set). The attack's view kick (0x141019540) and
//!   spread kick are that shot's; the pulses kick nothing. The next attack is then the end plus the
//!   length of the view model's sequence playing (0x140CC5940: the `fire` it just started, 40 frames
//!   at the hip, 33 aimed or crouched, 30 fps, blended by the zoom and crouchFraction), here at most
//!   the video's 0.90 s (the release's sequence was shorter than the retail one we have).
//! - `sustained_discharge_updates_charge` 1 (0x141007420): the charge (the crosshair's bar, the view
//!   model's `charge_loop_layer`) is the discharge's fraction while it lasts, then drains over
//!   `charge_cooldown_time` 1.0 s after `charge_cooldown_delay` 0.
//! - `is_semi_auto` 1 (`_base_sniper.txt`) with `bypass_semiauto_hold_protection` 1: holding the
//!   trigger starts the next discharge as soon as it is due (推断: the flag's name).
//! So one pull: the beam for 0.5 s (14 x 3) and the shot (45); held, again 0.9 s after the shot.
//!
//! Magazine `ammo_clip_size` 4, a round a discharge (reserve endless, D-003); `reload_time` and
//! `reloadempty_time` 2.0, the magazine full at the view model's `AE_WPN_FILLAMMO` (retail
//! `chargerifle_base_v_animRig.qc`: reload frame 106 of 136, reload_empty 143 of 173). An empty
//! trigger clicks (`allow_empty_click`, `sound_dryfire` "rifle_dryfire") and reloads
//! (`ammo_min_to_fire_autoreloads` default 1). No reload mid-discharge (the state-7 branch handles
//! nothing else).
//!
//! Spread of the shot: none, at the hip as aimed (the user, 2026-10-06: "no random spread for the
//! Charge Rifle"): the shot goes where the laser does, into the crosshair. S3's would be (degrees,
//! gun.rs's reading) hip stand 4, moving 5, sprint 6, crouched 3.75, air 8, aimed 0, towards it at
//! 75 / 75, the shot adding `spread_kick_on_fire_*_hip` 1 up to `spread_max_kick_*_hip` 12 / 10 / 12,
//! decaying at 4 after 0.1 s (`_base_sniper.txt`).
//!
//! The shot's view kick (0x141019540): pitch `viewkick_pitch_base` -1 plus a random sign times a
//! uniform up to `_random`/2 0.75; yaw 0 +/- up to 0.25; roll +/- 0.6..0.9. The soft part (times
//! `_softScale` 0 / 1 / 0.5) goes x20 into the kick spring's velocity, the hard part (`_hardScale`
//! 3 / 0 / 2.6) into its angle (0x140BE7F90); spring `viewkick_spring` `titan_arc`; the view shows
//! (1 - weaponFraction) of it (0.5 hip, 0.6 aimed: viewfx.rs `weapon_kick`).

use std::sync::Mutex;
use std::time::Instant;

use glam::Vec3;

use super::gun::{self, HudState, RayEnd, Zone};
use super::weapons::Trigger;
use crate::log;

// ---- S3 numbers ------------------------------------------------------------------------------

/// S3's `ammo_clip_size` 4, plus 4 (the user, 2026-10-09).
pub const CLIP: u32 = 8;
/// `sustained_discharge_duration`, `sustained_discharge_pulse_frequency`: the release's, from the
/// video (the TEMP file: 1.25, 0.104).
pub const DISCHARGE: f32 = 0.5;
pub const PULSE: f32 = 0.036;
/// The longest wait after a shot: the video's next discharge 0.90 s after one (aimed; the hip's 推断).
const REFIRE_MAX: f32 = 0.9;
/// The drain after a discharge (`charge_cooldown_delay`, `charge_cooldown_time`).
const CHARGE_COOLDOWN_DELAY: f32 = 0.0;
const CHARGE_COOLDOWN_TIME: f32 = 1.0;
const RELOAD: f32 = 2.0;
const RELOAD_EMPTY: f32 = 2.0;
/// AE_WPN_FILLAMMO in the retail view model's `reload` (106 of 136) and `reload_empty` (143 of 173).
const FILL_RELOAD: f32 = 106.0 / 136.0;
const FILL_RELOAD_EMPTY: f32 = 143.0 / 173.0;
/// `zoom_time_in` / `zoom_time_out` (s); `ads_fov_zoomfrac_start` / `_end` are not set (the optics
/// set them per sight; none here): the engine's 0 and 1.
pub const ZOOM_IN: f32 = 0.2;
pub const ZOOM_OUT: f32 = 0.15;
pub const ADS_FOV_FROM: f32 = 0.0;
pub const ADS_FOV_TO: f32 = 1.0;
/// The view model's `fire` (QC: 2x2 by `ads_blend` and `crouchFraction`): its samples' frames (hip,
/// aimed, crouched, aimed crouched; local Casts `animseq/weapons/defender/ptpov_defender/fire_0..3`), 30 fps.
const FIRE_FRAMES: [f32; 4] = [40.0, 33.0, 33.0, 33.0];
const FIRE_FPS: f32 = 30.0;

/// Damage (Apex units of distance): near, far; the very far value is not set (the far one holds).
/// The shot's 45 is the video's (the TEMP file: 5); a falloff is not known (none).
const DAMAGE_NEAR: i32 = 45;
const DAMAGE_FAR: i32 = 45;
const NEAR_DISTANCE: f32 = 7500.0;
const FAR_DISTANCE: f32 = 10000.0;
const HEAD_SCALE: f32 = 1.8;
const LEG_SCALE: f32 = 0.9;
const HEADSHOT_DISTANCE: f32 = 29528.0;
/// `sustained_laser_range` (units); `sustained_laser_damage_scale`: the video's 3 a pulse of the
/// shot's 45 (retail's railgun file still scales it apart: its `amped_damage` has x0.83333 for the
/// laser against x1.2 for the damage).
pub const LASER_RANGE: f32 = 19685.0;
const LASER_DAMAGE_SCALE: f32 = 3.0 / 45.0;

/// Spread (degrees): stand, moving (`spread_stand_hip_run`), sprint, crouch, air. None (the user;
/// S3's hip: 4, 5, 6, 3.75, 8).
const SPREAD_HIP: [f32; 5] = [0.0, 0.0, 0.0, 0.0, 0.0];
const SPREAD_ADS: [f32; 5] = [0.0, 0.0, 0.0, 0.0, 0.0];
const SPREAD_UP: f32 = 75.0;
const SPREAD_DOWN: f32 = 75.0;
/// spread_kick_on_fire_*_hip and spread_max_kick_*_hip: stand, crouch, air (aimed: 0, not set).
/// None either (S3's: 1 a shot).
const KICK_HIP: [f32; 3] = [0.0, 0.0, 0.0];
const KICK_MAX_HIP: [f32; 3] = [12.0, 10.0, 12.0];
const KICK_DELAY: f32 = 0.1;
const KICK_DECAY: f32 = 4.0;
/// Above this (m/s) he counts as moving (gun.rs's 推断).
const MOVING: f32 = 0.5;

/// The shot's view kick: (base, random, inner exclude, soft scale, hard scale) for pitch and yaw;
/// roll (base, random min, random max, soft, hard).
const KICK_PITCH: [f32; 5] = [-1.0, 1.5, 0.0, 0.0, 3.0];
const KICK_YAW: [f32; 5] = [0.0, 0.5, 0.0, 1.0, 0.0];
const KICK_ROLL: [f32; 5] = [0.0, 0.6, 0.9, 0.5, 2.6];
/// viewkick_hipfire_weaponFraction / viewkick_ads_weaponFraction.
pub const WEAPON_FRACTION_HIP: f32 = 0.5;
pub const WEAPON_FRACTION_ADS: f32 = 0.6;
/// `titan_arc` (S3 springs.txt): stiffness and damping (pitch, yaw, roll), hip and aimed.
pub const SPRING_K_HIP: Vec3 = Vec3::new(60.0, 20.0, 16000.0);
pub const SPRING_C_HIP: Vec3 = Vec3::new(27.0, 13.5, 7.0);
pub const SPRING_K_ADS: Vec3 = Vec3::new(50.0, 40.0, 16000.0);
pub const SPRING_C_ADS: Vec3 = Vec3::new(20.0, 13.5, 7.0);

// ---- sounds: play names of tools/fuseaudio/export_audio.py `--set defender` (the events' lower case)

const VOLUME: f32 = 0.5;
/// `fire_sound_1_player_1p`: of the event's six play actions, the punch, the close laser and the 1p
/// beam. Layers 2, 4 and 5 are third-person sounds (`3p_Shot_CloseSizzle`, `3p_Fire_Beam_Mid`,
/// `3p_Fire_Beam_Dist`: their sources' names) that Miles picks by distance; played together they
/// went on loudly for a second after the shot (the user, 2026-10-08: "after firing normally there
/// is still a firing sound").
const SOUND_FIRE: [&str; 3] = ["weapon_chargerifle_fire_1p", "weapon_chargerifle_fire_1p_layer1", "weapon_chargerifle_fire_1p_layer3"];
/// All six, to stop them all whatever an older build played.
const SOUND_FIRE_ALL: [&str; 6] = [
    "weapon_chargerifle_fire_1p",
    "weapon_chargerifle_fire_1p_layer1",
    "weapon_chargerifle_fire_1p_layer2",
    "weapon_chargerifle_fire_1p_layer3",
    "weapon_chargerifle_fire_1p_layer4",
    "weapon_chargerifle_fire_1p_layer5",
];
/// `charge_sound_1p` (stops when full) and `charge_drain_sound_1p` (stops when empty).
const SOUND_WIND_UP: &str = "weapon_chargerifle_windup_1p";
const SOUND_WIND_DOWN: &str = "weapon_chargerifle_winddown_1p";
/// `sound_trigger_pull` (the discharge's start) and `sound_trigger_release`.
const SOUND_TRIGGER_ON: &str = "weapon_chargerifle_triggeron";
const SOUND_TRIGGER_OFF: &str = "weapon_chargerifle_triggeroff";
/// The view model's `sustained_discharge` (QC AE_CL_PLAYSOUND frames 0, 24, 52, 78 of its 105-frame
/// loop at 30 fps; 0.5 s plays the first).
const SOUND_MECH: &str = "weapon_chargerifle_fastidle_mechfwdback_1p";
const MECH_FRAMES: [f32; 4] = [0.0, 24.0, 52.0, 78.0];
/// The impact tables' sounds to the shooter (`Sound_attacker`): the pulses
/// (`sustained_laser_impact_effect_table` exp_defender_small) and the shot (`impact_effect_table`
/// exp_defender), on a surface ("C": Elden Ring's surfaces have no Apex material, 推断) or flesh ("F").
/// Each event's two play actions together.
const SOUND_PULSE_SURFACE: [&str; 2] = ["chargerifle_smallbeam_bulletimpact_1p_vs_3p", "chargerifle_smallbeam_bulletimpact_1p_vs_3p_layer1"];
/// (Not its second play action: `TitanCoreAbility_LaserCannon_ThickBeam_FD_1P_..._LP`, a 4.3 s loop
/// Miles holds while the beam is on the target; played whole on each of the 14 pulses it went on for
/// seconds after the shot: the user, 2026-10-08, "on a monster the sound is still wrong".)
const SOUND_PULSE_FLESH: [&str; 1] = ["flesh_bulletimpact_chargerifle_beam_1p_vs_3p"];
const SOUND_PULSE_FLESH_LOOP: &str = "flesh_bulletimpact_chargerifle_beam_1p_vs_3p_layer1";
const SOUND_SHOT_SURFACE: [&str; 2] = ["chargerifle_fullshot_bulletimpact_1p_vs_3p", "chargerifle_fullshot_bulletimpact_1p_vs_3p_layer1"];
const SOUND_SHOT_FLESH: [&str; 2] = ["flesh_bulletimpact_chargerifle_shot_1p_vs_3p", "flesh_bulletimpact_chargerifle_shot_1p_vs_3p_layer1"];
const SOUND_ADS_IN: &str = "weapon_chargerifle_ads_in";
const SOUND_ADS_OUT: &str = "weapon_chargerifle_ads_out";
/// `_base_sniper.txt` `sound_dryfire`.
const SOUND_DRY: &str = "rifle_dryfire";
/// The view model's QC: `holster` 0, `draw` 0.
const SOUND_UNEQUIP: &str = "weapon_chargerifle_unequip";
const SOUND_EQUIP: &str = "weapon_chargerifle_equip";
/// The QC's reload sounds: (play name, frame) in `reload` (136 frames) and `reload_empty` (173).
const RELOAD_SOUNDS: [(&str, f32); 11] = [
    ("wpn_chargerifle_1p_reload_armlift_fr04", 2.0),
    ("wpn_chargerifle_1p_reload_ejectmag_fr13", 11.0),
    ("wpn_chargerifle_1p_reload_steamrelease_fr18", 16.0),
    ("wpn_chargerifle_1p_reload_steamrelease_fr18_layer1", 16.0),
    ("wpn_chargerifle_1p_reload_armlift_fr41", 39.0),
    ("wpn_chargerifle_1p_reload_insertmag_fr51", 49.0),
    ("wpn_chargerifle_1p_reload_twistlever_fr61", 59.0),
    ("wpn_chargerifle_1p_reload_pullout_fr77", 75.0),
    ("wpn_chargerifle_1p_reload_armlift_fr93", 91.0),
    ("wpn_chargerifle_1p_reload_slapclosed_fr105", 103.0),
    ("wpn_chargerifle_1p_reload_handsettle_fr121", 119.0),
];
const RELOAD_EMPTY_SOUNDS: [(&str, f32); 14] = [
    ("wpn_chargerifle_1p_reload_armlift_fr04", 2.0),
    ("wpn_chargerifle_1p_reload_ejectmag_fr13", 11.0),
    ("wpn_chargerifle_1p_reload_steamrelease_fr18", 16.0),
    ("wpn_chargerifle_1p_reload_steamrelease_fr18_layer1", 16.0),
    ("wpn_chargerifle_1p_reload_armlift_fr41", 39.0),
    ("wpn_chargerifle_1p_reload_insertmag_fr51", 48.0),
    ("wpn_chargerifle_1p_reload_twistlever_fr61", 59.0),
    ("wpn_chargerifle_1p_reload_pullout_fr77", 75.0),
    ("wpn_chargerifle_1p_reloadempty_switchflip_fr100", 97.0),
    ("wpn_chargerifle_1p_reloadempty_spinup_fr113", 111.0),
    ("wpn_chargerifle_1p_reload_armlift_fr93", 120.0),
    ("wpn_chargerifle_1p_reloadempty_spinup_fr136_pt2", 134.0),
    ("wpn_chargerifle_1p_reload_slapclosed_fr105", 138.0),
    ("wpn_chargerifle_1p_reload_handsettle_fr121", 156.0),
];
const RELOAD_FRAMES: f32 = 136.0;
const RELOAD_EMPTY_FRAMES: f32 = 173.0;

fn stop_sound(name: &'static str) {
    crate::audio::stop(name);
}

fn sound(name: &'static str) {
    crate::audio::play(name, VOLUME);
}

// ---- the pure core -------------------------------------------------------------------------------

/// How he moves, for the spread.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Stance {
    #[default]
    Standing,
    Moving,
    Sprinting,
    Crouched,
    Air,
}

/// What the rifle gets this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub trigger: bool,
    pub aim: bool,
    /// the zoom (0..1) and crouchFraction (0..1): the `fire` sequence's blend, its length the wait
    /// after a shot
    pub ads: f32,
    pub crouch: f32,
    /// the reload key, pressed this frame
    pub reload: bool,
    /// in hand (drawn, coming out or going away); false: in its holster
    pub in_hand: bool,
    /// drawn and free to fire (not mid-switch, no ability holding the hands)
    pub ready: bool,
    pub stance: Stance,
    /// S pressed this frame: the discharge stops (the user's control, 2026-10-06; S3 has none, its
    /// `sustained_discharge_require_input` is 0)
    pub cancel: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// a discharge starts (a round taken)
    Begin,
    /// a laser pulse (its index in the discharge, from 0)
    Pulse { index: u32 },
    /// the discharge ran its time: its end's shot, with the spread at it (degrees)
    Fire { spread: f32 },
    /// put away mid-discharge: no shot (the round is gone)
    Cut,
    /// S mid-discharge: no shot (the round is gone), the trigger let go before the next one
    Cancel,
    /// the trigger let go (`sound_trigger_release`)
    Release,
    DryFire,
    ReloadStart { empty: bool },
    Filled,
    Reloaded,
    /// put away mid-reload: the reload is off
    ReloadCut,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Reload {
    t: f32,
    total: f32,
    fill: f32,
    empty: bool,
    filled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Rifle {
    /// its own clock (s)
    pub now: f32,
    pub ammo: u32,
    /// S3 +10692 / +10696: when the discharge started and its next pulse is due (None: none going)
    discharge: Option<f32>,
    next_pulse: f32,
    pulses: u32,
    /// when the last discharge ended and its fraction then (the drain)
    charge_end: f32,
    end_frac: f32,
    /// the next attack (S3 +4656)
    next_attack: f32,
    /// the empty click: once a pull
    need_release: bool,
    trigger_was: bool,
    reload: Option<Reload>,
    pub spread: f32,
    pub kick: f32,
    since_shot: f32,
    pub shots: u32,
}

impl Default for Rifle {
    fn default() -> Self {
        Rifle::new()
    }
}

/// The view model's `fire` length (s) at this blend: Source's cycle rate of a blended sequence is
/// its samples' rates by their weights (fps / (frames - 1)), the length its inverse.
pub fn fire_seconds(ads: f32, crouch: f32) -> f32 {
    let (a, c) = (ads.clamp(0.0, 1.0), crouch.clamp(0.0, 1.0));
    let w = [(1.0 - a) * (1.0 - c), a * (1.0 - c), (1.0 - a) * c, a * c];
    let rate: f32 = w.iter().zip(FIRE_FRAMES).map(|(w, n)| w * FIRE_FPS / (n - 1.0)).sum();
    1.0 / rate.max(1e-3)
}

impl Rifle {
    pub const fn new() -> Rifle {
        Rifle {
            now: 0.0,
            ammo: CLIP,
            discharge: None,
            next_pulse: 0.0,
            pulses: 0,
            charge_end: -100.0,
            end_frac: 0.0,
            next_attack: 0.0,
            need_release: false,
            trigger_was: false,
            reload: None,
            spread: SPREAD_HIP[0],
            kick: 0.0,
            since_shot: 10.0,
            shots: 0,
        }
    }

    /// The charge, 0..1 (S3 0x141007420): the discharge's fraction, then the drain.
    pub fn frac(&self) -> f32 {
        if let Some(start) = self.discharge {
            return ((self.now - start) / DISCHARGE).clamp(0.0, 1.0);
        }
        let drained = ((self.now - self.charge_end - CHARGE_COOLDOWN_DELAY) / CHARGE_COOLDOWN_TIME).clamp(0.0, 1.0);
        (self.end_frac - drained).clamp(0.0, 1.0)
    }

    /// Seconds into the discharge going on.
    pub fn discharging(&self) -> Option<f32> {
        self.discharge.map(|s| self.now - s)
    }

    /// Reload progress (0..1) and whether it is the empty one.
    pub fn reloading(&self) -> Option<(f32, bool)> {
        self.reload.map(|r| ((r.t / r.total).clamp(0.0, 1.0), r.empty))
    }

    /// Ends the discharge (0x14100B7C0): its time and fraction for the drain.
    fn end_discharge(&mut self) {
        if let Some(start) = self.discharge.take() {
            self.end_frac = ((self.now - start) / DISCHARGE).clamp(0.0, 1.0);
            self.charge_end = self.now;
        }
    }

    fn start_reload(&mut self, ev: &mut Vec<Event>) {
        let empty = self.ammo == 0;
        let (total, fill) = if empty { (RELOAD_EMPTY, FILL_RELOAD_EMPTY) } else { (RELOAD, FILL_RELOAD) };
        self.reload = Some(Reload { t: 0.0, total, fill, empty, filled: false });
        ev.push(Event::ReloadStart { empty });
    }

    /// The spread his stance asks for and the kick row (stand, crouch, air).
    fn spread_target(stance: Stance, aim: bool) -> (f32, usize) {
        let table = if aim { SPREAD_ADS } else { SPREAD_HIP };
        match stance {
            Stance::Air => (table[4], 2),
            Stance::Crouched => (table[3], 1),
            Stance::Sprinting => (table[2], 0),
            Stance::Moving => (table[1], 0),
            Stance::Standing => (table[0], 0),
        }
    }

    /// One frame of `dt` seconds; what happened.
    pub fn step(&mut self, dt: f32, i: Input) -> Vec<Event> {
        let dt = dt.clamp(0.0, 0.1);
        self.now += dt;
        let mut ev = Vec::new();
        let released = self.trigger_was && !i.trigger;
        self.trigger_was = i.trigger;
        if !i.in_hand {
            // in its holster: a discharge or a reload going on when it went away is over
            if self.discharge.is_some() {
                self.end_discharge();
                ev.push(Event::Cut);
            }
            if self.reload.take().is_some() {
                ev.push(Event::ReloadCut);
            }
            self.need_release = self.need_release && i.trigger;
            return ev;
        }
        if released {
            ev.push(Event::Release);
        }
        // the spread: towards the stance's, the kick decaying after the delay
        let (target, kick_row) = Self::spread_target(i.stance, i.aim);
        let step = if self.spread < target { SPREAD_UP } else { SPREAD_DOWN } * dt;
        self.spread = if self.spread < target { (self.spread + step).min(target) } else { (self.spread - step).max(target) };
        self.since_shot += dt;
        if self.since_shot > KICK_DELAY {
            self.kick = (self.kick - KICK_DECAY * dt).max(0.0);
        }
        if !i.trigger {
            self.need_release = false;
        }
        // state 7: the end first, else the pulse due
        if let Some(start) = self.discharge {
            if self.now >= start + DISCHARGE {
                self.end_discharge();
                self.shots += 1;
                self.next_attack = self.now + fire_seconds(i.ads, i.crouch).min(REFIRE_MAX);
                let spread = self.spread + self.kick;
                if !i.aim {
                    self.kick = (self.kick + KICK_HIP[kick_row]).min(KICK_MAX_HIP[kick_row]);
                }
                self.since_shot = 0.0;
                ev.push(Event::Fire { spread });
            } else if i.cancel {
                // the charge drains from here; a held trigger does not start another
                self.end_discharge();
                self.need_release = true;
                ev.push(Event::Cancel);
            } else if self.now >= self.next_pulse {
                ev.push(Event::Pulse { index: self.pulses });
                self.pulses += 1;
                self.next_pulse += PULSE;
            }
            return ev;
        }
        if let Some(mut r) = self.reload {
            r.t += dt;
            if !r.filled && r.t >= r.total * r.fill {
                r.filled = true;
                self.ammo = CLIP;
                ev.push(Event::Filled);
            }
            if r.t >= r.total {
                self.reload = None;
                self.ammo = CLIP;
                ev.push(Event::Reloaded);
            } else {
                self.reload = Some(r);
            }
            return ev;
        }
        if i.reload && self.ammo < CLIP && i.ready {
            self.start_reload(&mut ev);
            return ev;
        }
        // held: is_semi_auto with bypass_semiauto_hold_protection, the next discharge when due
        if i.trigger && i.ready && !self.need_release && self.now >= self.next_attack {
            if self.ammo == 0 {
                // the empty click, then the reload (ammo_min_to_fire_autoreloads)
                self.need_release = true;
                ev.push(Event::DryFire);
                self.start_reload(&mut ev);
            } else {
                // 0x14100B460: the round taken, the first pulse due now (the next frame's update)
                self.ammo -= 1;
                self.discharge = Some(self.now);
                self.next_pulse = self.now;
                self.pulses = 0;
                self.next_attack = self.now;
                ev.push(Event::Begin);
            }
        }
        ev
    }
}

/// Apex damage at `distance` Apex units (S3 0x140C38490, linear falloff), before the zones.
pub fn damage_at(distance: f32) -> f32 {
    let lerp = |a: i32, b: i32, f: f32| (a as f32 + f * (b - a) as f32) as i32 as f32;
    if distance <= NEAR_DISTANCE {
        return DAMAGE_NEAR as f32;
    }
    if distance < FAR_DISTANCE {
        return lerp(DAMAGE_NEAR, DAMAGE_FAR, (distance - NEAR_DISTANCE) / (FAR_DISTANCE - NEAR_DISTANCE));
    }
    DAMAGE_FAR as f32
}

/// A hit's Apex damage: by distance (Apex units), the zone (the head only within
/// `headshot_distance`), times the ini multiplier (a pulse also times the laser's damage scale).
pub fn hit_damage(zone: Zone, distance: f32, mult: f32) -> f32 {
    let scale = match zone {
        Zone::Head if distance <= HEADSHOT_DISTANCE => HEAD_SCALE,
        Zone::Legs => LEG_SCALE,
        _ => 1.0,
    };
    damage_at(distance) * scale * mult
}

/// One shot's view kick (S3 0x141019540): the soft part (into the spring's velocity, x20 there) and
/// the hard part (into its angle), pitch / yaw / roll in degrees; `u` uniforms in 0..1 (pitch, its
/// sign, yaw, its sign, roll; the roll's sign the sixth).
pub fn view_kick(u: [f32; 6]) -> (Vec3, Vec3) {
    let sign = |s: f32| if s < 0.5 { 1.0 } else { -1.0 };
    let pick = |k: [f32; 5], r: f32, s: f32| {
        let (half, inner) = (k[1] * 0.5, k[2] * 0.5);
        k[0] + sign(s) * (inner + (half - inner) * r)
    };
    let pitch = pick(KICK_PITCH, u[0], u[1]);
    let yaw = pick(KICK_YAW, u[2], u[3]);
    let roll = KICK_ROLL[0] + sign(u[5]) * (KICK_ROLL[1] + (KICK_ROLL[2] - KICK_ROLL[1]) * u[4]);
    let soft = Vec3::new(pitch * KICK_PITCH[3], yaw * KICK_YAW[3], roll * KICK_ROLL[3]);
    let hard = Vec3::new(pitch * KICK_PITCH[4], yaw * KICK_YAW[4], roll * KICK_ROLL[4]);
    (soft, hard)
}

// ---- in the game ----------------------------------------------------------------------------------

/// What the HUD draws of the beams (hud/beam.rs): the laser while a discharge goes on (where it
/// ends now, seconds into it, whether on a character), and the last shot's beam (where it started:
/// the muzzle when it was fired, if known; where it ended, seconds since, on a character) with the
/// shots fired so far (its number); a discharge stopped with S (seconds since, how long it ran).
#[derive(Clone, Copy, Debug, Default)]
pub struct Beams {
    pub laser: Option<(Vec3, f32, bool)>,
    pub shot: Option<(Option<Vec3>, Vec3, f32, bool)>,
    pub shots: u32,
    pub cancel: Option<(f32, f32)>,
}

struct State {
    rifle: Rifle,
    rng: u32,
    was_aiming: bool,
    hits: u32,
    /// the wind-down plays (to cut when the charge is empty or a discharge starts again)
    winding_down: bool,
    trace_until: Option<Instant>,
    /// the laser's end now (world) and whether on a character; the last shot's start (the muzzle
    /// then), end and when
    laser_end: Option<(Vec3, bool)>,
    shot_end: Option<(Option<Vec3>, Vec3, Instant, bool)>,
    /// the last discharge stopped with S: when, and how long it had run
    cancelled: Option<(Instant, f32)>,
}

static STATE: Mutex<State> = Mutex::new(State {
    rifle: Rifle::new(),
    rng: 0x2545_f491,
    was_aiming: false,
    hits: 0,
    winding_down: false,
    trace_until: None,
    laser_end: None,
    shot_end: None,
    cancelled: None,
});

/// S last frame (its press is an edge), and a press asked for by the dev channel (`cr cancel`, `cr
/// fire <s> cancel <t>`): when.
static S_WAS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CANCEL_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// S pressed this frame, only while the game window has the focus (as weapons.rs `number_keys`);
/// S also moves him back (input.rs), as the user chose.
fn s_pressed() -> bool {
    use std::sync::atomic::Ordering;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    let down = pid == unsafe { GetCurrentProcessId() } && unsafe { GetAsyncKeyState(b'S' as i32) } as u16 & 0x8000 != 0;
    let was = S_WAS.swap(down, Ordering::Relaxed);
    let asked = {
        let mut at = CANCEL_AT.lock().unwrap_or_else(|e| e.into_inner());
        at.is_some_and(|t| Instant::now() >= t) && at.take().is_some()
    };
    (down && !was) || asked
}

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Uniform 0..1 (xorshift).
fn next(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

fn stance() -> Stance {
    match super::kcc::locomotion() {
        Some(l) if !l.grounded => Stance::Air,
        Some(l) if l.crouched => Stance::Crouched,
        Some(l) if l.sprinting => Stance::Sprinting,
        Some(l) if l.speed > MOVING => Stance::Moving,
        _ => Stance::Standing,
    }
}

/// The laser's and the shot's range (m).
fn range() -> f32 {
    LASER_RANGE / super::kcc::UNITS_PER_METRE
}

/// A shot's random draws (the spread cone, the view kick), taken under the lock.
struct Shot {
    spread: f32,
    r: (f32, f32),
    u: [f32; 6],
    shots: u32,
    ammo: u32,
}

/// Once a frame from weapons.rs: the rifle, in hand or not. The state's lock is held only for the
/// step and its bookkeeping, never while a ray reads the camera or a sound is sent: camera.rs asks
/// for the HUD state (this lock) while it holds its own.
pub fn update(dt: f32, t: Trigger, in_hand: bool, ready: bool) {
    // the hands held by an ability (the stim's switch to one hand, the pad's toss): no discharge
    let ready = ready && !super::pov::gun_away();
    let aim = super::weapons::aiming(t.aim) && in_hand;
    let crouch = super::kcc::locomotion().map_or(0.0, |l| l.duck_frac);
    let cancel = s_pressed();
    let input = Input { trigger: t.fire, aim, ads: crate::camera::ads_frac(), crouch, reload: t.reload, in_hand, ready, stance: stance(), cancel };
    let (ads_sound, events, shots, trace, stop_wind_down, ammo, discharging) = {
        let mut s = state();
        let ads_sound = (in_hand && s.was_aiming != aim).then_some(if aim { SOUND_ADS_IN } else { SOUND_ADS_OUT });
        s.was_aiming = aim && in_hand;
        let events = s.rifle.step(dt, input);
        let mut shots = Vec::new();
        for e in &events {
            match *e {
                Event::Begin => {
                    s.winding_down = false;
                    s.cancelled = None;
                }
                Event::Cancel => {
                    s.winding_down = true;
                    s.cancelled = Some((Instant::now(), s.rifle.end_frac * DISCHARGE));
                }
                Event::Fire { spread } => {
                    s.winding_down = true;
                    let r = (next(&mut s.rng), next(&mut s.rng));
                    let mut u = [0.0; 6];
                    for x in &mut u {
                        *x = next(&mut s.rng);
                    }
                    shots.push(Shot { spread, r, u, shots: s.rifle.shots, ammo: s.rifle.ammo });
                }
                Event::Cut if in_hand => s.winding_down = true,
                _ => {}
            }
        }
        let trace = s.trace_until.is_some_and(|u| Instant::now() < u).then(|| {
            let r = &s.rifle;
            format!(
                "cr trace: t {:.3} charge {:.3}{} ammo {} reload {:?} trigger {} ready {} spread {:.2}+{:.2}",
                r.now,
                r.frac(),
                r.discharging().map_or(String::new(), |d| format!(" discharging {d:.3}")),
                r.ammo,
                r.reloading(),
                t.fire,
                ready,
                r.spread,
                r.kick
            )
        });
        // the wind-down stops when the charge is empty (`charge_drain_sound_stop_when_empty`)
        let stop_wind_down = s.winding_down && s.rifle.discharging().is_none() && s.rifle.frac() <= 0.0;
        if stop_wind_down {
            s.winding_down = false;
        }
        if s.rifle.discharging().is_none() {
            s.laser_end = None;
        }
        (ads_sound, events, shots, trace, stop_wind_down, s.rifle.ammo, s.rifle.discharging())
    };
    if let Some(name) = ads_sound {
        sound(name);
    }
    if let Some(line) = trace {
        log(line);
    }
    let mut shots = shots.into_iter();
    let mut laser = None;
    for e in events {
        match e {
            Event::Begin => {
                stop_sound(SOUND_WIND_DOWN);
                sound(SOUND_TRIGGER_ON);
                // the charge rises with the discharge (`sustained_discharge_updates_charge`)
                sound(SOUND_WIND_UP);
                for f in MECH_FRAMES.iter().filter(|f| **f / 30.0 < DISCHARGE) {
                    crate::audio::play_in(SOUND_MECH, VOLUME, f / 30.0);
                }
                log(format!("cr: discharge ({DISCHARGE} s), {ammo} left in the magazine"));
            }
            Event::Pulse { index } => {
                // the laser's ray: along the shot's direction with all of the kick, no spread
                let out = gun::fire_ray_ex(0.0, (0.0, 0.0), crate::viewfx::weapon_aim_offset(), 1, range(), true, |zone, metres| {
                    // (not ini gun_damage_mult: the Charge Rifle keeps its own damage, the user's 2026-10-09 ask)
                    hit_damage(zone, metres * super::kcc::UNITS_PER_METRE, LASER_DAMAGE_SCALE)
                });
                if let Some(o) = out {
                    match o.on {
                        RayEnd::Target => SOUND_PULSE_FLESH.iter().for_each(|n| sound(n)),
                        RayEnd::Map => SOUND_PULSE_SURFACE.iter().for_each(|n| sound(n)),
                        RayEnd::Nothing => {}
                    }
                    if o.line.is_some() {
                        state().hits += 1;
                    }
                    if let Some(line) = &o.line {
                        log(format!("cr: pulse {index}: {line}"));
                    }
                    laser = Some((o.end, o.on == RayEnd::Target));
                }
            }
            Event::Fire { .. } => {
                let Some(shot) = shots.next() else { continue };
                stop_sound(SOUND_WIND_UP);
                for name in SOUND_FIRE {
                    sound(name);
                }
                sound(SOUND_WIND_DOWN);
                // the beam stays where it was fired (the video's swings with the view after it)
                let from = crate::firstperson::muzzle_world();
                let (soft, hard) = view_kick(shot.u);
                // the shot goes along the camera with the kick the view does not show (viewfx.rs);
                // not ini `gun_damage_mult` (the user's 2026-10-09 ask: the Charge Rifle keeps its own)
                let out = gun::fire_ray_ex(shot.spread, shot.r, crate::viewfx::weapon_aim_offset(), 1, range(), true, |zone, metres| {
                    hit_damage(zone, metres * super::kcc::UNITS_PER_METRE, 1.0)
                });
                crate::viewfx::weapon_kick(soft, hard);
                let mut line = None;
                if let Some(o) = out {
                    match o.on {
                        RayEnd::Target => SOUND_SHOT_FLESH.iter().for_each(|n| sound(n)),
                        RayEnd::Map => SOUND_SHOT_SURFACE.iter().for_each(|n| sound(n)),
                        RayEnd::Nothing => {}
                    }
                    if o.line.is_some() {
                        state().hits += 1;
                    }
                    state().shot_end = Some((from, o.end, Instant::now(), o.on == RayEnd::Target));
                    line = o.line;
                }
                log(format!("cr: shot {} ({}), ammo {}, {}", shot.shots, if aim { "aimed" } else { "hip" }, shot.ammo, line.unwrap_or_else(|| "miss".into())));
            }
            Event::Cut => {
                stop_firing_sounds();
                log("cr: discharge cut by the switch: no shot");
            }
            Event::Cancel => {
                // everything stops at once (the user, 2026-10-08: the drain sound after a cancel
                // read as the shot going on)
                stop_firing_sounds();
                log(format!("cr: discharge stopped with S: no shot, {ammo} left in the magazine"));
            }
            Event::Release => sound(SOUND_TRIGGER_OFF),
            Event::DryFire => sound(SOUND_DRY),
            Event::ReloadStart { empty } => {
                let (total, frames, list): (f32, f32, &[(&'static str, f32)]) =
                    if empty { (RELOAD_EMPTY, RELOAD_EMPTY_FRAMES, &RELOAD_EMPTY_SOUNDS) } else { (RELOAD, RELOAD_FRAMES, &RELOAD_SOUNDS) };
                for &(name, frame) in list {
                    crate::audio::play_in(name, VOLUME, frame / frames * total);
                }
                log(format!("cr: reloading ({total} s, {ammo} left in the magazine)"));
            }
            Event::Filled => log(format!("cr: magazine filled, {CLIP} rounds")),
            Event::Reloaded => log(format!("cr: reloaded, {CLIP} rounds")),
            Event::ReloadCut => {
                for (name, _) in RELOAD_EMPTY_SOUNDS.iter().chain(RELOAD_SOUNDS.iter()) {
                    stop_sound(name);
                }
                log("cr: reload cut by the switch");
            }
        }
    }
    // the laser drawn between pulses: where the aim meets something now (no damage)
    if discharging.is_some() && laser.is_none() {
        laser = gun::fire_ray_ex(0.0, (0.0, 0.0), crate::viewfx::weapon_aim_offset(), 1, range(), false, |_, _| 0.0).map(|o| (o.end, o.on == RayEnd::Target));
    }
    if discharging.is_some() && laser.is_some() {
        state().laser_end = laser;
    }
    if stop_wind_down {
        stop_sound(SOUND_WIND_DOWN);
    }
}

/// The rifle's own firing sounds, cut (the user, 2026-10-08: "after the shooting is interrupted
/// the sound plays on"): the charge, the mechanism, the shot's six layers (tails up to 3 s) and the
/// drain.
fn stop_firing_sounds() {
    for name in [SOUND_WIND_UP, SOUND_WIND_DOWN, SOUND_MECH, SOUND_TRIGGER_ON, SOUND_PULSE_FLESH_LOOP].into_iter().chain(SOUND_FIRE_ALL) {
        stop_sound(name);
    }
}

pub fn sound_holster() {
    // put away: what it was playing stops with it
    stop_firing_sounds();
    state().winding_down = false;
    sound(SOUND_UNEQUIP);
}

pub fn sound_draw(_first: bool) {
    // `deployfirst_time` is not set for this weapon: every draw is the normal one (weapons.rs)
    sound(SOUND_EQUIP);
}

/// The shot's look in slow motion for screenshots (dev `cr slow <x>`: its age runs x times slower).
static SLOW: Mutex<f32> = Mutex::new(1.0);

/// The beams for the HUD (hud/beam.rs).
pub fn beams() -> Beams {
    let slow = *SLOW.lock().unwrap_or_else(|e| e.into_inner());
    let s = state();
    let laser = s.rifle.discharging().and_then(|t| s.laser_end.map(|(at, hit)| (at, t, hit)));
    // its smoke lasts about a second
    let shot = s.shot_end.map(|(from, at, when, hit)| (from, at, when.elapsed().as_secs_f32() / slow, hit)).filter(|x| x.2 < 1.5);
    let cancel = s.cancelled.map(|(when, ran)| (when.elapsed().as_secs_f32(), ran)).filter(|x| x.0 < 0.5);
    Beams { laser, shot, shots: s.rifle.shots, cancel }
}

/// The charge (0..1) and the seconds into a discharge, for the view model (pov).
pub fn view() -> (f32, Option<f32>) {
    let s = state();
    (s.rifle.frac(), s.rifle.discharging())
}

/// The HUD's view of it while it is in hand.
pub fn hud() -> Option<HudState> {
    if !gun::enabled() || !crate::state::in_world() {
        return None;
    }
    let s = state();
    let r = &s.rifle;
    Some(HudState {
        ammo: r.ammo,
        clip: CLIP,
        reload: r.reloading().map(|x| x.0),
        aiming: super::weapons::aiming(gun::aim_held()),
        spread_deg: r.spread + r.kick,
        sprinting: super::kcc::locomotion().is_some_and(|l| l.sprinting),
        last_hit: gun::last_hit(),
        shots: r.shots,
        reload_empty: r.reloading().is_some_and(|x| x.1),
        slot: 1,
        charge: Some(r.frac()),
    })
}

pub fn status() -> String {
    let s = state();
    let r = &s.rifle;
    format!(
        "charge rifle (sustained): ammo {}/{CLIP}, charge {:.2}{}, reload {:?}, shots {}, hits {}, spread {:.2}+{:.2}",
        r.ammo,
        r.frac(),
        r.discharging().map_or(String::new(), |d| format!(" (discharging {d:.2} s)")),
        r.reloading(),
        r.shots,
        s.hits,
        r.spread,
        r.kick
    )
}

/// Dev channel `cr [fire <s> [cancel <t>] | trace <s> | ammo <n> | slow <x> | cancel]`.
pub fn dev(args: &[&str]) -> String {
    let secs = |i: usize| args.get(i).and_then(|a| a.parse::<f32>().ok());
    match args {
        [] => format!("{} | {}", super::weapons::describe(), status()),
        ["fire", _, "cancel", _] => {
            let t = secs(3).unwrap_or(0.25).max(0.0);
            *CANCEL_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + std::time::Duration::from_secs_f32(t));
            format!("{} (S in {t} s)", gun::hold_fire(secs(1).unwrap_or(1.0)))
        }
        ["fire", ..] => gun::hold_fire(secs(1).unwrap_or(1.0)),
        ["trace", ..] => {
            let t = secs(1).unwrap_or(5.0).max(0.0);
            state().trace_until = Some(Instant::now() + std::time::Duration::from_secs_f32(t));
            format!("cr: tracing every frame for {t} s")
        }
        ["ammo", n] => match n.parse::<u32>() {
            Ok(n) => {
                state().rifle.ammo = n.min(CLIP);
                status()
            }
            Err(_) => format!("usage: cr ammo <0-{CLIP}>"),
        },
        ["cancel"] => {
            *CANCEL_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            "cr: S pressed (a discharge going on stops)".into()
        }
        ["slow", x] => match x.parse::<f32>() {
            Ok(x) if x >= 1.0 => {
                *SLOW.lock().unwrap_or_else(|e| e.into_inner()) = x;
                format!("cr: the shot's look {x} times slower")
            }
            _ => "usage: cr slow <factor, 1 or more>".into(),
        },
        _ => "usage: cr [fire <seconds> [cancel <seconds>] | trace <seconds> | ammo <n> | slow <factor> | cancel]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn held(trigger: bool) -> Input {
        Input { trigger, in_hand: true, ready: true, ..Default::default() }
    }

    /// Runs until `until` (the rifle's clock), collecting (time, event).
    fn run(r: &mut Rifle, until: f32, i: Input, out: &mut Vec<(f32, Event)>) {
        while r.now < until - 1e-5 {
            for e in r.step(DT, i) {
                out.push((r.now, e));
            }
        }
    }

    fn times(ev: &[(f32, Event)], f: impl Fn(&Event) -> bool) -> Vec<f32> {
        ev.iter().filter(|e| f(&e.1)).map(|e| e.0).collect()
    }

    /// One pull, let go at once: the discharge runs its 0.5 s anyway (`require_input` 0), 14
    /// pulses 0.036 s apart from the frame after the start, the shot at its end; a round taken at
    /// the start, none by the shot; the charge rises with it and drains over 1 s after.
    #[test]
    fn a_pull_discharges_for_its_time() {
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        run(&mut r, DT, held(true), &mut ev);
        run(&mut r, 3.0, held(false), &mut ev);
        let begin = times(&ev, |e| *e == Event::Begin);
        assert_eq!(begin.len(), 1, "{ev:?}");
        let pulses = times(&ev, |e| matches!(e, Event::Pulse { .. }));
        // the video's 14 (the 15th would be due at 0.504 s, after the end)
        assert_eq!(pulses.len(), 14, "{ev:?}");
        assert!((pulses[0] - begin[0] - DT).abs() < 1e-4, "the first pulse the next frame: {pulses:?}");
        for (k, p) in pulses.iter().enumerate() {
            // due at start + k x 0.104, in the first frame at or after it
            let due = begin[0] + k as f32 * PULSE;
            assert!(*p >= due - 1e-4 && *p < due + DT + 1e-4, "pulse {k} at {p}, due {due}");
        }
        let fire = times(&ev, |e| matches!(e, Event::Fire { .. }));
        assert_eq!(fire.len(), 1);
        assert!((fire[0] - begin[0] - DISCHARGE).abs() <= DT + 1e-4, "{fire:?}");
        assert_eq!(r.ammo, CLIP - 1);
        // the charge: 0.5 half way, full at the end, drained 1 s later
        let mut probe = Rifle::new();
        probe.discharge = Some(0.0);
        probe.now = DISCHARGE * 0.5;
        assert!((probe.frac() - 0.5).abs() < 1e-5);
        probe.now = DISCHARGE;
        probe.end_discharge();
        assert_eq!(probe.frac(), 1.0);
        probe.now = DISCHARGE + 0.5;
        assert!((probe.frac() - 0.5).abs() < 1e-5, "{}", probe.frac());
        probe.now = DISCHARGE + 1.0;
        assert_eq!(probe.frac(), 0.0);
    }

    /// Held: the next discharge once the `fire` sequence started by the shot is over (the retail
    /// one: 1.3 s at the hip standing, 32/30 s aimed or crouched), at most the video's 0.9 s.
    #[test]
    fn held_trigger_repeats_after_the_wait() {
        assert!((fire_seconds(0.0, 0.0) - 39.0 / 30.0).abs() < 1e-5);
        assert!((fire_seconds(1.0, 0.0) - 32.0 / 30.0).abs() < 1e-5);
        assert!((fire_seconds(0.0, 1.0) - 32.0 / 30.0).abs() < 1e-5);
        for input in [held(true), Input { aim: true, ads: 1.0, ..held(true) }] {
            let mut r = Rifle::new();
            let mut ev = Vec::new();
            run(&mut r, 6.0, input, &mut ev);
            let begin = times(&ev, |e| *e == Event::Begin);
            let fire = times(&ev, |e| matches!(e, Event::Fire { .. }));
            assert!(begin.len() >= 2, "{ev:?}");
            assert!((begin[1] - fire[0] - REFIRE_MAX).abs() <= DT + 1e-4, "{begin:?} {fire:?}");
        }
    }

    /// A magazine's discharges, the next pull clicks and reloads (2 s, full at 143/173 of it); a reload
    /// asked for with rounds left also takes 2 s (full at 106/136); none mid-discharge.
    #[test]
    fn magazine_and_reloads() {
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        let mut t = 0.0;
        for _ in 0..=CLIP {
            t += 0.1;
            run(&mut r, t, held(true), &mut ev);
            t += 3.0;
            run(&mut r, t, held(false), &mut ev);
        }
        assert_eq!(times(&ev, |e| matches!(e, Event::Fire { .. })).len(), CLIP as usize, "{ev:?}");
        assert!(ev.iter().any(|e| e.1 == Event::DryFire));
        let start = ev.iter().find(|e| e.1 == Event::ReloadStart { empty: true }).expect("empty reload").0;
        assert_eq!(r.ammo, CLIP, "the reload ran in the last 3 s");
        let filled = ev.iter().find(|e| e.1 == Event::Filled).unwrap().0;
        assert!((filled - start - 2.0 * 143.0 / 173.0).abs() <= DT + 1e-4, "{filled} {start}");
        // a tactical reload: none while the discharge goes on, then 2 s
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        run(&mut r, 0.2, held(true), &mut ev);
        r.step(DT, Input { reload: true, ..held(false) });
        assert!(r.reloading().is_none(), "no reload mid-discharge");
        run(&mut r, 2.0, held(false), &mut ev);
        assert_eq!(r.ammo, CLIP - 1);
        r.step(DT, Input { reload: true, ..held(false) });
        let start = r.now;
        run(&mut r, start + 2.0 * 106.0 / 136.0 - 0.05, held(false), &mut ev);
        assert_eq!(r.ammo, CLIP - 1);
        run(&mut r, start + 2.0 * 106.0 / 136.0 + 0.05, held(false), &mut ev);
        assert_eq!(r.ammo, CLIP);
    }

    /// Put away mid-discharge: no shot, the round gone; mid-reload: the reload is off; not ready
    /// (mid-switch): nothing.
    #[test]
    fn holstering_cuts_the_discharge_and_reload() {
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        run(&mut r, 0.3, held(true), &mut ev);
        run(&mut r, 2.5, Input { trigger: true, ..Default::default() }, &mut ev);
        assert!(times(&ev, |e| matches!(e, Event::Fire { .. })).is_empty() && ev.iter().any(|e| e.1 == Event::Cut), "{ev:?}");
        assert_eq!(r.ammo, CLIP - 1);
        r.ammo = 2;
        r.step(DT, Input { reload: true, ..held(false) });
        let until = r.now + 0.5;
        run(&mut r, until, held(false), &mut ev);
        r.step(DT, Input::default());
        assert!(r.reloading().is_none() && r.ammo == 2);
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        run(&mut r, 2.0, Input { trigger: true, in_hand: true, ready: false, ..Default::default() }, &mut ev);
        assert!(ev.is_empty(), "{ev:?}");
    }

    /// The shot 45 and a pulse 3 at any distance (the video); x1.8 on the head within 750 m, x0.9
    /// on the legs.
    #[test]
    fn damage_by_zone() {
        for d in [0.0, 7500.0, 9000.0, 20000.0] {
            assert_eq!(damage_at(d), 45.0);
        }
        assert!((hit_damage(Zone::Head, 1000.0, 1.0) - 81.0).abs() < 1e-4);
        assert!((hit_damage(Zone::Legs, 1000.0, 1.0) - 40.5).abs() < 1e-4);
        assert!((hit_damage(Zone::Body, 1000.0, 3.0) - 135.0).abs() < 1e-4);
        assert_eq!(hit_damage(Zone::Head, 30000.0, 1.0), 45.0);
        assert!((hit_damage(Zone::Body, 1000.0, LASER_DAMAGE_SCALE) - 3.0).abs() < 1e-4);
        // a pull's whole: 14 pulses and the shot, the video's 87
        assert!((14.0 * hit_damage(Zone::Body, 1000.0, LASER_DAMAGE_SCALE) + damage_at(1000.0) - 87.0).abs() < 1e-3);
    }

    /// S mid-discharge: no pulses or shot after it, the round gone, the charge draining from where it
    /// was; a held trigger starts nothing until let go; S after the end does nothing.
    #[test]
    fn s_stops_the_discharge() {
        let mut r = Rifle::new();
        let mut ev = Vec::new();
        run(&mut r, 0.2, held(true), &mut ev);
        let at = r.now;
        for e in r.step(DT, Input { cancel: true, ..held(true) }) {
            ev.push((r.now, e));
        }
        run(&mut r, 2.0, held(true), &mut ev);
        assert_eq!(times(&ev, |e| *e == Event::Cancel).len(), 1, "{ev:?}");
        assert!(times(&ev, |e| matches!(e, Event::Fire { .. })).is_empty(), "{ev:?}");
        assert!(times(&ev, |e| matches!(e, Event::Pulse { .. })).iter().all(|t| *t <= at + 1e-4), "{ev:?}");
        assert_eq!(times(&ev, |e| *e == Event::Begin).len(), 1, "held: no new discharge");
        assert_eq!(r.ammo, CLIP - 1);
        assert!(r.frac() == 0.0, "drained 1.8 s later");
        // let go and pull: a new one at once
        run(&mut r, 2.1, held(false), &mut ev);
        run(&mut r, 2.2, held(true), &mut ev);
        assert_eq!(times(&ev, |e| *e == Event::Begin).len(), 2, "{ev:?}");
        // S between discharges: nothing
        let mut r = Rifle::new();
        assert!(r.step(DT, Input { cancel: true, ..held(false) }).is_empty());
    }

    /// The kick's ranges: pitch -1 -/+ 0..0.75 (all hard, x3), yaw -/+ 0..0.25 (all soft), roll
    /// -/+ 0.6..0.9 (soft x0.5, hard x2.6).
    #[test]
    fn view_kick_ranges() {
        let (soft, hard) = view_kick([0.0; 6]);
        assert!((soft - Vec3::new(0.0, 0.0, 0.3)).length() < 1e-5 && (hard - Vec3::new(-3.0, 0.0, 1.56)).length() < 1e-5, "{soft} {hard}");
        let (soft, hard) = view_kick([1.0; 6]);
        assert!((soft - Vec3::new(0.0, -0.25, -0.45)).length() < 1e-5, "{soft}");
        assert!((hard - Vec3::new(-1.75 * 3.0, 0.0, -0.9 * 2.6)).length() < 1e-5, "{hard}");
    }

    /// No spread (the user): none in any stance, at the hip or aimed, and no kick to it from a shot.
    #[test]
    fn no_spread() {
        let mut r = Rifle::new();
        for stance in [Stance::Moving, Stance::Sprinting, Stance::Crouched, Stance::Air, Stance::Standing] {
            for aim in [false, true] {
                r.step(0.1, Input { stance, aim, ..held(false) });
                assert_eq!(r.spread, 0.0, "{stance:?} aim {aim}");
            }
        }
        let mut spread = None;
        for _ in 0..200 {
            spread = r.step(DT, held(true)).into_iter().find_map(|e| if let Event::Fire { spread } = e { Some(spread) } else { None });
            if spread.is_some() {
                break;
            }
        }
        assert_eq!(spread.expect("a shot"), 0.0);
        assert_eq!(r.kick, 0.0);
    }
}
