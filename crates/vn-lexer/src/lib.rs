#![allow(clippy::needless_range_loop)]

mod char_class;
mod keywords;
mod scan;
mod scanner;
mod token_kind;
mod token_record;

pub use scan::{scan, scan_with_config, scan_with_trivia};
pub use scanner::LexerConfig;
