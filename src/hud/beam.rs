//! The Charge Rifle's beams (U3, spike/chargerifle.rs). S3 draws them as particles from the muzzle:
//! the laser while the discharge goes on (`sustained_laser_effect_1p` P_wpn_defender_beam_sustained,
//! looping), its end (`sustained_laser_impact_effect` P_defender_end_default, 3 units off the
//! surface), the charge at the muzzle (`charge_effect_1p` P_wpn_defender_charge_FP) and the shot's
//! tracer (`tracer_effect_first_person` P_wpn_defender_beam). Those particle systems are in S3's
//! .pcf files inside R5R's VPKs, which no local tool reads (待定): here they are drawn on the HUD as
//! the user's video of the release shows them (2026-10-06, scratch/video/cr1006; 视频实测 where a
//! number says so, the rest 推断), in soft shapes made here (a beam's cross-section and a round
//! glow, both falling off as exp(-4.5 x^2)):
//! - the laser from the muzzle (the view model's `muzzle_flash`, firstperson.rs `muzzle_world`) to
//!   where the ray ends: a warm white core (255, 244, 214) in a glow going orange (255, 150, 72),
//!   some 30 px across at the muzzle at 1080p, 10-15 px far off; thin in the scope (aimed, the gun
//!   is under the eye); a flare where it ends (a red one on a character), arcs at the muzzle;
//! - aimed, the screen warms over the discharge: from 0.11 s, eased over 0.3 s (1 - (1 - x)^2), to a
//!   0.57 blend of (255, 176, 108) in the middle and about half of it at the sides (from the hip,
//!   a fifth of it); it is gone 0.2 s after the shot;
//! - the shot: a flash of the screen (a frame of the video), its beam from where the muzzle was then
//!   (the video's stays in the world as the view swings after the shot) to its end, thick (some
//!   80 px across near), fading as 1 - (t / 0.35)^1.7, electric arcs about it for 0.3 s; then the
//!   muzzle glows and smokes (the video: smoke 0.4-0.9 s after a shot).
//! The glow's colour can be set (ini `cr_beam_rgb = r,g,b`, read fresh). Drawn under the rest of the
//! HUD; nothing stands between the muzzle and the ray's end (the ray stops at the first thing it
//! meets).

use std::f32::consts::TAU;
use std::sync::Once;

use glam::Vec3;
use hudhook::imgui::DrawListMut;

use super::{project, tex, view};
use crate::spike::chargerifle;

/// Colours: the core, the middle, the glow (its default), the laser's end on a character.
const CORE: [f32; 3] = [1.0, 0.957, 0.84];
const MID: [f32; 3] = [1.0, 0.76, 0.46];
const DEFAULT_GLOW: [f32; 3] = [1.0, 0.59, 0.28];
const RED: [f32; 3] = [1.0, 0.27, 0.16];
/// The aimed screen's warmth: its colour and the blend in the middle at its height.
const WARM: [f32; 3] = [1.0, 0.69, 0.42];
const WARM_PEAK: f32 = 0.57;
/// From the hip the warmth is this much of the aimed one.
const WARM_HIP: f32 = 0.2;

/// A beam's layers (glow, middle, core): pixels wide at the muzzle (at 1080p), metres wide at its
/// end and at least so many pixels there, alpha on its middle line.
type Layers = [(f32, f32, f32, f32); 3];
const LASER: Layers = [(70.0, 0.3, 30.0, 0.55), (30.0, 0.12, 12.0, 0.8), (12.0, 0.04, 5.0, 1.0)];
const SHOT: Layers = [(220.0, 1.0, 80.0, 0.6), (90.0, 0.4, 32.0, 0.85), (34.0, 0.12, 12.0, 1.0)];
/// The shot's beam fades as 1 - (t / SHOT_FADE)^SHOT_FADE_POW.
const SHOT_FADE: f32 = 0.35;
const SHOT_FADE_POW: f32 = 1.7;
/// How long the shot's arcs last (s); the smoke's puffs and how long each lasts.
const ARCS: f32 = 0.3;
const PUFFS: usize = 6;
const PUFF_LIFE: f32 = 0.7;
/// Nearer than this (m, along the view) a beam is cut (project() draws nothing behind 0.1 m).
const NEAR_Z: f32 = 0.15;

/// The soft shapes: white, their alpha the falloff across a beam / from a glow's centre.
const BEAM_TEX: &str = "cr/beam";
const GLOW_TEX: &str = "cr/glow";
static MADE: Once = Once::new();

fn make_textures() {
    MADE.call_once(|| {
        let fall = |x: f32| ((-4.5 * x * x).exp() * 255.0).round() as u8;
        let at = |i: u32, n: u32| (i as f32 + 0.5) / n as f32 * 2.0 - 1.0;
        let (w, h) = (64, 4);
        let beam = (0..h).flat_map(|_| (0..w).flat_map(move |i| [255, 255, 255, fall(at(i, w))])).collect();
        tex::generate(BEAM_TEX, w, h, beam);
        let n = 64;
        let glow = (0..n).flat_map(|j| (0..n).flat_map(move |i| [255, 255, 255, fall((at(i, n).powi(2) + at(j, n).powi(2)).sqrt())])).collect();
        tex::generate(GLOW_TEX, n, n, glow);
    });
}

fn glow_rgb() -> [f32; 3] {
    crate::paths::config("cr_beam_rgb")
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect();
            (v.len() == 3).then(|| [v[0] / 255.0, v[1] / 255.0, v[2] / 255.0])
        })
        .unwrap_or(DEFAULT_GLOW)
}

fn rgba(c: [f32; 3], a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a.clamp(0.0, 1.0)]
}

/// Uniform 0..1 from a seed (a hash: the arcs and puffs change when their seed does).
fn rnd(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Pixels a world length `w` at `p` covers on screen (through the camera's right).
fn px(p: Vec3, w: f32, size: [f32; 2]) -> Option<f32> {
    let (_, _, right, _) = view()?;
    let a = project(p, size)?;
    let b = project(p + right.normalize_or_zero() * w, size)?;
    Some(((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt())
}

/// Where the beam starts: the muzzle of the view model, else a little right of and below the eye.
fn muzzle() -> Option<Vec3> {
    crate::firstperson::muzzle_world().or_else(|| {
        let (eye, fwd, right, up) = view()?;
        Some(eye + fwd.normalize_or_zero() * 0.6 + right.normalize_or_zero() * 0.12 - up.normalize_or_zero() * 0.12)
    })
}

/// The part of the segment `a`-`b` in front of the eye (NEAR_Z or more along the view).
fn clip(a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
    let (eye, fwd, _, _) = view()?;
    let fwd = fwd.normalize_or_zero();
    let (za, zb) = ((a - eye).dot(fwd), (b - eye).dot(fwd));
    if za < NEAR_Z && zb < NEAR_Z {
        return None;
    }
    let cut = |p: Vec3, zp: f32, q: Vec3, zq: f32| if zp < NEAR_Z { p + (q - p) * ((NEAR_Z - zp) / (zq - zp)) } else { p };
    Some((cut(a, za, b, zb), cut(b, zb, a, za)))
}

/// A soft band from `a` to `b` (screen), `wa` and `wb` pixels across at its ends.
fn band(dl: &DrawListMut, a: [f32; 2], b: [f32; 2], wa: f32, wb: f32, col: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 || col[3] <= 0.0 {
        return;
    }
    let (nx, ny) = (-dy / len, dx / len);
    let (ha, hb) = (wa.max(1.0) * 0.5, wb.max(1.0) * 0.5);
    let q = [[a[0] + nx * ha, a[1] + ny * ha], [b[0] + nx * hb, b[1] + ny * hb], [b[0] - nx * hb, b[1] - ny * hb], [a[0] - nx * ha, a[1] - ny * ha]];
    match tex::get(BEAM_TEX) {
        // the texture's x runs across, from one side to the other
        Some((id, _)) => dl.add_image_quad(id, q[0], q[1], q[2], q[3]).uv([0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]).col(col).build(),
        // until it is uploaded: a flat band, fainter
        None => dl.add_polyline(q.to_vec(), [col[0], col[1], col[2], col[3] * 0.4]).filled(true).build(),
    }
}

/// A soft round glow, `r` pixels to where it has faded.
fn soft(dl: &DrawListMut, p: [f32; 2], r: f32, col: [f32; 4]) {
    if col[3] <= 0.0 || r <= 0.5 {
        return;
    }
    match tex::get(GLOW_TEX) {
        Some((id, _)) => dl.add_image(id, [p[0] - r, p[1] - r], [p[0] + r, p[1] + r]).col(col).build(),
        None => dl.add_circle(p, r * 0.4, [col[0], col[1], col[2], col[3] * 0.4]).filled(true).num_segments(20).build(),
    }
}

/// A flare: `r` pixels of `c` round a warm white middle.
fn flare(dl: &DrawListMut, p: [f32; 2], r: f32, c: [f32; 3], a: f32) {
    soft(dl, p, r, rgba(c, 0.6 * a));
    soft(dl, p, r * 0.45, rgba(MID, 0.85 * a));
    soft(dl, p, r * 0.2, rgba(CORE, a));
}

/// Aimed, how far along the screen segment `a`-`b` it comes out from behind the gun (0: not
/// behind it): the gun down the sights covers about the middle sixth of the screen's width below a
/// tenth of its height under the crosshair (our view model; the video's scope hides its laser so).
/// A beam fired while aimed and seen after the view turned is not behind it any more.
fn behind_gun(a: [f32; 2], b: [f32; 2], size: [f32; 2], ads: f32) -> f32 {
    if ads < 0.6 {
        return 0.0;
    }
    let (cx, half, top) = (size[0] * 0.5, size[0] * 0.085, size[1] * 0.6);
    let inside = |p: [f32; 2]| (p[0] - cx).abs() < half && p[1] > top;
    if !inside(a) {
        return 0.0;
    }
    // where it leaves the box: through its top or a side
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let mut t: f32 = 1.0;
    if dy < 0.0 {
        t = t.min((top - a[1]) / dy);
    }
    if dx.abs() > 1e-3 {
        let side = if dx > 0.0 { cx + half } else { cx - half };
        t = t.min((side - a[0]) / dx);
    }
    t.clamp(0.0, 1.0)
}

/// A beam from `from` to `to` in the world: its layers, the near widths times `near`, all of it
/// times `a`, aimed not over the gun. Returns its ends on screen (the muzzle's, not cut).
#[allow(clippy::too_many_arguments)]
fn beam(dl: &DrawListMut, size: [f32; 2], from: Vec3, to: Vec3, layers: &Layers, near: f32, glow: [f32; 3], a: f32, ads: f32) -> Option<([f32; 2], [f32; 2])> {
    let (from, to) = clip(from, to)?;
    let (pa, pb) = (project(from, size)?, project(to, size)?);
    let k = size[1] / 1080.0;
    let cut = behind_gun(pa, pb, size, ads);
    let start = [pa[0] + (pb[0] - pa[0]) * cut, pa[1] + (pb[1] - pa[1]) * cut];
    for (&(near_px, far_m, far_min, alpha), c) in layers.iter().zip([glow, MID, CORE]) {
        let far = px(to, far_m, size).unwrap_or(0.0).max(far_min * k);
        // never thinner at the muzzle than far off
        let wa = (near_px * k * near).max(far.min(near_px * k));
        band(dl, start, pb, wa + (far - wa) * cut, far, rgba(c, alpha * a));
    }
    Some((pa, pb))
}

/// An electric arc: a jagged line `len` pixels from `a` along `angle`, kinked by `seed`.
fn arc(dl: &DrawListMut, a: [f32; 2], angle: f32, len: f32, seed: u32, col: [f32; 4], width: f32) {
    const N: u32 = 7;
    let (dx, dy) = (angle.cos(), angle.sin());
    let pts: Vec<[f32; 2]> = (0..=N)
        .map(|i| {
            let f = i as f32 / N as f32;
            let off = if i == 0 || i == N { 0.0 } else { (rnd(seed.wrapping_add(i * 7919)) - 0.5) * len * 0.35 };
            [a[0] + dx * len * f - dy * off, a[1] + dy * len * f + dx * off]
        })
        .collect();
    dl.add_polyline(pts.clone(), [col[0], col[1], col[2], col[3] * 0.3]).thickness(width * 3.0).build();
    dl.add_polyline(pts, col).thickness(width).build();
}

/// `n` arcs out of `p`, new ones every 50 ms of `t`.
#[allow(clippy::too_many_arguments)]
fn arcs(dl: &DrawListMut, p: [f32; 2], n: u32, len: (f32, f32), t: f32, seed: u32, a: f32, k: f32) {
    let bucket = (t / 0.05) as u32;
    for i in 0..n {
        let s = seed.wrapping_mul(1000).wrapping_add(bucket * 31 + i * 7);
        let l = (len.0 + (len.1 - len.0) * rnd(s ^ 0x55)) * k;
        arc(dl, p, rnd(s) * TAU, l, s, [1.0, 0.88, 0.62, 0.9 * a], 1.6 * k);
    }
}

/// The screen warmed: `a` the blend in the middle (a little above the centre), about half of it at
/// the sides.
fn warmth(dl: &DrawListMut, size: [f32; 2], a: f32) {
    if a <= 0.003 {
        return;
    }
    let edge = a * 0.45;
    dl.add_rect([0.0, 0.0], size, rgba(WARM, edge)).filled(true).build();
    let (cx, cy, rx, ry) = (size[0] * 0.5, size[1] * 0.45, size[0] * 0.8, size[1] * 1.4);
    if let Some((id, _)) = tex::get(GLOW_TEX) {
        dl.add_image(id, [cx - rx, cy - ry], [cx + rx, cy + ry]).col(rgba(WARM, (a - edge) / (1.0 - edge))).build();
    }
}

/// The warmth `t` seconds into the discharge, before the aim's share (the video: from 0.11 s, eased
/// over 0.3 s).
fn warm_up(t: f32) -> f32 {
    let x = ((t - 0.11) / 0.3).clamp(0.0, 1.0);
    1.0 - (1.0 - x) * (1.0 - x)
}

/// The Sentinel's shots (the user's 2026-10-10 ask: brighter): an amped blue flash at the muzzle
/// for SN_FLASH s after each, and its homing rounds in flight (spike/homing.rs), a bright streak
/// each, its tail along where it came from, a glow at its head (推断: Apex's amped Sentinel tracer
/// is a particle system no local tool reads).
const SN_FLASH: f32 = 0.15;

pub fn draw_rounds(dl: &DrawListMut, size: [f32; 2]) {
    let rounds = crate::spike::homing::rounds();
    let flash = crate::spike::homing::since_shot().filter(|t| *t < SN_FLASH).map(|t| 1.0 - t / SN_FLASH);
    if rounds.is_empty() && flash.is_none() {
        return;
    }
    make_textures();
    let k = size[1] / 1080.0;
    let col = [0.45, 0.8, 1.0];
    if let Some(a) = flash
        && let Some(p) = muzzle().and_then(|m| project(m, size))
    {
        flare(dl, p, 190.0 * k, col, a * a);
        soft(dl, p, 420.0 * k, rgba(col, 0.25 * a));
    }
    for (pos, dir) in rounds {
        let tail = pos - dir * 4.0;
        let Some((a, b)) = clip(tail, pos) else { continue };
        let (Some(pa), Some(pb)) = (project(a, size), project(b, size)) else { continue };
        let w = px(pos, 0.25, size).unwrap_or(0.0).clamp(6.0 * k, 70.0 * k);
        band(dl, pa, pb, w * 0.5, w * 1.6, rgba(col, 0.45));
        band(dl, pa, pb, w * 0.3, w, rgba(col, 0.8));
        band(dl, pa, pb, w * 0.12, w * 0.45, rgba(CORE, 1.0));
        flare(dl, pb, w * 3.0, col, 1.0);
    }
}

/// Pathfinder's grapple cable (spike/grapple.rs): from his left hand (low left of the view) to the
/// hook, a dark line with a thin light core; reeling back it shortens to the hand.
pub fn draw_cable(dl: &DrawListMut, size: [f32; 2]) {
    let Some((end, back)) = crate::spike::grapple::cable() else { return };
    let Some((eye, fwd, right, up)) = view() else { return };
    let hand = eye + fwd.normalize_or_zero() * 0.5 - right.normalize_or_zero() * 0.22 - up.normalize_or_zero() * 0.2;
    let end = end + (hand - end) * back;
    let Some((a, b)) = clip(hand, end) else { return };
    let (Some(pa), Some(pb)) = (project(a, size), project(b, size)) else { return };
    let k = size[1] / 1080.0;
    dl.add_line(pa, pb, [0.08, 0.08, 0.09, 0.9]).thickness(4.0 * k).build();
    dl.add_line(pa, pb, [0.75, 0.8, 0.85, 0.6]).thickness(1.2 * k).build();
    dl.add_circle(pb, 4.0 * k, [0.2, 0.2, 0.22, 1.0]).filled(true).build();
}

pub fn draw(dl: &DrawListMut, size: [f32; 2]) {
    draw_rounds(dl, size);
    draw_cable(dl, size);
    let b = chargerifle::beams();
    if b.laser.is_none() && b.shot.is_none() && b.cancel.is_none() {
        return;
    }
    make_textures();
    let ads = crate::camera::ads_frac().clamp(0.0, 1.0);
    let aim = WARM_HIP + (1.0 - WARM_HIP) * ads;
    let k = size[1] / 1080.0;
    let glow = glow_rgb();
    let now = muzzle();

    // under the beams: the screen's warmth, rising with the laser and gone soon after the shot (or
    // after S stopped it: from where it had got to)
    let warm = match b.laser {
        Some((_, t, _)) => WARM_PEAK * warm_up(t),
        None => {
            let shot = b.shot.map_or(0.0, |(_, _, age, _)| WARM_PEAK * (-age / 0.07).exp());
            let stopped = b.cancel.map_or(0.0, |(since, ran)| WARM_PEAK * warm_up(ran) * (-since / 0.07).exp());
            shot.max(stopped)
        }
    };
    warmth(dl, size, warm * aim);

    if let (Some((to, t, on_chr)), Some(from)) = (b.laser, now) {
        // a looping particle's noise (推断)
        let flicker = 0.85 + 0.15 * (t * 47.0).sin() * (t * 23.0).cos();
        if let Some((pa, pb)) = beam(dl, size, from, to, &LASER, 1.0 - 0.75 * ads, glow, flicker, ads) {
            let r = px(to, 0.5, size).unwrap_or(0.0).clamp(40.0 * k, 110.0 * k);
            flare(dl, pb, r, if on_chr { RED } else { glow }, flicker);
            // the charge at the muzzle, its arcs about the barrel (behind the gun when aimed)
            let hip = 1.0 - ads;
            flare(dl, pa, (20.0 + 28.0 * (t / chargerifle::DISCHARGE).min(1.0)) * k, glow, hip);
            if hip > 0.05 {
                arcs(dl, pa, 2, (25.0, 60.0), t, b.shots.wrapping_add(1), 0.75 * hip, k);
            }
        }
    }

    if let Some((fired_from, to, age, _)) = b.shot {
        let a = (1.0 - (age / SHOT_FADE).powf(SHOT_FADE_POW)).clamp(0.0, 1.0);
        if a > 0.0
            && let Some(from) = fired_from.or(now)
            && let Some((pa, pb)) = beam(dl, size, from, to, &SHOT, 1.0, glow, a, ads)
        {
            let r = px(to, 1.2, size).unwrap_or(0.0).clamp(80.0 * k, 220.0 * k);
            flare(dl, pb, r * (0.8 + 0.4 * (age / SHOT_FADE).min(1.0)), glow, a);
            // the muzzle's flash and arcs: behind the gun when aimed (the screen flashes then)
            let hip = 1.0 - ads;
            let flash = (1.0 - age / 0.12).max(0.0) * hip;
            if flash > 0.0 {
                flare(dl, pa, 140.0 * k, glow, flash);
            }
            if age < ARCS {
                let fade = 1.0 - age / ARCS;
                arcs(dl, pb, 3, (60.0, 180.0), age, b.shots.wrapping_mul(3), fade, k);
                arcs(dl, pa, 2, (70.0, 200.0), age, b.shots.wrapping_mul(3) + 1, fade * hip, k);
            }
        }
        // then the muzzle: hot, smoking (behind the gun when aimed: the video shows its smoke
        // once the sights are down)
        if let Some(m) = now {
            let away = 1.0 - 0.85 * ads;
            let heat = (1.0 - age / 0.9).max(0.0);
            if heat > 0.0
                && let Some(p) = project(m, size)
            {
                flare(dl, p, 28.0 * k, glow, 0.7 * heat * away);
            }
            if let Some((_, _, right, up)) = view() {
                for i in 0..PUFFS {
                    let t = age - (0.1 + 0.09 * i as f32);
                    if !(0.0..PUFF_LIFE).contains(&t) {
                        continue;
                    }
                    let s = b.shots.wrapping_mul(17).wrapping_add(i as u32);
                    let at = m + up.normalize_or_zero() * (0.03 + 0.2 * t) + right.normalize_or_zero() * ((rnd(s) - 0.5) * 0.08);
                    let Some(p) = project(at, size) else { continue };
                    // the video's puffs: some 15-50 px at 1080p (world sizes this near the eye would
                    // fill the screen)
                    let r = (30.0 + 70.0 * t / PUFF_LIFE) * k;
                    soft(dl, p, r, [0.88, 0.87, 0.85, 0.3 * (1.0 - t / PUFF_LIFE) * (t / 0.08).min(1.0) * away]);
                }
            }
        }
        // on top: the shot's flash (a frame of the video)
        let flash = (1.0 - age / 0.1).max(0.0).powi(2) * (0.5 + 0.5 * ads);
        if flash > 0.0 {
            dl.add_rect([0.0, 0.0], size, [1.0, 0.88, 0.7, 0.3 * flash]).filled(true).build();
            soft(dl, [size[0] * 0.5, size[1] * 0.47], size[0] * 0.6, [1.0, 0.93, 0.8, 0.35 * flash]);
        }
    }
}
