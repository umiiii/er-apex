//! Homing rounds (the Sentinel, the user's 2026-10-09 ask): a shot of a gun with `homing` set
//! (gun.rs `Spec`) looks for an enemy in a cone straight ahead of the view (the way he faces; the
//! crosshair itself does not move) and, if there is one, fires a round that flies from the muzzle,
//! curving after the enemy as it moves, and hits it when it gets there (the damage then: gun.rs
//! `apply_hit`). With no enemy in the cone the shot is an ordinary one. hud/beam.rs draws the
//! rounds in flight.
//!
//! Every number is here, and each can be changed in er_apex.ini without a rebuild (read fresh on
//! every shot):
//!
//! | ini key | default | meaning |
//! |---|---|---|
//! | `homing` | 1 | 0 turns homing off |
//! | `homing_range` | 60 | metres: the cone's length (enemies farther away are not targeted) |
//! | `homing_angle` | 30 | degrees from straight ahead: the cone's half angle |
//! | `homing_height` | 0.6 | where on the enemy the round goes, from its feet (0) to its top (1) |
//! | `homing_speed` | 120 | the round's speed, metres a second |

use std::sync::Mutex;
use std::time::Instant;

use eldenring::cs::{ChrIns, FieldInsHandle, WorldChrMan};
use fromsoftware_shared::FromStatic;
use glam::Vec3;

use super::gun::Zone;
use crate::{log, paths};

pub const RANGE: f32 = 60.0;
pub const ANGLE_DEG: f32 = 30.0;
pub const HEIGHT: f32 = 0.6;
pub const SPEED: f32 = 120.0;
/// A round still flying after this long (seconds) is dropped.
const LIFE: f32 = 3.0;
/// How quickly a round turns after its target (radians a second): it leaves along the view and
/// bends onto it.
const TURN: f32 = 14.0;
/// Within this (metres) of its target a round hits.
const HIT_AT: f32 = 0.6;

/// The homing settings now (ini over the defaults above).
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub on: bool,
    pub range: f32,
    pub angle_deg: f32,
    pub height: f32,
    pub speed: f32,
}

pub fn settings() -> Settings {
    let num = |k: &str, d: f32| paths::number::<f32>(k).filter(|v| v.is_finite()).unwrap_or(d);
    Settings {
        on: paths::config("homing").is_none_or(|v| v.trim() != "0"),
        range: num("homing_range", RANGE).max(0.0),
        angle_deg: num("homing_angle", ANGLE_DEG).clamp(0.0, 89.0),
        height: num("homing_height", HEIGHT).clamp(0.0, 1.0),
        speed: num("homing_speed", SPEED).max(1.0),
    }
}

/// A round in flight.
#[derive(Clone)]
struct Round {
    pos: Vec3,
    vel: Vec3,
    target: FieldInsHandle,
    npc: u32,
    damage: f32,
    zone: Zone,
    born: Instant,
}

/// Elden Ring's team of the NPCs not (yet) hostile (NpcParam `teamType`).
const FRIENDLY_NPC_TEAM: u8 = 26;

static ROUNDS: Mutex<Vec<Round>> = Mutex::new(Vec::new());

/// The last shot of a homing gun (homing or not): the HUD's muzzle flash (hud/beam.rs).
static SHOT: Mutex<Option<Instant>> = Mutex::new(None);

pub fn shot_fired() {
    *SHOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// Seconds since the last shot.
pub fn since_shot() -> Option<f32> {
    SHOT.lock().unwrap_or_else(|e| e.into_inner()).map(|t| t.elapsed().as_secs_f32())
}

/// The camera the player sees: eye, forward.
fn view() -> Option<(Vec3, Vec3)> {
    let (eye, fwd, _, _) = crate::camera::view()?;
    Some((eye, fwd.normalize_or_zero()))
}

/// Where on a character a round goes (`height` of the way up its body).
fn aim_point(c: &ChrIns, height: f32) -> Vec3 {
    let (h, _) = super::body::cylinder(c);
    let q = c.modules.physics.position;
    Vec3::new(q.0, q.1 + h * height, q.2)
}

/// The enemy in the cone ahead nearest its middle: its handle, npc id, the point the round goes to.
fn acquire(eye: Vec3, fwd: Vec3, s: Settings) -> Option<(FieldInsHandle, u32, Vec3)> {
    let cos_max = s.angle_deg.to_radians().cos();
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let mut best: Option<(f32, FieldInsHandle, u32, Vec3)> = None;
    for c in wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()) {
        let c: &ChrIns = c;
        // not the friendly NPCs (team 26: Kalé, Varré, Boc, Irina... in NpcParam; angered they
        // turn 27): a round homing onto one turned Kalé and Varré against the player (2026-10-10).
        // Shot at on purpose they are hit, as in Elden Ring.
        if c.modules.data.hp <= 0 || !super::body::is_enemy_team(c.team_type) || c.team_type == FRIENDLY_NPC_TEAM {
            continue;
        }
        let at = aim_point(c, s.height);
        let to = at - eye;
        let dist = to.length();
        if dist < 0.5 || dist > s.range {
            continue;
        }
        let cos = (to / dist).dot(fwd);
        if cos >= cos_max && best.as_ref().is_none_or(|b| cos > b.0) {
            best = Some((cos, c.field_ins_handle.clone(), c.npc_param_id as u32, at));
        }
    }
    best.map(|b| (b.1, b.2, b.3))
}

/// A homing gun's shot: a round towards the enemy in the cone ahead, `damage` its damage for the
/// zone it will hit (`head_only`: a headshot, at the head). None: homing off or no enemy there (the
/// caller fires an ordinary shot).
pub fn fire(head_only: bool, damage: impl Fn(Zone) -> f32) -> Option<String> {
    let s = settings();
    if !s.on || s.range <= 0.0 || s.angle_deg <= 0.0 {
        return None;
    }
    let (eye, fwd) = view()?;
    let (target, npc, at) = acquire(eye, fwd, s)?;
    let from = crate::firstperson::muzzle_world().unwrap_or(eye + fwd * 0.6);
    // a head-only gun's round goes to the head and hits as a headshot
    let zone = if head_only { Zone::Head } else { super::gun::zone_at(s.height) };
    let round = Round { pos: from, vel: fwd * s.speed, target, npc, damage: damage(zone), zone, born: Instant::now() };
    let line = format!("homing round at npc {npc}, {:.1} m away", (at - eye).length());
    ROUNDS.lock().unwrap_or_else(|e| e.into_inner()).push(round);
    Some(line)
}

/// Once a frame: the rounds fly, turning after their targets; those that get there hit.
pub fn update(dt: f32) {
    let mut rounds = ROUNDS.lock().unwrap_or_else(|e| e.into_inner());
    if rounds.is_empty() {
        return;
    }
    let s = settings();
    let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else {
        rounds.clear();
        return;
    };
    let mut hits = Vec::new();
    rounds.retain_mut(|r| {
        if r.born.elapsed().as_secs_f32() > LIFE {
            return false;
        }
        // the target gone or dead: the round flies on and is dropped
        let Some(c) = wcm.chr_ins_by_handle(&r.target).filter(|c| c.modules.data.hp > 0) else {
            r.pos += r.vel * dt;
            return r.born.elapsed().as_secs_f32() < 0.5;
        };
        // a headshot round flies to the head (0.92 of the body: gun.rs HEAD_ZONE is 0.85 up)
        let at = aim_point(c, if r.zone == Zone::Head { 0.92 } else { s.height });
        let to = at - r.pos;
        let dist = to.length();
        let step = s.speed * dt;
        if dist <= HIT_AT.max(step) {
            hits.push((r.clone(), at));
            return false;
        }
        // turn the velocity towards the target, at most TURN radians a second
        let want = to / dist;
        let have = r.vel.normalize_or_zero();
        let angle = have.angle_between(want);
        let max = TURN * dt;
        let dir = if angle <= max || !angle.is_finite() { want } else { have.slerp(want, max / angle).normalize_or_zero() };
        r.vel = dir * s.speed;
        r.pos += r.vel * dt;
        true
    });
    drop(rounds);
    for (r, at) in hits {
        let res = super::gun::apply_hit(r.target, r.damage, r.zone, at, 0);
        log(format!("gun: homing round hit npc {} for {:.1}: {res}", r.npc, r.damage));
    }
}

/// The rounds in flight (hud/beam.rs): where each is and its direction.
pub fn rounds() -> Vec<(Vec3, Vec3)> {
    ROUNDS.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|r| (r.pos, r.vel.normalize_or_zero())).collect()
}
