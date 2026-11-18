//! Import touch control layouts from Java Android Minecraft launchers:
//! - PojavLauncher / Amethyst / MojoLauncher / Zalith v1 (`mControlDataList`, `mDrawerDataList`, `mJoystickDataList`)
//! - Fold Craft Launcher / FCL (`viewGroups`, `buttonList`, `directionList`)
//! - Zalith Launcher 2 (`layers`, `normalButtons`, `directionButtons`)
//! - Volt / Horizon native touch JSON (`horizon-touch.json`)
//! - `.zip` control packs wrapping any supported JSON layout

use super::{
    CustomButton,
    layout::{Control, ImportedMeta, MAX_CUSTOM_BUTTONS, Mode, Placement, Settings},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const REF_WIDTH_DP: f32 = 844.0;
const REF_HEIGHT_DP: f32 = 390.0;
const MAX_CONTROL_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_PRESETS: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetEntry {
    pub slug: String,
    pub meta: ImportedMeta,
}

#[derive(Clone, Debug)]
pub struct ImportedControlLayout {
    pub settings: Settings,
    pub meta: ImportedMeta,
}

pub fn import_file(path: &Path, display_name: &str) -> Result<ImportedControlLayout, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not read control file: {e}"))?;
    if meta.len() > 16 * 1024 * 1024 {
        return Err("Control package exceeds the 16 MiB limit".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read control file: {e}"))?;
    import_bytes(&bytes, display_name)
}

pub fn import_bytes(bytes: &[u8], display_name: &str) -> Result<ImportedControlLayout, String> {
    if bytes.is_empty() {
        return Err("Selected control file is empty".into());
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return import_zip(bytes, display_name);
    }
    if bytes.len() > MAX_CONTROL_JSON_BYTES {
        return Err("Control JSON exceeds the 4 MiB limit".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "Control file is not valid UTF-8 JSON or ZIP archive".to_owned())?;
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    let value: Value =
        serde_json::from_str(trimmed).map_err(|e| format!("Invalid control JSON: {e}"))?;
    import_json_value(&value, clean_layout_title(display_name))
}

fn import_zip(bytes: &[u8], display_name: &str) -> Result<ImportedControlLayout, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("Invalid control ZIP archive: {e}"))?;
    if archive.len() > 256 {
        return Err("Control ZIP archive contains too many entries".into());
    }
    let mut manifest_title: Option<String> = None;
    let mut candidates: Vec<(u8, String, Value)> = Vec::new();

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|e| format!("Corrupt ZIP entry: {e}"))?;
        if entry.is_dir() || entry.size() > MAX_CONTROL_JSON_BYTES as u64 {
            continue;
        }
        let name = entry.name().to_owned();
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".json") && !lower.ends_with(".control") && !lower.ends_with(".fcl") {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size().min(65536) as usize);
        entry
            .by_ref()
            .take(MAX_CONTROL_JSON_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("Could not extract {name}: {e}"))?;
        if buf.len() > MAX_CONTROL_JSON_BYTES {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&buf) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}').trim())
        else {
            continue;
        };
        let file_stem = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if matches!(
            file_stem.to_ascii_lowercase().as_str(),
            "manifest.json" | "info.json" | "pack.json"
        ) {
            if let Some(title) = json
                .get("name")
                .or_else(|| json.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                manifest_title = Some(title.to_owned());
            }
        }
        let score = layout_score(&json);
        if score > 0 {
            candidates.push((score, file_stem.into_owned(), json));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((_, entry_name, best)) = candidates.into_iter().next() else {
        return Err(
            "No supported PojavLauncher, Zalith, FCL or Volt control JSON found in ZIP".into(),
        );
    };
    let title = manifest_title.unwrap_or_else(|| {
        let outer = clean_layout_title(display_name);
        if outer.is_empty() || outer.eq_ignore_ascii_case("import") {
            clean_layout_title(&entry_name)
        } else {
            outer
        }
    });
    import_json_value(&best, title)
}

fn layout_score(v: &Value) -> u8 {
    let Some(obj) = v.as_object() else {
        return 0;
    };
    if obj.contains_key("mControlDataList")
        || obj.contains_key("mDrawerDataList")
        || obj.contains_key("mJoystickDataList")
    {
        return 100;
    }
    if obj.contains_key("viewGroups")
        || obj.contains_key("buttonList")
        || obj.contains_key("directionList")
    {
        return 95;
    }
    if obj.contains_key("layers")
        || obj.contains_key("controlLayers")
        || obj.contains_key("normalButtons")
        || obj.contains_key("directionButtons")
    {
        return 90;
    }
    if obj.contains_key("layouts")
        && (obj.contains_key("custom_buttons") || obj.contains_key("button_visibility"))
    {
        return 85;
    }
    0
}

fn clean_layout_title(raw: &str) -> String {
    let file = Path::new(raw)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| raw.to_owned());
    let mut stem = file.as_str();
    for ext in [".json", ".zip", ".control", ".fcl", ".txt"] {
        if stem.len() > ext.len() && stem[stem.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            stem = &stem[..stem.len() - ext.len()];
            break;
        }
    }
    let cleaned = stem.trim();
    if cleaned.is_empty() {
        "Imported Controls".into()
    } else {
        truncate_chars(cleaned, 44)
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn import_json_value(v: &Value, title: String) -> Result<ImportedControlLayout, String> {
    let Some(obj) = v.as_object() else {
        return Err("Control layout root must be a JSON object".into());
    };
    if obj.contains_key("mControlDataList")
        || obj.contains_key("mDrawerDataList")
        || obj.contains_key("mJoystickDataList")
    {
        return import_pojav(v, title);
    }
    if obj.contains_key("viewGroups")
        || obj.contains_key("buttonList")
        || obj.contains_key("directionList")
    {
        return import_fcl(v, title);
    }
    if obj.contains_key("layers")
        || obj.contains_key("controlLayers")
        || obj.contains_key("normalButtons")
        || obj.contains_key("directionButtons")
    {
        return import_zalith(v, title);
    }
    if obj.contains_key("layouts")
        && (obj.contains_key("custom_buttons") || obj.contains_key("button_visibility"))
    {
        return import_native(v, title);
    }
    Err(
        "Unrecognized control format. Expected PojavLauncher, Amethyst, Mojo, Zalith, FCL or Volt JSON."
            .into(),
    )
}

fn import_native(v: &Value, title: String) -> Result<ImportedControlLayout, String> {
    let mut settings: Settings =
        serde_json::from_value(v.clone()).map_err(|e| format!("Invalid Volt touch JSON: {e}"))?;
    let drawers = settings
        .custom_buttons
        .iter()
        .filter(|b| b.key.starts_with("special:drawer:"))
        .count();
    let has_joystick = settings.button_visibility[Control::Move as usize] != 2;
    let meta = settings.imported_from.clone().unwrap_or(ImportedMeta {
        name: title,
        format: "Volt".into(),
        buttons: settings.custom_buttons.len(),
        drawers,
        has_joystick,
        distinct: 0,
    });
    settings.imported_from = Some(meta.clone());
    settings.sanitize();
    Ok(ImportedControlLayout { settings, meta })
}

// ---------------------------------------------------------------------------
// 1. PojavLauncher / Amethyst / MojoLauncher / Zalith v1 (`ControlLayout`)
// ---------------------------------------------------------------------------

fn import_pojav(v: &Value, title: String) -> Result<ImportedControlLayout, String> {
    let version = v.get("version").and_then(Value::as_i64).unwrap_or(5);
    let scaled_at = v
        .get("scaledAt")
        .and_then(Value::as_f64)
        .map(|s| s as f32)
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(100.0);

    let (ref_w, ref_h, dp_px, px_scale) = if version <= 2 {
        (1920.0f32, 1080.0f32, 2.75f32, 1.0f32)
    } else {
        (REF_WIDTH_DP, REF_HEIGHT_DP, 1.0f32, 1.0f32 / 2.75f32)
    };
    let legacy_lwjgl2 = version == 1;

    let mut custom_buttons: Vec<CustomButton> = Vec::new();
    let mut drawer_count = 0usize;

    if let Some(list) = v.get("mControlDataList").and_then(Value::as_array) {
        for item in list {
            if custom_buttons.len() >= MAX_CUSTOM_BUTTONS {
                break;
            }
            if let Some(btn) = parse_pojav_button(
                item,
                ref_w,
                ref_h,
                dp_px,
                px_scale,
                scaled_at,
                legacy_lwjgl2,
                None,
                None,
                custom_buttons.len(),
            ) {
                custom_buttons.push(btn);
            }
        }
    }

    if let Some(drawers) = v.get("mDrawerDataList").and_then(Value::as_array) {
        for (idx, drawer) in drawers.iter().enumerate() {
            if custom_buttons.len() >= MAX_CUSTOM_BUTTONS {
                break;
            }
            let drawer_id = (idx + 1).min(u16::MAX as usize) as u16;
            let props = drawer.get("properties").unwrap_or(drawer);
            let (parent_x, parent_y, parent_w, parent_h) =
                pojav_box(props, ref_w, ref_h, dp_px, px_scale, scaled_at, idx + 1);
            if let Some(mut parent_btn) = parse_pojav_button(
                props,
                ref_w,
                ref_h,
                dp_px,
                px_scale,
                scaled_at,
                legacy_lwjgl2,
                None,
                Some(format!("special:drawer:{drawer_id}")),
                idx + 1,
            ) {
                if parent_btn.label.as_deref().unwrap_or_default().is_empty() {
                    parent_btn.label = Some("≡".into());
                }
                custom_buttons.push(parent_btn);
                drawer_count += 1;
            }

            let orientation = drawer_orientation(drawer.get("orientation"));
            if let Some(children) = drawer.get("buttonProperties").and_then(Value::as_array) {
                for (child_idx, child) in children.iter().enumerate() {
                    if custom_buttons.len() >= MAX_CUSTOM_BUTTONS {
                        break;
                    }
                    let override_pos = match orientation {
                        DrawerDir::Free => None,
                        dir => {
                            let step = (child_idx + 1) as f32;
                            let margin = 2.0 * dp_px;
                            let (dx, dy) = match dir {
                                DrawerDir::Down => (0.0, (parent_h + margin) * step),
                                DrawerDir::Up => (0.0, -(parent_h + margin) * step),
                                DrawerDir::Right => ((parent_w + margin) * step, 0.0),
                                DrawerDir::Left => (-(parent_w + margin) * step, 0.0),
                                DrawerDir::Free => (0.0, 0.0),
                            };
                            Some((parent_x + dx, parent_y + dy, parent_w, parent_h))
                        }
                    };
                    if let Some(mut child_btn) = parse_pojav_button(
                        child,
                        ref_w,
                        ref_h,
                        dp_px,
                        px_scale,
                        scaled_at,
                        legacy_lwjgl2,
                        override_pos,
                        None,
                        custom_buttons.len(),
                    ) {
                        child_btn.drawer_parent = Some(drawer_id);
                        custom_buttons.push(child_btn);
                    }
                }
            }
        }
    }

    let mut joystick: Option<(Placement, bool, bool)> = None;
    if let Some(sticks) = v.get("mJoystickDataList").and_then(Value::as_array)
        && let Some(stick) = sticks.first()
    {
        let (jx, jy, jw, jh) = pojav_box(stick, ref_w, ref_h, dp_px, px_scale, scaled_at, 0);
        let size = (jh.max(jw) / ref_h).clamp(0.12, 0.45);
        let cx = ((jx + jw * 0.5) / ref_w).clamp(0.05, 0.95);
        let cy = ((jy + jh * 0.5) / ref_h).clamp(0.05, 0.95);
        let opacity = normalize_opacity(stick.get("opacity").and_then(Value::as_f64));
        let forward_lock = stick
            .get("forwardLock")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let absolute = stick
            .get("absolute")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        joystick = Some((
            Placement {
                center: [cx, cy],
                size,
                opacity,
            },
            forward_lock,
            absolute,
        ));
    }

    if custom_buttons.is_empty() && joystick.is_none() {
        return Err("Pojav control file contains no buttons or joysticks".into());
    }

    let layout_name = v
        .get("name")
        .or_else(|| v.get("mName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 44))
        .unwrap_or(title);

    let format_name = if v.get("amethystVersion").is_some() {
        "Amethyst"
    } else if v.get("mojoVersion").is_some() {
        "MojoLauncher"
    } else {
        "PojavLauncher"
    };

    Ok(assemble_imported_settings(
        layout_name,
        format_name.into(),
        custom_buttons,
        drawer_count,
        joystick,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawerDir {
    Down,
    Left,
    Up,
    Right,
    Free,
}

fn drawer_orientation(v: Option<&Value>) -> DrawerDir {
    if let Some(s) = v.and_then(Value::as_str) {
        return match s.trim().to_ascii_uppercase().as_str() {
            "DOWN" => DrawerDir::Down,
            "LEFT" => DrawerDir::Left,
            "UP" => DrawerDir::Up,
            "RIGHT" => DrawerDir::Right,
            _ => DrawerDir::Free,
        };
    }
    if let Some(n) = v.and_then(Value::as_i64) {
        return match n {
            0 => DrawerDir::Down,
            1 => DrawerDir::Left,
            2 => DrawerDir::Up,
            3 => DrawerDir::Right,
            _ => DrawerDir::Free,
        };
    }
    DrawerDir::Free
}

/// A button with no position of its own still has to land somewhere.
///
/// It used to land at exact screen centre, which stacked every such button
/// on one spot and read as "all my controls teleported to the middle".
/// Spreading them round a ring keeps each one findable and draggable.
fn spread_fallback(axis_len: f32, side: f32, seed: usize, horizontal: bool) -> f32 {
    // Deterministic, so re-importing the same file always gives the same
    // layout: alternate between two columns and step down a row per seed.
    // Ladder that never repeats for realistic button counts: the old
    // `seed % 8` wrapped, so a 10+ button layout stacked two buttons on every
    // single slot and the labels drew on top of each other.
    const ROWS: usize = 16;
    let row = seed % ROWS;
    let col = (seed / ROWS) % 4;
    let xs = [0.28f32, 0.72, 0.50, 0.14];
    let start = if horizontal {
        axis_len * xs[col.min(3)]
    } else {
        axis_len * (0.06 + row as f32 * (0.88 / ROWS as f32))
    };
    let margin = axis_len * 0.02;
    (start - side * 0.5).clamp(margin, (axis_len - side - margin).max(margin))
}

fn pojav_box(
    obj: &Value,
    ref_w: f32,
    ref_h: f32,
    dp_px: f32,
    px_scale: f32,
    scaled_at: f32,
    seed: usize,
) -> (f32, f32, f32, f32) {
    // `scaledAt` stays what Pojav documents it as — the scale expressions
    // read through `${preferred_scale}` — rather than a size multiplier this
    // code would be guessing at.
    let raw_w = json_f32(obj.get("width")).unwrap_or(50.0).max(8.0) * dp_px;
    let raw_h = json_f32(obj.get("height")).unwrap_or(50.0).max(8.0) * dp_px;
    let vars = ExprVars {
        screen_w: ref_w,
        screen_h: ref_h,
        btn_w: raw_w,
        btn_h: raw_h,
        dp_px,
        px_scale,
        preferred_scale: scaled_at,
    };
    // Preferred order: a Pojav dynamic expression (dp after `${}` eval), then
    // ANY recognised coordinate key at top level or nested one level
    // (`position.x`, `rect.x`, ...), then the spread fallback.
    let x = obj
        .get("dynamicX")
        .and_then(Value::as_str)
        .and_then(|s| eval_dynamic_expr(s, &vars, ref_w))
        .or_else(|| {
            find_coord(obj, X_KEYS, 0).map(|raw| resolve_axis_px(raw, ref_w))
        })
        .unwrap_or_else(|| spread_fallback(ref_w, raw_w, seed, true));
    let y = obj
        .get("dynamicY")
        .and_then(Value::as_str)
        .and_then(|s| eval_dynamic_expr(s, &vars, ref_h))
        .or_else(|| {
            find_coord(obj, Y_KEYS, 1).map(|raw| resolve_axis_px(raw, ref_h))
        })
        .unwrap_or_else(|| spread_fallback(ref_h, raw_h, seed, false));
    (x, y, raw_w, raw_h)
}

#[allow(clippy::too_many_arguments)]
fn parse_pojav_button(
    obj: &Value,
    ref_w: f32,
    ref_h: f32,
    dp_px: f32,
    px_scale: f32,
    scaled_at: f32,
    legacy_lwjgl2: bool,
    override_box: Option<(f32, f32, f32, f32)>,
    override_token: Option<String>,
    seed: usize,
) -> Option<CustomButton> {
    let mut tokens: Vec<String> = Vec::new();
    if let Some(tok) = override_token {
        tokens.push(tok);
    } else {
        if let Some(arr) = obj.get("keycodes").and_then(Value::as_array) {
            for entry in arr {
                if let Some(code) = entry.as_i64()
                    && code != 0
                    && let Some(mapped) = map_java_keycode(code as i32, legacy_lwjgl2)
                    && !tokens.iter().any(|t| t == &mapped)
                {
                    tokens.push(mapped);
                }
            }
        }
        if tokens.is_empty()
            && let Some(single) = obj.get("keycode").and_then(Value::as_i64)
            && single != 0
            && let Some(mapped) = map_java_keycode(single as i32, legacy_lwjgl2)
        {
            tokens.push(mapped);
        }
    }
    // A button whose key this game cannot name is still a button the player
    // drew: dropping it silently made imported layouts look incomplete (and
    // shifted every later button's position). Keep it on screen with a key it
    // can never press, so the row, its size, its label and its position all
    // survive the import and the player can rebind it in the editor.
    if tokens.is_empty() {
        let named = obj
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| !n.trim().is_empty());
        let has_any_key = obj
            .get("keycodes")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
            || obj.get("keycode").and_then(Value::as_i64).is_some();
        // A button with nothing at all — no name, no key — is not a control.
        if !named && !has_any_key {
            return None;
        }
        tokens.push("key.keyboard.unknown".to_owned());
    }

    let (x, y, bw, bh) =
        override_box.unwrap_or_else(|| {
            pojav_box(obj, ref_w, ref_h, dp_px, px_scale, scaled_at, seed)
        });
    let norm_h = (bh / ref_h).clamp(0.025, 0.55);
    let width_ratio = (bw / bh.max(1.0)).clamp(0.2, 8.0);
    let cx = ((x + bw * 0.5) / ref_w).clamp(0.01, 0.99);
    let cy = ((y + bh * 0.5) / ref_h).clamp(0.01, 0.99);
    let opacity = normalize_opacity(obj.get("opacity").and_then(Value::as_f64));

    let raw_name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let label = if raw_name.is_empty() {
        None
    } else {
        Some(truncate_chars(raw_name, 32))
    };

    let toggle = obj
        .get("isToggle")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let swipeable = obj
        .get("isSwipeable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let passthrough = obj
        .get("passThruEnabled")
        .or_else(|| obj.get("passthroughEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let in_game = obj
        .get("displayInGame")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    // A Pojav file that says nothing about menu visibility means "show it":
    // the classic ControlLayout has no such field at all, and defaulting it
    // to false hid every imported button on the inventory/container screens,
    // so a layout that looked complete in game all but vanished in the
    // editor. Only an explicit `displayInMenu: false` hides a button there.
    let in_menu = obj
        .get("displayInMenu")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let bg_color = parse_argb_color(obj.get("bgColor")).or(Some([0, 0, 0, 115]));
    let stroke_width_pct = json_f32(obj.get("strokeWidth")).unwrap_or(0.0);
    let stroke_color = if stroke_width_pct > 0.0 {
        parse_argb_color(obj.get("strokeColor"))
    } else {
        parse_argb_color(obj.get("strokeColor")).filter(|c| c[3] > 0)
    };
    // Pojav strokeWidth / cornerRadius are percentages of the button size.
    let min_side = bw.min(bh).max(1.0);
    let stroke_width = (stroke_width_pct > 0.0)
        .then(|| (stroke_width_pct / 100.0 * min_side * 0.5).clamp(0.5, 12.0));
    let corner_radius = json_f32(obj.get("cornerRadius"))
        .filter(|pct| *pct > 0.0)
        .map(|pct| (pct / 100.0 * min_side * 0.5).clamp(0.5, 24.0));

    let primary = tokens.remove(0);
    let placement = Placement {
        center: [cx, cy],
        size: norm_h,
        opacity,
    };
    Some(CustomButton {
        key: primary,
        layouts: [placement; 3],
        extra_keys: tokens,
        label,
        width_ratio,
        toggle,
        swipeable,
        passthrough,
        in_game,
        in_menu,
        bg_color,
        stroke_color,
        corner_radius,
        stroke_width,
        background_image: None,
        drawer_parent: None,
    })
}

// ---------------------------------------------------------------------------
// 2. Fold Craft Launcher / FCL (`viewGroups` / `buttonList` / `directionList`)
// ---------------------------------------------------------------------------

fn import_fcl(v: &Value, title: String) -> Result<ImportedControlLayout, String> {
    let mut custom_buttons: Vec<CustomButton> = Vec::new();
    let mut joystick: Option<(Placement, bool, bool)> = None;
    let mut drawer_count = 0usize;

    let mut groups: Vec<(Option<u16>, &Value, bool)> = Vec::new();
    if let Some(arr) = v.get("viewGroups").and_then(Value::as_array) {
        let mut group_id_map: std::collections::BTreeMap<String, u16> =
            std::collections::BTreeMap::new();
        for (idx, g) in arr.iter().enumerate() {
            if let Some(id) = g.get("id").and_then(Value::as_str) {
                group_id_map.insert(id.to_owned(), (idx + 1) as u16);
            }
        }
        for (idx, g) in arr.iter().enumerate() {
            let visible_by_default = g
                .get("visibility")
                .and_then(Value::as_str)
                .is_none_or(|vis| !vis.eq_ignore_ascii_case("INVISIBLE"));
            let gid = (idx + 1) as u16;
            let data = g.get("viewData").unwrap_or(g);
            groups.push(((!visible_by_default).then_some(gid), data, visible_by_default));
        }
        // Parse buttons with group bindings
        for (drawer_parent, data, visible_by_default) in &groups {
            if !visible_by_default {
                drawer_count += 1;
            }
            parse_fcl_view_data(
                data,
                *drawer_parent,
                &group_id_map,
                &mut custom_buttons,
                &mut joystick,
            );
        }
    } else {
        let empty = std::collections::BTreeMap::new();
        parse_fcl_view_data(v, None, &empty, &mut custom_buttons, &mut joystick);
    }

    if custom_buttons.is_empty() && joystick.is_none() {
        return Err("FCL control file contains no buttons or direction pads".into());
    }

    let layout_name = v
        .get("manifest")
        .and_then(|m| m.get("name"))
        .or_else(|| v.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 44))
        .unwrap_or(title);

    Ok(assemble_imported_settings(
        layout_name,
        "Fold Craft (FCL)".into(),
        custom_buttons,
        drawer_count,
        joystick,
    ))
}

fn parse_fcl_view_data(
    data: &Value,
    drawer_parent: Option<u16>,
    group_id_map: &std::collections::BTreeMap<String, u16>,
    custom_buttons: &mut Vec<CustomButton>,
    joystick: &mut Option<(Placement, bool, bool)>,
) {
    if let Some(buttons) = data.get("buttonList").and_then(Value::as_array) {
        for btn in buttons {
            if custom_buttons.len() >= MAX_CUSTOM_BUTTONS {
                break;
            }
            if let Some(mut parsed) =
                parse_fcl_button(btn, group_id_map, custom_buttons.len())
            {
                parsed.drawer_parent = drawer_parent;
                custom_buttons.push(parsed);
            }
        }
    }
    if joystick.is_none()
        && let Some(dirs) = data.get("directionList").and_then(Value::as_array)
        && let Some(dir) = dirs.first()
    {
        let base = dir.get("baseInfo").unwrap_or(dir);
        let (cx, cy, _, norm_h, opacity, _, _) = parse_fcl_base_info(base, 0.28, 0);
        let style_dpad = dir
            .get("styleType")
            .and_then(Value::as_str)
            .is_some_and(|s| s.to_ascii_uppercase().contains("BUTTON"));
        let sneak_or_sprint = dir
            .get("event")
            .and_then(|e| e.get("sneak"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        *joystick = Some((
            Placement {
                center: [cx, cy],
                size: norm_h.clamp(0.14, 0.45),
                opacity,
            },
            sneak_or_sprint,
            style_dpad,
        ));
    }
}

fn parse_fcl_base_info(
    base: &Value,
    fallback_h: f32,
    seed: usize,
) -> (f32, f32, f32, f32, f32, bool, bool) {
    let size_type = base
        .get("sizeType")
        .and_then(Value::as_str)
        .unwrap_or("ABSOLUTE");
    let raw_w = json_f32(base.get("width")).unwrap_or(50.0).max(1.0);
    let raw_h = json_f32(base.get("height")).unwrap_or(50.0).max(1.0);

    let (norm_w, norm_h) = if size_type.eq_ignore_ascii_case("PERCENTAGE") {
        let w = if raw_w > 1.0 { raw_w / 1000.0 } else { raw_w };
        let h = if raw_h > 1.0 { raw_h / 1000.0 } else { raw_h };
        (w.clamp(0.02, 0.8), h.clamp(0.025, 0.55))
    } else {
        // FCL stores ABSOLUTE sizes either as dp*10 (e.g. 500 = 50.0dp) or plain dp (50)
        let w_dp = if raw_w > 160.0 { raw_w / 10.0 } else { raw_w };
        let h_dp = if raw_h > 160.0 { raw_h / 10.0 } else { raw_h };
        (
            (w_dp / REF_WIDTH_DP).clamp(0.02, 0.8),
            (h_dp / REF_HEIGHT_DP).clamp(0.025, 0.55),
        )
    };
    let norm_h = if norm_h.is_finite() {
        norm_h
    } else {
        fallback_h
    };
    // A control with no stored position must not land at dead centre, where
    // every such control stacks on one spot ("everything teleported to the
    // middle"). Spreading sends it to a free-ish slot instead.
    // `xPosition`/`yPosition` first (what FCL writes), then any other known
    // coordinate key at this level or nested one object deep, so a variant
    // that moved its geometry under `position`/`rect` still imports.
    let rx = json_f32(base.get("xPosition").or_else(|| base.get("x")))
        .or_else(|| find_coord(base, X_KEYS, 0));
    let ry = json_f32(base.get("yPosition").or_else(|| base.get("y")))
        .or_else(|| find_coord(base, Y_KEYS, 1));
    // FCL writes positions as dp*10 (500 = 50dp), plain dp (50), a 0..1
    // fraction, or a 0..100 percentage. Dividing everything by 1000 put
    // every dp position in the left/top few percent of the screen, which
    // read as a pile of controls in the corner. Convert by magnitude, then
    // clamp into the axis.
    let axis = |raw: f32, dp_axis: f32| -> f32 {
        if raw > 1.0 {
            let as_dp = raw / 10.0;
            if as_dp <= dp_axis {
                as_dp / dp_axis
            } else if raw <= 100.0 {
                raw / 100.0
            } else {
                raw / dp_axis
            }
        } else {
            raw
        }
        .clamp(0.0, 1.0)
    };
    let tx = rx
        .map(|v| axis(v, REF_WIDTH_DP))
        .unwrap_or_else(|| if seed % 2 == 0 { 0.28 } else { 0.72 });
    let ty = ry
        .map(|v| axis(v, REF_HEIGHT_DP))
        .unwrap_or_else(|| 0.14 + (seed % 8) as f32 * 0.09);
    let left = (1.0 - norm_w) * tx;
    let top = (1.0 - norm_h) * ty;
    let cx = (left + norm_w * 0.5).clamp(0.01, 0.99);
    let cy = (top + norm_h * 0.5).clamp(0.01, 0.99);

    let alpha = json_f32(base.get("alpha").or_else(|| base.get("opacity")))
        .map(|a| if a > 1.0 { a / 100.0 } else { a })
        .unwrap_or(0.75)
        .clamp(0.05, 1.0);

    let (in_game, in_menu) = parse_visibility_mode(base.get("visibilityType"));
    let width_ratio = ((norm_w * REF_WIDTH_DP) / (norm_h * REF_HEIGHT_DP).max(1.0)).clamp(0.2, 8.0);
    (cx, cy, width_ratio, norm_h, alpha, in_game, in_menu)
}

fn parse_fcl_button(
    btn: &Value,
    group_id_map: &std::collections::BTreeMap<String, u16>,
    seed: usize,
) -> Option<CustomButton> {
    let base = btn.get("baseInfo").unwrap_or(btn);
    let (cx, cy, width_ratio, norm_h, opacity, in_game, in_menu) =
        parse_fcl_base_info(base, 0.12, seed);
    let event = btn.get("event").unwrap_or(btn);
    let press = event.get("pressEvent").unwrap_or(event);

    let mut tokens: Vec<String> = Vec::new();
    if let Some(groups) = press
        .get("bindViewGroup")
        .or_else(|| event.get("bindViewGroup"))
        .and_then(Value::as_array)
    {
        for g in groups {
            if let Some(id_str) = g.as_str()
                && let Some(&gid) = group_id_map.get(id_str)
            {
                tokens.push(format!("special:drawer:{gid}"));
            }
        }
    }
    if let Some(codes) = press
        .get("outputKeycodes")
        .or_else(|| event.get("outputKeycodes"))
        .or_else(|| btn.get("keycodes"))
        .and_then(Value::as_array)
    {
        for entry in codes {
            if let Some(tok) = parse_fcl_or_zalith_key(entry)
                && !tokens.iter().any(|t| t == &tok)
            {
                tokens.push(tok);
            }
        }
    }
    if tokens.is_empty() {
        // Same rule as the Pojav path: a control FCL drew but this game has
        // no key name for keeps its row, size, label and position so the
        // layout still matches what the player authored. A button carrying
        // nothing at all is not a control.
        let named = btn
            .get("text")
            .and_then(Value::as_str)
            .is_some_and(|t| !t.trim().is_empty());
        let has_any_key = press
            .get("keycodes")
            .or_else(|| event.get("keycodes"))
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        if !named && !has_any_key {
            return None;
        }
        tokens.push("key.keyboard.unknown".to_owned());
    }

    let toggle = press
        .get("autoKeep")
        .or_else(|| event.get("autoKeep"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let passthrough = press
        .get("movable")
        .or_else(|| press.get("pointerFollow"))
        .or_else(|| event.get("pointerFollow"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let label = btn
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 32));

    let style = btn.get("style");
    let bg_color = style
        .and_then(|s| parse_argb_color(s.get("fillColor").or_else(|| s.get("bgColor"))))
        .or(Some([0, 0, 0, 115]));
    let stroke_color = style.and_then(|s| parse_argb_color(s.get("strokeColor")));

    let primary = tokens.remove(0);
    let placement = Placement {
        center: [cx, cy],
        size: norm_h,
        opacity,
    };
    Some(CustomButton {
        key: primary,
        layouts: [placement; 3],
        extra_keys: tokens,
        label,
        width_ratio,
        toggle,
        swipeable: false,
        passthrough,
        in_game,
        in_menu,
        bg_color,
        stroke_color,
        corner_radius: None,
        stroke_width: None,
        background_image: None,
        drawer_parent: None,
    })
}

// ---------------------------------------------------------------------------
// 3. Zalith Launcher 2 (`layers` / `normalButtons` / `directionButtons`)
// ---------------------------------------------------------------------------

fn import_zalith(v: &Value, title: String) -> Result<ImportedControlLayout, String> {
    let mut custom_buttons: Vec<CustomButton> = Vec::new();
    let mut joystick: Option<(Placement, bool, bool)> = None;
    let mut drawer_count = 0usize;

    let layers = v
        .get("layers")
        .or_else(|| v.get("controlLayers"))
        .and_then(Value::as_array);

    if let Some(layers) = layers {
        for (idx, layer) in layers.iter().enumerate() {
            let hidden = layer
                .get("hide")
                .or_else(|| layer.get("defaultHide"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let drawer_parent = hidden.then_some((idx + 1) as u16);
            if hidden {
                drawer_count += 1;
            }
            parse_zalith_layer(layer, drawer_parent, &mut custom_buttons, &mut joystick);
        }
    } else {
        parse_zalith_layer(v, None, &mut custom_buttons, &mut joystick);
    }

    if custom_buttons.is_empty() && joystick.is_none() {
        return Err("Zalith control file contains no active buttons or joysticks".into());
    }

    let layout_name = v
        .get("info")
        .and_then(|i| i.get("name"))
        .or_else(|| v.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, 44))
        .unwrap_or(title);

    Ok(assemble_imported_settings(
        layout_name,
        "Zalith Launcher".into(),
        custom_buttons,
        drawer_count,
        joystick,
    ))
}

fn parse_zalith_layer(
    layer: &Value,
    drawer_parent: Option<u16>,
    custom_buttons: &mut Vec<CustomButton>,
    joystick: &mut Option<(Placement, bool, bool)>,
) {
    if let Some(buttons) = layer
        .get("normalButtons")
        .or_else(|| layer.get("buttons"))
        .and_then(Value::as_array)
    {
        for btn in buttons {
            if custom_buttons.len() >= MAX_CUSTOM_BUTTONS {
                break;
            }
            if let Some(mut parsed) = parse_zalith_button(btn, custom_buttons.len()) {
                parsed.drawer_parent = drawer_parent;
                custom_buttons.push(parsed);
            }
        }
    }
    if joystick.is_none()
        && let Some(sticks) = layer
            .get("joystickButtons")
            .or_else(|| layer.get("directionButtons"))
            .or_else(|| layer.get("joysticks"))
            .and_then(Value::as_array)
        && let Some(stick) = sticks.first()
    {
        let (cx, cy, _, norm_h) = parse_zalith_rect(stick, 0.28, 0);
        let opacity = normalize_opacity(stick.get("alpha").or_else(|| stick.get("opacity")).and_then(Value::as_f64));
        *joystick = Some((
            Placement {
                center: [cx, cy],
                size: norm_h.clamp(0.14, 0.45),
                opacity,
            },
            true,
            false,
        ));
    }
}

fn parse_zalith_rect(obj: &Value, fallback_h: f32, seed: usize) -> (f32, f32, f32, f32) {
    // ---- size ----
    // Zalith 2 nests it as `buttonSize: {type: "dp", widthDp, heightDp}`.
    // Zalith 1 writes flat `width`/`height` with an `isPercentageSize` flag.
    let size = obj.get("buttonSize");
    // Zalith 2 joysticks carry a single `sizeDp` instead of width/height.
    let rw = size
        .and_then(|s| json_f32(s.get("widthDp")))
        .or_else(|| json_f32(obj.get("width")))
        .or_else(|| json_f32(obj.get("sizeDp")))
        .unwrap_or(50.0);
    let rh = size
        .and_then(|s| json_f32(s.get("heightDp")))
        .or_else(|| json_f32(obj.get("height")))
        .or_else(|| json_f32(obj.get("sizeDp")))
        .unwrap_or(50.0);
    let normalize_coord = |raw: f32, dp_axis: f32| -> f32 {
        if raw > 1000.0 {
            raw / 10000.0
        } else if raw > 100.0 {
            raw / 1000.0
        } else if raw > 1.0 {
            (raw / 100.0).min(raw / dp_axis)
        } else {
            raw
        }
    };
    // A dp size is density-independent: divide by the reference axis.
    let dp_sized = size.is_some()
        || obj.get("isPercentageSize").and_then(Value::as_bool) == Some(false)
        || (rw <= 250.0 && rh <= 250.0 && rw > 1.0);
    let norm_w = if dp_sized {
        (rw / REF_WIDTH_DP).clamp(0.02, 0.8)
    } else {
        normalize_coord(rw, REF_WIDTH_DP).clamp(0.02, 0.8)
    };
    let norm_h = if dp_sized {
        (rh / REF_HEIGHT_DP).clamp(0.025, 0.55)
    } else {
        normalize_coord(rh, REF_HEIGHT_DP).clamp(0.025, 0.55)
    };
    let norm_h = if norm_h.is_finite() { norm_h } else { fallback_h };

    // ---- position ----
    // Zalith 2 stores it nested: `position: {x, y}` on 0..=10000, where the
    // value is `pixel / (screen - widget)` — a FRACTION OF THE FREE SPACE,
    // exactly like Pojav's `${right}`/`${bottom}`. Neither x/y nor
    // dynamicX/dynamicY exists there, so reading only those dropped every
    // ZL2 button into the spread fallback and stacked them in two columns.
    let position = obj.get("position");
    let vars = ExprVars {
        screen_w: REF_WIDTH_DP,
        screen_h: REF_HEIGHT_DP,
        btn_w: rw.max(1.0),
        btn_h: rh.max(1.0),
        dp_px: 1.0,
        px_scale: 1.0 / 2.75,
        preferred_scale: 100.0,
    };
    let dynamic = |key: &str, axis: f32| -> Option<f32> {
        obj.get(key)
            .and_then(Value::as_str)
            .and_then(|s| eval_dynamic_expr(s, &vars, axis))
    };
    // Zalith 2's `position` object is a KNOWN schema: 0..=10000 of the free
    // space. Magnitude guessing must not touch it — a legitimate `y: 1000`
    // would otherwise be read as `1000/1000 = 1.0` (bottom edge).
    let zl2_free = |axis: &str| -> Option<f32> {
        position
            .and_then(|p| json_f32(p.get(axis)))
            .map(|raw| (raw / 10000.0).clamp(0.0, 1.0))
    };
    let tx = dynamic("dynamicX", REF_WIDTH_DP)
        .map(|px| (px / REF_WIDTH_DP).clamp(0.0, 1.0))
        .or_else(|| zl2_free("x"))
        .or_else(|| {
            find_coord(obj, X_KEYS, 0).map(|raw| {
                // Axis-aware, like the Pojav path: a value that fits the
                // reference width is dp, not a 0..=1000 fraction. Dividing
                // everything by magnitude squeezed whole layouts into the
                // left ~87% of the screen and left the right edge empty.
                (resolve_axis_px(raw, REF_WIDTH_DP) / REF_WIDTH_DP).clamp(0.0, 1.0)
            })
        })
        .unwrap_or_else(|| if seed % 2 == 0 { 0.28 } else { 0.72 });
    let ty = dynamic("dynamicY", REF_HEIGHT_DP)
        .map(|px| (px / REF_HEIGHT_DP).clamp(0.0, 1.0))
        .or_else(|| zl2_free("y"))
        .or_else(|| {
            find_coord(obj, Y_KEYS, 1).map(|raw| {
                (resolve_axis_px(raw, REF_HEIGHT_DP) / REF_HEIGHT_DP).clamp(0.0, 1.0)
            })
        })
        .unwrap_or_else(|| 0.14 + (seed % 8) as f32 * 0.09);

    let left = (1.0 - norm_w) * tx;
    let top = (1.0 - norm_h) * ty;
    let cx = (left + norm_w * 0.5).clamp(0.01, 0.99);
    let cy = (top + norm_h * 0.5).clamp(0.01, 0.99);
    let width_ratio = ((norm_w * REF_WIDTH_DP) / (norm_h * REF_HEIGHT_DP).max(1.0)).clamp(0.2, 8.0);
    (cx, cy, width_ratio, norm_h)
}

fn parse_zalith_button(btn: &Value, seed: usize) -> Option<CustomButton> {
    let (cx, cy, width_ratio, norm_h) = parse_zalith_rect(btn, 0.12, seed);
    let mut tokens: Vec<String> = Vec::new();
    if let Some(events) = btn.get("clickEvents").and_then(Value::as_array) {
        for ev in events {
            let key_val = ev.get("key").or_else(|| ev.get("keycode")).unwrap_or(ev);
            if let Some(tok) = parse_fcl_or_zalith_key(key_val)
                && !tokens.iter().any(|t| t == &tok)
            {
                tokens.push(tok);
            }
        }
    }
    if let Some(keys) = btn.get("keycodes").and_then(Value::as_array) {
        for k in keys {
            if let Some(tok) = parse_fcl_or_zalith_key(k)
                && !tokens.iter().any(|t| t == &tok)
            {
                tokens.push(tok);
            }
        }
    }
    if tokens.is_empty() {
        return None;
    }

    let opacity = normalize_opacity(
        btn.get("alpha")
            .or_else(|| btn.get("opacity"))
            .and_then(Value::as_f64),
    );
    let (in_game, in_menu) = parse_visibility_mode(btn.get("visibilityType"));
    let toggle = btn
        .get("isToggle")
        .or_else(|| btn.get("toggle"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let swipeable = btn
        .get("isSwipeable")
        .or_else(|| btn.get("swipeable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let passthrough = btn
        .get("isPointerFollow")
        .or_else(|| btn.get("passThruEnabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Zalith 2 keeps the caption as `text: {default: "..."}` — an object, so
    // `Value::as_str` never matched and every ZL2 button imported nameless.
    let text = btn.get("text");
    let raw_label = text
        .and_then(Value::as_str)
        .or_else(|| text.and_then(|t| t.get("default")).and_then(Value::as_str))
        .or_else(|| btn.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let label = raw_label.map(|s| truncate_chars(s, 32));

    let primary = tokens.remove(0);
    let placement = Placement {
        center: [cx, cy],
        size: norm_h,
        opacity,
    };
    Some(CustomButton {
        key: primary,
        layouts: [placement; 3],
        extra_keys: tokens,
        label,
        width_ratio,
        toggle,
        swipeable,
        passthrough,
        in_game,
        in_menu,
        bg_color: Some([0, 0, 0, 115]),
        stroke_color: None,
        corner_radius: Some(8.0),
        stroke_width: None,
        background_image: None,
        drawer_parent: None,
    })
}

// ---------------------------------------------------------------------------
// Shared Settings Assembler & Keycode / Expression Utilities
// ---------------------------------------------------------------------------

/// Nudge apart buttons that landed on top of each other.
///
/// Several launchers (and Zalith 2 layers in particular) let two controls sit
/// at the same spot, and some exports repeat a position when a control was
/// duplicated. Drawn as-is, each pair stacks and their captions print over one
/// another into unreadable mush ("toglean", "Water" over "ender"), which is
/// what "the controls are cramped and overlap" looks like.
///
/// Buttons are only moved when their centres are almost coincident, and only
/// far enough to separate them, so a layout that genuinely uses overlapping
/// rows is left alone.
fn separate_stacked_buttons(buttons: &mut [CustomButton]) {
    // Two centres closer than this (in both axes) are indistinguishable.
    const MIN_GAP: f32 = 0.045;
    const STEP: f32 = 0.035;
    const EDGE: f32 = 0.02;
    for _ in 0..16 {
        let mut moved = false;
        for i in 0..buttons.len() {
            for j in (i + 1)..buttons.len() {
                let a = buttons[i].layouts[0].center;
                let b = buttons[j].layouts[0].center;
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                if dx.abs() >= MIN_GAP || dy.abs() >= MIN_GAP {
                    continue;
                }
                // Push along whichever axis has room, preferring the one that
                // is already separating them so the layout keeps its shape.
                let (sx, sy) = if dx.abs() >= dy.abs() {
                    (if dx >= 0.0 { 1.0 } else { -1.0 }, 0.0)
                } else {
                    (0.0, if dy >= 0.0 { 1.0 } else { -1.0 })
                };
                // Move every mode row by the same amount: a button sits in
                // the same place whichever movement scheme is active, and
                // separating only one row would leave the others stacked.
                for (index, sign) in [(i, -1.0f32), (j, 1.0)] {
                    for layout in buttons[index].layouts.iter_mut() {
                        let c = &mut layout.center;
                        c[0] = (c[0] + sign * sx * STEP).clamp(EDGE, 1.0 - EDGE);
                        c[1] = (c[1] + sign * sy * STEP).clamp(EDGE, 1.0 - EDGE);
                    }
                }
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
}

/// Stretch a freshly imported layout across the whole screen.
///
/// Launchers author layouts against whichever device the author owned. When
/// that device was wider than the reference we normalise against, the whole
/// set lands in a band with an empty strip down the right-hand side — and
/// because everything is packed into less room than it needs, buttons collide
/// with each other. Anchored percentage layouts are independent per axis, so
/// scaling each axis on its own is the behaviour the launcher itself has.
///
/// Only layouts that clearly under-fill are touched: anything already using
/// three quarters of an axis is left exactly as authored.
fn stretch_to_screen(buttons: &mut [CustomButton]) {
    if buttons.len() < 2 {
        return;
    }
    const MIN_SPAN: f32 = 0.75;
    const MARGIN: f32 = 0.02;
    for axis in 0..2 {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for b in buttons.iter() {
            let c = b.layouts[0].center[axis];
            if c.is_finite() {
                lo = lo.min(c);
                hi = hi.max(c);
            }
        }
        let span = hi - lo;
        // A degenerate or already-wide layout would gain nothing and could
        // distort deliberately tight control clusters.
        if !span.is_finite() || span <= 0.0 || span >= MIN_SPAN {
            continue;
        }
        let scale = (1.0 - MARGIN * 2.0) / span;
        for b in buttons.iter_mut() {
            for layout in b.layouts.iter_mut() {
                let c = layout.center[axis];
                layout.center[axis] = (MARGIN + (c - lo) * scale).clamp(0.0, 1.0);
            }
            // Keep the non-active rows consistent with row 0 the importer
            // just produced; they are copies of the same placement anyway.
        }
    }
}

fn assemble_imported_settings(
    name: String,
    format: String,
    custom_buttons: Vec<CustomButton>,
    drawers: usize,
    joystick: Option<(Placement, bool, bool)>,
) -> ImportedControlLayout {
    let mut settings = Settings::default();
    let has_wasd = custom_buttons.iter().any(|b| {
        matches!(
            b.key.as_str(),
            "key.keyboard.w"
                | "key.keyboard.a"
                | "key.keyboard.s"
                | "key.keyboard.d"
                | "action:key.forward"
                | "action:key.left"
                | "action:key.back"
                | "action:key.right"
        )
    });
    let has_attack_or_use = custom_buttons.iter().any(|b| {
        std::iter::once(&b.key)
            .chain(b.extra_keys.iter())
            .any(|tok| {
                matches!(
                    tok.as_str(),
                    "action:key.attack"
                        | "action:key.use"
                        | "special:mouse_pri"
                        | "special:mouse_sec"
                )
            })
    });
    let has_escape_or_menu = custom_buttons.iter().any(|b| {
        std::iter::once(&b.key)
            .chain(b.extra_keys.iter())
            .any(|tok| matches!(tok.as_str(), "key.keyboard.escape" | "special:menu"))
    });

    // Hide duplicate built-in HUD buttons so only the imported Java layout appears.
    settings.button_visibility = [2; 15];
    let has_joystick = if let Some((stick_place, forward_sprint, absolute_dpad)) = joystick {
        settings.button_visibility[Control::Move as usize] = 1;
        settings.sprint_with_joystick = forward_sprint;
        for row in &mut settings.layouts {
            row[Control::Move as usize] = stick_place;
        }
        settings.mode = if absolute_dpad {
            Mode::Dpad
        } else {
            Mode::Crosshair
        };
        true
    } else if has_wasd {
        settings.button_visibility[Control::Move as usize] = 2;
        settings.mode = Mode::Crosshair;
        false
    } else {
        // Keep built-in movement stick when layout neither defines WASD nor a joystick.
        settings.button_visibility[Control::Move as usize] = 1;
        settings.mode = Mode::Crosshair;
        true
    };

    if !has_escape_or_menu {
        settings.button_visibility[Control::Pause as usize] = 1;
    }

    settings.split_controls = true;
    settings.crosshair_gestures = !has_attack_or_use;
    settings.inventory_controls = true;
    settings.offhand_button = false;
    settings.shield_button = false;

    let mut custom_buttons = custom_buttons;
    separate_stacked_buttons(&mut custom_buttons);
    stretch_to_screen(&mut custom_buttons);

    let distinct = {
        let mut seen: Vec<(i32, i32)> = Vec::with_capacity(custom_buttons.len());
        for b in &custom_buttons {
            let c = b.layouts[0].center;
            let key = ((c[0] * 1000.0).round() as i32, (c[1] * 1000.0).round() as i32);
            if !seen.contains(&key) {
                seen.push(key);
            }
        }
        seen.len()
    };
    let meta = ImportedMeta {
        name,
        format,
        buttons: custom_buttons.len(),
        drawers,
        has_joystick,
        distinct,
    };
    settings.custom_buttons = custom_buttons;
    settings.imported_from = Some(meta.clone());
    settings.sanitize();
    ImportedControlLayout { settings, meta }
}

fn parse_visibility_mode(v: Option<&Value>) -> (bool, bool) {
    match v
        .and_then(Value::as_str)
        .unwrap_or("ALWAYS")
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "IN_GAME" | "INGAME" | "GAME" => (true, false),
        "MENU" | "IN_MENU" | "INMENU" | "GUI" => (false, true),
        _ => (true, true),
    }
}

/// Every key name any launcher has ever used for a control's horizontal or
/// vertical anchor. Checked in order, at the top level *and* one object deep
/// (`position.x`, `layout.x`, ...), so a format we have never seen still
/// yields coordinates instead of silently collapsing to the fallback grid.
const X_KEYS: &[&str] = &[
    "dynamicX", "x", "xPosition", "positionX", "position_x", "centerX", "center_x",
    "left", "offsetX", "offset_x", "posX", "pos_x", "layoutX",
];
const Y_KEYS: &[&str] = &[
    "dynamicY", "y", "yPosition", "positionY", "position_y", "centerY", "center_y",
    "top", "offsetY", "offset_y", "posY", "pos_y", "layoutY",
];
/// Objects worth looking inside for a position, besides the button itself.
const POSITION_CONTAINERS: &[&str] = &[
    "position", "pos", "layout", "rect", "bounds", "placement", "location", "transform",
];

/// Turn an unknown-unit coordinate into a pixel offset on one axis.
///
/// A value that fits inside the axis is taken as dp. A value larger than the
/// axis cannot be dp — it would clamp to the screen edge — so it is read as a
/// normalised 0..=10000 or 0..=100 scale instead. Without this, a launcher
/// that ships coordinates on the 0..=10000 grid had every button pinned to
/// the right/bottom edge.
fn resolve_axis_px(raw: f32, axis_len: f32) -> f32 {
    if !raw.is_finite() {
        return axis_len * 0.5;
    }
    if raw > 0.0 && raw < 1.0 {
        return raw * axis_len;
    }
    if raw <= axis_len {
        return raw;
    }
    // Too big for dp. The 0..=10000 grid is the common case (Zalith 2 and
    // most converters), so test it before the 0..=100 percent scale.
    if raw <= 10000.0 {
        return (raw / 10000.0) * axis_len;
    }
    ((raw / 100.0).min(1.0)) * axis_len
}

/// Find a coordinate for `keys` on `obj`. The button itself is checked first
/// at every key name, then the well-known container objects (`position`,
/// `rect`, ...), then — because launchers keep inventing nesting — any nested
/// object a couple of levels deep. A two-element numeric array under a
/// position-ish key (`"position": [x, y]`) is also understood.
fn find_coord(obj: &Value, keys: &[&str], array_index: usize) -> Option<f32> {
    fn scan(v: &Value, keys: &[&str], array_index: usize, depth: usize) -> Option<f32> {
        let Some(map) = v.as_object() else {
            return None;
        };
        for key in keys {
            let Some(child) = map.get(*key) else {
                continue;
            };
            if let Some(n) = json_f32(Some(child)) {
                return Some(n);
            }
            // `"position": [x, y]` / `"pos": [x, y]`
            if let Some(arr) = child.as_array() {
                if arr.len() >= 2 {
                    if let Some(n) = arr.get(array_index).and_then(|n| json_f32(Some(n))) {
                        return Some(n);
                    }
                }
            }
            // `"position": {"value": 1200}`
            if let Some(n) = child.get("value").and_then(|n| json_f32(Some(n))) {
                return Some(n);
            }
        }
        if depth == 0 {
            return None;
        }
        // Preferred containers first so a generic scan cannot shadow them.
        for container in POSITION_CONTAINERS {
            if let Some(inner) = map.get(*container)
                && let Some(n) = scan(inner, keys, array_index, depth - 1)
            {
                return Some(n);
            }
        }
        for (name, inner) in map {
            if POSITION_CONTAINERS.contains(&name.as_str()) {
                continue;
            }
            if inner.is_object()
                && let Some(n) = scan(inner, keys, array_index, depth - 1)
            {
                return Some(n);
            }
        }
        None
    }
    scan(obj, keys, array_index, 3)
}

fn json_f32(v: Option<&Value>) -> Option<f32> {
    v.and_then(|val| {
        val.as_f64()
            .map(|n| n as f32)
            .or_else(|| val.as_str()?.trim().parse::<f32>().ok())
    })
    .filter(|n| n.is_finite())
}

fn normalize_opacity(raw: Option<f64>) -> f32 {
    let v = raw.unwrap_or(0.75) as f32;
    if !v.is_finite() {
        return 0.75;
    }
    if v > 1.0 {
        (v / 100.0).clamp(0.05, 1.0)
    } else {
        v.clamp(0.05, 1.0)
    }
}

fn parse_argb_color(v: Option<&Value>) -> Option<[u8; 4]> {
    let val = v?;
    if let Some(n) = val.as_i64() {
        let u = n as i32 as u32;
        return Some([
            ((u >> 16) & 0xFF) as u8,
            ((u >> 8) & 0xFF) as u8,
            (u & 0xFF) as u8,
            ((u >> 24) & 0xFF) as u8,
        ]);
    }
    let s = val.as_str()?.trim().trim_start_matches('#');
    if s.len() == 8
        && let Ok(u) = u32::from_str_radix(s, 16)
    {
        return Some([
            ((u >> 16) & 0xFF) as u8,
            ((u >> 8) & 0xFF) as u8,
            (u & 0xFF) as u8,
            ((u >> 24) & 0xFF) as u8,
        ]);
    }
    if s.len() == 6
        && let Ok(u) = u32::from_str_radix(s, 16)
    {
        return Some([
            ((u >> 16) & 0xFF) as u8,
            ((u >> 8) & 0xFF) as u8,
            (u & 0xFF) as u8,
            180,
        ]);
    }
    None
}

fn parse_fcl_or_zalith_key(v: &Value) -> Option<String> {
    if let Some(code) = v.as_i64() {
        let code = code as i32;
        // FCL special mouse/scroll codes (1000..=1006)
        return match code {
            1000 => Some("action:key.attack".into()),
            1001 => Some("action:key.pickItem".into()),
            1002 => Some("action:key.use".into()),
            1003 => Some("special:scroll_up".into()),
            1004 => Some("special:scroll_down".into()),
            1005 => Some("special:virtual_mouse".into()),
            1006 => Some("special:keyboard".into()),
            _ => map_java_keycode(code, false),
        };
    }
    let raw = v.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(code) = raw.parse::<i32>() {
        return map_java_keycode(code, false);
    }
    // Zalith 2 click events: `GLFW_KEY_W`, `GLFW_MOUSE_BUTTON_LEFT`, and
    // `launcher.event.*` names. The KEY_/mouse strips below cover the first
    // two; the launcher events are named directly.
    if let Some(event) = raw.strip_prefix("launcher.event.") {
        return match event.to_ascii_lowercase().as_str() {
            "switch_ime" | "keyboard" => Some("special:keyboard".into()),
            "switch_menu" | "menu" => Some("special:menu".into()),
            "scroll_up.single" | "scroll_up" => Some("special:scroll_up".into()),
            "scroll_down.single" | "scroll_down" => Some("special:scroll_down".into()),
            "switch_gui" | "gui" => Some("special:gui_toggle".into()),
            _ => None,
        };
    }
    if CustomButton::valid_token(raw) {
        return Some(raw.to_owned());
    }
    let upper = raw.to_ascii_uppercase();
    // Zalith 2 writes `GLFW_KEY_W`, `GLFW_MOUSE_BUTTON_LEFT` and
    // `GLFW_MOUSE_BUTTON_*`. Stripping only `GLFW_KEY_` left the mouse names
    // unrecognised, so attack/use buttons were dropped from those layouts.
    let stripped = upper
        .strip_prefix("GLFW_KEY_")
        .or_else(|| upper.strip_prefix("GLFW_MOUSE_BUTTON_"))
        .map(|rest| {
            // Rebuild the form the match arms expect.
            match rest {
                "LEFT" => "MOUSE_BUTTON_LEFT",
                "RIGHT" => "MOUSE_BUTTON_RIGHT",
                "MIDDLE" => "MOUSE_BUTTON_MIDDLE",
                other => other,
            }
        })
        .or_else(|| upper.strip_prefix("GLFW_"))
        .or_else(|| upper.strip_prefix("KEY_"))
        .unwrap_or(&upper);
    Some(
        match stripped {
            "MOUSE_BUTTON_LEFT" | "MOUSE_LEFT" | "MOUSE_PRI" | "LEFT_CLICK" | "ATTACK" => {
                "action:key.attack"
            }
            "MOUSE_BUTTON_RIGHT" | "MOUSE_RIGHT" | "MOUSE_SEC" | "RIGHT_CLICK" | "USE" => {
                "action:key.use"
            }
            "MOUSE_BUTTON_MIDDLE" | "MOUSE_MIDDLE" | "MOUSE_MID" | "PICK_ITEM" => {
                "action:key.pickItem"
            }
            "SCROLL_UP" | "MOUSE_SCROLL_UP" => "special:scroll_up",
            "SCROLL_DOWN" | "MOUSE_SCROLL_DOWN" => "special:scroll_down",
            "VIRTUAL_MOUSE" | "MOUSE_MODE" => "special:virtual_mouse",
            "KEYBOARD" | "OPEN_KEYBOARD" | "IME" => "special:keyboard",
            "GUI" | "TOGGLE_CTRL" | "HIDE_CONTROLS" => "special:gui_toggle",
            "MENU" | "GAME_MENU" => "special:menu",
            "SPACE" => "key.keyboard.space",
            "ESCAPE" | "ESC" => "key.keyboard.escape",
            "ENTER" | "RETURN" => "key.keyboard.enter",
            "TAB" => "key.keyboard.tab",
            "BACKSPACE" => "key.keyboard.backspace",
            "LEFT_SHIFT" | "LSHIFT" | "SHIFT" => "key.keyboard.left.shift",
            "RIGHT_SHIFT" | "RSHIFT" => "key.keyboard.right.shift",
            "LEFT_CONTROL" | "LCTRL" | "CTRL" => "key.keyboard.left.control",
            "RIGHT_CONTROL" | "RCTRL" => "key.keyboard.right.control",
            "LEFT_ALT" | "LALT" | "ALT" => "key.keyboard.left.alt",
            "RIGHT_ALT" | "RALT" => "key.keyboard.right.alt",
            "UP" => "key.keyboard.up",
            "DOWN" => "key.keyboard.down",
            "LEFT" => "key.keyboard.left",
            "RIGHT" => "key.keyboard.right",
            "SLASH" => "key.keyboard.slash",
            "GRAVE_ACCENT" => "key.keyboard.grave.accent",
            s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => {
                return Some(format!("key.keyboard.{}", s.to_ascii_lowercase()));
            }
            s if s.starts_with('F')
                && s[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
            {
                return Some(format!("key.keyboard.{}", s.to_ascii_lowercase()));
            }
            _ => return None,
        }
        .into(),
    )
}

/// Maps PojavLauncher / Amethyst / Mojo / GLFW / legacy LWJGL2 keycodes to Volt tokens.
pub fn map_java_keycode(code: i32, legacy_lwjgl2: bool) -> Option<String> {
    if code < 0 {
        return Some(
            match code {
                -1 => "special:keyboard",
                -2 => "special:gui_toggle",
                -3 => "action:key.attack",
                -4 => "action:key.use",
                -5 => "special:virtual_mouse",
                -6 => "action:key.pickItem",
                -7 => "special:scroll_up",
                -8 => "special:scroll_down",
                -9 => "special:menu",
                _ => return None,
            }
            .into(),
        );
    }
    if legacy_lwjgl2 {
        return map_lwjgl2_keycode(code).map(str::to_owned);
    }
    // An unmapped GLFW code (a media key, a layout-specific key this game has
    // no name for) must not delete the button: fall back to a token that
    // keeps the control and can be rebound later.
    map_glfw_keycode(code)
        .map(str::to_owned)
        .or_else(|| Some("key.keyboard.unknown".to_owned()))
}

fn map_glfw_keycode(code: i32) -> Option<&'static str> {
    Some(match code {
        32 => "key.keyboard.space",
        39 => "key.keyboard.apostrophe",
        44 => "key.keyboard.comma",
        45 => "key.keyboard.minus",
        46 => "key.keyboard.period",
        47 => "key.keyboard.slash",
        48 => "key.keyboard.0",
        49 => "key.keyboard.1",
        50 => "key.keyboard.2",
        51 => "key.keyboard.3",
        52 => "key.keyboard.4",
        53 => "key.keyboard.5",
        54 => "key.keyboard.6",
        55 => "key.keyboard.7",
        56 => "key.keyboard.8",
        57 => "key.keyboard.9",
        59 => "key.keyboard.semicolon",
        61 => "key.keyboard.equal",
        65 => "key.keyboard.a",
        66 => "key.keyboard.b",
        67 => "key.keyboard.c",
        68 => "key.keyboard.d",
        69 => "key.keyboard.e",
        70 => "key.keyboard.f",
        71 => "key.keyboard.g",
        72 => "key.keyboard.h",
        73 => "key.keyboard.i",
        74 => "key.keyboard.j",
        75 => "key.keyboard.k",
        76 => "key.keyboard.l",
        77 => "key.keyboard.m",
        78 => "key.keyboard.n",
        79 => "key.keyboard.o",
        80 => "key.keyboard.p",
        81 => "key.keyboard.q",
        82 => "key.keyboard.r",
        83 => "key.keyboard.s",
        84 => "key.keyboard.t",
        85 => "key.keyboard.u",
        86 => "key.keyboard.v",
        87 => "key.keyboard.w",
        88 => "key.keyboard.x",
        89 => "key.keyboard.y",
        90 => "key.keyboard.z",
        91 => "key.keyboard.left.bracket",
        92 => "key.keyboard.backslash",
        93 => "key.keyboard.right.bracket",
        96 => "key.keyboard.grave.accent",
        161 => "key.keyboard.world.1",
        256 => "key.keyboard.escape",
        257 => "key.keyboard.enter",
        258 => "key.keyboard.tab",
        259 => "key.keyboard.backspace",
        260 => "key.keyboard.insert",
        261 => "key.keyboard.delete",
        262 => "key.keyboard.right",
        263 => "key.keyboard.left",
        264 => "key.keyboard.down",
        265 => "key.keyboard.up",
        266 => "key.keyboard.page.up",
        267 => "key.keyboard.page.down",
        268 => "key.keyboard.home",
        269 => "key.keyboard.end",
        280 => "key.keyboard.caps.lock",
        281 => "key.keyboard.scroll.lock",
        282 => "key.keyboard.num.lock",
        283 => "key.keyboard.print.screen",
        284 => "key.keyboard.pause",
        290 => "key.keyboard.f1",
        291 => "key.keyboard.f2",
        292 => "key.keyboard.f3",
        293 => "key.keyboard.f4",
        294 => "key.keyboard.f5",
        295 => "key.keyboard.f6",
        296 => "key.keyboard.f7",
        297 => "key.keyboard.f8",
        298 => "key.keyboard.f9",
        299 => "key.keyboard.f10",
        300 => "key.keyboard.f11",
        301 => "key.keyboard.f12",
        302 => "key.keyboard.f13",
        303 => "key.keyboard.f14",
        304 => "key.keyboard.f15",
        305 => "key.keyboard.f16",
        306 => "key.keyboard.f17",
        307 => "key.keyboard.f18",
        308 => "key.keyboard.f19",
        309 => "key.keyboard.f20",
        310 => "key.keyboard.f21",
        311 => "key.keyboard.f22",
        312 => "key.keyboard.f23",
        313 => "key.keyboard.f24",
        320 => "key.keyboard.keypad.0",
        321 => "key.keyboard.keypad.1",
        322 => "key.keyboard.keypad.2",
        323 => "key.keyboard.keypad.3",
        324 => "key.keyboard.keypad.4",
        325 => "key.keyboard.keypad.5",
        326 => "key.keyboard.keypad.6",
        327 => "key.keyboard.keypad.7",
        328 => "key.keyboard.keypad.8",
        329 => "key.keyboard.keypad.9",
        330 => "key.keyboard.keypad.period",
        331 => "key.keyboard.keypad.divide",
        332 => "key.keyboard.keypad.multiply",
        333 => "key.keyboard.keypad.subtract",
        334 => "key.keyboard.keypad.add",
        335 => "key.keyboard.keypad.enter",
        336 => "key.keyboard.keypad.equal",
        340 => "key.keyboard.left.shift",
        341 => "key.keyboard.left.control",
        342 => "key.keyboard.left.alt",
        343 => "key.keyboard.left.win",
        344 => "key.keyboard.right.shift",
        345 => "key.keyboard.right.control",
        346 => "key.keyboard.right.alt",
        347 => "key.keyboard.right.win",
        348 => "key.keyboard.menu",
        _ => return None,
    })
}

fn map_lwjgl2_keycode(code: i32) -> Option<&'static str> {
    Some(match code {
        1 => "key.keyboard.escape",
        2 => "key.keyboard.1",
        3 => "key.keyboard.2",
        4 => "key.keyboard.3",
        5 => "key.keyboard.4",
        6 => "key.keyboard.5",
        7 => "key.keyboard.6",
        8 => "key.keyboard.7",
        9 => "key.keyboard.8",
        10 => "key.keyboard.9",
        11 => "key.keyboard.0",
        12 => "key.keyboard.minus",
        13 => "key.keyboard.equal",
        14 => "key.keyboard.backspace",
        15 => "key.keyboard.tab",
        16 => "key.keyboard.q",
        17 => "key.keyboard.w",
        18 => "key.keyboard.e",
        19 => "key.keyboard.r",
        20 => "key.keyboard.t",
        21 => "key.keyboard.y",
        22 => "key.keyboard.u",
        23 => "key.keyboard.i",
        24 => "key.keyboard.o",
        25 => "key.keyboard.p",
        26 => "key.keyboard.left.bracket",
        27 => "key.keyboard.right.bracket",
        28 => "key.keyboard.enter",
        29 => "key.keyboard.left.control",
        30 => "key.keyboard.a",
        31 => "key.keyboard.s",
        32 => "key.keyboard.d",
        33 => "key.keyboard.f",
        34 => "key.keyboard.g",
        35 => "key.keyboard.h",
        36 => "key.keyboard.j",
        37 => "key.keyboard.k",
        38 => "key.keyboard.l",
        39 => "key.keyboard.semicolon",
        40 => "key.keyboard.apostrophe",
        41 => "key.keyboard.grave.accent",
        42 => "key.keyboard.left.shift",
        43 => "key.keyboard.backslash",
        44 => "key.keyboard.z",
        45 => "key.keyboard.x",
        46 => "key.keyboard.c",
        47 => "key.keyboard.v",
        48 => "key.keyboard.b",
        49 => "key.keyboard.n",
        50 => "key.keyboard.m",
        51 => "key.keyboard.comma",
        52 => "key.keyboard.period",
        53 => "key.keyboard.slash",
        54 => "key.keyboard.right.shift",
        56 => "key.keyboard.left.alt",
        57 => "key.keyboard.space",
        58 => "key.keyboard.caps.lock",
        59 => "key.keyboard.f1",
        60 => "key.keyboard.f2",
        61 => "key.keyboard.f3",
        62 => "key.keyboard.f4",
        63 => "key.keyboard.f5",
        64 => "key.keyboard.f6",
        65 => "key.keyboard.f7",
        66 => "key.keyboard.f8",
        67 => "key.keyboard.f9",
        68 => "key.keyboard.f10",
        87 => "key.keyboard.f11",
        88 => "key.keyboard.f12",
        157 => "key.keyboard.right.control",
        184 => "key.keyboard.right.alt",
        199 => "key.keyboard.home",
        200 => "key.keyboard.up",
        201 => "key.keyboard.page.up",
        203 => "key.keyboard.left",
        205 => "key.keyboard.right",
        207 => "key.keyboard.end",
        208 => "key.keyboard.down",
        209 => "key.keyboard.page.down",
        210 => "key.keyboard.insert",
        211 => "key.keyboard.delete",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// PojavLauncher Dynamic Coordinate Expression Parser (`${right} - ${margin} * 2 - 50dp`)
// ---------------------------------------------------------------------------

struct ExprVars {
    screen_w: f32,
    screen_h: f32,
    btn_w: f32,
    btn_h: f32,
    dp_px: f32,
    px_scale: f32,
    preferred_scale: f32,
}

fn eval_dynamic_expr(input: &str, vars: &ExprVars, axis_len: f32) -> Option<f32> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 512 {
        return None;
    }
    let mut parser = ExprParser {
        bytes: trimmed.as_bytes(),
        pos: 0,
        vars,
        axis_len,
        depth: 0,
    };
    let val = parser.parse_add_sub()?;
    parser.skip_ws();
    if parser.pos == parser.bytes.len() && val.is_finite() {
        Some(val)
    } else {
        None
    }
}

struct ExprParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    vars: &'a ExprVars,
    axis_len: f32,
    depth: u8,
}

impl ExprParser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn parse_add_sub(&mut self) -> Option<f32> {
        let mut left = self.parse_mul_div()?;
        loop {
            self.skip_ws();
            let Some(&op) = self.bytes.get(self.pos) else {
                break;
            };
            if op != b'+' && op != b'-' {
                break;
            }
            self.pos += 1;
            let right = self.parse_mul_div()?;
            left = if op == b'+' {
                left + right
            } else {
                left - right
            };
        }
        Some(left)
    }

    fn parse_mul_div(&mut self) -> Option<f32> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws();
            let Some(&op) = self.bytes.get(self.pos) else {
                break;
            };
            if op != b'*' && op != b'/' && op != b'%' {
                break;
            }
            self.pos += 1;
            let right = self.parse_unary()?;
            left = match op {
                b'*' => left * right,
                b'/' => {
                    if right.abs() < 1e-6 {
                        0.0
                    } else {
                        left / right
                    }
                }
                _ => {
                    if right.abs() < 1e-6 {
                        0.0
                    } else {
                        left % right
                    }
                }
            };
        }
        Some(left)
    }

    fn parse_unary(&mut self) -> Option<f32> {
        self.skip_ws();
        match self.bytes.get(self.pos).copied() {
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary()
            }
            Some(b'-') => {
                self.pos += 1;
                self.parse_unary().map(|v| -v)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Option<f32> {
        self.skip_ws();
        let b = *self.bytes.get(self.pos)?;
        if b == b'(' {
            if self.depth >= 16 {
                return None;
            }
            self.depth += 1;
            self.pos += 1;
            let val = self.parse_add_sub()?;
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b')') {
                return None;
            }
            self.pos += 1;
            self.depth -= 1;
            return Some(self.apply_unit(val));
        }
        // Pojav 3.3+ and Zalith write unit conversions as functions —
        // `px(58.0)`, `dp(20)`, `dip(4)`, `sp(12)` — and nested in
        // `${...}` values like `${width} / 2`. Without these the whole
        // expression failed to parse, `pojav_box` fell back to a spread
        // slot, and every such button landed on top of the others.
        if b.is_ascii_alphabetic() {
            let start = self.pos;
            while self
                .bytes
                .get(self.pos)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                self.pos += 1;
            }
            let name = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
            if self.bytes.get(self.pos) == Some(&b'(') {
                if self.depth >= 16 {
                    return None;
                }
                self.pos += 1;
                self.depth += 1;
                let inner = self.parse_add_sub()?;
                self.skip_ws();
                if self.bytes.get(self.pos) != Some(&b')') {
                    return None;
                }
                self.pos += 1;
                self.depth -= 1;
                return Some(self.apply_unit_to(name, inner));
            }
            // A bare word is not a number.
            self.pos = start;
            return None;
        }
        if b == b'$' && self.bytes.get(self.pos + 1) == Some(&b'{') {
            self.pos += 2;
            let start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos] != b'}' {
                self.pos += 1;
            }
            if self.bytes.get(self.pos) != Some(&b'}') {
                return None;
            }
            let name = std::str::from_utf8(&self.bytes[start..self.pos])
                .ok()?
                .trim();
            self.pos += 1;
            let val = match name.to_ascii_lowercase().as_str() {
                // Pojav semantics (ControlData.insertDynamicPos): `${width}`
                // and `${height}` are the BUTTON's size; the screen is
                // `${screen_width}`/`${screen_height}`. Reading them as the
                // screen flung every formula layout's buttons off an edge.
                "width" | "btn_width" | "button_width" => self.vars.btn_w,
                "height" | "btn_height" | "button_height" => self.vars.btn_h,
                "screen_width" => self.vars.screen_w,
                "screen_height" => self.vars.screen_h,
                "half_width" => self.vars.screen_w * 0.5,
                "half_height" => self.vars.screen_h * 0.5,
                "right" => (self.vars.screen_w - self.vars.btn_w).max(0.0),
                "bottom" => (self.vars.screen_h - self.vars.btn_h).max(0.0),
                "top" | "left" => 0.0,
                "margin" => 2.0 * self.vars.dp_px,
                "preferred_scale" | "scale" => self.vars.preferred_scale,
                _ => return None,
            };
            return Some(self.apply_unit(val));
        }
        let start = self.pos;
        while let Some(&c) = self.bytes.get(self.pos) {
            if c.is_ascii_digit() || c == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        let num = std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()?
            .parse::<f32>()
            .ok()?;
        Some(self.apply_unit(num))
    }

    /// A unit function's value in reference pixels: `px(n)` is already in
    /// pixels, `dp`/`dip`/`sp` are density-independent.
    fn apply_unit_to(&self, name: &str, value: f32) -> f32 {
        if name.eq_ignore_ascii_case("px") {
            value * self.vars.px_scale
        } else if name.eq_ignore_ascii_case("dp")
            || name.eq_ignore_ascii_case("dip")
            || name.eq_ignore_ascii_case("sp")
        {
            value * self.vars.dp_px
        } else {
            // An unknown function is not a number: fail the parse rather than
            // silently treating it as zero, which would place the button at
            // the origin.
            f32::NAN
        }
    }

    fn apply_unit(&mut self, value: f32) -> f32 {
        let rest = &self.bytes[self.pos..];
        if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"dip") {
            self.pos += 3;
            value * self.vars.dp_px
        } else if rest.len() >= 2
            && (rest[..2].eq_ignore_ascii_case(b"dp") || rest[..2].eq_ignore_ascii_case(b"sp"))
        {
            self.pos += 2;
            value * self.vars.dp_px
        } else if rest.len() >= 2 && rest[..2].eq_ignore_ascii_case(b"px") {
            self.pos += 2;
            value * self.vars.px_scale
        } else if rest.first() == Some(&b'%')
            && !rest.get(1).is_some_and(|c| c.is_ascii_digit())
        {
            self.pos += 1;
            value * self.axis_len / 100.0
        } else {
            value
        }
    }
}

// ---------------------------------------------------------------------------
// Preset Library & Per-Profile Persistence
// ---------------------------------------------------------------------------

fn presets_dir(root_home: &Path) -> PathBuf {
    root_home.join("imported-controls")
}

fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "controls".into()
    } else {
        trimmed.to_owned()
    }
}

pub fn save_preset_and_apply(
    root_home: &Path,
    profile_home: &Path,
    imported: &ImportedControlLayout,
) -> Result<String, String> {
    let dir = presets_dir(root_home);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create preset folder: {e}"))?;
    if std::fs::symlink_metadata(&dir)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Refusing a redirected control preset directory".into());
    }
    let slug = slugify(&imported.meta.name);
    let preset_path = dir.join(format!("{slug}.preset.json"));
    imported
        .settings
        .save_to(&preset_path)
        .map_err(|e| format!("Could not save control preset: {e}"))?;
    prune_presets(&dir, &preset_path);
    apply_to_profile(profile_home, &imported.settings)?;
    Ok(slug)
}

fn prune_presets(dir: &Path, keep: &Path) {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.file_name()?.to_str()?.ends_with(".preset.json") {
                return None;
            }
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, p))
        })
        .collect();
    if files.len() <= MAX_PRESETS {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    for (_, path) in files.into_iter().take(MAX_PRESETS.min(8)) {
        if path != keep {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn list_presets(root_home: &Path) -> Vec<PresetEntry> {
    let dir = presets_dir(root_home);
    let Ok(meta) = std::fs::symlink_metadata(&dir) else {
        return Vec::new();
    };
    if meta.file_type().is_symlink() {
        return Vec::new();
    }
    let mut items: Vec<(std::time::SystemTime, PresetEntry)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name().to_str()?.to_owned();
            let slug = file_name.strip_suffix(".preset.json")?.to_owned();
            let bytes = std::fs::read(entry.path()).ok()?;
            let settings: Settings = serde_json::from_slice(&bytes).ok()?;
            let imported = settings.imported_from?;
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, PresetEntry { slug, meta: imported }))
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().map(|(_, entry)| entry).collect()
}

pub fn load_preset(root_home: &Path, slug: &str) -> Result<Settings, String> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid control preset name".into());
    }
    let path = presets_dir(root_home).join(format!("{slug}.preset.json"));
    if !path.is_file() {
        return Err("Control preset file is no longer available".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let mut settings: Settings =
        serde_json::from_slice(&bytes).map_err(|e| format!("Corrupt preset: {e}"))?;
    settings.sanitize();
    Ok(settings)
}

pub fn delete_preset(root_home: &Path, slug: &str) -> Result<(), String> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid control preset name".into());
    }
    let path = presets_dir(root_home).join(format!("{slug}.preset.json"));
    if !path.is_file() {
        return Err("Control preset file is no longer available".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("Could not delete preset: {e}"))
}

/// Evaluates one Pojav-dialect dynamic position formula (`${margin} * 2 +
/// ${width}`) in the 844x390 dp reference space and returns the normalized
/// `0..1` coordinate of the button's center on that axis. `btn_w_norm` /
/// `btn_h_norm` describe the button's size as fractions of the reference
/// screen. Used by the Controls editor's formula fields so typed formulas
/// behave exactly like an imported Pojav layout.
pub fn eval_position_formula(
    expr: &str,
    btn_w_norm: f32,
    btn_h_norm: f32,
    horizontal: bool,
) -> Option<f32> {
    let vars = ExprVars {
        screen_w: REF_WIDTH_DP,
        screen_h: REF_HEIGHT_DP,
        btn_w: (btn_w_norm * REF_WIDTH_DP).max(1.0),
        btn_h: (btn_h_norm * REF_HEIGHT_DP).max(1.0),
        dp_px: 1.0,
        px_scale: 1.0 / 2.75,
        preferred_scale: 100.0,
    };
    let axis = if horizontal { REF_WIDTH_DP } else { REF_HEIGHT_DP };
    let v = eval_dynamic_expr(expr, &vars, axis)?;
    let center = if horizontal {
        (v + vars.btn_w * 0.5) / REF_WIDTH_DP
    } else {
        (v + vars.btn_h * 0.5) / REF_HEIGHT_DP
    };
    Some(center.clamp(0.01, 0.99))
}

pub fn active_meta(profile_home: &Path) -> Option<ImportedMeta> {
    let bytes = std::fs::read(profile_home.join("horizon-touch.json")).ok()?;
    let settings: Settings = serde_json::from_slice(&bytes).ok()?;
    settings.imported_from
}

pub fn apply_to_profile(profile_home: &Path, settings: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(profile_home).map_err(|e| e.to_string())?;
    if std::fs::symlink_metadata(profile_home)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Refusing a redirected profile directory".into());
    }
    let mut clean = settings.clone();
    clean.sanitize();
    clean
        .save_to(&profile_home.join("horizon-touch.json"))
        .map_err(|e| format!("Could not write touch settings: {e}"))
}

pub fn apply_to_all_profiles(root_home: &Path, settings: &Settings) -> Result<usize, String> {
    let mut count = 0usize;
    apply_to_profile(root_home, settings)?;
    count += 1;
    let profiles_dir = root_home.join("profiles");
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("profile-"))
            {
                apply_to_profile(&p, settings)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

pub fn reset_profile(profile_home: &Path) -> Result<(), String> {
    apply_to_profile(profile_home, &Settings::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::touch::TouchControls;
    use std::io::Write;

    #[test]
    fn pojav_v8_layout_expressions_macros_drawers_and_joystick_import_cleanly() {
        let pojav = serde_json::json!({
            "version": 8,
            "scaledAt": 100.0,
            "mControlDataList": [
                {
                    "name": "GUI",
                    "keycodes": [-2, 0, 0, 0],
                    "dynamicX": "${margin}",
                    "dynamicY": "${top} + ${margin}",
                    "width": 80.0,
                    "height": 30.0,
                    "opacity": 0.8,
                    "isToggle": false,
                    "passThruEnabled": false,
                    "displayInGame": true,
                    "displayInMenu": true,
                    "bgColor": 1291845632
                },
                {
                    "name": "PRI",
                    "keycodes": [-3, 0, 0, 0],
                    "dynamicX": "${right} - ${margin}",
                    "dynamicY": "${bottom} - ${margin} * 2 - 50dp",
                    "width": 65.0,
                    "height": 65.0,
                    "opacity": 75.0,
                    "isToggle": false,
                    "passThruEnabled": true,
                    "displayInGame": true,
                    "displayInMenu": true
                },
                {
                    "name": "SEC",
                    "keycodes": [-4, 0, 0, 0],
                    "dynamicX": "${right} - 80dp",
                    "dynamicY": "${screen_height} * 0.45",
                    "width": 60.0,
                    "height": 60.0,
                    "opacity": 0.7,
                    "passThruEnabled": true,
                    "displayInGame": true,
                    "displayInMenu": true
                },
                {
                    "name": "Sneak",
                    "keycodes": [340, 0, 0, 0],
                    "dynamicX": "20dp",
                    "dynamicY": "${bottom} - 120dp",
                    "width": 50.0,
                    "height": 50.0,
                    "isToggle": true,
                    "displayInGame": true,
                    "displayInMenu": true
                },
                {
                    "name": "Hitbox",
                    "keycodes": [292, 66, 0, 0],
                    "dynamicX": "${screen_width} * 0.5 - 25dp",
                    "dynamicY": "${margin}",
                    "width": 50.0,
                    "height": 30.0,
                    "displayInGame": true,
                    "displayInMenu": false
                },
                {
                    "name": "Esc",
                    "keycodes": [256, 0, 0, 0],
                    "dynamicX": "${right} - ${margin}",
                    "dynamicY": "${margin}",
                    "width": 50.0,
                    "height": 30.0,
                    "displayInGame": true,
                    "displayInMenu": true
                }
            ],
            "mDrawerDataList": [
                {
                    "orientation": "DOWN",
                    "properties": {
                        "name": "Extras",
                        "keycodes": [0, 0, 0, 0],
                        "dynamicX": "100dp",
                        "dynamicY": "${margin}",
                        "width": 60.0,
                        "height": 30.0,
                        "displayInGame": true,
                        "displayInMenu": false
                    },
                    "buttonProperties": [
                        {
                            "name": "3rd",
                            "keycodes": [294, 0, 0, 0],
                            "width": 60.0,
                            "height": 30.0,
                            "displayInGame": true,
                            "displayInMenu": false
                        }
                    ]
                }
            ],
            "mJoystickDataList": [
                {
                    "dynamicX": "24dp",
                    "dynamicY": "${bottom} - 140dp",
                    "width": 120.0,
                    "height": 120.0,
                    "opacity": 0.65,
                    "forwardLock": true,
                    "absolute": false
                }
            ]
        });

        let imported = import_bytes(&serde_json::to_vec(&pojav).unwrap(), "PvP_Claw.json").unwrap();
        assert_eq!(imported.meta.name, "PvP_Claw");
        assert_eq!(imported.meta.format, "PojavLauncher");
        assert_eq!(imported.meta.drawers, 1);
        assert!(imported.meta.has_joystick);
        assert_eq!(imported.settings.custom_buttons.len(), 8);
        assert!(imported.settings.inventory_controls);
        assert!(!imported.settings.crosshair_gestures);
        assert!(imported.settings.sprint_with_joystick);
        assert_eq!(
            imported.settings.button_visibility[Control::Move as usize],
            1
        );
        assert_eq!(
            imported.settings.button_visibility[Control::Jump as usize],
            2
        );

        let mut touch = TouchControls {
            settings: imported.settings.clone(),
            loaded: true,
            ..Default::default()
        };
        touch.viewport(2400, 1080);

        // Tap PRI button & drag -> emits attack AND rotates camera via passthrough
        let pri_center = touch.settings.custom_buttons[1].layouts[0].center;
        touch.update(1, pri_center, true, false);
        assert_eq!(touch.key_events(), [("action:key.attack".into(), true)]);
        let look = touch.update(1, [pri_center[0] - 0.04, pri_center[1]], false, false);
        assert!(look[0] < -0.02);
        touch.update(1, pri_center, false, true);
        assert_eq!(touch.key_events(), [("action:key.attack".into(), false)]);

        // Tap Hitbox (F3 + B) -> emits F3 then B in order
        let hb_center = touch.settings.custom_buttons[4].layouts[0].center;
        touch.update(2, hb_center, true, false);
        assert_eq!(
            touch.key_events(),
            [
                ("key.keyboard.f3".into(), true),
                ("key.keyboard.b".into(), true)
            ]
        );
        touch.update(2, hb_center, false, true);
        assert_eq!(
            touch.key_events(),
            [
                ("key.keyboard.b".into(), false),
                ("key.keyboard.f3".into(), false)
            ]
        );

        // Tap GUI toggle -> hides other buttons while keeping GUI button reachable
        let gui_center = touch.settings.custom_buttons[0].layouts[0].center;
        touch.update(3, gui_center, true, false);
        touch.update(3, gui_center, false, true);
        assert!(touch.controls_hidden);
        assert_eq!(touch.custom_at(pri_center), None);
        assert_eq!(touch.custom_at(gui_center), Some(0));
    }

    #[test]
    fn fcl_and_zalith_zip_archives_import_and_roundtrip_presets() {
        let fcl = serde_json::json!({
            "manifest": { "name": "FCL Bedwars" },
            "viewGroups": [
                {
                    "id": "main",
                    "visibility": "VISIBLE",
                    "viewData": {
                        "buttonList": [
                            {
                                "text": "Attack",
                                "baseInfo": {
                                    "xPosition": 900,
                                    "yPosition": 650,
                                    "width": 600,
                                    "height": 600,
                                    "sizeType": "ABSOLUTE",
                                    "visibilityType": "ALWAYS"
                                },
                                "event": {
                                    "pressEvent": {
                                        "autoKeep": false,
                                        "movable": true,
                                        "outputKeycodes": [1000]
                                    }
                                }
                            },
                            {
                                "text": "Jump",
                                "baseInfo": {
                                    "xPosition": 880,
                                    "yPosition": 850,
                                    "width": 500,
                                    "height": 500,
                                    "sizeType": "ABSOLUTE",
                                    "visibilityType": "IN_GAME"
                                },
                                "event": {
                                    "outputKeycodes": [32]
                                }
                            }
                        ],
                        "directionList": [
                            {
                                "styleType": "ROCKER",
                                "baseInfo": {
                                    "xPosition": 120,
                                    "yPosition": 750,
                                    "width": 1200,
                                    "height": 1200,
                                    "sizeType": "ABSOLUTE"
                                }
                            }
                        ]
                    }
                }
            ]
        });
        let fcl_res = import_bytes(&serde_json::to_vec(&fcl).unwrap(), "bedwars.json").unwrap();
        assert_eq!(fcl_res.meta.name, "FCL Bedwars");
        assert_eq!(fcl_res.meta.format, "Fold Craft (FCL)");
        assert_eq!(fcl_res.settings.custom_buttons.len(), 2);
        assert!(fcl_res.settings.custom_buttons[0].passthrough);
        assert!(!fcl_res.settings.custom_buttons[1].in_menu);

        // Wrap a Zalith 2 layout inside a real ZIP archive
        let zalith = serde_json::json!({
            "info": { "name": "Zalith Speedrun" },
            "layers": [
                {
                    "hide": false,
                    "normalButtons": [
                        {
                            "text": "Swap",
                            "x": 8200,
                            "y": 4000,
                            "width": 50,
                            "height": 50,
                            "visibilityType": "IN_GAME",
                            "clickEvents": [{ "key": "GLFW_KEY_F" }]
                        },
                        {
                            "text": "Mouse",
                            "x": 500,
                            "y": 500,
                            "width": 60,
                            "height": 32,
                            "visibilityType": "IN_MENU",
                            "clickEvents": [{ "key": "VIRTUAL_MOUSE" }]
                        }
                    ]
                }
            ]
        });
        let mut zip_bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut zip_bytes));
            let opts = zip::write::SimpleFileOptions::default();
            writer.start_file("manifest.json", opts).unwrap();
            writer
                .write_all(br#"{"name":"Zalith Pack"}"#)
                .unwrap();
            writer.start_file("controls/layout.json", opts).unwrap();
            writer
                .write_all(&serde_json::to_vec(&zalith).unwrap())
                .unwrap();
            writer.finish().unwrap();
        }
        let zip_res = import_bytes(&zip_bytes, "speedrun.zip").unwrap();
        assert_eq!(zip_res.meta.format, "Zalith Launcher");
        assert_eq!(zip_res.settings.custom_buttons.len(), 2);
        assert_eq!(zip_res.settings.custom_buttons[0].key, "key.keyboard.f");
        assert_eq!(
            zip_res.settings.custom_buttons[1].key,
            "special:virtual_mouse"
        );
    }

    /// Pojav's stock `default.json` (v4). Every button is formula-positioned,
    /// so this pins the exact `${width}`/`${height}` = BUTTON-size semantics
    /// from Pojav's `ControlData.fillConversionMap` (dpToPx(width), margin =
    /// dpToPx(2), right = screen - width). Before the fix these buttons all
    /// clamped to screen edges.
    /// A control with no stored position used to land at exact screen centre,
    /// so every such control stacked on one spot. Nothing may sit dead centre
    /// unless the file actually says so.
    #[test]
    fn unpositioned_controls_do_not_all_stack_in_the_middle() {
        let pojav = serde_json::json!({
            "version": 5,
            "scaledAt": 100.0,
            "mControlDataList": [
                // No x, y, dynamicX or dynamicY at all.
                { "name": "A", "width": 50.0, "height": 50.0, "keycodes": [32] },
                { "name": "B", "width": 50.0, "height": 50.0, "keycodes": [340] },
                { "name": "C", "width": 50.0, "height": 50.0, "keycodes": [69] },
            ],
        });
        let imported =
            import_bytes(&serde_json::to_vec(&pojav).unwrap(), "NoPos.json").unwrap();
        let centers: Vec<[f32; 2]> = imported
            .settings
            .custom_buttons
            .iter()
            .map(|b| b.layouts[0].center)
            .collect();
        assert_eq!(centers.len(), 3);
        for c in &centers {
            assert!(
                (c[0] - 0.5).abs() > 0.02 || (c[1] - 0.5).abs() > 0.02,
                "control landed at dead centre: {c:?}"
            );
        }
        // And they must not all share one slot either.
        for (i, a) in centers.iter().enumerate() {
            for b in &centers[i + 1..] {
                assert!(
                    (a[0] - b[0]).abs() > 0.01 || (a[1] - b[1]).abs() > 0.01,
                    "two unpositioned controls share a slot: {a:?} {b:?}"
                );
            }
        }
        // Whatever the fallback, everything stays on screen.
        for c in &centers {
            assert!((0.0..=1.0).contains(&c[0]), "off screen x: {c:?}");
            assert!((0.0..=1.0).contains(&c[1]), "off screen y: {c:?}");
        }
    }

    /// Pojav never stores x/y: it stores `dynamicX`/`dynamicY` expressions
    /// and evaluates them at load. Real files also use px()/dp() calls and
    /// `n dp` suffixes. Every one of these must land at its own spot — if an
    /// expression fails to parse the button falls back to a spread slot, and
    /// a whole layout of parse failures looks like everything stacked up.
    #[test]
    fn pojav_dynamic_expressions_place_each_button_at_its_own_spot() {
        let pojav = serde_json::json!({
            "version": 4,
            "scaledAt": 100,
            "mControlDataList": [
                { "name": "Keyboard", "width": 80, "height": 30, "keycodes": [-1],
                  "dynamicX": "${margin} * 3 + ${width} * 2", "dynamicY": "${margin}" },
                { "name": "GUI", "width": 50, "height": 50, "keycodes": [-2],
                  "dynamicX": "${margin}", "dynamicY": "${bottom} - ${margin}" },
                { "name": "PRI", "width": 50, "height": 50, "keycodes": [-3],
                  "dynamicX": "${margin}", "dynamicY": "${screen_height} - ${margin} * 3 - ${height} * 3" },
                { "name": "SEC", "width": 50, "height": 50, "keycodes": [-4],
                  "dynamicX": "${margin} * 3 + ${width} * 2", "dynamicY": "${screen_height} - ${margin} * 3 - ${height} * 3" },
                { "name": "Mouse", "width": 80, "height": 30, "keycodes": [-5],
                  "dynamicX": "${right}", "dynamicY": "${margin}" },
                { "name": "Sneak", "width": 50, "height": 50, "keycodes": [340],
                  "dynamicX": "20dp", "dynamicY": "${bottom} - 120dp" },
                { "name": "Space", "width": 50, "height": 50, "keycodes": [32],
                  "dynamicX": "${right} - ${margin} * 2 - ${width}",
                  "dynamicY": "${bottom} - ${margin} * 2 - ${height}" },
                // Pojav 3.3+ / Zalith unit-function syntax.
                { "name": "Fn", "width": 50, "height": 50, "keycodes": [70],
                  "dynamicX": "px(58.0) / 100.0 * ${preferred_scale} + dp(20)",
                  "dynamicY": "dip(10) * 2" },
            ],
        });
        let imported =
            import_bytes(&serde_json::to_vec(&pojav).unwrap(), "dyn.json").unwrap();
        let centers: Vec<[f32; 2]> = imported
            .settings
            .custom_buttons
            .iter()
            .map(|b| b.layouts[0].center)
            .collect();
        assert_eq!(centers.len(), 8, "every control must survive the import");
        for (i, c) in centers.iter().enumerate() {
            assert!(
                (c[0] - 0.5).abs() > 0.02 || (c[1] - 0.5).abs() > 0.02,
                "control {i} landed at dead centre: {c:?}"
            );
            assert!((0.0..=1.0).contains(&c[0]), "control {i} off screen: {c:?}");
            assert!((0.0..=1.0).contains(&c[1]), "control {i} off screen: {c:?}");
        }
        for (i, a) in centers.iter().enumerate() {
            for (j, b) in centers.iter().enumerate().skip(i + 1) {
                assert!(
                    (a[0] - b[0]).abs() > 0.005 || (a[1] - b[1]).abs() > 0.005,
                    "controls {i} and {j} share a slot: {a:?} {b:?}"
                );
            }
        }
        // The left-hand pair and the right-hand pair must actually be on
        // opposite sides, not nudged into the same half.
        assert!(centers[0][0] < 0.4, "Keyboard should be on the left: {:?}", centers[0]);
        assert!(centers[4][0] > 0.6, "Mouse should be on the right: {:?}", centers[4][0]);
    }

    /// Every button at the same spot is the extreme of "the controls are
    /// cramped and overlap": they must come out separated, never stacked.
    #[test]
    fn identical_import_positions_are_separated() {
        let stacked = serde_json::json!({
            "version": 4,
            "mControlDataList": [
                { "name": "A", "width": 50, "height": 50, "keycodes": [87], "x": 400, "y": 200 },
                { "name": "B", "width": 50, "height": 50, "keycodes": [83], "x": 400, "y": 200 },
                { "name": "C", "width": 50, "height": 50, "keycodes": [65], "x": 400, "y": 200 },
                { "name": "D", "width": 50, "height": 50, "keycodes": [68], "x": 400, "y": 200 },
            ],
        });
        let imported = import_bytes(&serde_json::to_vec(&stacked).unwrap(), "stack.json").unwrap();
        assert_eq!(imported.meta.buttons, 4);
        assert_eq!(
            imported.meta.distinct, 4,
            "coincident controls must not share a slot: {:?}",
            imported
                .settings
                .custom_buttons
                .iter()
                .map(|b| b.layouts[0].center)
                .collect::<Vec<_>>()
        );
    }

    /// A layout authored for a wider screen lands in a band on the left with
    /// an empty strip down the right, which is what made every control feel
    /// cramped. It is stretched onto the whole screen instead.
    #[test]
    fn underfilled_import_is_stretched_to_the_screen() {
        let narrow = |name: &str, key: i32, x: f32| {
            serde_json::json!({
                "name": name, "width": 50.0, "height": 50.0, "keycodes": [key],
                "x": x, "y": 0.15,
            })
        };
        let file = serde_json::json!({
            "version": 4,
            "mControlDataList": [
                narrow("A", 87, 0.10),
                narrow("B", 83, 0.20),
                narrow("C", 65, 0.30),
                narrow("D", 68, 0.40),
            ],
        });
        let imported = import_bytes(&serde_json::to_vec(&file).unwrap(), "narrow.json").unwrap();
        let xs: Vec<f32> = imported
            .settings
            .custom_buttons
            .iter()
            .map(|b| b.layouts[0].center[0])
            .collect();
        let lo = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            hi - lo > 0.8,
            "a squeezed layout must fill the screen width, got {xs:?}"
        );
        assert!(hi > 0.9, "the right-hand control must reach the right edge: {xs:?}");
    }

    /// Zalith Launcher 2 layout: `editorVersion` + `layers`, geometry nested
    /// under `position: {x, y}` on a 0..=10000 free-space scale and size under
    /// `buttonSize`. None of those keys is `x`, `y`, `dynamicX` or
    /// `dynamicY`, so the old parser fell back for every single button and the
    /// layout appeared as two neat columns — "towers" of controls.
    #[test]
    fn zalith2_nested_positions_place_every_button() {
        let zl2 = serde_json::json!({
            "info": { "name": { "default": "ZL2 Test" }, "author": { "default": "" },
                      "description": { "default": "" }, "versionCode": 1, "versionName": "1.0" },
            "editorVersion": 12,
            "styles": [],
            "joystickStyles": [],
            "layers": [{
                "name": "Layer 1", "uuid": "abc", "hide": false,
                "hideWhenMouse": true, "hideWhenGamepad": true, "visibilityType": "always",
                "normalButtons": [
                    { "text": { "default": "W" }, "uuid": "b1",
                      "position": { "x": 1200, "y": 1000 },
                      "buttonSize": { "type": "dp", "widthDp": 50.0, "heightDp": 50.0 },
                      "visibilityType": "always",
                      "clickEvents": [{ "type": "key", "key": "GLFW_KEY_W" }],
                      "isSwipple": false, "isPenetrable": false, "isToggleable": false },
                    { "text": { "default": "S" }, "uuid": "b2",
                      "position": { "x": 1200, "y": 6000 },
                      "buttonSize": { "type": "dp", "widthDp": 50.0, "heightDp": 50.0 },
                      "visibilityType": "always",
                      "clickEvents": [{ "type": "key", "key": "GLFW_KEY_S" }],
                      "isSwipple": false, "isPenetrable": false, "isToggleable": false },
                    { "text": { "default": "Jump" }, "uuid": "b3",
                      "position": { "x": 8800, "y": 7500 },
                      "buttonSize": { "type": "dp", "widthDp": 60.0, "heightDp": 60.0 },
                      "visibilityType": "in_game",
                      "clickEvents": [{ "type": "key", "key": "GLFW_KEY_SPACE" }],
                      "isSwipple": false, "isPenetrable": false, "isToggleable": false },
                    { "text": { "default": "Attack" }, "uuid": "b4",
                      "position": { "x": 9000, "y": 2000 },
                      "buttonSize": { "type": "dp", "widthDp": 70.0, "heightDp": 70.0 },
                      "visibilityType": "always",
                      "clickEvents": [{ "type": "launcher_event", "key": "GLFW_MOUSE_BUTTON_LEFT" }],
                      "isSwipple": false, "isPenetrable": false, "isToggleable": false },
                ],
                "textBoxes": [],
                "joystickButtons": [],
            }],
        });
        let imported = import_bytes(&serde_json::to_vec(&zl2).unwrap(), "zl2.json").unwrap();
        let centers: Vec<[f32; 2]> = imported
            .settings
            .custom_buttons
            .iter()
            .map(|b| b.layouts[0].center)
            .collect();
        assert_eq!(centers.len(), 4, "all four ZL2 buttons must import");
        // 1200/10000 = 0.12 of free space -> left side; 9000/10000 -> right.
        assert!(centers[0][0] < 0.35, "W should be on the left: {:?}", centers[0]);
        assert!(centers[1][0] < 0.35, "S should be on the left: {:?}", centers[1]);
        assert!(centers[2][0] > 0.65, "Jump should be on the right: {:?}", centers[2]);
        assert!(centers[3][0] > 0.65, "Attack should be on the right: {:?}", centers[3]);
        assert!(centers[0][1] < centers[1][1], "W must sit above S");
        assert!(centers[3][1] < centers[2][1], "Attack must sit above Jump");
        // The spread fallback would put everything at 0.28/0.72 on a fixed
        // ladder; a real parse yields distinct free-space fractions.
        for (i, a) in centers.iter().enumerate() {
            for (j, b) in centers.iter().enumerate().skip(i + 1) {
                assert!(
                    (a[0] - b[0]).abs() > 0.005 || (a[1] - b[1]).abs() > 0.005,
                    "ZL2 controls {i} and {j} share a slot: {a:?} {b:?}"
                );
            }
        }
        assert_eq!(imported.meta.distinct, 4, "metadata must report 4 distinct slots");
        // Labels come from `text.default`, not from a string `text`.
        let keys: Vec<&str> = imported.settings.custom_buttons.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.iter().any(|k| k.contains('w')), "GLFW_KEY_W must map: {keys:?}");
        assert!(keys.iter().any(|k| k.contains("attack")), "mouse-left must map: {keys:?}");
    }

    /// Unknown launcher schemas must still place buttons by magnitude rather
    /// than dropping into the fallback grid: `position`-style nesting under
    /// arbitrary container names, percent strings, and 0..=1 fractions.
    #[test]
    fn unknown_layout_shapes_still_get_coordinates() {
        let odd = serde_json::json!({
            "version": 9,
            "mControlDataList": [
                { "name": "A", "width": 50, "height": 50, "keycodes": [87],
                  "rect": { "x": 0.10, "y": 0.20 } },
                { "name": "B", "width": 50, "height": 50, "keycodes": [83],
                  "x": "80%", "y": "70%" },
                { "name": "C", "width": 50, "height": 50, "keycodes": [65],
                  "positionX": 4000, "positionY": 5000 },
                { "name": "D", "width": 50, "height": 50, "keycodes": [68],
                  "bounds": { "left": 1500, "top": 8000 } },
            ],
        });
        let imported = import_bytes(&serde_json::to_vec(&odd).unwrap(), "odd.json").unwrap();
        assert_eq!(imported.meta.buttons, 4);
        assert_eq!(
            imported.meta.distinct, 4,
            "every odd-shaped button needs its own slot, got {:?}",
            imported
                .settings
                .custom_buttons
                .iter()
                .map(|b| b.layouts[0].center)
                .collect::<Vec<_>>()
        );
    }

    /// A Pojav file with no `displayInMenu` field means "show on menu
    /// screens": defaulting it to false hid every imported button in the
    /// inventory editor, so a complete layout looked nearly empty.
    #[test]
    fn imported_pojav_buttons_stay_visible_on_menu_screens() {
        let pojav = serde_json::json!({
            "version": 5,
            "scaledAt": 100.0,
            "mControlDataList": [
                { "name": "W", "width": 50.0, "height": 50.0, "keycodes": [87],
                  "dynamicX": "10.0", "dynamicY": "10.0" },
                { "name": "SPACE", "width": 80.0, "height": 40.0, "keycodes": [32],
                  "dynamicX": "10.0", "dynamicY": "70.0" },
            ],
        });
        let imported =
            import_bytes(&serde_json::to_vec(&pojav).unwrap(), "menu-vis.json").unwrap();
        assert_eq!(imported.settings.custom_buttons.len(), 2);
        for b in &imported.settings.custom_buttons {
            assert!(b.in_menu, "{} was hidden on menu screens", b.key);
            assert!(b.in_game, "{} was hidden in game", b.key);
        }
        // An explicit false is still honoured.
        let explicit = serde_json::json!({
            "version": 5,
            "scaledAt": 100.0,
            "mControlDataList": [
                { "name": "Quiet", "width": 50.0, "height": 50.0, "keycodes": [81],
                  "displayInMenu": false, "dynamicX": "10.0", "dynamicY": "10.0" },
            ],
        });
        let imported =
            import_bytes(&serde_json::to_vec(&explicit).unwrap(), "explicit.json").unwrap();
        assert!(!imported.settings.custom_buttons[0].in_menu);
    }

    /// A launcher control this game has no key name for must still import:
    /// dropping it silently made a full layout look sparse and shifted every
    /// later button's slot.
    #[test]
    fn unmappable_buttons_are_kept_and_rebindable() {
        let pojav = serde_json::json!({
            "version": 5,
            "scaledAt": 100.0,
            "mControlDataList": [
                // 0x1000 is not a GLFW key this game names.
                { "name": "Media", "width": 50.0, "height": 50.0, "keycodes": [4096],
                  "dynamicX": "10.0", "dynamicY": "10.0" },
                // A name but no key at all.
                { "name": "Sneak", "width": 50.0, "height": 50.0,
                  "dynamicX": "10.0", "dynamicY": "70.0" },
                // Nothing whatsoever: not a control.
                { "width": 50.0, "height": 50.0,
                  "dynamicX": "10.0", "dynamicY": "90.0" },
            ],
        });
        let imported =
            import_bytes(&serde_json::to_vec(&pojav).unwrap(), "unmapped.json").unwrap();
        let kept = &imported.settings.custom_buttons;
        assert_eq!(kept.len(), 2, "only the empty entry should be dropped");
        assert!(kept.iter().all(|b| b.key == "key.keyboard.unknown"));
        assert_eq!(kept[0].label.as_deref(), Some("Media"));
        assert_eq!(kept[1].label.as_deref(), Some("Sneak"));
        // The placeholder token survives sanitize, so the row is still there
        // after the import is applied.
        assert!(kept
            .iter()
            .all(|b| crate::touch::CustomButton::valid_token(&b.key)));
    }

    #[test]
    fn pojav_stock_default_layout_imports_one_to_one() {
        let bytes = include_bytes!("fixtures/pojav-default.json");
        let imported = import_bytes(bytes, "default.json").unwrap();
        assert_eq!(imported.meta.format, "PojavLauncher");
        assert_eq!(imported.settings.custom_buttons.len(), 16);

        let center_of = |label: &str| -> [f32; 2] {
            imported
                .settings
                .custom_buttons
                .iter()
                .find(|b| b.label.as_deref() == Some(label))
                .unwrap_or_else(|| panic!("missing button {label}"))
                .layouts[0]
                .center
        };
        let close = |got: [f32; 2], want: [f32; 2]| {
            assert!(
                (got[0] - want[0]).abs() < 0.002 && (got[1] - want[1]).abs() < 0.002,
                "center {got:?} != expected {want:?}"
            );
        };

        // Reference space 844x390dp, margin = 2dp.
        // Keyboard 80x30: x = 2*3 + 80*2 = 166, y = 2.
        close(center_of("Keyboard"), [206.0 / 844.0, 17.0 / 390.0]);
        // GUI 50x50: x = 2, y = bottom - margin = (390-50) - 2 = 338.
        close(center_of("GUI"), [27.0 / 844.0, 363.0 / 390.0]);
        // PRI 50x50: y = screen_height - margin*3 - height*3 = 234.
        close(center_of("PRI"), [27.0 / 844.0, 259.0 / 390.0]);
        // Jump 50x50: x = right - margin*2 - width = 794-4-50 = 740,
        //             y = bottom - margin*2 - height = 340-4-50 = 286.
        close(center_of("\u{2b1b}"), [765.0 / 844.0, 311.0 / 390.0]);
        // Chat 80x30: x = margin*2 + width = 84, y = margin.
        close(center_of("Chat"), [124.0 / 844.0, 17.0 / 390.0]);

        // Special negative keycodes resolve to Volt tokens.
        let key_of = |label: &str| -> String {
            imported
                .settings
                .custom_buttons
                .iter()
                .find(|b| b.label.as_deref() == Some(label))
                .unwrap()
                .key
                .clone()
        };
        assert_eq!(key_of("Keyboard"), "special:keyboard");
        assert_eq!(key_of("PRI"), "action:key.attack");
        assert_eq!(key_of("SEC"), "action:key.use");
        assert_eq!(key_of("Mouse"), "special:virtual_mouse");
    }
}
