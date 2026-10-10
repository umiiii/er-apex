//! Fuse's sounds: T007's er-apex-audio mixer (cpal) on a thread of its own. Game tasks only send a
//! sound name and when to play it; nothing on the game's threads waits for audio.
//!
//! The sounds (WAV + manifest.json) are exported from the player's own Apex install
//! (tools/fuseaudio): `audio_dir` in er_apex.ini, or the mod folder's `audio\`; Octane's ability
//! sounds (T018, `--set octane`) from its `octane\` folder, the Charge Rifle's (U3, `--set
//! defender`) from `defender\` and the frag grenade's (U9, `--set frag`) from `frag_grenade\`
//! when they are there, in the same bank.

use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use er_apex_audio::{Mixer, MixerConfig};

use crate::{log, paths};

enum Msg {
    Play { name: &'static str, volume: f32, at: Instant },
    /// one sound: its voices and its scheduled plays
    Stop(&'static str),
    StopAll,
}

static TX: Mutex<Option<mpsc::Sender<Msg>>> = Mutex::new(None);

/// Starts the audio thread (once). Without a manifest the mod stays silent and says so in the log.
pub fn start() {
    let mut tx = TX.lock().unwrap_or_else(|e| e.into_inner());
    if tx.is_some() {
        return;
    }
    let dir = paths::config("audio_dir").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| paths::file("audio"));
    let (send, recv) = mpsc::channel();
    *tx = Some(send);
    std::thread::spawn(move || run(dir, recv));
}

fn run(dir: PathBuf, rx: mpsc::Receiver<Msg>) {
    let mut config = MixerConfig::default();
    config.max_voices = 32;
    config.group_limits.insert("weapon".into(), 16);
    config.group_limits.insert("reload".into(), 4);
    config.group_limits.insert("ability".into(), 8);
    let mut mixer = match Mixer::with_config(config) {
        Ok(m) => m,
        Err(e) => {
            log(format!("audio: no output device ({e}); silent"));
            return;
        }
    };
    let octane = dir.join("octane");
    // U3: the Charge Rifle's and the weapon switch's (tools/fuseaudio/export_audio.py --set defender)
    let defender = dir.join("defender");
    // the frag grenade's (U9, `--set frag`)
    let frag = dir.join("frag_grenade");
    // the Wingman's, in the R-301's place (`--set wingman`)
    let wingman = dir.join("wingman");
    // the R-99's (the weapon wheel; `--set r99`)
    let r99 = dir.join("r99");
    // the kunai's (the holstered mode; `--set kunai`)
    let kunai = dir.join("kunai");
    // the Flatline's (the weapon wheel; `--set flatline`)
    let flatline = dir.join("flatline");
    // the Sentinel's (`--set sentinel`)
    let sentinel = dir.join("sentinel");
    // a damaging hit's (the armour break, `--set hits`)
    let hits = dir.join("hits");
    // Pathfinder's grapple (`--set grapple`)
    let grapple = dir.join("grapple");
    let mut dirs = vec![dir.as_path()];
    for set in [&octane, &defender, &frag, &wingman, &r99, &kunai, &flatline, &sentinel, &hits, &grapple] {
        if set.join("manifest.json").exists() {
            dirs.push(set.as_path());
        }
    }
    match mixer.load_manifests(&dirs) {
        Ok(n) => {
            let o = mixer.output_info();
            let from: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
            log(format!("audio: {n} sounds from {} ({} Hz, {} ch)", from.join(" + "), o.sample_rate, o.channels));
        }
        Err(e) => {
            log(format!("audio: no sounds in {} ({e}); silent", dir.display()));
            return;
        }
    }
    let master = paths::number::<f32>("volume").unwrap_or(0.6).clamp(0.0, 4.0);
    // ini `audio_log = 1`: a log line per sound played (checking the timings without listening)
    let trace = paths::number::<u32>("audio_log").unwrap_or(0) == 1;
    let mut pending: Vec<(Instant, &'static str, f32)> = Vec::new();
    let mut errors = 0u32;
    loop {
        let wait = pending.iter().map(|p| p.0).min().map_or(Duration::from_secs(3600), |t| t.saturating_duration_since(Instant::now()));
        match rx.recv_timeout(wait) {
            Ok(Msg::Play { name, volume, at }) => pending.push((at, name, volume)),
            Ok(Msg::Stop(name)) => {
                pending.retain(|p| p.1 != name);
                // a sound the bank lacks was never played
                let _ = mixer.stop(name);
            }
            Ok(Msg::StopAll) => {
                pending.clear();
                mixer.stop_all();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        pending.retain(|&(at, name, volume)| {
            if at > now {
                return true;
            }
            if let Err(e) = mixer.play(name, volume * master) {
                errors += 1;
                if errors <= 10 {
                    log(format!("audio: {name}: {e}"));
                }
            } else if trace {
                log(format!("audio: play {name}"));
            }
            false
        });
    }
}

/// Plays a sound now.
pub fn play(name: &'static str, volume: f32) {
    play_in(name, volume, 0.0);
}

/// Plays a sound `delay` seconds from now.
pub fn play_in(name: &'static str, volume: f32, delay: f32) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let at = Instant::now() + Duration::from_secs_f32(delay.max(0.0));
        let _ = tx.send(Msg::Play { name, volume, at });
    }
}

/// Cancels one sound, playing or scheduled (a charge cut short).
pub fn stop(name: &'static str) {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(Msg::Stop(name));
    }
}

/// Cancels everything playing or scheduled (a reload interrupted, a death...).
pub fn stop_all() {
    if let Some(tx) = TX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(Msg::StopAll);
    }
}
