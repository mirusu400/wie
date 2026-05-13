"""One-shot generator: prints the #[options(...)] attribute stack and the
SLOTS / KEYCODE_VALUES tables for src/options.rs. Run on layout change.

Output is paste-into-rust text; not compile-time-included.
"""

# Order matters — must match SLOTS table in options.rs.
SLOTS = [
    ("up",     "UP",     "D-Pad Up"),
    ("down",   "DOWN",   "D-Pad Down"),
    ("left",   "LEFT",   "D-Pad Left"),
    ("right",  "RIGHT",  "D-Pad Right"),
    ("a",      "A",      "A (OK)"),
    ("b",      "B",      "B (Clear)"),
    ("x",      "X",      "X (#)"),
    ("y",      "Y",      "Y (0)"),
    ("l",      "L",      "L (Soft Left)"),
    ("r",      "R",      "R (Soft Right)"),
    ("start",  "START",  "Start (Call)"),
    ("select", "SELECT", "Select (Hangup)"),
    ("l2",     "L2",     "L2 (1)"),
    ("r2",     "R2",     "R2 (3)"),
    ("l3",     "L3",     "L3 (7)"),
    ("r3",     "R3",     "R3 (9)"),
]

# KeyCode choices offered per slot. "None" disables the slot.
KEYCODES = [
    "None", "UP", "DOWN", "LEFT", "RIGHT", "OK",
    "LEFT_SOFT_KEY", "RIGHT_SOFT_KEY", "CLEAR", "CALL", "HANGUP",
    "NUM0", "NUM1", "NUM2", "NUM3", "NUM4", "NUM5",
    "NUM6", "NUM7", "NUM8", "NUM9", "HASH", "STAR",
    "VOLUME_UP", "VOLUME_DOWN",
]

# Preset name → per-slot KeyCode mapping (first key = primary).
PRESETS = {
    "Phone Keypad (default)": {
        "up": "UP", "down": "DOWN", "left": "LEFT", "right": "RIGHT",
        "a": "OK", "b": "CLEAR", "x": "HASH", "y": "NUM0",
        "l": "LEFT_SOFT_KEY", "r": "RIGHT_SOFT_KEY",
        "start": "CALL", "select": "HANGUP",
        "l2": "NUM1", "r2": "NUM3", "l3": "NUM7", "r3": "NUM9",
    },
    "Numpad Only": {
        "up": "NUM2", "down": "NUM8", "left": "NUM4", "right": "NUM6",
        "a": "NUM5", "b": "STAR", "x": "HASH", "y": "NUM0",
        "l": "LEFT_SOFT_KEY", "r": "RIGHT_SOFT_KEY",
        "start": "CALL", "select": "HANGUP",
        "l2": "NUM1", "r2": "NUM3", "l3": "NUM7", "r3": "NUM9",
    },
    "D-Pad Nav Only": {
        "up": "UP", "down": "DOWN", "left": "LEFT", "right": "RIGHT",
        "a": "OK", "b": "CLEAR", "x": "HASH", "y": "NUM0",
        "l": "LEFT_SOFT_KEY", "r": "RIGHT_SOFT_KEY",
        "start": "CALL", "select": "HANGUP",
        "l2": "NUM1", "r2": "NUM3", "l3": "NUM7", "r3": "NUM9",
    },
    "Custom": {},  # use per-slot options
}

PRESET_KEY = "wie_input_layout"
CATEGORY = "input_settings"


def fmt_options():
    out = []
    # 1) preset option
    values = ",\n        ".join(f'{{ "{n}" }}' for n in PRESETS)
    out.append(f'''#[options({{
    "{PRESET_KEY}",
    "Input > Layout",
    "Layout Preset",
    "Pick a preset or Custom for per-slot dropdowns below.",
    "Pick a preset or Custom for per-slot mappings.",
    "{CATEGORY}",
    {{
        {values},
    }}
}})]''')
    # 2) per-slot options
    kc_values = ",\n        ".join(f'{{ "{k}" }}' for k in KEYCODES)
    for slot_id, _, label in SLOTS:
        key = f"wie_input_{slot_id}"
        out.append(f'''#[options({{
    "{key}",
    "Input > {label}",
    "{label}",
    "WIPI key sent when this RetroPad slot is pressed (used only when Layout = Custom).",
    "WIPI key sent when this slot is pressed (Custom layout only).",
    "{CATEGORY}",
    {{
        {kc_values},
    }}
}})]''')
    return "\n".join(out)


def fmt_slots_table():
    rows = ",\n    ".join(
        f'("wie_input_{sid}", JoypadState::{js})' for sid, js, _ in SLOTS
    )
    return f"pub const SLOTS: &[(&str, JoypadState)] = &[\n    {rows},\n];"


def fmt_presets_table():
    lines = []
    for name, mapping in PRESETS.items():
        if not mapping:
            lines.append(f'    ("{name}", &[]),')
            continue
        # Index matches SLOTS order. None = use whatever value sits in the
        # per-slot core option (lets a preset partially override).
        cells = ", ".join(f"KeyCode::{mapping[sid]}" for sid, _, _ in SLOTS)
        lines.append(f'    ("{name}", &[{cells}]),')
    body = "\n".join(lines)
    return f"pub const PRESETS: &[(&str, &[KeyCode])] = &[\n{body}\n];"


def write_options_rs(path: str):
    """Generate the full src/options.rs — both the #[options(...)] stack and
    the slot/preset tables — so the rust file stays in sync with this script.
    Do not hand-edit options.rs; edit this generator and re-run.
    """
    body = f"""//! Auto-generated from `options_gen.py`. Do not hand-edit; run the
//! generator instead. Hosts the libretro core options derive + the
//! preset/slot tables consumed by `input.rs`.

#![allow(clippy::needless_raw_string_hashes)]

// The `CoreOptions` derive macro expands to code referencing libc::c_char,
// SetEnvironmentContext, GenericContext, and a handful of `retro_*` sys
// structs — keep all three globs in scope or the macro fails to expand.
use rust_libretro::{{contexts::*, proc::CoreOptions, sys::*, types::JoypadState}};
use wie_backend::KeyCode;

{fmt_options()}
#[derive(CoreOptions, Default)]
pub struct Options;

{fmt_slots_table()}

{fmt_presets_table()}

pub const PRESET_KEY: &str = "{PRESET_KEY}";
pub const CUSTOM_PRESET: &str = "Custom";
pub const DEFAULT_PRESET: &str = "Phone Keypad (default)";

/// Build the active (JoypadState, KeyCode) mapping from the current
/// frontend variable values. Called once at init and whenever
/// `on_options_changed` fires.
pub fn resolve_mapping(ctx: &OptionsChangedContext) -> Vec<(JoypadState, KeyCode)> {{
    let preset = ctx.get_variable(PRESET_KEY).unwrap_or(DEFAULT_PRESET);
    let preset_row = PRESETS.iter().find(|(n, _)| *n == preset);
    let mut out = Vec::with_capacity(SLOTS.len());
    for (i, (key, state)) in SLOTS.iter().enumerate() {{
        // Custom layout (or any preset whose row is empty) reads per-slot
        // options; otherwise the preset row supplies the KeyCode directly.
        let kc = match preset_row {{
            Some((_, row)) if !row.is_empty() => Some(row[i]),
            _ => ctx.get_variable(key).and_then(parse_keycode),
        }};
        if let Some(kc) = kc {{
            out.push((*state, kc));
        }}
    }}
    out
}}

fn parse_keycode(s: &str) -> Option<KeyCode> {{
    match s {{
        "None" => None,
        "UP" => Some(KeyCode::UP),
        "DOWN" => Some(KeyCode::DOWN),
        "LEFT" => Some(KeyCode::LEFT),
        "RIGHT" => Some(KeyCode::RIGHT),
        "OK" => Some(KeyCode::OK),
        "LEFT_SOFT_KEY" => Some(KeyCode::LEFT_SOFT_KEY),
        "RIGHT_SOFT_KEY" => Some(KeyCode::RIGHT_SOFT_KEY),
        "CLEAR" => Some(KeyCode::CLEAR),
        "CALL" => Some(KeyCode::CALL),
        "HANGUP" => Some(KeyCode::HANGUP),
        "NUM0" => Some(KeyCode::NUM0),
        "NUM1" => Some(KeyCode::NUM1),
        "NUM2" => Some(KeyCode::NUM2),
        "NUM3" => Some(KeyCode::NUM3),
        "NUM4" => Some(KeyCode::NUM4),
        "NUM5" => Some(KeyCode::NUM5),
        "NUM6" => Some(KeyCode::NUM6),
        "NUM7" => Some(KeyCode::NUM7),
        "NUM8" => Some(KeyCode::NUM8),
        "NUM9" => Some(KeyCode::NUM9),
        "HASH" => Some(KeyCode::HASH),
        "STAR" => Some(KeyCode::STAR),
        "VOLUME_UP" => Some(KeyCode::VOLUME_UP),
        "VOLUME_DOWN" => Some(KeyCode::VOLUME_DOWN),
        _ => None,
    }}
}}

"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(body)


if __name__ == "__main__":
    write_options_rs("wie_libretro/src/options.rs")
    print("wrote wie_libretro/src/options.rs")
