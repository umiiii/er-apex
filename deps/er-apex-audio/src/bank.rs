use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::{AudioError, Result};

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub sounds: BTreeMap<String, SoundDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct SoundDefinition {
    pub group: String,
    pub max_instances: usize,
    pub variants: Vec<VariantDefinition>,
}

#[derive(Debug, Deserialize)]
pub struct VariantDefinition {
    pub file: String,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self> {
        let manifest: Self =
            serde_json::from_str(json).map_err(|e| AudioError::Manifest(e.to_string()))?;
        if manifest.version != 1 || manifest.sounds.is_empty() {
            return Err(AudioError::Manifest("expected version 1 and sounds".into()));
        }
        for (name, sound) in &manifest.sounds {
            if name.is_empty()
                || sound.group.is_empty()
                || sound.max_instances == 0
                || sound.variants.is_empty()
            {
                return Err(AudioError::Manifest(format!(
                    "invalid name/group/limit/variants for {name}"
                )));
            }
            for variant in &sound.variants {
                let path = Path::new(&variant.file);
                if variant.file.is_empty()
                    || !path.components().all(|p| matches!(p, Component::Normal(_)))
                {
                    return Err(AudioError::Manifest(format!(
                        "WAV paths must be relative without traversal: {}",
                        variant.file
                    )));
                }
            }
        }
        Ok(manifest)
    }
}

pub(crate) struct Clip {
    pub frames: Vec<[f32; 2]>,
    pub leading_silence_frames: usize,
}

pub(crate) struct Sound {
    pub group: usize,
    pub group_limit: usize,
    pub max_instances: usize,
    pub variants: Vec<usize>,
}

#[derive(Default)]
pub(crate) struct SampleBank {
    pub clips: Vec<Clip>,
    pub sounds: Vec<Sound>,
    pub names: BTreeMap<String, usize>,
}

/// One bank from the manifests of several directories (each WAV stays inside its own manifest's
/// directory); a sound name may appear in only one of them.
pub(crate) fn load_bank(
    dirs: &[&Path],
    rate: u32,
    voice_limit: usize,
    group_limits: &BTreeMap<String, usize>,
) -> Result<SampleBank> {
    if dirs.is_empty() {
        return Err(AudioError::Manifest("no manifest directory".into()));
    }
    let mut bank = SampleBank::default();
    let mut files = HashMap::new();
    let mut groups = HashMap::new();
    for dir in dirs {
        load_into(&mut bank, &mut files, &mut groups, dir, rate, voice_limit, group_limits)?;
    }
    Ok(bank)
}

fn load_into(
    bank: &mut SampleBank,
    files: &mut HashMap<std::path::PathBuf, usize>,
    groups: &mut HashMap<String, usize>,
    dir: &Path,
    rate: u32,
    voice_limit: usize,
    group_limits: &BTreeMap<String, usize>,
) -> Result<()> {
    let root = dir.canonicalize()?;
    let manifest = Manifest::parse(&std::fs::read_to_string(root.join("manifest.json"))?)?;
    for (name, definition) in manifest.sounds {
        // a sound two sets both export (the same event: the R-301's equip in the Charge Rifle's
        // set and the Flatline's) keeps the first manifest's; one such name used to silence it all
        if bank.names.contains_key(&name) {
            continue;
        }
        let next_group = groups.len();
        let group = *groups.entry(definition.group.clone()).or_insert(next_group);
        let group_limit = group_limits
            .get(&definition.group)
            .copied()
            .unwrap_or(voice_limit)
            .min(voice_limit);
        let mut variants = Vec::with_capacity(definition.variants.len());
        for variant in definition.variants {
            let path = root.join(&variant.file).canonicalize()?;
            if !path.starts_with(&root) {
                return Err(AudioError::Manifest(
                    "WAV escapes manifest directory".into(),
                ));
            }
            let index = match files.get(&path) {
                Some(&index) => index,
                None => {
                    let index = bank.clips.len();
                    bank.clips.push(read_wav(&path, rate)?);
                    files.insert(path, index);
                    index
                }
            };
            variants.push(index);
        }
        bank.names.insert(name, bank.sounds.len());
        bank.sounds.push(Sound {
            group,
            group_limit,
            max_instances: definition.max_instances.min(voice_limit),
            variants,
        });
    }
    Ok(())
}

fn read_wav(path: &Path, output_rate: u32) -> Result<Clip> {
    let mut file = std::fs::File::open(path)?;
    let bytes = file.metadata()?.len();
    if !(44..=128 * 1024 * 1024).contains(&bytes) {
        return Err(AudioError::Wav("empty/oversized WAV".into()));
    }
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF"
        || &header[8..] != b"WAVE"
        || u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8 != bytes
    {
        return Err(AudioError::Wav("RIFF header/length mismatch".into()));
    }
    let mut reader = hound::WavReader::open(path).map_err(|e| AudioError::Wav(e.to_string()))?;
    let spec = reader.spec();
    if !matches!(spec.channels, 1 | 2)
        || !(8_000..=192_000).contains(&spec.sample_rate)
        || !(8_000..=192_000).contains(&output_rate)
    {
        return Err(AudioError::Wav(
            "expected mono/stereo and rate 8000..192000; downmix offline".into(),
        ));
    }
    let mut samples = Vec::with_capacity(reader.len() as usize);
    match spec.sample_format {
        hound::SampleFormat::Float if spec.bits_per_sample == 32 => {
            for value in reader.samples::<f32>() {
                samples.push(value.map_err(|e| AudioError::Wav(e.to_string()))?);
            }
        }
        hound::SampleFormat::Int if matches!(spec.bits_per_sample, 8 | 16 | 24 | 32) => {
            let divisor = 2.0_f64.powi(i32::from(spec.bits_per_sample) - 1);
            for value in reader.samples::<i32>() {
                samples.push(
                    (f64::from(value.map_err(|e| AudioError::Wav(e.to_string()))?) / divisor)
                        as f32,
                );
            }
        }
        _ => return Err(AudioError::Wav("unsupported sample encoding".into())),
    }
    if samples.is_empty()
        || samples.len() % usize::from(spec.channels) != 0
        || samples.iter().any(|s| !s.is_finite() || s.abs() > 16.0)
    {
        return Err(AudioError::Wav(
            "empty, incomplete or invalid samples".into(),
        ));
    }
    let frames: Vec<[f32; 2]> = samples
        .chunks_exact(usize::from(spec.channels))
        .map(|c| [c[0], c[usize::from(spec.channels) - 1]])
        .collect();
    let frames = resample(&frames, spec.sample_rate, output_rate);
    let leading_silence_frames = frames
        .iter()
        .position(|f| f[0].abs().max(f[1].abs()) > 1.0e-6)
        .unwrap_or(frames.len());
    Ok(Clip {
        frames,
        leading_silence_frames,
    })
}

/// Linear interpolation is deliberately done once, before playback.
fn resample(frames: &[[f32; 2]], input_rate: u32, output_rate: u32) -> Vec<[f32; 2]> {
    if input_rate == output_rate {
        return frames.to_vec();
    }
    let length =
        (frames.len() as u64 * u64::from(output_rate)).div_ceil(u64::from(input_rate)) as usize;
    (0..length)
        .map(|i| {
            let position = i as f64 * f64::from(input_rate) / f64::from(output_rate);
            let index = (position as usize).min(frames.len() - 1);
            let next = (index + 1).min(frames.len() - 1);
            let fraction = (position - index as f64) as f32;
            std::array::from_fn(|c| {
                frames[index][c] + (frames[next][c] - frames[index][c]) * fraction
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn manifest_validates_contract() {
        let valid = r#"{"version":1,"sounds":{"shot":{"group":"gun","max_instances":2,"variants":[{"file":"r301/shot.wav","event":"real evidence"}]}}}"#;
        let parsed = Manifest::parse(valid).unwrap();
        assert_eq!(parsed.sounds["shot"].variants[0].file, "r301/shot.wav");
        for bad in [
            valid.replace("r301/shot.wav", "../shot.wav"),
            valid.replace("\"version\":1", "\"version\":2"),
            valid.replace("\"max_instances\":2", "\"max_instances\":0"),
            "{\"version\":1,\"sounds\":{}}".into(),
        ] {
            assert!(Manifest::parse(&bad).is_err());
        }
    }

    /// Two manifests make one bank (a shared group counts once); a name in both is refused.
    #[test]
    fn manifests_merge_and_names_stay_unique() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "target/test-banks/{}-{}",
            std::process::id(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let write = |dir: &Path, sounds: &[(&str, &str)]| {
            std::fs::create_dir_all(dir).unwrap();
            let mut entries = Vec::new();
            for (name, group) in sounds {
                let mut w = hound::WavWriter::create(dir.join(format!("{name}.wav")), spec).unwrap();
                for v in [0.0_f32, 0.0, 0.5, -0.5] {
                    w.write_sample(v).unwrap();
                }
                w.finalize().unwrap();
                entries.push(format!(
                    r#""{name}":{{"group":"{group}","max_instances":2,"variants":[{{"file":"{name}.wav"}}]}}"#
                ));
            }
            std::fs::write(
                dir.join("manifest.json"),
                format!(r#"{{"version":1,"sounds":{{{}}}}}"#, entries.join(",")),
            )
            .unwrap();
        };
        let (a, b, c) = (base.join("a"), base.join("b"), base.join("c"));
        write(&a, &[("shot", "weapon"), ("reload", "reload")]);
        write(&b, &[("stim", "ability"), ("pad", "weapon")]);
        write(&c, &[("shot", "weapon")]);
        let limits = BTreeMap::from([("weapon".to_string(), 3)]);
        let bank = load_bank(&[&a, &b], 48_000, 8, &limits).unwrap();
        assert_eq!(bank.names.len(), 4);
        assert_eq!(bank.clips.len(), 4);
        let (shot, pad) = (&bank.sounds[bank.names["shot"]], &bank.sounds[bank.names["pad"]]);
        assert_eq!((shot.group, shot.group_limit), (pad.group, 3));
        // a name in two manifests: the first one's is kept
        let both = load_bank(&[&a, &c], 48_000, 8, &limits).unwrap();
        assert_eq!(both.names.len(), 2);
        assert!(load_bank(&[], 48_000, 8, &limits).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

    fn temp_file() -> std::path::PathBuf {
        // Test outputs stay under this crate's exclusive writable root.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-wavs");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!(
            "{}-{}.wav",
            std::process::id(),
            TEMP_SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn bad_wav_is_rejected() {
        let path = temp_file();
        std::fs::write(&path, b"not a wave").unwrap();
        assert!(read_wav(&path, 48_000).is_err());
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        writer.write_sample(f32::NAN).unwrap();
        writer.write_sample(0.0_f32).unwrap();
        writer.finalize().unwrap();
        assert!(read_wav(&path, 48_000).is_err());
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for sample in [0.0_f32, 0.0, 0.5, -0.5, 0.25, -0.25] {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        let loaded = read_wav(&path, 48_000).unwrap();
        assert_eq!(loaded.frames.len(), 4);
        assert!(loaded.frames.iter().all(|f| f[0] >= 0.0 && f[1] <= 0.0));
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.pop();
        std::fs::write(&path, bytes).unwrap();
        assert!(read_wav(&path, 48_000).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
