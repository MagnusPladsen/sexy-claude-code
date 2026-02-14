#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyModifiers};

pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn matches(&self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        self.code == code && self.modifiers == modifiers
    }
}

pub fn quit_binding() -> KeyBinding {
    KeyBinding {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::CONTROL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quit_binding() {
        let binding = quit_binding();
        assert!(binding.matches(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(!binding.matches(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!binding.matches(KeyCode::Char('a'), KeyModifiers::CONTROL));
    }
}
