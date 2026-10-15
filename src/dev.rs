//! Developer command channel: the test harness writes commands into dev/cmd.txt in the mod folder,
//! the mod runs them on the game's frame task and logs the results ("cmd> ..."). One command per
//! line. Only with `dev = 1` in er_apex.ini.
//!
//!   ping                     log "pong"
//!   state                    log the current state snapshot
//!   skeleton [file]          dump the player's skeleton (spike S2c) to dev/<file>
//!   flag <id>                log an event flag
//!   setflag <id> <0|1>       set an event flag (test save only)
//!   pad <spec> [ms]          hold a synthetic pad, e.g. `pad A 150`, `pad LY=1+B 2000` (input.rs)
//!   pad connect|disconnect   report a connected idle pad (needed before presses without a real pad)
//!   padstat                  pad hook diagnostics
//!   qb                       quick boot stage (spike S7)
//!   cam                      the game's character camera, the render camera and ours
//!   fe [hide on|off]        the game's own HUD: hidden?, its enemy tags and boss bars
//!   warp <grace>             the game's grace warp to a grace entity id (must be unlocked)
//!   time [h] [m]             the world clock; with an hour, ask the game to move time there
//!   fuse [hp]                Fuse's HP and the no-death guard (spike S8); with a number, set it
//!   hp <n>                   set the Tarnished's HP
//!   tp <dx> <dy> <dz>        move the player by metres (Havok space; dy up)
//!   stim / jumppad / octane  Octane's stim (as key Q) / launch pad toss (as key Z) / their state
//!                            (spike/octane.rs)
//!   weapon [1|2]             the weapon slots: switch as keys 1 / 2, or show them (spike/weapons.rs)
//!   cr [fire <s>|trace <s>|ammo <n>]  the Charge Rifle: its state; hold the trigger; log every
//!                            frame; set its magazine (spike/chargerifle.rs; `fire <s>` works too)
//!   nade [g | throw [hold s] [aim] | aim <s> | cancel | boom | drop [dx dz]]
//!                            the frag grenade (spike/grenade.rs): its state; as key G; G then the
//!                            trigger for `hold` s once ready; aiming (the arc); put away; explode
//!                            the live ones now; a live one at the feet (self damage)
//!   speffect <id>            apply a SpEffect to the player (tests, e.g. damage over time)
//!   chrhp <entity> [ratio]   a character's HP by its MSB entity id; with a ratio (0..1), set it
//!   burst <entity> <n> [dmg] [rate]  n gun hits on a character (spike S6; Apex damage, shots/s)
//!   chrs [range]             characters within range metres (default 60): entity, npc param, hp, team, distance
//!   burstnear <n> [dmg] [rate]  gun hits on the nearest regular enemy
//!   atk <correction%> <0|1>  tweak the gun's attack rows live (physical correction, add base attack)
//!   quit                     end the game process at once (nothing is saved)

use std::sync::Mutex;

use eldenring::cs::CSEventFlagMan;
use fromsoftware_shared::FromStatic;

use crate::{log, paths, spike, state};

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| paths::flag("dev"))
}

/// Checks for commands every quarter second (call once per frame from a game task).
pub fn poll(dt: f32) {
    if !enabled() {
        return;
    }
    static ACC: Mutex<f32> = Mutex::new(0.0);
    {
        let mut acc = ACC.lock().unwrap_or_else(|e| e.into_inner());
        *acc += dt;
        if *acc < 0.25 {
            return;
        }
        *acc = 0.0;
    }
    auto_quit();
    let path = paths::file("dev/cmd.txt");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        log(format!("cmd> {line}"));
        let out = run(line);
        log(format!("cmd< {out}"));
    }
}

fn run(line: &str) -> String {
    let mut words = line.split_whitespace();
    let cmd = words.next().unwrap_or("");
    let args: Vec<&str> = words.collect();
    let num = |i: usize| args.get(i).and_then(|a| a.parse::<u32>().ok());
    match cmd {
        "ping" => "pong".into(),
        "state" => state::LAST.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or("no state yet".into(), |s| s.json()),
        "skeleton" => spike::skeleton::dump(args.first().copied().unwrap_or("skeleton.json")),
        "flag" => match (num(0), unsafe { CSEventFlagMan::instance() }) {
            (Some(id), Ok(m)) => format!("flag {id} = {}", m.virtual_memory_flag.get_flag(id)),
            (None, _) => "usage: flag <id>".into(),
            (_, Err(e)) => format!("no event flag manager: {e:?}"),
        },
        "setflag" => match (num(0), num(1), unsafe { CSEventFlagMan::instance_mut() }) {
            (Some(id), Some(v), Ok(m)) => {
                m.virtual_memory_flag.set_flag(id, v != 0);
                format!("flag {id} := {}", m.virtual_memory_flag.get_flag(id))
            }
            (None, _, _) | (_, None, _) => "usage: setflag <id> <0|1>".into(),
            (_, _, Err(e)) => format!("no event flag manager: {e:?}"),
        },
        "pad" if args.first() == Some(&"connect") => {
            crate::input::set_virtual(true);
            "virtual pad connected".into()
        }
        "pad" if args.first() == Some(&"disconnect") => {
            crate::input::set_virtual(false);
            "virtual pad disconnected".into()
        }
        "pad" if args.first() == Some(&"seq") => {
            // pad seq <spec> <ms> <spec> <ms> ... (`-` for nothing held)
            let steps: Result<Vec<_>, String> = args[1..]
                .chunks(2)
                .map(|c| {
                    let s = if c[0] == "-" { Ok(crate::input::Synthetic::default()) } else { crate::input::parse(c[0]) }?;
                    let ms = c.get(1).and_then(|m| m.parse().ok()).ok_or(format!("no ms after {}", c[0]))?;
                    Ok((s, ms))
                })
                .collect();
            match steps {
                Ok(steps) => {
                    let total: u64 = steps.iter().map(|s| s.1).sum();
                    let n = steps.len();
                    crate::input::hold_seq(steps);
                    format!("pad seq: {n} steps, {total} ms")
                }
                Err(e) => e,
            }
        }
        "pad" => match args.first().map(|s| crate::input::parse(s)) {
            Some(Ok(p)) => {
                let ms = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(150);
                crate::input::hold(p, ms);
                format!("holding {p:?} for {ms} ms")
            }
            Some(Err(e)) => e,
            None => "usage: pad <A+B+LY=1...> [ms]".into(),
        },
        "padstat" => crate::input::status(),
        "qb" => spike::quickboot::status(),
        "cam" => crate::camera::status(),
        "autorot" => crate::camera::auto_rotation(args.first().copied()),
        "fp" if args.first() == Some(&"vm") => spike::pov::describe(),
        "fp" if args.first() == Some(&"trace") => spike::pov::trace(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(10.0)),
        "fp" if matches!(args.first(), Some(&("hide" | "force" | "flip" | "hold" | "play"))) => spike::pov::dev(&args),
        "fp" => crate::firstperson::tune(&args),
        "fpdbg" => crate::firstperson::debug(&args),
        "fpgroup" => spike::pose::probe(&args),
        "fe" => crate::fe::command(&args),
        "cursor" => crate::cursor::command(&args),
        "time" => {
            let h = args.first().and_then(|a| a.parse::<u32>().ok());
            let m = args.get(1).and_then(|a| a.parse::<u32>().ok()).unwrap_or(0);
            match (h, unsafe { eldenring::cs::WorldAreaTime::instance_mut() }) {
                (Some(h), Ok(w)) => {
                    w.request_time(h, m, 0);
                    format!("time requested {h:02}:{m:02} (now {:02}:{:02})", w.clock.hours(), w.clock.minutes())
                }
                (None, Ok(w)) => format!("time now {:02}:{:02}", w.clock.hours(), w.clock.minutes()),
                (_, Err(e)) => format!("no WorldAreaTime: {e:?}"),
            }
        }
        "warp" => match args.first().and_then(|a| a.parse::<i32>().ok()) {
            Some(g) => spike::quickboot::warp_to(g).map_or_else(|e| e, |_| format!("warping to grace {g}")),
            None => "usage: warp <grace entity id>".into(),
        },
        "armor" => {
            match args.first().copied() {
                Some("off") => spike::armor::request(None, false),
                Some("1280") => spike::armor::request(Some(1280), false),
                Some("999") => spike::armor::request(Some(999), args.get(1).is_some_and(|s| *s == "1")),
                _ => "usage: armor 1280|999 [hide=0|1] | off (S3 test)".into(),
            }
        }
        "armorstat" => spike::armor::status(),
        "gun" => spike::gun::status(),
        // U3: `weapon [1|2]` switch / show the slots; `cr [fire <s> | trace <s> | ammo <n>]` the
        // Charge Rifle
        "weapon" => spike::weapons::dev(&args),
        "cr" => spike::chargerifle::dev(&args),
        "kcc" if args.first() == Some(&"probe") => spike::kcc::probe(),
        "kcc" if args.first() == Some(&"dump") => spike::kcc::dump(),
        "kcc" if args.first() == Some(&"trace") => spike::kcc::trace(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(30.0)),
        "kcc" if args.first() == Some(&"layers") => spike::kcc::layers(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(4.0)),
        "kcc" => spike::kcc::status(),
        "stats" => spike::stats::status(),
        "stim" => spike::octane::stim("dev"),
        "jumppad" => spike::octane::toss("dev"),
        "octane" => spike::octane::status(),
        "battery" => spike::battery::dev(&args),
        "shield" => {
            if let Some(v) = args.first().and_then(|a| a.parse::<f32>().ok()) {
                spike::lethal::set_shield(v);
            }
            spike::lethal::status()
        }
        "nade" => spike::grenade::dev(&args),
        "fx" => spike::fx::dev(&args),
        "disarm" => spike::armor::disarm(),
        // `grapple`: Q's ability to the grapple and Q pressed; `grapple stim`: back to the stim
        "grapple" if args.first() == Some(&"stim") => spike::grapple::select(spike::grapple::QAbility::Stim, "dev"),
        "grapple" => {
            let s = spike::grapple::select(spike::grapple::QAbility::Grapple, "dev");
            format!("{s}; {}", spike::grapple::press("dev"))
        }
        "mode" => crate::mode::set(args.first().is_none_or(|a| *a != "tarnished")),
        "pose" => spike::pose::command(args.first().copied()),
        "fire" => spike::gun::hold_fire(args.first().and_then(|a| a.parse().ok()).unwrap_or(1.0)),
        "fuse" => {
            if let Some(v) = args.first().and_then(|a| a.parse::<f32>().ok()) {
                spike::lethal::set_fuse_hp(v);
            }
            spike::lethal::status()
        }
        "hp" => match (args.first().and_then(|a| a.parse::<i32>().ok()), player()) {
            (Some(v), Some(p)) => {
                p.chr_ins.modules.data.hp = v;
                format!("Tarnished hp := {v} (max {})", p.chr_ins.modules.data.max_hp)
            }
            _ => "usage: hp <n> (in the world)".into(),
        },
        "speffect" => match (args.first().and_then(|a| a.parse::<i32>().ok()), player()) {
            (Some(id), Some(p)) => {
                use eldenring::cs::ChrInsExt;
                p.apply_speffect(id, false);
                format!("applied SpEffect {id} to the player")
            }
            _ => "usage: speffect <id> (in the world)".into(),
        },
        "chrhp" => match args.first().and_then(|a| a.parse::<u32>().ok()) {
            Some(id) => {
                let ratio = args.get(1).and_then(|a| a.parse::<f32>().ok());
                let mut out = format!("entity {id} not loaded");
                if let Ok(wcm) = unsafe { eldenring::cs::WorldChrMan::instance_mut() } {
                    for set in wcm.chr_sets.iter().flatten() {
                        for chr in set.characters() {
                            let chr: &mut eldenring::cs::ChrIns = chr;
                            if chr.event_entity_id == id {
                                let d = &mut chr.modules.data;
                                if let Some(r) = ratio {
                                    d.hp = ((d.max_hp as f32) * r).round() as i32;
                                }
                                out = format!("entity {id}: hp {} / {}", d.hp, d.max_hp);
                            }
                        }
                    }
                }
                out
            }
            None => "usage: chrhp <entity> [ratio]".into(),
        },
        "burstnear" => {
            let n = args.first().and_then(|a| a.parse::<u32>().ok()).unwrap_or(1);
            let dmg = args.get(1).and_then(|a| a.parse::<f32>().ok()).unwrap_or(15.0);
            let rate = args.get(2).and_then(|a| a.parse::<f32>().ok()).unwrap_or(13.5);
            spike::combat::burst_near(n, dmg, rate)
        }
        "atk" => {
            let c = args.first().and_then(|a| a.parse::<u16>().ok()).unwrap_or(0);
            let b = args.get(1).is_some_and(|a| *a == "1");
            spike::combat::tweak_atk(c, b)
        }
        "chrs" => {
            let range = args.first().and_then(|a| a.parse::<f32>().ok()).unwrap_or(60.0);
            let me = player().map(|p| p.chr_ins.modules.physics.position);
            let mut rows = Vec::new();
            if let (Ok(wcm), Some(me)) = (unsafe { eldenring::cs::WorldChrMan::instance() }, me) {
                for set in wcm.chr_sets.iter().flatten() {
                    for chr in set.characters() {
                        let chr: &eldenring::cs::ChrIns = chr;
                        let q = chr.modules.physics.position;
                        let d = ((q.0 - me.0).powi(2) + (q.1 - me.1).powi(2) + (q.2 - me.2).powi(2)).sqrt();
                        if d <= range {
                            let (ch, cr) = spike::body::capsule(chr);
                            let (bh, br) = spike::body::cylinder(chr);
                            rows.push((d, format!(
                                "{} npc {} hp {}/{} team {} type {:?} {:.1} m capsule {ch:.2}x{cr:.2} drawn top {} aim {bh:.2}x{br:.2}",
                                chr.event_entity_id, chr.npc_param_id, chr.modules.data.hp, chr.modules.data.max_hp,
                                chr.team_type, chr.chr_type, d, spike::body::top(chr).map_or("-".into(), |t| format!("{t:.2}"))
                            )));
                        }
                    }
                }
            }
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            format!("{} characters:
  {}", rows.len(), rows.into_iter().map(|r| r.1).collect::<Vec<_>>().join("
  "))
        }
        "burst" => {
            let entity = args.first().and_then(|a| a.parse::<u32>().ok());
            let n = args.get(1).and_then(|a| a.parse::<u32>().ok()).unwrap_or(1);
            let dmg = args.get(2).and_then(|a| a.parse::<f32>().ok()).unwrap_or(15.0);
            let rate = args.get(3).and_then(|a| a.parse::<f32>().ok()).unwrap_or(13.5);
            match entity {
                Some(e) => spike::combat::burst(e, n, dmg, rate),
                None => "usage: burst <entity> <n> [dmg] [rate]".into(),
            }
        }
        "tp" => {
            let f = |i: usize| args.get(i).and_then(|a| a.parse::<f32>().ok()).unwrap_or(0.0);
            match player() {
                Some(p) => {
                    let ph = &mut p.chr_ins.modules.physics;
                    let h = ph.position;
                    ph.position = eldenring::position::HavokPosition(h.0 + f(0), h.1 + f(1), h.2 + f(2), 0.0);
                    ph.chr_proxy_pos_update_requested = true;
                    // the movement controller starts over there (it would take this for a re-based origin)
                    spike::kcc::teleported();
                    format!("moved from ({:.2}, {:.2}, {:.2}) by ({}, {}, {})", h.0, h.1, h.2, f(0), f(1), f(2))
                }
                None => "not in the world".into(),
            }
        }
        "quit" => {
            log("quit requested by the dev channel");
            quit()
        }
        _ => format!("unknown command {cmd:?}"),
    }
}

fn player() -> Option<&'static mut eldenring::cs::PlayerIns> {
    unsafe { eldenring::cs::WorldChrMan::instance_mut() }.ok().and_then(|w| w.main_player.as_deref_mut())
}

/// Ends the game at once (test runs; the game saves nothing on the way out).
fn quit() -> ! {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // the log's writer thread gets to write the last lines
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = unsafe { TerminateProcess(GetCurrentProcess(), 0) };
    std::process::exit(0)
}

/// `dev_quit_after = <seconds>`: ends the game that long after the DLL loaded (unattended runs
/// can't leave a game running).
fn auto_quit() {
    static LIMIT: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    if let Some(limit) = *LIMIT.get_or_init(|| paths::number::<f32>("dev_quit_after")) {
        if crate::log::uptime() > limit {
            log(format!("dev_quit_after = {limit} s reached"));
            quit();
        }
    }
}
