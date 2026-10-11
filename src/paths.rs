//! Where the mod's files live: everything sits next to er_apex.dll (the mod folder), so the mod
//! works wherever it is unpacked. Settings come from er_apex.ini there.
//!
//! Adapted from er-mario (MIT, Copyright (c) 2026 Delta).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
    GetModuleHandleExW,
};
use windows::core::PCWSTR;

pub const CONFIG: &str = "er_apex.ini";

/// The folder er_apex.dll was loaded from.
pub fn mod_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut module = HMODULE::default();
        let mut buf = [0u16; 1024];
        let len = unsafe {
            let anchor = mod_dir as *const () as *const u16;
            let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;
            if GetModuleHandleExW(flags, PCWSTR(anchor), &mut module).is_err() {
                return PathBuf::from(".");
            }
            GetModuleFileNameW(Some(module), &mut buf) as usize
        };
        let dll = PathBuf::from(String::from_utf16_lossy(&buf[..len]));
        dll.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
    })
}

pub fn file(name: &str) -> PathBuf {
    mod_dir().join(name)
}

/// `key = value` from er_apex.ini (lines starting with # or ; are comments). Read fresh on every
/// call: developer switches can be flipped while the game runs. A key set twice: the last one wins
/// (play.ps1's settings come after the dev ini's).
pub fn config(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file(CONFIG)).ok()?;
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            return None;
        }
        let (k, v) = line.split_once('=')?;
        // (a trailing "# comment" after the value is allowed)
        let v = v.split(" #").next().unwrap_or(v);
        (k.trim().eq_ignore_ascii_case(key)).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// A yes/no setting: 1/true/yes/on.
pub fn flag(key: &str) -> bool {
    config(key).is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// A numeric setting.
pub fn number<T: std::str::FromStr>(key: &str) -> Option<T> {
    config(key).and_then(|v| v.parse().ok())
}
