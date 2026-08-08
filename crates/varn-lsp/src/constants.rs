pub const STDLIB_LINE_MARKER: u32 = u32::MAX;
pub use varn_modules::spec::STD_PREFIX;

pub const SORT_LOCAL: &str = "0_";
pub const SORT_PARAM: &str = "1_";
pub const SORT_GLOBAL: &str = "2_";
pub const SORT_AUTOIMPORT: &str = "z_";

pub const SEVERITY_ERROR: u8 = 1;
pub const SEVERITY_WARNING: u8 = 2;
pub const SEVERITY_HINT: u8 = 3;
