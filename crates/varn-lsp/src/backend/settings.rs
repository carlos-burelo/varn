//! Client settings the server honours.
//!
//! `Varn.inlayHints.enabled` shipped in the extension manifest — with a
//! description and a default — and nothing read it, because there was no
//! configuration handler at all. A setting that silently does nothing is worse
//! than an absent one: the user concludes the feature is broken rather than the
//! switch.

use std::sync::atomic::{AtomicBool, Ordering};

/// The settings, held atomically because request handlers read them from
/// tokio's pool while a configuration notification writes them.
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

    /// Take from `value` whatever it states, and leave the rest alone.
    pub fn apply(&self, value: &serde_json::Value) {
        if let Some(enabled) = Self::inlay_hints_enabled_in(value) {
            self.inlay_hints.store(enabled, Ordering::Relaxed);
        }
    }

    /// `Varn.inlayHints.enabled` as a payload states it, if it does.
    ///
    /// Two shapes because clients send two: `didChangeConfiguration` nests the
    /// section under its name, `initializationOptions` often does not. `None`
    /// means the payload says nothing about the setting — which must leave the
    /// current value as it is, not reset it to a default.
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
