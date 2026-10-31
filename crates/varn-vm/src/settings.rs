#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecSettings {
    pub no_jit: bool,

    pub trace: bool,
}

impl ExecSettings {
    pub fn from_env(trace: bool) -> Self {
        Self {
            no_jit: varn_core::env::settings().no_jit,
            trace,
        }
    }
}
