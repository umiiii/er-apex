//! Apex's first-person view model (D-017): the R-301 and Fuse's first-person arms (armour model
//! 998, T011) animated by Apex's own ptpov animations (`fuse_pov.anim`, pack.rs).
//!
//! Once a frame (`step`, from firstperson.rs's frame update) the animation graph (graph.rs: the
//! sequences, cycles and weights for what Fuse does) is stepped and the pack's skeleton
//! (ptpov_rspn101, 102 bones) posed: the base sequences blended in order, the additive ones on top
//! (T006's rule: t += w * dt, r = r * slerp(1, dr, w)). Then each carrier bone
//! (tools/apexpov/carriers.json) gets
//!
//! `[inverse(jx_c_pov) . world(owner) . inverse(mesh bind of owner)]` (inches to metres) `o` ER bind
//!
//! in the frame of `jx_c_pov` (Apex's CAMERA_BASE; metres, mirrored into Elden Ring's handedness:
//! `mirror`), which firstperson.rs puts at the eye; `pose` hands out that frame's result to every
//! pose call. The rig and clips stay in Apex's units and axes.
//!
//! Apex turns the render camera by the camera bone relative to `jx_c_pov` (spec §2.4: sprint
//! ±0.75°, up to 8.9° down in the reloads): viewfx.rs does, with `drawn_camera_turn`, so on screen
//! the view model is where Apex draws it and the world turns with the animation.

mod ability;
mod graph;
mod ordnance;
mod pack;
mod padworld;
mod sway;

use std::sync::Mutex;
use std::time::Instant;

use glam::{Quat, Vec3};

pub use ability::{pick_flourish, stim_throw_at, Kind as AbilityKind, STIM_DEPLOY};
pub use graph::Moving;
pub use pack::Xf as PovXf;
use graph::{Graph, Out, Params, Pass, Signals, Weapon};
use pack::{Clip, Pack, Xf, with_pack};
use sway::{R301_HIP, R301_ZOOMED, Sway, SwayIn, angle_matrix};

use crate::{log, paths};

const INCH: f32 = 0.0254;
/// Carrier groups: 0 the arms, 1 the R-301, 2 the injector, 3 the pad, 4 the battery (T021), 5
/// the Charge Rifle, 6 the frag grenade in the hand, 7 and 8 two thrown grenades (T022), 9 the
/// Wingman (in the R-301's slot: tools/apexpov/bake_wingman.py).
const GROUPS: usize = 14;
/// The Charge Rifle's, the hand grenade's and the Wingman's groups.
const RIFLE: usize = 5;
const FRAG: usize = 6;
const WINGMAN: usize = 9;
/// The R-99's group (part LG: bake_wingman.py).
const R99G: usize = 10;
/// The kunai's group (part LG: bake_wingman.py), the holstered mode's (key 3: weapons.rs `Melee`).
const KUNAI: usize = 11;
/// The Flatline's group (part LG: bake_wingman.py).
const FLATLINEG: usize = 12;
/// The Sentinel's group (part LG: bake_wingman.py).
const SENTINELG: usize = 13;
pub const THROWN: [u8; 2] = [7, 8];

/// Where the carriers of a group that does not show go (dev `fp hide`). The renderer takes
/// pose.rs's model-space scale (2026-10-05: the R-301 shrunk where it was vanished), so all of a
/// group's carriers go to one point and every triangle of its meshes collapses there (no triangle
/// spans two groups: T020's 998 checked); the point stays near the hand, well in front of the eye.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hide {
    /// at the hand that holds it (the R-301 the right, the props the left), at least
    /// `HIDDEN_MIN_AHEAD` in front of the eye: where the prop comes back
    Hand,
    /// where the animation has it, each carrier shrunk at its own origin (wherever its ER bind puts
    /// it): triangles between those origins drew as streaks (until 15:22)
    Posed,
    /// at the eye (15:22–15:26)
    Eye,
    /// 20 m behind the eye (15:26–15:29; with T020's head and legs pieces on, the pad's toss drew
    /// dark and green blocks in 3 of 24 frames, at the hand in none)
    Behind,
}

/// Where a hidden group goes without the hand that holds it (mirrored `jx_c_pov` frame, metres:
/// x left, y up, z backward): 0.4 m ahead and 0.1 m down.
const HIDDEN_AHEAD: Vec3 = Vec3::new(0.0, -0.1, -0.4);
/// The least a hidden group's point is ahead of the eye (m): the hands come to the eye's plane
/// (the R-301's put-away, backToss: 1.5 cm behind it, offline 2026-10-05).
const HIDDEN_MIN_AHEAD: f32 = 0.3;

/// Dev knobs (`fp hide`, `fp force`, `fp flip`, `fp hold`): the hiding, a group's visibility forced
/// or toggled every n frames, an ability's clips held at one time.
struct Dev {
    hide: Hide,
    force: [Option<bool>; GROUPS],
    flip: Option<(u8, u64)>,
    hold: Option<ability::Ability>,
    frames: u64,
}

static DEV: Mutex<Dev> = Mutex::new(Dev { hide: Hide::Hand, force: [None; GROUPS], flip: None, hold: None, frames: 0 });

fn dev_state() -> std::sync::MutexGuard<'static, Dev> {
    DEV.lock().unwrap_or_else(|e| e.into_inner())
}

/// `muzzle_flash` on `def_c_base` (r301_base_v's bind pose: inches; -90° about y), for `fp trace`.
const MUZZLE: Xf = Xf { t: Vec3::new(0.0, 3.978_2, 26.367_5), r: Quat::from_xyzw(0.0, -std::f32::consts::FRAC_1_SQRT_2, 0.0, std::f32::consts::FRAC_1_SQRT_2) };

/// `muzzle_flash` on the Charge Rifle's `def_c_base` (retail `chargerifle_base_v.qc` `$definebone`:
/// inches; its rotation is not needed, only where the beam starts: hud/beam.rs).
const CR_MUZZLE: Xf = Xf { t: Vec3::new(0.0, 0.895_842, 20.380_127), r: Quat::IDENTITY };

/// `muzzle_flash` on the Wingman's `def_c_base` (retail `wingman_base_v.qc` `$definebone`: inches,
/// the R-301's -90° rotation).
const WM_MUZZLE: Xf = Xf { t: Vec3::new(0.0, 2.795_282, 9.549_05), r: MUZZLE.r };

/// `muzzle_flash` on the R-99's `def_c_base` (retail `r99_base_v.qc` `$definebone`).
const R9_MUZZLE: Xf = Xf { t: Vec3::new(0.0, 3.288_844, 17.839_268), r: MUZZLE.r };
/// `muzzle_flash` on the Flatline's `def_c_base` (retail `flatline_base_v.qc` and `flatline_v20_trshunter_v.qc` `$definebone`:
/// on `def_barrel` (0 2.3622 16.1839), 0.039064 up and 4.496695 ahead of it).
const FL_MUZZLE: Xf = Xf { t: Vec3::new(0.0, 2.401_264, 20.680_613), r: MUZZLE.r };
/// `muzzle_flash` on the Sentinel's `def_c_base` (retail `sentinel_base_v.qc` `$definebone`).
const SN_MUZZLE: Xf = Xf { t: Vec3::new(0.004_915, 5.297_734, 40.482_044), r: MUZZLE.r };

/// A weapon's muzzle on its `def_c_base` (the Charge Rifle's turn not needed: the R-301's).
fn muzzle_of(w: Weapon) -> Xf {
    match w {
        Weapon::Wingman => WM_MUZZLE,
        Weapon::R99 => R9_MUZZLE,
        Weapon::Flatline => FL_MUZZLE,
        Weapon::Sentinel => SN_MUZZLE,
        _ => MUZZLE,
    }
}

/// Whether the pack is there (then first person poses Apex's view model).
pub fn available() -> bool {
    paths::file("fuse_pov.anim").exists()
}

/// Whether the mod's package has armour model 998 (T011) for Fuse to wear.
pub fn model_installed() -> bool {
    paths::mod_dir().join("package/parts/am_m_0998.partsbnd.dcx").exists()
}

/// Whether the pack has the abilities' props (FPOV v2 carriers in groups 2 and 3: model 998's
/// head and legs pieces carry them, so those are worn too).
pub fn has_props() -> bool {
    with_pack(|p| p.carriers.iter().any(|c| c.group >= 2)).unwrap_or(false)
}

/// The carrier bones' c0000 names, in the order `pose` returns them.
pub fn carrier_names() -> Option<Vec<String>> {
    with_pack(|p| p.carriers.iter().map(|c| c.bone.clone()).collect())
}

/// The world jump pad's rig and sequences (padworld.rs), loaded once; None without the file.
fn padworld_rig() -> Option<&'static padworld::Rig> {
    static RIG: std::sync::OnceLock<Option<padworld::Rig>> = std::sync::OnceLock::new();
    RIG.get_or_init(|| {
        let path = paths::file("padworld.json");
        if !path.exists() {
            log(format!("pov: no {} (the pad on the ground stays a ring)", path.display()));
            return None;
        }
        match padworld::load(&path) {
            Ok(r) => {
                log(format!("pov: world jump pad from {} ({} bones)", path.display(), r.names.len()));
                Some(r)
            }
            Err(e) => {
                log(format!("pov: {}: {e}", path.display()));
                None
            }
        }
    })
    .as_ref()
}

/// The jump pad's carriers (group 3) for the pad standing on the ground (R4): `origin` is where
/// its `jx_c_origin` goes in model space (metres; its y the pad's up, mirrored as the rest of the
/// view model is), `age` seconds since it landed, `bounce` since it last launched someone. Each
/// carrier as `pose_pack` computes them (`world . inverse(mesh bind)`, mirrored, `. er_bind`) with
/// the world prop's pose for the hand-held pad's bones, and its scale (the pads and bladders scale
/// as it opens and bounces; along the carrier's own axes, which these binds keep aligned). Returns
/// (carrier index, translation, rotation, scale); None without the pack's pad or the world rig.
pub fn world_pad(origin: Xf, age: f32, bounce: Option<f32>) -> Option<Vec<(usize, Vec3, Quat, Vec3)>> {
    let rig = padworld_rig()?;
    let posed = rig.pose(age, bounce);
    with_pack(|p| {
        let mut out = Vec::new();
        for (i, c) in p.carriers.iter().enumerate().filter(|(_, c)| c.group == 3) {
            let owner = &p.names[c.owner];
            let name = owner.strip_prefix("jumppad:").unwrap_or(owner);
            let b = rig.names.iter().position(|n| n == name)?;
            let f = posed[b];
            // A = origin . mirror(F) . S . (mirror(inverse mesh bind) . er_bind), metres
            let g = origin.mul(mirror(Xf { t: f.xf.t * INCH, r: f.xf.r }));
            let y = mirror(Xf { t: c.inv_mesh_bind.t * INCH, r: c.inv_mesh_bind.r }).mul(c.er_bind);
            let ry = glam::Mat3::from_quat(y.r);
            let m = ry.transpose() * glam::Mat3::from_diagonal(f.scale) * ry;
            let scale = Vec3::new(m.x_axis.x, m.y_axis.y, m.z_axis.z);
            out.push((i, g.t + g.r * (f.scale * y.t), (g.r * y.r).normalize(), scale));
        }
        (!out.is_empty()).then_some(out)
    })
    .flatten()
}

/// A rigid prop's carriers (one group: a thrown grenade, T022's groups 7/8) placed in the world:
/// `origin` its model's frame in model space (metres, mirrored as the view model is). Each carrier
/// as `pose_pack` computes them with the owner at its bind (`inverse(inverse mesh bind)`): (carrier
/// index, translation, rotation, scale). None without the group in the pack.
pub fn world_rigid(group: u8, origin: Xf) -> Option<Vec<(usize, Vec3, Quat, Vec3)>> {
    with_pack(|p| {
        let out: Vec<_> = p
            .carriers
            .iter()
            .enumerate()
            .filter(|(_, c)| c.group == group)
            .map(|(i, c)| {
                let f = c.inv_mesh_bind.inverse();
                let g = origin.mul(mirror(Xf { t: f.t * INCH, r: f.r }));
                let y = mirror(Xf { t: c.inv_mesh_bind.t * INCH, r: c.inv_mesh_bind.r }).mul(c.er_bind);
                let m = g.mul(y);
                (i, m.t, m.r.normalize(), Vec3::ONE)
            })
            .collect();
        (!out.is_empty()).then_some(out)
    })
    .flatten()
}

/// Apex's camera frame (x left, y up, z forward) is right-handed, Elden Ring's model space
/// left-handed, so the mesh is stored mirrored in Z (T011, as T005's `Q`) and every Apex transform
/// is conjugated by the same mirror: `F . m . F` with F = diag(1, 1, -1).
fn mirror(m: Xf) -> Xf {
    Xf { t: mirror_point(m.t), r: Quat::from_xyzw(-m.r.x, -m.r.y, m.r.z, m.r.w) }
}

fn mirror_point(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, -p.z)
}

/// What drives the view model this frame.
pub struct Inputs {
    /// the zoom, 0..1
    pub ads: f32,
    /// shots fired so far (a change is a new shot)
    pub shots: u32,
    /// reload progress 0..1 and whether it is the empty-magazine one
    pub reload: Option<(f32, bool)>,
    /// the movement controller and the frame it is from (its events count once); None: it is off
    pub moving: Option<(Moving, u64)>,
    /// the eye's axes (world: forward, left, up), for the turn the sway reads; None: no camera
    pub eye: Option<(Vec3, Vec3, Vec3)>,
    /// velocity (world, Apex units/s) and whether on the ground, for the bob and sway; None: the
    /// controller is off
    pub velocity: Option<(Vec3, bool)>,
    /// the Charge Rifle (U3): its shots so far, its reload, seconds into its discharge, its charge
    pub cr_shots: u32,
    pub cr_reload: Option<(f32, bool)>,
    pub cr_discharge: Option<f32>,
    pub cr_charge: f32,
}

struct Posed {
    /// the carriers, as `pose` returns them
    carriers: Vec<(Vec3, Quat, Vec3, bool)>,
    /// `jx_c_camera` relative to `jx_c_pov`: how the animation turns Apex's camera
    camera_turn: Quat,
    /// `muzzle_flash` relative to `jx_c_pov` (inches; x left, y up, z forward)
    muzzle: Option<Xf>,
    /// the shown weapon's muzzle where the view model is held (the carriers' mirrored frame,
    /// metres): the Charge Rifle's when its group shows, else the R-301's when it shows
    muzzle_view: Option<Vec3>,
}

struct Anim {
    graph: Graph,
    /// the Charge Rifle's graph (T022's `cr_*` clips), stepped beside the R-301's
    cr_graph: Graph,
    cr_last_shots: Option<u32>,
    /// whose graph posed this frame
    view: Weapon,
    /// an ability's first-person clips over the graph (ability.rs), while they play
    ability: Option<ability::Ability>,
    ability_out: Option<ability::Out>,
    last_shots: Option<u32>,
    last_moving: Option<u64>,
    posed: Option<Posed>,
    /// the camera turn of the pose before this frame's: the one being drawn (the renderer takes
    /// the pose before the frame update steps the graph: firstperson.rs `DRAWN_GROUP`)
    drawn_turn: Option<Quat>,
    /// the last step's signals, for `fp trace`
    signals: Signals,
    /// Apex's bob and sway (sway.rs) and the eye's axes last frame (the turn it reads)
    sway: Sway,
    last_eye: Option<(Vec3, Vec3, Vec3)>,
    started: Instant,
    trace_until: Option<Instant>,
    /// the inspect playing (key 5): whose, seconds into it
    inspect: Option<(Weapon, f32)>,
    /// the holstered mode's kunai (its own clips over the graph's pose)
    kunai: Kunai,
    /// the kunai's inspect playing: which (`KUNAI_INSPECTS`), seconds into it
    kunai_inspect: Option<(usize, f32)>,
    /// the one a new press cut: which, seconds into it, its weight then and the seconds since
    kunai_inspect_out: Option<(usize, f32, f32, f32)>,
    /// the Sentinel's bolt after a shot: seconds into it, and the one a new shot cut (seconds into
    /// it, seconds since cut)
    rechamber: Option<f32>,
    rechamber_out: Option<(f32, f32)>,
}

/// The kunai's clips (retail `heirloom_wraith_v18_kunai_v_animRig.qc` through
/// apex-data/pov/octane_wingman/kunai_sequences.json): `idle` / `crouch` (48 frames, 30 fps, absolute
/// loops, by crouch), `sprint` (21 frames, 42 fps, absolute loop), `jump` (31 frames, additive),
/// `land` (19 frames, additive, by crouch), `melee_idle_swipe` (26 frames) for a swing, `inspect`
/// (145 frames).
#[derive(Clone, Copy, Debug, Default)]
struct Kunai {
    /// seconds of the idle loop, of the sprint loop
    idle: f32,
    sprint_t: f32,
    /// the sprint pose's weight, eased over FADE
    sprint_w: f32,
    jump: Option<f32>,
    land: Option<f32>,
    /// seconds since the controller last said sprinting (a frame or two without it keeps the pose)
    since_sprint: f32,
}

const KUNAI_FADE: f32 = 0.2;
/// How long the sprint pose stays after the controller stops saying sprinting.
const SPRINT_HOLD: f32 = 0.15;
/// The kunai's inspects (retail QC: every `ACT_VM_WEAPON_INSPECT` sequence, picked at random by its
/// activity weight as Apex does): its clip, frames (30 fps), weight, whether only crouched (the
/// `crouch` activity modifier), its sounds at their QC frames. `inspect_file` (3) and
/// `inspect_sweaty` (2) hold props the pack does not have (a folder, a towel): left out.
struct KunaiInspect {
    clip: &'static str,
    frames: u32,
    weight: u32,
    crouched: bool,
    sounds: &'static [(u32, &'static str)],
}
const KUNAI_BASIC_SOUNDS: &[(u32, &str)] = &[(1, "wraith_mvmt_kunai_inspect_basic_p1"), (43, "wraith_mvmt_kunai_inspect_basic_p2"), (124, "wraith_mvmt_kunai_inspect_basic_p3")];
const KUNAI_INSPECTS: [KunaiInspect; 5] = [
    KunaiInspect { clip: "kn_inspect_0", frames: 145, weight: 5, crouched: false, sounds: KUNAI_BASIC_SOUNDS },
    KunaiInspect {
        clip: "kn_inspect_fly_0",
        frames: 331,
        weight: 4,
        crouched: false,
        sounds: &[(0, "wraith_mvmt_kunai_inspect_fly_p1"), (107, "wraith_mvmt_kunai_inspect_fly_p2"), (208, "wraith_mvmt_kunai_inspect_fly_p3"), (263, "wraith_mvmt_kunai_inspect_fly_p4")],
    },
    KunaiInspect { clip: "kn_inspect_generic_0", frames: 145, weight: 1, crouched: false, sounds: KUNAI_BASIC_SOUNDS },
    KunaiInspect {
        clip: "kn_inspect_insignia_0",
        frames: 113,
        weight: 1,
        crouched: true,
        sounds: &[(0, "wraith_mvmt_kunai_inspect_insignia_p1"), (42, "wraith_mvmt_kunai_inspect_insignia_charged"), (58, "wraith_mvmt_kunai_inspect_insignia_p2"), (68, "wraith_mvmt_kunai_inspect_insignia_w_appears")],
    },
    // the run's (never picked at random): `drawsprint_twirl` (ACT_VM_DRAW_TO_SPRINT, the first draw's
    // twirl), key 5 on a sprint, again from its start on every press
    KunaiInspect { clip: "kn_drawsprint_twirl_0", frames: 63, weight: 0, crouched: false, sounds: &[(0, "wraith_mvmt_kunai_firstdraw")] },
];
/// `KUNAI_INSPECTS`' sprint twirl.
const KUNAI_TWIRL: usize = 4;
/// A new press's crossfade from the inspect it cuts (seconds).
const KUNAI_INSPECT_BLEND: f32 = 0.15;

fn kunai_inspect_seconds(k: usize) -> f32 {
    (KUNAI_INSPECTS[k].frames - 1) as f32 / 30.0
}

fn stop_kunai_sounds(k: usize) {
    KUNAI_INSPECTS[k].sounds.iter().for_each(|(_, n)| crate::audio::stop(n));
}

/// The inspect's layer `t` seconds in (faded in and out over 0.2 s), times `scale`.
fn kunai_inspect_layer(k: usize, t: f32, scale: f32) -> ability::Layer {
    let total = kunai_inspect_seconds(k);
    let weight = (t / 0.2).min((total - t) / 0.2).clamp(0.0, 1.0) * scale;
    ability::Layer { samples: vec![(KUNAI_INSPECTS[k].clip.into(), 1.0)], cycle: (t / total).clamp(0.0, 1.0), weight, mode: ability::Mode::Over }
}

impl Kunai {
    fn step(&mut self, dt: f32, m: &Moving) {
        self.idle += dt;
        self.sprint_t += dt;
        self.since_sprint = if m.sprinting && !m.sliding { 0.0 } else { self.since_sprint + dt };
        let want = if self.since_sprint < SPRINT_HOLD { 1.0 } else { 0.0 };
        let d = dt / KUNAI_FADE;
        // toward the wanted weight (at it: stays; `>` alone took 0.08 off every other frame, the knife shook)
        self.sprint_w = if want >= self.sprint_w { (self.sprint_w + d).min(want) } else { (self.sprint_w - d).max(want) };
        // a one-shot playing is not started again (a run over bumps lands every few frames: each
        // restart snapped the knife back to the clip's first frame)
        if m.jumped && self.jump.is_none() {
            self.jump = Some(0.0);
        }
        if m.landed && self.land.is_none() {
            self.land = Some(0.0);
        }
        self.jump = self.jump.map(|t| t + dt).filter(|t| *t < 30.0 / 30.0);
        self.land = self.land.map(|t| t + dt).filter(|t| *t < 18.0 / 30.0);
    }

    /// Its layers over the graph's pose, in order (the first one covers it whole).
    fn layers(&self, crouch: f32) -> Vec<ability::Layer> {
        use ability::{Layer, Mode};
        let c = crouch.clamp(0.0, 1.0);
        let two = |a: &str, b: &str| [(a.to_string(), 1.0 - c), (b.to_string(), c)].into_iter().filter(|s| s.1 > 0.0).collect::<Vec<_>>();
        let mut v = vec![Layer { samples: two("kn_idle_0", "kn_crouch_0"), cycle: (self.idle * 30.0 / 47.0).rem_euclid(1.0), weight: 1.0, mode: Mode::Over }];
        if self.sprint_w > 0.0 {
            v.push(Layer { samples: vec![("kn_sprint_0".into(), 1.0)], cycle: (self.sprint_t * 42.0 / 20.0).rem_euclid(1.0), weight: self.sprint_w, mode: Mode::Over });
        }
        if let Some(t) = self.jump {
            let weight = ((1.0 - t) / 0.35).clamp(0.0, 1.0);
            v.push(Layer { samples: vec![("kn_jump_0".into(), 1.0)], cycle: t.clamp(0.0, 1.0), weight, mode: Mode::Add });
        }
        if let Some(t) = self.land {
            let total = 18.0 / 30.0;
            let weight = (t / 0.05).min((total - t) / 0.2).clamp(0.0, 1.0);
            v.push(Layer { samples: two("kn_land_0", "kn_land_1"), cycle: (t / total).clamp(0.0, 1.0), weight, mode: Mode::Add });
        }
        // a swing (weapons.rs `melee_update`): `melee_idle_swipe` over it all
        if let Some(t) = super::weapons::swing_age().filter(|t| *t < 25.0 / 30.0) {
            let total = 25.0 / 30.0;
            let weight = (t / 0.05).min((total - t) / 0.15).clamp(0.0, 1.0);
            v.push(Layer { samples: vec![("kn_melee_idle_swipe_0".into(), 1.0)], cycle: (t / total).clamp(0.0, 1.0), weight, mode: Mode::Over });
        }
        v
    }
}

/// Whether the hands' slot is the holstered mode's (the kunai in hand, coming out or going away).
fn melee_slot() -> bool {
    use super::weapons::{Phase, Slot};
    match super::weapons::phase() {
        Phase::Ready(s) | Phase::Holstering { slot: s, .. } | Phase::Drawing { slot: s, .. } => s == Slot::Melee,
    }
}

static ANIM: Mutex<Option<Anim>> = Mutex::new(None);

/// The sway's input: the turn since last frame (pitch down from the world's up, Y; yaw to the left
/// about it) and the velocity in the eye's frame.
fn sway_input(dt: f32, i: &Inputs, last: Option<(Vec3, Vec3, Vec3)>) -> SwayIn {
    let mut s = SwayIn { dt, ads: i.ads, sliding: i.moving.as_ref().is_some_and(|(m, _)| m.sliding), ..Default::default() };
    let Some((f, l, u)) = i.eye else { return s };
    if let Some((pf, pl, _)) = last {
        let pitch = |v: Vec3| -v.y.clamp(-1.0, 1.0).asin().to_degrees();
        let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
        let yaw = flat(f).dot(flat(pl)).atan2(flat(f).dot(flat(pf))).to_degrees();
        s.turn = Vec3::new(pitch(f) - pitch(pf), yaw, 0.0);
    }
    if let Some((v, grounded)) = i.velocity {
        s.velocity = Vec3::new(v.dot(f), v.dot(l), v.dot(u));
        s.grounded = grounded;
    }
    s
}

/// The weapon whose view model the hands hold this frame (weapons.rs: the one going away during
/// its put-away, then the one coming out): the Charge Rifle only with its clips in the pack (T022),
/// else the R-301's slot's graph (the Wingman's with its clips in the pack) poses as before U3's
/// stage 2.
fn view_weapon(rifle: bool, primary: Weapon) -> Weapon {
    use super::weapons::{Phase, Slot};
    let slot = match super::weapons::phase() {
        Phase::Ready(s) | Phase::Holstering { slot: s, .. } | Phase::Drawing { slot: s, .. } => s,
    };
    if slot == Slot::ChargeRifle && rifle { Weapon::ChargeRifle } else { primary }
}

/// The weapon of the R-301's slot (weapons.rs `primary_gun`, the wheel's choice) when the pack has
/// it (bake_wingman.py), else the R-301.
fn primary_weapon() -> Weapon {
    use super::weapons::Gun;
    match super::weapons::primary_gun() {
        Gun::Wingman if with_pack(|p| p.has_wingman()).unwrap_or(false) => Weapon::Wingman,
        Gun::R99 if with_pack(|p| p.has_r99()).unwrap_or(false) => Weapon::R99,
        Gun::Flatline if with_pack(|p| p.has_flatline()).unwrap_or(false) => Weapon::Flatline,
        Gun::Sentinel if with_pack(|p| p.has_sentinel()).unwrap_or(false) => Weapon::Sentinel,
        _ => Weapon::R301,
    }
}

/// Once a frame: steps the animation graphs by `dt` seconds and poses the view model.
pub fn step(dt: f32, i: &Inputs) {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let a = g.get_or_insert_with(|| Anim {
        graph: Graph::new_for(primary_weapon()),
        cr_graph: Graph::new_for(Weapon::ChargeRifle),
        cr_last_shots: None,
        view: Weapon::R301,
        ability: None,
        ability_out: None,
        last_shots: None,
        last_moving: None,
        posed: None,
        drawn_turn: None,
        signals: Signals::default(),
        sway: Sway::default(),
        last_eye: None,
        started: Instant::now(),
        trace_until: None,
        inspect: None,
        kunai: Kunai::default(),
        kunai_inspect: None,
        kunai_inspect_out: None,
        rechamber: None,
        rechamber_out: None,
    });
    let mut s = sway_input(dt, i, a.last_eye);
    // the kunai's inspect on the run: the hands as when standing (no run bob or sway under it)
    if a.kunai_inspect.is_some_and(|(k, _)| k != KUNAI_TWIRL) {
        (s.velocity, s.sliding) = (Vec3::ZERO, false);
    }
    a.last_eye = i.eye;
    a.sway.step(&s, &R301_HIP, &R301_ZOOMED);
    a.drawn_turn = a.posed.as_ref().map(|p| p.camera_turn);
    let rifle = with_pack(|p| p.has_rifle()).unwrap_or(false);
    // slot 1's gun changed (the wheel): its own graph
    let primary = primary_weapon();
    if a.graph.weapon() != primary {
        a.graph = Graph::new_for(primary);
    }
    a.view = view_weapon(rifle, primary);
    let r301_shot = a.last_shots.is_some_and(|n| i.shots > n);
    a.last_shots = Some(i.shots);
    let cr_shot = a.cr_last_shots.is_some_and(|n| i.cr_shots > n);
    a.cr_last_shots = Some(i.cr_shots);
    let shot = if a.view == Weapon::ChargeRifle { cr_shot } else { r301_shot };
    let moving = i.moving.map(|(mut m, frame)| {
        // the controller's events stay up until its next step: act on them once
        if a.last_moving == Some(frame) {
            (m.jumped, m.landed, m.duck_started, m.unduck_started) = (false, false, false, false);
        }
        a.last_moving = Some(frame);
        m
    });
    // while the weapon is one-handed (the stim) the ability plays its one-handed fire, jump and
    // landing instead of the graph's two-handed ones (last frame's ability output decides)
    let onehanded = a.ability_out.as_ref().is_some_and(|o| o.onehanded > 0.5);
    let (jumped, landed) = moving.map_or((false, false), |m| (m.jumped, m.landed));
    // the Sentinel with the stim: the gun and the right hand as without it, the graph's two-handed
    // pose, shots, jumps and landings (the stim's arms take the left hand: the user's 2026-10-10
    // ask), none of the one-handed clips
    let sn_stim = onehanded && a.view == Weapon::Sentinel;
    let graph_moving = moving.map(|mut m| {
        if onehanded && !sn_stim {
            (m.jumped, m.landed) = (false, false);
        }
        // and running with it, the walk's pose, not the sprint's (the user, 2026-10-10)
        if sn_stim {
            m.sprinting = false;
        }
        m
    });
    a.signals = Signals { ads: i.ads, shot: r301_shot && (!onehanded || sn_stim), reload: i.reload, moving: graph_moving };
    a.graph.step(dt, &a.signals);
    a.cr_graph.step(dt, &Signals { ads: i.ads, shot: cr_shot && !onehanded, reload: i.cr_reload, moving: graph_moving });
    let out = if a.view == Weapon::ChargeRifle { a.cr_graph.out() } else { a.graph.out() };
    let (hold, hide, frame) = {
        let mut d = dev_state();
        d.frames += 1;
        (d.hold, d.hide, d.frames)
    };
    let m = moving.unwrap_or_default();
    // the Charge Rifle without its own clips: no weapon to pull out after the battery or the grenade
    let other_weapon = (super::weapons::active() != super::weapons::Slot::R301 && !rifle) || super::weapons::active() == super::weapons::Slot::Melee;
    let params = ability::Params { crouch: m.duck_frac, sprinting: m.sprinting, ads: i.ads, shot, jumped, landed, other_weapon };
    if let Some(h) = hold {
        a.ability = Some(h);
    } else if let Some(ab) = a.ability.as_mut() {
        ab.step(dt, params);
    }
    if hold.is_none() && a.ability.is_some_and(|ab| ab.done()) {
        a.ability = None;
    }
    a.ability_out = a.ability.map(|ab| ab.out(params));
    // the Sentinel with the stim: only the stim's own clips (the injector, the left arm) over the
    // graph's two-handed pose; its one-handed ones (the switch, idle, sprint, fire, aim) turned the
    // gun and took the right hand over to the left (the user, 2026-10-10)
    if a.view == Weapon::Sentinel
        && let Some(o) = a.ability_out.as_mut()
        && (o.show_stim || o.onehanded > 0.0)
    {
        o.layers.retain(|l| l.samples.first().is_none_or(|s| s.0.starts_with("stim_")));
    }
    // U9: the frag grenade out (spike/grenade.rs) takes the hands, over any ability
    if let Some(v) = crate::spike::grenade::view() {
        a.ability_out = Some(ordnance::out(v, params));
    }
    let mut show = groups_shown(a.ability_out.as_ref(), frame, a.view);
    // the weapon switch (weapons.rs): the R-301's or the Charge Rifle's put-away and pull-out over
    // its graph; without the rifle's clips the R-301's arms stay lowered at its put-away's end
    let swap = weapon_swap(a.view, rifle, m.duck_frac);
    let gun_group = gun_group(a.view);
    if swap.as_ref().is_some_and(|v| !v.2) && dev_state().force[gun_group].is_none() {
        show[gun_group] = false;
    }
    // the inspect (key 5): cut by a shot, aiming, a reload, a switch, sprint, an ability or another
    // weapon in the hands (推断: Apex's inspect ends on any of them)
    let busy = shot || i.ads > 0.02 || i.reload.is_some() || i.cr_reload.is_some() || swap.is_some() || a.ability_out.is_some() || m.sprinting;
    let inspect_layer = match a.inspect {
        Some((w, t)) if w == a.view && !busy && t < inspect_seconds(w) => {
            a.inspect = Some((w, t + dt));
            Some(inspect_layer(w, t))
        }
        Some((w, t)) => {
            a.inspect = None;
            if t < inspect_seconds(w) {
                inspect_sounds(w).iter().for_each(|(_, n)| crate::audio::stop(n));
            }
            None
        }
        None => None,
    };
    let swapping = swap.is_some();
    let mut posing = with_swap(a.ability_out.as_ref(), swap.map(|v| (v.0, v.1)));
    if let Some(l) = inspect_layer {
        posing.get_or_insert_with(|| ability::Out { show_gun: true, ..Default::default() }).layers.insert(0, l);
    }
    // the holstered mode (key 3): the kunai's clips under the switch and the abilities, no gun shown
    let melee = melee_slot();
    a.kunai.step(dt, &m);
    show[KUNAI] = false;
    if melee {
        let d = dev_state();
        for g in [1, RIFLE, WINGMAN, R99G, FLATLINEG, SENTINELG] {
            if d.force[g].is_none() {
                show[g] = false;
            }
        }
        let gun_shown = a.ability_out.as_ref().is_none_or(|o| o.show_gun);
        let away = matches!(super::weapons::phase(), super::weapons::Phase::Holstering { cycle, .. } if cycle >= 1.0);
        show[KUNAI] = d.force[KUNAI].unwrap_or(gun_shown && !away);
        drop(d);
        // (with the ability's layers: the battery's hands and clips stay over the kunai)
        let o = posing.get_or_insert_with(|| a.ability_out.clone().unwrap_or(ability::Out { show_gun: true, ..Default::default() }));
        let mut layers = a.kunai.layers(m.duck_frac);
        // its inspect (key 5) over the loops, cut as the guns' is
        // (not sprint: the kunai is inspected on the run too, the user's 2026-10-09 ask)
        let kunai_busy = swapping || a.ability_out.is_some() || super::weapons::swing_age().is_some_and(|t| t < 1.0);
        // the one a new press cut, fading out under the new one
        a.kunai_inspect_out = match a.kunai_inspect_out {
            Some((k, t, w, s)) if !kunai_busy && s < KUNAI_INSPECT_BLEND && t < kunai_inspect_seconds(k) => {
                let mut l = kunai_inspect_layer(k, t, 1.0);
                l.weight = w * (1.0 - s / KUNAI_INSPECT_BLEND);
                layers.push(l);
                Some((k, t + dt, w, s + dt))
            }
            _ => None,
        };
        a.kunai_inspect = match a.kunai_inspect {
            Some((k, t)) if !kunai_busy && t < kunai_inspect_seconds(k) => {
                layers.push(kunai_inspect_layer(k, t, 1.0));
                Some((k, t + dt))
            }
            Some((k, t)) => {
                // cut short: its sounds end with it (played out: they ring on)
                if t < kunai_inspect_seconds(k) {
                    stop_kunai_sounds(k);
                }
                None
            }
            None => None,
        };
        for (k, l) in layers.into_iter().enumerate() {
            o.layers.insert(k, l);
        }
    } else {
        a.kunai_inspect_out = None;
        if let Some((k, _)) = a.kunai_inspect.take() {
            stop_kunai_sounds(k);
        }
    }
    // the Sentinel's bolt (`rechamber`, additive) after each shot, sped up to the mod's shot every
    // 0.8 s; a new shot cuts it, the old one fading out under the new. With the stim, the
    // two-handed one too, under the stim's arms (its `rechamber_onehanded` is not a delta on this
    // pose: the right hand 60 units off, the gun 40° turned)
    if a.view == Weapon::Sentinel && !melee {
        if shot {
            a.rechamber_out = a.rechamber.map(|t| (t, 0.0));
            a.rechamber = Some(0.0);
        }
        let mut layers = Vec::new();
        if let Some((t, s)) = a.rechamber_out {
            let mut l = rechamber_layer(t, i.ads, m.duck_frac);
            l.weight *= 1.0 - s / RECHAMBER_BLEND;
            layers.push(l);
            a.rechamber_out = Some((t + dt, s + dt)).filter(|(_, s)| *s < RECHAMBER_BLEND);
        }
        if let Some(t) = a.rechamber {
            layers.push(rechamber_layer(t, i.ads, m.duck_frac));
            for (frame, name) in RECHAMBER_SOUNDS {
                let at = *frame as f32 / 63.0 * RECHAMBER_SECONDS;
                if t <= at && at < t + dt {
                    crate::audio::play(name, 0.7);
                }
            }
            a.rechamber = Some(t + dt).filter(|t| *t < RECHAMBER_SECONDS);
        }
        if !layers.is_empty() {
            let o = posing.get_or_insert_with(|| a.ability_out.clone().unwrap_or(ability::Out { show_gun: true, ..Default::default() }));
            // first: the stim's arms over it keep the left hand on the injector
            o.layers.splice(0..0, layers);
        }
    } else {
        (a.rechamber, a.rechamber_out) = (None, None);
    }
    // the Charge Rifle's discharge: `sustained_discharge` and its `charge_loop_layer` added on
    if a.view == Weapon::ChargeRifle
        && let Some(t) = i.cr_discharge
    {
        // (with the ability's layers: the battery's hands and clips stay over the kunai)
        let o = posing.get_or_insert_with(|| a.ability_out.clone().unwrap_or(ability::Out { show_gun: true, ..Default::default() }));
        o.layers.extend(discharge_layers(t, i.cr_charge, i.ads, m.duck_frac));
    }
    let t0 = Instant::now();
    let vis = Vis { show, hide, weapon: a.view };
    MELEE_POSE.store(melee, std::sync::atomic::Ordering::Relaxed);
    SN_STIM_POSE.store(sn_stim, std::sync::atomic::Ordering::Relaxed);
    a.posed = with_pack(|p| pose_pack(p, &out, &a.sway, posing.as_ref().or(a.ability_out.as_ref()), vis)).flatten();
    let us = t0.elapsed().as_secs_f32() * 1e6;
    if a.trace_until.is_some_and(|t| Instant::now() < t) {
        let layers = posing.as_ref().map_or(String::new(), |o| o.layers.iter().map(|l| format!("{}{:?} {:.2}@{:.2}", l.samples.first().map_or("", |s| s.0.as_str()), l.mode, l.weight, l.cycle)).collect::<Vec<_>>().join(", "));
        log(format!("{} | kunai {} sprint_w {:.2} since {:.2} | layers [{layers}]", trace_line(a, &out, us), melee, a.kunai.sprint_w, a.kunai.since_sprint));
    }
}

/// A weapon's inspect: its clip (Wingman `inspect`, 199 frames; Charge Rifle `inspect_basic`, 336
/// frames; 30 fps, absolute: bake_wingman.py), and its sounds at their QC frames.
fn inspect_clip(w: Weapon) -> Option<(&'static str, u32)> {
    match w {
        Weapon::Wingman => Some(("wm_inspect_0", 199)),
        Weapon::ChargeRifle => Some(("cr_inspect_basic_0", 336)),
        // T012's `inspect_basic` and the R-99's `inspect_new`
        Weapon::R301 => Some(("inspect_basic_0", 336)),
        Weapon::R99 => Some(("r9_inspect_new_0", 316)),
        // `ptpov_vinson.qc` inspect_basic_new
        Weapon::Flatline => Some(("fl_inspect_basic_new_0", 336)),
        // `sentinel_base_v_animRig.qc` inspect
        Weapon::Sentinel => Some(("sn_inspect_0", 336)),
    }
}

fn inspect_seconds(w: Weapon) -> f32 {
    inspect_clip(w).map_or(0.0, |(_, n)| (n - 1) as f32 / 30.0)
}

fn inspect_sounds(w: Weapon) -> &'static [(u32, &'static str)] {
    match w {
        // `wingman_base_v_animRig.qc` inspect
        Weapon::Wingman => &[
            (0, "weapon_wingman_inspect_part01"),
            (24, "weapon_wingman_inspect_part02"),
            (77, "weapon_wingman_inspect_part03"),
            (130, "weapon_wingman_inspect_part04"),
            (158, "weapon_wingman_inspect_end"),
        ],
        // `chargerifle_base_v_animRig.qc` inspect_basic
        Weapon::ChargeRifle => &[(4, "weapon_inspect_sniper_start"), (91, "weapon_inspect_sniper_mid"), (234, "weapon_inspect_sniper_mid"), (315, "weapon_inspect_sniper_end")],
        // `r99_base_v_animRig.qc` inspect_new (the R-301's: none exported)
        Weapon::R99 => &[(0, "weapon_r97_inspect")],
        Weapon::Flatline => &[(0, "weapon_vinson_inspect_basicnew")],
        // the Charge Rifle's sniper inspect sounds, at the Sentinel's QC frames
        Weapon::Sentinel => &[(4, "weapon_inspect_sniper_start"), (91, "weapon_inspect_sniper_mid"), (234, "weapon_inspect_sniper_mid"), (315, "weapon_inspect_sniper_end")],
        Weapon::R301 => &[],
    }
}

/// The inspect's layer `t` seconds in: over the graph, faded in and out over FADE-like 0.2 s.
fn inspect_layer(w: Weapon, t: f32) -> ability::Layer {
    let (name, _) = inspect_clip(w).unwrap_or(("", 2));
    let total = inspect_seconds(w).max(1e-3);
    let weight = (t / 0.2).min((total - t) / 0.2).clamp(0.0, 1.0);
    ability::Layer { samples: vec![(name.to_string(), 1.0)], cycle: (t / total).clamp(0.0, 1.0), weight, mode: ability::Mode::Over }
}

/// Key 5: the weapon in the hands plays its inspect (again from the start when it is playing).
pub fn start_inspect() -> String {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(a) = g.as_mut() else { return "inspect: no view model".into() };
    if melee_slot() {
        // on the run: the sprint twirl, from its start on every press (the user's 2026-10-09 ask, as
        // Apex does on a sprint)
        let twirl = a.signals.moving.is_some_and(|m| m.sprinting && !m.sliding);
        // Apex: a random one of them by weight, every press (again while one plays: the new one
        // crossfades over it)
        let crouched = a.signals.moving.is_some_and(|m| m.crouched);
        let ok: Vec<usize> = (0..KUNAI_INSPECTS.len())
            .filter(|&k| KUNAI_INSPECTS[k].crouched <= crouched && with_pack(|p| p.clip(KUNAI_INSPECTS[k].clip).is_some()).unwrap_or(false))
            .collect();
        let total: u32 = ok.iter().map(|&k| KUNAI_INSPECTS[k].weight).sum();
        if total == 0 {
            return "inspect: no kunai inspect in the pack".into();
        }
        let mut roll = ((a.started.elapsed().as_nanos() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % total as u64;
        let k = if twirl { KUNAI_TWIRL } else { ok
            .iter()
            .copied()
            .find(|&k| {
                let w = KUNAI_INSPECTS[k].weight as u64;
                if roll < w {
                    true
                } else {
                    roll -= w;
                    false
                }
            })
            .unwrap_or(ok[0]) };
        if let Some((old, t)) = a.kunai_inspect.take() {
            stop_kunai_sounds(old);
            let w = kunai_inspect_layer(old, t, 1.0).weight;
            a.kunai_inspect_out = Some((old, t, w, 0.0));
        }
        a.kunai_inspect = Some((k, 0.0));
        for &(frame, n) in KUNAI_INSPECTS[k].sounds {
            crate::audio::play_in(n, 0.5, frame as f32 / 30.0);
        }
        return format!("inspect: {} ({:.1} s)", KUNAI_INSPECTS[k].clip, kunai_inspect_seconds(k));
    }
    let w = a.view;
    let Some((name, _)) = inspect_clip(w) else { return "inspect: none for this weapon".into() };
    if with_pack(|p| p.clip(name).is_none()).unwrap_or(true) {
        return format!("inspect: {name} not in the pack");
    }
    if let Some((old, _)) = a.inspect {
        inspect_sounds(old).iter().for_each(|(_, n)| crate::audio::stop(n));
    }
    a.inspect = Some((w, 0.0));
    for &(frame, n) in inspect_sounds(w) {
        crate::audio::play_in(n, 0.5, frame as f32 / 30.0);
    }
    format!("inspect: {name} ({:.1} s)", inspect_seconds(w))
}

/// The carriers' transforms in the mirrored frame of `jx_c_pov` (x left, y up, z backward: see
/// `mirror`; metres), in the order of `carrier_names`, each with where its Apex bone is (for debug
/// marks: a carrier's own origin is wherever its ER bind puts it, often far from the bone it
/// carries); as this frame's `step` posed them.
pub fn pose() -> Option<Vec<(Vec3, Quat, Vec3, bool)>> {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.posed.as_ref().map(|p| p.carriers.clone())
}

/// The shown weapon's muzzle in the carriers' mirrored `jx_c_pov` frame (metres), as this frame's
/// `step` posed it (firstperson.rs puts that frame at the eye: `muzzle_world`).
pub fn muzzle_view() -> Option<Vec3> {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.posed.as_ref()?.muzzle_view
}

/// Starts an ability's first-person clips (octane.rs: the stim with the flourish it picked, the
/// pad's toss). None without the view model.
pub fn start_ability(kind: AbilityKind) -> Option<String> {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let a = g.as_mut()?;
    let ab = ability::Ability::new(kind);
    let line = format!("pov: ability {}", ab.describe());
    a.ability = Some(ab);
    Some(line)
}

/// The held stim's injector thrown now (gun.rs: a reload wants the left hand): its flourish and the
/// seconds to the throw's start. None when no injector is held.
pub fn throw_stim() -> Option<(usize, f32)> {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let ab = g.as_mut()?.ability.as_mut()?;
    let AbilityKind::Stim(flourish) = ab.kind else { return None };
    ab.throw_now().map(|delay| (flourish, delay))
}

/// The shield battery's use was cancelled (battery.rs): its clips put it away now.
pub fn cancel_battery() {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ab) = g.as_mut().and_then(|a| a.ability.as_mut()) {
        ab.cancel_battery();
    }
}

/// Whether the R-301 is away for an ability (gun.rs: no shots, no reload).
pub fn gun_away() -> bool {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|a| a.ability_out.as_ref()).is_some_and(|o| o.gun_away)
}

/// Whether an ability's clips (the stim, the pad's toss) are playing: the grenade waits for them.
pub fn ability_playing() -> bool {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|a| a.ability.is_some())
}

/// Whether the left hand is busy with an ability (the injector held): gun.rs does not reload.
pub fn left_busy() -> bool {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|a| a.ability_out.as_ref()).is_some_and(|o| o.left_busy)
}

/// How the animation turns Apex's camera in the pose being drawn (`jx_c_camera` relative to
/// `jx_c_pov`), as Source's view angles: pitch (down), yaw (left) and roll (left side up), degrees.
pub fn drawn_camera_turn() -> Vec3 {
    let g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    // `angles` gives vmparse.py's roll, the opposite of Source's
    g.as_ref().and_then(|a| a.drawn_turn).map_or(Vec3::ZERO, |q| angles(flu(q * Vec3::Z), flu(q * Vec3::Y)) * Vec3::new(1.0, 1.0, -1.0))
}

/// Whether `fp trace` is on (firstperson.rs adds the camera effects' line).
pub fn tracing() -> bool {
    ANIM.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|a| a.trace_until.is_some_and(|t| Instant::now() < t))
}

/// A sequence's samples at `cycle` for bone `b`, blended by their weights (the additive ones each
/// scaled by the bone's weights first).
fn blend(clips: &[(&Clip, f32)], b: usize, cycle: f32, nb: usize, additive: bool) -> Xf {
    let mut x = Xf::IDENTITY;
    let mut sum = 0.0;
    for &(c, w) in clips {
        let mut s = c.sample(b, cycle, nb);
        if additive {
            let (wt, wr) = c.weights[b];
            s = Xf { t: s.t * wt, r: Quat::IDENTITY.slerp(s.r, wr) };
        }
        sum += w;
        x = if sum == w { s } else { x.lerp(s, w / sum) };
    }
    x
}

/// The R-301's `viewmodel_offset_hip` / `_ads` (apex-data/export/weapon/mp_weapon_rspn101.txt;
/// right, forward, up, units), the 2 units Apex moves every view model back, and Fuse's
/// `viewmodelDuckOffset` (pilot_survival_fuse_medium.json).
const OFFSET_HIP: Vec3 = Vec3::new(0.0, 1.1, 0.4);
const OFFSET_ADS: Vec3 = Vec3::new(0.0, 0.38, 0.0);
/// The Charge Rifle's (`mp_weapon_defender_sustained.txt`: `viewmodel_offset_hip` "0 -3 -0.75",
/// `viewmodel_offset_ads` "0 -8 -0.45").
const CR_OFFSET_HIP: Vec3 = Vec3::new(0.0, -3.0, -0.75);
const CR_OFFSET_ADS: Vec3 = Vec3::new(0.0, -8.0, -0.45);
/// The Wingman's (`mp_weapon_wingman.txt`: no `viewmodel_offset_hip` (0 0 0), `viewmodel_offset_ads`
/// "0 1.0 0").
const WM_OFFSET_HIP: Vec3 = Vec3::ZERO;
const WM_OFFSET_ADS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const OFFSET_BACK: f32 = -2.0;
const DUCK_OFFSET: f32 = -0.751;

/// Where Apex holds the view model's root (`jx_c_pov`) from the eye (spec §3: 0x14062A940,
/// 0x14062A350), in `jx_c_pov`'s axes (x left, y up, z forward), units: the hip and aiming offsets
/// eased by (1 - cos πa)/2, 2 units back, and the duck offset by sin(crouchFraction π/2) at the
/// hip. R5R measured (forward -0.90, up +0.40) at the hip and (-1.62, 0) aiming.
fn hold_offset(p: Params, w: Weapon) -> Vec3 {
    let a = p.ads.clamp(0.0, 1.0);
    let e = (1.0 - (std::f32::consts::PI * a).cos()) / 2.0;
    let (hip, ads) = match w {
        Weapon::R301 => (OFFSET_HIP, OFFSET_ADS),
        Weapon::ChargeRifle => (CR_OFFSET_HIP, CR_OFFSET_ADS),
        Weapon::Wingman => (WM_OFFSET_HIP, WM_OFFSET_ADS),
        // the R-99's (`mp_weapon_r97.txt`: `viewmodel_offset_ads` "0 0 0", no hip one)
        Weapon::R99 => (Vec3::ZERO, Vec3::ZERO),
        // the Flatline's (`mp_weapon_vinson.txt`: `viewmodel_offset_hip` "0 -0.5 -0.3", `_ads` "0 0.04 0")
        Weapon::Flatline => (Vec3::new(0.0, -0.5, -0.3), Vec3::new(0.0, 0.04, 0.0)),
        // the Sentinel's (`mp_weapon_sentinel.txt`: `viewmodel_offset_ads` "0 0.0 0", no hip one)
        Weapon::Sentinel => (Vec3::ZERO, Vec3::ZERO),
    };
    let o = hip.lerp(ads, e) + Vec3::new(0.0, OFFSET_BACK, 0.0);
    let duck = DUCK_OFFSET * (p.crouch.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin() * (1.0 - a);
    // (right, forward, up) -> (left, up, forward)
    Vec3::new(-o.x, o.z + duck, o.y)
}

/// A pass with its samples' clips and weights: clips, cycle, weight.
type Resolved<'a> = (Vec<(&'a Clip, f32)>, f32, f32);

/// Apex's CAMERA_BASE attachment on `jx_c_pov` (r301_base_v.qc: rotate -90 -90 0): the view
/// model's own space in Source's axes (x forward, y left, z up), the space the sway works in.
fn camera_base() -> Xf {
    Xf { t: Vec3::ZERO, r: Quat::from_mat3(&angle_matrix(Vec3::new(-90.0, -90.0, 0.0))) }
}

/// The sway's pivots in CAMERA_BASE's space (r301_base_v.qc): `SWAY_ROTATE` on `weapon_bone`
/// (rotate -90 90 0) and `SWAY_ROTATE_ZOOMED` 196.85 units out along `muzzle_flash` (rotate 0 0 90).
fn sway_pivots(p: &Pack, world: &[Xf], w: Weapon) -> Option<((Vec3, Quat), (Vec3, Quat))> {
    let base_inv = camera_base().inverse().mul(world[p.pov].inverse());
    let at = |b: usize, local: Xf| {
        let x = base_inv.mul(world[b]).mul(local);
        (x.t, x.r)
    };
    let (gun, weapon) = p.gun_bones(w);
    let hip = at(weapon?, Xf { t: Vec3::ZERO, r: Quat::from_mat3(&angle_matrix(Vec3::new(-90.0, 90.0, 0.0))) });
    let zoomed = at(gun?, muzzle_of(w).mul(Xf { t: Vec3::new(196.85, 0.0, 0.0), r: Quat::from_mat3(&angle_matrix(Vec3::new(0.0, 0.0, 90.0))) }));
    Some((hip, zoomed))
}

/// An ability's layer for bone `b`: its samples (by name) at its cycle, blended by their weights;
/// with the bone's weights in the first sample (the QC weight list). None: a clip is missing.
fn blend_named(p: &Pack, l: &ability::Layer, b: usize, nb: usize, weapon: Weapon) -> Option<(Xf, (f32, f32))> {
    let mut x = Xf::IDENTITY;
    let mut sum = 0.0;
    let mut bone_w = (1.0, 1.0);
    for (i, (name, w)) in l.samples.iter().enumerate() {
        let c = clip_for(p, name, weapon)?;
        if i == 0 {
            bone_w = c.weights.get(b).copied().unwrap_or((1.0, 1.0));
        }
        let s = c.sample(b, l.cycle, nb);
        sum += w;
        x = if sum == *w { s } else { x.lerp(s, w / sum) };
    }
    Some((x, bone_w))
}

/// A layer's clip by name: with the Charge Rifle in the hands a weapon clip (the R-301's names the
/// abilities use: `holster`, `draw`, `switch_to_onehanded`, `fire_onehanded`...) is the rifle's own
/// `cr_*` when the pack has it (its frames played at the R-301's times: 近似); the props' clips
/// (`stim_`, `pad_`, `battery_`, `frag_`) and the rifle's as they are.
fn clip_for<'a>(p: &'a Pack, name: &str, w: Weapon) -> Option<&'a Clip> {
    let prop = ["stim_", "pad_", "battery_", "frag_", "cr_", "wm_", "kn_"].iter().any(|k| name.starts_with(k));
    // the kunai in the hands (the holstered mode): an ability's weapon clips are the kunai's own
    // (`kn_draw`, `kn_holster`...) or none (the gun's arms over the kunai showed a gun's grip: the
    // user, 2026-10-09, tossing the pad)
    if !prop && MELEE_POSE.load(std::sync::atomic::Ordering::Relaxed) {
        return p.clip(&format!("kn_{name}"));
    }
    if w != Weapon::R301 && !prop {
        if let Some(c) = p.clip(&format!("{}{name}", w.prefix())) {
            return Some(c);
        }
    }
    p.clip(name)
}

/// An ability's layers over the graph's pose, in order: the R-301's own clips over everything, the
/// offhand's clips bone by bone by their weight lists, the additive ones added on by them.
fn apply_ability(p: &Pack, ab: &ability::Out, local: &mut [Xf], w: Weapon) {
    let nb = local.len();
    let sn_stim = SN_STIM_POSE.load(std::sync::atomic::Ordering::Relaxed);
    let spine = p.names.iter().position(|n| n == "def_c_spineC");
    // (relative to `jx_c_pov`, which the view model is drawn from)
    let before = if sn_stim { spine.map(|s| world_of(p, local, p.pov).inverse().mul(world_of(p, local, s))) } else { None };
    for l in &ab.layers {
        for (b, x) in local.iter_mut().enumerate() {
            let Some((s, (wt, wr))) = blend_named(p, l, b, nb, w) else { break };
            *x = match l.mode {
                ability::Mode::Over => Xf { t: x.t.lerp(s.t, l.weight), r: x.r.slerp(s.r, l.weight).normalize() },
                ability::Mode::Masked => Xf { t: x.t.lerp(s.t, wt * l.weight), r: x.r.slerp(s.r, wr * l.weight).normalize() },
                ability::Mode::Add => Xf { t: x.t + s.t * (wt * l.weight), r: (x.r * Quat::IDENTITY.slerp(s.r, wr * l.weight)).normalize() },
            };
        }
    }
    // the Sentinel with the stim: the gun and the right arm back where the spine had them
    if let (Some(s), Some(before)) = (spine, before) {
        let fix = world_of(p, local, s).inverse().mul(world_of(p, local, p.pov)).mul(before);
        for (b, n) in p.names.iter().enumerate() {
            if p.parents[b] == s as i16 && (n == "def_r_clav" || n.starts_with("sn:")) {
                local[b] = fix.mul(local[b]);
            }
        }
    }
}

/// The weapon switch's layer over the weapon's graph (weapons.rs phases): the samples of its
/// `holster` (16 frames) or `draw` (by crouch) at the cycle, and whether its group shows. The R-301
/// with `r301_view_of`'s rules; the Charge Rifle (T022) its `cr_holster` / `cr_draw`; without the
/// rifle's clips the R-301's arms kept at its put-away's end (`r301_view_of`). None: no switch.
fn weapon_swap(view: Weapon, rifle: bool, crouch: f32) -> Option<(Vec<(String, f32)>, f32, bool)> {
    use super::weapons::{Phase, Slot};
    let draw = |name: &str| {
        let c = crouch.clamp(0.0, 1.0);
        [(format!("{name}_0"), 1.0 - c), (format!("{name}_1"), c)].into_iter().filter(|s| s.1 > 0.0).collect::<Vec<_>>()
    };
    if view == Weapon::ChargeRifle {
        return match super::weapons::phase() {
            Phase::Holstering { slot: Slot::ChargeRifle, cycle } => Some((vec![("cr_holster_0".to_string(), 1.0)], cycle, cycle < 1.0)),
            Phase::Drawing { slot: Slot::ChargeRifle, cycle, .. } => Some((draw("cr_draw"), cycle, true)),
            _ => None,
        };
    }
    let phase = super::weapons::phase();
    // the kunai's own put-away and pull-out (the holstered mode)
    match phase {
        Phase::Holstering { slot: Slot::Melee, cycle } => return Some((vec![("kn_holster_0".to_string(), 1.0)], cycle, cycle < 1.0)),
        Phase::Drawing { slot: Slot::Melee, cycle, .. } => return Some((draw("kn_draw"), cycle, true)),
        Phase::Ready(Slot::Melee) => return None,
        _ => {}
    }
    if rifle && !matches!(phase, Phase::Ready(Slot::R301) | Phase::Holstering { slot: Slot::R301, .. } | Phase::Drawing { slot: Slot::R301, .. }) {
        return None;
    }
    let v = super::weapons::r301_view_of(phase)?;
    let samples = if v.clip == "draw" { draw("draw") } else { vec![(format!("{}_0", v.clip), 1.0)] };
    Some((samples, v.cycle, v.shown))
}

/// A weapon switch's layer over the whole pose, under the abilities' layers. None: no switch (the
/// abilities' output as it is).
fn with_swap(ab: Option<&ability::Out>, swap: Option<(Vec<(String, f32)>, f32)>) -> Option<ability::Out> {
    let (samples, cycle) = swap?;
    let mut out = ab.cloned().unwrap_or_default();
    out.layers.insert(0, ability::Layer { samples, cycle: cycle.clamp(0.0, 1.0), weight: 1.0, mode: ability::Mode::Over });
    Some(out)
}

/// The Sentinel's `rechamber` (64 frames, additive; `sentinel_base_v_animRig.qc`) played over
/// this many seconds (retail: 2.1 s; sped up for the mod's shot every 0.8 s: ready to fire, frame
/// 50, at the next shot; the bolt's back and front at frames 12 and 22), and its QC sounds.
const RECHAMBER_SECONDS: f32 = 1.0;
const RECHAMBER_BLEND: f32 = 0.08;
const RECHAMBER_SOUNDS: &[(u32, &str)] = &[(12, "weapon_sentinel_boltback"), (12, "weapon_sentinel_boltback_layer1"), (22, "weapon_sentinel_boltfront")];

/// The bolt's layer `t` seconds in: by `ads_blend` and `crouchFraction`, in over 0.05 s, out over
/// its last 0.15 s.
fn rechamber_layer(t: f32, ads: f32, crouch: f32) -> ability::Layer {
    let (a, c) = (ads.clamp(0.0, 1.0), crouch.clamp(0.0, 1.0));
    let samples: Vec<(String, f32)> = [(0, (1.0 - a) * (1.0 - c)), (1, a * (1.0 - c)), (2, (1.0 - a) * c), (3, a * c)]
        .into_iter()
        .filter(|s| s.1 > 0.0)
        .map(|(k, w)| (format!("sn_rechamber_{k}"), w))
        .collect();
    let weight = (t / 0.05).min((RECHAMBER_SECONDS - t) / 0.15).clamp(0.0, 1.0);
    ability::Layer { samples, cycle: (t / RECHAMBER_SECONDS).clamp(0.0, 1.0), weight, mode: ability::Mode::Add }
}

/// The Charge Rifle's `sustained_discharge` (105 frames, 30 fps, looping, by ads x crouch) and its
/// addlayer `charge_loop_layer` (by ads x chargeFraction: still at no charge, its 105-frame loop
/// at full), both added on, `t` seconds into the discharge (U3; T022's clips).
fn discharge_layers(t: f32, charge: f32, ads: f32, crouch: f32) -> [ability::Layer; 2] {
    let cycle = (t.max(0.0) * 30.0 / 104.0).rem_euclid(1.0);
    let grid = |name: &str, a: f32, b: f32| {
        let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
        [(0, (1.0 - a) * (1.0 - b)), (1, a * (1.0 - b)), (2, (1.0 - a) * b), (3, a * b)]
            .into_iter()
            .filter(|s| s.1 > 0.0)
            .map(|(k, w)| (format!("{name}_{k}"), w))
            .collect::<Vec<_>>()
    };
    let fade = (t / 0.1).clamp(0.0, 1.0);
    [
        ability::Layer { samples: grid("cr_sustained_discharge", ads, crouch), cycle, weight: fade, mode: ability::Mode::Add },
        ability::Layer { samples: grid("cr_charge_loop_layer", ads, charge), cycle, weight: fade, mode: ability::Mode::Add },
    ]
}

/// Which carrier groups show: 0 the arms, 1 the R-301 or 5 the Charge Rifle (the weapon in the
/// hands: `view`), 2 the injector, 3 the pad, 4 the battery, 6 the grenade in the hand (7 and 8,
/// the thrown ones, are placed in the world by firstperson.rs) (v1 packs: every carrier is group 0);
/// then the dev overrides (`fp force`, `fp flip`) at this frame.
fn groups_shown(ab: Option<&ability::Out>, frame: u64, view: Weapon) -> [bool; GROUPS] {
    let (gun, stim, pad, battery, frag) = match ab {
        Some(o) => (o.show_gun, o.show_stim, o.show_pad, o.show_battery, o.show_frag),
        None => (true, false, false, false, false),
    };
    let held = gun_group(view);
    let mut s = [true, gun && held == 1, stim, pad, battery, gun && held == RIFLE, frag, false, false, gun && held == WINGMAN, gun && held == R99G, false, gun && held == FLATLINEG, gun && held == SENTINELG];
    let d = dev_state();
    for (g, f) in d.force.iter().enumerate() {
        if let Some(f) = f {
            s[g] = *f;
        }
    }
    if let Some((g, n)) = d.flip {
        s[g as usize] = (frame / n.max(1)) % 2 == 0;
    }
    s
}

/// The carrier group of a weapon's model: 1 the R-301, 5 the Charge Rifle, 9 the Wingman.
fn gun_group(w: Weapon) -> usize {
    match w {
        Weapon::R301 => 1,
        Weapon::ChargeRifle => RIFLE,
        Weapon::Wingman => WINGMAN,
        Weapon::R99 => R99G,
        Weapon::Flatline => FLATLINEG,
        Weapon::Sentinel => SENTINELG,
    }
}

/// The kunai is the hands' this frame (`clip_for`).
static MELEE_POSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set before pose_pack: the Sentinel with the stim. The stim's clips turn the body (hip, spine)
/// with the one-handed sprint in mind; the Sentinel and the right arm hang off the spine, so its
/// barrel pointed left on the run (the user, 2026-10-10). Left alone, the body put the injector
/// in the right side. So the body turns as the stim has it (the left arm's stab lands), and the
/// gun and the right arm are carried back to where the body without the stim had them.
static SN_STIM_POSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A bone's world transform from local ones.
fn world_of(p: &Pack, local: &[Xf], b: usize) -> Xf {
    let mut x = local[b];
    let mut i = p.parents[b];
    while i >= 0 {
        x = local[i as usize].mul(x);
        i = p.parents[i as usize];
    }
    x
}

/// What shows this frame and how the rest is put away.
#[derive(Clone, Copy)]
struct Vis {
    show: [bool; GROUPS],
    hide: Hide,
    /// the weapon in the hands: the abilities' weapon clips are its own (`cr_*`, `wm_*`) when it has them
    weapon: Weapon,
}

fn pose_pack(p: &Pack, o: &Out, sway: &Sway, ab: Option<&ability::Out>, vis: Vis) -> Option<Posed> {
    let nb = p.parents.len();
    // each pass's samples with their weights, found once
    let resolve = |pass: &Pass| -> Option<Resolved> {
        let d = pass.seq.def_in(o.w);
        let w = d.weights(o.params);
        let clips = (0..d.samples.len()).filter(|k| w[*k] > 0.0).map(|k| Some((p.sample_in(pass.seq, k, o.w)?, w[k]))).collect::<Option<Vec<_>>>()?;
        Some((clips, pass.cycle, pass.weight))
    };
    let base: Vec<_> = o.base.iter().filter_map(resolve).collect();
    let add: Vec<_> = o.add.iter().filter(|a| a.weight > 0.0).filter_map(resolve).collect();
    let (first, over) = base.split_first()?;
    let mut local = Vec::with_capacity(nb);
    for b in 0..nb {
        let mut x = blend(&first.0, b, first.1, nb, false);
        for (clips, cycle, w) in over {
            x = x.lerp(blend(clips, b, *cycle, nb, false), *w);
        }
        for (clips, cycle, w) in &add {
            let d = blend(clips, b, *cycle, nb, true);
            x.t += d.t * *w;
            x.r = (x.r * Quat::IDENTITY.slerp(d.r, *w)).normalize();
        }
        local.push(x);
    }
    if let Some(ab) = ab {
        apply_ability(p, ab, &mut local, vis.weapon);
    }
    let mut world: Vec<Xf> = Vec::with_capacity(nb);
    for (&l, &parent) in local.iter().zip(&p.parents) {
        world.push(if parent >= 0 { world[parent as usize].mul(l) } else { l });
    }
    let pov_inv = world[p.pov].inverse();
    // the bob and sway matrix (CAMERA_BASE's axes), in `jx_c_pov`'s
    let base = camera_base();
    let swayed = sway_pivots(p, &world, o.w).map_or(Xf::IDENTITY, |(hip, zoomed)| {
        let (r, t) = sway.matrix(hip, zoomed);
        base.mul(Xf { t, r }).mul(base.inverse())
    });
    // the root where Apex holds it from the eye, put through the sway in its own space
    let held = Xf { t: hold_offset(o.params, o.w), r: Quat::IDENTITY }.mul(swayed).mul(pov_inv);
    // where each hidden group collapses (`Hide`)
    let hand = |name: &str| {
        let at = p.carriers.iter().find(|c| c.bone == name).map(|c| mirror_point(held.mul(world[c.owner]).t * INCH))?;
        Some(Vec3::new(at.x, at.y, at.z.min(-HIDDEN_MIN_AHEAD)))
    };
    let (left, right) = (hand("L_Hand").unwrap_or(HIDDEN_AHEAD), hand("R_Hand").unwrap_or(HIDDEN_AHEAD));
    let carriers = p
        .carriers
        .iter()
        .map(|c| {
            let bone = held.mul(world[c.owner]);
            let at = mirror_point(bone.t * INCH);
            let shown = vis.show.get(c.group as usize).copied().unwrap_or(true);
            let m = bone.mul(c.inv_mesh_bind);
            let m = mirror(Xf { t: m.t * INCH, r: m.r }).mul(c.er_bind);
            match (shown, vis.hide) {
                (true, _) | (false, Hide::Posed) => (m.t, m.r, at, shown),
                (false, Hide::Hand) => (if matches!(c.group as usize, 1 | RIFLE | FRAG | WINGMAN | R99G | KUNAI | FLATLINEG | SENTINELG) { right } else { left }, Quat::IDENTITY, at, false),
                (false, Hide::Eye) => (Vec3::ZERO, Quat::IDENTITY, at, false),
                (false, Hide::Behind) => (Vec3::new(0.0, 0.0, 20.0), Quat::IDENTITY, at, false),
            }
        })
        .collect();
    let shown = |g: usize| vis.show.get(g).copied().unwrap_or(false);
    let muzzle_view = match (p.cr_gun, p.wm_gun, p.r9_gun, p.gun) {
        (Some(cr), _, _, _) if shown(RIFLE) => Some(held.mul(world[cr]).mul(CR_MUZZLE)),
        (_, Some(wm), _, _) if shown(WINGMAN) => Some(held.mul(world[wm]).mul(WM_MUZZLE)),
        (_, _, Some(r9), _) if shown(R99G) => Some(held.mul(world[r9]).mul(R9_MUZZLE)),
        _ if shown(FLATLINEG) && p.fl_gun.is_some() => p.fl_gun.map(|fl| held.mul(world[fl]).mul(FL_MUZZLE)),
        _ if shown(SENTINELG) && p.sn_gun.is_some() => p.sn_gun.map(|sn| held.mul(world[sn]).mul(SN_MUZZLE)),
        (_, _, _, Some(g)) if shown(1) => Some(held.mul(world[g]).mul(MUZZLE)),
        _ => None,
    }
    .map(|m| mirror_point(m.t * INCH));
    let gun = p.gun_bones(o.w).0;
    Some(Posed { carriers, camera_turn: pov_inv.mul(world[p.camera]).r, muzzle: gun.map(|g| pov_inv.mul(world[g]).mul(muzzle_of(o.w))), muzzle_view })
}

/// Apex's camera axes (x left, y up, z forward) as (forward, left, up).
fn flu(v: Vec3) -> Vec3 {
    Vec3::new(v.z, v.x, v.y)
}

/// (pitch, yaw, roll) in degrees of an orientation with forward `f` and up `u` in a (forward, left,
/// up) frame, as tools/r5/vmparse.py `rel_angles` gives R5R's: pitch down and yaw left positive,
/// roll atan2(-left.z, up.z), positive with the left axis down (opposite to Source's view roll).
fn angles(f: Vec3, u: Vec3) -> Vec3 {
    let l = u.cross(f);
    Vec3::new(-f.z.clamp(-1.0, 1.0).asin().to_degrees(), f.y.atan2(f.x).to_degrees(), (-l.z).atan2(u.z).to_degrees())
}

/// The muzzle (position: forward, right, up, inches; angles with its x forward and z up, the axes
/// R5R's `muzzle_flash` attachment has: about -92° roll at the hip) and the camera turn (pitch,
/// yaw, roll), relative to `jx_c_pov`.
fn muzzle_and_turn(p: &Posed) -> (Option<(Vec3, Vec3)>, Vec3) {
    let muzzle = p.muzzle.map(|m| (Vec3::new(m.t.z, -m.t.x, m.t.y), angles(flu(m.r * Vec3::X), flu(m.r * Vec3::Z))));
    (muzzle, angles(flu(p.camera_turn * Vec3::Z), flu(p.camera_turn * Vec3::Y)))
}

fn trace_line(a: &Anim, o: &Out, us: f32) -> String {
    let s = &a.signals;
    let m = s.moving.unwrap_or_default();
    let ev: Vec<&str> = [(s.shot, "shot"), (m.jumped, "jump"), (m.landed, "land"), (m.duck_started, "duck"), (m.unduck_started, "unduck")].iter().filter(|e| e.0).map(|e| e.1).collect();
    let list = |v: &[Pass]| v.iter().map(|p| format!("{} {:.3}@{:.3}", p.seq.def_in(o.w).name, p.weight, p.cycle)).collect::<Vec<_>>().join(", ");
    let (muzzle, turn) = a.posed.as_ref().map_or((None, Vec3::NAN), muzzle_and_turn);
    let muzzle = muzzle.map_or("-".into(), |(t, r)| format!("{:.2} {:.2} {:.2} | {:.2} {:.2} {:.2}", t.x, t.y, t.z, r.x, r.y, r.z));
    format!(
        "fp trace: t {:.3} | ads {:.3} sprint {} slide {} crouched {} duck {:.3} speed {:.0} reload {} ev {} | base [{}] add [{}] | muzzle (f r u | p y r) {muzzle} | cam turn {:.3} {:.3} {:.3} | {} | {us:.0} µs",
        a.started.elapsed().as_secs_f32(),
        s.ads,
        m.sprinting as u8,
        m.sliding as u8,
        m.crouched as u8,
        m.duck_frac,
        m.speed,
        s.reload.map_or("-".into(), |(r, e)| format!("{r:.2}{}", if e { " empty" } else { "" })),
        if ev.is_empty() { "-".into() } else { ev.join(",") },
        list(&o.base),
        list(&o.add),
        turn.x,
        turn.y,
        turn.z,
        a.sway.describe()
    )
}

/// Dev `fp vm`: the graph's sequences and weights, the muzzle and the camera turn.
pub fn describe() -> String {
    let g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(a) = g.as_ref() else { return "pov: not stepped yet".into() };
    let (muzzle, turn) = a.posed.as_ref().map_or((None, Vec3::NAN), muzzle_and_turn);
    format!(
        "{}{} | muzzle {} | cam turn {:.2} {:.2} {:.2}",
        if a.view == Weapon::ChargeRifle { a.cr_graph.describe() } else { a.graph.describe() },
        a.ability.map_or(String::new(), |ab| format!(" | ability {}", ab.describe())),
        muzzle.map_or("-".into(), |(t, r)| format!("({:.2} {:.2} {:.2}) ({:.1} {:.1} {:.1})", t.x, t.y, t.z, r.x, r.y, r.z)),
        turn.x,
        turn.y,
        turn.z
    )
}

/// Dev `fp trace <seconds>`: every frame's signals, sequences, muzzle and camera turn in the log.
pub fn trace(seconds: f32) -> String {
    let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(a) = g.as_mut() else { return "pov: not stepped yet".into() };
    a.trace_until = Some(Instant::now() + std::time::Duration::from_secs_f32(seconds.max(0.0)));
    format!("pov: tracing every frame for {seconds} s")
}

/// Dev `fp hide hand|posed|eye|behind`, `fp force <group> show|hide|auto`, `fp flip <group>
/// <frames>|off`, `fp hold stim <flourish 0-5> <seconds> | pad <seconds> | off`, `fp play stim
/// <flourish 0-5> | pad -` (the clips only).
pub fn dev(args: &[&str]) -> String {
    let group = |s: Option<&&str>| s.and_then(|g| g.parse::<usize>().ok()).filter(|&g| g < GROUPS);
    let secs = |s: Option<&&str>| s.and_then(|t| t.parse::<f32>().ok());
    let mut d = dev_state();
    match args {
        ["hide", mode] => {
            d.hide = match *mode {
                "hand" => Hide::Hand,
                "posed" => Hide::Posed,
                "eye" => Hide::Eye,
                "behind" => Hide::Behind,
                _ => return "usage: fp hide hand|posed|eye|behind".into(),
            };
        }
        ["force", g, v] => match (group(Some(g)), *v) {
            (Some(g), "show" | "hide" | "auto") => d.force[g] = (*v != "auto").then_some(*v == "show"),
            _ => return "usage: fp force <group 0-10> show|hide|auto".into(),
        },
        ["flip", "off"] => d.flip = None,
        ["flip", g, n] => match (group(Some(g)), n.parse::<u64>().ok().filter(|&n| n > 0)) {
            (Some(g), Some(n)) => d.flip = Some((g as u8, n)),
            _ => return "usage: fp flip <group 0-10> <frames> | off".into(),
        },
        ["play", "stim", f] | ["play", "pad", f] => {
            let kind = match (args[1], f.parse::<usize>().ok()) {
                ("stim", Some(f)) if f < ability::FLOURISHES.len() => ability::Kind::Stim(f),
                ("pad", _) => ability::Kind::Pad,
                _ => return "usage: fp play stim <flourish 0-5> | pad -".into(),
            };
            drop(d);
            // only the view model's clips (no stim, no pad): for looking at them again and again
            let mut g = ANIM.lock().unwrap_or_else(|e| e.into_inner());
            let Some(a) = g.as_mut() else { return "pov: no view model".into() };
            let ab = ability::Ability::new(kind);
            let line = format!("pov dev: playing {}", ab.describe());
            a.ability = Some(ab);
            return line;
        }
        ["hold", "off"] => d.hold = None,
        ["hold", "stim", f, t] => match (f.parse::<usize>().ok().filter(|&f| f < ability::FLOURISHES.len()), secs(Some(t))) {
            (Some(f), Some(t)) => d.hold = Some(ability::Ability::at(ability::Kind::Stim(f), t)),
            _ => return "usage: fp hold stim <flourish 0-5> <seconds>".into(),
        },
        ["hold", "pad", t] => match secs(Some(t)) {
            Some(t) => d.hold = Some(ability::Ability::at(ability::Kind::Pad, t)),
            None => return "usage: fp hold pad <seconds>".into(),
        },
        _ => return "usage: fp hide <mode> | force <g> show|hide|auto | flip <g> <frames>|off | hold stim <f> <s>|pad <s>|off".into(),
    }
    format!("pov dev: hide {:?}, force {:?}, flip {:?}, hold {}", d.hide, d.force, d.flip, d.hold.map_or("off".into(), |h| h.describe()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The muzzle's angle convention against R5R (spec §2.2: about -92° roll at the hip, -45° in
    /// sprint, -37° crouched; measured offline from the pack 2026-10-05: -96°, -45°, -36.8°).
    #[test]
    fn angles_like_vmparse() {
        // level: no turn
        let a = angles(Vec3::X, Vec3::Z);
        assert!(a.length() < 1e-4, "{a}");
        // the muzzle at rest: x forward, z right (the bone's up is horizontal)
        let m = MUZZLE.r;
        let a = angles(flu(m * Vec3::X), flu(m * Vec3::Z));
        assert!((a.z.abs() - 90.0).abs() < 1e-3 && a.x.abs() < 1e-3, "{a}");
        // a camera pitched down 5° about its left axis (Apex's x)
        let r = Quat::from_rotation_x(5f32.to_radians());
        let a = angles(flu(r * Vec3::Z), flu(r * Vec3::Y));
        assert!((a.x - 5.0).abs() < 1e-3 && a.y.abs() < 1e-3 && a.z.abs() < 1e-3, "{a}");
    }

    /// A9's offsets (spec §3): R5R's measured (forward -0.90, up +0.40) at the hip, (-1.62, 0)
    /// aiming; crouched at the hip 0.751 lower, aiming not.
    #[test]
    fn hold_offsets() {
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-4;
        let p = Params::default();
        assert!(near(hold_offset(p, Weapon::R301), Vec3::new(0.0, 0.4, -0.9)), "{}", hold_offset(p, Weapon::R301));
        assert!(near(hold_offset(Params { ads: 1.0, ..p }, Weapon::R301), Vec3::new(0.0, 0.0, -1.62)));
        assert!(near(hold_offset(Params { crouch: 1.0, ..p }, Weapon::R301), Vec3::new(0.0, 0.4 - 0.751, -0.9)));
        assert!(near(hold_offset(Params { crouch: 1.0, ads: 1.0, ..p }, Weapon::R301), Vec3::new(0.0, 0.0, -1.62)));
        // half way the cosine is half way
        let h = hold_offset(Params { ads: 0.5, ..p }, Weapon::R301);
        assert!(near(h, Vec3::new(0.0, 0.2 - 0.751 * 0.0, (-0.9 - 1.62) / 2.0)), "{h}");
    }

    /// A5 offline: the camera bone's turn (vmparse's angles, as `fp trace`) through sprint, a slide
    /// out of it and a duck, against R5R (spec 2.4): sprint pitch -0.59..+0.28°, yaw -0.73..+0.62°;
    /// slide start pitch -0.7° -> +0.8° in 0.4 s; ducking roll -0.7°, pitch +0.42°, yaw +0.09°.
    #[test]
    fn camera_turn_like_r5r() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        const DT: f32 = 1.0 / 60.0;
        let turn = |g: &Graph| muzzle_and_turn(&pose_pack(&p, &g.out(), &Sway::default(), None, Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap()).1;
        let range = |v: &[Vec3], k: usize| v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t[k]), hi.max(t[k])));
        let moving = |f: &dyn Fn(&mut graph::Moving)| {
            let mut m = graph::Moving::default();
            f(&mut m);
            Signals { moving: Some(m), ..Default::default() }
        };

        let mut g = Graph::new();
        let mut sprint = Vec::new();
        for i in 0..120 {
            g.step(DT, &moving(&|m| m.sprinting = true));
            if i >= 18 {
                sprint.push(turn(&g));
            }
        }
        let (p_lo, p_hi) = range(&sprint, 0);
        let (y_lo, y_hi) = range(&sprint, 1);
        assert!((p_lo + 0.59).abs() < 0.05 && (p_hi - 0.28).abs() < 0.05, "sprint pitch {p_lo} {p_hi}");
        assert!((y_lo + 0.73).abs() < 0.05 && (y_hi - 0.62).abs() < 0.05, "sprint yaw {y_lo} {y_hi}");

        // the slide: sprint ends, the slide and the duck start in one step; crouchFraction goes to 1
        // in 0.2 s (a fast duck)
        let mut slide = Vec::new();
        for i in 0..30 {
            let f = ((i + 1) as f32 * DT / 0.2).min(1.0);
            g.step(DT, &moving(&|m| {
                m.sliding = true;
                m.crouched = true;
                m.duck_frac = f * f * (3.0 - 2.0 * f);
                m.duck_started = i == 0;
            }));
            slide.push(turn(&g));
        }
        // R5R: -0.7 -> +0.8 (spec 2.4; T2 frame by frame: -0.50 at 0.1 s, +0.81 at 0.25 s, 0 from
        // 0.45 s, roll under 0.05°). Without the duck transition on a slide (graph.rs) only the
        // current Apex `sprintslide` turns the camera: about +1.05° at its top, more than S3's; the
        // frame by frame RMS is tools/r5/vmcompare.py's (2026-10-05: pitch 0.105°, roll 0.005°)
        let (s_lo, s_hi) = range(&slide, 0);
        let (r_lo, r_hi) = range(&slide, 2);
        assert!((s_lo + 0.7).abs() < 0.2, "slide pitch low {s_lo}");
        assert!(s_hi > 0.8 && s_hi < 1.15, "slide pitch high {s_hi}");
        assert!(r_lo > -0.08 && r_hi < 0.08, "slide roll {r_lo} {r_hi}");

        // ducking standing still, from the hip
        let mut g = Graph::new();
        g.step(DT, &Signals::default());
        let mut duck = Vec::new();
        for i in 0..60 {
            let f = ((i + 1) as f32 * DT / 0.4).min(1.0);
            g.step(DT, &moving(&|m| {
                m.crouched = true;
                m.duck_frac = f * f * (3.0 - 2.0 * f);
                m.duck_started = i == 0;
            }));
            duck.push(turn(&g));
        }
        let peak = *duck.iter().max_by(|a, b| a.z.abs().total_cmp(&b.z.abs())).unwrap();
        assert!((peak.z + 0.7).abs() < 0.05 && (peak.x - 0.42).abs() < 0.05 && (peak.y - 0.09).abs() < 0.05, "duck {peak}");
    }

    /// A9 offline: R5R's T1 walk (scratch/r5anim/T1.log from 809.232: the speeds below, then 173.5,
    /// level view, no turn), the view model's origin and angles relative to the eye (the sway matrix
    /// about the pack's real pivot) peak to peak over the same 41 frames R5R gives (0.3–1.0 s after
    /// the speed reached 170): forward 0.183, right 0.406, up 0.258 units; pitch 0.982, yaw 2.803,
    /// roll 2.053° (vmparse's vm_eye / vm_eye_ang: the entity's origin, which no animation moves).
    #[test]
    fn walking_sway_like_r5r() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        let mut g = Graph::new();
        g.step(1.0 / 60.0, &Signals::default());
        let mut o = g.out();
        o.add.clear();
        // the hip pose's world transforms, for the pivots
        let nb = p.parents.len();
        let base = &o.base[0];
        let def = base.seq.def();
        let w = def.weights(o.params);
        let clips: Vec<_> = (0..def.samples.len()).filter(|k| w[*k] > 0.0).map(|k| (p.sample(base.seq, k).unwrap(), w[k])).collect();
        let mut world: Vec<Xf> = Vec::with_capacity(nb);
        for b in 0..nb {
            let l = blend(&clips, b, base.cycle, nb, false);
            world.push(if p.parents[b] >= 0 { world[p.parents[b] as usize].mul(l) } else { l });
        }
        let (hip, zoomed) = sway_pivots(&p, &world, Weapon::R301).unwrap();
        const RAMP: [f32; 11] = [0.0, 0.0, 41.67, 83.33, 125.0, 132.5, 140.0, 147.5, 155.0, 162.5, 170.0];
        let mut s = Sway::default();
        let mut frames = Vec::new();
        for i in 0..71 {
            let speed = RAMP.get(i).copied().unwrap_or(173.5);
            s.step(&SwayIn { dt: 1.0 / 60.0, velocity: Vec3::new(speed, 0.0, 0.0), grounded: true, ..Default::default() }, &R301_HIP, &R301_ZOOMED);
            if i >= 30 {
                let (r, t) = s.matrix(hip, zoomed);
                // Source axes (forward, left, up): the origin's move, and the angles as vmparse
                frames.push((t, angles(r * Vec3::X, r * Vec3::Z)));
            }
        }
        let pp = |f: &dyn Fn(&(Vec3, Vec3)) -> f32| {
            let (lo, hi) = frames.iter().map(f).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            hi - lo
        };
        let got = [pp(&|x| x.0.x), pp(&|x| x.0.y), pp(&|x| x.0.z), pp(&|x| x.1.x), pp(&|x| x.1.y), pp(&|x| x.1.z)];
        println!("walk p-p: forward {:.3} right {:.3} up {:.3} pitch {:.3} yaw {:.3} roll {:.3}", got[0], got[1], got[2], got[3], got[4], got[5]);
        let r5r = [0.183, 0.406, 0.258, 0.982, 2.803, 2.053];
        // across: the formula alone (2026-10-05: 0.405, 2.806°, 2.050°)
        for k in [1, 4, 5] {
            assert!((got[k] - r5r[k]).abs() <= 0.02 * r5r[k], "component {k}: {} vs R5R {}", got[k], r5r[k]);
        }
        // up, forward and pitch depend on where the pivot is and how it is tilted, and R5R's S3 R-301
        // view model is not the current one (spec §3.1: with this pivot squared up the pitch is 0.910,
        // with it at the eye up is 0.318; R5R lies between): this pivot gives 0.256, 0.199, 1.118
        for (k, now) in [(0, 0.256), (2, 0.199), (3, 1.118)] {
            assert!((got[k] - now).abs() < 0.005 && (got[k] - r5r[k]).abs() <= 0.45 * r5r[k], "component {k}: {}", got[k]);
        }
    }

    /// For tools/r5/vmcompare.py (A5 frame by frame): the camera bone's turn (vmparse's angles) at
    /// 60 Hz from three events, as `<name> <k> <pitch> <yaw> <roll>` lines: a sprint's start
    /// (`sprint`), a duck from standing (`duck`: crouchFraction over the 400 ms smoothstep), and a
    /// slide out of a sprint (`slide`: after 1.5 s of sprint; the fast 200 ms duck).
    /// `cargo test --release camera_turn_curves -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn camera_turn_curves() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        const DT: f32 = 1.0 / 60.0;
        let smooth = |r: f32| r * r * (3.0 - 2.0 * r);
        let turn = |g: &Graph| muzzle_and_turn(&pose_pack(&p, &g.out(), &Sway::default(), None, Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap()).1;
        let emit = |name: &str, k: usize, t: Vec3| println!("{name} {k} {:.4} {:.4} {:.4}", t.x, t.y, t.z);

        let mut g = Graph::new();
        g.step(DT, &Signals::default());
        for k in 0..150 {
            let m = graph::Moving { sprinting: true, speed: 260.0, ..Default::default() };
            g.step(DT, &Signals { moving: Some(m), ..Default::default() });
            emit("sprint", k, turn(&g));
        }
        // a slide out of a sprint as long as R5R's T2 (936.765 -> 937.832: 64 frames)
        let mut g = Graph::new();
        g.step(DT, &Signals::default());
        for _ in 0..64 {
            let m = graph::Moving { sprinting: true, speed: 260.0, ..Default::default() };
            g.step(DT, &Signals { moving: Some(m), ..Default::default() });
        }
        for k in 0..90 {
            let f = smooth(((k + 1) as f32 * DT / 0.2).min(1.0));
            let m = graph::Moving { sliding: k < 60, crouched: true, duck_frac: f, duck_started: k == 0, speed: 300.0, ..Default::default() };
            g.step(DT, &Signals { moving: Some(m), ..Default::default() });
            emit("slide", k, turn(&g));
        }

        let mut g = Graph::new();
        g.step(DT, &Signals::default());
        for k in 0..90 {
            let f = smooth(((k + 1) as f32 * DT / 0.4).min(1.0));
            let m = graph::Moving { crouched: true, duck_frac: f, duck_started: k == 0, ..Default::default() };
            g.step(DT, &Signals { moving: Some(m), ..Default::default() });
            emit("duck", k, turn(&g));
        }
    }

    /// T021's pack (T020's and the battery) through every ability, standing and sprinting: every
    /// clip an ability asks for is in it, and a hidden group's point (`Hide::Hand`) stays well in
    /// front of the eye (its motion vectors never near w = 0).
    #[test]
    fn hidden_groups_stay_ahead_of_the_eye() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/octane_battery/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        const DT: f32 = 1.0 / 60.0;
        let kinds = (0..ability::FLOURISHES.len()).map(ability::Kind::Stim).chain([ability::Kind::Pad, ability::Kind::Battery]);
        for kind in kinds {
            for sprinting in [false, true] {
                let mut g = Graph::new();
                let mut ab = ability::Ability::new(kind);
                let (mut nearest, mut hidden) = ([f32::MAX; GROUPS], 0);
                while !ab.done() {
                    let m = graph::Moving { sprinting, speed: if sprinting { 260.0 } else { 0.0 }, ..Default::default() };
                    g.step(DT, &Signals { moving: Some(m), ..Default::default() });
                    let params = ability::Params { sprinting, ..Default::default() };
                    ab.step(DT, params);
                    let out = ab.out(params);
                    for l in &out.layers {
                        for (name, _) in &l.samples {
                            assert!(p.clips.iter().any(|c| &c.name == name), "{kind:?}: clip {name} not in the pack");
                        }
                    }
                    let posed = pose_pack(&p, &g.out(), &Sway::default(), Some(&out), Vis { show: groups_shown(Some(&out), 0, Weapon::R301), hide: Hide::Hand, weapon: Weapon::R301 }).unwrap();
                    for (c, x) in p.carriers.iter().zip(&posed.carriers).filter(|(_, x)| !x.3) {
                        let g = c.group as usize;
                        nearest[g] = nearest[g].min(-x.0.z);
                        hidden += 1;
                    }
                }
                let least = nearest.iter().copied().fold(f32::MAX, f32::min);
                println!("{kind:?} sprinting {sprinting}: hidden carriers {hidden}, nearest by group {nearest:.3?} m ahead");
                assert!(hidden > 0 && least >= HIDDEN_MIN_AHEAD - 1e-4, "{kind:?}: a hidden group {least} m ahead of the eye");
            }
        }
    }

    /// T022's pack (if generated): the Charge Rifle's sequences as the graph's rifle table has them
    /// (frames, fps), the clips the grenade's timeline and the discharge name, groups 5 to 8, and
    /// at rest the rifle in the hip view, the R-301's carriers hidden.
    #[test]
    fn rifle_and_grenade_in_the_t022_pack() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/octane_weapons/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        assert!(p.has_rifle() && p.clips.len() >= 301, "{} clips", p.clips.len());
        for s in graph::Seq::ALL {
            let def = s.def_in(Weapon::ChargeRifle);
            for (k, &(frames, fps)) in def.samples.iter().enumerate() {
                let c = p.sample_in(s, k, Weapon::ChargeRifle).unwrap_or_else(|| panic!("{}_{k} missing", def.name));
                assert_eq!((c.frames as u32, c.fps), (frames, fps), "{}_{k}", def.name);
            }
        }
        for g in 5..=8u8 {
            assert!(p.carriers.iter().any(|c| c.group == g), "no carrier of group {g}");
        }
        use crate::spike::grenade::HandView;
        let params = ability::Params::default();
        let mut names: Vec<String> = Vec::new();
        for v in [HandView::Away(0.1, 0.55), HandView::Out(0.2), HandView::Out(2.0), HandView::Prep(0.1), HandView::Prep(1.0), HandView::Toss(0.1, false), HandView::Toss(0.1, true), HandView::Holster(0.1), HandView::Back(0.1, 0.6, 0.48)] {
            for sprinting in [false, true] {
                for crouch in [0.0, 1.0] {
                    let o = ordnance::out(v, ability::Params { sprinting, crouch, ..params });
                    names.extend(o.layers.iter().flat_map(|l| l.samples.iter().map(|s| s.0.clone())));
                }
            }
        }
        for l in discharge_layers(0.5, 0.4, 0.5, 0.5) {
            names.extend(l.samples.iter().map(|s| s.0.clone()));
        }
        for n in &names {
            assert!(clip_for(&p, n, Weapon::ChargeRifle).is_some(), "clip {n} not in the pack");
        }
        // the rifle at rest: its carriers in the hip view, the R-301's hidden (collapsed)
        let mut g = Graph::new_for(Weapon::ChargeRifle);
        g.step(1.0 / 60.0, &Signals::default());
        let show = groups_shown(None, 0, Weapon::ChargeRifle);
        assert!(show[RIFLE] && !show[1]);
        let posed = pose_pack(&p, &g.out(), &Sway::default(), None, Vis { show, hide: Hide::Hand, weapon: Weapon::ChargeRifle }).unwrap();
        let v = (35f32.to_radians().tan() * 0.75).atan();
        let (tv, th) = (v.tan(), v.tan() * 16.0 / 9.0);
        let in_view = |at: Vec3| at.z < -0.05 && (at.y / -at.z).abs() < tv && (at.x / -at.z).abs() < th;
        let rifle_in_view = p.carriers.iter().zip(&posed.carriers).filter(|(c, x)| c.group as usize == RIFLE && x.3 && in_view(x.2)).count();
        assert!(rifle_in_view > 0, "no rifle carrier in view");
        assert!(p.carriers.iter().zip(&posed.carriers).filter(|(c, _)| c.group == 1).all(|(_, x)| !x.3));
        assert!(posed.muzzle_view.is_some(), "the rifle's muzzle");
        println!("T022 pack: {} bones, {} carriers, {} clips; rifle carriers in view {rifle_in_view}; {} clip names checked", p.names.len(), p.carriers.len(), p.clips.len(), names.len());
    }

    /// The Wingman's pack (tools/apexpov/bake_wingman.py, if generated): its sequences as the graph's
    /// table has them, the switch's and the abilities' weapon clips found as `wm_*`, and at rest (hip,
    /// then aimed) the pistol in view with its muzzle, the R-301's carriers hidden.
    #[test]
    fn wingman_in_its_pack() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/octane_wingman/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        assert!(p.has_wingman() && p.has_rifle() && p.has_r99() && p.has_flatline() && p.has_sentinel(), "{} clips", p.clips.len());
        for w in [Weapon::Wingman, Weapon::R99, Weapon::Flatline, Weapon::Sentinel] {
            for s in graph::Seq::ALL {
                let def = s.def_in(w);
                for (k, &(frames, fps)) in def.samples.iter().enumerate() {
                    let c = p.sample_in(s, k, w).unwrap_or_else(|| panic!("{}_{k} missing", def.name));
                    assert_eq!((c.frames as u32, c.fps), (frames, fps), "{}_{k}", def.name);
                }
            }
        }
        for n in ["holster_0", "draw_0", "draw_1"] {
            assert!(clip_for(&p, n, Weapon::R99).is_some_and(|c| c.name == format!("r9_{n}")), "r9_{n}");
        }
        for w in [Weapon::Wingman, Weapon::ChargeRifle, Weapon::R99, Weapon::R301] {
            let (name, frames) = inspect_clip(w).unwrap();
            let c = p.clip(name).unwrap_or_else(|| panic!("{name} missing"));
            assert!(c.frames as u32 == frames && c.fps == 30.0 && !c.additive, "{name}");
            let l = inspect_layer(w, 1.0);
            assert!(l.weight == 1.0 && l.cycle > 0.0 && l.cycle < 1.0);
        }
        for n in ["holster_0", "draw_0", "draw_1", "switch_to_onehanded_0", "idle_onehanded_0", "fire_onehanded_0"] {
            assert!(clip_for(&p, n, Weapon::Wingman).is_some_and(|c| c.name == format!("wm_{n}")), "wm_{n}");
        }
        let v = (35f32.to_radians().tan() * 0.75).atan();
        let (tv, th) = (v.tan(), v.tan() * 16.0 / 9.0);
        let in_view = |at: Vec3| at.z < -0.05 && (at.y / -at.z).abs() < tv && (at.x / -at.z).abs() < th;
        for (w, group, muzzle) in [(Weapon::Wingman, WINGMAN, WM_MUZZLE), (Weapon::R99, R99G, R9_MUZZLE), (Weapon::Flatline, FLATLINEG, FL_MUZZLE), (Weapon::Sentinel, SENTINELG, SN_MUZZLE)] {
        let _ = muzzle;
        for ads in [0.0, 1.0] {
            let mut g = Graph::new_for(w);
            for _ in 0..60 {
                g.step(1.0 / 60.0, &Signals { ads, ..Default::default() });
            }
            let show = groups_shown(None, 0, w);
            assert!(show[group] && !show[1] && !show[RIFLE]);
            let posed = pose_pack(&p, &g.out(), &Sway::default(), None, Vis { show, hide: Hide::Hand, weapon: w }).unwrap();
            let shown = p.carriers.iter().zip(&posed.carriers).filter(|(c, x)| c.group as usize == group && x.3 && in_view(x.2)).count();
            assert!(shown > 0, "{w:?} ads {ads}: no carrier in view");
            assert!(p.carriers.iter().zip(&posed.carriers).filter(|(c, _)| c.group == 1).all(|(_, x)| !x.3));
            let m = posed.muzzle_view.expect("the Wingman's muzzle");
            println!("{w:?} ads {ads}: {shown} carriers in view; muzzle {m:.3} (m, mirrored pov frame)");
            // (the Sentinel's long barrel: its muzzle 1.2 m out)
            assert!(m.z < -0.1 && m.length() < 1.5, "muzzle {m}");
        }
        }
    }

    /// U3: with the Charge Rifle out (no view model of its own yet) the R-301's put-away is held at
    /// its end: every arm and gun bone is then out of the hip view (Apex's 70° 4:3 field of view on a
    /// 16:9 screen), so no empty hands show; at the put-away's start they are in it.
    #[test]
    fn r301_put_away_leaves_the_view() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/octane_ability/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        let mut g = Graph::new();
        g.step(1.0 / 60.0, &Signals::default());
        // the vertical half angle of a 70° horizontal 4:3 view, the horizontal one on 16:9
        let v = (35f32.to_radians().tan() * 0.75).atan();
        let (tv, th) = (v.tan(), v.tan() * 16.0 / 9.0);
        let in_view = |at: Vec3| at.z < -0.05 && (at.y / -at.z).abs() < tv && (at.x / -at.z).abs() < th;
        let posed_at = |cycle: f32| {
            let out = with_swap(None, Some((vec![("holster_0".to_string(), 1.0)], cycle))).unwrap();
            pose_pack(&p, &g.out(), &Sway::default(), Some(&out), Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap()
        };
        let visible = |posed: &Posed| p.carriers.iter().zip(&posed.carriers).filter(|(c, x)| c.group <= 1 && in_view(x.2)).map(|(c, _)| c.bone.clone()).collect::<Vec<_>>();
        let start = visible(&posed_at(0.0));
        assert!(!start.is_empty(), "at rest the arms are in view");
        let end = visible(&posed_at(1.0));
        println!("in view at rest: {}, at the put-away's end: {end:?}", start.len());
        assert!(end.is_empty(), "still in view: {end:?}");
    }

    /// Diagnostic: each carrier's forward distance from the eye (its Apex bone) at rest, and the
    /// least it comes to through each ability (shown or not).
    /// `cargo test --release carriers_near_the_eye -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn carriers_near_the_eye() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/octane_ability/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        const DT: f32 = 1.0 / 60.0;
        let mut g = Graph::new();
        g.step(DT, &Signals::default());
        let rest = pose_pack(&p, &g.out(), &Sway::default(), None, Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap();
        let kinds: Vec<ability::Kind> = (0..ability::FLOURISHES.len()).map(ability::Kind::Stim).chain([ability::Kind::Pad]).collect();
        let mut least = vec![vec![(f32::MAX, 0.0f32); kinds.len()]; p.carriers.len()];
        for (k, kind) in kinds.iter().enumerate() {
            let mut g = Graph::new();
            let mut ab = ability::Ability::new(*kind);
            while !ab.done() {
                g.step(DT, &Signals::default());
                ab.step(DT, ability::Params::default());
                let out = ab.out(ability::Params::default());
                let posed = pose_pack(&p, &g.out(), &Sway::default(), Some(&out), Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap();
                for (i, x) in posed.carriers.iter().enumerate() {
                    if -x.2.z < least[i][k].0 {
                        least[i][k] = (-x.2.z, ab.t);
                    }
                }
            }
        }
        for (i, c) in p.carriers.iter().enumerate() {
            let per: Vec<String> = least[i].iter().map(|(z, t)| format!("{z:6.3}@{t:.2}")).collect();
            println!("{:>2} g{} {:<28} rest {:6.3} | {}", i, c.group, c.bone, -rest.carriers[i].2.z, per.join(" "));
        }
    }

    /// The baked pack (if generated) posed at the hip: the muzzle where the offline check put it.
    #[test]
    fn hip_muzzle_from_the_pack() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = pack::parse(&d).unwrap();
        let mut g = Graph::new();
        g.step(1.0 / 60.0, &Signals::default());
        let mut o = g.out();
        // without the additive layers: the bare hip pose (ads_out at its end)
        o.add.clear();
        let posed = pose_pack(&p, &o, &Sway::default(), None, Vis { show: [true; GROUPS], hide: Hide::Hand, weapon: Weapon::R301 }).unwrap();
        let (muzzle, turn) = muzzle_and_turn(&posed);
        let (t, r) = muzzle.unwrap();
        assert!((t - Vec3::new(32.96, 2.93, -3.52)).length() < 0.02, "{t}");
        assert!((r - Vec3::new(0.84, 0.09, -95.96)).length() < 0.05, "{r}");
        assert!(turn.length() < 1e-3, "{turn}");
        assert_eq!(posed.carriers.len(), p.carriers.len());
    }
}
