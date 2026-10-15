//! Fuse's camera (plan 3.6): first person (ini `first_person = 1`, D-017) or over the shoulder
//! (`camera_shoulder = 1`, D-014). Either way it is the game's own orbit camera, re-placed: the
//! game keeps steering it (right stick / mouse, lock-on).
//!
//! First person: at Apex's view height above Fuse's feet (the eye: firstperson.rs places his arms
//! and gun in front of it, and he moves by its view), with Apex's field of view (player `fov` 70,
//! the R-301's `zoom_fov` 55 aiming, read as 4:3 horizontal degrees, as Source-engine values are:
//! inferred) eased over the weapon's zoom times. The render camera is the eye's view turned and
//! zoomed by Apex's camera effects (viewfx.rs: punch, slide roll and FOV, the view model's camera
//! bone).
//!
//! Over the shoulder: moved to Fuse's right shoulder and a little closer, so the crosshair at the
//! screen centre aims past him; a ray pulls it in front of walls.
//! Locked on, the game centres the target in *its* camera; ours sits beside it, so it turns to the
//! target's lock point (over a quarter second) to keep the target under the crosshair.
//!
//! The source is always the game's camera as it computed it this frame; what we write goes into
//! the render cameras (CSCamera pers_cam_1..4) at every step from the camera update to drawing,
//! as er-mario's lakitu.rs does (MIT, Copyright (c) 2026 Delta), since the game copies its own
//! back in between. If the render camera still holds what we wrote (the game hasn't updated it
//! since), it is not taken as a new source: that would add the offset twice.
//!
//! The game's character camera (WorldChrMan.chr_cam) is never written: tried in game, the game
//! takes it as input for the next frame, so the offset piled up and the camera got stuck inside
//! Fuse. The game's own world-space UI (lock-on dot, enemy health tags) still projects with the
//! game's camera and so sits off target; the HUD draws its own versions with this camera.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use eldenring::cs::{CSCamera, CSHavokMan, GameDataMan, PlayerIns, WorldChrMan};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::FromStatic;
use glam::{Quat, Vec3};

use crate::{log, paths, state};

/// Metres to the right, up and forward (closer) of the game's camera.
const SIDE: f32 = 0.55;
const UP: f32 = 0.10;
const IN: f32 = 1.00;
/// The camera never comes nearer than this to Fuse's head (m).
const MIN_DIST: f32 = 2.2;
/// Room kept in front of a wall the camera would go into (m).
const WALL_MARGIN: f32 = 0.20;
/// Map collision for the wall ray (er-mario's ground probes).
const MAP_RAY: u32 = 0x08;

/// Seconds to turn to (or back from) a lock-on target.
const CONVERGE_TIME: f32 = 0.25;

/// Apex's player `fov` 70 (the default) and the R-301's `zoom_fov` 55: 4:3 horizontal degrees.
const FOV_HIP: f32 = 70.0;

/// The first-person field of view (ini `fov`, Apex's setting: 4:3 horizontal degrees, 70 to 110;
/// default 70). The zoom keeps Apex's magnification: a weapon's `zoom_fov` scaled by fov / 70
/// (推断: Apex's ADS field of view follows the player's).
fn fov_hip() -> f32 {
    static FOV: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *FOV.get_or_init(|| paths::number::<f32>("fov").map_or(FOV_HIP, |f| f.clamp(70.0, 110.0)))
}
const FOV_ADS: f32 = 55.0;
/// The R-301's zoom_time_in / zoom_time_out (s).
const ZOOM_IN: f32 = 0.27;
const ZOOM_OUT: f32 = 0.23;
/// Near plane in first person (m): the gun's stock comes within a few centimetres of the eye.
const NEAR_FIRST: f32 = 0.02;

/// Which camera Fuse has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Game,
    Shoulder,
    First,
}

pub fn mode() -> Mode {
    // F5 (mode.rs): the Tarnished has the game's own camera
    if !crate::mode::apex() {
        return Mode::Game;
    }
    static M: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
    *M.get_or_init(|| {
        if paths::flag("first_person") {
            Mode::First
        } else if paths::flag("camera_shoulder") {
            Mode::Shoulder
        } else {
            Mode::Game
        }
    })
}

/// How far aiming has zoomed in (0 hip .. 1 sights), and when that was updated.
static ADS: Mutex<(f32, Option<Instant>)> = Mutex::new((0.0, None));

/// Aiming progress (0..1) in first person, eased towards whether the aim button is held.
pub fn ads_frac() -> f32 {
    ADS.lock().unwrap_or_else(|e| e.into_inner()).0
}

fn update_ads(aiming: bool) -> f32 {
    let mut a = ADS.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let dt = a.1.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.1);
    // the weapon in hand's zoom times (U3: the Charge Rifle's 0.2 / 0.15; the R-301's ZOOM_IN / OUT)
    let (zoom_in, zoom_out) = match crate::spike::weapons::active() {
        crate::spike::weapons::Slot::R301 => (ZOOM_IN, ZOOM_OUT),
        _ => {
            let z = crate::spike::weapons::zoom();
            (z.0, z.1)
        }
    };
    a.0 = if aiming { (a.0 + dt / zoom_in).min(1.0) } else { (a.0 - dt / zoom_out).max(0.0) };
    a.1 = Some(now);
    a.0
}

/// The first-person eye: Apex's view height above Fuse's feet (as drawn, with Apex's view model).
fn eye(p: &PlayerIns) -> Option<Vec3> {
    // with Apex's view model: where the body the renderer draws puts the eye (firstperson.rs)
    if let Some(e) = crate::firstperson::drawn_eye() {
        return Some(e);
    }
    let q = p.chr_ins.modules.physics.position;
    let e = Vec3::new(q.0, q.1 + crate::firstperson::eye_height(), q.2);
    e.is_finite().then_some(e)
}

/// The first-person eye's frame this frame (position, forward, right, up), even while the debug
/// orbit (`fp`) shows the rig from outside: the gun is placed in this frame (firstperson.rs).
static EYE: Mutex<Option<([Vec3; 4], Instant)>> = Mutex::new(None);

pub fn eye_view() -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let e = EYE.lock().unwrap_or_else(|e| e.into_inner());
    let (r, at) = e.as_ref()?;
    (at.elapsed().as_secs_f32() < 0.2).then(|| (r[3], r[2], r[0], r[1]))
}

/// What we last wrote: right, up, forward, position (rows of the view matrix), and when.
struct Written {
    rows: [Vec3; 4],
    at: Instant,
}

static LAST: Mutex<Option<Written>> = Mutex::new(None);
/// How far the camera has turned to the lock-on target (0..1), and when that was updated.
static CONVERGE: Mutex<(f32, Option<Instant>)> = Mutex::new((0.0, None));

pub fn enabled() -> bool {
    mode() != Mode::Game
}

fn rows_of(m: &fromsoftware_shared::F32ViewMatrix) -> [Vec3; 4] {
    [
        Vec3::new(m.0.0, m.0.1, m.0.2),
        Vec3::new(m.1.0, m.1.1, m.1.2),
        Vec3::new(m.2.0, m.2.1, m.2.2),
        Vec3::new(m.3.0, m.3.1, m.3.2),
    ]
}

fn write_rows(m: &mut fromsoftware_shared::F32ViewMatrix, r: &[Vec3; 4]) {
    (m.0.0, m.0.1, m.0.2) = (r[0].x, r[0].y, r[0].z);
    (m.1.0, m.1.1, m.1.2) = (r[1].x, r[1].y, r[1].z);
    (m.2.0, m.2.1, m.2.2) = (r[2].x, r[2].y, r[2].z);
    (m.3.0, m.3.1, m.3.2) = (r[3].x, r[3].y, r[3].z);
}

/// The game's option "camera auto rotation" turns its camera to follow his movement. With the
/// movement controller he moves relative to the view (strafing, a slide the slope bends), so the
/// view kept turning on its own: measured 2026-10-05, 2 s of strafing right turned it 120°, a
/// slide down the slope by The First Step about 8°. Kept off while ours is on, unless `autorot 1`.
static AUTO_ROTATION_ALLOWED: AtomicBool = AtomicBool::new(false);

fn keep_auto_rotation_off() {
    if AUTO_ROTATION_ALLOWED.load(Ordering::Relaxed) {
        return;
    }
    let Ok(game_data) = (unsafe { GameDataMan::instance_mut() }) else { return };
    if game_data.game_settings.camera_auto_rotation {
        game_data.game_settings.camera_auto_rotation = false;
        log("camera: turned the game's camera auto rotation off (it turns the view to follow movement)");
    }
}

/// Dev channel `autorot [0|1]`: the game's camera auto rotation; 1 lets it be on (for comparison),
/// 0 keeps it off again.
pub fn auto_rotation(arg: Option<&str>) -> String {
    let Ok(game_data) = (unsafe { GameDataMan::instance_mut() }) else { return "no GameDataMan".into() };
    match arg {
        Some("1") => {
            AUTO_ROTATION_ALLOWED.store(true, Ordering::Relaxed);
            game_data.game_settings.camera_auto_rotation = true;
        }
        Some("0") => {
            AUTO_ROTATION_ALLOWED.store(false, Ordering::Relaxed);
            game_data.game_settings.camera_auto_rotation = false;
        }
        _ => {}
    }
    format!(
        "camera auto rotation {} (kept off: {})",
        game_data.game_settings.camera_auto_rotation,
        !AUTO_ROTATION_ALLOWED.load(Ordering::Relaxed)
    )
}

/// Every camera task group: take the game's fresh camera (if there is one) and write ours.
pub fn update() {
    if !enabled() || !state::in_world() {
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return;
    }
    keep_auto_rotation_off();
    let Ok(camera) = (unsafe { CSCamera::instance_mut() }) else { return };
    let current = rows_of(&camera.pers_cam_1.matrix);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = match last.as_ref() {
        // still ours: the game hasn't recomputed its camera since we wrote it
        Some(w) => current[3].distance(w.rows[3]) > 1e-4 || current[2].distance(w.rows[2]) > 1e-4,
        None => true,
    };
    let first = mode() == Mode::First;
    let ads = if first { update_ads(crate::spike::weapons::hud().is_some_and(|g| g.aiming)) } else { 0.0 };
    let rows = if fresh {
        let [right, up, fwd, pos] = current;
        let player = (unsafe { WorldChrMan::instance() }).ok().and_then(|w| w.main_player.as_ref());
        // closer by IN, but never nearer than MIN_DIST to Fuse's head (indoors the game's camera is
        // close already)
        let closer = player.map_or(IN, |p| {
            let q = p.chr_ins.modules.physics.position;
            let dist = pos.distance(Vec3::new(q.0, q.1 + 1.6, q.2));
            IN.min((dist - MIN_DIST).max(0.0))
        });
        let mut target = pos + right * SIDE + up * UP + fwd * closer;
        if first {
            target = player.and_then(|p| eye(p)).unwrap_or(target);
        }
        // keep it in front of walls: a ray from Fuse's chest to where the camera would go
        if let Some(p) = player.filter(|_| !first) {
            let q = p.chr_ins.modules.physics.position;
            let chest = Vec3::new(q.0, q.1 + 1.4, q.2);
            let to = target - chest;
            if let Some(hit) = unsafe { CSHavokMan::instance() }
                .ok()
                .and_then(|h| h.phys_world.cast_ray(MAP_RAY, &HavokPosition(chest.x, chest.y, chest.z, 0.0), PositionDelta(to.x, to.y, to.z), p))
            {
                let h = Vec3::new(hit.0, hit.1, hit.2);
                let d = (h - chest).length();
                target = chest + to.normalize_or_zero() * (d - WALL_MARGIN).max(0.0);
            }
        }
        // locked on: turn towards the target's lock point
        let lock = player.and_then(|p| lock_point(p));
        let w = {
            let mut c = CONVERGE.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let dt = c.1.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.1);
            let step = dt / CONVERGE_TIME;
            c.0 = if lock.is_some() { (c.0 + step).min(1.0) } else { (c.0 - step).max(0.0) };
            c.1 = Some(now);
            c.0
        };
        let rows = match lock {
            Some(t) if w > 0.0 => {
                let to = (t - target).normalize_or_zero();
                let f = fwd.lerp(to, w).normalize_or_zero();
                if f == Vec3::ZERO {
                    [right, up, fwd, target]
                } else {
                    let [r, u, f] = basis(f, [right, up, fwd]);
                    [r, u, f, target]
                }
            }
            _ => [right, up, fwd, target],
        };
        if !first {
            rows
        } else {
            // the eye's view: he moves by it and the view model is placed in it
            *EYE.lock().unwrap_or_else(|e| e.into_inner()) = Some((rows, Instant::now()));
            // debug orbit (`fp` back and angle): circle the eye, looking at the gun's sight
            let orbit = crate::firstperson::orbit().map(|(back, angle)| {
                let look = crate::firstperson::last_sight().map_or(rows[3], |s| s.0);
                let flat = Vec3::new(rows[2].x, 0.0, rows[2].z).normalize_or(Vec3::Z);
                let at = rows[3] - Quat::from_rotation_y(angle.to_radians()) * flat * back + Vec3::Y * 0.10;
                let [r, u, f] = basis((look - at).normalize_or(rows[2]), [rows[0], rows[1], rows[2]]);
                [r, u, f, at]
            });
            // the render camera: the eye's view turned by the punch, the slide's roll and the view
            // model's camera bone (viewfx.rs)
            orbit.unwrap_or_else(|| {
                let [r, u, f] = crate::viewfx::turned(rows[0], rows[1], rows[2], crate::viewfx::current(rows[2]).0);
                [r, u, f, rows[3]]
            })
        }
    } else {
        match last.as_ref() {
            Some(w) => w.rows,
            None => return,
        }
    };
    // eased in and out over the zoom (R5R T8: an S curve; its exact shape is inferred), times the
    // slide's scale, on Apex's 4:3 horizontal angle; ER's is vertical
    let fov = first.then(|| {
        // U3: the field of view moves over the weapon's `ads_fov_zoomfrac_start` .. `_end` of the
        // zoom (the R-301 0 .. 1: as before; the Charge Rifle 0.25 .. 0.75), eased as before (推断)
        let (_, _, from, to) = crate::spike::weapons::zoom();
        let ads = if to > from { ((ads - from) / (to - from)).clamp(0.0, 1.0) } else { ads };
        let s = ads * ads * (3.0 - 2.0 * ads);
        // the weapon in hand's `zoom_fov` (the Wingman 60; FOV_ADS the R-301's and the Charge Rifle's)
        let fov_ads = if crate::spike::gun::enabled() { crate::spike::weapons::zoom_fov() } else { FOV_ADS };
        let (hip, fov_ads) = (fov_hip(), fov_ads * fov_hip() / FOV_HIP);
        let h = (hip + (fov_ads - hip) * s) * crate::viewfx::current(rows[2]).1;
        2.0 * ((h.to_radians() / 2.0).tan() * 0.75).atan()
    });
    for cam in [&mut camera.pers_cam_1, &mut camera.pers_cam_2, &mut camera.pers_cam_3, &mut camera.pers_cam_4] {
        write_rows(&mut cam.matrix, &rows);
        if let Some(f) = fov {
            cam.fov = f;
            cam.near_plane = NEAR_FIRST;
        }
    }
    *last = Some(Written { rows, at: if fresh { Instant::now() } else { last.as_ref().map_or_else(Instant::now, |w| w.at) } });
}

/// Fuse's lock-on target's lock point (where the game's lock-on marker sits), if he has one.
/// Seen in game (2026-10-03): the *player's* `lock_on_target_position` is the point on the target
/// (1.3 m above a soldier's feet); the target's own field holds an unrelated constant.
fn lock_point(me: &PlayerIns) -> Option<Vec3> {
    if !me.chr_ins.is_locked_on || me.locked_on_enemy.is_empty() {
        return None;
    }
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let c = wcm.chr_ins_by_handle(&me.locked_on_enemy)?;
    let q = c.modules.physics.position;
    let base = Vec3::new(q.0, q.1, q.2);
    let l = me.chr_ins.lock_on_target_position;
    let lp = Vec3::new(l.0, l.1, l.2);
    // the lock point if it is on the target (within a few metres), else the middle of its capsule
    if lp.is_finite() && lp.distance(base) < 6.0 {
        Some(lp)
    } else {
        let h = c.modules.physics.chr_hit_height;
        Some(base + Vec3::Y * if h.is_finite() && h > 0.2 { h * 0.6 } else { 1.1 })
    }
}

/// Right, up, forward for a new forward, keeping the game matrix's handedness and up side
/// (er-mario's lakitu.rs `write`).
fn basis(fwd: Vec3, old: [Vec3; 3]) -> [Vec3; 3] {
    let handed = old[0].dot(old[1].cross(old[2])).signum();
    let mut right = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
    let up = fwd.cross(right).normalize_or(Vec3::Y);
    if right.dot(up.cross(fwd)).signum() != handed {
        right = -right;
    }
    let up = if up.y < 0.0 { -up } else { up };
    [right, up, fwd]
}

/// The camera the player sees right now (ours while it is fresh): position and forward, for the
/// gun's aim ray.
pub fn view() -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let w = last.as_ref().filter(|w| w.at.elapsed().as_secs_f32() < 0.2)?;
    Some((w.rows[3], w.rows[2], w.rows[0], w.rows[1]))
}

/// The game's camera as it went to drawing (`Draw_Pre`), this frame's and the one before: rows and
/// vertical field of view, and when. The HUD is drawn when a frame is presented, and the image on
/// screen then is the one drawn with the camera of the frame before (measured 2026-10-05: with the
/// latest camera, world-anchored marks were a frame ahead of the image).
static DRAWN: Mutex<[Option<([Vec3; 4], f32, Instant)>; 2]> = Mutex::new([None, None]);

/// `Draw_Pre`, after this frame's camera is written.
pub fn snapshot_drawn() {
    let Ok(camera) = (unsafe { CSCamera::instance() }) else { return };
    let rows = rows_of(&camera.pers_cam_1.matrix);
    let mut d = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    d[1] = d[0].take();
    d[0] = Some((rows, camera.pers_cam_1.fov, Instant::now()));
}

/// The camera of the image on screen (position, forward, right, up) and its vertical field of view,
/// for the HUD's projection: the one a frame before the last sent to drawing.
pub fn drawn_view() -> Option<((Vec3, Vec3, Vec3, Vec3), f32)> {
    let d = DRAWN.lock().unwrap_or_else(|e| e.into_inner());
    let (r, fov, _) = d[1].as_ref().or(d[0].as_ref()).filter(|d| d.2.elapsed().as_secs_f32() < 0.2)?;
    Some(((r[3], r[2], r[0], r[1]), *fov))
}

/// Dev channel `cam`: the game's character camera, the render camera and ours, and the lock-on.
pub fn status() -> String {
    let wcm = unsafe { WorldChrMan::instance() }.ok();
    let chr = wcm.and_then(|w| w.chr_cam).map(|c| unsafe { c.as_ref() });
    let render = unsafe { CSCamera::instance() }.ok().map(|c| rows_of(&c.pers_cam_1.matrix));
    let chr_rows = chr.map(|c| rows_of(&c.pers_cam.matrix));
    let f = |r: Option<[Vec3; 4]>| {
        r.map_or("-".into(), |r| format!("pos {:.3},{:.3},{:.3} fwd {:.4},{:.4},{:.4} up {:.4},{:.4},{:.4}", r[3].x, r[3].y, r[3].z, r[2].x, r[2].y, r[2].z, r[1].x, r[1].y, r[1].z))
    };
    let lock = wcm.and_then(|w| w.main_player.as_ref()).map_or("-".into(), |p| {
        let l = p.chr_ins.lock_on_target_position;
        let target = (!p.locked_on_enemy.is_empty())
            .then(|| unsafe { WorldChrMan::instance() }.ok().and_then(|w| w.chr_ins_by_handle(&p.locked_on_enemy)))
            .flatten()
            .map_or("-".into(), |c| {
                let q = c.modules.physics.position;
                format!("npc {} pos {:.2},{:.2},{:.2}", c.npc_param_id, q.0, q.1, q.2)
            });
        format!(
            "me is_locked_on {} lock_pos {:.2},{:.2},{:.2} enemy_empty {} target [{target}] point {:?} w {:.2}",
            p.chr_ins.is_locked_on,
            l.0,
            l.1,
            l.2,
            p.locked_on_enemy.is_empty(),
            lock_point(p),
            CONVERGE.lock().unwrap_or_else(|e| e.into_inner()).0
        )
    });
    let lens = unsafe { CSCamera::instance() }.ok().map_or("-".into(), |c| {
        let p = &c.pers_cam_1;
        format!(
            "fov {:.3}/{:.3}/{:.3}/{:.3} chr {:.3} aspect {:.3} near {:.2} far {:.0} mask {:#x}",
            p.fov,
            c.pers_cam_2.fov,
            c.pers_cam_3.fov,
            c.pers_cam_4.fov,
            chr.map_or(f32::NAN, |c| c.pers_cam.fov),
            p.aspect_ratio,
            p.near_plane,
            p.far_plane,
            c.camera_mask
        )
    });
    let me = wcm.and_then(|w| w.main_player.as_ref()).map_or("-".into(), |p| {
        let q = p.chr_ins.modules.physics.position;
        format!("{:.3},{:.3},{:.3}", q.0, q.1, q.2)
    });
    let lens = format!("{lens} hud_fov {:.3} me {me}", f32::from_bits(crate::hud::LAST_FOV.load(std::sync::atomic::Ordering::Relaxed)));
    let s = format!(
        "chr_cam type {:?}: {} | render: {} ({lens}) | ours: {} | lock: {lock}",
        chr.map(|c| c.camera_type),
        f(chr_rows),
        f(render),
        f(LAST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|w| w.rows))
    );
    log(format!("camera: {s}"));
    s
}
