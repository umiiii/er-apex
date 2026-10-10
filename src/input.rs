//! Controller input: the game's XInputGetState goes through us (its import table entry is
//! swapped, as in er-mario). We see the real pad and can add a synthetic one on top, which drives
//! menus in automated runs (dev `pad` command, quick boot) without the window needing the focus
//! (with the dev profile's Game.Debug.IsEnableControlOnDisactiveWindow).
//!
//! The import-table patch is adapted from er-mario (MIT, Copyright (c) 2026 Delta).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::XboxController::{XINPUT_GAMEPAD_BUTTON_FLAGS, XINPUT_STATE};

use crate::log;

type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// The real XInputGetState (or whatever was in the import table before: Steam's overlay hook).
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
const ERROR_SUCCESS: u32 = 0;
const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

fn keyboard_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::paths::flag("keyboard_only"))
}

/// A synthetic pad held until `until`: buttons are added to the real pad's, non-zero sticks and
/// triggers replace the real ones.
#[derive(Clone, Copy, Default, Debug)]
pub struct Synthetic {
    pub buttons: u16,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
    pub lt: u8,
    pub rt: u8,
}

static SYNTH: Mutex<Option<(Synthetic, Instant)>> = Mutex::new(None);
/// Report a connected (idle) pad on player 0 even when none is plugged in. Without a pad the game
/// only probes the XInput slots every few seconds; once one answers it reads it every frame, so
/// synthetic presses need this switched on a few seconds before them.
pub static VIRTUAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Packet numbers for the synthetic pad (XInput callers compare them to see changes).
static PACKET: AtomicU32 = AtomicU32::new(1);
/// Diagnostics: calls of the hook, the last return code of the real XInputGetState, the last
/// player index asked for.
pub static CALLS: AtomicU32 = AtomicU32::new(0);
pub static LAST_RC: AtomicU32 = AtomicU32::new(u32::MAX);
pub static LAST_INDEX: AtomicU32 = AtomicU32::new(u32::MAX);
/// The last real pad state player 0's pad reported, and when.
pub static REAL: Mutex<Option<(XINPUT_STATE, Instant)>> = Mutex::new(None);

/// Holds the synthetic pad for `ms` milliseconds (replacing any earlier one).
pub fn hold(s: Synthetic, ms: u64) {
    *SYNTH.lock().unwrap_or_else(|e| e.into_inner()) = Some((s, Instant::now() + Duration::from_millis(ms)));
    PACKET.fetch_add(1, Ordering::Relaxed);
}

/// The running `hold_seq`, if any (a newer one stops it).
static SEQ_GEN: AtomicU32 = AtomicU32::new(0);

/// Holds each synthetic pad in turn for its milliseconds (dev `pad seq`: motion tests need steps
/// timed closer than separate dev commands can be).
pub fn hold_seq(steps: Vec<(Synthetic, u64)>) {
    let generation = SEQ_GEN.fetch_add(1, Ordering::Relaxed) + 1;
    std::thread::spawn(move || {
        for (s, ms) in steps {
            if SEQ_GEN.load(Ordering::Relaxed) != generation {
                return;
            }
            // a little longer than the step, so the next one takes over without a gap
            hold(s, ms + 50);
            std::thread::sleep(Duration::from_millis(ms));
        }
    });
}

/// Releases the synthetic pad.
pub fn release() {
    *SYNTH.lock().unwrap_or_else(|e| e.into_inner()) = None;
    PACKET.fetch_add(1, Ordering::Relaxed);
}

/// Whether a synthetic press is being held right now.
pub fn holding() -> bool {
    SYNTH.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|(_, until)| Instant::now() < until)
}

/// Parses "A+DOWN+LY=1.0" (buttons and axes, '+' separated) into a synthetic pad.
pub fn parse(spec: &str) -> Result<Synthetic, String> {
    let mut s = Synthetic::default();
    for tok in spec.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        if let Some((axis, v)) = tok.split_once('=') {
            let v: f32 = v.parse().map_err(|_| format!("bad value in {tok}"))?;
            let stick = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
            let trig = (v.clamp(0.0, 1.0) * 255.0) as u8;
            match axis.to_ascii_uppercase().as_str() {
                "LX" => s.lx = stick,
                "LY" => s.ly = stick,
                "RX" => s.rx = stick,
                "RY" => s.ry = stick,
                "LT" => s.lt = trig,
                "RT" => s.rt = trig,
                _ => return Err(format!("unknown axis {axis}")),
            }
            continue;
        }
        s.buttons |= match tok.to_ascii_uppercase().as_str() {
            "UP" => 0x0001,
            "DOWN" => 0x0002,
            "LEFT" => 0x0004,
            "RIGHT" => 0x0008,
            "START" => 0x0010,
            "BACK" => 0x0020,
            "LS" => 0x0040,
            "RS" => 0x0080,
            "LB" => 0x0100,
            "RB" => 0x0200,
            "A" => 0x1000,
            "B" => 0x2000,
            "X" => 0x4000,
            "Y" => 0x8000,
            "LT" => {
                s.lt = 255;
                0
            }
            "RT" => {
                s.rt = 255;
                0
            }
            _ => return Err(format!("unknown button {tok}")),
        };
    }
    Ok(s)
}

unsafe extern "system" fn xinput_get_state(index: u32, state: *mut XINPUT_STATE) -> u32 {
    let original: XInputGetStateFn = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Relaxed)) };
    let rc = unsafe { original(index, state) };
    CALLS.fetch_add(1, Ordering::Relaxed);
    LAST_INDEX.store(index, Ordering::Relaxed);
    if index == 0 {
        LAST_RC.store(rc, Ordering::Relaxed);
    }
    if index != 0 || state.is_null() {
        return if keyboard_only() { ERROR_DEVICE_NOT_CONNECTED } else { rc };
    }
    let s = unsafe { &mut *state };
    if rc == ERROR_SUCCESS {
        *REAL.lock().unwrap_or_else(|e| e.into_inner()) = Some((*s, Instant::now()));
    }
    let synth = {
        let mut g = SYNTH.lock().unwrap_or_else(|e| e.into_inner());
        match *g {
            Some((p, until)) if Instant::now() < until => Some(p),
            Some(_) => {
                *g = None;
                PACKET.fetch_add(1, Ordering::Relaxed);
                None
            }
            None => None,
        }
    };
    let Some(p) = synth.or_else(|| VIRTUAL.load(Ordering::Relaxed).then(Synthetic::default)) else {
        // ini `keyboard_only = 1` (play.ps1 unless -Pad): no pad at all for the game, so its
        // prompts stay keyboard and mouse (a connected pad, or Steam's, kept them on the pad's)
        if keyboard_only() {
            return ERROR_DEVICE_NOT_CONNECTED;
        }
        if rc == ERROR_SUCCESS {
            take_movement(s);
        }
        return rc;
    };
    if rc != ERROR_SUCCESS {
        // no real pad: the synthetic one is the whole state
        *s = XINPUT_STATE::default();
    }
    let g = &mut s.Gamepad;
    g.wButtons = XINPUT_GAMEPAD_BUTTON_FLAGS(g.wButtons.0 | p.buttons);
    for (dst, v) in [(&mut g.sThumbLX, p.lx), (&mut g.sThumbLY, p.ly), (&mut g.sThumbRX, p.rx), (&mut g.sThumbRY, p.ry)] {
        if v != 0 {
            *dst = v;
        }
    }
    g.bLeftTrigger = g.bLeftTrigger.max(p.lt);
    g.bRightTrigger = g.bRightTrigger.max(p.rt);
    s.dwPacketNumber = s.dwPacketNumber.wrapping_add(PACKET.load(Ordering::Relaxed) << 16);
    take_movement(s);
    ERROR_SUCCESS
}

/// The pad as the movement controller sees it (spike S5): the left stick and buttons of the last
/// state the game read, real and synthetic merged.
#[derive(Clone, Copy, Default, Debug)]
pub struct MovePad {
    pub lx: i16,
    pub ly: i16,
    pub buttons: u16,
}

static MOVE_PAD: Mutex<Option<(MovePad, Instant)>> = Mutex::new(None);
/// The movement controller owns walking: the game gets no left stick (er-mario's xinput_filter).
pub static MOVEMENT_TAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The pad state for the movement controller, if the game read one in the last 0.25 s.
pub fn move_pad() -> Option<MovePad> {
    MOVE_PAD.lock().unwrap_or_else(|e| e.into_inner()).filter(|(_, t)| t.elapsed().as_secs_f32() < 0.25).map(|(p, _)| p)
}

/// Keeps the pad for the movement controller and, while it owns walking, hides the left stick
/// from the game (buttons still reach it; the actions they'd make are stripped in kcc.rs).
fn take_movement(s: &mut XINPUT_STATE) {
    let g = &mut s.Gamepad;
    *MOVE_PAD.lock().unwrap_or_else(|e| e.into_inner()) = Some((MovePad { lx: g.sThumbLX, ly: g.sThumbLY, buttons: g.wButtons.0 }, Instant::now()));
    if MOVEMENT_TAKEN.load(Ordering::Relaxed) {
        g.sThumbLX = 0;
        g.sThumbLY = 0;
    }
}

/// Switches the virtual pad on or off and tells the game the devices changed (WM_DEVICECHANGE),
/// so it probes the XInput slots again: without a pad it only does that at startup and on device
/// changes.
pub fn set_virtual(on: bool) {
    VIRTUAL.store(on, Ordering::Relaxed);
    use fromsoftware_shared::FromStatic;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
    const WM_DEVICECHANGE: u32 = 0x0219;
    const DBT_DEVNODES_CHANGED: usize = 0x0007;
    match unsafe { eldenring::cs::CSWindowImp::instance() } {
        Ok(w) if w.window_handle != 0 => {
            let hwnd = HWND(w.window_handle as *mut std::ffi::c_void);
            let ok = unsafe { PostMessageW(Some(hwnd), WM_DEVICECHANGE, WPARAM(DBT_DEVNODES_CHANGED), LPARAM(0)) }.is_ok();
            log(format!(
                "input: virtual pad {}; device change posted: {ok}; real pad {}",
                if on { "on" } else { "off" },
                if LAST_RC.load(Ordering::Relaxed) == ERROR_SUCCESS { "connected" } else { "none" }
            ));
        }
        _ => log(format!("input: virtual pad {} (no game window yet)", if on { "on" } else { "off" })),
    }
}

/// Movement keys for the movement controller (Apex's PC defaults): WASD, Shift sprint (held),
/// Space jump, Ctrl hold crouch / slide, C toggle crouch. Only while the game window has the focus.
#[derive(Clone, Copy, Default, Debug)]
pub struct MoveKeys {
    pub x: f32,
    pub y: f32,
    pub sprint: bool,
    pub jump: bool,
    pub crouch: bool,
    pub crouch_toggle: bool,
}

pub fn move_keys() -> Option<MoveKeys> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT, VK_SPACE};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    if pid != unsafe { GetCurrentProcessId() } {
        return None;
    }
    let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    let axis = |plus: u8, minus: u8| (down(plus as u16) as i32 - down(minus as u16) as i32) as f32;
    Some(MoveKeys {
        x: axis(b'D', b'A'),
        y: axis(b'W', b'S'),
        sprint: down(VK_SHIFT.0),
        jump: down(VK_SPACE.0),
        crouch: down(VK_CONTROL.0),
        crouch_toggle: down(b'C' as u16),
    })
}

/// The ability keys (Apex's PC defaults, D-029): Q tactical, Z ultimate. Only while the game window
/// has the focus.
pub fn ability_keys() -> Option<(bool, bool)> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    if pid != unsafe { GetCurrentProcessId() } {
        return None;
    }
    let down = |vk: u8| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    Some((down(b'Q'), down(b'Z')))
}

/// The shield battery's key (D-032), only while the game window has the focus: 4 (S3's
/// `+scriptCommand4`, the survival slot). Keys 1 / 2 are the weapon slots' (weapons.rs).
#[derive(Clone, Copy, Default, Debug)]
pub struct BatteryKeys {
    pub use_key: bool,
}

pub fn battery_keys() -> Option<BatteryKeys> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    if pid != unsafe { GetCurrentProcessId() } {
        return None;
    }
    let down = |vk: u8| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    Some(BatteryKeys { use_key: down(b'4') })
}

/// Whether a key (Windows virtual-key code: 'G', '1' ...) is down, only while the game window has the
/// focus (None otherwise). The frag grenade's G and the weapon keys it is put away with
/// (spike/grenade.rs).
pub fn key_down(vk: u8) -> Option<bool> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    if pid != unsafe { GetCurrentProcessId() } {
        return None;
    }
    Some(unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0)
}

/// One line of diagnostics for the dev channel.
pub fn status() -> String {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    let real = REAL.lock().unwrap_or_else(|e| e.into_inner()).map(|(s, t)| (s.Gamepad.wButtons.0, t.elapsed().as_secs_f32()));
    format!(
        "hook calls {}, last index {}, real pad 0 rc {} ({}), real buttons/age {:?}, synthetic {}, game focused {}",
        CALLS.load(Ordering::Relaxed),
        LAST_INDEX.load(Ordering::Relaxed),
        LAST_RC.load(Ordering::Relaxed),
        match LAST_RC.load(Ordering::Relaxed) { 0 => "connected", 1167 => "not connected", _ => "?" },
        real,
        holding(),
        pid == unsafe { GetCurrentProcessId() }
    )
}

/// Swaps the game's import of XInputGetState (ordinal 2 of xinput1_4) for ours.
pub unsafe fn install() {
    match unsafe { patch_import("xinput", c"XInputGetState", Some(2), xinput_get_state as *const () as usize) } {
        Some((dll, previous)) => {
            ORIGINAL.store(previous, Ordering::Relaxed);
            log(format!("input: hooked XInputGetState through the game's import table ({dll})"));
            // dev runs drive menus with synthetic presses: the pad has to be there from the start
            if crate::paths::flag("virtual_pad") {
                VIRTUAL.store(true, Ordering::Relaxed);
                log("input: virtual pad on from the start (virtual_pad = 1)");
            }
        }
        None => log("input: XInputGetState not in the game's import table; no pad hook"),
    }
}

/// Replaces the game executable's import of `name` (or `ordinal`) from a DLL whose name starts with
/// `dll_prefix` (any case) with `replacement`. Returns (dll name, the pointer that was there).
pub(crate) unsafe fn patch_import(dll_prefix: &str, name: &std::ffi::CStr, ordinal: Option<u16>, replacement: usize) -> Option<(String, usize)> {
    use windows::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualProtect};
    let base = unsafe { GetModuleHandleW(None) }.ok()?.0 as usize;
    let u32_at = |a: usize| unsafe { (a as *const u32).read_unaligned() };
    let nt = base + u32_at(base + 0x3C) as usize;
    if u32_at(nt) != 0x4550 {
        return None;
    }
    // PE32+: optional header at +0x18, data directories at +0x70 into it; [1] = imports
    let imports = u32_at(nt + 0x18 + 0x70 + 8) as usize;
    if imports == 0 {
        return None;
    }
    let mut desc = base + imports;
    loop {
        let (lookup, dll_name, iat) = (u32_at(desc) as usize, u32_at(desc + 12) as usize, u32_at(desc + 16) as usize);
        if dll_name == 0 {
            return None;
        }
        let dll = unsafe { std::ffi::CStr::from_ptr((base + dll_name) as *const std::ffi::c_char) }.to_string_lossy().to_string();
        if dll.to_ascii_lowercase().starts_with(dll_prefix) {
            let names = if lookup != 0 { lookup } else { iat };
            for k in 0.. {
                let entry = unsafe { ((base + names + k * 8) as *const u64).read_unaligned() };
                if entry == 0 {
                    break;
                }
                let hit = if entry & (1 << 63) != 0 {
                    Some((entry & 0xFFFF) as u16) == ordinal
                } else {
                    let n = unsafe { std::ffi::CStr::from_ptr((base + entry as usize + 2) as *const std::ffi::c_char) };
                    n == name
                };
                if hit {
                    let slot = (base + iat + k * 8) as *mut usize;
                    let mut old = PAGE_PROTECTION_FLAGS(0);
                    unsafe { VirtualProtect(slot as *const _, 8, PAGE_READWRITE, &mut old) }.ok()?;
                    let previous = unsafe { slot.read() };
                    unsafe { slot.write(replacement) };
                    let mut back = PAGE_PROTECTION_FLAGS(0);
                    let _ = unsafe { VirtualProtect(slot as *const _, 8, old, &mut back) };
                    return Some((dll, previous));
                }
            }
        }
        desc += 20;
    }
}
