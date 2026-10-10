//! ER-Apex: play Elden Ring as an Apex Legends legend, now Octane (D-027; docs/plan/plan-octane.md;
//! the M0 work was done as ER-Fuse, fuse-mod-plan.md).
//!
//! M0 skeleton: loads through me3, checks the game version, follows the game state and runs the
//! feasibility spikes. Infrastructure adapted from er-mario (MIT, Copyright (c) 2026 Delta).

mod audio;
mod camera;
mod fe;
mod fps;
mod dev;
mod explore;
mod firstperson;
mod havok_col;
mod hud;
mod input;
mod cursor;
mod kbd;
mod log;
mod mode;
mod paths;
mod scan;
mod spike;
mod state;
mod version;
mod viewfx;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use eldenring::cs::{CSTaskGroupIndex, CSTaskImp};
use eldenring::fd4::FD4TaskData;
use fromsoftware_shared::SharedTaskImpExt;

pub(crate) use log::{dlog, log};

/// This DLL's module handle.
static MODULE: AtomicUsize = AtomicUsize::new(0);

/// The main per-frame task (ChrIns_PostPhysics, like er-mario's frame).
fn frame(data: &FD4TaskData) {
    let dt = data.delta_time.time;
    state::update(dt);
    // F5: the Tarnished or Octane (mode.rs)
    mode::update();
    spike::quickboot::update();
    spike::lethal::update();
    spike::gun::update(dt);
    // the Sentinel's homing rounds in flight (homing.rs)
    spike::homing::update(dt);
    spike::stats::update();
    spike::octane::update(dt);
    spike::battery::update();
    spike::grenade::update(dt);
    spike::kcc::update(dt);
    firstperson::update(dt);
    spike::combat::update();
    spike::armor::auto_update();
    spike::armor::update();
    dev::poll(dt);
}

/// Wraps a task so a Rust panic in it is logged instead of unwinding into the game. This only
/// covers unwinding panics, not crashes from bad pointers or aborts (audit #9).
fn guarded(name: &'static str, f: fn(&FD4TaskData)) -> impl Fn(&FD4TaskData) + Send + 'static {
    move |d: &FD4TaskData| {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(d))).is_err() {
            static REPORTED: AtomicUsize = AtomicUsize::new(0);
            if REPORTED.fetch_add(1, Ordering::Relaxed) < 20 {
                log(format!("task {name} panicked (see PANIC above)"));
            }
        }
    }
}

fn boot() {
    log(format!("er-apex {} loaded from {}", env!("CARGO_PKG_VERSION"), paths::mod_dir().display()));
    std::panic::set_hook(Box::new(|info| log(format!("PANIC: {info}"))));
    let cs_task = match CSTaskImp::wait_for_instance(Duration::MAX) {
        Ok(t) => t,
        Err(e) => {
            log(format!("CSTaskImp never appeared ({e:?}); ER-Apex stays off"));
            return;
        }
    };
    if let Err(e) = version::check() {
        log(format!("this game version is not supported ({e}); ER-Apex stays off"));
        return;
    }
    log(format!("game version ok; dev channel {}", if dev::enabled() { "on" } else { "off" }));
    // ini fps_limit: the game's frame cap
    fps::install();
    unsafe { input::install() };
    // the mouse is not held in the window while the game is in the background
    unsafe { cursor::install() };
    if spike::kcc::enabled() {
        // the controller's keys hidden from the game while it moves Fuse
        unsafe { kbd::install() };
    }
    if spike::gun::enabled() {
        hud::install(MODULE.load(Ordering::Relaxed));
        // gunfire, reload and ADS sounds (T007); silent if no sounds are exported
        audio::start();
    }
    cs_task.run_recurring(guarded("frame", frame), CSTaskGroupIndex::ChrIns_PostPhysics);
    // right after the game turned the pad / mouse into character actions (er-mario's input_task)
    cs_task.run_recurring(guarded("gun_input", |_| spike::gun::input_task()), CSTaskGroupIndex::ChrIns_PreBehaviorSafe);
    cs_task.run_recurring(guarded("kcc_input", |_| spike::kcc::input_task()), CSTaskGroupIndex::ChrIns_PreBehaviorSafe);
    // S4 pose override: the game re-animates the skeleton at several points of the frame, so
    // re-apply after each (er-mario's list); a no-op unless `pose <clip>` is playing
    for group in [
        CSTaskGroupIndex::ChrIns_PrePhysics_End,
        CSTaskGroupIndex::ChrIns_RagdollSafe,
        CSTaskGroupIndex::LocationUpdate_PostCloth,
        CSTaskGroupIndex::ChrIns_PreCloth,
        CSTaskGroupIndex::ChrIns_PreClothSafe,
        CSTaskGroupIndex::HavokClothUpdate_Pre_ClothModelInsSafe,
        CSTaskGroupIndex::ChrIns_PostPhysics,
        CSTaskGroupIndex::GameFlowStep_Post,
        CSTaskGroupIndex::ChrIns_PrePhysicsSafe,
        CSTaskGroupIndex::LocationUpdate_PrePhysics,
        CSTaskGroupIndex::LocationUpdate_PrePhysics_Post,
        CSTaskGroupIndex::LocationUpdate_PostCloth_Post,
        CSTaskGroupIndex::HavokWorldUpdate_Post,
        CSTaskGroupIndex::ChrIns_PostPhysicsSafe,
        CSTaskGroupIndex::WorldChrMan_PostPhysics,
        CSTaskGroupIndex::Draw_Pre,
    ] {
        cs_task.run_recurring(guarded("pose", |d| spike::pose::apply_in_group(d.task_group_id)), group);
    }
    // over-the-shoulder camera (camera_shoulder = 1): at every step from the game's camera update
    // to drawing (er-mario's lakitu list), since the game copies its own camera back in between
    for group in [
        CSTaskGroupIndex::CameraStep,
        CSTaskGroupIndex::DrawParamUpdate,
        CSTaskGroupIndex::ChrIns_PostPhysicsSafe,
        CSTaskGroupIndex::CSDistViewManager_Update,
        CSTaskGroupIndex::WorldChrMan_PostPhysics,
        CSTaskGroupIndex::GameFlowStep_Post,
        CSTaskGroupIndex::Draw_Pre,
    ] {
        cs_task.run_recurring(guarded("camera", |_| camera::update()), group);
    }
    // the camera each frame goes to drawing with, for the HUD (drawn when a frame is presented)
    cs_task.run_recurring(guarded("camera_drawn", |_| camera::snapshot_drawn()), CSTaskGroupIndex::Draw_Pre);
    // the game's own HUD: hide what the Apex-style HUD replaces (hud_hide), after the game copies
    // it (MenuMan) and before Scaleform draws it
    cs_task.run_recurring(guarded("fe", |_| fe::before_menu()), CSTaskGroupIndex::WorldChrMan_PostPhysics);
    cs_task.run_recurring(guarded("fe", |_| fe::after_menu()), CSTaskGroupIndex::GameFlowStep_Post);
    log("tasks registered");
}

#[unsafe(no_mangle)]
/// # Safety
/// Called by the Windows loader only.
pub unsafe extern "C" fn DllMain(hmodule: usize, reason: u32) -> bool {
    if reason == 1 {
        MODULE.store(hmodule, Ordering::Relaxed);
        std::thread::spawn(boot);
    }
    true
}
