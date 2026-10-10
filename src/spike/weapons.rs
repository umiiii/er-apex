//! The two primary weapon slots (U3): 1 the Wingman in the R-301's place (gun.rs; the slot keeps
//! its `R301` name in the code), 2 the Charge Rifle (chargerifle.rs,
//! S3 `mp_weapon_defender`). Keys 1 and 2 are S3's `weaponSelectPrimary0` / `weaponSelectPrimary1`
//! (dev `weapon 1|2`).
//!
//! A switch as S3's weapon settings time it (R5Reloaded `platform/scripts/weapons`): the weapon in
//! hand is put away over its `holster_time`, then the other is drawn over its `deploy_time`, or its
//! `deployfirst_time` the first time it is drawn; it can fire (and aim) from its draw's
//! `AE_WPN_READYTOFIRE` frame on, the rule ability.rs uses for the R-301's pull-out after the pad.
//! The frames are the local retail view models' (D-021): R-301 `ptpov_rspn101.qc` draw 12 of 25
//! (26 frames), drawfirst 38 of 44; Charge Rifle `chargerifle_base_v_animRig.qc` draw 12 of 20,
//! drawfirst 36 of 45 (tools/apexpov/export_defender_pov.py; the Casts' frame counts).
//!
//! | | holster_time | deploy_time | deployfirst_time |
//! |---|---|---|---|
//! | R-301 (`_base_assault_rifle.txt`, `mp_weapon_rspn101.txt`) | 0.55 | 0.6 | 1.1 |
//! | Charge Rifle (`mp_weapon_defender.txt`) | 0.5 | 0.8 | 1.5 |
//!
//! The R-301 is out at the start, drawn before: its first draw never plays. The slot the HUD
//! highlights changes when the new weapon starts coming out (推断: S3 tracks the active weapon).
//! Aiming waits for the drawn weapon too (推断, R5R 对照项). Pressing the slot already in hand does
//! nothing; pressing the other one during a switch starts a new switch from the weapon in hand.
//! The S3 engine's own switch code was not read (待定): the order holster -> deploy and the timings
//! are the settings' names taken at their word.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::log;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// the bullet gun's slot (key 1): the R-301, the R-99 or the Wingman (`Gun`, the wheel picks)
    R301 = 0,
    ChargeRifle = 1,
    /// key 3: the holstered mode, Wraith's heirloom kunai in the hand (no gun; a little faster: `move_scale`)
    Melee = 2,
}

impl Slot {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Slot::R301 => primary_gun().name(),
            Slot::ChargeRifle => "Charge Rifle",
            Slot::Melee => "Kunai",
        }
    }
}

/// The bullet guns of slot 1 (gun.rs runs them all; the weapon wheel picks one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gun {
    R301 = 0,
    R99 = 1,
    Wingman = 2,
    /// the VK-47 Flatline (its Teal Zeal model)
    Flatline = 3,
    /// the Sentinel (automatic, a self-filling magazine, homing shots: gun.rs `SENTINEL`)
    Sentinel = 4,
}

impl Gun {
    pub const ALL: [Gun; 5] = [Gun::R301, Gun::R99, Gun::Wingman, Gun::Flatline, Gun::Sentinel];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Gun::R301 => "R-301",
            Gun::R99 => "R-99",
            Gun::Wingman => "Wingman",
            Gun::Flatline => "Flatline",
            Gun::Sentinel => "Sentinel",
        }
    }

    /// Its switch timings (S3/retail weapon settings and the view models' QCs):
    /// R-301 `holster_time` 0.55, `deploy_time` 0.6, `deployfirst_time` 1.1, draw ready 12 of 25,
    /// drawfirst 38 of 44; R-99 (`mp_weapon_r97.txt`) 0.3, 0.35, 1.0, draw ready 6 of 12 (its
    /// drawfirst has a scaled prop the pack cannot hold: the draw stands in, 推断); Wingman
    /// (`mp_weapon_wingman.txt`) 0.36, 0.4, 1.45, draw ready 14 of 20, drawfirst 41 of 48.
    pub fn timing(self) -> Timing {
        match self {
            Gun::R301 => Timing { holster: 0.55, deploy: 0.6, deploy_first: 1.1, ready: 12.0 / 25.0, ready_first: 38.0 / 44.0 },
            Gun::R99 => Timing { holster: 0.3, deploy: 0.35, deploy_first: 0.35, ready: 6.0 / 12.0, ready_first: 6.0 / 12.0 },
            Gun::Wingman => R301_TIMING,
            // `mp_weapon_vinson.txt`: 0.55, 0.6, 1.25; its view model's draw ready 10 of 21, drawfirst 25 of 30
            Gun::Flatline => Timing { holster: 0.55, deploy: 0.6, deploy_first: 1.25, ready: 10.0 / 21.0, ready_first: 25.0 / 30.0 },
            // `mp_weapon_sentinel.txt`: 0.7, 1.0, 1.6; its view model's draw ready 15 of 36, drawfirst 42 of 50
            Gun::Sentinel => Timing { holster: 0.7, deploy: 1.0, deploy_first: 1.6, ready: 15.0 / 36.0, ready_first: 42.0 / 50.0 },
        }
    }
}

/// A weapon's switch timings (seconds) and its draws' ready-to-fire points (fractions of the draw).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timing {
    pub holster: f32,
    pub deploy: f32,
    pub deploy_first: f32,
    pub ready: f32,
    pub ready_first: f32,
}

/// The Wingman (`mp_weapon_wingman.txt`: `holster_time` 0.36, `deploy_time` 0.4,
/// `deployfirst_time` 1.45; retail `wingman_base_v_animRig.qc`: draw's `AE_WPN_READYTOFIRE` 14 of 20
/// (21 frames), drawfirst's 41 of 48).
pub const R301_TIMING: Timing = Timing { holster: 0.36, deploy: 0.4, deploy_first: 1.45, ready: 14.0 / 20.0, ready_first: 41.0 / 48.0 };
/// The sustained-discharge Charge Rifle (`mp_weapon_defender_sustained.txt`): `holster_time` 0.5,
/// `deploy_time` 0.8; `deployfirst_time` not set (the engine's 0): every draw the normal one (推断);
/// ready at `draw`'s AE_WPN_READYTOFIRE (retail QC frame 12 of 20).
pub const CHARGE_RIFLE_TIMING: Timing = Timing { holster: 0.5, deploy: 0.8, deploy_first: 0.8, ready: 12.0 / 20.0, ready_first: 12.0 / 20.0 };

/// A slot's weapon's timings (slot 1: the bullet gun in it now).
pub fn timing(slot: Slot) -> Timing {
    timing_of(slot, primary_gun())
}

/// The kunai (retail `heirloom_wraith_v18_kunai_v_animRig.qc`): `holster` 13 frames at 33 fps,
/// `draw` 17 at 30 (its `drawfirst` is left out of the pack: the draw stands in); ready for a swing
/// near the draw's end (推断).
pub const KUNAI_TIMING: Timing = Timing { holster: 12.0 / 33.0, deploy: 16.0 / 30.0, deploy_first: 16.0 / 30.0, ready: 0.75, ready_first: 0.75 };

fn timing_of(slot: Slot, gun: Gun) -> Timing {
    match slot {
        Slot::R301 => gun.timing(),
        Slot::ChargeRifle => CHARGE_RIFLE_TIMING,
        Slot::Melee => KUNAI_TIMING,
    }
}
/// Where a switch is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// in hand and ready to fire (also between the ready frame and the draw's end: `Drawing`'s
    /// `ready` is true then)
    Ready(Slot),
    /// being put away: the cycle of its holster (0..1)
    Holstering { slot: Slot, cycle: f32 },
    /// coming out: the cycle of its draw (0..1), whether it is the first draw, whether it can fire
    Drawing { slot: Slot, cycle: f32, first: bool, ready: bool },
}

#[derive(Clone, Copy, Debug)]
struct Switch {
    from: Slot,
    to: Slot,
    /// seconds since it started; the holster of `from` then the draw of `to`
    t: f32,
    holster: f32,
    first: bool,
    /// slot 1's gun going away and coming out (a change of gun in slot 1 is a switch from it to it)
    from_gun: Gun,
    to_gun: Gun,
}

/// The slots and the switch between them (pure: offline tests).
#[derive(Clone, Copy, Debug)]
pub struct Loadout {
    /// the weapon in hand or coming out (the one being put away until its holster ends)
    active: Slot,
    /// slot 1's gun (during a change of gun: the new one once the old one is away)
    primary: Gun,
    switch: Option<Switch>,
    /// whether each weapon has been drawn before (`deployfirst_time` only the first time): the
    /// five guns, the Charge Rifle, the kunai
    drawn: [bool; 7],
}

impl Default for Loadout {
    fn default() -> Self {
        Loadout::new(Gun::Wingman)
    }
}

impl Loadout {
    pub const fn new(primary: Gun) -> Loadout {
        let mut drawn = [false; 7];
        drawn[primary as usize] = true;
        Loadout { active: Slot::R301, primary, switch: None, drawn }
    }

    pub fn active(&self) -> Slot {
        self.active
    }

    /// Slot 1's gun as the hands have it: the one going away during its holster, then the one coming.
    pub fn shown_gun(&self) -> Gun {
        match self.switch {
            Some(s) if s.t < s.holster && s.from == Slot::R301 => s.from_gun,
            Some(s) if s.to == Slot::R301 => s.to_gun,
            _ => self.primary,
        }
    }

    fn drawn_index(slot: Slot, gun: Gun) -> usize {
        match slot {
            Slot::R301 => gun.index(),
            Slot::ChargeRifle => 5,
            Slot::Melee => 6,
        }
    }

    /// The slot the switch goes to (the active one when there is none).
    pub fn target(&self) -> Slot {
        self.switch.map_or(self.active, |s| s.to)
    }

    /// Slot 1's gun the switch goes to.
    fn target_gun(&self) -> Gun {
        self.switch.filter(|s| s.to == Slot::R301).map_or(self.primary, |s| s.to_gun)
    }

    /// Asks for a slot (a key, the dev channel). Whether a switch started.
    pub fn request(&mut self, slot: Slot) -> bool {
        let gun = self.target_gun();
        self.request_with(slot, gun)
    }

    /// Asks for a weapon by the wheel: a gun goes into slot 1 (put away and drawn there if slot 1 is
    /// in hand), the Charge Rifle is slot 2. Whether a switch started.
    pub fn request_gun(&mut self, gun: Option<Gun>) -> bool {
        match gun {
            None => self.request(Slot::ChargeRifle),
            Some(g) => self.request_with(Slot::R301, g),
        }
    }

    fn request_with(&mut self, slot: Slot, gun: Gun) -> bool {
        if slot == self.target() && (slot != Slot::R301 || gun == self.target_gun()) {
            return false;
        }
        // the weapon in hand: the one being put away while its holster lasts, else the active one
        let (from, from_gun) = match self.switch {
            Some(s) if s.t < s.holster => (s.from, s.from_gun),
            Some(s) if s.to == Slot::R301 => (self.active, s.to_gun),
            _ => (self.active, self.primary),
        };
        if from == slot && (slot != Slot::R301 || from_gun == gun) {
            // changed his mind during the put-away: it comes out again (a draw, not a first one)
            self.active = slot;
            self.primary = from_gun;
            self.switch = Some(Switch { from: slot, to: slot, t: 0.0, holster: 0.0, first: false, from_gun, to_gun: from_gun });
            return true;
        }
        self.active = from;
        if from == Slot::R301 {
            self.primary = from_gun;
        }
        let first = !self.drawn[Self::drawn_index(slot, gun)];
        self.switch = Some(Switch { from, to: slot, t: 0.0, holster: timing_of(from, from_gun).holster, first, from_gun, to_gun: gun });
        true
    }

    pub fn step(&mut self, dt: f32) {
        let Some(mut s) = self.switch else { return };
        s.t += dt.max(0.0);
        if s.t >= s.holster {
            self.active = s.to;
            if s.to == Slot::R301 {
                self.primary = s.to_gun;
            }
            self.drawn[Self::drawn_index(s.to, s.to_gun)] = true;
        }
        let t = timing_of(s.to, s.to_gun);
        let deploy = if s.first { t.deploy_first } else { t.deploy };
        self.switch = (s.t < s.holster + deploy).then_some(s);
    }

    pub fn phase(&self) -> Phase {
        let Some(s) = self.switch else { return Phase::Ready(self.active) };
        if s.t < s.holster {
            return Phase::Holstering { slot: s.from, cycle: (s.t / s.holster.max(1e-6)).clamp(0.0, 1.0) };
        }
        let t = timing_of(s.to, s.to_gun);
        let (deploy, ready) = if s.first { (t.deploy_first, t.ready_first) } else { (t.deploy, t.ready) };
        let cycle = ((s.t - s.holster) / deploy.max(1e-6)).clamp(0.0, 1.0);
        Phase::Drawing { slot: s.to, cycle, first: s.first, ready: cycle >= ready }
    }

    /// Whether the active weapon can fire (and aim).
    pub fn ready(&self) -> bool {
        match self.phase() {
            Phase::Ready(_) => true,
            Phase::Drawing { ready, .. } => ready,
            Phase::Holstering { .. } => false,
        }
    }

    /// The weapon in hand comes out again (after an offhand that put it away: the shield battery),
    /// over its draw (not a first one).
    pub fn redraw(&mut self) {
        let slot = self.target();
        let gun = self.target_gun();
        self.active = slot;
        self.primary = gun;
        self.switch = Some(Switch { from: slot, to: slot, t: 0.0, holster: 0.0, first: false, from_gun: gun, to_gun: gun });
    }

    fn slot_name(&self, slot: Slot, gun: Gun) -> &'static str {
        match slot {
            Slot::R301 => gun.name(),
            Slot::ChargeRifle => "Charge Rifle",
            Slot::Melee => "Kunai",
        }
    }

    pub fn describe(&self) -> String {
        let s = self.switch;
        match self.phase() {
            Phase::Ready(slot) => format!("{} ready", self.slot_name(slot, self.primary)),
            Phase::Holstering { slot, cycle } => format!(
                "{} holstering {:.0} %, then {}",
                self.slot_name(slot, s.map_or(self.primary, |s| s.from_gun)),
                cycle * 100.0,
                self.slot_name(self.target(), self.target_gun())
            ),
            Phase::Drawing { slot, cycle, first, ready } => {
                format!("{} {} {:.0} %{}", self.slot_name(slot, self.shown_gun()), if first { "first draw" } else { "draw" }, cycle * 100.0, if ready { ", ready" } else { "" })
            }
        }
    }
}
static LOADOUT: Mutex<Loadout> = Mutex::new(Loadout::new(Gun::Wingman));

fn loadout() -> std::sync::MutexGuard<'static, Loadout> {
    LOADOUT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn active() -> Slot {
    loadout().active()
}

/// Slot 1's gun as the hands have it now (the old one while it is put away).
pub fn primary_gun() -> Gun {
    loadout().shown_gun()
}

/// The weapon wheel's choice (hud): a gun into slot 1, or the Charge Rifle (None).
pub fn select_gun(gun: Option<Gun>, by: &str) -> String {
    super::battery::cancel(by);
    let started = loadout().request_gun(gun);
    let name = gun.map_or("Charge Rifle", |g| g.name());
    if !started {
        return format!("weapon ({by}): {name} already {}", describe());
    }
    log(format!("weapons ({by}): switching to {name}: {}", describe()));
    super::gun::holster_check();
    format!("weapon ({by}): {}", describe())
}

pub fn phase() -> Phase {
    loadout().phase()
}

/// Whether the active weapon can fire and aim (not mid-switch).
pub fn ready() -> bool {
    loadout().ready()
}

/// A slot asked for: by a key or the dev channel. What happened.
pub fn select(slot: Slot, by: &str) -> String {
    // the shield battery's use ends (D-032): S3's `AttemptCancelHeal` takes weaponSelectPrimary0/1
    super::battery::cancel(by);
    let started = loadout().request(slot);
    if !started {
        return format!("weapon ({by}): {} already {}", slot.name(), describe());
    }
    log(format!("weapons ({by}): switching to {}: {}", slot.name(), describe()));
    // the R-301 put away: its reload is off (the rifle's own update does the same for it)
    super::gun::holster_check();
    format!("weapon ({by}): {}", describe())
}

pub fn describe() -> String {
    loadout().describe()
}

/// The shield battery or the grenade is over (battery.rs, grenade.rs; `by` names it for the log):
/// the Charge Rifle, if it is the weapon in hand, comes out again (its draw: no shots before its
/// ready frame). The R-301's pull-out is their own (pov/ability.rs, pov/ordnance.rs).
pub fn redraw_after_offhand(by: &str) {
    let mut l = loadout();
    if l.target() != Slot::R301 {
        let name = l.target().name();
        l.redraw();
        drop(l);
        log(format!("weapons: {name} out again after {by}: {}", describe()));
    }
}

/// Keys 1 and 2 last frame (their presses are edges).
static KEYS_WERE: Mutex<[bool; 6]> = Mutex::new([false; 6]);
/// The sounds of the phase changes already played (`Phase` index of the last frame).
static LAST_PHASE: Mutex<Option<Phase>> = Mutex::new(None);
static ANNOUNCED: AtomicBool = AtomicBool::new(false);

/// Keys 1 and 2 (Apex's PC defaults: `weaponSelectPrimary0/1`), only while the game window has the
/// focus (as input.rs `ability_keys`), 5 the inspect, Tab the weapon wheel, 3 and 4 its last two
/// choices while it is open. kbd.rs hides them from the game (not 3 and 4: 4 is the battery's).
fn number_keys() -> Option<[bool; 6]> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    if pid != unsafe { GetCurrentProcessId() } {
        return None;
    }
    let down = |vk: u8| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    Some([down(b'1'), down(b'2'), down(b'5'), down(0x09), down(b'3'), down(b'4')])
}

/// A choice of the weapon wheel: a gun into slot 1, the Charge Rifle (slot 2), or Q's ability (the
/// stim or Pathfinder's grapple: grapple.rs; the user's 2026-10-09 ask).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Gun(Option<Gun>),
    Q(super::grapple::QAbility),
}

/// The weapon wheel's choices, clockwise from the top.
pub const WHEEL: [(Pick, &str); 8] = [
    (Pick::Gun(Some(Gun::R301)), "R-301"),
    (Pick::Gun(Some(Gun::R99)), "R-99"),
    (Pick::Gun(Some(Gun::Wingman)), "Wingman"),
    (Pick::Gun(Some(Gun::Flatline)), "Flatline"),
    (Pick::Gun(Some(Gun::Sentinel)), "Sentinel"),
    (Pick::Gun(None), "Charge Rifle"),
    (Pick::Q(super::grapple::QAbility::Stim), "Stim"),
    (Pick::Q(super::grapple::QAbility::Grapple), "Grapple"),
];
/// How far the view must turn (degrees) to point at a choice of the wheel.
const WHEEL_DEADZONE: f32 = 2.0;

/// The weapon wheel while Tab is held: the view's axes when it opened, the choice pointed at.
#[derive(Clone, Copy, Debug)]
pub struct Wheel {
    axes: (glam::Vec3, glam::Vec3, glam::Vec3),
    pub hovered: Option<usize>,
}

static WHEEL_STATE: Mutex<Option<Wheel>> = Mutex::new(None);

/// The open wheel, for the HUD.
pub fn wheel() -> Option<Wheel> {
    *WHEEL_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The view's axes (forward, right, up) from the camera the player sees.
fn view_axes() -> Option<(glam::Vec3, glam::Vec3, glam::Vec3)> {
    let (_, f, r, u) = crate::camera::view()?;
    Some((f.normalize_or_zero(), r.normalize_or_zero(), u.normalize_or_zero()))
}

/// The wheel: opened by Tab, the choice by turning the view towards it (up, right, down, left) or by
/// 1-3, made when Tab is let go.
fn wheel_step(tab: bool, pick: Option<usize>) {
    let mut w = WHEEL_STATE.lock().unwrap_or_else(|e| e.into_inner());
    match (tab, w.as_mut()) {
        (true, None) => {
            if let Some(axes) = view_axes() {
                *w = Some(Wheel { axes, hovered: None });
            }
        }
        (true, Some(o)) => {
            if let Some(k) = pick {
                o.hovered = Some(k);
            } else if let Some((f, _, _)) = view_axes() {
                let (f0, r0, u0) = o.axes;
                let d = f - f0;
                let (x, y) = (d.dot(r0).to_degrees(), d.dot(u0).to_degrees());
                if x.hypot(y) > WHEEL_DEADZONE {
                    // clockwise from the top, a sector each
                    let n = WHEEL.len();
                    let sector = 360.0 / n as f32;
                    let a = x.atan2(y).to_degrees().rem_euclid(360.0);
                    o.hovered = Some((((a + sector / 2.0) / sector) as usize) % n);
                }
            }
        }
        (false, Some(o)) => {
            let chosen = o.hovered;
            *w = None;
            drop(w);
            if let Some(k) = chosen {
                log(match WHEEL[k].0 {
                    Pick::Gun(g) => select_gun(g, "wheel"),
                    Pick::Q(a) => super::grapple::select(a, "wheel"),
                });
            }
        }
        (false, None) => {}
    }
}
/// What the gun does this frame (gun.rs `update` reads it and calls `update`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Trigger {
    pub fire: bool,
    pub aim: bool,
    /// the reload key, pressed (an edge)
    pub reload: bool,
}

/// Once a frame from gun.rs `update` (in the world, the gun on): the keys, the switch, the Charge
/// Rifle. Whether the R-301 sits this frame out (put away, coming out, or the Charge Rifle out).
pub fn update(dt: f32, t: Trigger) -> bool {
    if !ANNOUNCED.swap(true, Ordering::Relaxed) {
        log("weapons: slot 1 R-301 / R-99 / Wingman / Flatline / Sentinel (the wheel: Tab), slot 2 Charge Rifle (keys 1 / 2, dev `weapon 1|2|r301|r99|wingman|flatline|sentinel`)");
    }
    if crate::fe::in_play_view() {
        let keys = number_keys().unwrap_or([false; 6]);
        let pressed = {
            let mut was = KEYS_WERE.lock().unwrap_or_else(|e| e.into_inner());
            let p: [bool; 6] = std::array::from_fn(|i| keys[i] && !was[i]);
            *was = keys;
            p
        };
        // the wheel open: 1-4 pick its choices instead of the slots
        let wheel_open = keys[3] || wheel().is_some();
        let pick = [0, 1, 4].iter().position(|&i| pressed[i]).filter(|_| wheel_open);
        wheel_step(keys[3], pick);
        if wheel_open {
        } else if pressed[0] {
            log(select(Slot::R301, "key 1"));
        } else if pressed[1] {
            log(select(Slot::ChargeRifle, "key 2"));
        } else if pressed[4] {
            log(toggle_melee("key 3"));
        } else if pressed[2] && ready() {
            log(super::pov::start_inspect());
        }
    }
    let (phase, ready, active, gun) = {
        let mut l = loadout();
        l.step(dt);
        (l.phase(), l.ready(), l.active(), l.shown_gun())
    };
    phase_sounds(phase, gun);
    // the rifle in hand: drawn or coming out (its put-away already ends a charge or a reload, as
    // gun.rs `holster_check` does the R-301's)
    // (nor while the shield battery has the hands: battery.rs; after it the rifle comes out again,
    // `redraw`)
    let rifle_in_hand = active == Slot::ChargeRifle
        && !matches!(phase, Phase::Holstering { .. })
        && !super::battery::busy()
        && !super::grenade::gun_away();
    super::chargerifle::update(dt, t, rifle_in_hand, rifle_in_hand && ready);
    melee_update(t.fire, active == Slot::Melee && ready && !super::battery::busy() && !super::grenade::gun_away());
    active != Slot::R301 || !ready
}

/// The slot to go back to when key 3 is pressed again in the holstered mode.
static BEFORE_MELEE: Mutex<Slot> = Mutex::new(Slot::R301);

/// Key 3: the holstered mode (the kunai) on, or back to the weapon before it.
pub fn toggle_melee(by: &str) -> String {
    let target = loadout().target();
    if target == Slot::Melee {
        let back = *BEFORE_MELEE.lock().unwrap_or_else(|e| e.into_inner());
        select(back, by)
    } else {
        *BEFORE_MELEE.lock().unwrap_or_else(|e| e.into_inner()) = target;
        select(Slot::Melee, by)
    }
}

/// The holstered mode's run (ini `holster_speed`, default 1.1): the kunai in hand or coming out.
pub fn move_scale() -> f32 {
    let on = match phase() {
        Phase::Ready(s) | Phase::Drawing { slot: s, .. } => s == Slot::Melee,
        Phase::Holstering { .. } => false,
    };
    if on { crate::paths::number::<f32>("holster_speed").map_or(1.1, |m| m.clamp(0.5, 2.0)) } else { 1.0 }
}

/// Apex's melee: 30 damage out to 2 m (`melee_damage`, `melee_range` 推断 from the retail feel); one
/// swing each `SWING_EVERY` seconds, its `melee_idle_swipe` (26 frames at 30 fps) in the hands.
const MELEE_DAMAGE: f32 = 30.0;
const MELEE_RANGE: f32 = 2.0;
const SWING_EVERY: f32 = 0.6;
/// The last swing (pov/mod.rs plays it) and the trigger last frame.
static SWING: Mutex<(Option<std::time::Instant>, bool)> = Mutex::new((None, false));

fn melee_update(fire: bool, can: bool) {
    let mut s = SWING.lock().unwrap_or_else(|e| e.into_inner());
    let pulled = fire && !s.1;
    s.1 = fire;
    if !(pulled && can) || s.0.is_some_and(|t| t.elapsed().as_secs_f32() < SWING_EVERY) {
        return;
    }
    s.0 = Some(std::time::Instant::now());
    drop(s);
    // the swing's sounds (`melee_idle_swipe` frames 0 and 2; stand-ins: export_audio.py --set kunai)
    crate::audio::play("karambit_mvmt_melee_idle_swipe_1p", R301_VOLUME);
    crate::audio::play_in("octane_effort_melee_1p", R301_VOLUME, 2.0 / 30.0);
    // (the kill feed's icon: 3 the kunai: hud/apex.rs kill_feed)
    let hit = super::gun::fire_ray_ex(0.0, (0.0, 0.0), glam::Vec3::ZERO, 3, MELEE_RANGE, true, |_, _| MELEE_DAMAGE * super::gun::damage_mult());
    if let Some(line) = hit.and_then(|o| o.line) {
        crate::audio::play("generic_kunaiimpact_1p_vs_3p", R301_VOLUME);
        log(format!("melee: {line}"));
    }
}

/// Seconds since the last kunai swing (pov/mod.rs: its `melee_idle_swipe`).
pub fn swing_age() -> Option<f32> {
    SWING.lock().unwrap_or_else(|e| e.into_inner()).0.map(|t| t.elapsed().as_secs_f32())
}

/// Slot 1's guns' put-away and pull-out sounds (their view models' QCs: `holster` frame 0
/// `*_UnEquip`, `draw` frame 0 `*_Equip`), each event's play actions together: the R-301's
/// (`ptpov_rspn101.qc`, `--set defender`), the R-99's (`r99_base_v_animRig.qc`, `--set r99`), the
/// Wingman's (`wingman_base_v_animRig.qc`, `--set wingman`).
fn gun_sounds(gun: Gun) -> (&'static [&'static str], &'static [&'static str]) {
    match gun {
        Gun::R301 => (&["weapon_r101_unequip", "weapon_r101_unequip_layer1"], &["weapon_r101_equip", "weapon_r101_equip_layer1", "weapon_r101_equip_layer2"]),
        Gun::R99 => (&["weapon_r97_unequip"], &["weapon_r97_equip"]),
        // the Flatline's QC: the R-301's (`Weapon_R101_UnEquip` / `_Equip`, `--set flatline`)
        Gun::Flatline => (&["weapon_r101_unequip", "weapon_r101_unequip_layer1"], &["weapon_r101_equip", "weapon_r101_equip_layer1", "weapon_r101_equip_layer2"]),
        Gun::Sentinel => (&["weapon_sentinel_holster"], &["weapon_sentinel_draw"]),
        Gun::Wingman => (
            &["weapon_wingman_unequip", "weapon_wingman_unequip_layer1", "weapon_wingman_unequip_layer2"],
            &["weapon_wingman_equip", "weapon_wingman_equip_layer1", "weapon_wingman_equip_layer2"],
        ),
    }
}
const R301_VOLUME: f32 = 0.5;

/// The holster and draw sounds as the phases begin (the view models' QCs: `holster` frame 0
/// `*_UnEquip`, `draw` frame 0 `*_Equip`, the Charge Rifle's `drawfirst` frame 1
/// `weapon_chargerifle_firstdraw_1p`).
fn phase_sounds(now: Phase, gun: Gun) {
    let mut last = LAST_PHASE.lock().unwrap_or_else(|e| e.into_inner());
    let began = |p: Phase| match (p, *last) {
        (Phase::Holstering { slot, .. }, Some(Phase::Holstering { slot: s, .. })) => slot != s,
        (Phase::Holstering { .. }, _) => true,
        (Phase::Drawing { slot, .. }, Some(Phase::Drawing { slot: s, .. })) => slot != s,
        (Phase::Drawing { .. }, _) => true,
        _ => false,
    };
    if began(now) {
        let (unequip, equip) = gun_sounds(gun);
        match now {
            Phase::Holstering { slot: Slot::ChargeRifle, .. } => super::chargerifle::sound_holster(),
            Phase::Drawing { slot: Slot::ChargeRifle, first, .. } => super::chargerifle::sound_draw(first),
            Phase::Holstering { slot: Slot::R301, .. } => unequip.iter().for_each(|n| crate::audio::play(n, R301_VOLUME)),
            // the kunai (`--set kunai`): its draw's QC sound, and the grip turned back for its put-away
            // (its holster has none: a stand-in)
            Phase::Holstering { slot: Slot::Melee, .. } => crate::audio::play("wraith_mvmt_kunai_grip_standard2reverse", R301_VOLUME),
            Phase::Drawing { slot: Slot::Melee, .. } => crate::audio::play("wraith_mvmt_kunai_grip_reverse2standard", R301_VOLUME),
            Phase::Drawing { slot: Slot::R301, .. } => equip.iter().for_each(|n| crate::audio::play(n, R301_VOLUME)),
            _ => {}
        }
    }
    *last = Some(now);
}
/// The R-301 in the view model during and after a switch (pov/mod.rs): its put-away (`holster`, 16
/// frames, over the R-301's holster time), then held at its end (the arms lowered out of view) while
/// the Charge Rifle is out, its pull-out (`draw`, 26 frames, by crouch) over its deploy time; and
/// whether the R-301 shows (group 1). None: nothing to add (the R-301 in hand and ready).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct R301View {
    /// the R-301 clip (`holster` or `draw`), its cycle
    pub clip: &'static str,
    pub cycle: f32,
    pub shown: bool,
}

pub fn r301_view_of(p: Phase) -> Option<R301View> {
    match p {
        Phase::Ready(Slot::R301) => None,
        Phase::Drawing { slot: Slot::R301, cycle, .. } => Some(R301View { clip: "draw", cycle, shown: true }),
        Phase::Holstering { slot: Slot::R301, cycle } => Some(R301View { clip: "holster", cycle, shown: cycle < 1.0 }),
        // the Charge Rifle in hand: no view model of it yet (U3 stage 2), the R-301's arms kept down
        _ => Some(R301View { clip: "holster", cycle: 1.0, shown: false }),
    }
}

/// Zoom times (in, out: seconds) and the part of the zoom over which the field of view changes
/// (`ads_fov_zoomfrac_start` / `_end`) of the weapon in hand: R-301 0.27 / 0.23 (S3
/// `_base_assault_rifle.txt`), R-99 0.22 / 0.2 (`mp_weapon_r97.txt`), Wingman 0.18 / 0.16, 0 .. 1
/// (the defaults); Charge Rifle 0.2 / 0.15, 0.25 .. 0.75 (`mp_weapon_defender.txt`).
pub fn zoom() -> (f32, f32, f32, f32) {
    match active() {
        Slot::R301 => match primary_gun() {
            Gun::R301 => (0.27, 0.23, 0.0, 1.0),
            Gun::R99 => (0.22, 0.2, 0.0, 1.0),
            Gun::Wingman => (0.18, 0.16, 0.0, 1.0),
            Gun::Flatline => (0.27, 0.23, 0.0, 1.0),
            // `mp_weapon_sentinel.txt`: 0.31 / 0.28, the view model's offset over 0.2 .. 0.8
            Gun::Sentinel => (0.31, 0.28, 0.2, 0.8),
        },
        Slot::ChargeRifle => (super::chargerifle::ZOOM_IN, super::chargerifle::ZOOM_OUT, super::chargerifle::ADS_FOV_FROM, super::chargerifle::ADS_FOV_TO),
        // the kunai does not aim (`aiming`): the zoom only goes out
        Slot::Melee => (0.2, 0.2, 0.0, 1.0),
    }
}

/// The weapon in hand's `zoom_fov` (4:3 horizontal degrees, camera.rs): R-301 55, R-99 and Wingman
/// 60, Charge Rifle 55.
pub fn zoom_fov() -> f32 {
    match active() {
        Slot::R301 if matches!(primary_gun(), Gun::R301 | Gun::Flatline | Gun::Sentinel) => 55.0,
        Slot::R301 => 60.0,
        Slot::ChargeRifle => 55.0,
        Slot::Melee => 70.0,
    }
}

/// The weapon in hand's view kick spring (`viewkick_spring`, springs.txt: stiffness and damping,
/// pitch / yaw / roll, hip then aimed) and `viewkick_*_weaponFraction` (hip, aimed): slot 1's the
/// Wingman's `wingman`, 0.4 / 0.3 (only it kicks the view: gun.rs); the Charge Rifle's `titan_arc`,
/// 0.5 / 0.6 (chargerifle.rs).
pub fn kick_spring() -> ([glam::Vec3; 4], (f32, f32)) {
    use glam::Vec3;
    match active() {
        Slot::R301 | Slot::Melee => ([Vec3::new(120.0, 60.0, 150.0), Vec3::new(30.0, 30.0, 30.0), Vec3::new(100.0, 55.0, 150.0), Vec3::new(25.0, 25.0, 20.0)], (0.4, 0.3)),
        Slot::ChargeRifle => {
            use super::chargerifle::*;
            ([SPRING_K_HIP, SPRING_C_HIP, SPRING_K_ADS, SPRING_C_ADS], (WEAPON_FRACTION_HIP, WEAPON_FRACTION_ADS))
        }
    }
}
/// Whether the aim button counts this frame: held, and the weapon in hand drawn (推断).
pub fn aiming(held: bool) -> bool {
    held && ready() && active() != Slot::Melee
}

/// What the HUD draws for the weapon in hand (gun.rs / chargerifle.rs state).
pub fn hud() -> Option<super::gun::HudState> {
    match active() {
        // the holstered mode keeps the HUD as it was (the user's 2026-10-09 ask): slot 1's gun
        Slot::Melee if *BEFORE_MELEE.lock().unwrap_or_else(|e| e.into_inner()) == Slot::ChargeRifle => super::chargerifle::hud(),
        Slot::R301 | Slot::Melee => super::gun::hud(),
        Slot::ChargeRifle => super::chargerifle::hud(),
    }
}

/// Dev channel `weapon [1|2]`.
pub fn dev(args: &[&str]) -> String {
    match args.first().copied() {
        Some("1") => select(Slot::R301, "dev"),
        Some("2") => select(Slot::ChargeRifle, "dev"),
        Some("3" | "kunai") => toggle_melee("dev"),
        Some("r301") => select_gun(Some(Gun::R301), "dev"),
        Some("r99") => select_gun(Some(Gun::R99), "dev"),
        Some("wingman") => select_gun(Some(Gun::Wingman), "dev"),
        Some("flatline") => select_gun(Some(Gun::Flatline), "dev"),
        Some("sentinel") => select_gun(Some(Gun::Sentinel), "dev"),
        None => format!("weapon: {} | {}", describe(), super::chargerifle::status()),
        _ => "usage: weapon [1|2|3|r301|r99|wingman|flatline|sentinel]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(l: &mut Loadout, seconds: f32) {
        let mut t = 0.0;
        while t < seconds - 1e-6 {
            l.step(DT);
            t += DT;
        }
    }

    /// Wingman -> Charge Rifle: the Wingman put away over 0.36 s (no shots), the rifle's first draw
    /// (no `deployfirst_time` for the sustained rifle: a plain one) over 0.8 s, ready from 12/20 of it.
    #[test]
    fn switch_to_the_charge_rifle_and_back() {
        let mut l = Loadout::default();
        assert_eq!(l.phase(), Phase::Ready(Slot::R301));
        assert!(l.request(Slot::ChargeRifle));
        assert!(!l.request(Slot::ChargeRifle), "same slot again: nothing");
        run(&mut l, 0.3);
        assert!(matches!(l.phase(), Phase::Holstering { slot: Slot::R301, .. }) && !l.ready() && l.active() == Slot::R301);
        run(&mut l, 0.1);
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::ChargeRifle, first: true, ready: false, .. }), "{:?}", l.phase());
        assert_eq!(l.active(), Slot::ChargeRifle);
        // ready at 0.36 + 0.8 x 12/20 = 0.84 s
        run(&mut l, 0.84 - 0.4 - 0.05);
        assert!(!l.ready(), "{:?}", l.phase());
        run(&mut l, 0.1);
        assert!(l.ready(), "{:?}", l.phase());
        run(&mut l, 0.5);
        assert_eq!(l.phase(), Phase::Ready(Slot::ChargeRifle));
        // back: the rifle away over 0.5 s, the Wingman drawn (not a first draw) over 0.4 s, ready at 14/20
        assert!(l.request(Slot::R301));
        run(&mut l, 0.45);
        assert!(matches!(l.phase(), Phase::Holstering { slot: Slot::ChargeRifle, .. }));
        run(&mut l, 0.1);
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::R301, first: false, .. }));
        run(&mut l, 0.4 * 14.0 / 20.0 - 0.05 - 0.02);
        assert!(!l.ready());
        run(&mut l, 0.05);
        assert!(l.ready());
        // the rifle again: its plain draw now, ready at 0.36 + 0.8 x 12/20 = 0.84 s
        run(&mut l, 1.0);
        assert!(l.request(Slot::ChargeRifle));
        run(&mut l, 0.8);
        assert!(!l.ready() && matches!(l.phase(), Phase::Drawing { first: false, .. }));
        run(&mut l, 0.06);
        assert!(l.ready());
    }

    /// Key 3: the Wingman put away (0.36 s), the kunai drawn (0.53 s), then the Wingman back with its
    /// plain draw; slot 1's gun stays the Wingman throughout.
    #[test]
    fn holstered_mode_and_back() {
        let mut l = Loadout::default();
        assert!(l.request(Slot::Melee));
        assert!(!l.request(Slot::Melee), "the kunai again: nothing");
        run(&mut l, 0.3);
        assert!(matches!(l.phase(), Phase::Holstering { slot: Slot::R301, .. }));
        run(&mut l, 0.1);
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::Melee, .. }), "{:?}", l.phase());
        run(&mut l, 0.6);
        assert_eq!(l.phase(), Phase::Ready(Slot::Melee));
        assert_eq!(l.shown_gun(), Gun::Wingman);
        assert!(l.request(Slot::R301));
        run(&mut l, 0.4);
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::R301, first: false, .. }), "{:?}", l.phase());
        run(&mut l, 0.5);
        assert_eq!(l.phase(), Phase::Ready(Slot::R301));
    }

    /// The wheel: another gun into slot 1 while it is in hand: the Wingman put away (0.36 s), the R-99
    /// drawn (its first draw, 0.35 s); the Charge Rifle from the wheel is slot 2.
    #[test]
    fn wheel_changes_the_gun_in_slot_1() {
        let mut l = Loadout::default();
        assert!(l.request_gun(Some(Gun::R99)));
        assert!(!l.request_gun(Some(Gun::R99)), "same gun again: nothing");
        run(&mut l, 0.3);
        assert!(matches!(l.phase(), Phase::Holstering { slot: Slot::R301, .. }) && l.shown_gun() == Gun::Wingman);
        run(&mut l, 0.1);
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::R301, first: true, .. }) && l.shown_gun() == Gun::R99, "{:?}", l.phase());
        run(&mut l, 0.4);
        assert_eq!(l.phase(), Phase::Ready(Slot::R301));
        assert_eq!(l.shown_gun(), Gun::R99);
        assert!(l.request_gun(None));
        run(&mut l, 2.0);
        assert_eq!(l.phase(), Phase::Ready(Slot::ChargeRifle));
        // from the rifle to the R-301: slot 1 comes out with it
        assert!(l.request_gun(Some(Gun::R301)));
        run(&mut l, 0.6);
        assert_eq!(l.shown_gun(), Gun::R301);
        run(&mut l, 2.0);
        assert_eq!(l.phase(), Phase::Ready(Slot::R301));
    }

    /// Back to the R-301 during its own put-away: it comes out again at once (a plain draw).
    #[test]
    fn change_of_mind_during_the_holster() {
        let mut l = Loadout::default();
        l.request(Slot::ChargeRifle);
        run(&mut l, 0.2);
        assert!(l.request(Slot::R301));
        assert!(matches!(l.phase(), Phase::Drawing { slot: Slot::R301, first: false, ready: false, .. }), "{:?}", l.phase());
        run(&mut l, 0.6);
        assert_eq!(l.phase(), Phase::Ready(Slot::R301));
        // the rifle was never drawn: its next draw is still the first
        l.request(Slot::ChargeRifle);
        run(&mut l, 0.6);
        assert!(matches!(l.phase(), Phase::Drawing { first: true, .. }));
    }

    /// The view model: the R-301's put-away shown to its end, then hidden with the arms held at it;
    /// its pull-out shown from the start.
    #[test]
    fn r301_view_through_a_switch() {
        assert_eq!(r301_view_of(Phase::Ready(Slot::R301)), None);
        let v = r301_view_of(Phase::Holstering { slot: Slot::R301, cycle: 0.5 }).unwrap();
        assert!(v.shown && v.clip == "holster" && v.cycle == 0.5);
        let v = r301_view_of(Phase::Drawing { slot: Slot::ChargeRifle, cycle: 0.2, first: true, ready: false }).unwrap();
        assert!(!v.shown && v.clip == "holster" && v.cycle == 1.0);
        let v = r301_view_of(Phase::Ready(Slot::ChargeRifle)).unwrap();
        assert!(!v.shown);
        let v = r301_view_of(Phase::Holstering { slot: Slot::ChargeRifle, cycle: 0.5 }).unwrap();
        assert!(!v.shown && v.cycle == 1.0);
        let v = r301_view_of(Phase::Drawing { slot: Slot::R301, cycle: 0.3, first: false, ready: false }).unwrap();
        assert!(v.shown && v.clip == "draw" && v.cycle == 0.3);
    }
}
