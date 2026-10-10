//! The Apex-style HUD (M0 HUD, D-015). First laid out after the user's screenshot of retail Apex
//! (2026-10-04, D-016); since 2026-10-05 (D-031) the bottom row follows Season 3's HUD in the
//! user's R5Reloaded video (1600×900, scaled to the 1920×1080 canvas): the unit frame bottom left
//! (squad colour, portrait, name, empty equipment squares, the shield bar's empty segments,
//! health; no upgrade row), the survival (4) and tactical (Q) slots beside it, the ultimate bottom
//! centre, the weapon frame and the ordnance slot (G) bottom right. The compass and the match
//! stats top right are still the screenshot's. Images, colours, names and fonts are the local
//! Apex pack's (T009/T017); positions, sizes and the colours no palette key gives are measured.
//!
//! What the mod has no mechanism for is drawn the way Apex draws its absence: empty equipment,
//! attachment and ordnance slots, an empty second weapon slot. Reserve ammo is endless (D-003),
//! shown as ∞. Since D-032 the shield is real (a purple body shield, 4 segments) and the survival
//! slot holds the endless shield battery.
//!
//! Also here: the crosshair, hit marker, damage numbers, an Apex-style bar for Elden Ring's boss
//! and a health bar over the last enemy hit (the game's own floating bars are hidden, fe.rs).

use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::Mutex;
use std::time::Instant;

use eldenring::cs::WorldChrMan;
use fromsoftware_shared::FromStatic;
use glam::Vec3;
use hudhook::imgui::DrawListMut;

use super::font::{self, Align, Face};
use super::pack::Pack;
use super::{kicked_aim, project, tex, view};
use crate::{fe, spike};

/// Fuse's level 2 and 3 upgrades as in the screenshot (for packs without `legend_upgrades`), the
/// light-ammo badge (R-301's `ammo_pool_type` is "bullet"): images beyond the pack's MVP elements,
/// by Apex path (mod.rs loads these).
const UPGRADE_2: &str = "rui/hud/legend_upgrades/upgrade_fuse_big_bang";
const UPGRADE_3: &str = "rui/hud/legend_upgrades/upgrade_fuse_explosive_recharge";

/// The two upgrade icons the upgrade row shows: the pack's first two `legend_upgrades` (their
/// levels are 待定 in the local data, T017; the row is looks only, D-016), else Fuse's.
pub fn upgrade_icons(pack: &Pack) -> [&str; 2] {
    match pack.upgrades.as_slice() {
        [a, b, ..] => [a.as_str(), b.as_str()],
        _ => [UPGRADE_2, UPGRADE_3],
    }
}
const AMMO_BADGE: &str = "rui/hud/gametype_icons/survival/sur_ammo_bullet";
/// HUD v3, named by R5Reloaded's S3 scripts (tools/apexhud/export_extra.py): the kills counter's
/// skull, the damage counter's icon (white on black: drawn as a mask), the kill feed's headshot.
const SKULL: &str = "rui/rui_screens/skull";
const DEALT: &str = "rui/hud/poi_icons/poi_dealt_damage";
const HEADSHOT: &str = "rui/hud/obituary/obituary_headshot";
/// The weapon frame (plan-gun-motion-hud.md 2.6, D-023): S3's fire mode icon beside its key, and
/// the R-301's empty attachment slots (barrel, magazine, sight, stock), S3's
/// `emptyAttachmentSlotImages`.
const FIRE_MODE: &str = "rui/hud/weapon_toggle/automatic";
const EMPTY_SLOTS: [&str; 4] = [
    "rui/pilot_loadout/mods/empty_barrel_stabilizer",
    "rui/pilot_loadout/mods/empty_mag",
    "rui/pilot_loadout/mods/empty_sight",
    "rui/pilot_loadout/mods/empty_stock_tactical",
];
/// The survival slot (key 4): the shield battery (D-032; mp_ability_consumable `shield_large`
/// `hud_icon`).
const BATTERY_ICON: &str = "rui/hud/loot/loot_stim_shield_large";
/// Shield per segment of the shield bar (S3: 50/75/100 show 2/3/4 segments).
const SHIELD_SEGMENT: f32 = 25.0;
/// U3: the Charge Rifle (slot 2, tools/apexhud/export_extra.py): its `hud_icon` (S3 weapon
/// settings), the sniper ammo's badge (S3 `ammo_pool_type` "sniper"; the local loot table's
/// `hudIcon`), S3's fire mode for a single-shot-only weapon (cl_weapon_status.gnut
/// `#FIRE_MODE_SINGLE_ONLY`), its empty slots: the sight and the sniper stock (its #bases
/// `_base_sniper_optics.txt`, `_base_stocks_sniper.txt`; S3's loot table, which lists a weapon's
/// slots, is not local: magazine / hop-up 待定).
const CHARGE_RIFLE_ICON: &str = "rui/weapon_icons/r5/weapon_charge_rifle";
const SNIPER_BADGE: &str = "rui/hud/gametype_icons/survival/sur_ammo_sniper";
const SINGLE_SHOT: &str = "rui/hud/weapon_toggle/single_shot";
const EMPTY_STOCK_SNIPER: &str = "rui/pilot_loadout/mods/empty_stock_sniper";
/// The Wingman in slot 1 (in the R-301's place; tools/apexhud/export_wingman.py): its `hud_icon`
/// (the retail weapon settings), retail's `ammo_pool_type` "sniper", semi-auto (single shot), its
/// empty slots the magazine and the sight (推断: the pistol's attachments as retail's loot has them).
const WINGMAN_ICON: &str = "rui/weapon_icons/r5/weapon_wingman";
/// The R-99 (the weapon wheel; tools/apexhud/export_wingman.py `--weapon r99`): its `hud_icon`, light
/// ammo, automatic, its empty slots the barrel, the magazine, the sight and the stock.
const R99_ICON: &str = "rui/weapon_icons/r5/weapon_r97";
const R99_SLOTS: [&str; 4] = EMPTY_SLOTS;
/// The VK-47 Flatline (the weapon wheel; tools/apexhud/export_wingman.py `--weapon flatline`): its
/// `hud_icon`, heavy ammo (its colour; the R-301's badge stands in: 推断), automatic, the R-301's slots.
const FLATLINE_ICON: &str = "rui/weapon_icons/r5/weapon_flatline";
/// The kunai (a swing in the holstered mode): its melee skin's `equipImage` (export_wingman.py
/// `--weapon kunai`), the kill feed's icon for it.
/// The Sentinel (the weapon wheel; tools/apexhud/export_wingman.py `--weapon sentinel`): its
/// `hud_icon`, sniper ammo, automatic here (the user's), the sniper's slots.
const SENTINEL_ICON: &str = "rui/weapon_icons/r5/weapon_sentinel";
/// Pathfinder's grapple (Q's other ability: spike/grapple.rs): its item flavour's icon
/// (`settings/itemflav/ability/pathfinder_tac_grapple.rpak`; export_wingman.py `--weapon grapple`).
pub const GRAPPLE_ICON: &str = "rui/hud/tactical_icons/tactical_pathfinder";
const KUNAI_ICON: &str = "rui/menu/buttons/melee_skins/wraith_kunai";
pub const IMAGES: &[(&str, tex::Kind)] = &[
    (BATTERY_ICON, tex::Kind::Color),
    (AMMO_BADGE, tex::Kind::Color),
    (SKULL, tex::Kind::Color),
    (DEALT, tex::Kind::Mask),
    (HEADSHOT, tex::Kind::Color),
    (FIRE_MODE, tex::Kind::Color),
    (EMPTY_SLOTS[0], tex::Kind::Faint),
    (EMPTY_SLOTS[1], tex::Kind::Faint),
    (EMPTY_SLOTS[2], tex::Kind::Faint),
    (EMPTY_SLOTS[3], tex::Kind::Faint),
    (CHARGE_RIFLE_ICON, tex::Kind::Color),
    (SNIPER_BADGE, tex::Kind::Color),
    (SINGLE_SHOT, tex::Kind::Color),
    (EMPTY_STOCK_SNIPER, tex::Kind::Faint),
    (WINGMAN_ICON, tex::Kind::Color),
    (R99_ICON, tex::Kind::Color),
    (FLATLINE_ICON, tex::Kind::Color),
    (SENTINEL_ICON, tex::Kind::Color),
    (KUNAI_ICON, tex::Kind::Color),
    (GRAPPLE_ICON, tex::Kind::Color),
    // the frag grenade's `hud_icon` (U9; tools/apexhud/export_extra.py)
    (super::grenade::ICON, tex::Kind::Color),
];

/// How the weapon frame shows a slot's weapon (S3 cl_weapon_status.gnut: `weaponNameTab0/1` the
/// `shortprintname`, `ammoColorTab0/1` its ammo's colour, the selected one's icon, ammo badge,
/// fire mode and attachment slots).
struct Look {
    name_key: &'static str,
    fallback: &'static str,
    /// the image: an element of the pack (the R-301's `weapon_slot`) or an Apex path
    icon: &'static str,
    ammo_key: &'static str,
    ammo_fallback: [u8; 3],
    badge: &'static str,
    mode: &'static str,
    slots: &'static [&'static str],
}

const WINGMAN_SLOTS: [&str; 2] = [EMPTY_SLOTS[1], EMPTY_SLOTS[2]];
const CHARGE_RIFLE_SLOTS: [&str; 2] = [EMPTY_SLOTS[2], EMPTY_STOCK_SNIPER];

fn look(slot: u8) -> Look {
    if slot == 1 {
        Look {
            name_key: "#WPN_CHARGE_RIFLE_SHORT",
            fallback: "Charge Rifle",
            icon: CHARGE_RIFLE_ICON,
            ammo_key: "AMMO_SNIPER_COLOR",
            ammo_fallback: [110, 95, 206],
            badge: SNIPER_BADGE,
            mode: SINGLE_SHOT,
            slots: &CHARGE_RIFLE_SLOTS,
        }
    } else if slot == 0 && crate::spike::weapons::primary_gun() == crate::spike::weapons::Gun::R301 {
        Look {
            name_key: "#WPN_RSPN101_SHORT",
            fallback: "R-301",
            icon: "weapon_slot",
            ammo_key: "AMMO_SMALL_COLOR",
            ammo_fallback: [180, 123, 68],
            badge: AMMO_BADGE,
            mode: FIRE_MODE,
            slots: &EMPTY_SLOTS,
        }
    } else if slot == 0 && crate::spike::weapons::primary_gun() == crate::spike::weapons::Gun::R99 {
        Look {
            name_key: "#WPN_R97_SHORT",
            fallback: "R-99",
            icon: R99_ICON,
            ammo_key: "AMMO_SMALL_COLOR",
            ammo_fallback: [180, 123, 68],
            badge: AMMO_BADGE,
            mode: FIRE_MODE,
            slots: &R99_SLOTS,
        }
    } else if slot == 0 && crate::spike::weapons::primary_gun() == crate::spike::weapons::Gun::Flatline {
        Look {
            name_key: "#WPN_VINSON_SHORT",
            fallback: "Flatline",
            icon: FLATLINE_ICON,
            ammo_key: "AMMO_HEAVY_COLOR",
            ammo_fallback: [86, 160, 128],
            badge: AMMO_BADGE,
            mode: FIRE_MODE,
            slots: &EMPTY_SLOTS,
        }
    } else if slot == 0 && crate::spike::weapons::primary_gun() == crate::spike::weapons::Gun::Sentinel {
        Look {
            name_key: "#WPN_SENTINEL_SHORT",
            fallback: "Sentinel",
            icon: SENTINEL_ICON,
            ammo_key: "AMMO_SNIPER_COLOR",
            ammo_fallback: [110, 95, 206],
            badge: SNIPER_BADGE,
            mode: FIRE_MODE,
            slots: &CHARGE_RIFLE_SLOTS,
        }
    } else {
        Look {
            name_key: "#WPN_WINGMAN_SHORT",
            fallback: "Wingman",
            icon: WINGMAN_ICON,
            ammo_key: "AMMO_SNIPER_COLOR",
            ammo_fallback: [110, 95, 206],
            badge: SNIPER_BADGE,
            mode: SINGLE_SHOT,
            slots: &WINGMAN_SLOTS,
        }
    }
}

/// A slot's weapon's short name in the HUD language (the pack's R-301 name as the R-301's last
/// resort).
fn weapon_name<'a>(pack: &'a Pack, l: &Look) -> &'a str {
    match pack.strings.get(l.name_key).filter(|s| !s.is_empty()) {
        Some(s) => s,
        None if l.icon == "weapon_slot" => &pack.weapon_name,
        None => l.fallback,
    }
}

fn ammo_colour(pack: &Pack, l: &Look) -> [f32; 4] {
    color(pack, l.ammo_key, rgb(l.ammo_fallback[0], l.ammo_fallback[1], l.ammo_fallback[2], 1.0))
}

/// Seconds: hit marker, health trail, boss trail.
const MARKER_SECONDS: f32 = 0.15;
const TRAIL_SECONDS: f32 = 0.35;
const BOSS_TRAIL_SECONDS: f32 = 0.4;
/// Apex's damage numbers as the user's R5R video shows them (2026-10-05 16:53, a firing-range
/// dummie; S3 draws them in native code, so the timings are measured or 推断): over the enemy's
/// name plate a running total of the hits (the stack, `STACK_*`), white with a dark edge, its
/// newest hit first shown alone for 3 frames (a headshot in HUD_DAMAGE_HEADSHOT), and each hit's
/// own number, small, rising over it (`FLOAT_*`). Hits less than STACK_GAP apart add up; the stack
/// holds STACK_HOLD after the last hit (推断) and rises and fades over STACK_OUT, at once when the
/// enemy dies (the video: 72 px over 0.5 s after the knock-down). Sizes are the video's 900p ones
/// scaled to 1080p; the face is the pack's 11 (the video's octagonal 0 and footed 1).
const STACK_GAP: f32 = 2.0;
const STACK_HOLD: f32 = 2.0;
const STACK_OUT: f32 = 0.5;
const STACK_RISE: f32 = 72.0;
const STACK_HEIGHT: f32 = 27.0;
const STACK_HIT_SHOW: f32 = 0.05;
const FLOAT_SECONDS: f32 = 0.6;
const FLOAT_RISE: f32 = 22.0;
const FLOAT_HEIGHT: f32 = 15.0;
/// The name plate over the last enemy hit: its name, a white bar of its health (Apex's for a
/// dummie; the bleed red before), shown with the stack.
const PLATE_NAME_HEIGHT: f32 = 13.0;
const PLATE_BAR: [f32; 2] = [96.0, 6.0];

/// How far Apex's slanted edges lean: x per px of height (measured off the screenshot).
const SLANT: f32 = 0.45;

/// Where a part of the HUD is held when the screen is not 16:9.
#[derive(Clone, Copy)]
enum Anchor {
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Draws in 1920×1080 canvas units, scaled uniformly, kept at the anchor's screen edges.
struct Pen<'a, 'ui> {
    dl: &'a DrawListMut<'ui>,
    k: f32,
    ox: f32,
    oy: f32,
}

impl<'a, 'ui> Pen<'a, 'ui> {
    fn new(dl: &'a DrawListMut<'ui>, size: [f32; 2], anchor: Anchor) -> Self {
        let k = (size[0] / 1920.0).min(size[1] / 1080.0);
        let (cx, bottom) = ((size[0] - 1920.0 * k) * 0.5, size[1] - 1080.0 * k);
        let (ox, oy) = match anchor {
            Anchor::TopCenter => (cx, 0.0),
            Anchor::TopRight => (size[0] - 1920.0 * k, 0.0),
            Anchor::BottomLeft => (0.0, bottom),
            Anchor::BottomCenter => (cx, bottom),
            Anchor::BottomRight => (size[0] - 1920.0 * k, bottom),
        };
        Pen { dl, k, ox, oy }
    }

    fn p(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [self.ox + x * self.k, self.oy + y * self.k]
    }

    /// A convex polygon, filled.
    fn poly(&self, pts: &[[f32; 2]], col: [f32; 4]) {
        self.dl.add_polyline(pts.iter().map(|&q| self.p(q)).collect::<Vec<_>>(), col).filled(true).build();
    }

    fn outline(&self, pts: &[[f32; 2]], col: [f32; 4], t: f32) {
        let mut v: Vec<[f32; 2]> = pts.iter().map(|&q| self.p(q)).collect();
        v.push(v[0]);
        self.dl.add_polyline(v, col).thickness(t * self.k).build();
    }

    fn line(&self, a: [f32; 2], b: [f32; 2], col: [f32; 4], t: f32) {
        self.dl.add_line(self.p(a), self.p(b), col).thickness(t * self.k).build();
    }

    fn rect(&self, [x, y, w, h]: [f32; 4], col: [f32; 4]) {
        self.dl.add_rect(self.p([x, y]), self.p([x + w, y + h]), col).filled(true).build();
    }

    /// A rounded rectangle, filled, with a rim.
    fn rounded(&self, [x, y, w, h]: [f32; 4], r: f32, fill: [f32; 4], rim: [f32; 4]) {
        let (a, b) = (self.p([x, y]), self.p([x + w, y + h]));
        self.dl.add_rect(a, b, fill).filled(true).rounding(r * self.k).build();
        self.dl.add_rect(a, b, rim).rounding(r * self.k).thickness(1.5 * self.k).build();
    }

    /// A horizontal gradient from `left` to `right`.
    fn hgrad(&self, [x, y, w, h]: [f32; 4], left: [f32; 4], right: [f32; 4]) {
        self.dl.add_rect_filled_multicolor(self.p([x, y]), self.p([x + w, y + h]), left, right, right, left);
    }

    /// A texture fitted inside the rect, centred.
    fn image(&self, name: &str, rect: [f32; 4], a: f32) {
        self.tinted(name, rect, [1.0, 1.0, 1.0, a]);
    }

    /// A texture fitted inside the rect, centred, multiplied by `col`.
    fn tinted(&self, name: &str, [x, y, w, h]: [f32; 4], col: [f32; 4]) {
        let Some((id, [iw, ih])) = tex::get(name) else { return };
        let s = (w / iw).min(h / ih);
        let (dw, dh) = (iw * s, ih * s);
        let (x0, y0) = (x + (w - dw) * 0.5, y + (h - dh) * 0.5);
        self.dl.add_image(id, self.p([x0, y0]), self.p([x0 + dw, y0 + dh])).col(col).build();
    }

    /// Part `uv` of a texture placed over `rect`, showing only inside the quad `q` (corners
    /// inside `rect`): the image is cut along `q`'s edges, not stretched to them.
    fn image_cut(&self, name: &str, rect: [f32; 4], uv: [f32; 4], q: [[f32; 2]; 4], a: f32) {
        let Some((id, _)) = tex::get(name) else { return };
        let [x, y, w, h] = rect;
        let at = |[px, py]: [f32; 2]| [uv[0] + (px - x) / w * (uv[2] - uv[0]), uv[1] + (py - y) / h * (uv[3] - uv[1])];
        self.dl
            .add_image_quad(id, self.p(q[0]), self.p(q[1]), self.p(q[2]), self.p(q[3]))
            .uv(at(q[0]), at(q[1]), at(q[2]), at(q[3]))
            .col([1.0, 1.0, 1.0, a])
            .build();
    }

    /// Text with the top of a '0' at `y` and a '0' `h` tall; returns the width (canvas units).
    #[allow(clippy::too_many_arguments)]
    fn text(&self, face: Face, s: &str, x: f32, y: f32, h: f32, col: [f32; 4], align: Align) -> f32 {
        font::draw(self.dl, face, s, self.p([x, y]), h * self.k, col, align) / self.k
    }
}

/// A slanted cell: top edge `w` wide at (x, y), `h` tall, leaning right by `slant` per px going
/// down (negative leans left, as on the right of the screen).
fn cell(x: f32, y: f32, w: f32, h: f32, slant: f32) -> [[f32; 2]; 4] {
    let d = h * slant;
    [[x, y], [x + w, y], [x + w + d, y + h], [x + d, y + h]]
}

/// A `w`×`h` rect centred in a cell (on the mean of its corners: the middle of the cell at half
/// its height), as S3 centres a slot's icon (the video's medkit, the user's 10-06 frame).
fn centred(q: &[[f32; 2]; 4], w: f32, h: f32) -> [f32; 4] {
    let (x, y) = (q.iter().map(|c| c[0]).sum::<f32>() / 4.0, q.iter().map(|c| c[1]).sum::<f32>() / 4.0);
    [x - w * 0.5, y - h * 0.5, w, h]
}

/// A bar that shows what was just lost for a moment (Apex's damage trail): the value it trails
/// from, and since when.
struct Trail {
    shown: f32,
    from: f32,
    since: Instant,
}

impl Trail {
    /// The trailing value for `now_value`, `seconds` to catch up.
    fn update(slot: &Mutex<Option<Trail>>, value: f32, seconds: f32) -> f32 {
        let mut t = slot.lock().unwrap_or_else(|e| e.into_inner());
        let tr = t.get_or_insert(Trail { shown: value, from: value, since: Instant::now() });
        if value > tr.shown {
            *tr = Trail { shown: value, from: value, since: Instant::now() };
        } else if value < tr.shown && (tr.from - tr.shown).abs() < f32::EPSILON {
            tr.since = Instant::now();
        }
        let f = (tr.since.elapsed().as_secs_f32() / seconds).min(1.0);
        tr.shown = tr.from + (value - tr.from) * f;
        if f >= 1.0 {
            tr.from = value;
            tr.shown = value;
        }
        tr.shown.max(value)
    }
}

static BOSS_TRAIL: Mutex<Option<Trail>> = Mutex::new(None);

/// S3's unit frame after a hit, as the user's R5R video shows it (the stim's cost, 24.6 s): the
/// health just lost stays bright red on the bar for LOST_HOLD, then fades over LOST_FADE (more
/// damage meanwhile adds to it); the frame's rim and chevron turn red and the portrait flushes
/// red, pulsing every DAMAGE_PULSE, for as long (measured: red until 26.2 s, gone by 26.8 s).
const LOST_HOLD: f32 = 1.6;
const LOST_FADE: f32 = 0.4;
const DAMAGE_PULSE: f32 = 0.4;

/// The health lost lately: the value it was at, the last value seen, when.
struct Lost {
    top: f32,
    last: f32,
    at: Instant,
}
static HEALTH_LOST: Mutex<Option<Lost>> = Mutex::new(None);
/// The shield lost lately (its flash on the shield bar, D-032).
static SHIELD_LOST: Mutex<Option<Lost>> = Mutex::new(None);

/// The top of the red part and its opacity, if it shows.
fn lost(hp: f32) -> Option<(f32, f32)> {
    lost_in(&HEALTH_LOST, hp)
}

/// `lost` for any bar: the top of what was just lost and its opacity, if it shows.
fn lost_in(slot: &Mutex<Option<Lost>>, hp: f32) -> Option<(f32, f32)> {
    let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
    let l = g.get_or_insert(Lost { top: hp, last: hp, at: Instant::now() });
    let age = l.at.elapsed().as_secs_f32();
    let showing = age < LOST_HOLD + LOST_FADE && l.top > l.last;
    if hp < l.last {
        if !showing {
            l.top = l.last;
        }
        l.at = Instant::now();
    } else if hp > l.top {
        l.top = hp;
    }
    l.last = hp;
    let age = l.at.elapsed().as_secs_f32();
    let a = 1.0 - ((age - LOST_HOLD) / LOST_FADE).clamp(0.0, 1.0);
    (a > 0.0 && l.top > hp).then_some((l.top, a))
}

/// How red the frame is after a hit that reached health (0 none .. 1), pulsing (a hit the shield
/// takes whole flashes the shield bar instead).
fn hurt() -> f32 {
    let Some(t) = spike::lethal::since_health_damage() else { return 0.0 };
    let k = 1.0 - ((t - LOST_HOLD) / LOST_FADE).clamp(0.0, 1.0);
    if k <= 0.0 {
        return 0.0;
    }
    k * (0.6 + 0.4 * (0.5 + 0.5 * (std::f32::consts::TAU * t / DAMAGE_PULSE).cos()))
}

fn color(pack: &Pack, key: &str, fallback: [f32; 4]) -> [f32; 4] {
    pack.colors.get(key).copied().unwrap_or(fallback)
}

/// `a` turned towards `b` by `f` (0..1), alpha included.
fn mix_colour(a: [f32; 4], b: [f32; 4], f: f32) -> [f32; 4] {
    let f = f.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, a[3] + (b[3] - a[3]) * f]
}

fn alpha(c: [f32; 4], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * a]
}

/// RGB scaled by `f` (a darker shade), alpha `a`.
fn shade(c: [f32; 4], f: f32, a: f32) -> [f32; 4] {
    [c[0] * f, c[1] * f, c[2] * f, a]
}

/// 8-bit RGB (sampled off the screenshot where the palette has no key) and alpha.
fn rgb(r: u8, g: u8, b: u8, a: f32) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

/// How dark the crosshair's and hit marker's outlines are (the video's 1 px edges darken the sand
/// behind them to about a third).
const OUTLINE_ALPHA: f32 = 0.7;

/// A bar from `a` to `b`, `t` wide, with a dark outline `o` wide all round it (its ends too): the
/// crosshair's ticks and the hit marker's legs in the user's R5R video.
fn outlined_bar(dl: &DrawListMut, a: [f32; 2], b: [f32; 2], col: [f32; 4], t: f32, o: f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / len * o, dy / len * o);
    dl.add_line([a[0] - ux, a[1] - uy], [b[0] + ux, b[1] + uy], [0.0, 0.0, 0.0, OUTLINE_ALPHA * col[3]]).thickness(t + 2.0 * o).build();
    dl.add_line(a, b, col).thickness(t).build();
}

/// The HUD's player name: ini `player_name` when set, else the character name from the save (not
/// Fuse's).
fn player_name() -> Option<String> {
    static NAME: Mutex<Option<String>> = Mutex::new(None);
    if let Some(n) = NAME.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(n);
    }
    if let Some(n) = crate::paths::config("player_name").filter(|n| !n.is_empty()) {
        *NAME.lock().unwrap_or_else(|e| e.into_inner()) = Some(n.clone());
        return Some(n);
    }
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    let p = wcm.main_player.as_ref()?;
    let data = unsafe { p.player_game_data.as_ref() };
    let raw = &data.character_name;
    let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    let name = String::from_utf16_lossy(&raw[..len]);
    if name.is_empty() {
        return None;
    }
    *NAME.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.clone());
    Some(name)
}

/// Compass heading of the camera, degrees clockwise from north. Elden Ring's world: +X east, +Z
/// north (the m60 tile grid; confirmed against the in-game map, journal 10-04).
pub fn heading() -> Option<f32> {
    let (_, fwd, _, _) = view()?;
    (fwd.x.abs() + fwd.z.abs() > 1e-4).then(|| fwd.x.atan2(fwd.z).to_degrees().rem_euclid(360.0))
}

/// Shared colours.
struct Colors {
    white: [f32; 4],
    /// Panels' and slots' dark fill, their light rim (sampled).
    panel: [f32; 4],
    rim: [f32; 4],
    bleed: [f32; 4],
    head: [f32; 4],
}

pub fn draw(dl: &DrawListMut, pack: &Pack, size: [f32; 2], fov: f32) {
    let c = Colors {
        white: color(pack, "DEFAULT", [1.0; 4]),
        panel: rgb(46, 60, 72, 0.82),
        rim: alpha(color(pack, "HUD_LOOT_TIER1", [0.72, 0.72, 0.72, 1.0]), 0.85),
        bleed: color(pack, "HUD_DAMAGE_TEXT_BLEED", [0.82, 0.24, 0.25, 1.0]),
        head: color(pack, "HUD_DAMAGE_HEADSHOT", [1.0, 0.74, 0.0, 1.0]),
    };
    // the weapon in hand (U3: the R-301 or the Charge Rifle, spike/weapons.rs)
    let gun = spike::weapons::hud();

    if let Some(h) = heading() {
        compass(&Pen::new(dl, size, Anchor::TopCenter), h, &c);
    }
    if let Some(hp) = spike::lethal::fuse_hp() {
        player_frame(&Pen::new(dl, size, Anchor::BottomLeft), pack, hp, &c);
    }
    if spike::lethal::fuse_hp().is_some() {
        ultimate_slot(&Pen::new(dl, size, Anchor::BottomCenter), &c);
        battery_use(&Pen::new(dl, size, Anchor::BottomCenter), pack, &c);
    }
    if let Some(g) = gun.as_ref() {
        weapon_frame(&Pen::new(dl, size, Anchor::BottomRight), pack, g, &c);
    }
    boss_bar(&Pen::new(dl, size, Anchor::BottomCenter), pack, &c);
    let top_right = Pen::new(dl, size, Anchor::TopRight);
    streak_badge(&top_right, &c);
    match_stats(&top_right, &c);
    kill_feed(&top_right, pack);
    damage_indicators(dl, pack, size);
    knock_message(dl, pack, size, &c);
    if let Some(w) = spike::weapons::wheel() {
        weapon_wheel(&Pen::new(dl, size, Anchor::TopCenter), pack, w.hovered, &c);
    }
    let Some(g) = gun else { return };
    aim(dl, pack, size, fov, &g, &c);
}

/// The weapon wheel (Tab held: spike/weapons.rs): a sector each round the screen's centre, clockwise
/// from the top the R-301, the R-99, the Wingman, the Flatline and the Charge Rifle, each with its
/// icon and name; the one pointed at bright, the one in hand marked.
/// Then Q's two abilities (the stim, Pathfinder's grapple).
fn weapon_wheel(p: &Pen, pack: &Pack, hovered: Option<usize>, c: &Colors) {
    use spike::weapons::{Gun, Slot};
    let (cx, cy) = (960.0, 540.0);
    let (r0, r1) = (110.0, 260.0);
    let held = match spike::weapons::active() {
        Slot::ChargeRifle => 5,
        // the kunai is not on the wheel: slot 1's gun marked
        Slot::R301 | Slot::Melee => match spike::weapons::primary_gun() {
            Gun::R301 => 0,
            Gun::R99 => 1,
            Gun::Wingman => 2,
            Gun::Flatline => 3,
            Gun::Sentinel => 4,
        },
    };
    let icons = ["weapon_slot", R99_ICON, WINGMAN_ICON, FLATLINE_ICON, SENTINEL_ICON, CHARGE_RIFLE_ICON, "tactical", GRAPPLE_ICON];
    // Q's ability in use marked as well
    let q_held = match spike::grapple::q_ability() {
        spike::grapple::QAbility::Stim => 6,
        spike::grapple::QAbility::Grapple => 7,
    };
    let n = spike::weapons::WHEEL.len();
    let sector = std::f32::consts::TAU / n as f32;
    for (k, (_, name)) in spike::weapons::WHEEL.iter().enumerate() {
        // the sector's middle, clockwise from the top
        let mid = k as f32 * sector;
        let at = |a: f32, r: f32| [cx + r * a.sin(), cy - r * a.cos()];
        let steps = 12;
        let span = sector - 0.06;
        let a0 = mid - span * 0.5;
        let on = hovered == Some(k);
        let fill = if on { alpha(c.white, 0.35) } else { alpha(c.panel, 0.75) };
        for i in 0..steps {
            let (u, v) = (a0 + span * i as f32 / steps as f32, a0 + span * (i + 1) as f32 / steps as f32);
            p.poly(&[at(u, r0), at(u, r1), at(v, r1), at(v, r0)], fill);
        }
        let rim = if on { c.white } else { c.rim };
        let outer: Vec<[f32; 2]> = (0..=steps).map(|i| at(a0 + span * i as f32 / steps as f32, r1)).collect();
        for w in outer.windows(2) {
            p.line(w[0], w[1], rim, if on { 3.0 } else { 1.5 });
        }
        let centre = at(mid, (r0 + r1) * 0.5);
        let ability = matches!(spike::weapons::WHEEL[k].0, spike::weapons::Pick::Q(_));
        let rect = if ability { [centre[0] - 19.0, centre[1] - 44.0, 38.0, 49.0] } else { [centre[0] - 55.0, centre[1] - 34.0, 110.0, 44.0] };
        p.image(icons[k], rect, if on { 1.0 } else { 0.8 });
        let label = if k == 0 { pack.weapon_name.as_str() } else { name };
        p.text(Face::Bold, label, centre[0], centre[1] + 22.0, 13.0, if on { c.white } else { alpha(c.white, 0.75) }, Align::Center);
        if k == held || k == q_held {
            p.text(Face::Body, "IN HAND", centre[0], centre[1] + 46.0, 10.0, alpha(c.white, 0.6), Align::Center);
        }
    }
}

/// Inside a convex polygon (either winding)?
fn inside(q: [f32; 2], poly: &[[f32; 2]]) -> bool {
    let n = poly.len();
    let side = |i: usize| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0])
    };
    let first = side(0);
    (1..n).all(|i| side(i) * first >= 0.0)
}

/// The fine dot screen Apex's stat cells are filled with (measured off the screenshot: a dot
/// every 4 px, rows offset).
fn dots(p: &Pen, poly: &[[f32; 2]], col: [f32; 4]) {
    let (x0, x1) = poly.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[0]), hi.max(q[0])));
    let (y0, y1) = poly.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[1]), hi.max(q[1])));
    let mut y = y0 + 2.0;
    let mut row = 0;
    while y < y1 - 1.0 {
        let mut x = x0 + 2.0 + if row % 2 == 1 { 2.0 } else { 0.0 };
        while x < x1 - 1.0 {
            if inside([x, y], poly) {
                p.rect([x - 0.6, y - 0.6, 1.2, 1.2], col);
            }
            x += 4.0;
        }
        y += 4.0;
        row += 1;
    }
}

/// The streak badges, Apex's rank emblems in the user's own pictures (`rank/<name>.png` in or beside
/// `hud_dir`).
pub const RANK_BADGES: [&str; 3] = ["D1", "M1", "P1"];

/// Top right, where Apex shows the ranked badge (the user's 2026-10-10 ask, sized and placed after
/// their Apex screenshot): a kill-streak counter, D1 for 0-3 kills in the last minute, M1 for 4-10,
/// P1 above 10, the count under it.
fn streak_badge(p: &Pen, c: &Colors) {
    let n = spike::stats::recent_kills();
    let badge = RANK_BADGES[match n {
        0..=3 => 0,
        4..=10 => 1,
        _ => 2,
    }];
    const RIGHT: f32 = 1906.0;
    const TOP: f32 = 6.0;
    const SIZE: f32 = 104.0;
    p.image(badge, [RIGHT - SIZE, TOP, SIZE, SIZE], 1.0);
    p.text(Face::Bold, &format!("{n} KILLS"), RIGHT - SIZE * 0.5, TOP + SIZE + 3.0, 13.0, c.white, Align::Center);
}

/// Top right, as the screenshot's match stats: kills (skull) and damage dealt, each in a slanted
/// dotted cell. The screenshot's assists and knockdowns cells are left out: Elden Ring has
/// neither (D-016: nothing made up). Positions measured off the screenshot (the kills cell moved
/// next to the damage cell, where the two left out were).
fn match_stats(p: &Pen, c: &Colors) {
    let (kills, damage) = spike::stats::totals();
    // under the streak badge's middle, left of it (moved 2026-10-10 from 82, x 13 to the right)
    const TOP: f32 = 96.0;
    const H: f32 = 22.0;
    const LEAN: f32 = -0.78;
    let fill = rgb(28, 44, 58, 0.62);
    for (x, w, icon, value, mask) in [(1619.0, 67.0, SKULL, kills.to_string(), false), (1698.0, 88.0, DEALT, format!("{damage:.0}"), true)] {
        let q = cell(x, TOP, w, H, LEAN);
        p.poly(&q, fill);
        dots(p, &q, rgb(150, 170, 185, 0.16));
        p.outline(&q, alpha(c.rim, 0.55), 1.2);
        let ix = x - 6.0;
        if mask {
            p.tinted(icon, [ix, TOP + 1.0, 22.0, 20.0], c.white);
        } else {
            p.image(icon, [ix, TOP + 2.0, 20.0, 18.0], 1.0);
        }
        p.text(Face::Bold, &value, ix + 25.0, TOP + 4.0, 14.5, c.white, Align::Left);
    }
}

/// Top right, under the stats: Apex's obituary for Fuse's kills, newest at the bottom, fading out:
/// player name, the R-301, a headshot mark, the enemy's name (Elden Ring's text). Box behind at
/// alpha 0.5 as Apex does for lines with the local player (R5Reloaded cl_obituary.gnut).
fn kill_feed(p: &Pen, pack: &Pack) {
    let lines = spike::stats::feed();
    if lines.is_empty() {
        return;
    }
    let me = player_name().unwrap_or_default();
    let squad = color(pack, "MEMBER_COLOR1", rgb(125, 175, 10, 1.0));
    let enemy = color(pack, "ENEMY", rgb(255, 45, 13, 1.0));
    const RIGHT: f32 = 1796.0;
    const H: f32 = 28.0;
    const TEXT: f32 = 14.5;
    const GAP: f32 = 8.0;
    const GUN: f32 = 54.0;
    const HEAD: f32 = 18.0;
    for (i, (age, victim, head, weapon)) in lines.into_iter().enumerate() {
        let a = (1.0 - (age - (spike::stats::FEED_SECONDS - 0.5)).max(0.0) / 0.5).clamp(0.0, 1.0);
        let y = 206.0 + i as f32 * (H + 4.0);
        let victim_w = victim.as_deref().map_or(0.0, |v| font::width(Face::Body, v, TEXT));
        let me_w = font::width(Face::Body, &me, TEXT);
        let total = 2.0 * GAP + me_w + GAP + GUN + if head { GAP + HEAD } else { 0.0 } + if victim.is_some() { GAP + victim_w } else { 0.0 };
        p.rect([RIGHT - total, y, total, H], [0.0, 0.0, 0.0, 0.5 * a]);
        // right to left: victim, headshot, gun, player
        let mut x = RIGHT - GAP;
        if let Some(v) = &victim {
            p.text(Face::Body, v, x, y + (H - TEXT) * 0.5, TEXT, alpha(enemy, a), Align::Right);
            x -= victim_w + GAP;
        }
        if head {
            p.image(HEADSHOT, [x - HEAD, y + 4.0, HEAD, H - 8.0], a);
            x -= HEAD + GAP;
        }
        // the weapon of the killing hit (U3; the frag grenade's ordnance icon, square, U9)
        if weapon == 2 {
            let s = H - 6.0;
            p.image(super::grenade::ICON, [x - (GUN + s) * 0.5, y + 3.0, s, s], a);
        } else if weapon == 3 {
            // the kunai's picture (98 x 112) on its side would be too small: upright, as tall as the line
            let h = H - 4.0;
            let w = h * 98.0 / 112.0;
            p.image(KUNAI_ICON, [x - (GUN + w) * 0.5, y + 2.0, w, h], a);
        } else {
            p.image(look(weapon).icon, [x - GUN, y + 3.0, GUN, H - 6.0], a);
        }
        x -= GUN + GAP;
        p.text(Face::Body, &me, x, y + (H - TEXT) * 0.5, TEXT, alpha(squad, a), Align::Right);
    }
}

/// Apex's 2D damage indicator: a red arc round the crosshair towards where each hit came from,
/// following the view, 4 s (R5Reloaded cl_damage_indicator.gnut: direction on the ground plane,
/// DAMAGE_INDICATOR_DURATION 4.0, at most 8). The arc's radius, width and fade are not in the data
/// (the RUI draws it in its shader): 推断, sized after Apex footage.
fn damage_indicators(dl: &DrawListMut, pack: &Pack, size: [f32; 2]) {
    let taken = spike::stats::taken();
    if taken.is_empty() {
        return;
    }
    let Some((eye, fwd, _, _)) = view() else { return };
    let red = color(pack, "ENEMY", rgb(255, 45, 13, 1.0));
    let k = (size[0] / 1920.0).min(size[1] / 1080.0);
    let (cx, cy) = (size[0] * 0.5, size[1] * 0.5);
    let f = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
    // the view's right on the ground: forward turned a quarter clockwise seen from above
    // (Elden Ring: +X east, +Z north, +Y up)
    let r = Vec3::new(f.z, 0.0, -f.x);
    for (age, from, _) in taken {
        let d = Vec3::new(from.x - eye.x, 0.0, from.z - eye.z).normalize_or_zero();
        if d == Vec3::ZERO || f == Vec3::ZERO {
            continue;
        }
        // screen angle: 0 straight up (ahead), clockwise
        let ang = d.dot(r).atan2(d.dot(f));
        let a = if age < 1.0 { 1.0 } else { 1.0 - (age - 1.0) / (spike::stats::TAKEN_SECONDS - 1.0) };
        let (radius, half, thick) = (150.0 * k, 22f32.to_radians(), 9.0 * k);
        let steps = 16;
        for s in 0..steps {
            let t0 = -half + 2.0 * half * s as f32 / steps as f32;
            let t1 = -half + 2.0 * half * (s + 1) as f32 / steps as f32;
            // brightest in the middle
            let w = 1.0 - ((t0 + t1) * 0.5 / half).abs();
            let at = |t: f32, rr: f32| [cx + (ang + t).sin() * rr, cy - (ang + t).cos() * rr];
            let quad = vec![at(t0, radius), at(t1, radius), at(t1, radius + thick), at(t0, radius + thick)];
            dl.add_polyline(quad, alpha(red, a * (0.35 + 0.65 * w))).filled(true).build();
        }
    }
}

/// Top centre: the heading strip, every 15° (N E S W large, the rest small under a tick), and the
/// heading in its box under the pointer.
fn compass(p: &Pen, heading: f32, c: &Colors) {
    const TOP: f32 = 39.0;
    const BOTTOM: f32 = 74.0;
    const HALF: f32 = 456.0;
    const FADE: f32 = 130.0;
    const PX_PER_DEG: f32 = 300.0 / 90.0;
    let band = rgb(6, 14, 22, 0.26);
    let clear = alpha(band, 0.0);
    p.hgrad([960.0 - HALF, TOP, FADE, BOTTOM - TOP], clear, band);
    p.rect([960.0 - HALF + FADE, TOP, 2.0 * (HALF - FADE), BOTTOM - TOP], band);
    p.hgrad([960.0 + HALF - FADE, TOP, FADE, BOTTOM - TOP], band, clear);
    let span = HALF / PX_PER_DEG;
    let (first, last) = (((heading - span) / 15.0).ceil() as i32, ((heading + span) / 15.0).floor() as i32);
    for i in first..=last {
        let x = 960.0 + (i as f32 * 15.0 - heading) * PX_PER_DEG;
        let a = ((HALF - (x - 960.0).abs()) / FADE).clamp(0.0, 1.0);
        if a <= 0.0 {
            continue;
        }
        let deg = (i * 15).rem_euclid(360);
        let label = match deg {
            0 => "N".to_string(),
            90 => "E".into(),
            180 => "S".into(),
            270 => "W".into(),
            45 => "NE".into(),
            135 => "SE".into(),
            225 => "SW".into(),
            315 => "NW".into(),
            d => d.to_string(),
        };
        if deg % 90 == 0 {
            p.text(Face::Bold, &label, x, 46.0, 23.0, alpha(c.white, a), Align::Center);
        } else {
            p.line([x, 45.0], [x, 52.0], alpha(c.white, 0.75 * a), 1.5);
            p.text(Face::Body, &label, x, 58.5, 11.5, alpha(c.white, 0.9 * a), Align::Center);
        }
    }
    p.poly(&[[954.0, BOTTOM], [960.0, BOTTOM - 6.0], [966.0, BOTTOM]], alpha(c.white, 0.85));
    let (top, bottom) = (78.0, 110.0);
    let frame = [[919.0, top], [1001.0, top], [983.0, bottom], [937.0, bottom]];
    p.poly(&frame, rgb(6, 14, 22, 0.5));
    p.line(frame[0], frame[3], alpha(c.white, 0.9), 2.5);
    p.line(frame[1], frame[2], alpha(c.white, 0.9), 2.5);
    p.text(Face::Body, &format!("{:.0}", heading.round().rem_euclid(360.0)), 960.0, 86.0, 19.0, c.white, Align::Center);
}

/// Bottom left, as S3's unit frame in the user's R5R video (2026-10-05 16:53, measured at 1080p;
/// it replaces the 10-04 retail screenshot's layout, D-031): a dot-screened panel with a slanted
/// right edge, the squad colour as a chevron down its left edge and a tick at its lower right
/// (the video's player is MEMBER_COLOR1), the portrait, the name, two equipment squares (armour,
/// helmet: empty; their S3 icons are 待定), the shield bar's two empty segments (no armour,
/// D-016) and the health bar with a pointed end. No upgrade row (S3 has none). Then the survival
/// slot (key 4, empty: no healing items) and the tactical (key Q).
fn player_frame(p: &Pen, pack: &Pack, hp: f32, c: &Colors) {
    let red = rgb(232, 24, 18, 1.0);
    let hurt = hurt();
    let squad = mix_colour(color(pack, "MEMBER_COLOR1", rgb(125, 175, 10, 1.0)), red, hurt);
    let frame = [[66.0, 955.0], [401.0, 955.0], [435.0, 1027.0], [66.0, 1027.0]];
    p.poly(&frame, rgb(34, 33, 34, 0.66));
    dots(p, &frame, rgb(150, 150, 150, 0.12));
    let rim = mix_colour(alpha(c.rim, 0.4), red, hurt);
    p.line(frame[0], frame[1], rim, 1.2 + 1.3 * hurt);
    p.line(frame[1], frame[2], rim, 1.2 + 1.3 * hurt);
    p.line(frame[2], frame[3], rim, 1.2 + 1.3 * hurt);
    // the face's window in the portrait (our framing: Fuse's face is left of centre, Octane's right)
    let face = if pack.portrait.as_deref().is_some_and(|a| a.ends_with("/octane")) { [0.36, 0.0, 0.92, 0.86] } else { [0.08, 0.0, 0.64, 0.86] };
    let window = [[86.0, 955.0], [172.0, 955.0], [172.0, 1027.0], [86.0, 1027.0]];
    p.image_cut("health", [86.0, 953.0, 86.0, 76.0], face, window, 1.0);
    if hurt > 0.0 {
        p.poly(&window, alpha(red, 0.38 * hurt));
    }
    p.poly(&[[58.0, 955.0], [67.0, 955.0], [57.0, 991.0], [67.0, 1027.0], [58.0, 1027.0], [48.0, 991.0]], squad);
    p.poly(&[[419.0, 1020.0], [431.0, 1020.0], [435.0, 1027.0], [423.0, 1027.0]], squad);
    if let Some(name) = player_name() {
        p.text(Face::Body, &name, 178.0, 969.0, 15.0, alpha(c.white, 0.92), Align::Left);
    }
    for x in [331.0, 366.0] {
        p.rounded([x, 961.0, 29.0, 27.0], 2.0, rgb(18, 18, 20, 0.72), alpha(c.rim, 0.75));
    }
    shield_bar(p, pack);
    let max = spike::lethal::FUSE_MAX_HP;
    let frac = (hp / max).clamp(0.0, 1.0);
    let bar = |f: f32| {
        let r = 177.0 + 235.0 * f;
        [[177.0, 1008.0], [r, 1008.0], [r + 5.0, 1014.0], [r, 1020.0], [177.0, 1020.0]]
    };
    p.poly(&bar(1.0), [0.0, 0.0, 0.0, 0.45]);
    // Octane's passive shows where it heals to (R5R's community script: the "green regen HUD
    // bar", target_health; its colour here is our guess)
    if let Some(target) = spike::octane::regen_target() {
        let t = (target / max).clamp(0.0, 1.0);
        if t > frac {
            p.poly(&bar(t), rgb(120, 220, 120, 0.75));
        }
    }
    if let Some((top, a)) = lost(hp) {
        p.poly(&bar((top / max).clamp(0.0, 1.0)), alpha(red, a));
    }
    if frac > 0.0 {
        p.poly(&bar(frac), c.white);
    }

    // survival (empty) and tactical, slanted cells with their keys under them. The tactical looks
    // ready while the stim lasts (the video: unchanged 24.6-30.6 s), then dims and shows the
    // seconds left where its key was ("%s1 sec", the video's "0 sec" at 30.70 s).
    let state = spike::octane::abilities();
    let slot = |x: f32| cell(x, 975.0, 62.0, 52.0, SLANT);
    let q = slot(462.0);
    p.poly(&q, rgb(34, 33, 34, 0.7));
    dots(p, &q, rgb(150, 150, 150, 0.12));
    // the shield battery, endless (D-032): its icon in the middle of the cell and ∞ lower right,
    // where the video's medkit has them; the rim lights while it is in use (ours)
    let using = spike::battery::hud().using.is_some();
    p.outline(&q, if using { c.white } else { alpha(c.rim, 0.8) }, if using { 2.0 } else { 1.5 });
    p.image(BATTERY_ICON, centred(&q, 23.0, 44.0), 1.0);
    p.text(Face::Bold, "∞", 534.0, 1010.0, 14.0, alpha(c.white, 0.95), Align::Right);
    key_cap(p, [505.0, 1035.0, 20.0, 18.0], "4");
    // Q's ability: the stim, or the grapple the weapon wheel picked (no cooldown: grapple.rs)
    let grapple = spike::grapple::q_ability() == spike::grapple::QAbility::Grapple;
    let cooling = !grapple && !state.tactical_active && state.tactical_left > 0.0;
    let q = slot(560.0);
    p.poly(&q, rgb(34, 33, 34, 0.7));
    dots(p, &q, rgb(150, 150, 150, 0.12));
    if cooling {
        p.poly(&q, [0.0, 0.0, 0.0, 0.45]);
    }
    p.outline(&q, alpha(c.rim, if cooling { 0.45 } else { 0.8 }), 1.5);
    if grapple {
        // (its icon is 252 x 324)
        p.image(GRAPPLE_ICON, centred(&q, 34.0, 44.0), 1.0);
    } else {
        p.image("tactical", [570.0, 979.0, 62.0, 44.0], if cooling { 0.4 } else { 1.0 });
    }
    if cooling {
        let n = format!("{:.0}", state.tactical_left.floor());
        let s = pack.strings.get("0xf90fd1bcaaced48a").filter(|s| !s.is_empty()).map_or(n.clone(), |f| f.replace("%s1", &n));
        p.text(Face::Body, &s, 624.0, 1037.0, 15.0, c.white, Align::Center);
    } else {
        key_cap(p, [614.0, 1035.0, 20.0, 18.0], "Q");
    }
}

/// The shield bar above the health bar (D-032): a segment per SHIELD_SEGMENT of the armour's
/// capacity, filled in the armour's tier colour (purple, HUD_LOOT_TIER3); the shield just lost
/// flashes white and fades like the health's red; while the battery charges, what it will fill
/// pulses (S3 shows a `target_shields` preview; this look is ours). Without armour
/// (`shield_tier = 0`), the two grey segments of D-031.
fn shield_bar(p: &Pen, pack: &Pack) {
    let (shield, max) = spike::lethal::shield().unwrap_or((0.0, 0.0));
    let (x0, w, gap, y, h) = (177.0, 56.0, 3.0, 996.0, 7.0);
    let seg = |x: f32, a: f32, b: f32| [[x + w * a, y], [x + w * b, y], [x + w * b, y + h], [x + w * a, y + h]];
    if max <= 0.0 {
        for i in 0..2 {
            p.poly(&seg(x0 + i as f32 * (w + gap), 0.0, 1.0), rgb(150, 148, 146, 0.8));
        }
        return;
    }
    let tier = color(pack, "HUD_LOOT_TIER3", rgb(152, 41, 245, 1.0));
    let trail = lost_in(&SHIELD_LOST, shield);
    let charging = spike::battery::hud().using.filter(|&t| t >= spike::battery::RAISE);
    let n = (max / SHIELD_SEGMENT).round().max(1.0) as usize;
    for i in 0..n {
        let x = x0 + i as f32 * (w + gap);
        let lo = i as f32 * SHIELD_SEGMENT;
        let fill = ((shield - lo) / SHIELD_SEGMENT).clamp(0.0, 1.0);
        p.poly(&seg(x, 0.0, 1.0), rgb(20, 20, 22, 0.55));
        if let Some(t) = charging
            && fill < 1.0
        {
            let pulse = 0.5 + 0.5 * (std::f32::consts::TAU * t / 0.8).cos();
            p.poly(&seg(x, fill, 1.0), alpha(tier, 0.25 + 0.25 * pulse));
        }
        if let Some((top, a)) = trail {
            let t = ((top - lo) / SHIELD_SEGMENT).clamp(0.0, 1.0);
            if t > fill {
                p.poly(&seg(x, fill, t), rgb(255, 255, 255, 0.85 * a));
            }
        }
        if fill > 0.0 {
            p.poly(&seg(x, 0.0, fill), tier);
        }
        p.outline(&seg(x, 0.0, 1.0), alpha(tier, 0.45), 1.0);
    }
}

/// The battery in use and its refusals, at the centre (D-032). S3 shows `ui/consumable_progress`
/// (icon, name, raise and charge times; its RUI is not decoded): here a ring under the crosshair
/// that fills over the 5 s, the icon in it and the name under it (推断). A refusal
/// (`#DENY_SHIELD_FULL`...) shows below it for 2 s (S3: a hint).
fn battery_use(p: &Pen, pack: &Pack, c: &Colors) {
    let b = spike::battery::hud();
    let (cx, cy, r) = (960.0, 640.0, 34.0);
    if let Some(t) = b.using {
        let f = (t / spike::battery::USE_SECONDS).clamp(0.0, 1.0);
        let at = |k: f32| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * k;
            [cx + r * a.cos(), cy + r * a.sin()]
        };
        const N: usize = 64;
        for i in 0..N {
            let (k0, k1) = (i as f32 / N as f32, (i + 1) as f32 / N as f32);
            p.line(at(k0), at(k1), rgb(0, 0, 0, 0.45), 6.0);
            if k0 < f {
                p.line(at(k0), at(k1.min(f)), alpha(c.white, 0.95), 4.0);
            }
        }
        p.image(BATTERY_ICON, [cx - 11.0, cy - 21.0, 22.0, 42.0], 1.0);
        if let Some(name) = pack.strings.get("#SURVIVAL_PICKUP_HEALTH_COMBO_LARGE").filter(|s| !s.is_empty()) {
            p.text(Face::Body, name, cx, cy + r + 10.0, 14.0, alpha(c.white, 0.95), Align::Center);
        }
    }
    if let Some((key, age)) = b.deny
        && let Some(text) = pack.strings.get(key).filter(|s| !s.is_empty())
    {
        let a = 1.0 - ((age - 1.5) / 0.5).clamp(0.0, 1.0);
        p.text(Face::Body, text, cx + 1.5, cy + r + 37.5, 16.0, rgb(0, 0, 0, 0.6 * a), Align::Center);
        p.text(Face::Body, text, cx, cy + r + 36.0, 16.0, alpha(c.white, a), Align::Center);
    }
}

/// Bottom centre, as S3's ultimate in the user's R5R video: a six-sided frame (narrow top, wide
/// base) with the icon, dimmed while it charges, the charge in % under it (the video: 32 -> 42 %
/// over 9 s, the pad's 90 s); the key once it is ready (推断).
fn ultimate_slot(p: &Pen, c: &Colors) {
    let state = spike::octane::abilities();
    let ready = state.ultimate_left <= 0.0;
    let hex = [[935.0, 921.0], [987.0, 921.0], [1025.0, 1002.0], [1010.0, 1025.0], [912.0, 1025.0], [897.0, 1002.0]];
    p.poly(&hex, rgb(30, 28, 28, 0.72));
    dots(p, &hex, rgb(150, 150, 150, 0.1));
    let inner = [[939.0, 929.0], [983.0, 929.0], [1015.0, 999.0], [1004.0, 1016.0], [918.0, 1016.0], [907.0, 999.0]];
    p.outline(&inner, alpha(c.rim, 0.35), 1.2);
    p.outline(&hex, rgb(196, 190, 186, if ready { 1.0 } else { 0.85 }), 2.5);
    p.image("ultimate", [925.0, 945.0, 72.0, 64.0], if ready { 1.0 } else { 0.45 });
    if ready {
        key_cap(p, [950.0, 1035.0, 20.0, 18.0], "Z");
    } else {
        p.text(Face::Body, &format!("{:.0}%", (state.ultimate_ready * 100.0).floor()), 961.0, 1037.0, 17.0, c.white, Align::Center);
    }
}

/// A key cap: light rounded square, the key in black (gun-motion spec 6.2).
fn key_cap(p: &Pen, [x, y, w, h]: [f32; 4], key: &str) {
    let cap = rgb(208, 205, 191, 0.95);
    p.rounded([x, y, w, h], 3.0, cap, cap);
    p.text(Face::Bold, key, x + w * 0.5, y + h * 0.2, h * 0.6, rgb(10, 10, 10, 1.0), Align::Center);
}

/// The magazine: always two digits, right-aligned at x (S3, spec 6.3); leading zeros dark grey,
/// the rest white glowing in the ammo colour; empty, its last 0 in the ammo colour with a box of
/// that colour (the leading 0 in a grey one).
fn magazine(p: &Pen, n: u32, x: f32, y: f32, h: f32, white: [f32; 4], ammo: [f32; 4]) {
    let s = format!("{:02}", n.min(99));
    let grey = rgb(96, 100, 104, 0.9);
    let w = font::width(Face::Bold, "0", h);
    let lead = s.len() - s.trim_start_matches('0').len().max(1);
    for (i, ch) in s.chars().enumerate() {
        let right = x - (s.len() - 1 - i) as f32 * w;
        let d = ch.to_string();
        if n == 0 {
            let col = if i + 1 == s.len() { ammo } else { grey };
            p.rounded([right - w - 2.0, y - 4.0, w + 4.0, h + 8.0], 3.0, [0.0; 4], col);
            p.text(Face::Bold, &d, right, y, h, col, Align::Right);
        } else if i < lead {
            p.text(Face::Bold, &d, right, y, h, grey, Align::Right);
        } else {
            for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                p.text(Face::Bold, &d, right + dx, y + dy, h, alpha(ammo, 0.45), Align::Right);
            }
            p.text(Face::Bold, &d, right, y, h, white, Align::Right);
        }
    }
}

/// Bottom right, as S3's weapon frame in the user's R5R video: a dark dot-screened panel rimmed in
/// the weapon's ammo colour, the weapon, the magazine, the fire mode, the reserve (endless: ∞,
/// D-003), the ammo badge, the empty attachment slots, and under it the two slots' names on their
/// keys, each tab in its weapon's ammo colour, the selected one bright (U3: 1 the R-301, 2 the
/// Charge Rifle; S3 `weaponTabSelected`, `ammoColorTab0/1`). The ordnance slot (G) beside it.
fn weapon_frame(p: &Pen, pack: &Pack, g: &spike::gun::HudState, c: &Colors) {
    let l = look(g.slot);
    let ammo = ammo_colour(pack, &l);
    let body = [[1527.0, 955.0], [1880.0, 955.0], [1870.0, 1031.0], [1510.0, 1031.0], [1510.0, 972.0]];
    p.poly(&body, rgb(36, 33, 32, 0.68));
    dots(p, &body, rgb(150, 150, 150, 0.12));
    p.hgrad([1510.0, 1012.0, 360.0, 19.0], alpha(ammo, 0.0), alpha(ammo, 0.18));
    p.outline(&body, alpha(ammo, 0.85), 1.5);
    p.image(l.icon, [1568.0, 960.0, 132.0, 42.0], 1.0);
    magazine(p, g.ammo, 1781.0, 965.0, 30.0, c.white, ammo);
    // the fire mode as it is (no switching: D-023), the endless reserve under the magazine
    p.image(l.mode, [1700.0, 996.0, 20.0, 10.0], 1.0);
    p.text(Face::Bold, "∞", 1781.0, 1003.0, 16.0, alpha(ammo, 0.9), Align::Right);
    p.image(l.badge, [1789.0, 958.0, 72.0, 64.0], 1.0);
    // its attachment slots, empty (the R-301: barrel, magazine, sight, stock)
    for (i, slot) in l.slots.iter().enumerate() {
        let x = 1514.0 + i as f32 * 29.0;
        p.rounded([x, 1001.0, 25.0, 23.0], 3.0, rgb(40, 38, 38, 0.6), alpha(c.rim, 0.35));
        p.tinted(slot, [x + 3.0, 1003.0, 19.0, 19.0], rgb(150, 150, 150, 0.45));
    }
    // the slots' names (S3 writes their short names) on their keys: the selected tab bright
    let tabs = [[[1522.0, 1033.0], [1702.0, 1033.0], [1692.0, 1058.0], [1530.0, 1058.0]], [[1702.0, 1033.0], [1836.0, 1033.0], [1826.0, 1058.0], [1692.0, 1058.0]]];
    for (i, quad) in tabs.iter().enumerate() {
        let tab = look(i as u8);
        let selected = i as u8 == g.slot;
        let colour = ammo_colour(pack, &tab);
        p.poly(quad, if selected { shade(colour, 0.55, 0.9) } else { shade(colour, 0.18, 0.88) });
        let (key_x, centre) = if i == 0 { (1532.0, 1612.0) } else { (1705.0, 1773.0) };
        key_cap(p, [key_x, 1037.0, 18.0, 18.0], if i == 0 { "1" } else { "2" });
        let text = if selected { c.white } else { rgb(200, 200, 196, 0.85) };
        p.text(Face::Body, weapon_name(pack, &tab), centre, 1040.0, 13.0, text, Align::Center);
    }

    // ordnance (S3: G): the frag grenade, endless (D-003/D-032: ∞), its slot lit while it is in the
    // hand (U9; the lit rim is 推断); the empty look without its icon. Icon in the middle, the
    // count lower left, mirroring the survival slot (the cell leans the other way; the user's
    // 10-06 frame: "1" there)
    let x = 1415.0;
    let q = cell(x, 975.0, 62.0, 52.0, -0.49);
    let held = spike::grenade::holding();
    p.poly(&q, rgb(34, 33, 34, 0.7));
    dots(p, &q, rgb(150, 150, 150, 0.12));
    p.outline(&q, if held { alpha(c.white, 0.95) } else { alpha(c.rim, 0.8) }, if held { 2.5 } else { 1.5 });
    if spike::grenade::enabled() && tex::get(super::grenade::ICON).is_some() {
        p.tinted(super::grenade::ICON, centred(&q, 44.0, 36.0), alpha(c.white, if held { 1.0 } else { 0.9 }));
        p.text(Face::Bold, "∞", x - 12.0, 1010.0, 14.0, alpha(c.white, 0.9), Align::Left);
    } else {
        p.line([x - 2.0, 1014.0], [x + 33.0, 991.0], alpha(c.rim, 0.6), 2.0);
    }
    key_cap(p, [x - 4.0, 1035.0, 18.0, 18.0], "G");
}

/// Elden Ring's boss in an Apex bar (bottom centre, above the frames).
fn boss_bar(p: &Pen, pack: &Pack, c: &Colors) {
    let Some(boss) = fe::bosses().into_iter().next() else { return };
    let Some(&[x, y, w, _]) = pack.bounds.get("boss_health") else { return };
    let frac = (boss.hp as f32 / boss.hp_max.max(1) as f32).clamp(0.0, 1.0);
    let trail = Trail::update(&BOSS_TRAIL, frac, BOSS_TRAIL_SECONDS).clamp(0.0, 1.0);
    p.text(Face::Body, &boss.name, x + 6.0, y, 21.0, c.white, Align::Left);
    p.text(Face::Bold, &format!("{:.0}%", (frac * 100.0).ceil()), x + w - 6.0, y + 2.0, 18.0, c.white, Align::Right);
    let track = cell(x, y + 30.0, w - 8.0, 16.0, SLANT);
    p.poly(&track, c.panel);
    p.outline(&track, c.rim, 1.5);
    let bar = |f: f32| cell(x + 3.0, y + 33.0, (w - 14.0) * f, 10.0, SLANT);
    if trail > frac {
        p.poly(&bar(trail), alpha(c.white, 0.6));
    }
    if frac > 0.0 {
        p.poly(&bar(frac), c.bleed);
    }
}

/// Screen centre and the world: crosshair, hit marker, damage numbers, the last target's health.
fn aim(dl: &DrawListMut, pack: &Pack, size: [f32; 2], fov: f32, g: &spike::gun::HudState, c: &Colors) {
    let k = (size[0] / 1920.0).min(size[1] / 1080.0);
    // the Charge Rifle's crosshair is where its shots go: its shot's kick swings it up and back as
    // it does the gun, and the laser ends in it (the user, 2026-10-06)
    let [cx, cy] = g.charge.and_then(|_| kicked_aim(size)).unwrap_or([size[0] * 0.5, size[1] * 0.5]);

    // crosshair: three ticks out at the spread (hip; the Charge Rifle: its charge arcs), the dot
    // alone aiming (and with the frag grenade out: its RUI_CrosshairData is ui/crosshair_dot, U9),
    // nothing reloading or sprinting
    if g.reload.is_none() && !g.sprinting {
        let x = &pack.crosshair;
        let o = x.outline_px * k;
        if !g.aiming && !spike::grenade::holding() {
            if let Some(charge) = g.charge {
                charge_arcs(dl, [cx, cy], k, charge, c);
            } else {
                let gap = (g.spread_deg.to_radians().tan() / (fov * 0.5).tan() * size[1] * 0.5).max(x.min_gap_px * k);
                for a in hip_ticks(&x.angles_deg) {
                    // the pack's angles have y up (90° the top tick); the screen's y is down
                    let (s, co) = a.to_radians().sin_cos();
                    let (r0, r1) = (gap, gap + x.tick_px * k);
                    outlined_bar(dl, [cx + co * r0, cy - s * r0], [cx + co * r1, cy - s * r1], c.white, x.thickness_px * k, o);
                }
            }
        }
        let h = x.dot_px * k;
        if x.square_dot {
            dl.add_rect([cx - h - o, cy - h - o], [cx + h + o, cy + h + o], [0.0, 0.0, 0.0, OUTLINE_ALPHA]).filled(true).build();
            dl.add_rect([cx - h, cy - h], [cx + h, cy + h], c.white).filled(true).build();
        } else {
            dl.add_circle([cx, cy], h + o, [0.0, 0.0, 0.0, OUTLINE_ALPHA]).filled(true).build();
            dl.add_circle([cx, cy], h, c.white).filled(true).build();
        }
    }

    // hit marker: an X round the centre, yellow on headshots
    if let Some((at, is_head)) = g.last_hit {
        let t = at.elapsed().as_secs_f32();
        if t < MARKER_SECONDS {
            let m = &pack.hit_marker;
            let col = alpha(if is_head { c.head } else { c.white }, 1.0 - t / MARKER_SECONDS);
            let (r0, r1) = (m.inner_px * k * FRAC_1_SQRT_2, m.outer_px * k * FRAC_1_SQRT_2);
            for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                outlined_bar(dl, [cx + dx * r0, cy + dy * r0], [cx + dx * r1, cy + dy * r1], col, m.thickness_px * k, m.outline_px * k);
            }
        }
    }

    damage_numbers(dl, pack, size, c);
}

/// The hip crosshair's ticks of slot 1's gun, as its retail `RUI_CrosshairData` names its RUI:
/// R-301 `ui/crosshair_tri` (the pack's three, measured on the user's R5R video), R-99 and Wingman
/// `ui/crosshair_plus`, Sentinel `ui/crosshair_plus_dot` (four: up, left, down, right; the dot as
/// always), Flatline `ui/crosshair_alternator` (its RUI is not decoded: the plus stands in, 推断).
/// The ticks' lengths, widths and gap rule are the R-301's for all of them (推断).
fn hip_ticks(tri: &[f32]) -> Vec<f32> {
    use spike::weapons::Gun;
    match spike::weapons::primary_gun() {
        Gun::R301 => tri.to_vec(),
        Gun::R99 | Gun::Wingman | Gun::Flatline | Gun::Sentinel => vec![0.0, 90.0, 180.0, 270.0],
    }
}

/// U3: the Charge Rifle's crosshair round its dot (the user, 2026-10-06: "the left and right are its
/// charge bars"). S3's `ui/crosshair_charge_rifle` takes the charge (`chargeFrac` <-
/// `player_chargeFrac`) but its RUI is not decoded (RSX: render program not implemented); the look
/// is the user's video's (scratch/video/cr1006, 1080p: frames 549, 610-617): two arcs either side,
/// 46 px out, 55° above and below level, dark when empty, filling white from their lower ends up
/// with the charge (half way 0.27 s into a 0.5 s discharge), and draining so after it. No spread
/// ticks (the video shows none); not while aiming (its scope shows none).
fn charge_arcs(dl: &DrawListMut, [cx, cy]: [f32; 2], k: f32, charge: f32, c: &Colors) {
    const R: f32 = 46.0;
    const HALF: f32 = 55.0;
    const N: usize = 24;
    let (r, f) = (R * k, charge.clamp(0.0, 1.0));
    // a side's arc from its lower end (degrees, the screen's y down) up to `upto` of it
    let arc = |from: f32, dir: f32, upto: f32| -> Vec<[f32; 2]> {
        (0..=N)
            .map(|i| {
                let a = (from + dir * 2.0 * HALF * upto * i as f32 / N as f32).to_radians();
                [cx + r * a.cos(), cy + r * a.sin()]
            })
            .collect()
    };
    for (from, dir) in [(180.0 - HALF, 1.0), (HALF, -1.0)] {
        dl.add_polyline(arc(from, dir, 1.0), [0.0, 0.0, 0.0, 0.45]).thickness(5.0 * k).build();
        if f > 0.0 {
            dl.add_polyline(arc(from, dir, f), alpha(c.white, 0.95)).thickness(3.2 * k).build();
        }
    }
}

/// The damage numbers and the name plate (see `STACK_*`).
fn damage_numbers(dl: &DrawListMut, pack: &Pack, size: [f32; 2], c: &Colors) {
    let _ = pack;
    let k = (size[0] / 1920.0).min(size[1] / 1080.0);
    let hits = spike::gun::hits();
    let Some(newest) = hits.last() else { return };
    let key = |h: &spike::gun::Hit| (h.target.selector.0, h.target.block_id.0);
    let target = key(newest);
    // the stack: the last enemy's hits back to a gap of STACK_GAP
    let mine: Vec<&spike::gun::Hit> = hits.iter().filter(|h| key(h) == target).collect();
    let Some(last) = mine.last().copied() else { return };
    let mut total = 0.0;
    let mut prev: Option<std::time::Instant> = None;
    for h in mine.iter().rev() {
        if prev.is_some_and(|p| p.duration_since(h.at).as_secs_f32() > STACK_GAP) {
            break;
        }
        total += h.damage;
        prev = Some(h.at);
    }
    let wcm = unsafe { WorldChrMan::instance() }.ok();
    let Some(chr) = wcm.and_then(|w| w.chr_ins_by_handle(&last.target)) else { return };
    let ph = &chr.modules.physics;
    let height = spike::body::cylinder(chr).0;
    let (hp, max) = (chr.modules.data.hp.max(0) as f32, chr.modules.data.max_hp.max(1) as f32);
    let Some(q) = project(Vec3::new(ph.position.0, ph.position.1 + height + 0.15, ph.position.2), size) else { return };
    let since = last.at.elapsed().as_secs_f32();
    // out: at once when it died, else after the hold
    let out_from = if hp <= 0.0 { 0.0 } else { STACK_HOLD };
    let out = ((since - out_from) / STACK_OUT).clamp(0.0, 1.0);
    if out >= 1.0 {
        return;
    }
    let a = 1.0 - out * out;
    let rise = STACK_RISE * k * out;
    let edge = [0.0, 0.0, 0.0, 0.7 * a];
    let (bw, bh) = (PLATE_BAR[0] * k, PLATE_BAR[1] * k);
    // keep the plate in view close up
    // close up a tall enemy's head is above the screen: keep the plate below the compass and its
    // heading (Godrick from 3 m put the numbers on the compass, 2026-10-05 19:26)
    let bar_y = (q[1] - bh).max(150.0 * k);
    let x0 = q[0] - bw * 0.5;
    let name_h = PLATE_NAME_HEIGHT * k;
    let name_top = bar_y - 4.0 * k - name_h;
    let mut name_w = 0.0;
    // a boss has its own bar at the bottom: no plate, the numbers only
    if hp > 0.0 && !spike::combat::is_boss(chr) {
        let name = spike::stats::name_of(chr.npc_param_id);
        name_w = font::draw_outlined(dl, Face::Body, &name, [x0, name_top], name_h, alpha(c.white, a), edge, 1.0, Align::Left);
        dl.add_rect([x0 - 1.0, bar_y - 1.0], [x0 + bw + 1.0, bar_y + bh + 1.0], [0.0, 0.0, 0.0, 0.45 * a]).filled(true).build();
        let frac = (hp / max).clamp(0.0, 1.0);
        let trail = Trail::update(&TARGET_TRAIL, frac, TRAIL_SECONDS).clamp(0.0, 1.0);
        if trail > frac {
            dl.add_rect([x0, bar_y], [x0 + bw * trail, bar_y + bh], [0.6, 0.6, 0.6, 0.8 * a]).filled(true).build();
        }
        dl.add_rect([x0, bar_y], [x0 + bw * frac, bar_y + bh], alpha(c.white, a)).filled(true).build();
    }
    // the stack: right of the name, its foot on the name's baseline; the newest hit alone first
    let sh = STACK_HEIGHT * k;
    let sx = x0 + (name_w - 6.0 * k).max(0.0);
    let sy = name_top + name_h - sh - rise;
    let (value, col) = if since < STACK_HIT_SHOW {
        (last.damage, if last.head { c.head } else { c.white })
    } else {
        (total, c.white)
    };
    font::draw_outlined(dl, Face::Body, &format!("{value:.0}"), [sx, sy], sh, alpha(col, a), edge, 1.2 * k, Align::Left);
    // each hit's own number, small, rising over the stack
    for h in mine.iter().filter(|h| h.at.elapsed().as_secs_f32() < FLOAT_SECONDS) {
        let t = h.at.elapsed().as_secs_f32() / FLOAT_SECONDS;
        let fa = (1.0 - t * t) * a;
        let col = if h.head { c.head } else { c.white };
        let fy = sy - FLOAT_HEIGHT * k - 4.0 * k - FLOAT_RISE * k * t;
        font::draw_outlined(dl, Face::Body, &format!("{:.0}", h.damage), [sx + 4.0 * k, fy], FLOAT_HEIGHT * k, alpha(col, fa), [0.0, 0.0, 0.0, 0.7 * fa], 1.0, Align::Left);
    }
}

static TARGET_TRAIL: Mutex<Option<Trail>> = Mutex::new(None);

/// S3's knock-down message (stats.rs `Knock`): `#SCORE_EVENT_SUR_DOWNED_PILOT_HUD` in the HUD
/// language, its runs coloured by their RUI codes (`0 white, `1 the enemy's red, `2 grey, `3 white
/// figures), centred below the crosshair; it comes in large and faint and settles in 0.12 s, then
/// fades over its last 0.25 s. Place, sizes, panel and the codes' faces measured off the user's R5R
/// video (2026-10-05: "KNOCKED DOWN DUMMIE / 120 DAMAGE INFLICTED", 19.10-21.55 s).
fn knock_message(dl: &DrawListMut, pack: &Pack, size: [f32; 2], c: &Colors) {
    let Some((age, victim, inflicted)) = spike::stats::knock() else { return };
    let Some(format) = pack.strings.get("#SCORE_EVENT_SUR_DOWNED_PILOT_HUD").filter(|s| !s.is_empty()) else { return };
    let text = format.replace("%s1", &victim).replace("%s2", &format!("{inflicted:.0}"));
    let k = (size[0] / 1920.0).min(size[1] / 1080.0);
    let cx = size[0] * 0.5;
    const IN: f32 = 0.12;
    const OUT: f32 = 0.25;
    let settle = (age / IN).min(1.0);
    let scale = 1.0 + (1.0 - settle) * (1.0 - settle);
    let a = (0.35 + 0.65 * settle) * (1.0 - ((age - (spike::stats::KNOCK_SECONDS - OUT)) / OUT).clamp(0.0, 1.0));
    let red = color(pack, "ENEMY", rgb(255, 45, 13, 1.0));
    let grey = rgb(178, 178, 178, 1.0);
    // the panel behind
    let (top, bottom) = (size[1] * 0.705, size[1] * 0.763);
    dl.add_rect([cx - 170.0 * k, top], [cx + 170.0 * k, bottom], [0.0, 0.0, 0.0, 0.35 * a * settle]).filled(true).build();
    // the localization keeps the line break as the two characters \ n
    for (i, line) in text.split("\\n").enumerate() {
        let (h, y) = if i == 0 { (18.0, 0.712) } else { (12.0, 0.744) };
        let h = h * k * scale;
        let y = size[1] * y - (scale - 1.0) * h;
        // runs: (code, text)
        let mut runs: Vec<(char, String)> = vec![('0', String::new())];
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '`' && chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                runs.push((chars.next().unwrap_or('0'), String::new()));
            } else if let Some(r) = runs.last_mut() {
                r.1.push(ch);
            }
        }
        let style = |code: char| match code {
            '1' => (Face::Numeric, red),
            '2' => (Face::Body, grey),
            '3' => (Face::Bold, c.white),
            _ => (Face::Body, c.white),
        };
        let widths: Vec<f32> = runs.iter().map(|(code, t)| font::width(style(*code).0, t, h)).collect();
        let mut x = cx - widths.iter().sum::<f32>() * 0.5;
        for ((code, t), w) in runs.iter().zip(&widths) {
            let (face, col) = style(*code);
            font::draw(dl, face, t, [x, y], h, alpha(col, a), Align::Left);
            x += w;
        }
    }
}
