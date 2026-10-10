//! Minimal combat HUD (M0, D-014): crosshair sized to the spread, hit marker, magazine and reload,
//! Fuse's HP. A 2D overlay on the game's final image through hudhook's DirectX 12 hook, as in
//! er-mario's hud.rs (MIT, Copyright (c) 2026 Delta). The Apex-style HUD proper is M3.

use eldenring::cs::{CSCamera, WorldChrMan};
use fromsoftware_shared::FromStatic;
use glam::Vec3;
use hudhook::imgui;
use hudhook::{ImguiRenderLoop, RenderContext};

use crate::{camera, log, spike};

mod apex;
mod font;
mod beam;
mod grenade;
mod pack;
mod tex;

/// Seconds a damage number stays up, and how far it rises (px at 1080p).
const NUMBER_SECONDS: f32 = 0.9;
const NUMBER_RISE: f32 = 40.0;
/// Seconds the health bar stays over the last enemy hit.
const TARGET_BAR_SECONDS: f32 = 3.0;

/// The field of view the last projection used (render thread), for `cam`.
pub static LAST_FOV: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::paths::flag("hud_debug"))
}

/// The camera of the image on screen: position, forward, right, up, and the vertical field of view.
/// The one that image was drawn with (camera.rs `drawn_view`); else ours over the shoulder or the
/// game's render camera as they stand now.
fn drawn() -> Option<((Vec3, Vec3, Vec3, Vec3), f32)> {
    camera::drawn_view().or_else(|| {
        let cam = unsafe { CSCamera::instance() }.ok()?;
        let m = &cam.pers_cam_1.matrix;
        let view = camera::view().unwrap_or((
            Vec3::new(m.3.0, m.3.1, m.3.2),
            Vec3::new(m.2.0, m.2.1, m.2.2),
            Vec3::new(m.0.0, m.0.1, m.0.2),
            Vec3::new(m.1.0, m.1.1, m.1.2),
        ));
        Some((view, cam.pers_cam_1.fov))
    })
}

/// The camera the player sees: position, forward, right, up (`drawn`).
fn view() -> Option<(Vec3, Vec3, Vec3, Vec3)> {
    drawn().map(|d| d.0)
}

/// World position -> screen pixel through the camera the player sees (`drawn`), as er-mario's
/// hud.rs `project`; None behind the camera.
fn project(p: Vec3, size: [f32; 2]) -> Option<[f32; 2]> {
    let ((pos, fwd, right, up), fov) = drawn()?;
    let d = p - pos;
    let z = d.dot(fwd.normalize_or_zero());
    if z < 0.1 {
        return None;
    }
    LAST_FOV.store(fov.to_bits(), std::sync::atomic::Ordering::Relaxed);
    let t = (fov * 0.5).tan();
    let x = d.dot(right.normalize_or_zero()) / (z * t * size[0] / size[1]);
    let y = d.dot(up.normalize_or_zero()) / (z * t);
    Some([size[0] * 0.5 * (1.0 + x), size[1] * 0.5 * (1.0 - y)])
}

/// Where the shots go on screen when the weapon kick's part the view does not show turns them
/// (viewfx `weapon_aim_offset`, the Charge Rifle's shots: chargerifle.rs): the camera's forward
/// turned by it, 100 m out. None without a kick (the screen's centre).
fn kicked_aim(size: [f32; 2]) -> Option<[f32; 2]> {
    let offset = crate::viewfx::weapon_aim_offset();
    if offset == Vec3::ZERO {
        return None;
    }
    let (pos, fwd, right, up) = view()?;
    let [_, _, f] = crate::viewfx::turned(right, up, fwd, offset);
    project(pos + f * 100.0, size)
}

/// Calibration (ini `hud_debug = 1`): every enemy's feet, `hit_height` and `chr_hit_height`, Fuse at
/// 0 / 0.9 / 1.8 m and the lock point, projected like the HUD.
fn debug_overlay(dl: &imgui::DrawListMut, size: [f32; 2]) {
    if debug() {
        if let Ok(wcm) = unsafe { WorldChrMan::instance() } {
            for c in wcm.chr_sets.iter().flatten().flat_map(|s| s.characters()) {
                let c: &eldenring::cs::ChrIns = c;
                if !crate::spike::body::is_enemy_team(c.team_type) {
                    continue;
                }
                let ph = &c.modules.physics;
                let base = Vec3::new(ph.position.0, ph.position.1, ph.position.2);
                for (h, col) in [(0.0, [0.2, 1.0, 0.2, 1.0]), (ph.hit_height, [0.2, 0.6, 1.0, 1.0]), (ph.chr_hit_height, [1.0, 0.3, 1.0, 1.0])] {
                    if let Some(at) = project(base + Vec3::Y * h, size) {
                        dl.add_rect([at[0] - 8.0, at[1] - 1.5], [at[0] + 8.0, at[1] + 1.5], col).filled(true).build();
                    }
                }
            }
            // Fuse: feet, hips, head height (white)
            if let Some(p) = wcm.main_player.as_ref() {
                let q = p.chr_ins.modules.physics.position;
                for h in [0.0, 0.9, 1.8] {
                    if let Some(at) = project(Vec3::new(q.0, q.1 + h, q.2), size) {
                        dl.add_rect([at[0] - 12.0, at[1] - 1.5], [at[0] + 12.0, at[1] + 1.5], [1.0, 1.0, 1.0, 1.0]).filled(true).build();
                    }
                }
            }
            // first person: where the gun's sight centre was put (cyan) and 30 cm along the barrel
            if let Some((s, f)) = crate::firstperson::last_sight() {
                if let (Some(a), Some(b)) = (project(s, size), project(s + f * 0.3, size)) {
                    dl.add_line(a, b, [0.2, 1.0, 1.0, 1.0]).thickness(2.0).build();
                    dl.add_circle(a, 5.0, [0.2, 1.0, 1.0, 1.0]).thickness(2.0).build();
                }
            }
            // Apex's view model: its bones (left arm blue, right arm red, gun yellow, rest orange)
            let names = crate::spike::pov::carrier_names().unwrap_or_default();
            for (i, b) in crate::firstperson::view_model_bones().into_iter().enumerate() {
                let n = names.get(i).map_or("", |s| s.as_str());
                let col = if n.starts_with("L_") && !n.contains("Pectoral") {
                    [0.2, 0.5, 1.0, 1.0]
                } else if n.starts_with("R_") && !n.contains("Pectoral") {
                    [1.0, 0.2, 0.2, 1.0]
                } else if n.contains("Pectoral") || n == "Collar" {
                    [1.0, 1.0, 0.1, 1.0]
                } else {
                    [1.0, 0.6, 0.1, 1.0]
                };
                if let Some(a) = project(b, size) {
                    dl.add_circle(a, 5.0, col).filled(true).build();
                }
            }
            if let Some(p) = wcm.main_player.as_ref().filter(|p| p.chr_ins.is_locked_on) {
                let l = p.chr_ins.lock_on_target_position;
                if let Some(at) = project(Vec3::new(l.0, l.1, l.2), size) {
                    dl.add_circle(at, 6.0, [1.0, 1.0, 0.2, 1.0]).thickness(2.0).build();
                }
            }
        }
    }
}

/// Octane's launch pad when its model is not drawn (D-029; R4 draws it in first person with model
/// 998's pad, firstperson.rs): two rings on its surface, the outer one its trigger's radius; a dot
/// while it flies.
fn octane_overlay(dl: &imgui::DrawListMut, size: [f32; 2]) {
    if crate::firstperson::world_pad_drawn() {
        return;
    }
    let Some((at, up, planted)) = spike::octane::pad() else { return };
    let amber = [1.0, 0.72, 0.1, 0.9];
    if !planted {
        if let Some(p) = project(at, size) {
            dl.add_circle(p, 6.0, amber).filled(true).build();
        }
        return;
    }
    let up = up.normalize_or(Vec3::Y);
    let u1 = up.cross(if up.x.abs() < 0.9 { Vec3::X } else { Vec3::Z }).normalize();
    let u2 = up.cross(u1);
    for (r, t) in [(spike::octane::pad_radius(), 3.0), (spike::octane::pad_radius() * 0.45, 2.0)] {
        let pts: Option<Vec<[f32; 2]>> = (0..=32)
            .map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                project(at + up * 0.03 + (u1 * a.cos() + u2 * a.sin()) * r, size)
            })
            .collect();
        if let Some(pts) = pts {
            dl.add_polyline(pts, amber).thickness(t).build();
        }
    }
}

/// Octane's stim on the screen (S3 sh_stim.gnut): `ScreenFlash(2, 3.5, 2, 0.15, 0.3)` as it starts,
/// `ScreenFlash(2, 2, 2, 0.2, 0.2)` as its visual effect ends (6 s), and in between the cockpit
/// effect P_pilot_stim_hld_FP, stronger with speed (0..360 units/s to its control point 5..200).
/// ScreenFlash scales the screen's colour by (r, g, b) for `hold` seconds, then fades over `fade`;
/// the particle effect is not ours. Both are drawn as overlays here: an approximation (近似),
/// matched to the user's R5R video (2026-10-05 16:53): the start washes the view yellow-white
/// (the sky 124,89,71 -> 213,194,136 at 24.57 s, back by 24.95 s), the end whitens it, and the
/// edges in between turn only faintly and patchily green (the side bands' green minus the mean of
/// red and blue up from about -5 to -1..+5), so the edge glow is thin and weak.
fn stim_overlay(dl: &imgui::DrawListMut, size: [f32; 2]) {
    let Some(t) = spike::octane::stim_age() else { return };
    let visual = spike::octane::STIM_VISUAL_SECONDS;
    let flash = |t: f32, hold: f32, fade: f32| {
        if t < 0.0 {
            0.0
        } else if t < hold {
            1.0
        } else {
            (1.0 - (t - hold) / fade).max(0.0)
        }
    };
    let start = flash(t, 0.15, 0.3);
    if start > 0.0 {
        dl.add_rect([0.0, 0.0], size, [0.85, 1.0, 0.6, 0.45 * start]).filled(true).build();
    }
    let end = flash(t - visual, 0.2, 0.2);
    if end > 0.0 {
        dl.add_rect([0.0, 0.0], size, [1.0, 1.0, 1.0, 0.32 * end]).filled(true).build();
    }
    if t < visual {
        let speed = spike::kcc::locomotion().map_or(0.0, |l| l.velocity.length() * spike::kcc::UNITS_PER_METRE);
        let edge = [0.55, 1.0, 0.45, 0.02 + 0.06 * (speed / 360.0).clamp(0.0, 1.0)];
        let clear = [0.55, 1.0, 0.45, 0.0];
        let ([w, h], b) = (size, size[1] * 0.08);
        dl.add_rect_filled_multicolor([0.0, 0.0], [w, b], edge, edge, clear, clear);
        dl.add_rect_filled_multicolor([0.0, h - b], [w, h], clear, clear, edge, edge);
        dl.add_rect_filled_multicolor([0.0, 0.0], [b, h], edge, clear, clear, edge);
        dl.add_rect_filled_multicolor([w - b, 0.0], [w, h], clear, edge, edge, clear);
    }
}

/// The Apex HUD pack (T009), if there is one: then the Apex HUD replaces the minimal one.
static APEX: std::sync::OnceLock<Option<pack::Pack>> = std::sync::OnceLock::new();

/// Reads the pack and starts decoding its images and fonts (threads of their own).
fn load_pack() {
    APEX.get_or_init(|| {
        let dir = pack::dir();
        let language = crate::paths::config("hud_language").filter(|l| !l.is_empty()).unwrap_or_else(|| "schinese".into());
        match pack::load(&dir, &language) {
            Ok(p) => {
                log(format!("hud: Apex HUD pack from {} ({language}): {} images, weapon \"{}\"", dir.display(), p.images.len(), p.weapon_name));
                let mut images: Vec<(String, std::path::PathBuf, tex::Kind)> = p.images.iter().map(|(n, path)| (n.clone(), path.clone(), tex::Kind::Color)).collect();
                let upgrades = apex::upgrade_icons(&p).map(|u| (u, tex::Kind::Color));
                for &(name, kind) in apex::IMAGES.iter().chain(upgrades.iter()) {
                    match p.images_by_path.get(name) {
                        Some(path) => images.push((name.to_string(), path.clone(), kind)),
                        None => log(format!("hud: {name} not in the pack (re-export with tools/apexhud)")),
                    }
                }
                // the streak badges (apex.rs `streak_badge`): the user's own pictures, in `rank` in the
                // pack's folder or beside it (apex-data/hud/rank: a re-export leaves them be)
                for name in apex::RANK_BADGES {
                    let file = format!("{name}.png");
                    let inside = dir.join("rank").join(&file);
                    let path = if inside.is_file() { inside } else { dir.parent().map_or(inside, |d| d.join("rank").join(&file)) };
                    if path.is_file() {
                        images.push((name.to_string(), path, tex::Kind::Color));
                    } else {
                        log(format!("hud: no {} (the streak badge)", path.display()));
                    }
                }
                tex::load(images);
                font::load(&p.font.atlas, &p.font.meta, vec![(font::Face::Body, p.font.body), (font::Face::Numeric, p.font.numeric), (font::Face::Bold, p.font.bold)], custom_fonts(&dir));
                Some(p)
            }
            Err(e) => {
                log(format!("hud: no Apex HUD pack in {} ({e}); minimal HUD", dir.display()));
                None
            }
        }
    });
}

/// Your own fonts for the digits and English letters (tools/apexhud/custom_font.py: an atlas in ini
/// `hud_font_dir`, default `custom_font` beside the pack's folder): each face's font is ini
/// `hud_font_body` / `hud_font_numeric` / `hud_font_bold`, else `hud_font`, else "apex regular" (or
/// the first font); a face set to `off` keeps Apex's, and `hud_font = off` turns them all off.
fn custom_fonts(pack_dir: &std::path::Path) -> Option<font::Custom> {
    let ini = |k: &str| crate::paths::config(k).map(|v| v.trim().to_lowercase()).filter(|v| !v.is_empty());
    let all = ini("hud_font");
    if all.as_deref() == Some("off") {
        return None;
    }
    let dir = crate::paths::config("hud_font_dir").map(std::path::PathBuf::from).unwrap_or_else(|| pack_dir.parent().unwrap_or(pack_dir).join("custom_font"));
    let (atlas, meta) = (dir.join("atlas.png"), dir.join("meta.json"));
    let text = std::fs::read_to_string(&meta).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    let names: Vec<String> = doc["fonts"].as_array()?.iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
    let default = all.or_else(|| names.iter().find(|n| *n == "apex regular").or(names.first()).cloned())?;
    let faces: Vec<(font::Face, String)> = [(font::Face::Body, "hud_font_body"), (font::Face::Numeric, "hud_font_numeric"), (font::Face::Bold, "hud_font_bold")]
        .into_iter()
        .map(|(face, key)| (face, ini(key).unwrap_or_else(|| default.clone())))
        .filter(|(_, n)| n != "off")
        .collect();
    Some(font::Custom { atlas, meta, faces })
}

struct Overlay;

/// Starts the overlay (hooks the game's DirectX 12 presentation).
pub fn install(module: usize) {
    load_pack();
    use hudhook::hooks::dx12::ImguiDx12Hooks;
    let hmodule = hudhook::windows::Win32::Foundation::HINSTANCE(module as _);
    match hudhook::Hudhook::builder().with::<ImguiDx12Hooks>(Overlay).with_hmodule(hmodule).build().apply() {
        Ok(()) => log("hud: overlay hooked"),
        Err(e) => log(format!("hud: overlay hook failed: {e:?}")),
    }
}

impl ImguiRenderLoop for Overlay {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, _render_context: &'a mut dyn RenderContext) {
        ctx.io_mut().mouse_draw_cursor = false;
        ctx.set_ini_filename(None);
    }

    fn before_render<'a>(&'a mut self, _ctx: &mut imgui::Context, render_context: &'a mut dyn RenderContext) {
        tex::upload(render_context);
        font::upload(render_context);
    }

    fn render(&mut self, ui: &mut imgui::Ui) {
        if let Some(Some(pack)) = APEX.get() {
            // not over a loading screen, the map or a menu
            if crate::state::playable() && crate::fe::in_play_view() && crate::mode::apex() {
                let size = ui.io().display_size;
                let dl = ui.get_foreground_draw_list();
                let fov = drawn().map_or(0.838, |d| d.1);
                stim_overlay(&dl, size);
                beam::draw(&dl, size);
                apex::draw(&dl, pack, size, fov);
                debug_overlay(&dl, size);
                octane_overlay(&dl, size);
                grenade::draw(&dl, size);
            }
            return;
        }
        // the weapon in hand (U3: spike/weapons.rs)
        let Some(gun) = spike::weapons::hud() else { return };
        let size = ui.io().display_size;
        let s = (size[1] / 1080.0).max(0.5);
        let (cx, cy) = (size[0] * 0.5, size[1] * 0.5);
        let dl = ui.get_foreground_draw_list();
        beam::draw(&dl, size);
        let white = [1.0, 1.0, 1.0, 0.9];
        let shadow = [0.0, 0.0, 0.0, 0.6];

        // crosshair: four ticks at the spread cone's radius on screen (a dot when aiming)
        let fov = drawn().map_or(0.8, |d| d.1);
        let gap = if gun.aiming {
            0.0
        } else {
            (gun.spread_deg.to_radians().tan() / (fov * 0.5).tan() * size[1] * 0.5).max(4.0 * s)
        };
        if gun.aiming {
            dl.add_circle([cx, cy], 2.5 * s, shadow).filled(true).build();
            dl.add_circle([cx, cy], 1.8 * s, white).filled(true).build();
        } else {
            let len = 8.0 * s;
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let a = [cx + dx * gap, cy + dy * gap];
                let b = [cx + dx * (gap + len), cy + dy * (gap + len)];
                dl.add_line(a, b, shadow).thickness(3.0 * s).build();
                dl.add_line(a, b, white).thickness(1.5 * s).build();
            }
        }

        // hit marker: an X for 0.15 s (red on headshots)
        if let Some((at, head)) = gun.last_hit {
            let t = at.elapsed().as_secs_f32();
            if t < 0.15 {
                let alpha = 1.0 - t / 0.15;
                let col = if head { [1.0, 0.25, 0.2, alpha] } else { [1.0, 1.0, 1.0, alpha] };
                let (i, o) = (6.0 * s, 13.0 * s);
                for (dx, dy) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    dl.add_line([cx + dx * i, cy + dy * i], [cx + dx * o, cy + dy * o], col).thickness(2.0 * s).build();
                }
            }
        }

        debug_overlay(&dl, size);
        octane_overlay(&dl, size);
        grenade::draw(&dl, size);

        // the last enemy hit: its health over its head for a few seconds
        let hits = spike::gun::hits();
        if let Some(last) = hits.last().filter(|h| h.at.elapsed().as_secs_f32() < TARGET_BAR_SECONDS) {
            let wcm = unsafe { WorldChrMan::instance() }.ok();
            if let Some(c) = wcm.and_then(|w| w.chr_ins_by_handle(&last.target)) {
                let ph = &c.modules.physics;
                let h = if ph.chr_hit_height.is_finite() && ph.chr_hit_height > 0.2 { ph.chr_hit_height } else { 1.8 };
                let top = Vec3::new(ph.position.0, ph.position.1 + h + 0.15, ph.position.2);
                let (hp, max) = (c.modules.data.hp.max(0) as f32, c.modules.data.max_hp.max(1) as f32);
                if let Some(at) = project(top, size) {
                    let (w, bh) = (90.0 * s, 6.0 * s);
                    let (x0, y0) = (at[0] - w * 0.5, at[1] - bh);
                    dl.add_rect([x0 - 1.0, y0 - 1.0], [x0 + w + 1.0, y0 + bh + 1.0], shadow).filled(true).build();
                    dl.add_rect([x0, y0], [x0 + w * (hp / max).clamp(0.0, 1.0), y0 + bh], [0.92, 0.25, 0.2, 0.95]).filled(true).build();
                }
            }
        }

        if let Some(p) = gun.reload {
            let (w, h, y) = (120.0 * s, 4.0 * s, cy + 40.0 * s);
            dl.add_rect([cx - w * 0.5, y], [cx + w * 0.5, y + h], shadow).filled(true).build();
            dl.add_rect([cx - w * 0.5, y], [cx - w * 0.5 + w * p, y + h], white).filled(true).build();
        }
        let fuse_hp = spike::lethal::fuse_hp();
        if let Some(hp) = fuse_hp {
            let (x, y, w, h) = (60.0 * s, size[1] - 150.0 * s, 260.0 * s, 10.0 * s);
            let frac = (hp / spike::lethal::FUSE_MAX_HP).clamp(0.0, 1.0);
            dl.add_rect([x, y], [x + w, y + h], shadow).filled(true).build();
            dl.add_rect([x, y], [x + w * frac, y + h], [0.9, 0.9, 0.9, 0.95]).filled(true).build();
        }
        // text in an invisible full-screen window (text outside any window makes imgui open its
        // own "Debug" window)
        ui.window("##fuse_hud")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(size, imgui::Condition::Always)
            .bg_alpha(0.0)
            .no_decoration()
            .no_inputs()
            .build(|| {
                ui.set_window_font_scale(1.8 * s);
                let text = |pos: [f32; 2], col: [f32; 4], t: &str| {
                    ui.set_cursor_screen_pos([pos[0] + 2.0, pos[1] + 2.0]);
                    ui.text_colored(shadow, t);
                    ui.set_cursor_screen_pos(pos);
                    ui.text_colored(col, t);
                };
                let ammo = format!("{} / {}", gun.ammo, gun.clip);
                let x = size[0] - 300.0 * s;
                text([x, size[1] - 175.0 * s], if gun.ammo == 0 { [1.0, 0.3, 0.3, 1.0] } else { white }, &ammo);
                ui.set_window_font_scale(1.2 * s);
                text([x, size[1] - 140.0 * s], [0.85, 0.85, 0.85, 0.9], if gun.slot == 1 { "CHARGE RIFLE" } else { "WINGMAN" });
                if let Some(hp) = fuse_hp {
                    text([60.0 * s, size[1] - 180.0 * s], white, &format!("FUSE {hp:.0}"));
                }
                // damage numbers at the hit, rising and fading
                ui.set_window_font_scale(1.5 * s);
                for h in &hits {
                    let age = h.at.elapsed().as_secs_f32();
                    if age > NUMBER_SECONDS {
                        continue;
                    }
                    if let Some(at) = project(h.pos, size) {
                        let a = 1.0 - (age / NUMBER_SECONDS).powi(2);
                        let col = if h.head { [1.0, 0.85, 0.2, a] } else { [1.0, 1.0, 1.0, a] };
                        text([at[0] + 12.0 * s, at[1] - 10.0 * s - NUMBER_RISE * s * age / NUMBER_SECONDS], col, &format!("{:.0}", h.damage));
                    }
                }
            });
    }
}
