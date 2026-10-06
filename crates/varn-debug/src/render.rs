






pub use varn_core::term::colors::*;
pub use varn_core::term::terminal::Section;





pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return "…".to_owned();
    }
    if console::measure_text_width(s) <= width {
        return s.to_owned();
    }
    console::truncate_str(s, width, "…").into_owned()
}


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
        
        assert_eq!(truncate("ññññ", 3), "ññ…");
    }

    #[test]
    fn zero_width_is_just_the_ellipsis() {
        assert_eq!(truncate("abc", 0), "…");
    }
}
