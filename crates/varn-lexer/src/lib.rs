#![allow(clippy::needless_range_loop)]

mod char_class;
mod ffi;
mod keywords;
mod scanner;
mod token_kind;
mod token_record;

pub use ffi::scan;
