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
