//! The game's keyboard while the movement controller owns walking (spike S5): its movement keys
//! (WASD, Space, Shift, Ctrl, C; `input::move_keys`) look released to the game, so WASD doesn't
//! also walk the Tarnished against the controller (forward/back shook, 2026-10-04 user test).
//! The game reads the keyboard through DirectInput; GetAsyncKeyState, which the controller reads,
//! is not affected.
//!
//! Adapted from er-mario's kbd.rs (MIT, Copyright (c) 2026 Delta): the DirectInput hooks are its;
//! the hidden keys and the switch are ours.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::core::{GUID, s, w};

use crate::input::MOVEMENT_TAKEN;
use crate::log;

/// DirectInput scancodes the game must not see while the controller moves the legend:
/// W A S D, Space, left/right Shift, left Ctrl, C; Q and Z, Octane's abilities (D-029; Q is the
/// game's lock-on); 1 and 2, the weapon slots (U3); 4, the shield battery (D-032); G, the frag
/// grenade (U9; the game's map key: the map stays in the menu); 5, the weapon's inspect; Tab and 3,
/// the weapon wheel; F5, the switch to the Tarnished and back (mode.rs)
const HIDDEN: [u32; 19] = [0x11, 0x1E, 0x1F, 0x20, 0x39, 0x2A, 0x36, 0x1D, 0x2E, 0x10, 0x2C, 0x02, 0x03, 0x05, 0x22, 0x06, 0x0F, 0x04, 0x3F];

const IID_IDIRECTINPUT8W: GUID = GUID::from_u128(0xBF798031_483A_4DA2_AA99_5D64ED369700);
const GUID_SYSKEYBOARD: GUID = GUID::from_u128(0x6F1D2B61_D5A0_11CF_BFC7_444553540000);
const DI8DEVTYPE_KEYBOARD: u32 = 0x13;

type DirectInput8Create = unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void, *mut c_void) -> i32;
type CreateDevice = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void, *mut c_void) -> i32;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;
type GetDeviceInfo = unsafe extern "system" fn(*mut c_void, *mut u8) -> i32;

/// device -> is a keyboard
static KEYBOARDS: Mutex<Option<HashMap<usize, bool>>> = Mutex::new(None);
static GET_INFO: Mutex<Option<usize>> = Mutex::new(None);

fn capture() -> bool {
    MOVEMENT_TAKEN.load(Ordering::Relaxed)
}

fn is_keyboard(device: usize) -> bool {
    let mut map = KEYBOARDS.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    *map.entry(device).or_insert_with(|| {
        let Some(info) = *GET_INFO.lock().unwrap_or_else(|e| e.into_inner()) else { return false };
        let f: GetDeviceInfo = unsafe { std::mem::transmute(info) };
        // DIDEVICEINSTANCEW: dwSize, guidInstance, guidProduct, dwDevType, ...
        let mut buf = [0u8; 0x400];
        let size = 4 + 16 + 16 + 4 + 260 * 2 * 2 + 16 + 16 + 4 + 2;
        buf[..4].copy_from_slice(&(size as u32).to_le_bytes());
        let ok = unsafe { f(device as *mut c_void, buf.as_mut_ptr()) } >= 0;
        let kind = u32::from_le_bytes(buf[36..40].try_into().unwrap());
        ok && kind & 0xFF == DI8DEVTYPE_KEYBOARD
    })
}

/// Hooks the DirectInput device methods the game reads keys with (GetDeviceState for the whole
/// keyboard, GetDeviceData for buffered key events).
pub unsafe fn install() {
    use ilhook::x64::{CallbackOption, HookFlags, hook_closure_retn};
    let result = (|| -> Result<(), String> {
        let dll = unsafe { GetModuleHandleW(w!("dinput8.dll")).or_else(|_| LoadLibraryW(w!("dinput8.dll"))) }.map_err(|e| e.to_string())?;
        let create: DirectInput8Create =
            unsafe { std::mem::transmute(GetProcAddress(dll, s!("DirectInput8Create")).ok_or("no DirectInput8Create")?) };
        let hinst = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
        let mut di: *mut c_void = std::ptr::null_mut();
        if unsafe { create(hinst.0, 0x0800, &IID_IDIRECTINPUT8W, &mut di, std::ptr::null_mut()) } < 0 || di.is_null() {
            return Err("DirectInput8Create failed".into());
        }
        let di_vtbl = unsafe { *(di as *const *const usize) };
        let create_device: CreateDevice = unsafe { std::mem::transmute(*di_vtbl.add(3)) };
        let mut dev: *mut c_void = std::ptr::null_mut();
        if unsafe { create_device(di, &GUID_SYSKEYBOARD, &mut dev, std::ptr::null_mut()) } < 0 || dev.is_null() {
            return Err("CreateDevice(keyboard) failed".into());
        }
        let vtbl = unsafe { *(dev as *const *const usize) };
        let (get_state, get_data, get_info) = unsafe { (*vtbl.add(9), *vtbl.add(10), *vtbl.add(15)) };
        *GET_INFO.lock().unwrap_or_else(|e| e.into_inner()) = Some(get_info);
        // the methods live in dinput8.dll's code: the device can go again
        unsafe {
            (std::mem::transmute::<usize, Release>(*vtbl.add(2)))(dev);
            (std::mem::transmute::<usize, Release>(*di_vtbl.add(2)))(di);
        }

        // GetDeviceState(this, size, data): the whole keyboard as 256 bytes
        let state_hook = |reg: *mut ilhook::x64::Registers, original: usize| -> usize {
            let (this, size, data) = unsafe { ((*reg).rcx, (*reg).rdx as u32, (*reg).r8 as *mut u8) };
            let f: unsafe extern "system" fn(u64, u32, *mut u8) -> i32 = unsafe { std::mem::transmute(original) };
            let rc = unsafe { f(this, size, data) };
            if rc >= 0 && size == 256 && !data.is_null() && capture() {
                for &k in &HIDDEN {
                    unsafe { *data.add(k as usize) = 0 };
                }
            }
            rc as u32 as usize
        };
        // GetDeviceData(this, object size, data, in/out count, flags): buffered events
        let data_hook = |reg: *mut ilhook::x64::Registers, original: usize| -> usize {
            let (this, obj, data, count, flags) =
                unsafe { ((*reg).rcx, (*reg).rdx as u32, (*reg).r8 as *mut u8, (*reg).r9 as *mut u32, *(((*reg).rsp + 0x28) as *const u32)) };
            let f: unsafe extern "system" fn(u64, u32, *mut u8, *mut u32, u32) -> i32 = unsafe { std::mem::transmute(original) };
            let rc = unsafe { f(this, obj, data, count, flags) };
            if rc >= 0 && !data.is_null() && !count.is_null() && obj >= 8 && capture() && is_keyboard(this as usize) {
                // DIDEVICEOBJECTDATA: dwOfs (the scancode), dwData (0x80 = pressed), ...
                for i in 0..unsafe { *count } as usize {
                    let e = unsafe { data.add(i * obj as usize) };
                    if HIDDEN.contains(&unsafe { *(e as *const u32) }) {
                        unsafe { *(e.add(4) as *mut u32) = 0 };
                    }
                }
            }
            rc as u32 as usize
        };
        for (addr, name) in [(get_state, "GetDeviceState"), (get_data, "GetDeviceData")] {
            let res = if name == "GetDeviceState" {
                unsafe { hook_closure_retn(addr, state_hook, CallbackOption::None, HookFlags::empty()) }
            } else {
                unsafe { hook_closure_retn(addr, data_hook, CallbackOption::None, HookFlags::empty()) }
            };
            match res {
                Ok(h) => std::mem::forget(h),
                Err(e) => return Err(format!("{name}: {e:?}")),
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => log("kbd: hooked the game's keyboard (DirectInput)"),
        Err(e) => log(format!("kbd: keyboard hook failed ({e}); WASD also reaches the game")),
    }
}
