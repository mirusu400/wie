use rust_libretro::types::JoypadState;
use wie_backend::KeyCode;

/// RetroPad button → WIPI/J2ME KeyCode.
/// Order doesn't matter; we iterate the whole table on every poll.
const MAPPING: &[(JoypadState, KeyCode)] = &[
    (JoypadState::UP, KeyCode::UP),
    (JoypadState::DOWN, KeyCode::DOWN),
    (JoypadState::LEFT, KeyCode::LEFT),
    (JoypadState::RIGHT, KeyCode::RIGHT),
    (JoypadState::A, KeyCode::OK),
    (JoypadState::B, KeyCode::CLEAR),
    (JoypadState::X, KeyCode::HASH),
    (JoypadState::Y, KeyCode::NUM0),
    (JoypadState::L, KeyCode::LEFT_SOFT_KEY),
    (JoypadState::R, KeyCode::RIGHT_SOFT_KEY),
    (JoypadState::START, KeyCode::CALL),
    (JoypadState::SELECT, KeyCode::HANGUP),
    (JoypadState::L2, KeyCode::NUM1),
    (JoypadState::R2, KeyCode::NUM3),
    (JoypadState::L3, KeyCode::NUM7),
    (JoypadState::R3, KeyCode::NUM9),
];

pub struct InputTracker {
    prev: JoypadState,
}

impl Default for InputTracker {
    fn default() -> Self {
        Self { prev: JoypadState::empty() }
    }
}

#[derive(Clone, Copy)]
pub enum InputDelta {
    Pressed(KeyCode),
    Released(KeyCode),
}

impl InputTracker {
    pub fn diff(&mut self, curr: JoypadState) -> Vec<InputDelta> {
        let mut out = Vec::new();
        for (button, key) in MAPPING {
            let was = self.prev.contains(*button);
            let is = curr.contains(*button);
            if !was && is {
                out.push(InputDelta::Pressed(*key));
            } else if was && !is {
                out.push(InputDelta::Released(*key));
            }
        }
        self.prev = curr;
        out
    }
}
