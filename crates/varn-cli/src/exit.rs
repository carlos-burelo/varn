#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Fatal = 1,
    Usage = 2,
    Diagnostics = 3,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        self as i32
    }
}
