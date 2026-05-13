use rust_libretro::{contexts::OptionsChangedContext, types::JoypadState};
use wie_backend::KeyCode;

use crate::options;

pub struct InputTracker {
    prev: JoypadState,
    mapping: Vec<(JoypadState, KeyCode)>,
}

impl Default for InputTracker {
    fn default() -> Self {
        Self {
            prev: JoypadState::empty(),
            mapping: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub enum InputDelta {
    Pressed(KeyCode),
    Released(KeyCode),
}

impl InputTracker {
    /// Rebuild the RetroPad → KeyCode table from the frontend's current
    /// core-option values. Called once at init and again every time the
    /// user touches the Options menu.
    pub fn refresh_mapping(&mut self, ctx: &OptionsChangedContext) {
        self.mapping = options::resolve_mapping(ctx);
    }

    pub fn active_slot_count(&self) -> usize {
        self.mapping.len()
    }

    pub fn diff(&mut self, curr: JoypadState) -> Vec<InputDelta> {
        let mut out = Vec::new();
        for (button, key) in &self.mapping {
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
