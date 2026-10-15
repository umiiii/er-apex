//! The game's frame cap (ini `fps_limit`, e.g. 120): Elden Ring writes its frame time, 1/60 s, into
//! its flipper as an immediate (`mov dword ptr [reg+off], 0x3C888889`: the pattern the community's
//! FPS unlockers patch, uberhalit's EldenRingFpsUnlockAndMore); the immediate becomes 1/limit. Off
//! (the game's 60) without the key. The screen's refresh rate and vsync still cap what is shown.

use windows::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect};

use crate::{log, paths, scan};

/// 1/60 as the game's f32 bits.
const SIXTIETH: [u8; 4] = [0x89, 0x88, 0x88, 0x3C];

pub fn install() {
    let Some(limit) = paths::number::<f32>("fps_limit").filter(|l| *l > 0.0) else { return };
    let limit = limit.clamp(30.0, 360.0);
    let pat = [Some(0xC7), None, None, Some(SIXTIETH[0]), Some(SIXTIETH[1]), Some(SIXTIETH[2]), Some(SIXTIETH[3]), Some(0xEB)];
    let Some(at) = scan::unique_text(&pat) else {
        log(format!("fps: frame cap {limit}: the game's 1/60 not found (or not once); left at 60"));
        return;
    };
    let imm = at + 3;
    let bytes = (1.0f32 / limit).to_le_bytes();
    let mut old = PAGE_PROTECTION_FLAGS(0);
    unsafe {
        if VirtualProtect(imm as *const _, 4, PAGE_EXECUTE_READWRITE, &mut old).is_err() {
            log("fps: could not unprotect the code; left at 60");
            return;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), imm as *mut u8, 4);
        let _ = VirtualProtect(imm as *const _, 4, old, &mut old);
    }
    log(format!("fps: frame cap {limit} (frame time {:.6} s patched at +{:#x})", 1.0 / limit, imm - scan::base()));
}
