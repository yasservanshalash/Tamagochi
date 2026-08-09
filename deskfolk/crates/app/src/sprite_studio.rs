//! Sprite Studio — a **developer-only** tool for regenerating a character's
//! sprites from its own specs, in place, with a Deskfolk-styled UI.
//!
//! Two ways to work:
//!  - **Flat sets** — the classic `sprites/` catalogue or the modular
//!    `modular_sheet/sheets/` parts: list, regenerate one, accept in place.
//!  - **Batches (advanced)** — the flux pack's real workflow: each slot has one
//!    or more *batches*, and a batch is a 4×2 **sheet** of variant cells with
//!    its own prompt (GLOBAL_FLUX + the batch text). Generate the sheet, then
//!    slice it into the individual variant parts and accept the good cells.
//!
//! Gated behind `DESKFOLK_DEV=1` (or a debug build). All I/O is through Tauri
//! commands; generation reuses the shared `OPENROUTER_API_KEY` from `brain/.env`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use image::{DynamicImage, GenericImageView, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::{config, paths, settings};

const MODEL_ENV: &str = "DESKFOLK_IMAGE_MODEL";
const DEFAULT_MODEL: &str = "google/gemini-3.1-flash-image"; // Nano Banana 2
const API_BASE: &str = "https://openrouter.ai/api/v1";
const CANVAS: u32 = 320;

/// Is the developer surface enabled? Env flag first (works in release), else a
/// debug build. Mirrors the `DESKFOLK_MODULAR` precedent.
pub fn enabled() -> bool {
    matches!(
        std::env::var("DESKFOLK_DEV").ok().as_deref(),
        Some("1") | Some("true") | Some("on") | Some("yes")
    ) || cfg!(debug_assertions)
}

#[tauri::command]
pub fn dev_mode() -> bool {
    enabled()
}

// --- Character / paths -----------------------------------------------------

fn character_id(app: &AppHandle) -> String {
    std::env::var("DESKFOLK_CHARACTER").unwrap_or_else(|_| settings::load(app).character)
}

/// The active character's package dir — the one the running app loads.
fn character_root(app: &AppHandle) -> PathBuf {
    paths::characters_dir(app).join(character_id(app))
}

/// The in-repo source copy, if it differs from the active (resource) dir.
fn source_root(app: &AppHandle) -> Option<PathBuf> {
    let id = character_id(app);
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters")
        .join(&id)
        .canonicalize()
        .ok()?;
    let active = character_root(app).canonicalize().ok();
    if Some(&src) == active.as_ref() || !src.is_dir() {
        None
    } else {
        Some(src)
    }
}

/// The flux-pack `prompts/` dir — source copy first (it may not be mirrored into
/// the resource dir), then the active dir.
fn flux_prompts_dir(app: &AppHandle) -> Option<PathBuf> {
    let id = character_id(app);
    let candidates = [
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../characters")
            .join(&id)
            .join("flux_pack/prompts"),
        character_root(app).join("flux_pack/prompts"),
    ];
    candidates.into_iter().find(|p| p.is_dir())
}

// --- Specs -----------------------------------------------------------------

#[derive(Serialize, Default)]
pub struct Specs {
    name: String,
    palette: Vec<String>,
    style_notes: String,
    character_notes: String,
    canvas: [u32; 2],
}

#[tauri::command]
pub fn sprite_specs(app: AppHandle) -> Specs {
    let root = character_root(&app);
    let mut specs = Specs::default();
    specs.canvas = [CANVAS, CANVAS];

    if let Ok(text) = std::fs::read_to_string(root.join("character.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(n) = v.get("name").and_then(|x| x.as_str()) {
                specs.name = n.to_string();
            }
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("modular_sheet").join("definition.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(pal) = v.pointer("/style_lock/palette").and_then(|p| p.as_array()) {
                specs.palette = pal.iter().filter_map(|c| c.as_str().map(str::to_string)).collect();
            }
            let sl = &v["style_lock"];
            let mut bits = vec![];
            if let Some(m) = sl.get("medium").and_then(|x| x.as_str()) {
                bits.push(m.to_string());
            }
            if sl.get("anti_aliasing").and_then(|x| x.as_bool()) == Some(false) {
                bits.push("no anti-aliasing, crisp pixel edges".into());
            }
            if let Some(o) = sl.pointer("/outline/type").and_then(|x| x.as_str()) {
                bits.push(format!("{o} outline"));
            }
            if let Some(s) = sl.get("silhouette").and_then(|x| x.as_str()) {
                bits.push(format!("silhouette: {s}"));
            }
            bits.push("no gradients, no blur, no glow, no drop shadow".into());
            specs.style_notes = bits.join("; ");

            let cl = &v["character_lock"];
            let mut who = vec![];
            for key in ["skin", "body_shape", "beard", "eyes", "eyebrows"] {
                if let Some(s) = cl.get(key).and_then(|x| x.as_str()) {
                    who.push(format!("{key}: {s}"));
                }
            }
            if let Some(clothes) = cl.get("default_clothing").and_then(|x| x.as_object()) {
                for (k, val) in clothes {
                    if let Some(s) = val.as_str() {
                        who.push(format!("{k}: {s}"));
                    }
                }
            }
            specs.character_notes = who.join(", ");
        }
    }

    if specs.character_notes.is_empty() {
        specs.character_notes =
            "relaxed streetwise guy, stocky build, full dark beard, beanie, over-ear headphones, \
             brown puffer jacket, dark jeans, sneakers"
                .into();
    }
    if specs.style_notes.is_empty() {
        specs.style_notes = "pixel art; hard 1px outline; no anti-aliasing; \
                             no gradients, blur, glow or drop shadow; warm brown palette"
            .into();
    }
    specs
}

// --- Flat listing (classic / modular files) --------------------------------

#[derive(Serialize)]
pub struct SpriteEntry {
    id: String,
    label: String,
    rel_path: String,
    data_url: String,
    suggested_prompt: String,
    size: [u32; 2],
}

fn read_data_url(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

fn png_size(path: &Path) -> [u32; 2] {
    image::image_dimensions(path).map(|(w, h)| [w, h]).unwrap_or([0, 0])
}

fn iso_rules() -> &'static str {
    "Output a SINGLE sprite on a FULLY TRANSPARENT background. No text, labels, \
     grid, frame, watermark, or drop shadow. One centered instance, trimmed tight. \
     Match the palette and style EXACTLY."
}

fn seed_prompt(specs: &Specs, role: &str) -> String {
    let pal = if specs.palette.is_empty() {
        String::new()
    } else {
        format!(" Palette (stay within): {}.", specs.palette.join(", "))
    };
    format!(
        "{name} — {who}. Style: {style}.{pal} Draw: {role}. {iso}",
        name = if specs.name.is_empty() { "Character" } else { &specs.name },
        who = specs.character_notes,
        style = specs.style_notes,
        role = role,
        iso = iso_rules(),
    )
}

#[tauri::command]
pub fn list_sprites(app: AppHandle, set: String) -> Result<Vec<SpriteEntry>, String> {
    let root = character_root(&app);
    let specs = sprite_specs(app.clone());
    let dir = match set.as_str() {
        "modular" => root.join("modular_sheet").join("sheets"),
        _ => root.join("sprites"),
    };
    if !dir.is_dir() {
        return Err(format!("no sprites at {}", dir.display()));
    }

    let mut labels: std::collections::HashMap<String, String> = Default::default();
    if set == "modular" {
        if let Ok(text) = std::fs::read_to_string(root.join("modular_sheet").join("manifest.json")) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(insts) = v.get("instances").and_then(|x| x.as_array()) {
                    for inst in insts {
                        let src = inst.get("source").and_then(|x| x.as_str()).unwrap_or("");
                        let stem = Path::new(src).file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let id = inst.get("id").and_then(|x| x.as_str()).unwrap_or("");
                        if !stem.is_empty() && !id.is_empty() {
                            labels.entry(stem.to_string()).or_default().push_str(&format!("{id} "));
                        }
                    }
                }
            }
        }
    }

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("png"))
        .collect();
    files.sort();

    let mut out = vec![];
    for path in files {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if stem.is_empty() {
            continue;
        }
        let label = labels
            .get(&stem)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| stem.trim_start_matches("img_").replace('_', " "));
        let rel = if set == "modular" {
            format!("modular_sheet/sheets/{stem}.png")
        } else {
            format!("sprites/{stem}.png")
        };
        let Some(data_url) = read_data_url(&path) else { continue };
        out.push(SpriteEntry {
            id: stem.clone(),
            suggested_prompt: seed_prompt(&specs, &label),
            label,
            rel_path: rel,
            data_url,
            size: png_size(&path),
        });
    }
    Ok(out)
}

// --- Batches (advanced: sheets of variant cells) ---------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct Bounds {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

#[derive(Serialize)]
pub struct Cell {
    index: u32,
    /// Canonical variant id from the definition (positional map).
    variant: String,
    /// Human name from the batch ("3/4 Left").
    label: String,
    has_sprite: bool,
    data_url: Option<String>,
}

#[derive(Serialize)]
pub struct Batch {
    slot: String,
    batch_id: String,
    title: String,
    cols: u32,
    rows: u32,
    cell_px: u32,
    bounds: Option<Bounds>,
    pivot: Option<Vec<i64>>,
    /// The full prompt to paste: GLOBAL_FLUX + this batch's text.
    prompt: String,
    cells: Vec<Cell>,
}

struct ParsedBatch {
    title: String,
    cols: u32,
    rows: u32,
    cell_px: u32,
    bounds: Option<Bounds>,
    pivot: Option<Vec<i64>>,
    cell_names: Vec<String>,
}

fn num_after<'a>(line: &'a str, key: &str) -> Option<i64> {
    let idx = line.find(key)? + key.len();
    let rest = &line[idx..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn parse_batch(text: &str) -> ParsedBatch {
    let mut title = String::new();
    let (mut cols, mut rows, mut cell_px) = (4u32, 2u32, CANVAS);
    let mut bounds = None;
    let mut pivot = None;
    let mut cell_names = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        if title.is_empty() && line.starts_with("# ") {
            title = line[2..].trim().to_string();
        } else if line.starts_with("Grid:") {
            if let Some(c) = num_after(line, "Grid: ") {
                cols = c as u32;
            }
            if let Some(r) = num_after(line, "columns × ").or_else(|| num_after(line, "x ")) {
                rows = r as u32;
            }
        } else if line.starts_with("Cell size:") {
            if let Some(c) = num_after(line, "Cell size: ") {
                cell_px = c as u32;
            }
        } else if line.starts_with("Bounds in each cell:") {
            let (x, y, w, h) = (
                num_after(line, "x="),
                num_after(line, "y="),
                num_after(line, "width="),
                num_after(line, "height="),
            );
            if let (Some(x), Some(y), Some(width), Some(height)) = (x, y, w, h) {
                bounds = Some(Bounds { x, y, width, height });
            }
        } else if line.starts_with("Pivot in each cell:") {
            if let (Some(x), Some(y)) = (num_after(line, "x="), num_after(line, "y=")) {
                pivot = Some(vec![x, y]);
            }
        } else if let Some(rest) = line.strip_prefix("Cell ") {
            // "Cell 3: Open Palm"
            if let Some((_, name)) = rest.split_once(':') {
                cell_names.push(name.trim().to_string());
            }
        }
    }
    ParsedBatch { title, cols, rows, cell_px, bounds, pivot, cell_names }
}

fn slot_prompt_subdir(slot: &str) -> String {
    match slot {
        "props" => "props_effects/props".into(),
        "effects" => "props_effects/effects".into(),
        other => format!("body_parts/{other}"),
    }
}

/// Every flux-pack batch, enriched with the real per-batch prompt, positional
/// variant mapping, and per-cell has-sprite/missing state.
#[tauri::command]
pub fn batches(app: AppHandle) -> Result<Vec<Batch>, String> {
    let root = character_root(&app);
    let modular = root.join("modular_sheet");
    let def: Value = serde_json::from_str(
        &std::fs::read_to_string(modular.join("definition.json"))
            .map_err(|e| format!("no definition.json: {e}"))?,
    )
    .map_err(|e| e.to_string())?;
    let prompts = flux_prompts_dir(&app).ok_or("flux_pack/prompts not found")?;
    let global = std::fs::read_to_string(prompts.join("GLOBAL_FLUX.md")).unwrap_or_default();

    // Existing instances.
    let mut have: std::collections::HashMap<String, String> = Default::default();
    if let Ok(t) = std::fs::read_to_string(modular.join("manifest.json")) {
        if let Ok(mv) = serde_json::from_str::<Value>(&t) {
            if let Some(insts) = mv.get("instances").and_then(|x| x.as_array()) {
                for i in insts {
                    if let (Some(id), Some(src)) = (
                        i.get("id").and_then(|x| x.as_str()),
                        i.get("source").and_then(|x| x.as_str()),
                    ) {
                        have.insert(id.to_string(), src.to_string());
                    }
                }
            }
        }
    }

    let slots = def.get("slots").and_then(|s| s.as_object()).ok_or("no slots")?;
    let mut out = vec![];
    for (slot, info) in slots {
        let variants: Vec<String> = info
            .get("variants")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let def_bounds = info.get("default_bounds").and_then(|b| {
            Some(Bounds {
                x: b.get("x")?.as_i64()?,
                y: b.get("y")?.as_i64()?,
                width: b.get("width")?.as_i64()?,
                height: b.get("height")?.as_i64()?,
            })
        });
        let def_pivot = info
            .get("pivot")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_i64()).collect::<Vec<_>>());

        let dir = prompts.join(slot_prompt_subdir(slot));
        if !dir.is_dir() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
            .collect();
        files.sort();

        let mut offset = 0usize; // running index into `variants`
        for file in files {
            let batch_id = file.file_stem().and_then(|s| s.to_str()).unwrap_or("batch").to_string();
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let p = parse_batch(&text);
            let mut cells = vec![];
            let mut real = 0usize; // real (non-padding) cells seen in this batch
            for (i, label) in p.cell_names.iter().enumerate() {
                let l = label.to_lowercase();
                if l.contains("unused") || l.contains("transparent") {
                    continue; // padding cell — skip, but its grid slot stays empty
                }
                let variant = variants.get(offset + real).cloned().unwrap_or_else(|| {
                    label.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "_")
                });
                let id = format!("{slot}/{variant}");
                let (has_sprite, data_url) = match have.get(&id) {
                    Some(src) => (true, read_data_url(&modular.join(src))),
                    None => (false, None),
                };
                cells.push(Cell { index: i as u32, variant, label: label.clone(), has_sprite, data_url });
                real += 1;
            }
            offset += real;

            let prompt = if global.is_empty() {
                text.clone()
            } else {
                format!("{global}\n\n{text}")
            };
            out.push(Batch {
                slot: slot.clone(),
                batch_id,
                title: if p.title.is_empty() { slot.clone() } else { p.title },
                cols: p.cols,
                rows: p.rows,
                cell_px: p.cell_px,
                bounds: p.bounds.or_else(|| def_bounds.clone()),
                pivot: p.pivot.or_else(|| def_pivot.clone()),
                prompt,
                cells,
            });
        }
    }
    Ok(out)
}

/// The master reference sheet as a data URL — attached to every generation so
/// the model keeps his identity. Prefers the ultimate pack's copy.
#[tauri::command]
pub fn reference_sheet(app: AppHandle) -> Option<String> {
    let candidates = [
        Some(character_root(&app).join("ultimate_pack/master_reference_sheet.png")),
        flux_prompts_dir(&app).map(|p| p.join("../references/master_reference_sheet.png")),
        Some(character_root(&app).join("flux_pack/references/master_reference_sheet.png")),
    ];
    for c in candidates.into_iter().flatten() {
        if c.is_file() {
            if let Some(u) = read_data_url(&c) {
                return Some(u);
            }
        }
    }
    None
}

/// The ultimate pack's prompt text for one slot: every `variants_*.md` found in
/// a prompt_library directory whose path contains the slot name, concatenated.
/// This is the DEFAULT prompt source for the part editor.
#[tauri::command]
pub fn slot_prompt(app: AppHandle, slot: String) -> Option<String> {
    let lib = character_root(&app).join("ultimate_pack/prompt_library");
    if !lib.is_dir() {
        return None;
    }
    // Walk the library, collect .md files under a directory named after the slot
    // ("effects"/"props" live directly under modules/).
    fn walk(dir: &Path, slot: &str, hit: bool, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if p.is_dir() {
                    walk(&p, slot, hit || name == slot, out);
                } else if hit && name.ends_with(".md") {
                    out.push(p);
                }
            }
        }
    }
    let mut files = vec![];
    walk(&lib, &slot, false, &mut files);
    files.sort();
    if files.is_empty() {
        return None;
    }
    let text = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    Some(text)
}

/// Current image-output models on OpenRouter (id + label, newest first) so the
/// picker never goes stale. Falls back to the built-in default on error.
#[tauri::command]
pub async fn image_models() -> Vec<[String; 2]> {
    let fetched = tauri::async_runtime::spawn_blocking(|| {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .ok()?;
        let v: Value = client
            .get(format!("{API_BASE}/models"))
            .send()
            .ok()?
            .json()
            .ok()?;
        let mut models: Vec<(i64, String, String)> = v
            .get("data")?
            .as_array()?
            .iter()
            .filter(|m| {
                m.pointer("/architecture/output_modalities")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().any(|o| o.as_str() == Some("image")))
                    .unwrap_or(false)
            })
            .filter_map(|m| {
                let id = m.get("id")?.as_str()?.to_string();
                if id.starts_with("openrouter/") {
                    return None; // routers, not models
                }
                let name = m.get("name").and_then(|x| x.as_str()).unwrap_or(&id).to_string();
                let created = m.get("created").and_then(|x| x.as_i64()).unwrap_or(0);
                Some((created, id, name))
            })
            .collect();
        models.sort_by(|a, b| b.0.cmp(&a.0));
        Some(models.into_iter().map(|(_, id, name)| [id, name]).collect::<Vec<_>>())
    })
    .await
    .ok()
    .flatten();
    let mut list =
        fetched.unwrap_or_else(|| vec![[DEFAULT_MODEL.to_string(), "Nano Banana 2 (default)".to_string()]]);
    // FLUX lives on Black Forest Labs' own API (not OpenRouter); the `bfl:`
    // prefix routes there. Needs BFL_API_KEY in brain/.env.
    let has_bfl = config::secret("BFL_API_KEY").filter(|k| !k.trim().is_empty()).is_some();
    let tag = if has_bfl { "" } else { " — needs BFL_API_KEY" };
    for (id, name) in [
        ("bfl:flux-2-pro", "FLUX.2 Pro (BFL)"),
        ("bfl:flux-2-flex", "FLUX.2 Flex (BFL)"),
        ("bfl:flux-kontext-max", "FLUX Kontext Max (BFL, image editing)"),
    ] {
        list.push([id.to_string(), format!("{name}{tag}")]);
    }
    list
}

// --- State accept: a whole generated character state -------------------------
//
// The reliable pipeline: the model draws the WHOLE character in a state
// (sitting, sleeping, waving…) — the granularity where identity survives —
// and we register it one of two ways:
//  - "modular": slice at the proven neck/hem seams into head/torso/legs and
//    bind each as a variant named after the state (the reference-sheet method).
//  - "classic": save it as a plain full-frame sprite (sprites/img_y_<name>.png),
//    the classical one-sheet-per-motion path that powers the walk today.

#[derive(Deserialize)]
pub struct StateReq {
    name: String,
    sheet: String,
    mode: String, // "modular" | "classic"
}

#[tauri::command]
pub fn state_accept(app: AppHandle, req: StateReq) -> Result<Vec<String>, String> {
    let bytes = decode_data_url(&req.sheet)?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("bad image: {e}"))?;
    let mut rgba = img.to_rgba8();
    key_background(&mut rgba);

    // Trim to the character.
    let (w, h) = rgba.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if rgba.get_pixel(x, y)[3] > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return Err("image is empty after background keying".into());
    }
    let body = DynamicImage::ImageRgba8(rgba).crop_imm(x0, y0, x1 - x0, y1 - y0);
    let safe = |s: &str| {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect::<String>()
    };
    let name = safe(&req.name);
    if name.is_empty() {
        return Err("give the state a name".into());
    }
    let root = character_root(&app);

    if req.mode == "classic" {
        // One full-frame sprite, the classical way.
        let fname = format!("img_y_{name}.png");
        let target = root.join("sprites").join(&fname);
        if target.exists() {
            let b = backup_path(&root, &format!("sprites/{fname}"));
            if let Some(p) = b.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&target, &b);
        }
        let mut png = std::io::Cursor::new(Vec::new());
        body.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        let png = png.into_inner();
        std::fs::write(&target, &png).map_err(|e| e.to_string())?;
        if let Some(src) = source_root(&app) {
            let _ = std::fs::write(src.join("sprites").join(&fname), &png);
        }
        tracing::info!("sprite studio: classic state saved {fname}");
        return Ok(vec![format!("sprites/{fname}")]);
    }

    // Modular: normalise to the 300-unit character height used by the sheet
    // slices, then cut at the proven seam fractions (neck ~26%, hem ~65%).
    const TARGET_H: u32 = 300;
    const TOP_Y: u32 = 10;
    let scale = TARGET_H as f64 / body.height() as f64;
    let sw = ((body.width() as f64 * scale).round() as u32).max(1);
    let scaled = body
        .resize_exact(sw, TARGET_H, image::imageops::FilterType::Nearest)
        .to_rgba8();
    let seams = [
        (0u32, (TARGET_H as f64 * 0.26) as u32, "head_base"),
        ((TARGET_H as f64 * 0.26) as u32, (TARGET_H as f64 * 0.65) as u32, "torso"),
        ((TARGET_H as f64 * 0.65) as u32, TARGET_H, "left_leg"),
    ];
    let modular = root.join("modular_sheet");
    std::fs::create_dir_all(modular.join("sheets")).map_err(|e| e.to_string())?;
    let src_root = source_root(&app);
    let cx = (CANVAS / 2) as i64;
    let mut written = vec![];
    for (ya, yb, slot) in seams {
        let band = image::DynamicImage::ImageRgba8(scaled.clone()).crop_imm(0, ya, sw, yb - ya);
        let mut png = std::io::Cursor::new(Vec::new());
        band.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        let png = png.into_inner();
        let fname = format!("{slot}__{name}.png");
        let rel = format!("sheets/{fname}");
        std::fs::write(modular.join("sheets").join(&fname), &png).map_err(|e| e.to_string())?;
        let cb = serde_json::json!({
            "x": cx - (sw as i64) / 2,
            "y": TOP_Y + ya,
            "width": sw,
            "height": yb - ya,
        });
        let region = serde_json::json!({ "x": 0, "y": 0, "w": sw, "h": yb - ya });
        let pivot = serde_json::json!([160, 160]);
        upsert_instance(&modular, slot, &name, &rel, region.clone(), cb.clone(), pivot.clone())?;
        if let Some(src) = &src_root {
            let sm = src.join("modular_sheet");
            let _ = std::fs::create_dir_all(sm.join("sheets"));
            let _ = std::fs::write(sm.join("sheets").join(&fname), &png);
            let _ = upsert_instance(&sm, slot, &name, &rel, region, cb, pivot);
        }
        written.push(format!("{slot}/{name}"));
    }
    tracing::info!("sprite studio: modular state '{name}' registered ({} bands)", written.len());
    Ok(written)
}

// --- Region accept: pick a part out of a generated parts sheet --------------

#[derive(Deserialize)]
pub struct RegionReq {
    slot: String,
    variant: String,
    /// Data URL of the generated sheet.
    sheet: String,
    /// Selected rectangle on the sheet, in sheet pixels.
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// Crop a hand-picked region out of a generated parts sheet, key its
/// background, trim to content, fit it into the slot's canonical bounds on a
/// 320 canvas, then write + upsert the manifest like any accepted part.
#[tauri::command]
pub fn region_accept(app: AppHandle, req: RegionReq) -> Result<String, String> {
    let bytes = decode_data_url(&req.sheet)?;
    let sheet = image::load_from_memory(&bytes).map_err(|e| format!("bad sheet: {e}"))?;
    let (sw, sh) = sheet.dimensions();
    if req.w == 0 || req.h == 0 || req.x + req.w > sw || req.y + req.h > sh {
        return Err("selection is outside the sheet".into());
    }
    let mut crop = sheet.crop_imm(req.x, req.y, req.w, req.h).to_rgba8();
    key_background(&mut crop);

    // Trim to the opaque content.
    let (cw, ch) = crop.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (cw, ch, 0u32, 0u32);
    for y in 0..ch {
        for x in 0..cw {
            if crop.get_pixel(x, y)[3] > 8 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return Err("selection is empty after background keying".into());
    }
    let trimmed = image::DynamicImage::ImageRgba8(crop).crop_imm(x0, y0, x1 - x0, y1 - y0);

    // Fit into the slot's canonical bounds (aspect-preserving, centered).
    let root = character_root(&app);
    let modular = root.join("modular_sheet");
    let def: Value = serde_json::from_str(
        &std::fs::read_to_string(modular.join("definition.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let sb = def.pointer(&format!("/slots/{}/default_bounds", req.slot)).cloned();
    let pivot = def
        .pointer(&format!("/slots/{}/pivot", req.slot))
        .cloned()
        .unwrap_or_else(|| serde_json::json!([160, 160]));
    let (bx, by, bw, bh) = match &sb {
        Some(b) => (
            b.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            b.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            b.get("width").and_then(|v| v.as_i64()).unwrap_or(64) as u32,
            b.get("height").and_then(|v| v.as_i64()).unwrap_or(64) as u32,
        ),
        None => (110, 110, 100, 100),
    };
    let (tw, th) = (trimmed.width().max(1), trimmed.height().max(1));
    let scale = (bw as f64 / tw as f64).min(bh as f64 / th as f64);
    let (fw, fh) = (
        ((tw as f64 * scale).round() as u32).max(1),
        ((th as f64 * scale).round() as u32).max(1),
    );
    let fitted = trimmed.resize_exact(fw, fh, image::imageops::FilterType::Nearest);
    let mut canvas = RgbaImage::new(CANVAS, CANVAS);
    let ox = bx + (bw - fw) / 2;
    let oy = by + (bh - fh) / 2;
    image::imageops::overlay(&mut canvas, &fitted.to_rgba8(), ox as i64, oy as i64);

    let mut png = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let png = png.into_inner();

    let safe = |s: &str| s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>();
    let fname = format!("{}__{}.png", safe(&req.slot), safe(&req.variant));
    let rel = format!("sheets/{fname}");
    let target = modular.join("sheets").join(&fname);
    std::fs::create_dir_all(modular.join("sheets")).map_err(|e| e.to_string())?;
    if target.exists() {
        let b = backup_path(&root, &format!("modular_sheet/{rel}"));
        if let Some(p) = b.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::copy(&target, &b);
    }
    std::fs::write(&target, &png).map_err(|e| format!("write failed: {e}"))?;

    let region = serde_json::json!({ "x": 0, "y": 0, "w": CANVAS, "h": CANVAS });
    let canonical = serde_json::json!({ "x": 0, "y": 0, "width": CANVAS, "height": CANVAS });
    upsert_instance(&modular, &req.slot, &req.variant, &rel, region.clone(), canonical.clone(), pivot.clone())?;
    if let Some(src) = source_root(&app) {
        let sm = src.join("modular_sheet");
        let st = sm.join("sheets").join(&fname);
        if let Some(p) = st.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&st, &png);
        let _ = upsert_instance(&sm, &req.slot, &req.variant, &rel, region, canonical, pivot);
    }
    tracing::info!("sprite studio: region-accepted {}/{}", req.slot, req.variant);
    Ok(target.display().to_string())
}

// --- Assembly (live builder data) ------------------------------------------

/// Back-to-front slot draw order for the live builder. Not every slot has parts
/// yet; the UI simply skips empty ones. Kept here so the browser composites him
/// the same way the engine layers him.
const BUILDER_ORDER: &[&str] = &[
    "left_leg", "right_leg", "left_boot", "right_boot", "pelvis", "torso",
    "left_upper_arm", "right_upper_arm", "left_forearm", "right_forearm",
    "left_hand", "right_hand", "head_base", "eyes", "eyebrows", "nose", "mouth",
    "beard", "beanie", "headphones", "props", "effects",
];

#[derive(Serialize)]
pub struct PartInstance {
    variant: String,
    data_url: String,
    region: Value,
    canonical_bounds: Value,
}

#[derive(Serialize)]
pub struct SlotParts {
    slot: String,
    /// The slot's spec placement (definition `default_bounds` / `pivot`) — lets
    /// the builder offer every slot as a clickable zone even with no sprite.
    bounds: Option<Value>,
    pivot: Option<Value>,
    instances: Vec<PartInstance>,
}

#[derive(Serialize)]
pub struct Assembly {
    canvas: [u32; 2],
    /// Slots in back-to-front draw order (only those the definition declares).
    order: Vec<String>,
    slots: Vec<SlotParts>,
}

/// Everything the browser needs to composite the modular character live: the
/// canvas size, the slot draw order, and every slot's available part instances
/// (variant + image + region + placement) read from the manifest.
#[tauri::command]
pub fn assembly(app: AppHandle) -> Result<Assembly, String> {
    let root = character_root(&app);
    let modular = root.join("modular_sheet");
    let def: Value = serde_json::from_str(
        &std::fs::read_to_string(modular.join("definition.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let canvas = [
        def.pointer("/coordinate_system/canvas/width").and_then(|x| x.as_u64()).unwrap_or(320) as u32,
        def.pointer("/coordinate_system/canvas/height").and_then(|x| x.as_u64()).unwrap_or(320) as u32,
    ];
    let declared: std::collections::HashSet<String> = def
        .get("slots")
        .and_then(|s| s.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    // Group manifest instances by slot.
    let mut by_slot: std::collections::HashMap<String, Vec<PartInstance>> = Default::default();
    if let Ok(t) = std::fs::read_to_string(modular.join("manifest.json")) {
        if let Ok(mv) = serde_json::from_str::<Value>(&t) {
            if let Some(insts) = mv.get("instances").and_then(|x| x.as_array()) {
                for i in insts {
                    let slot = i.get("slot").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let variant = i.get("variant").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let src = i.get("source").and_then(|x| x.as_str()).unwrap_or("");
                    if slot.is_empty() || variant.is_empty() || variant.starts_with("without_") {
                        continue;
                    }
                    let Some(data_url) = read_data_url(&modular.join(src)) else { continue };
                    by_slot.entry(slot).or_default().push(PartInstance {
                        variant,
                        data_url,
                        region: i.get("region").cloned().unwrap_or(Value::Null),
                        canonical_bounds: i.get("canonical_bounds").cloned().unwrap_or(Value::Null),
                    });
                }
            }
        }
    }

    // Draw order: the curated list, filtered to declared slots, then any extras.
    let mut order: Vec<String> = BUILDER_ORDER
        .iter()
        .map(|s| s.to_string())
        .filter(|s| declared.contains(s))
        .collect();
    for s in declared {
        if !order.contains(&s) {
            order.push(s);
        }
    }

    let slot_defs = def.get("slots").and_then(|s| s.as_object());
    let slots = order
        .iter()
        .map(|slot| SlotParts {
            slot: slot.clone(),
            bounds: slot_defs.and_then(|o| o.get(slot)).and_then(|s| s.get("default_bounds")).cloned(),
            pivot: slot_defs.and_then(|o| o.get(slot)).and_then(|s| s.get("pivot")).cloned(),
            instances: by_slot.remove(slot).unwrap_or_default(),
        })
        .collect();

    Ok(Assembly { canvas, order, slots })
}

// --- Rig (skeleton + gestures from the definition) -------------------------

/// The bone graph of `skeleton_front` — parent→child pairs, static because the
/// spec's joint names are fixed by the schema.
const BONES: &[(&str, &str)] = &[
    ("root", "pelvis"), ("pelvis", "chest"), ("chest", "neck"), ("neck", "head"),
    ("chest", "left_shoulder"), ("left_shoulder", "left_elbow"), ("left_elbow", "left_wrist"),
    ("chest", "right_shoulder"), ("right_shoulder", "right_elbow"), ("right_elbow", "right_wrist"),
    ("pelvis", "left_hip"), ("left_hip", "left_knee"), ("left_knee", "left_ankle"),
    ("pelvis", "right_hip"), ("right_hip", "right_knee"), ("right_knee", "right_ankle"),
];

/// Which skeleton joint each slot hangs off. Moving a joint moves these parts.
const ATTACH: &[(&str, &str)] = &[
    ("torso", "chest"), ("pelvis", "pelvis"),
    ("head_base", "head"), ("eyes", "head"), ("eyebrows", "head"), ("nose", "head"),
    ("mouth", "head"), ("beard", "head"), ("beanie", "head"), ("headphones", "head"),
    ("left_upper_arm", "left_shoulder"), ("left_forearm", "left_elbow"), ("left_hand", "left_wrist"),
    ("right_upper_arm", "right_shoulder"), ("right_forearm", "right_elbow"), ("right_hand", "right_wrist"),
    ("left_leg", "left_hip"), ("right_leg", "right_hip"),
    ("left_boot", "left_ankle"), ("right_boot", "right_ankle"),
];

#[derive(Serialize)]
pub struct Rig {
    /// Joint name -> [x, y] on the authoring canvas (the character's dimensions).
    joints: Value,
    /// Parent→child joint pairs for drawing bones.
    bones: Vec<[String; 2]>,
    /// Slot -> joint it attaches to.
    attach: Vec<[String; 2]>,
    /// Preset name -> { slot: variant } (hand sequences collapse to first frame).
    gestures: Value,
}

/// The character's rig, read from `definition.json`: `skeleton_front` is the
/// placement authority — pivots in the slot table coincide with these joints,
/// so a character with different dimensions moves the joints and every
/// attached part follows.
#[tauri::command]
pub fn rig(app: AppHandle) -> Result<Rig, String> {
    let root = character_root(&app);
    let def: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("modular_sheet").join("definition.json"))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let joints = def.get("skeleton_front").cloned().ok_or("definition has no skeleton_front")?;

    // Collapse "*_sequence" arrays to their first frame so a preset is a plain
    // slot->variant map the builder can apply in one shot.
    let mut gestures = serde_json::Map::new();
    if let Some(g) = def.get("gesture_presets").and_then(|x| x.as_object()) {
        for (name, preset) in g {
            let mut flat = serde_json::Map::new();
            if let Some(p) = preset.as_object() {
                for (k, v) in p {
                    if let Some(seq) = v.as_array() {
                        if let (Some(slot), Some(first)) = (k.strip_suffix("_sequence"), seq.first()) {
                            flat.insert(slot.to_string(), first.clone());
                        }
                    } else {
                        flat.insert(k.clone(), v.clone());
                    }
                }
            }
            gestures.insert(name.clone(), Value::Object(flat));
        }
    }

    Ok(Rig {
        joints,
        bones: BONES.iter().map(|(a, b)| [a.to_string(), b.to_string()]).collect(),
        attach: ATTACH.iter().map(|(s, j)| [s.to_string(), j.to_string()]).collect(),
        gestures: Value::Object(gestures),
    })
}

// --- Generation ------------------------------------------------------------

#[derive(Serialize)]
pub struct GenResult {
    images: Vec<String>,
}

fn ref_data_url(app: &AppHandle, set: &str, id: &str) -> Option<String> {
    let root = character_root(app);
    let path = match set {
        "modular" => root.join("modular_sheet").join("sheets").join(format!("{id}.png")),
        _ => root.join("sprites").join(format!("{id}.png")),
    };
    read_data_url(&path)
}

#[tauri::command]
pub async fn generate_sprite(
    app: AppHandle,
    prompt: String,
    set: String,
    ref_id: Option<String>,
    ref_url: Option<String>,
    n: Option<u32>,
    model: Option<String>,
) -> Result<GenResult, String> {
    let key = config::secret("OPENROUTER_API_KEY")
        .filter(|k| !k.trim().is_empty())
        .ok_or("no OPENROUTER_API_KEY found in brain/.env")?;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .or_else(|| std::env::var(MODEL_ENV).ok())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let reference = ref_url
        .filter(|u| u.starts_with("data:"))
        .or_else(|| ref_id.and_then(|id| ref_data_url(&app, &set, &id)));
    let count = n.unwrap_or(1).clamp(1, 4);

    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|e| e.to_string())?;
        let mut images = Vec::new();
        let mut last_err = String::new();
        for i in 0..count {
            let p = if count > 1 { format!("{prompt} (variation {})", i + 1) } else { prompt.clone() };
            match generate_one(&client, &key, &model, &p, reference.as_deref()) {
                Ok(url) => images.push(url),
                Err(e) => last_err = e,
            }
        }
        if images.is_empty() {
            Err(format!("generation failed: {last_err}"))
        } else {
            Ok(GenResult { images })
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Generate via the Black Forest Labs FLUX API (models prefixed `bfl:`).
/// Async submit + poll, ported from tools/generate_sprites.py::bfl_generate.
fn bfl_generate(
    client: &reqwest::blocking::Client,
    model: &str, // endpoint name, e.g. "flux-2-pro"
    prompt: &str,
    reference: Option<&str>,
) -> Result<String, String> {
    let key = config::secret("BFL_API_KEY")
        .filter(|k| !k.trim().is_empty())
        .ok_or("FLUX needs a BFL_API_KEY in brain/.env (get one at api.bfl.ai) — OpenRouter does not host FLUX image models")?;
    let base = std::env::var("BFL_API_BASE").unwrap_or_else(|_| "https://api.bfl.ai".into());
    let mut payload = serde_json::json!({
        "prompt": prompt,
        "output_format": "png",
        "prompt_upsampling": false,
        "safety_tolerance": 6,
    });
    if let Some(url) = reference {
        // BFL wants raw base64, not a data URL.
        let b64 = url.split_once(',').map(|(_, d)| d).unwrap_or(url);
        payload["input_image"] = Value::String(b64.to_string());
    }
    let submit: Value = client
        .post(format!("{base}/v1/{model}"))
        .header("x-key", &key)
        .header("Content-Type", "application/json")
        .body(payload.to_string())
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let poll_url = submit
        .get("polling_url")
        .or_else(|| submit.get("result_url"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("no polling_url from BFL: {submit}"))?
        .to_string();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let status: Value = client
            .get(&poll_url)
            .header("x-key", &key)
            .send()
            .map_err(|e| e.to_string())?
            .json()
            .map_err(|e| e.to_string())?;
        match status.get("status").and_then(|x| x.as_str()) {
            Some("Ready") => {
                let sample = status
                    .pointer("/result/sample")
                    .and_then(|x| x.as_str())
                    .ok_or("BFL Ready but no result.sample")?;
                let bytes = client
                    .get(sample)
                    .send()
                    .map_err(|e| e.to_string())?
                    .bytes()
                    .map_err(|e| e.to_string())?;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                return Ok(format!("data:image/png;base64,{b64}"));
            }
            Some(s @ ("Error" | "Failed" | "Content Moderated" | "Request Moderated")) => {
                return Err(format!("BFL generation {s}: {}", status.get("result").unwrap_or(&Value::Null)));
            }
            _ => {}
        }
    }
    Err("timed out waiting for the FLUX image".into())
}

fn generate_one(
    client: &reqwest::blocking::Client,
    key: &str,
    model: &str,
    prompt: &str,
    reference: Option<&str>,
) -> Result<String, String> {
    // FLUX routes to the BFL API; everything else goes through OpenRouter.
    if let Some(bfl_model) = model.strip_prefix("bfl:") {
        return bfl_generate(client, bfl_model, prompt, reference);
    }
    let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
    if let Some(url) = reference {
        content.push(serde_json::json!({ "type": "image_url", "image_url": { "url": url } }));
    }
    let body = serde_json::json!({
        "model": model,
        "modalities": ["image", "text"],
        "messages": [{ "role": "user", "content": content }],
    });
    let resp = client
        .post(format!("{API_BASE}/chat/completions"))
        .header("Authorization", format!("Bearer {key}"))
        .header("HTTP-Referer", "https://deskfolk.local")
        .header("X-Title", "Deskfolk Sprite Studio")
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", text.chars().take(300).collect::<String>()));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let msg = v.pointer("/choices/0/message").ok_or("no message in response")?;
    if let Some(url) = msg.pointer("/images/0/image_url/url").and_then(|x| x.as_str()) {
        return Ok(url.to_string());
    }
    if let Some(arr) = msg.get("content").and_then(|c| c.as_array()) {
        for part in arr {
            if let Some(url) = part.pointer("/image_url/url").and_then(|x| x.as_str()) {
                return Ok(url.to_string());
            }
        }
    }
    Err("model returned no image".into())
}

// --- Accept: flat set (backup + overwrite) ---------------------------------

fn decode_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let b64 = data_url.split_once(',').map(|(_, d)| d).unwrap_or(data_url);
    base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| e.to_string())
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n"
}

#[tauri::command]
pub fn accept_sprite(app: AppHandle, set: String, id: String, data_url: String) -> Result<String, String> {
    let bytes = decode_data_url(&data_url)?;
    if !is_png(&bytes) {
        return Err("candidate is not a PNG".into());
    }
    let root = character_root(&app);
    let target = match set.as_str() {
        "modular" => root.join("modular_sheet").join("sheets").join(format!("{id}.png")),
        _ => root.join("sprites").join(format!("{id}.png")),
    };
    if !target.exists() {
        return Err(format!("no such sprite: {}", target.display()));
    }
    let rel = match set.as_str() {
        "modular" => format!("modular_sheet/sheets/{id}.png"),
        _ => format!("sprites/{id}.png"),
    };
    let backup = backup_path(&root, &rel);
    if let Some(p) = backup.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&target, &backup).map_err(|e| format!("backup failed: {e}"))?;
    std::fs::write(&target, &bytes).map_err(|e| format!("write failed: {e}"))?;
    if let Some(src) = source_root(&app) {
        let st = src.join(&rel);
        if let Some(p) = st.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::write(&st, &bytes);
    }
    Ok(backup.display().to_string())
}

fn backup_path(root: &Path, rel: &str) -> PathBuf {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    root.join("backups").join(ts.to_string()).join(rel)
}

// --- Accept: slice a generated sheet into variant parts --------------------

#[derive(Deserialize)]
pub struct CellPick {
    index: u32,
    variant: String,
}

#[derive(Deserialize)]
pub struct SliceReq {
    slot: String,
    sheet: String, // data URL of the generated sheet
    cols: u32,
    rows: u32,
    cells: Vec<CellPick>,
    bounds: Option<Bounds>,
    pivot: Option<Vec<i64>>,
}

/// Slice the chosen cells out of a generated sheet, key each to transparency,
/// resize to the canonical canvas, write `sheets/<slot>__<variant>.png`, and
/// upsert the manifest instance so it renders. Returns the ids written.
#[tauri::command]
pub fn slice_accept(app: AppHandle, req: SliceReq) -> Result<Vec<String>, String> {
    let bytes = decode_data_url(&req.sheet)?;
    let sheet = image::load_from_memory(&bytes).map_err(|e| format!("bad sheet: {e}"))?;
    let (sw, sh) = sheet.dimensions();
    let cols = req.cols.max(1);
    let rows = req.rows.max(1);
    let cw = sw / cols;
    let ch = sh / rows;
    if cw == 0 || ch == 0 {
        return Err("sheet too small to slice".into());
    }

    let root = character_root(&app);
    let modular = root.join("modular_sheet");
    std::fs::create_dir_all(modular.join("sheets")).map_err(|e| e.to_string())?;
    let src_root = source_root(&app);

    let safe = |s: &str| s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect::<String>();
    let region = serde_json::json!({ "x": 0, "y": 0, "w": CANVAS, "h": CANVAS });
    let canonical = serde_json::json!({ "x": 0, "y": 0, "width": CANVAS, "height": CANVAS });
    let pivot = req
        .pivot
        .clone()
        .map(|p| serde_json::json!(p))
        .unwrap_or_else(|| serde_json::json!([160, 160]));

    let mut written = vec![];
    for pick in &req.cells {
        let col = pick.index % cols;
        let row = pick.index / cols;
        if row >= rows {
            continue;
        }
        let cell = sheet.crop_imm(col * cw, row * ch, cw, ch);
        // Normalise to the canonical canvas and key the background.
        let mut rgba = if cw == CANVAS && ch == CANVAS {
            cell.to_rgba8()
        } else {
            cell.resize_exact(CANVAS, CANVAS, image::imageops::FilterType::Nearest).to_rgba8()
        };
        key_background(&mut rgba);
        let mut png = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let png = png.into_inner();

        let fname = format!("{}__{}.png", safe(&req.slot), safe(&pick.variant));
        let rel = format!("sheets/{fname}");
        let target = modular.join("sheets").join(&fname);
        if target.exists() {
            let b = backup_path(&root, &format!("modular_sheet/{rel}"));
            if let Some(p) = b.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::copy(&target, &b);
        }
        std::fs::write(&target, &png).map_err(|e| format!("write failed: {e}"))?;
        upsert_instance(&modular, &req.slot, &pick.variant, &rel, region.clone(), canonical.clone(), pivot.clone())?;

        if let Some(src) = &src_root {
            let sm = src.join("modular_sheet");
            let st = sm.join("sheets").join(&fname);
            if let Some(p) = st.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::write(&st, &png);
            let _ = upsert_instance(&sm, &req.slot, &pick.variant, &rel, region.clone(), canonical.clone(), pivot.clone());
        }
        written.push(format!("{}/{}", req.slot, pick.variant));
    }
    tracing::info!("sprite studio: sliced {} cells from a {sw}×{sh} sheet", written.len());
    Ok(written)
}

/// Flood-fill the background to transparency from the borders, over pixels that
/// are near-black OR near-white-neutral. Skipped if the image already carries
/// real transparency (the model sometimes returns a proper alpha sheet).
fn key_background(img: &mut RgbaImage) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let transparent = img.pixels().filter(|p| p[3] < 8).count();
    if (transparent as f32) > 0.03 * (w * h) as f32 {
        return; // already has an alpha background
    }
    let bglike = |p: &image::Rgba<u8>| -> bool {
        let (r, g, b) = (p[0] as i32, p[1] as i32, p[2] as i32);
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        (mx < 46) || (mn > 218 && (mx - mn) < 26)
    };
    let mut bg = vec![false; (w * h) as usize];
    let mut dq = VecDeque::new();
    let mut push = |x: u32, y: u32, bg: &mut Vec<bool>, dq: &mut VecDeque<(u32, u32)>| {
        let i = (y * w + x) as usize;
        if !bg[i] && bglike(img.get_pixel(x, y)) {
            bg[i] = true;
            dq.push_back((x, y));
        }
    };
    for x in 0..w {
        push(x, 0, &mut bg, &mut dq);
        push(x, h - 1, &mut bg, &mut dq);
    }
    for y in 0..h {
        push(0, y, &mut bg, &mut dq);
        push(w - 1, y, &mut bg, &mut dq);
    }
    while let Some((x, y)) = dq.pop_front() {
        let mut nb = |nx: u32, ny: u32, bg: &mut Vec<bool>, dq: &mut VecDeque<(u32, u32)>| {
            let i = (ny * w + nx) as usize;
            if !bg[i] && bglike(img.get_pixel(nx, ny)) {
                bg[i] = true;
                dq.push_back((nx, ny));
            }
        };
        if x > 0 { nb(x - 1, y, &mut bg, &mut dq); }
        if x + 1 < w { nb(x + 1, y, &mut bg, &mut dq); }
        if y > 0 { nb(x, y - 1, &mut bg, &mut dq); }
        if y + 1 < h { nb(x, y + 1, &mut bg, &mut dq); }
    }
    for y in 0..h {
        for x in 0..w {
            if bg[(y * w + x) as usize] {
                img.get_pixel_mut(x, y)[3] = 0;
            }
        }
    }
}

/// Insert or update a manifest instance for `slot/variant`.
fn upsert_instance(
    modular: &Path,
    slot: &str,
    variant: &str,
    rel_source: &str,
    region: Value,
    canonical_bounds: Value,
    pivot: Value,
) -> Result<(), String> {
    let def_id = std::fs::read_to_string(modular.join("definition.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("id").cloned())
        .unwrap_or(Value::Null);

    let mpath = modular.join("manifest.json");
    let mut manifest: Value = std::fs::read_to_string(&mpath)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({ "definition_id": def_id, "instances": [] }));

    let instance = serde_json::json!({
        "id": format!("{slot}/{variant}"),
        "slot": slot,
        "variant": variant,
        "view": "front",
        "source": rel_source,
        "region": region,
        "canonical_bounds": canonical_bounds,
        "pivot": pivot,
        "angle": 0,
        "mirror_ok": false,
    });
    let arr = manifest
        .get_mut("instances")
        .and_then(|x| x.as_array_mut())
        .ok_or("manifest has no instances array")?;
    let id = format!("{slot}/{variant}");
    if let Some(existing) = arr.iter_mut().find(|i| i.get("id").and_then(|x| x.as_str()) == Some(&id)) {
        *existing = instance;
    } else {
        arr.push(instance);
    }
    std::fs::write(&mpath, serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(())
}
