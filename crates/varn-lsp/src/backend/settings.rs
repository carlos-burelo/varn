







use std::sync::atomic::{AtomicBool, Ordering};



pub struct Settings {
    inlay_hints: AtomicBool,
}

impl Settings {
    pub fn new() -> Self {
        Self {
            inlay_hints: AtomicBool::new(true),
        }
    }

    pub fn inlay_hints_enabled(&self) -> bool {
        self.inlay_hints.load(Ordering::Relaxed)
    }

    
    pub fn apply(&self, value: &serde_json::Value) {
        if let Some(enabled) = Self::inlay_hints_enabled_in(value) {
            self.inlay_hints.store(enabled, Ordering::Relaxed);
        }
    }

    
    
    
    
    
    
    pub fn inlay_hints_enabled_in(value: &serde_json::Value) -> Option<bool> {
        value
            .pointer("/Varn/inlayHints/enabled")
            .or_else(|| value.pointer("/inlayHints/enabled"))
            .and_then(serde_json::Value::as_bool)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}
