//! What the Apex HUD counts and reports (HUD v3): Fuse's kills and damage dealt (Apex's match
//! stats, top right), the kill feed (Apex's obituary: who, the R-301, a headshot mark, whom) and
//! where the hits Fuse takes come from (Apex's damage indicator).
//!
//! A kill is an enemy Fuse hit in the last HIT_KEEP seconds (gun.rs keeps them) that is now dead,
//! whoever dealt the last blow (the game's own finishing blows included); Apex credits the last
//! attacker the same way. Damage dealt adds up the Apex damage of each hit (not capped at what the
//! enemy had left, as Apex's counter is: 待定). Hits taken come from lethal.rs; where from is the
//! Tarnished's `last_hit_by` character, else (the game leaves it empty for enemy melee) the
//! nearest living enemy.
//!
//! Enemy names: NpcParam `nameId` into the game's NpcName text, read from an export of it
//! (er-data, `ertool fmg NpcName.fmg --json`; ini `npc_names`, else <mod>\npc_names.json). Elden
//! Ring names only bosses and NPCs (a soldier's `nameId` is 0). For the rest (the user, 2026-10-09:
//! "the kill feed says enemy for every monster"): the game's own name of its kind where NpcName has
//! one, the spirit ashes' entries (ids 9MMMMMVVV for character model cMMMM, an NpcParam row
//! MMMMxxxx being of model MMMM: 34000000 a Grave Warden Duelist, 903400300 守墓斗士), the model's
//! or its family's (MMMx); else the community name of its NpcParam row (Paramdex, English: ini
//! `npc_param_names`, play.ps1 sets it); only then NAMELESS.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use eldenring::cs::{FieldInsHandle, NpcParam, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::FromStatic;
use glam::Vec3;

use crate::{log, paths};

pub struct Kill {
    pub at: Instant,
    pub victim: Option<String>,
    pub head: bool,
    /// U3: the slot of the weapon of the last hit (0 the R-301, 1 the Charge Rifle; 2 the frag grenade, U9)
    pub weapon: u8,
}

/// Apex's center message for a knock-down (S3 `Sur_DownedPilot`: cl_hud.gnut `AddScoreEventMessage`,
/// `ui/centerevent_info.rpak`, `MessageData.duration` 2.5 s, `messageScale` 1.5): an Elden Ring kill
/// is the moment Apex knocks an enemy down (the user's R5R video shows a firing-range dummie's).
pub struct Knock {
    pub at: Instant,
    pub victim: String,
    /// What Fuse's hits on it added up to (S3's "Damage Inflicted"; our hits of the last
    /// HIT_KEEP seconds).
    pub inflicted: f32,
}

pub struct Taken {
    pub at: Instant,
    /// The attacker's position (world).
    pub from: Vec3,
    pub damage: f32,
}

#[derive(Default)]
struct Stats {
    kills: u32,
    damage: f32,
    /// Enemies already counted (selector, block), so a corpse counts once.
    counted: Vec<(u32, i32)>,
    feed: VecDeque<Kill>,
    /// When each recent kill was (STREAK_SECONDS): the HUD's streak badge.
    recent: VecDeque<Instant>,
    knock: Option<Knock>,
    taken: VecDeque<Taken>,
    /// Hits taken this frame, by whom: placed by `update` (lethal.rs holds the player then).
    pending: Vec<(f32, FieldInsHandle)>,
    /// Bosses with a bar, by handle: whether their health was last at BOSS_BREAK_FRACTION or above.
    boss_above: HashMap<(u32, i32), bool>,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Stats) -> T) -> T {
    let mut s = STATS.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(Stats::default))
}

/// Apex's kill feed keeps a line this long (待定: R5Reloaded's obituary timing not read).
pub const FEED_SECONDS: f32 = 6.0;
/// The kill feed's name for an enemy the game gives no name.
const NAMELESS: &str = "敌人";
/// Apex's damage indicator (R5Reloaded cl_damage_indicator.gnut DAMAGE_INDICATOR_DURATION).
pub const TAKEN_SECONDS: f32 = 4.0;
/// S3 cl_hud.gnut `MessageData.duration`.
pub const KNOCK_SECONDS: f32 = 2.5;
/// The sound the attacker hears on a killing shot at something not a player (S3
/// _codecallbacks.gnut: `EmitSoundOnEntityOnlyToPlayer( attacker, attacker,
/// "flesh_bulletimpact_downedshot_1p_vs_3p" )`); the user's R5R video has it at the knock-down
/// (`imp_bullet_lightballistic_killshot_human_1ch_v1_01`, 19.07 s). Exported with Octane's sounds.
const KNOCK_SOUND: &str = "flesh_bulletimpact_downedshot_1p_vs_3p";
const KNOCK_VOLUME: f32 = 0.5;
/// Apex's shield break, to the attacker (S3 _codecallbacks.gnut: `EmitSoundOnEntityOnlyToPlayer(
/// attacker, attacker, "humanshield_break_1p_vs_3p" )` when a shield runs out). The MVP has no
/// shields: the user's choice (2026-10-05) is to play it once when a boss's health falls below
/// BOSS_BREAK_FRACTION. Exported with Octane's sounds.
const BOSS_BREAK_SOUND: &str = "humanshield_break_1p_vs_3p";
const BOSS_BREAK_VOLUME: f32 = 0.5;
const BOSS_BREAK_FRACTION: f32 = 0.7;

/// A hit Fuse dealt (gun.rs).
pub fn dealt(damage: f32) {
    with(|s| s.damage += damage);
}

/// A hit Fuse took (lethal.rs): Apex damage, and the Tarnished's last attacker (`last_hit_by`).
pub fn took(damage: f32, by: FieldInsHandle) {
    with(|s| s.pending.push((damage, by)));
}

/// Once a frame (game thread, after lethal.rs): where this frame's hits came from; enemies Fuse
/// hit that have died since.
pub fn update() {
    let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
    for (damage, by) in with(|s| std::mem::take(&mut s.pending)) {
        let from = match wcm.chr_ins_by_handle(&by) {
            Some(chr) => {
                let q = chr.modules.physics.position;
                Some(Vec3::new(q.0, q.1, q.2))
            }
            None => nearest_enemy(wcm),
        };
        let Some(from) = from else { continue };
        with(|s| {
            s.taken.push_back(Taken { at: Instant::now(), from, damage });
            // Apex keeps at most 8 indicators (damageIndicatorThreshold)
            while s.taken.len() > 8 {
                s.taken.pop_front();
            }
        });
    }
    // the boss's "shield break": once as its health falls below BOSS_BREAK_FRACTION, again only
    // after it is back above (a boss first seen below it plays nothing)
    for b in crate::fe::bosses() {
        if b.hp_max == 0 {
            continue;
        }
        let frac = b.hp as f32 / b.hp_max as f32;
        let above = frac >= BOSS_BREAK_FRACTION;
        if with(|s| s.boss_above.insert(b.key, above)) == Some(true) && !above && b.hp > 0 {
            log(format!("stats: boss {} below {:.0} % ({}/{}): shield break sound", b.name, BOSS_BREAK_FRACTION * 100.0, b.hp, b.hp_max));
            crate::audio::play(BOSS_BREAK_SOUND, BOSS_BREAK_VOLUME);
        }
    }
    let hits = super::gun::hits();
    for h in hits.iter().rev() {
        let key = (h.target.selector.0, h.target.block_id.0);
        if with(|s| s.counted.contains(&key)) {
            continue;
        }
        let Some(chr) = wcm.chr_ins_by_handle(&h.target) else { continue };
        if chr.modules.data.hp > 0 {
            continue;
        }
        let name_id = unsafe { SoloParamRepository::instance() }.ok().and_then(|r| r.get::<NpcParam>(chr.npc_param_id as u32).map(|n| n.name_id()));
        let victim = npc_name(chr.npc_param_id);
        let inflicted: f32 = hits.iter().filter(|x| (x.target.selector.0, x.target.block_id.0) == key).map(|x| x.damage).sum();
        log(format!("stats: kill npc {} name id {name_id:?} ({}), {inflicted:.0} inflicted", chr.npc_param_id, victim.as_deref().unwrap_or("no name")));
        crate::audio::play(KNOCK_SOUND, KNOCK_VOLUME);
        let victim = victim.unwrap_or_else(|| NAMELESS.into());
        with(|s| {
            s.kills += 1;
            s.counted.push(key);
            if s.counted.len() > 256 {
                s.counted.remove(0);
            }
            s.feed.push_back(Kill { at: Instant::now(), victim: Some(victim.clone()), head: h.head, weapon: h.weapon });
            s.recent.push_back(Instant::now());
            while s.feed.len() > 5 {
                s.feed.pop_front();
            }
            s.knock = Some(Knock { at: Instant::now(), victim, inflicted });
        });
    }
}

/// The nearest living enemy within NEAREST_M of the Tarnished: where a hit came from when the game
/// leaves the player's `last_hit_by` empty (it does for enemy melee, 2026-10-04 21:20). 推断: right
/// for the enemy in front, wrong when a farther one (an archer) hit.
fn nearest_enemy(wcm: &WorldChrMan) -> Option<Vec3> {
    const NEAREST_M: f32 = 30.0;
    let me = wcm.main_player.as_ref()?.chr_ins.modules.physics.position;
    let me = Vec3::new(me.0, me.1, me.2);
    wcm.chr_sets
        .iter()
        .flatten()
        .flat_map(|s| s.characters())
        .filter(|c| crate::spike::body::is_enemy_team(c.team_type) && c.modules.data.hp > 0)
        .map(|c| {
            let q = c.modules.physics.position;
            Vec3::new(q.0, q.1, q.2)
        })
        .filter(|p| p.distance(me) < NEAREST_M)
        .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)))
}

/// Kills and damage dealt so far.
pub fn totals() -> (u32, f32) {
    with(|s| (s.kills, s.damage))
}

/// The streak badge's window (the user's 2026-10-10 ask: kills in the last minute).
pub const STREAK_SECONDS: f32 = 60.0;

/// Kills in the last STREAK_SECONDS.
pub fn recent_kills() -> u32 {
    with(|s| {
        while s.recent.front().is_some_and(|t| t.elapsed().as_secs_f32() >= STREAK_SECONDS) {
            s.recent.pop_front();
        }
        s.recent.len() as u32
    })
}

/// Kill feed lines younger than FEED_SECONDS, oldest first: (age s, victim, headshot, weapon slot).
pub fn feed() -> Vec<(f32, Option<String>, bool, u8)> {
    with(|s| {
        s.feed.retain(|k| k.at.elapsed().as_secs_f32() < FEED_SECONDS);
        s.feed.iter().map(|k| (k.at.elapsed().as_secs_f32(), k.victim.clone(), k.head, k.weapon)).collect()
    })
}

/// The last knock-down's message while it shows: (age s, victim, damage inflicted).
pub fn knock() -> Option<(f32, String, f32)> {
    with(|s| {
        s.knock
            .as_ref()
            .map(|k| (k.at.elapsed().as_secs_f32(), k.victim.clone(), k.inflicted))
            .filter(|k| k.0 < KNOCK_SECONDS)
    })
}

/// Hits taken younger than TAKEN_SECONDS: (age s, source, damage).
pub fn taken() -> Vec<(f32, Vec3, f32)> {
    with(|s| {
        s.taken.retain(|t| t.at.elapsed().as_secs_f32() < TAKEN_SECONDS);
        s.taken.iter().map(|t| (t.at.elapsed().as_secs_f32(), t.from, t.damage)).collect()
    })
}

/// Dev channel `stats`.
pub fn status() -> String {
    with(|s| format!("stats: kills {}, damage {:.0}, feed {}, taken {}", s.kills, s.damage, s.feed.len(), s.taken.len()))
}

/// An enemy's name for the HUD's name plate: its NpcName text, else NAMELESS.
pub fn name_of(npc_param_id: i32) -> String {
    npc_name(npc_param_id).unwrap_or_else(|| NAMELESS.into())
}

/// The NpcName text of an NpcParam row, from the export (see the module comment).
fn npc_name(npc_param_id: i32) -> Option<String> {
    static NAMES: OnceLock<HashMap<i32, String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let path = paths::config("npc_names").filter(|p| !p.is_empty()).map(std::path::PathBuf::from).unwrap_or_else(|| paths::mod_dir().join("npc_names.json"));
        let read = || -> Result<HashMap<i32, String>, String> {
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            Ok(v["Entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| Some((e["ID"].as_i64()? as i32, e["Text"].as_str().filter(|t| !t.is_empty())?.to_string())))
                .collect())
        };
        match read() {
            Ok(m) => {
                log(format!("stats: {} enemy names from {}", m.len(), path.display()));
                m
            }
            Err(e) => {
                log(format!("stats: no enemy names ({}: {e}); the kill feed shows none", path.display()));
                HashMap::new()
            }
        }
    });
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    let id = repo.get::<NpcParam>(npc_param_id as u32)?.name_id();
    if let Some(n) = names.get(&id) {
        return Some(n.clone());
    }
    model_name(names, npc_param_id).or_else(|| paramdex_name(npc_param_id))
}

/// The game's name of an enemy's kind: the spirit ashes' NpcName entries by character model (see
/// the module comment), a plain one before a titled one ("“铁棘”艾隆梅尔" is one Bell Bearing
/// Hunter, "铃珠猎人" the kind). Only the model's own: its family's (the model / 10) named other
/// kinds (a Radahn soldier as Godrick's: checked against Paramdex's names).
fn model_name(names: &HashMap<i32, String>, npc_param_id: i32) -> Option<String> {
    static BY_MODEL: OnceLock<HashMap<i32, String>> = OnceLock::new();
    let by_model = BY_MODEL.get_or_init(|| {
        let mut ids: Vec<i32> = names.keys().copied().filter(|id| (900_000_000..910_000_000).contains(id)).collect();
        ids.sort_unstable();
        let mut m: HashMap<i32, String> = HashMap::new();
        for id in ids {
            let name = &names[&id];
            let titled = |n: &str| n.contains('“');
            match m.get(&((id - 900_000_000) / 1000)) {
                Some(have) if !titled(have) || titled(name) => {}
                _ => {
                    m.insert((id - 900_000_000) / 1000, name.clone());
                }
            }
        }
        m
    });
    if npc_param_id < 10_000_000 {
        return None;
    }
    by_model.get(&(npc_param_id / 10_000)).cloned()
}

/// The community name of an NpcParam row (Paramdex `Names/NpcParam.txt`: "<id> <name>"), its
/// trailing "(place)" dropped.
fn paramdex_name(npc_param_id: i32) -> Option<String> {
    static NAMES: OnceLock<HashMap<i32, String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let Some(path) = paths::config("npc_param_names").filter(|p| !p.is_empty()) else { return HashMap::new() };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let m: HashMap<i32, String> = text
                    .lines()
                    .filter_map(|l| {
                        let (id, name) = l.trim().split_once(' ')?;
                        let name = match name.rfind(" (") {
                            Some(i) if name.ends_with(')') => &name[..i],
                            _ => name,
                        };
                        Some((id.parse().ok()?, name.trim().to_string()))
                    })
                    .filter(|(_, n): &(i32, String)| !n.is_empty())
                    .collect();
                log(format!("stats: {} NpcParam row names from {path}", m.len()));
                m
            }
            Err(e) => {
                log(format!("stats: no NpcParam row names ({path}: {e})"));
                HashMap::new()
            }
        }
    });
    names.get(&npc_param_id).cloned()
}
