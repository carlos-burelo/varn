//! Single ANSI palette and shared render helpers (DEBUG_PLAN §3.4/§7).
//!
//! The palette already has one home in `varn_core::term::colors`; several
//! `varn-debug` phases re-declared the same bytes locally. This module is the
//! one place phases import from, so `Plain` output keeps one source of truth
//! and `Text` can render colorless without touching a phase's data.

pub use varn_core::term::colors::*;
pub use varn_core::term::terminal::Section;

/// Truncate `s` to at most `width` terminal columns, appending `…` when it
/// does not fit. Va por `console::truncate_str`: entiende ANSI y cuenta
/// ancho visible (CJK/emoji = 2), así que alinear tablas con contenido no
/// ASCII ya no las descoloca. Semántica histórica preservada para ASCII.
pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return "…".to_owned();
    }
    if console::measure_text_width(s) <= width {
        return s.to_owned();
    }
    console::truncate_str(s, width, "…").into_owned()
}

/// Final path component (handles both `/` and `\`), for `Text` headers.
pub fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn short_string_is_unchanged() {
        assert_eq!(truncate("abc", 8), "abc");
        assert_eq!(truncate("abcd", 4), "abcd");
    }

    #[test]
    fn long_string_is_widened_to_ellipsis() {
        assert_eq!(truncate("abcdefgh", 5), "abcd…");
    }

    #[test]
    fn counts_chars_not_bytes() {
        // "ñ" is two bytes but one char.
        assert_eq!(truncate("ññññ", 3), "ññ…");
    }

    #[test]
    fn zero_width_is_just_the_ellipsis() {
        assert_eq!(truncate("abc", 0), "…");
    }
}
