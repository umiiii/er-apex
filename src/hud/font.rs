//! Apex's own HUD fonts (T009): one signed-distance-field atlas (R8, 8192×3904) and, per font, a
//! table of code point -> rect (`source.meta.json`). Glyphs are cut out when the HUD first asks
//! for them, turned into coverage with the threshold of T009's preview (the game's shader is not
//! recovered), packed into one cache texture and uploaded on the next frame. Any text, Elden
//! Ring's boss names included, draws in the Apex face.
//!
//! Metrics are inferred (the atlas keeps no bearings: each glyph's cell is its ink plus the field's
//! margin, so cells differ in height): text height = the ink height of '0'; advance = ink width +
//! 8 % of the height (T009's preview); space 40 %, missing glyph 55 % (left blank, no system font).
//! Vertically, glyphs sit on the baseline (the bottom of '0'), except descenders (g j p q y , ;:
//! top at the x-height), dashes and the like (centred on the x-height) and quotes (top at the
//! cap height).
//!
//! Your own fonts (tools/apexhud/custom_font.py, ini `hud_font`): digits and English letters (printable
//! ASCII) of a face drawn from their atlas instead, everything else from Apex's; a glyph of the other
//! atlas is scaled by the two atlases' '0' so the sizes match.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use hudhook::RenderContext;
use hudhook::imgui::{DrawListMut, TextureId};
use serde::Deserialize;

use crate::log;

/// Coverage from the distance value: T009 preview's `clip((v - 176) * 255 / 16)`.
const EDGE: f32 = 176.0;
const RAMP: f32 = 16.0;
const CACHE_W: usize = 2048;
const CACHE_H: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    Body,
    Numeric,
    /// Bold, slashed zero: big numbers and the compass's cardinal points.
    Bold,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Deserialize)]
struct Meta {
    fonts: Vec<MetaFont>,
}

#[derive(Deserialize)]
struct MetaFont {
    font_index: u32,
    /// the custom atlas's (custom_font.py: the font file's stem, lower case); Apex's meta has null
    #[serde(default)]
    name: Option<String>,
    /// [code point, texture index, x, y, w, h]
    unicode_to_texture_rect: Vec<[i64; 6]>,
}

/// An atlas's R channel.
struct Atlas {
    px: Vec<u8>,
    w: usize,
    h: usize,
}

struct Fonts {
    /// Apex's atlas, then the custom one (if any)
    atlases: Vec<Atlas>,
    /// Face -> code point -> Apex's atlas rect (x, y, w, h).
    tables: HashMap<Face, HashMap<u32, [usize; 4]>>,
    /// Face -> printable ASCII -> the custom atlas's rect (atlas 1).
    custom: HashMap<Face, HashMap<u32, [usize; 4]>>,
}

/// The custom fonts to use: their atlas and meta (custom_font.py), and each face's font by name.
pub struct Custom {
    pub atlas: std::path::PathBuf,
    pub meta: std::path::PathBuf,
    pub faces: Vec<(Face, String)>,
}

static FONTS: Mutex<Option<Arc<Fonts>>> = Mutex::new(None);

/// Loads the atlas and the faces' tables (face, font index) on a thread of its own; with `custom`,
/// its fonts' digits and letters over the faces it names.
pub fn load(atlas: &Path, meta: &Path, faces: Vec<(Face, u32)>, custom: Option<Custom>) {
    let (atlas, meta) = (atlas.to_path_buf(), meta.to_path_buf());
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        match read(&atlas, &meta, &faces) {
            Ok(mut f) => {
                let counts: Vec<String> = faces.iter().map(|(face, i)| format!("{face:?} {i}: {}", f.tables[face].len())).collect();
                log(format!("hud: font atlas {}x{}, glyphs {}, in {:.1} s", f.atlases[0].w, f.atlases[0].h, counts.join(", "), t0.elapsed().as_secs_f32()));
                if let Some(c) = custom {
                    match read_custom(&c) {
                        Ok((a, tables)) => {
                            let names: Vec<String> = c.faces.iter().map(|(face, n)| format!("{face:?} {n}")).collect();
                            log(format!("hud: custom fonts {}x{}: {}", a.w, a.h, names.join(", ")));
                            f.atlases.push(a);
                            f.custom = tables;
                        }
                        Err(e) => log(format!("hud: custom fonts not loaded ({e}); Apex's fonts only")),
                    }
                }
                *FONTS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
            }
            Err(e) => log(format!("hud: fonts not loaded ({e}); Apex text stays blank")),
        }
    });
}

fn read_meta(meta: &Path) -> Result<Meta, String> {
    serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(meta).map_err(|e| format!("{}: {e}", meta.display()))?)).map_err(|e| format!("{}: {e}", meta.display()))
}

fn rects(font: &MetaFont) -> HashMap<u32, [usize; 4]> {
    font.unicode_to_texture_rect
        .iter()
        .filter(|r| r[4] > 0 && r[5] > 0 && r[0] >= 0)
        .map(|r| (r[0] as u32, [r[2] as usize, r[3] as usize, r[4] as usize, r[5] as usize]))
        .collect()
}

/// The custom atlas and, per face, its font's printable ASCII.
fn read_custom(c: &Custom) -> Result<(Atlas, HashMap<Face, HashMap<u32, [usize; 4]>>), String> {
    let m = read_meta(&c.meta)?;
    let mut tables = HashMap::new();
    for (face, name) in &c.faces {
        let font = m.fonts.iter().find(|f| f.name.as_deref() == Some(name.as_str())).ok_or_else(|| format!("no font {name} in {}", c.meta.display()))?;
        tables.insert(*face, rects(font).into_iter().filter(|(cp, _)| (33..127).contains(cp)).collect());
    }
    Ok((read_atlas(&c.atlas)?, tables))
}

fn read(atlas: &Path, meta: &Path, faces: &[(Face, u32)]) -> Result<Fonts, String> {
    let m = read_meta(meta)?;
    let mut tables = HashMap::new();
    for &(face, index) in faces {
        let font = m.fonts.iter().find(|f| f.font_index == index).ok_or_else(|| format!("font {index} not in {}", meta.display()))?;
        tables.insert(face, rects(font));
    }
    Ok(Fonts { atlases: vec![read_atlas(atlas)?], tables, custom: HashMap::new() })
}

fn read_atlas(atlas: &Path) -> Result<Atlas, String> {
    // row by row, keeping only R: the full RGBA image would be 128 MB
    let file = std::fs::File::open(atlas).map_err(|e| format!("{}: {e}", atlas.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if reader.info().interlaced {
        return Err("interlaced atlas".into());
    }
    let channels = reader.output_color_type().0.samples();
    let mut r = Vec::with_capacity(w * h);
    while let Some(row) = reader.next_row().map_err(|e| e.to_string())? {
        r.extend(row.data().iter().step_by(channels));
    }
    if r.len() != w * h {
        return Err(format!("atlas: {} of {} pixels", r.len(), w * h));
    }
    Ok(Atlas { px: r, w, h })
}

/// Where a glyph sits vertically (see the module notes).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Baseline,
    Descender,
    Middle,
    High,
}

fn place(ch: char) -> Place {
    match ch {
        'g' | 'j' | 'p' | 'q' | 'y' | ',' | ';' => Place::Descender,
        '-' | '–' | '—' | '~' | '=' | '+' | '*' | '·' | '•' => Place::Middle,
        '\'' | '"' | '`' | '^' | '°' | '‘' | '’' | '“' | '”' => Place::High,
        _ => Place::Baseline,
    }
}

/// A glyph in the cache texture: its ink (atlas px) and where it sits.
#[derive(Clone, Copy)]
struct Glyph {
    uv0: [f32; 2],
    uv1: [f32; 2],
    w: f32,
    h: f32,
    place: Place,
}

/// Per face: the ink heights of '0' (text height) and 'x' (x-height), atlas px.
#[derive(Clone, Copy)]
struct Reference {
    ink_h: f32,
    x_h: f32,
}

struct Cache {
    rgba: Vec<u8>,
    glyphs: HashMap<(Face, u32), Option<Glyph>>,
    refs: HashMap<Face, Reference>,
    x: usize,
    y: usize,
    row_h: usize,
    dirty: bool,
    texture: Option<TextureId>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn coverage(v: u8) -> u8 {
    ((v as f32 - EDGE) * (255.0 / RAMP)).clamp(0.0, 255.0) as u8
}

impl Cache {
    fn new() -> Self {
        Cache { rgba: vec![0; CACHE_W * CACHE_H * 4], glyphs: HashMap::new(), refs: HashMap::new(), x: 1, y: 1, row_h: 0, dirty: false, texture: None }
    }

    fn reference(&mut self, fonts: &Fonts, face: Face) -> Option<Reference> {
        if let Some(r) = self.refs.get(&face) {
            return Some(*r);
        }
        let ink_h = ink(fonts, face, '0' as u32)?.4 as f32;
        let x_h = ink(fonts, face, 'x' as u32).map_or(ink_h * 0.72, |i| i.4 as f32);
        let r = Reference { ink_h, x_h };
        self.refs.insert(face, r);
        Some(r)
    }

    /// The glyph, cut into the cache if it is not there yet (None: no such glyph or no room).
    fn glyph(&mut self, fonts: &Fonts, face: Face, cp: u32) -> Option<Glyph> {
        if let Some(g) = self.glyphs.get(&(face, cp)) {
            return *g;
        }
        let g = self.cut(fonts, face, cp);
        self.glyphs.insert((face, cp), g);
        g
    }

    fn cut(&mut self, fonts: &Fonts, face: Face, cp: u32) -> Option<Glyph> {
        let (src, ix, iy, iw, ih) = ink(fonts, face, cp)?;
        // in the face's units (its '0'): a glyph of the other atlas scaled by the two atlases' '0'
        let k = match (ink(fonts, face, '0' as u32), ink_in(fonts, face, src, '0' as u32)) {
            (Some(r), Some(own)) if r.0 != src => r.4 as f32 / own.3.max(1) as f32,
            _ => 1.0,
        };
        let a = &fonts.atlases[src];
        if self.x + iw + 1 > CACHE_W {
            self.x = 1;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.y + ih + 1 > CACHE_H {
            return None;
        }
        let (cx, cy) = (self.x, self.y);
        for j in 0..ih {
            for i in 0..iw {
                let o = ((cy + j) * CACHE_W + cx + i) * 4;
                self.rgba[o..o + 4].copy_from_slice(&[255, 255, 255, coverage(a.px[(iy + j) * a.w + ix + i])]);
            }
        }
        self.x += iw + 1;
        self.row_h = self.row_h.max(ih);
        self.dirty = true;
        let (tw, th) = (CACHE_W as f32, CACHE_H as f32);
        Some(Glyph {
            uv0: [cx as f32 / tw, cy as f32 / th],
            uv1: [(cx + iw) as f32 / tw, (cy + ih) as f32 / th],
            w: iw as f32 * k,
            h: ih as f32 * k,
            place: char::from_u32(cp).map_or(Place::Baseline, place),
        })
    }
}

/// A glyph's ink (atlas, x, y, w, h), after the coverage threshold: the custom font's for printable
/// ASCII when the face has one, else Apex's.
fn ink(fonts: &Fonts, face: Face, cp: u32) -> Option<(usize, usize, usize, usize, usize)> {
    let src = usize::from(fonts.atlases.len() > 1 && fonts.custom.get(&face).is_some_and(|t| t.contains_key(&cp)));
    ink_in(fonts, face, src, cp).map(|(x, y, w, h)| (src, x, y, w, h))
}

/// A glyph's ink (x, y, w, h) in one atlas (0 Apex's, 1 the custom one).
fn ink_in(fonts: &Fonts, face: Face, src: usize, cp: u32) -> Option<(usize, usize, usize, usize)> {
    let table = if src == 0 { &fonts.tables } else { &fonts.custom };
    let [x, y, w, h] = *table.get(&face)?.get(&cp)?;
    let a = fonts.atlases.get(src)?;
    if x >= a.w || y >= a.h {
        return None;
    }
    // some cells at the atlas's edge run past it by a pixel or so (卫: y 3851 + h 54 of 3904 rows);
    // their ink is inside
    let (w, h) = (w.min(a.w - x), h.min(a.h - y));
    let on = |i: usize, j: usize| coverage(a.px[(y + j) * a.w + x + i]) > 0;
    let cols: Vec<usize> = (0..w).filter(|&i| (0..h).any(|j| on(i, j))).collect();
    let rows: Vec<usize> = (0..h).filter(|&j| (0..w).any(|i| on(i, j))).collect();
    let (left, right, top, bottom) = (*cols.first()?, *cols.last()?, *rows.first()?, *rows.last()?);
    Some((x + left, y + top, right + 1 - left, bottom + 1 - top))
}

/// Render thread, before each frame: upload the cache if glyphs were added.
pub fn upload(render_context: &mut dyn RenderContext) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(c) = cache.as_mut() else { return };
    if !c.dirty {
        return;
    }
    let result = match c.texture {
        Some(id) => render_context.replace_texture(id, &c.rgba, CACHE_W as u32, CACHE_H as u32),
        None => render_context.load_texture(&c.rgba, CACHE_W as u32, CACHE_H as u32).map(|id| {
            c.texture = Some(id);
        }),
    };
    match result {
        Ok(()) => c.dirty = false,
        Err(e) => log(format!("hud: glyph cache upload failed: {e:?}")),
    }
}

/// The width `draw` gives `text` at this height (0 until the fonts are loaded).
pub fn width(face: Face, text: &str, height: f32) -> f32 {
    let fonts = FONTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(fonts) = fonts else { return 0.0 };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(c) = cache.as_mut() else { return 0.0 };
    let Some(r) = c.reference(&fonts, face) else { return 0.0 };
    let (scale, gap) = (height / r.ink_h, height * 0.08);
    let mut last_ink = false;
    let mut w = 0.0;
    for ch in text.chars() {
        let (ink, advance) = match ch {
            ' ' => (false, height * 0.4),
            _ => match c.glyph(&fonts, face, ch as u32) {
                Some(g) => (true, g.w * scale + gap),
                None => (false, height * 0.55),
            },
        };
        w += advance;
        last_ink = ink;
    }
    w - if last_ink { gap } else { 0.0 }
}

/// Draws `text` with the top of a '0' at `pos[1]` and a '0' `height` px tall; returns the width.
/// Nothing is drawn until the fonts are loaded (a frame later for new glyphs).
pub fn draw(dl: &DrawListMut, face: Face, text: &str, pos: [f32; 2], height: f32, color: [f32; 4], align: Align) -> f32 {
    let fonts = FONTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some(fonts) = fonts else { return 0.0 };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = cache.get_or_insert_with(|| {
        // ASCII up front: a glyph cut while drawing shows from the next frame (the cache uploads
        // before each frame), so a new digit in the ammo count would blink out once
        let mut c = Cache::new();
        for &face in fonts.tables.keys() {
            for cp in 33..127 {
                c.glyph(&fonts, face, cp);
            }
        }
        c
    });
    let Some(r) = c.reference(&fonts, face) else { return 0.0 };
    let scale = height / r.ink_h;
    let gap = height * 0.08;
    let glyphs: Vec<(Option<Glyph>, f32)> = text
        .chars()
        .map(|ch| match ch {
            ' ' => (None, height * 0.4),
            _ => match c.glyph(&fonts, face, ch as u32) {
                Some(g) => (Some(g), g.w * scale + gap),
                None => (None, height * 0.55),
            },
        })
        .collect();
    let width: f32 = glyphs.iter().map(|g| g.1).sum::<f32>() - if glyphs.last().is_some_and(|g| g.0.is_some()) { gap } else { 0.0 };
    let mut x = match align {
        Align::Left => pos[0],
        Align::Center => pos[0] - width * 0.5,
        Align::Right => pos[0] - width,
    };
    let baseline = pos[1] + height;
    let x_top = baseline - r.x_h * scale;
    if let Some(tex) = c.texture {
        for (g, advance) in &glyphs {
            if let Some(g) = g {
                let gh = g.h * scale;
                let y = match g.place {
                    Place::Baseline => baseline - gh,
                    Place::Descender => x_top,
                    Place::Middle => (baseline + x_top) * 0.5 - gh * 0.5,
                    Place::High => pos[1],
                };
                dl.add_image(tex, [x, y], [x + g.w * scale, y + gh]).uv_min(g.uv0).uv_max(g.uv1).col(color).build();
            }
            x += advance;
        }
    }
    width
}

/// `draw` with a dark edge: the text drawn `edge` px off in eight directions in `outline` first
/// (Apex's damage numbers and name plates have one; its SDF shader's outline is not recovered).
#[allow(clippy::too_many_arguments)]
pub fn draw_outlined(dl: &DrawListMut, face: Face, text: &str, pos: [f32; 2], height: f32, color: [f32; 4], outline: [f32; 4], edge: f32, align: Align) -> f32 {
    if outline[3] > 0.0 && edge > 0.0 {
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)] {
            draw(dl, face, text, [pos[0] + dx * edge, pos[1] + dy * edge], height, outline, align);
        }
    }
    draw(dl, face, text, pos, height, color, align)
}
