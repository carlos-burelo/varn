











#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecSettings {
    
    
    
    
    
    
    
    pub no_jit: bool,
    
    pub trace: bool,
}

impl ExecSettings {
    
    
    
    pub fn from_env(trace: bool) -> Self {
        Self {
            no_jit: std::env::var_os("VARN_NO_JIT").is_some(),
            trace,
        }
    }
}
