//! `fuse_pov.anim` (tools/apexpov/bake_pov.py, FPOV v1): the ptpov_rspn101 skeleton, the carrier
//! bones and every blend sample of the view model's sequences as clips `<sequence>_<sample>` (T012).
//! Apex's units (inches) and axes. FPOV v2 (T020, tools/apexpov/bake_ability.py) adds a byte to each
//! carrier: its group (0 the arms, 1 the R-301, 2 the stim injector, 3 the jump pad), and has the
//! props' bones, the abilities' clips (`stim_*`, `pad_*`) and more of the R-301's.

use std::sync::Mutex;

use glam::{Quat, Vec3};

use super::graph::{Seq, Weapon};
use crate::{log, paths};

#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub t: Vec3,
    pub r: Quat,
}

impl Xf {
    pub const IDENTITY: Xf = Xf { t: Vec3::ZERO, r: Quat::IDENTITY };

    pub fn mul(self, o: Xf) -> Xf {
        Xf { t: self.t + self.r * o.t, r: (self.r * o.r).normalize() }
    }
    pub fn inverse(self) -> Xf {
        let r = self.r.inverse();
        Xf { t: -(r * self.t), r }
    }
    pub fn lerp(self, o: Xf, a: f32) -> Xf {
        Xf { t: self.t.lerp(o.t, a), r: self.r.slerp(o.r, a).normalize() }
    }
}

pub struct Clip {
    pub name: String,
    pub fps: f32,
    pub frames: usize,
    pub looping: bool,
    pub additive: bool,
    /// per bone: translation, rotation weight (additive)
    pub weights: Vec<(f32, f32)>,
    /// frames x bones
    poses: Vec<Xf>,
}

impl Clip {
    /// Bone `b` at `cycle` (0..1 over the clip, Source's convention: the last frame at 1; looping
    /// clips wrap).
    pub fn sample(&self, b: usize, cycle: f32, nb: usize) -> Xf {
        let c = if self.looping { cycle.rem_euclid(1.0) } else { cycle.clamp(0.0, 1.0) };
        let f = c * (self.frames - 1) as f32;
        let i = (f as usize).min(self.frames - 1);
        let j = (i + 1).min(self.frames - 1);
        self.poses[i * nb + b].lerp(self.poses[j * nb + b], f - i as f32)
    }
}

pub struct Carrier {
    pub bone: String,
    pub owner: usize,
    pub inv_mesh_bind: Xf,
    pub er_bind: Xf,
    /// FPOV v2's group (v1: 0, always shown)
    pub group: u8,
}

pub struct Pack {
    pub names: Vec<String>,
    pub parents: Vec<i16>,
    /// `jx_c_camera`, and its parent `jx_c_pov` (Apex's CAMERA_BASE: the view model's root)
    pub camera: usize,
    pub pov: usize,
    /// `def_c_base`, the gun (the muzzle hangs off it), if the rig has it
    pub gun: Option<usize>,
    /// `weapon_bone` (the hip sway turns about its `SWAY_ROTATE` attachment), if the rig has it
    pub weapon: Option<usize>,
    /// T022: the Charge Rifle's own `def_c_base` copy (its muzzle hangs off it), if the pack has it
    pub cr_gun: Option<usize>,
    /// the Wingman's own `def_c_base` and `weapon_bone` copies, if the pack has it
    pub wm_gun: Option<usize>,
    pub wm_weapon: Option<usize>,
    /// the R-99's own `def_c_base` and `weapon_bone` copies, if the pack has it
    pub r9_gun: Option<usize>,
    pub r9_weapon: Option<usize>,
    /// the Flatline's own `def_c_base` and `weapon_bone` copies, if the pack has it
    pub fl_gun: Option<usize>,
    pub fl_weapon: Option<usize>,
    /// the Sentinel's own `def_c_base` and `weapon_bone` copies, if the pack has it
    pub sn_gun: Option<usize>,
    pub sn_weapon: Option<usize>,
    pub carriers: Vec<Carrier>,
    pub clips: Vec<Clip>,
    /// the clip of each sample of each sequence, by `Seq as usize`: the R-301's, the Charge Rifle's
    /// (T022 `cr_*`; all None without them)
    samples: Vec<[Option<usize>; 4]>,
    cr_samples: Vec<[Option<usize>; 4]>,
    wm_samples: Vec<[Option<usize>; 4]>,
    r9_samples: Vec<[Option<usize>; 4]>,
    fl_samples: Vec<[Option<usize>; 4]>,
    sn_samples: Vec<[Option<usize>; 4]>,
    /// every clip by name (the abilities' clips are found by name)
    by_name: std::collections::HashMap<String, usize>,
}

impl Pack {
    /// Sample `k` of a sequence (clip `<name>_<k>`).
    pub fn sample(&self, seq: Seq, k: usize) -> Option<&Clip> {
        self.samples.get(seq as usize)?.get(k).copied().flatten().map(|i| &self.clips[i])
    }

    /// Sample `k` of a weapon's sequence.
    pub fn sample_in(&self, seq: Seq, k: usize, w: Weapon) -> Option<&Clip> {
        let table = match w {
            Weapon::R301 => &self.samples,
            Weapon::ChargeRifle => &self.cr_samples,
            Weapon::Wingman => &self.wm_samples,
            Weapon::R99 => &self.r9_samples,
            Weapon::Flatline => &self.fl_samples,
            Weapon::Sentinel => &self.sn_samples,
        };
        table.get(seq as usize)?.get(k).copied().flatten().map(|i| &self.clips[i])
    }

    /// Whether the pack has the Charge Rifle (T022: its bones and clips).
    pub fn has_rifle(&self) -> bool {
        self.cr_gun.is_some() && self.cr_samples.iter().all(|k| k[0].is_some())
    }

    /// Whether the pack has the R-99 (its bones and clips).
    pub fn has_r99(&self) -> bool {
        self.r9_gun.is_some() && self.r9_weapon.is_some() && self.r9_samples.iter().all(|k| k[0].is_some())
    }

    /// Whether the pack has the Flatline (its bones and clips).
    pub fn has_flatline(&self) -> bool {
        self.fl_gun.is_some() && self.fl_weapon.is_some() && self.fl_samples.iter().all(|k| k[0].is_some())
    }

    /// Whether the pack has the Sentinel (its bones and clips).
    pub fn has_sentinel(&self) -> bool {
        self.sn_gun.is_some() && self.sn_weapon.is_some() && self.sn_samples.iter().all(|k| k[0].is_some())
    }

    /// Whether the pack has the Wingman (its bones and clips).
    pub fn has_wingman(&self) -> bool {
        self.wm_gun.is_some() && self.wm_weapon.is_some() && self.wm_samples.iter().all(|k| k[0].is_some())
    }

    /// The gun (`def_c_base`) and `weapon_bone` of a weapon's graph: the Wingman's own copies, else
    /// the R-301's (the Charge Rifle's sway pivots are the R-301's, as before).
    pub fn gun_bones(&self, w: Weapon) -> (Option<usize>, Option<usize>) {
        match w {
            Weapon::Wingman if self.wm_gun.is_some() => (self.wm_gun, self.wm_weapon),
            Weapon::R99 if self.r9_gun.is_some() => (self.r9_gun, self.r9_weapon),
            Weapon::Flatline if self.fl_gun.is_some() => (self.fl_gun, self.fl_weapon),
            Weapon::Sentinel if self.sn_gun.is_some() => (self.sn_gun, self.sn_weapon),
            _ => (self.gun, self.weapon),
        }
    }

    pub fn clip(&self, name: &str) -> Option<&Clip> {
        self.by_name.get(name).map(|&i| &self.clips[i])
    }
}

pub fn parse(d: &[u8]) -> Result<Pack, String> {
    let mut p = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let s = d.get(p..p + n).ok_or("truncated")?;
        p += n;
        Ok(s)
    };
    macro_rules! u32le { () => { u32::from_le_bytes(take(4)?.try_into().unwrap()) } }
    macro_rules! f32le { () => { f32::from_le_bytes(take(4)?.try_into().unwrap()) } }
    macro_rules! name { () => {{ let n = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize; String::from_utf8(take(n)?.to_vec()).map_err(|e| e.to_string())? }} }
    macro_rules! xf { () => {{
        let t = Vec3::new(f32le!(), f32le!(), f32le!());
        let r = Quat::from_xyzw(f32le!(), f32le!(), f32le!(), f32le!());
        if !t.is_finite() || !r.is_finite() || (r.length() - 1.0).abs() > 0.01 { return Err("bad transform".into()); }
        Xf { t, r: r.normalize() }
    }} }
    if take(4)? != b"FPOV" {
        return Err("not a fuse_pov.anim".into());
    }
    let version = u32le!();
    if version != 1 && version != 2 {
        return Err(format!("fuse_pov.anim v{version}: only v1 and v2 are known"));
    }
    let nb = u32le!() as usize;
    if nb == 0 || nb > 512 {
        return Err("bad bone count".into());
    }
    let mut names = Vec::with_capacity(nb);
    let mut parents = Vec::with_capacity(nb);
    for b in 0..nb {
        names.push(name!());
        let parent = i16::from_le_bytes(take(2)?.try_into().unwrap());
        if parent >= b as i16 {
            return Err("parents must come first".into());
        }
        parents.push(parent);
        // the rest pose: the absolute clips carry every bone, the additive ones layer on them
        let _ = xf!();
    }
    let camera = u32le!() as usize;
    let nc = u32le!() as usize;
    if nc > 256 {
        return Err(format!("{nc} carriers: at most 256"));
    }
    let mut carriers = Vec::with_capacity(nc);
    for _ in 0..nc {
        let bone = name!();
        let owner = u32le!() as usize;
        let (inv_mesh_bind, er_bind) = (xf!(), xf!());
        let group = if version >= 2 { take(1)?[0] } else { 0 };
        carriers.push(Carrier { bone, owner, inv_mesh_bind, er_bind, group });
    }
    if camera >= nb || carriers.iter().any(|c| c.owner >= nb) {
        return Err("bone index out of range".into());
    }
    let pov = usize::try_from(parents[camera]).map_err(|_| "the camera bone has no parent")?;
    // T022's pack has 301 clips (the R-301's, the abilities', the Charge Rifle's and the grenade's)
    let nclips = u32le!() as usize;
    if nclips > 1024 {
        return Err(format!("{nclips} clips: at most 1024"));
    }
    let mut clips = Vec::new();
    for _ in 0..nclips {
        let name = name!();
        let fps = f32le!();
        let frames = u32le!() as usize;
        let looping = take(1)?[0] != 0;
        let additive = take(1)?[0] != 0;
        if !(fps > 0.0 && fps < 1000.0) || frames == 0 || frames > 100_000 {
            return Err(format!("clip {name}: bad fps/frames"));
        }
        let weights = (0..nb).map(|_| Ok((f32le!(), f32le!()))).collect::<Result<Vec<_>, String>>()?;
        let mut poses = Vec::with_capacity(frames * nb);
        for _ in 0..frames * nb {
            poses.push(xf!());
        }
        clips.push(Clip { name, fps, frames, looping, additive, weights, poses });
    }
    let table = |w: Weapon| -> Vec<[Option<usize>; 4]> {
        Seq::ALL
            .iter()
            .map(|s| {
                let mut k = [None; 4];
                for (i, slot) in k.iter_mut().enumerate().take(s.def_in(w).samples.len()) {
                    let n = format!("{}_{i}", s.def_in(w).name);
                    *slot = clips.iter().position(|c| c.name == n);
                }
                k
            })
            .collect()
    };
    let (samples, cr_samples, wm_samples, r9_samples, fl_samples, sn_samples) = (table(Weapon::R301), table(Weapon::ChargeRifle), table(Weapon::Wingman), table(Weapon::R99), table(Weapon::Flatline), table(Weapon::Sentinel));
    let gun = names.iter().position(|n| n == "def_c_base");
    let weapon = names.iter().position(|n| n == "weapon_bone");
    let cr_gun = names.iter().position(|n| n == "cr:def_c_base");
    let wm_gun = names.iter().position(|n| n == "wm:def_c_base");
    let wm_weapon = names.iter().position(|n| n == "wm:weapon_bone");
    let r9_gun = names.iter().position(|n| n == "r9:def_c_base");
    let r9_weapon = names.iter().position(|n| n == "r9:weapon_bone");
    let fl_gun = names.iter().position(|n| n == "fl:def_c_base");
    let fl_weapon = names.iter().position(|n| n == "fl:weapon_bone");
    let sn_gun = names.iter().position(|n| n == "sn:def_c_base");
    let sn_weapon = names.iter().position(|n| n == "sn:weapon_bone");
    let by_name = clips.iter().enumerate().map(|(i, c)| (c.name.clone(), i)).collect();
    Ok(Pack { names, parents, camera, pov, gun, weapon, cr_gun, wm_gun, wm_weapon, r9_gun, r9_weapon, fl_gun, fl_weapon, sn_gun, sn_weapon, carriers, clips, samples, cr_samples, wm_samples, r9_samples, fl_samples, sn_samples, by_name })
}

static PACK: Mutex<Option<Result<Pack, String>>> = Mutex::new(None);

pub fn with_pack<R>(f: impl FnOnce(&Pack) -> R) -> Option<R> {
    let mut g = PACK.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        let path = paths::file("fuse_pov.anim");
        let r = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|d| parse(&d));
        match &r {
            Ok(p) => {
                // each sample as the graph's table has it (a re-bake that changed them is logged)
                let missing: Vec<String> = Seq::ALL
                    .iter()
                    .flat_map(|s| {
                        s.def().samples.iter().enumerate().filter_map(move |(k, &(frames, fps))| match p.sample(*s, k) {
                            None => Some(format!("{}_{k}", s.def().name)),
                            Some(c) if (c.frames, c.fps, c.additive) != (frames as usize, fps, s.additive()) => Some(format!("{} (frames {} fps {} additive {})", c.name, c.frames, c.fps, c.additive)),
                            Some(_) => None,
                        })
                    })
                    .collect();
                log(format!(
                    "pov: loaded {} ({} bones, camera {} under {}, {} carriers, {} clips{})",
                    path.display(),
                    p.parents.len(),
                    p.names[p.camera],
                    p.names[p.pov],
                    p.carriers.len(),
                    p.clips.len(),
                    if missing.is_empty() { String::new() } else { format!("; not as the graph expects: {}", missing.join(", ")) }
                ))
            }
            Err(e) => log(format!("pov: no view model ({}: {e})", path.display())),
        }
        *g = Some(r);
    }
    g.as_ref().and_then(|r| r.as_ref().ok()).map(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"FPOV").is_err());
        assert!(parse(b"XXXX\x01\0\0\0").is_err());
    }

    #[test]
    fn xf_inverse() {
        let a = Xf { t: Vec3::new(1.0, 2.0, 3.0), r: Quat::from_rotation_y(0.7) };
        let i = a.mul(a.inverse());
        assert!(i.t.length() < 1e-5 && i.r.angle_between(Quat::IDENTITY) < 1e-5);
    }
}

#[cfg(test)]
mod pack_file {
    use super::super::graph::Axes;
    use super::*;

    /// Every clip T012 baked (plan 2.1): the graph's samples and group B.
    const CLIPS: [&str; 39] = [
        "ads_in_0", "ads_in_1", "ads_out_0", "ads_out_1", "idle_0", "idle_1", "crouch_0", "crouch_1",
        "idle_to_crouch_0", "idle_to_crouch_1", "crouch_to_idle_0", "crouch_to_idle_1", "fire_0", "fire_1",
        "fire_2", "fire_3", "jump_0", "jump_1", "jump_2", "jump_3", "land_0", "land_1", "land_2", "land_3",
        "sprint_0", "sprintraise_0", "sprintslide_0", "reload_0", "reload_1", "reload_empty_0",
        "reload_empty_1", "wind_effect_layer_0", "wind_effect_layer_1", "holster_0", "draw_0",
        "drawfirst_0", "raise_0", "lower_0", "inspect_basic_0",
    ];

    /// The baked pack, if it has been generated (apex-data is not in git): T012's clips, and the
    /// graph's sequence table (frames, fps, looping, additive) as baked.
    #[test]
    fn parses_baked_pack() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("apex-data/pov/fuse_pov.anim");
        let Ok(d) = std::fs::read(&path) else { return };
        let p = parse(&d).unwrap();
        assert_eq!(p.parents.len(), 102);
        let bone = |n: &str| p.names.iter().position(|b| b == n);
        assert_eq!(p.camera, bone("jx_c_camera").expect("jx_c_camera"));
        assert_eq!(p.pov, bone("jx_c_pov").expect("jx_c_pov"));
        assert!(p.gun.is_some());
        for name in CLIPS {
            assert!(p.clips.iter().any(|c| c.name == name), "no clip {name}");
        }
        assert_eq!(p.clips.len(), CLIPS.len());
        for s in Seq::ALL {
            let d = s.def();
            let additive = s.additive();
            let samples = match d.axes {
                Axes::One => 1,
                Axes::AdsCrouch => 4,
                _ => 2,
            };
            assert_eq!(d.samples.len(), samples, "{}", d.name);
            for (k, (frames, fps)) in d.samples.iter().enumerate() {
                let c = p.sample(s, k).unwrap_or_else(|| panic!("{}_{k}", d.name));
                assert_eq!((c.frames, c.fps, c.additive), (*frames as usize, *fps, additive), "{}", c.name);
                // only real loops loop (zeroanim and wind_effect don't, the graph wraps the wind)
                assert_eq!(c.looping, d.looping, "{}", c.name);
            }
        }
    }
}
