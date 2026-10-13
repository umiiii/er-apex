//! Spike S6: Fuse's hits on Elden Ring characters (er-mario's damage bridge, fuse-mod-plan.md 3.7).
//!
//! A hit takes a share of the target's max HP: the Apex damage over the HP an enemy counts as in
//! Apex terms (`enemy_apex_hp`, bosses `boss_apex_hp_mult` times more), the same at every level.
//! It never takes the last point: a small game bullet spawned on the target, owned by the player,
//! brings the stagger, blood and hit sound and deals the final blow, so the death, the runes and a
//! boss's defeat stay the game's own. If that bullet can't land, the target is finished directly
//! after 0.5 s (audit: this fallback path is checked separately, see the log line it writes).
//!
//! Bullet 65 / attack 994: unused dev rows (er-mario repurposes 65-70 / 994-999 the same way),
//! rewritten at runtime; nothing is added to the params and regulation.bin isn't touched.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use eldenring::cs::{AtkParam_Pc, Bullet, CSBulletManager, ChrIns, FieldInsHandle, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::{F32Vector4, FromStatic};

use crate::{dlog, log, paths};

const BULLET: i32 = 65;
const ATK: u32 = 994;
/// `bullet_mode = approach`: a second unused pair, a bullet that starts in front of the target and
/// flies into it (in case one born inside the target doesn't register a hit)
const BULLET_FLY: i32 = 66;
const ATK_FLY: u32 = 995;
const FLY_START: f32 = 1.5;
const FLY_SPEED: f32 = 30.0;
/// The bullet's own flat damage: enough to always land the final blow from 1 HP.
const BULLET_DAMAGE: u16 = 10;
const TEAM_STRONG_ENEMY: u8 = 7;

/// Mirror of the game's bullet spawn request (fields are private in the bindings; layout from
/// er-mario's combat.rs, MIT, Copyright (c) 2026 Delta).
#[repr(C)]
struct SpawnRequest {
    owner: FieldInsHandle,
    behavior_id: i32,
    magic_id: i32,
    unk10: u32,
    bullet_id: i32,
    goods_id: i32,
    dummy_poly_id: i32,
    target: [u8; 8],
    unk28: u32,
    unk2c: u32,
    unk30: F32Vector4,
    unk40: u32,
    unk44: u32,
    pad48: [u8; 8],
    acceleration_angle: F32Vector4,
    unk60: F32Vector4,
    angle: F32Vector4,
    position: F32Vector4,
    rest: [u8; 0x80],
}
const _: () = assert!(size_of::<SpawnRequest>() == 0x110);

struct State {
    patched: bool,
    /// targets at 1 HP waiting for the bullet's final blow: (handle, since)
    finishing: Vec<(FieldInsHandle, Instant)>,
    /// queued shots: (target entity id, Apex damage, when)
    queue: VecDeque<(u32, f32, Instant)>,
    runes: Option<u32>,
    /// characters hit recently, to report their deaths: (handle, entity id)
    victims: Vec<(FieldInsHandle, u32)>,
    /// bullets just spawned: (target, its HP right after our share, its animation, when), to
    /// measure what the bullet itself did (damage, and a hit reaction animation)
    pending: Vec<(FieldInsHandle, i32, i32, f32, Instant)>,
    /// bullet damage seen (hits, total)
    bullet_hits: (u32, i64),
    /// the target of `burstnear`
    near: Option<FieldInsHandle>,
}

static STATE: Mutex<State> =
    Mutex::new(State {
        patched: false,
        finishing: Vec::new(),
        queue: VecDeque::new(),
        runes: None,
        victims: Vec::new(),
        pending: Vec::new(),
        bullet_hits: (0, 0),
        near: None,
    });

fn key(h: &FieldInsHandle) -> u64 {
    unsafe { std::mem::transmute_copy::<FieldInsHandle, u64>(h) }
}

fn patch_params() -> bool {
    patch_row(BULLET, ATK, 0.0, 0.1, 0.6) && patch_row(BULLET_FLY, ATK_FLY, FLY_SPEED, 0.3, 0.3)
}

fn patch_row(bullet: i32, atk: u32, speed: f32, life: f32, radius: f32) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let Some(b) = repo.get_mut::<Bullet>(bullet as u32) else { return false };
    b.set_atk_id_bullet(atk as i32);
    b.set_life(life);
    b.set_dist(0.0);
    b.set_init_vellocity(speed);
    b.set_max_vellocity(speed);
    b.set_min_vellocity(speed);
    b.set_accel_in_range(0.0);
    b.set_accel_out_range(0.0);
    b.set_gravity_in_range(0.0);
    b.set_gravity_out_range(0.0);
    b.set_hit_radius(radius);
    b.set_hit_radius_max(radius);
    b.set_spread_time(0.0);
    b.set_num_shoot(1);
    b.set_homing_angle(0);
    b.set_is_penetrate_chr(true);
    b.set_is_penetrate_obj(true);
    b.set_is_penetrate_map(true);
    b.set_is_hit_both_team(false);
    let Some(a) = repo.get_mut::<AtkParam_Pc>(atk) else { return false };
    // 0% correction gave 0 damage on a soldier (npc 30001014); 100% + the base attack lands
    // 5-7 points, so the bullet can deal the final blow itself (journal 18:55-19:20)
    a.set_atk_phys_correction(100);
    a.set_atk_mag_correction(0);
    a.set_atk_fire_correction(0);
    a.set_atk_thun_correction(0);
    a.set_atk_stam_correction(100);
    a.set_guard_atk_rate_correction(100);
    a.set_guard_break_correction(100);
    a.set_atk_phys(BULLET_DAMAGE);
    a.set_atk_obj(300);
    a.set_atk_mag(0);
    a.set_atk_fire(0);
    a.set_atk_thun(0);
    a.set_atk_stam(30);
    a.set_atk_super_armor(10.0);
    a.set_dmg_level(1);
    a.set_atk_attribute(2); // pierce: bullets
    a.set_is_add_base_atk(true);
    a.set_oppose_target(true);
    a.set_friendly_target(false);
    a.set_self_target(false);
    log(format!("combat: bullet {bullet} / attack {atk} rewritten as Fuse's gun hit (speed {speed}, life {life}, radius {radius})"));
    true
}

/// The target's poise (super armor): current, max.
fn poise_of(chr: &ChrIns) -> (f32, f32) {
    let sa = &chr.modules.super_armor;
    (sa.sa_durability, sa.sa_durability_max)
}

fn anim_of(chr: &ChrIns) -> i32 {
    let t = &chr.modules.time_act;
    t.anim_queue[(t.read_idx % 10) as usize].anim_id
}

/// Whether a character is on screen as a boss (its health bar), or counts as one; the HUD shows no
/// plate over a boss's head (its bar is at the bottom).
pub fn is_boss(chr: &ChrIns) -> bool {
    let k = key(&chr.field_ins_handle);
    let bar = unsafe { eldenring::cs::CSFeManImp::instance() }
        .is_ok_and(|fe| fe.boss_health_displays.iter().any(|e| !e.field_ins_handle.is_empty() && key(&e.field_ins_handle) == k));
    bar || chr.team_type == TEAM_STRONG_ENEMY || chr.modules.data.max_hp >= 2500
}

/// The nearest living enemy (team 6/7) within `range` metres that isn't boss-class.
fn nearest_enemy(range: f32) -> Option<(FieldInsHandle, u32)> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let me = wcm.main_player.as_ref()?.chr_ins.modules.physics.position;
    let mut best: Option<(f32, FieldInsHandle, u32)> = None;
    for c in wcm.chr_sets.iter().flatten().flat_map(|s| s.characters()) {
        let c: &ChrIns = c;
        if !crate::spike::body::is_enemy(c) || c.modules.data.hp <= 0 || is_boss(c) {
            continue;
        }
        let q = c.modules.physics.position;
        let d = ((q.0 - me.0).powi(2) + (q.1 - me.1).powi(2) + (q.2 - me.2).powi(2)).sqrt();
        if d <= range && best.as_ref().is_none_or(|b| d < b.0) {
            best = Some((d, c.field_ins_handle.clone(), c.npc_param_id as u32));
        }
    }
    best.map(|b| (b.1, b.2))
}

/// Queues `count` shots on the nearest regular enemy (the dev channel's `burstnear`).
pub fn burst_near(count: u32, apex_damage: f32, rate: f32) -> String {
    let Some((h, npc)) = nearest_enemy(60.0) else { return "no regular enemy within 60 m".into() };
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    st.near = Some(h);
    let now = Instant::now();
    for i in 0..count {
        st.queue.push_back((NEAR, apex_damage, now + Duration::from_secs_f32(i as f32 / rate.max(0.1))));
    }
    format!("{count} shots of {apex_damage} at {rate}/s queued on the nearest enemy (npc param {npc})")
}

/// Tweaks the two attack rows live (dev channel `atk`): physical correction % and add-base flag.
pub fn tweak_atk(correction: u16, add_base: bool) -> String {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return "no params".into() };
    for atk in [ATK, ATK_FLY] {
        if let Some(a) = repo.get_mut::<AtkParam_Pc>(atk) {
            a.set_atk_phys_correction(correction);
            a.set_is_add_base_atk(add_base);
        }
    }
    format!("attack rows {ATK}/{ATK_FLY}: physical correction {correction}%, add base {add_base}")
}

/// The pseudo entity id `burstnear` queues under.
const NEAR: u32 = u32::MAX;

fn find(entity: u32) -> Option<FieldInsHandle> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    wcm.chr_sets.iter().flatten().flat_map(|s| s.characters()).find_map(|c| {
        let c: &ChrIns = c;
        (c.event_entity_id == entity && c.modules.data.hp > 0).then(|| c.field_ins_handle.clone())
    })
}

/// One hit of `apex_damage` on the character with this MSB entity id.
fn hit(st: &mut State, entity: u32, apex_damage: f32) -> String {
    let target = if entity == NEAR { st.near.clone() } else { find(entity) };
    hit_target(st, Some(entity), target, apex_damage)
}

/// One gun hit (spike::gun) on a character found by its handle.
pub fn shoot(handle: FieldInsHandle, apex_damage: f32) -> String {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    hit_target(&mut st, None, Some(handle), apex_damage)
}

/// `entity` labels the log and the death report (the character's own entity id when None).
fn hit_target(st: &mut State, entity: Option<u32>, target: Option<FieldInsHandle>, apex_damage: f32) -> String {
    if !st.patched {
        st.patched = patch_params();
        if !st.patched {
            return "params not ready".into();
        }
    }
    let Some(handle) = target else { return format!("entity {entity:?} not loaded or dead") };
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return "no WorldChrMan".into() };
    let Some(player) = wcm.main_player.as_ref() else { return "no player".into() };
    let owner = player.chr_ins.field_ins_handle.clone();
    let from = player.chr_ins.modules.physics.position;
    let Some(chr) = wcm.chr_ins_by_handle_mut(&handle) else { return "gone".into() };
    let entity = entity.unwrap_or(chr.event_entity_id);
    // a dead target must stay dead: our share would set it back to 1 HP
    if chr.modules.data.hp <= 0 {
        return format!("entity {entity} already dead, shot dropped");
    }
    let boss = is_boss(chr);
    let apex_hp = paths::number::<f32>("enemy_apex_hp").unwrap_or(150.0)
        * if boss { paths::number::<f32>("boss_apex_hp_mult").unwrap_or(20.0) } else { 1.0 };
    let pct = apex_damage / apex_hp;
    let data = &mut chr.modules.data;
    let (hp, max) = (data.hp, data.max_hp.max(1));
    let dmg = ((max as f32 * pct).ceil() as i32).max(1);
    data.hp = (hp - dmg).max(1);
    let left = data.hp;
    if left == 1 && !st.finishing.iter().any(|(h, _)| key(h) == key(&handle)) {
        st.finishing.push((handle.clone(), Instant::now()));
    }
    if !st.victims.iter().any(|(h, _)| key(h) == key(&handle)) {
        st.victims.push((handle.clone(), entity));
    }
    // the bullet in the middle of the target's body (a metre above the feet missed Godrick, who
    // is ~5 m tall: no bullet damage at all, the fallback had to finish him), facing away from the
    // player
    let p = chr.modules.physics.position;
    let height = super::body::cylinder(chr).0;
    let lift = (height * 0.5).clamp(0.4, 4.0);
    let dir = glam::Vec3::new(p.0 - from.0, 0.0, p.2 - from.2).normalize_or(glam::Vec3::Z);
    let fly = paths::config("bullet_mode").is_some_and(|m| m == "approach");
    let back = if fly { FLY_START } else { 0.0 };
    let at = F32Vector4(p.0 - dir.x * back, p.1 + lift, p.2 - dir.z * back, 0.0);
    let bullet_id = if fly { BULLET_FLY } else { BULLET };
    st.pending.push((handle.clone(), left, anim_of(chr), poise_of(chr).0, Instant::now()));
    let request = SpawnRequest {
        owner,
        behavior_id: -1,
        magic_id: -1,
        unk10: 0,
        bullet_id,
        goods_id: -1,
        dummy_poly_id: -1,
        target: [0xFF; 8],
        unk28: 0,
        unk2c: 0,
        unk30: at,
        unk40: 0,
        unk44: 0,
        pad48: [0; 8],
        acceleration_angle: F32Vector4(dir.x, dir.y, dir.z, 0.0),
        unk60: F32Vector4(0.0, 0.0, 0.0, 0.0),
        angle: F32Vector4(dir.x, dir.y, dir.z, 0.0),
        position: at,
        rest: [0; 0x80],
    };
    let result = unsafe { CSBulletManager::instance_mut() }
        .map(|m| m.spawn_bullet(unsafe { &*(&request as *const SpawnRequest as *const _) }))
        .map_err(|e| format!("{e:?}"));
    format!(
        "hit {entity}{}: {apex_damage} Apex dmg of {apex_hp} -> {dmg} of {max} HP, {hp} -> {left}; bullet {bullet_id} at +{lift:.2} m (body {height:.2} m) {result:?}",
        if boss { " (boss)" } else { "" }
    )
}

/// Queues `count` shots at `rate` per second (the dev channel's `burst`).
pub fn burst(entity: u32, count: u32, apex_damage: f32, rate: f32) -> String {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    for i in 0..count {
        st.queue.push_back((entity, apex_damage, now + Duration::from_secs_f32(i as f32 / rate.max(0.1))));
    }
    format!("{count} shots of {apex_damage} at {rate}/s queued on entity {entity}")
}

/// Once per frame: queued shots, final-blow fallback, kills and runes.
pub fn update() {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    while st.queue.front().is_some_and(|q| q.2 <= now) {
        let (entity, dmg, _) = st.queue.pop_front().unwrap();
        let out = hit(&mut st, entity, dmg);
        log(format!("combat: {out}"));
    }
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
    // what the bullets themselves did, 0.25 s after each (the game applies their hit a frame or
    // two later)
    let mut seen = Vec::new();
    st.pending.retain(|(h, after, anim, poise, t)| {
        if t.elapsed() < Duration::from_millis(250) {
            return true;
        }
        if let Some(chr) = wcm.chr_ins_by_handle(h) {
            seen.push((*after - chr.modules.data.hp, *anim, anim_of(chr), *poise, poise_of(chr)));
        }
        false
    });
    for (d, before, now, poise_before, (poise, poise_max)) in seen {
        st.bullet_hits.0 += 1;
        st.bullet_hits.1 += d as i64;
        let line = format!(
            "combat: bullet check {}: target lost {d} more HP within 0.25 s (total {} over {} bullets), anim {before} -> {now}, poise {poise_before:.1} -> {poise:.1} / {poise_max:.1}",
            st.bullet_hits.0, st.bullet_hits.1, st.bullet_hits.0
        );
        if st.bullet_hits.0 <= 5 || st.bullet_hits.0 % 50 == 0 {
            log(line);
        } else {
            dlog(line);
        }
    }
    // the bullet should land the final blow; if it couldn't within 0.5 s, finish directly
    let mut finished = Vec::new();
    st.finishing.retain(|(h, since)| {
        if since.elapsed() < Duration::from_millis(500) {
            return true;
        }
        if let Some(chr) = wcm.chr_ins_by_handle_mut(h) {
            if chr.modules.data.hp == 1 {
                chr.modules.data.hp = 0;
                finished.push(chr.event_entity_id);
            }
        }
        false
    });
    for e in finished {
        log(format!("combat: entity {e}: final blow missed, finished directly (fallback path)"));
    }
    let mut dead = Vec::new();
    st.victims.retain(|(h, e)| match wcm.chr_ins_by_handle(h) {
        Some(c) if c.modules.data.hp <= 0 => {
            dead.push(*e);
            false
        }
        Some(_) => true,
        None => false,
    });
    for e in dead {
        log(format!("combat: entity {e} died"));
    }
    if let Some(p) = wcm.main_player.as_ref() {
        let runes = unsafe { p.player_game_data.as_ref() }.rune_count;
        if let Some(old) = st.runes {
            if runes != old {
                log(format!("combat: runes {old} -> {runes} ({:+})", runes as i64 - old as i64));
            }
        }
        st.runes = Some(runes);
    }
}
