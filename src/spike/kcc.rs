//! Spike S5: Fuse moves by Apex's rules, the movement controller of deps/er-apex-move (T004), over
//! Elden Ring's own collision (havok_col.rs), and the Tarnished is put where it says every frame,
//! er-mario's way: position written after physics, the proxy told to follow, the game's fall timer
//! and fall motion off, gravity only while grounded (so the game still allows interactions).
//! ini `kcc = 1`; dev command `kcc` for its numbers.
//!
//! The pad: the left stick is the controller's alone (input.rs hides it from the game); A jumps,
//! B crouches (slides from a sprint), the left stick button sprints (Apex's pad layout; one press
//! keeps sprinting while running on). The keyboard when the stick is idle: WASD, Shift sprint,
//! Space jump, Ctrl hold crouch, C toggle crouch (Apex's PC defaults; kbd.rs hides them).
//! The game's jump, roll, backstep and crouch actions are stripped; interact, lock-on, the camera
//! and the gun stay as they are.
//!
//! Hands off (the controller restarts from wherever the game left the character): what the game
//! drives (fog gates, doors, levers: animations 60000-69999; ladders), the map and menus, death,
//! loading.
//!
//! Numbers: the Apex movement settings are data (er-apex-move). Four scales are not in the data and
//! are set here for M0, to be decided: metres per unit = the model's own scale (T005 align.json:
//! 0.0254 × 0.970), walkable slope 50°, speed ×1. Gravity now uses S3's 750 units/s² (A3).
//!
//! Checks while it runs: each frame's step time; each frame a ray of the game's own (map filter)
//! from the last position to the new one at knee and chest height: a hit means the step went
//! through a wall (logged, counted); frames the controller reports stuck.

use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use eldenring::cs::{CSCamera, CSHavokMan, PlayerIns, WorldChrMan};
use eldenring::position::{HavokPosition, PositionDelta};
use eldenring::rotation::Quaternion;
use fromsoftware_shared::FromStatic;
use er_apex_move::{Controller, MoveEvents, MoveInput, MoveParams, Pose, Triangle, Vector3, World};
use glam::Vec3;

use crate::havok_col::{HavokCollision, QueryRun, Window};
use crate::input::{MOVEMENT_TAKEN, move_pad};
use crate::{dlog, log, paths, state};

/// Metres per Apex unit: the scale Fuse's model was converted with (T005 align.json).
const MPU: f32 = 0.0254 * 0.970_002_1;
/// S3's gravity, converted with the model scale. Slope and speed multiplier remain M0 choices.
// S3 sv_gravity = 750 Source units/s² (apex-slide-spec and s3-octane-reference).
// The previous M0 value of 16 m/s² made pad launches rise further than the reference.
const GRAVITY: f32 = 750.0 * MPU;
const MAX_SLOPE_DEG: f32 = 50.0;
const SPEED_MULT: f32 = 1.0;
/// Havok layers the controller collides with (er-mario's: terrain, buildings, props).
const LAYERS: [u32; 9] = [0x1e, 0x37, 0x38, 0x39, 0x3a, 0x47, 0x48, 0x49, 0x51];
/// The game's ray filter for map geometry (er-mario's ground probes; the gun's wall check).
const MAP_RAY: u32 = 0x08;
/// The triangles the controller gets: 12 m around, 6 m down and up, a 40 m column below.
/// Downhill this can miss the ground ahead: on The First Step's slope a sprint jump came down 10 m
/// below the window's centre, 8 m ahead, before the next window was read (~0.9 s), and fell through
/// (2026-10-04 23:44; 12 m down missed it as well and hit the 20,000-triangle cap at the start).
/// The fall is caught in `update`; a window reaching further down while airborne is still to do.
/// At most 40,000 triangles, the nearest: Volcano Manor's rooms give 25,000-73,000 in this window,
/// and the 20,000 it had left holes he fell through (2026-10-10).
const WINDOW: Window = Window { radius: 12.0, down: 6.0, up: 6.0, column: 2.0, below: 40.0, max_tris: 40_000 };
/// At most one fall-through recovery (see `update`) in this many seconds: a floor the game's ray
/// hits but the controller can't collide with must not catch the Tarnished every frame.
const RECOVER_GAP_S: f32 = 2.0;
/// Stuck this long, the controller lets the game move him for STUCK_HANDS_OFF_MS.
const STUCK_RELEASE_S: f32 = 0.3;
const STUCK_HANDS_OFF_MS: u64 = 600;
/// Metres from the window's centre before the next window is fetched.
const REFETCH: f32 = 4.0;
/// A window is read again at least this often (seconds): the map's collision streams in late.
const WINDOW_MAX_AGE_S: f32 = 1.0;
/// Havok bodies read per frame while fetching a window (the world has ~4000; reading them all in
/// one frame took 3-4 ms, journal 2026-10-04).
const BODIES_PER_FRAME: usize = 400;
/// ...and at most this much time per frame (µs), checked as each body starts: one body's own
/// picking can take ~0.5 ms more (500 left frames at 1.1 ms near the grace, 2026-10-04).
const FETCH_BUDGET_US: u64 = 300;
/// Metres from the controller's origin before it restarts (keeps its coordinates small).
const RECENTRE: f32 = 300.0;

const PAD_A: u16 = 0x1000;
const PAD_B: u16 = 0x2000;
const PAD_LS: u16 = 0x0040;
/// XInput's own left stick dead zone.
const DEAD: f32 = 7849.0 / 32767.0;
/// The Tarnished's actions the controller takes over (fromsoftware-rs ChrActions bits): dodge 5,
/// jump 6, crouch 13, backstep 16, rolling 17, emergency step 25.
const TAKEN: u64 = (1 << 5) | (1 << 6) | (1 << 13) | (1 << 16) | (1 << 17) | (1 << 25);

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| paths::flag("kcc")) && crate::mode::apex()
}

/// F5 to the Tarnished (mode.rs): the controller lets go of the player now (update stops calling).
pub fn hands_off() {
    let player = (unsafe { WorldChrMan::instance_mut() }).ok().and_then(|w| w.main_player.as_mut()).map(|p| &mut **p);
    if let Some(k) = KCC.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        k.release(player);
    }
    MOVEMENT_TAKEN.store(false, Ordering::Relaxed);
}

fn params() -> MoveParams {
    MoveParams {
        base_gravity: Some(GRAVITY / MPU),
        speed_multiplier: Some(SPEED_MULT),
        meters_per_unit: Some(MPU),
        max_slope_radians: Some(MAX_SLOPE_DEG.to_radians()),
        // 5 slide iterations, not the library's 8: rubble made 8-iteration ticks cost over 1 ms
        // (journal 2026-10-04); fewer only stop a slide along very rough walls a little sooner
        collision_iterations: 5,
        ..MoveParams::default()
    }
}

/// Step times (µs) of the last frames, and the counts the S5 test reads.
#[derive(Default)]
struct Stats {
    frames: u64,
    recent: Vec<f32>,
    step_max: f32,
    over_1ms: u64,
    /// the whole frame's cost: window fetching slice + step (µs)
    frame_max: f32,
    frame_over_1ms: u64,
    start_fetch_ms_max: f32,
    fetches: u64,
    fetch_ms_max: f32,
    build_ms_max: f32,
    tris: usize,
    crossings: u64,
    /// falls through the controller's world caught (see `update`)
    recoveries: u64,
    stuck: u64,
    starts: u64,
}

impl Stats {
    fn frame(&mut self, us: f32) {
        self.frame_max = self.frame_max.max(us);
        if us > 1000.0 {
            self.frame_over_1ms += 1;
        }
    }

    fn step(&mut self, us: f32) {
        self.frames += 1;
        self.step_max = self.step_max.max(us);
        if us > 1000.0 {
            self.over_1ms += 1;
        }
        if self.recent.len() >= 600 {
            self.recent.remove(0);
        }
        self.recent.push(us);
    }

    fn line(&self) -> String {
        let mut v = self.recent.clone();
        v.sort_by(f32::total_cmp);
        let at = |q: f32| v.get(((v.len() as f32 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
        let mean = if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
        format!(
            "frames {}, step µs (last {}) mean {mean:.0} p50 {:.0} p99 {:.0} max {:.0} (all-time max {:.0}, >1 ms {}); whole frame (fetch slice + step) max {:.0} µs, >1 ms {}; fetches {} (slices max {:.2} ms, build max {:.1} ms off-thread; at start {:.1} ms), {} tris; wall crossings {} (falls caught {}), stuck frames {}, starts {}",
            self.frames,
            v.len(),
            at(0.5),
            at(0.99),
            at(1.0),
            self.step_max,
            self.over_1ms,
            self.frame_max,
            self.frame_over_1ms,
            self.fetches,
            self.fetch_ms_max,
            self.build_ms_max,
            self.start_fetch_ms_max,
            self.tris,
            self.crossings,
            self.recoveries,
            self.stuck,
            self.starts
        )
    }
}

struct Kcc {
    havok: HavokCollision,
    ctl: Option<Controller>,
    /// World position (metres) of the controller's (0, 0, 0).
    origin: Vec3,
    /// Centre of the installed triangle window.
    window: Vec3,
    /// When the window installed was read: re-read after `WINDOW_MAX_AGE_S` even standing still (the
    /// game streams a map tile's collision in after he enters it: read too early, the window lacks
    /// the floor ahead, 2026-10-08)
    window_read: Instant,
    /// A window being read from the game, a few hundred bodies a frame.
    fetching: Option<QueryRun>,
    /// A window being built off-thread: its centre, its triangle count, the result.
    building: Option<(Vec3, usize, Receiver<Built>)>,
    /// Where the Tarnished was put last frame.
    last: Option<Vec3>,
    stats: Stats,
    log_at: Option<Instant>,
    /// The last fetched window as the game has it (world metres, layer, body), for `kcc probe`.
    last_window: Vec<([Vec3; 3], u32, u32)>,
    /// Slow steps written to dev/ so far.
    slow_dumps: u32,
    /// Wall crossings written to dev/ so far.
    cross_dumps: u32,
    /// No ground in the last window: the game keeps the Tarnished until then (no window every
    /// frame while he falls through nothing).
    retry_at: Option<Instant>,
    /// Since when the capsule has been stuck in the triangles (it cannot move while it is)
    stuck_since: Option<Instant>,
    /// Frames stepped: `Locomotion::frame`.
    loco_frame: u64,
    /// After a fall through the controller's world: the horizontal velocity (world m/s) to restart
    /// with, and when it was caught.
    carry: Option<(Vec3, Instant)>,
    last_recovery: Option<Instant>,
    /// The game's ground under his feet last frame (its height) and the frames in a row it moved
    /// while he stood still: a lift (`on_moving_ground`)
    ground: Option<f32>,
    ground_moving: u32,
    /// On a lift: the game's ground under him last frame (released) and since when it has not moved:
    /// the controller takes him back only once the lift has stood still for `LIFT_HOLD_MS`
    lift: Option<(Option<f32>, Instant)>,
    /// In the air: where the flight comes down (the game's map ray along the arc) and when that
    /// was worked out (`window_centre`).
    landing: Option<(Vec3, Instant)>,
}

static KCC: Mutex<Option<Kcc>> = Mutex::new(None);

/// A window built off-thread: the controller's world, the build time (ms), the window as the game
/// has it (for `kcc probe`). Converting and copying 17,000 triangles on the game thread took
/// up to 3 ms (Boss room, 2026-10-04).
type Built = (Result<World, String>, f32, Vec<([Vec3; 3], u32, u32)>);

fn v3(v: Vec3) -> Vector3<f32> {
    Vector3::new(v.x, v.y, v.z)
}

fn g3(v: &Vector3<f32>) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

impl Kcc {
    fn new() -> Self {
        Kcc {
            havok: HavokCollision::new(LAYERS.to_vec()),
            ctl: None,
            origin: Vec3::ZERO,
            window: Vec3::ZERO,
            window_read: Instant::now(),
            fetching: None,
            building: None,
            last: None,
            stats: Stats::default(),
            log_at: None,
            last_window: Vec::new(),
            slow_dumps: 0,
            cross_dumps: 0,
            retry_at: None,
            stuck_since: None,
            loco_frame: 0,
            carry: None,
            last_recovery: None,
            ground: None,
            ground_moving: 0,
            lift: None,
            landing: None,
        }
    }

    fn local(&self, w: Vec3) -> Vector3<f32> {
        v3((w - self.origin) / MPU)
    }

    fn world(&self, l: &Vector3<f32>) -> Vec3 {
        self.origin + g3(l) * MPU
    }

    /// The game's triangles around `centre`, in the controller's units, all at once (game
    /// thread: it reads the live Havok world). For the controller's start.
    fn fetch(&mut self, centre: Vec3) -> Option<Vec<Triangle>> {
        let t0 = Instant::now();
        let tris = self.havok.query(centre, WINDOW)?;
        let out = self.adopt(tris);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.stats.start_fetch_ms_max = self.stats.start_fetch_ms_max.max(ms);
        dlog(format!("kcc: window at {centre:.1?}: {} triangles in {ms:.2} ms (at once)", out.len()));
        Some(out)
    }

    /// Writes the window (the controller's units) with a state and input to dev/<name>, for
    /// deps/er-apex-move/examples/replay.rs. Lines: `mpu gravity_mps2 max_slope_deg speed_mult`,
    /// `position xyz velocity xyz grounded`, `input wish xyz sprint crouch jump ticks us`, then
    /// one triangle per line (9 numbers).
    fn dump_to(&self, name: &str, st: &er_apex_move::MoveState, input: &MoveInput, ticks: usize, us: f32) -> String {
        let mut out = format!("{MPU} {GRAVITY} {MAX_SLOPE_DEG} {SPEED_MULT}\n");
        out += &format!("{} {} {} {} {} {} {}\n", st.position.x, st.position.y, st.position.z, st.velocity.x, st.velocity.y, st.velocity.z, st.grounded as u8);
        out += &format!(
            "input {} {} {} {} {} {} {ticks} {us}\n",
            input.wish.x, input.wish.y, input.wish.z, input.sprint as u8, input.crouch as u8, input.jump as u8
        );
        for (t, _, _) in &self.last_window {
            let l = t.map(|v| self.local(v));
            out += &format!("{} {} {} {} {} {} {} {} {}\n", l[0].x, l[0].y, l[0].z, l[1].x, l[1].y, l[1].z, l[2].x, l[2].y, l[2].z);
        }
        let path = paths::file("dev").join(name);
        match std::fs::write(&path, out) {
            Ok(()) => format!("wrote {}", path.display()),
            Err(e) => format!("{}: {e}", path.display()),
        }
    }

    /// Keeps a fetched window (for `kcc probe`) and returns it in the controller's units.
    fn adopt(&mut self, tris: Vec<([Vec3; 3], u32, u32)>) -> Vec<Triangle> {
        let out: Vec<Triangle> = tris.iter().map(|(t, _, _)| t.map(|v| self.local(v))).collect();
        self.last_window = tris;
        self.stats.fetches += 1;
        out
    }

    /// Lets go of the Tarnished: the game moves him again.
    fn release(&mut self, player: Option<&mut PlayerIns>) {
        if self.ctl.take().is_some() {
            if let Some(p) = player {
                p.chr_ins.modules.fall.disable_fall_motion = false;
                p.chr_ins.modules.physics.gravity_disabled = false;
            }
            dlog("kcc: hands off");
        }
        self.fetching = None;
        self.building = None;
        self.last = None;
        crate::firstperson::set_view_height(None);
        *LOCOMOTION.lock().unwrap_or_else(|e| e.into_inner()) = None;
        MOVEMENT_TAKEN.store(false, Ordering::Relaxed);
    }
}

/// How Fuse moves, for the gun's spread, the crosshair, the viewmodel and the camera effects (None
/// while the controller is off: they fall back to standing still).
#[derive(Clone, Copy, Debug)]
pub struct Locomotion {
    pub sprinting: bool,
    /// Crouched or sliding (Apex's crouch spread applies to both).
    pub crouched: bool,
    pub grounded: bool,
    /// Horizontal speed, m/s.
    pub speed: f32,
    pub sliding: bool,
    /// m/s, world axes.
    pub velocity: Vec3,
    /// Apex's crouchFraction (gun-motion spec §2.3): the viewmodel's crouch blend.
    pub duck_frac: f32,
    /// Apex's visual sprint fraction and the eye offset it gives, metres (negative: lower; spec
    /// §4.1). The offset is already in the view height the camera gets; kept here for readers that
    /// want them apart (none yet).
    #[allow(dead_code)]
    pub sprint_frac: f32,
    #[allow(dead_code)]
    pub eye_sprint_offset: f32,
    /// slideLongJumpAllowed: with `sliding`, the slide FOV (spec §4.4).
    pub slide_long_jump: bool,
    /// What happened in the controller's step of frame `frame` (landing speeds in raw Apex
    /// units/s). Readers act on a frame once: they keep the last frame they handled.
    pub events: MoveEvents,
    pub frame: u64,
}

static LOCOMOTION: Mutex<Option<Locomotion>> = Mutex::new(None);

pub fn locomotion() -> Option<Locomotion> {
    *LOCOMOTION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Apex units per metre, for callers working in Apex's units (octane.rs).
pub const UNITS_PER_METRE: f32 = 1.0 / MPU;

/// Half the player's box height now, Apex units (standing or crouched; the launch pad's punch).
pub fn hull_half_height() -> Option<f32> {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let ctl = g.as_ref()?.ctl.as_ref()?;
    let p = ctl.params();
    Some(0.5 * if ctl.state.crouched { p.crouching.height } else { p.standing.height })
}

/// The first surface of the controller's triangle window between two world points (metres): the
/// point and the surface normal facing back along the segment. None with the controller off or no
/// surface in the window (Octane's thrown launch pad).
pub fn ray_cast(from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let k = g.as_ref()?;
    let ctl = k.ctl.as_ref()?;
    let (a, b) = (k.local(from), k.local(to));
    let d = b - a;
    let (_, p, n) = ctl.world().ray_cast(a, d, d.norm())?;
    Some((k.world(&p), g3(&n)))
}

/// The player's feet (world metres) as the controller has them.
pub fn feet() -> Option<Vec3> {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let k = g.as_ref()?;
    k.ctl.as_ref().map(|c| k.world(&c.state.position))
}

/// Launches the player at this velocity (world m/s), with one double jump if asked and gravity
/// scaled until landing (Octane's launch pad; er-apex-move `Controller::launch`). False with the
/// controller off.
pub fn launch(velocity: Vec3, double_jump: bool, gravity_scale: f32) -> bool {
    let mut g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ctl) = g.as_mut().and_then(|k| k.ctl.as_mut()) else { return false };
    ctl.launch(v3(velocity / MPU), double_jump, gravity_scale).is_ok()
}

/// Sets the velocity (world m/s) with gravity so scaled (Pathfinder's grapple each frame:
/// er-apex-move `Controller::pull`). False with the controller off.
pub fn pull(velocity: Vec3, gravity_scale: f32) -> bool {
    let mut g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ctl) = g.as_mut().and_then(|k| k.ctl.as_mut()) else { return false };
    ctl.pull(v3(velocity / MPU), gravity_scale).is_ok()
}

/// The last step's move input (world, flat, camera-relative, length 0..1): the grapple's swing.
static WISH: Mutex<Vec3> = Mutex::new(Vec3::ZERO);

pub fn wish() -> Vec3 {
    *WISH.lock().unwrap_or_else(|e| e.into_inner())
}

/// The velocity (world m/s), whether on the ground.
pub fn velocity() -> Option<(Vec3, bool)> {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let ctl = g.as_ref()?.ctl.as_ref()?;
    Some((g3(&ctl.state.velocity) * MPU, ctl.state.grounded))
}

/// A step's events for `kcc trace` ("-": none).
fn events_line(e: &MoveEvents) -> String {
    let mut s: Vec<String> = Vec::new();
    if e.jumped {
        s.push("jump".into());
    }
    if let Some(l) = e.landed {
        s.push(format!("land {:.0}{}", l.speed, if l.crouched { " crouched" } else { "" }));
    }
    if let Some(boost) = e.slide_started {
        s.push(if boost { "slide boost" } else { "slide" }.into());
    }
    for (on, name) in [
        (e.sprint_started, "sprint+"),
        (e.sprint_ended, "sprint-"),
        (e.duck_started, "duck"),
        (e.unduck_started, "unduck"),
        (e.launched, "launch"),
        (e.double_jumped, "double jump"),
    ] {
        if on {
            s.push(name.into());
        }
    }
    if s.is_empty() { "-".into() } else { s.join(",") }
}

/// Events the game drives: fog gates, doors, levers (60000-69999) and ladders (er-mario's
/// `game_driven`, audit #7).
fn game_driven(anim: i32) -> bool {
    (60000..70000).contains(&anim) || (28000..29000).contains(&anim) || (51100..51200).contains(&anim)
}

/// The camera's forward and right on the ground plane: in first person the eye's, before the
/// camera effects turn it (a punch or the slide's roll must not steer him: viewfx.rs).
fn camera_flat() -> Option<(Vec3, Vec3)> {
    let (fwd, right) = match crate::camera::eye_view().or_else(crate::camera::view) {
        Some((_, f, r, _)) => (f, r),
        None => {
            let m = &unsafe { CSCamera::instance() }.ok()?.pers_cam_1.matrix;
            (Vec3::new(m.2.0, m.2.1, m.2.2), Vec3::new(m.0.0, m.0.1, m.0.2))
        }
    };
    let f = Vec3::new(fwd.x, 0.0, fwd.z).try_normalize()?;
    let r = Vec3::new(right.x, 0.0, right.z).try_normalize()?;
    Some((f, r))
}

/// Whether the game's map geometry lies on the segment a–b (lifted by `h`).
/// Seconds a window takes from the decision to fetch it to its install (bodies read a few hundred
/// a frame, then built off-thread): how far ahead of a mover on the ground the next one goes.
const WINDOW_LEAD_S: f32 = 0.5;
/// In the air the window is read faster (a sprint jump down The First Step's slope lands in about
/// 0.65 s, a window took about 1 s at the ground's pace): more bodies and time a frame, for the
/// second or so it lasts.
const BODIES_PER_FRAME_AIR: usize = 1500;
const FETCH_BUDGET_AIR_US: u64 = 1200;
/// Moving fast on the ground, the window reaches further down (the slope ahead: 11 m down over
/// 10 m once, 2026-10-05 19:14).
const FAST_GROUND_MPS: f32 = 6.0;
/// The arc is followed this far (seconds, in steps) to find where a flight comes down, and worked
/// out again this often.
const ARC_S: f32 = 4.0;
const ARC_STEP_S: f32 = 0.1;
const ARC_EVERY_S: f32 = 0.1;

/// Where the next triangle window should be centred (U6). On the ground: ahead along the speed by
/// the window's lead. In the air: where the flight comes down, the game's map ray followed along
/// the arc (the controller's gravity now, a launch's scale included) for up to `ARC_S`: a pad
/// launch (about 3 s and 30-40 m) or a sprint jump down The First Step's slope otherwise came down
/// outside the triangles and fell through (2026-10-04 23:44, 2026-10-05 15:53, 17:38, 19:03). The
/// window then holds the landing, not the air around him; a wall in the way of the flight is not
/// collided with (rare; the fall-through catch stays).
/// The next window: where (`window_centre`) and its shape: as `WINDOW`, but reaching 12 m down when
/// moving fast on the ground, and in the air from the landing up to where he is (its radius as
/// usual round the landing).
fn window_for(k: &mut Kcc, at: Vec3, player: &PlayerIns) -> (Vec3, Window) {
    let centre = window_centre(k, at, player);
    let Some(st) = k.ctl.as_ref().map(|c| &c.state) else { return (centre, WINDOW) };
    let vel = g3(&st.velocity) * MPU;
    let mut win = WINDOW;
    if !st.grounded {
        win.up = (at.y - centre.y + 3.0).clamp(WINDOW.up, 30.0);
    } else if Vec3::new(vel.x, 0.0, vel.z).length() > FAST_GROUND_MPS {
        win.down = 12.0;
    }
    (centre, win)
}

fn window_centre(k: &mut Kcc, at: Vec3, player: &PlayerIns) -> Vec3 {
    let Some(ctl) = k.ctl.as_ref() else { return at };
    let st = &ctl.state;
    let vel = g3(&st.velocity) * MPU;
    let ahead = at + (Vec3::new(vel.x, 0.0, vel.z) * WINDOW_LEAD_S).clamp_length_max(WINDOW.radius * 0.5);
    if st.grounded {
        k.landing = None;
        return ahead;
    }
    if let Some((p, t)) = k.landing
        && t.elapsed().as_secs_f32() < ARC_EVERY_S
    {
        return p;
    }
    let g = ctl.air_gravity_now() * MPU;
    let (mut p, mut v) = (at + Vec3::Y * 0.5, vel);
    let mut landing = None;
    for _ in 0..(ARC_S / ARC_STEP_S) as usize {
        let nv = v - Vec3::Y * g * ARC_STEP_S;
        let np = p + (v + nv) * (0.5 * ARC_STEP_S);
        if let Some(hit) = wall_between(p, np, 0.0, player) {
            landing = Some(hit);
            break;
        }
        (p, v) = (np, nv);
    }
    let centre = landing.unwrap_or(ahead);
    k.landing = Some((centre, Instant::now()));
    centre
}

/// The game keeps him on a lift until the ground under him has not moved for this long.
const LIFT_HOLD_MS: u64 = 1500;

/// Whether the game's ground under his feet has moved up or down for 3 frames in a row while he
/// stood still (across and on the controller's ground): a lift or another moving platform.
fn on_moving_ground(k: &mut Kcc, st: &er_apex_move::MoveState, pos: Vec3, player: &PlayerIns) -> bool {
    let ground = st.grounded.then(|| wall_between(pos + Vec3::Y * 0.6, pos - Vec3::Y * 0.6, 0.0, player)).flatten().map(|g| g.y);
    let still = k.last.is_some_and(|l| Vec3::new(pos.x - l.x, 0.0, pos.z - l.z).length() < 0.01);
    let moved = matches!((ground, k.ground), (Some(g), Some(p)) if (g - p).abs() > 0.005);
    k.ground_moving = if still && moved { k.ground_moving + 1 } else { 0 };
    k.ground = ground;
    k.ground_moving >= 3
}

fn wall_between(a: Vec3, b: Vec3, h: f32, player: &PlayerIns) -> Option<Vec3> {
    let havok = unsafe { CSHavokMan::instance() }.ok()?;
    let (a, d) = (a + Vec3::Y * h, b - a);
    if d.length() < 0.01 {
        return None;
    }
    let hit = havok.phys_world.cast_ray(MAP_RAY, &HavokPosition(a.x, a.y, a.z, 0.0), PositionDelta(d.x, d.y, d.z), player)?;
    Some(Vec3::new(hit.0, hit.1, hit.2))
}

/// ChrIns_PreBehaviorSafe, after the game turned the pad into actions: while the controller owns
/// the movement, the Tarnished doesn't jump, roll or crouch on its buttons.
pub fn input_task() {
    if !enabled() || !MOVEMENT_TAKEN.load(Ordering::Relaxed) {
        return;
    }
    let Some(player) = (unsafe { WorldChrMan::instance_mut() }).ok().and_then(|w| w.main_player.as_deref_mut()) else { return };
    let req: &mut eldenring::cs::CSChrActionRequestModule = &mut player.chr_ins.modules.action_request;
    let bits = |a: &mut eldenring::cs::ChrActions| unsafe { &mut *(a as *mut _ as *mut u64) };
    for a in [&mut req.action_requests, &mut req.new_action_presses, &mut req.queued_action_inputs, &mut req.cancel_ready_actions] {
        *bits(a) &= !TAKEN;
    }
}

/// Every frame (ChrIns_PostPhysics).
/// Set by the dev channel's `tp`: the Tarnished was really moved, which the controller would take
/// for the game re-basing its coordinates (below) and carry its ground along (stood on air,
/// 2026-10-05); instead it lets go and starts over where he is.
static TELEPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn teleported() {
    TELEPORTED.store(true, Ordering::Relaxed);
}

pub fn update(dt: f32) {
    if !enabled() {
        return;
    }
    let mut guard = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let k = guard.get_or_insert_with(Kcc::new);
    let player = unsafe { WorldChrMan::instance_mut() }.ok().and_then(|w| w.main_player.as_deref_mut());
    let Some(player) = player else {
        k.release(None);
        return;
    };
    if TELEPORTED.swap(false, Ordering::Relaxed) {
        log("kcc: moved by the dev channel, starting over there");
        k.release(Some(player));
        return;
    }
    let anim = state::current_anim(player);
    let booting = super::quickboot::enabled() && !super::quickboot::settled();
    if !state::in_world() || booting || !crate::fe::in_play_view() || game_driven(anim) || player.chr_ins.modules.data.hp <= 0 {
        k.release(Some(player));
        return;
    }
    let p = player.chr_ins.modules.physics.position;
    let here = Vec3::new(p.0, p.1, p.2);

    // Elden Ring re-bases its physics coordinates as you travel (floating origin, er-mario
    // lib.rs): the Tarnished suddenly metres from where we put him means every body moved with
    // him. Without this our frame stayed behind, the next window held nothing and he fell out of
    // Godrick's arena (2026-10-04). The installed triangles stay valid (relative to `origin`); a
    // window being read or built is in the old frame: dropped, read again.
    if let (Some(_), Some(last)) = (k.ctl.as_ref(), k.last) {
        let d = here - last;
        if d.length() > 2.0 {
            log(format!("kcc: world origin shifted by {d:.2?}"));
            k.origin += d;
            k.window += d;
            // the predicted landing too (a launch crossed a tile mid-flight and the next window
            // went to the old frame's landing: fell through, 2026-10-05 19:11)
            if let Some((p, _)) = k.landing.as_mut() {
                *p += d;
            }
            super::octane::world_shift(d);
            super::grenade::world_shift(d);
            k.last = Some(here);
            k.ground = None;
            k.fetching = None;
            k.building = None;
            for (t, _, _) in k.last_window.iter_mut() {
                for v in t.iter_mut() {
                    *v += d;
                }
            }
        }
    }

    // (re)start where the Tarnished stands, with a window fetched right away
    if k.ctl.is_none() {
        // on a lift the game carries him until it has stood still a while (a lift going down left
        // him standing on the air when the controller took him back mid-way, 2026-10-08)
        if let Some((last, still)) = k.lift {
            let ground = wall_between(here + Vec3::Y * 0.5, here - Vec3::Y * 3.0, 0.0, player).map(|g| g.y);
            let moved = match (ground, last) {
                (Some(g), Some(l)) => (g - l).abs() > 0.003,
                (a, b) => a.is_some() != b.is_some(),
            };
            let still = if moved { Instant::now() } else { still };
            if still.elapsed().as_millis() < LIFT_HOLD_MS as u128 {
                k.lift = Some((ground, still));
                return;
            }
            log(format!("kcc: the lift stopped at {here:.2?}: the controller takes him back"));
            k.lift = None;
        }
        if k.retry_at.is_some_and(|t| Instant::now() < t) {
            return;
        }
        k.origin = here;
        let Some(tris) = k.fetch(here) else { return };
        // nothing to stand on (off a cliff, below the map: Godrick's arena edge, 2026-10-04): the
        // game's own fall, fall damage and death, not ours forever through nothing
        if tris.is_empty() {
            if k.retry_at.is_none() {
                log(format!("kcc: no ground at {here:.2?}, the game keeps the Tarnished"));
            }
            k.retry_at = Some(Instant::now() + std::time::Duration::from_millis(500));
            return;
        }
        k.retry_at = None;
        let built = World::from_triangles(&tris).map_err(|e| format!("{e:?}")).and_then(|w| Controller::new(w, params(), Vector3::zeros()).map_err(|e| format!("{e:?}")));
        match built {
            Ok(mut c) => {
                // restarted after a fall through: on with the speed he had (if that was just now)
                if let Some((v, at)) = k.carry.take().filter(|(_, at)| at.elapsed().as_secs_f32() < 0.5) {
                    c.state.velocity = v3(v / MPU);
                    log(format!("kcc: restarted on the ground with {:.2} m/s (caught {:.0} ms ago)", v.length(), at.elapsed().as_secs_f32() * 1000.0));
                }
                k.ctl = Some(c);
                k.window = here;
                k.window_read = Instant::now();
                k.stats.tris = tris.len();
                k.stats.starts += 1;
                log(format!("kcc: controller on at {here:.2?} ({} triangles, anim {anim})", tris.len()));
            }
            Err(e) => {
                log(format!("kcc: no controller: {e}"));
                return;
            }
        }
    }

    // keep the window around the player: read here a few hundred bodies a frame, built
    // off-thread, installed when ready
    let frame_t0 = Instant::now();
    let at = k.world(&k.ctl.as_ref().unwrap().state.position);
    let airborne = k.ctl.as_ref().is_some_and(|c| !c.state.grounded);
    if k.building.is_none() {
        let (want, win) = window_for(k, at, player);
        match k.fetching.as_ref() {
            None if want.distance(k.window) > REFETCH || k.window_read.elapsed().as_secs_f32() > WINDOW_MAX_AGE_S => k.fetching = Some(QueryRun::new(want, win)),
            // in the air a window still being read for somewhere else (where he took off) is
            // dropped for the landing's: it would come too late
            Some(run) if airborne && want.distance(run.center) > REFETCH && want.distance(k.window) > REFETCH => {
                k.fetching = Some(QueryRun::new(want, win));
            }
            _ => {}
        }
    }
    if let Some(mut run) = k.fetching.take() {
        let t0 = Instant::now();
        let (bodies, budget) = if airborne { (BODIES_PER_FRAME_AIR, FETCH_BUDGET_AIR_US) } else { (BODIES_PER_FRAME, FETCH_BUDGET_US) };
        match k.havok.query_step_until(&mut run, bodies, Some(t0 + std::time::Duration::from_micros(budget))) {
            Some(None) => k.fetching = Some(run),
            Some(Some(mut raw)) => {
                let centre = run.center;
                let origin = k.origin;
                let (tx, rx) = channel();
                let n = raw.len().min(WINDOW.max_tris);
                k.stats.fetches += 1;
                std::thread::spawn(move || {
                    let t0 = Instant::now();
                    crate::havok_col::cap(&mut raw, centre, WINDOW.max_tris);
                    let tris: Vec<Triangle> = raw.iter().map(|(t, _, _)| t.map(|v| v3((v - origin) / MPU))).collect();
                    let w = World::from_triangles(&tris).map_err(|e| format!("{e:?}"));
                    let _ = tx.send((w, t0.elapsed().as_secs_f32() * 1000.0, raw));
                });
                k.building = Some((centre, n, rx));
                dlog(format!("kcc: window at {centre:.1?}: {n} triangles"));
            }
            None => {}
        }
        k.stats.fetch_ms_max = k.stats.fetch_ms_max.max(t0.elapsed().as_secs_f32() * 1000.0);
    }
    match k.building.as_ref().map(|(centre, n, rx)| (*centre, *n, rx.try_recv())) {
        Some((centre, 0, Ok(_))) => {
            log(format!("kcc: no ground in the window at {centre:.2?}, hands off"));
            k.release(Some(player));
            k.retry_at = Some(Instant::now() + std::time::Duration::from_millis(500));
            return;
        }
        Some((centre, n, Ok((Ok(w), ms, raw)))) => {
            let old = k.ctl.as_mut().unwrap().replace_world(w);
            std::thread::spawn(move || drop(old));
            let old = std::mem::replace(&mut k.last_window, raw);
            std::thread::spawn(move || drop(old));
            k.window = centre;
            k.window_read = Instant::now();
            k.stats.tris = n;
            k.stats.build_ms_max = k.stats.build_ms_max.max(ms);
            k.building = None;
        }
        Some((_, _, Ok((Err(e), _, _)))) => {
            log(format!("kcc: window not built: {e}"));
            k.building = None;
        }
        Some((_, _, Err(TryRecvError::Disconnected))) => k.building = None,
        _ => {}
    }
    if at.distance(k.origin) > RECENTRE {
        k.release(Some(player));
        return;
    }

    // the pad: stick relative to the camera, A jump, B crouch, stick button sprints
    let pad = move_pad().unwrap_or_default();
    let mut stick = glam::Vec2::new(pad.lx as f32 / 32767.0, pad.ly as f32 / 32767.0);
    let len = stick.length();
    stick = if len < DEAD { glam::Vec2::ZERO } else { stick / len * ((len - DEAD) / (1.0 - DEAD)).min(1.0) };
    // the keyboard when the stick is idle: Apex's PC defaults (Shift sprint, Ctrl hold crouch,
    // C toggle crouch)
    let keys = crate::input::move_keys().unwrap_or_default();
    if stick == glam::Vec2::ZERO && (keys.x != 0.0 || keys.y != 0.0) {
        stick = glam::Vec2::new(keys.x, keys.y).normalize();
    }
    let flat = camera_flat();
    let wish = flat.map_or(Vec3::ZERO, |(f, r)| r * stick.x + f * stick.y);
    *WISH.lock().unwrap_or_else(|e| e.into_inner()) = wish;
    // sprint is the held button: one press keeps sprinting while running on (Apex's sticky
    // sprint lives in the controller)
    let input = MoveInput {
        wish: v3(wish),
        forward: v3(flat.map_or(Vec3::ZERO, |(f, _)| f)),
        sprint: pad.buttons & PAD_LS != 0 || keys.sprint,
        crouch: pad.buttons & PAD_B != 0 || keys.crouch,
        crouch_toggle: keys.crouch_toggle,
        jump: pad.buttons & PAD_A != 0 || keys.jump,
        // Octane's stim (octane.rs)
        speed_boost: super::octane::speed_boost(),
        // the shield battery out (battery.rs: S3 `offhand_blocks_sprint`)
        sprint_blocked: super::battery::busy(),
        ..Default::default()
    };

    let ctl = k.ctl.as_mut().unwrap();
    // the holstered mode (key 3, the kunai out: weapons.rs) runs a little faster
    ctl.set_speed_multiplier(SPEED_MULT * super::weapons::move_scale());
    let before = ctl.state;
    let t0 = Instant::now();
    let result = ctl.step(&input, dt.clamp(0.0, 0.1));
    let us = t0.elapsed().as_secs_f32() * 1e6;
    let st = ctl.state;
    let events = ctl.take_events();
    // the eye, with Apex's sprint view offset (gun-motion spec §4.1): the view model goes with it
    crate::firstperson::set_view_height(Some(st.eye_height + st.eye_sprint_offset));
    k.loco_frame += 1;
    {
        let v = g3(&st.velocity) * MPU;
        *LOCOMOTION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Locomotion {
            sprinting: st.sprinting,
            crouched: st.crouched,
            grounded: st.grounded,
            speed: Vec3::new(v.x, 0.0, v.z).length(),
            sliding: st.sliding,
            velocity: v,
            duck_frac: st.duck_fraction,
            sprint_frac: st.sprint_fraction,
            eye_sprint_offset: st.eye_sprint_offset * MPU,
            slide_long_jump: st.slide_long_jump,
            events,
            frame: k.loco_frame,
        });
    }
    // `kcc trace <s>`: every frame's input and result, for play-test reports
    if TRACE_UNTIL.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| Instant::now() < t) {
        let v = g3(&st.velocity) * MPU;
        log(format!(
            "kcc trace: keys x {} y {} shift {} space {} crouch {} toggle {} | pad {:.2},{:.2} | sprint in {} | {:?} {:?} sprinting {} sliding {} crouched {} grounded {} | speed {:.2} m/s | y {:.3} vy {:.2} | eye {:.1} duck {:.3} sprintf {:.3} eye_sprint {:.3} slj {} | ev {} | anim {anim}",
            keys.x, keys.y, keys.sprint as u8, keys.jump as u8, keys.crouch as u8, keys.crouch_toggle as u8, stick.x, stick.y, input.sprint as u8,
            st.pose, st.duck, st.sprinting as u8, st.sliding as u8, st.crouched as u8, st.grounded as u8, Vec3::new(v.x, 0.0, v.z).length(),
            (k.origin + g3(&st.position) * MPU).y, v.y, st.eye_height,
            st.duck_fraction, st.sprint_fraction, st.eye_sprint_offset, st.slide_long_jump as u8, events_line(&events)
        ));
    }
    if us > 1000.0 && k.slow_dumps < 5 {
        k.slow_dumps += 1;
        let ticks = result.as_ref().map_or(0, |n| *n);
        let name = format!("kcc_slow_{}.txt", k.slow_dumps);
        let r = k.dump_to(&name, &before, &input, ticks, us);
        log(format!("kcc: slow step {us:.0} µs ({ticks} ticks, {:?} -> {:?}): {r}", before.pose, st.pose));
    }
    if let Err(e) = result {
        log(format!("kcc: step failed: {e:?}"));
        k.release(Some(player));
        return;
    }
    let pos = k.origin + g3(&st.position) * MPU;
    let vel = g3(&st.velocity) * MPU;
    k.stats.step(us);
    k.stats.frame(frame_t0.elapsed().as_secs_f32() * 1e6);
    if st.stuck {
        k.stats.stuck += 1;
    }
    // stuck in the triangles (a wedge the capsule cannot get out of: in Volcano Manor, 2026-10-10,
    // he stood frozen for 200 s): the game moves him for a moment, then the controller starts over
    // where he is
    if !st.stuck {
        k.stuck_since = None;
    } else if k.stuck_since.get_or_insert_with(Instant::now).elapsed().as_secs_f32() > STUCK_RELEASE_S {
        log(format!("kcc: stuck in the map at {pos:.2?} for {STUCK_RELEASE_S} s: the game moves him, the controller starts over"));
        k.stuck_since = None;
        k.retry_at = Some(Instant::now() + std::time::Duration::from_millis(STUCK_HANDS_OFF_MS));
        k.release(Some(player));
        return;
    }

    // a lift: the game's ground under him moves while he stands still (the controller's triangles
    // are a still copy of it: the lift went up and he stayed, 2026-10-08): the game carries him
    // until it stops
    if on_moving_ground(k, &st, pos, player) {
        log(format!("kcc: the ground moves under him at {pos:.2?} (a lift): the game carries him"));
        k.ground = None;
        k.ground_moving = 0;
        k.release(Some(player));
        k.lift = Some((None, Instant::now()));
        return;
    }

    // a step through a wall would show as map geometry between the last position and this one
    let mut caught = None;
    if let Some(last) = k.last.filter(|_| st.pose != Pose::Mantling) {
        for h in [0.5, 1.2] {
            if let Some(hit) = wall_between(last, pos, h, player) {
                k.stats.crossings += 1;
                log(format!("kcc: WALL CROSSING at {h} m: {last:.3?} -> {pos:.3?}, map hit at {hit:.3?} (pose {:?})", st.pose));
                // what the controller had there and what the game has on every layer
                if k.cross_dumps < 3 {
                    k.cross_dumps += 1;
                    let name = format!("kcc_cross_{}.txt", k.cross_dumps);
                    let r = k.dump_to(&name, &before, &input, 0, us);
                    log(format!("kcc: {r}; velocity {:.2?} m/s, input wish {:.2?} jump {} sprint {}", g3(&st.velocity) * MPU, wish, input.jump, input.sprint));
                    let _ = layers_at(last, 3.0, player);
                }
                // falling through a floor the controller's world lacks (more down than across):
                // nothing below would ever catch him (2026-10-04 23:44, The First Step), so he is
                // put on it. Walls only log: the game's ray and the controller disagree on some
                // layers (S5 report), and putting him back there could hold him at a doorway.
                let across = Vec3::new(pos.x - last.x, 0.0, pos.z - last.z).length();
                let recent = k.last_recovery.is_some_and(|t| t.elapsed().as_secs_f32() < RECOVER_GAP_S);
                if last.y - pos.y > across && !recent {
                    caught = Some(hit + Vec3::Y * 0.05);
                }
                break;
            }
        }
    }
    // sinking through a floor at a shallow slant (more across than down: the check above lets it
    // pass, and once below the map nothing catches him: 2026-10-08, The First Step, walking): the
    // game's map between 0.3 and 1 m above his feet, straight up, means a floor cuts through him
    // (a wall cannot, and no ceiling is that low). Only falling: on its own ground the controller
    // walks up stairs and steep slopes with the game's ground that high under his middle (the
    // user, 2026-10-08: the check snapped him on slopes and stairs); a floor it lacks drops him.
    if caught.is_none()
        && !st.grounded
        && vel.y < 0.0
        && st.pose != Pose::Mantling
        && !k.last_recovery.is_some_and(|t| t.elapsed().as_secs_f32() < RECOVER_GAP_S)
        && let Some(hit) = wall_between(pos + Vec3::Y * 1.0, pos + Vec3::Y * 0.3, 0.0, player)
    {
        log(format!("kcc: SINKING: the map {:.2} m above the feet at {pos:.3?} (pose {:?})", hit.y - pos.y, st.pose));
        caught = Some(hit + Vec3::Y * 0.05);
    }
    if let Some(at) = caught {
        // stand him on the floor and restart the controller there next frame, with a window read
        // at once and his horizontal speed
        k.stats.recoveries += 1;
        k.last_recovery = Some(Instant::now());
        log(format!("kcc: fell through the controller's world: put on the map at {at:.3?}, restarting there"));
        let ph = &mut player.chr_ins.modules.physics;
        ph.position = HavokPosition(at.x, at.y, at.z, 0.0);
        ph.chr_proxy_pos_update_requested = true;
        // a fall along a steep slope can have turned into more speed across than Apex ever gives
        // (12.2 m/s once, 2026-10-04): at most a slide's cap, 400 units/s
        k.carry = Some((Vec3::new(vel.x, 0.0, vel.z).clamp_length_max(400.0 * MPU), Instant::now()));
        k.release(Some(player));
        return;
    }
    k.last = Some(pos);

    // put the Tarnished there
    let ph = &mut player.chr_ins.modules.physics;
    ph.position = HavokPosition(pos.x, pos.y, pos.z, 0.0);
    ph.chr_proxy_pos_update_requested = true;
    ph.gravity_disabled = !st.grounded;
    if st.grounded {
        ph.is_falling = false;
        ph.is_touching_ground = true;
        ph.standing_on_solid_ground = true;
        ph.touching_solid_ground = true;
    }
    // facing: where he goes (model forward is -Z: er-mario's orientation, see journal 10-04)
    let flat = Vec3::new(vel.x, 0.0, vel.z);
    // (first person: he faces the camera instead, firstperson.rs)
    if flat.length() > 0.5 && crate::camera::mode() != crate::camera::Mode::First {
        let q = glam::Quat::from_rotation_y((-flat.x).atan2(-flat.z));
        ph.orientation = Quaternion(q.x, q.y, q.z, q.w);
    }
    let fall = &mut player.chr_ins.modules.fall;
    fall.fall_timer = 0.0;
    fall.disable_fall_motion = true;
    MOVEMENT_TAKEN.store(true, Ordering::Relaxed);

    if k.log_at.is_none_or(|t| t.elapsed().as_secs_f32() > 5.0) {
        k.log_at = Some(Instant::now());
        log(format!("kcc: {} | at {pos:.2?} speed {:.2} m/s {:?}{}", k.stats.line(), flat.length(), st.pose, if st.grounded { " grounded" } else { " airborne" }));
    }
}

/// Ray–triangle distance (Möller–Trumbore), if the ray from `o` along unit `d` hits within `max`.
fn ray_tri(o: Vec3, d: Vec3, t: &[Vec3; 3], max: f32) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let s = o - t[0];
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = d.dot(q) / det;
    let dist = e2.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && dist >= 0.0 && dist <= max).then_some(dist)
}

/// Dev channel `kcc probe`: around the Tarnished, in 8 directions at 0.3 / 1.0 / 1.6 m, the
/// distance to the map by the game's own ray and by the controller's triangles (with the layer
/// and body of the nearest one): where they disagree, the controller collides with something the
/// game doesn't (or misses something).
pub fn probe() -> String {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let Some(k) = g.as_ref() else { return "kcc not running".into() };
    let Some(player) = (unsafe { WorldChrMan::instance() }).ok().and_then(|w| w.main_player.as_deref()) else { return "no player".into() };
    let p = player.chr_ins.modules.physics.position;
    let here = Vec3::new(p.0, p.1, p.2);
    let mut rows = Vec::new();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let d = Vec3::new(a.sin(), 0.0, a.cos());
        for h in [0.3, 1.0, 1.6] {
            let o = here + Vec3::Y * h;
            let game = wall_between(here, here + d * 2.0, h, player).map(|hit| (hit - o).length());
            let ours = k.last_window.iter().filter_map(|(t, layer, body)| ray_tri(o, d, t, 2.0).map(|dist| (dist, *layer, *body))).min_by(|a, b| a.0.total_cmp(&b.0));
            let fmt = |v: Option<f32>| v.map_or("-".to_string(), |x| format!("{x:.2}"));
            let flag = match (game, ours.map(|o| o.0)) {
                (None, Some(_)) => " <-- only ours",
                (Some(_), None) => " <-- only the game's",
                (Some(a), Some(b)) if (a - b).abs() > 0.25 => " <-- differ",
                _ => "",
            };
            rows.push(format!(
                "dir ({:.2},{:.2}) h {h}: game {} ours {}{}{flag}",
                d.x,
                d.z,
                fmt(game),
                fmt(ours.map(|o| o.0)),
                ours.map_or(String::new(), |o| format!(" (layer {:#x} body {})", o.1, o.2))
            ));
        }
    }
    let s = format!("probe at {here:.2?}, {} triangles:
  {}", k.last_window.len(), rows.join("
  "));
    log(format!("kcc {s}"));
    s
}

/// Dev channel `kcc layers [r]`: every Havok layer within `r` m (default 4) of the Tarnished, read
/// afresh (works with the controller off), with the nearest hit per layer in 8 directions at
/// 0.3 / 1.0 / 1.6 m. Finds the layer of walls the game stops the Tarnished at but `LAYERS` leaves
/// out (the boss arena edge, 2026-10-04).
pub fn layers(r: f32) -> String {
    let Some(player) = (unsafe { WorldChrMan::instance() }).ok().and_then(|w| w.main_player.as_deref()) else { return "no player".into() };
    let p = player.chr_ins.modules.physics.position;
    layers_at(Vec3::new(p.0, p.1, p.2), r, player)
}

fn layers_at(here: Vec3, r: f32, player: &PlayerIns) -> String {
    let mut havok = HavokCollision::new(Vec::new());
    let win = Window { radius: r, down: 2.0, up: 3.0, column: 0.5, below: 4.0, max_tris: 200_000 };
    let Some(tris) = havok.query(here, win) else { return "no Havok world".into() };
    let mut count: std::collections::BTreeMap<u32, usize> = Default::default();
    for (_, layer, _) in &tris {
        *count.entry(*layer).or_default() += 1;
    }
    let mut rows = vec![format!("layers at {here:.2?} within {r} m: {} triangles, by layer {count:x?}", tris.len())];
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let d = Vec3::new(a.sin(), 0.0, a.cos());
        for h in [0.3, 1.0, 1.6] {
            let o = here + Vec3::Y * h;
            let mut near: std::collections::BTreeMap<u32, (f32, u32)> = Default::default();
            for (t, layer, body) in &tris {
                if let Some(dist) = ray_tri(o, d, t, r) {
                    let e = near.entry(*layer).or_insert((f32::MAX, 0));
                    if dist < e.0 {
                        *e = (dist, *body);
                    }
                }
            }
            let game = wall_between(here, here + d * r, h, player).map(|hit| (hit - o).length());
            let hits: Vec<String> = near.iter().map(|(l, (dist, b))| format!("{l:#x}:{dist:.2}(#{b})")).collect();
            rows.push(format!("dir ({:.2},{:.2}) h {h}: game {} | {}", d.x, d.z, game.map_or("-".into(), |g| format!("{g:.2}")), hits.join(" ")));
        }
    }
    let s = rows.join("\n  ");
    log(format!("kcc {s}"));
    s
}

/// Dev channel `kcc dump`: the controller's triangle window (its units, relative to its origin)
/// and state to dev/kcc_window.txt, to replay a slow or odd spot offline
/// (deps/er-apex-move/examples/replay.rs). Lines: `mpu gravity_mps2 max_slope_deg speed_mult`,
/// `position xyz velocity xyz grounded`, then one triangle per line (9 numbers).
pub fn dump() -> String {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    let Some(k) = g.as_ref() else { return "kcc not running".into() };
    let Some(c) = k.ctl.as_ref() else { return "hands off".into() };
    k.dump_to("kcc_window.txt", &c.state, &MoveInput::default(), 0, 0.0)
}

static TRACE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Dev channel `kcc trace <seconds>`.
pub fn trace(seconds: f32) -> String {
    *TRACE_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + std::time::Duration::from_secs_f32(seconds.max(0.0)));
    format!("kcc: tracing every frame for {seconds} s")
}

/// Dev channel `kcc`.
pub fn status() -> String {
    let g = KCC.lock().unwrap_or_else(|e| e.into_inner());
    match g.as_ref() {
        None => format!("kcc {}", if enabled() { "on, not started" } else { "off (ini kcc = 1)" }),
        Some(k) => {
            let st = k.ctl.as_ref().map(|c| {
                let s = &c.state;
                format!("{:?} grounded {} sprint {} crouched {} stuck {} at {:.2?}", s.pose, s.grounded, s.sprinting, s.crouched, s.stuck, k.world(&s.position))
            });
            format!("kcc: {} | {}", st.unwrap_or_else(|| "hands off".into()), k.stats.line())
        }
    }
}
