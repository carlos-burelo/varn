use crate::PipelineError;
pub use varn_debug::flags::DebugFlags;
pub use varn_types::capabilities::CapabilitySet;

pub fn parse_debug_opt(spec: Option<&str>) -> Result<DebugFlags, PipelineError> {
    match spec {
        Some(s) => DebugFlags::parse(s).map_err(|e| PipelineError::new(e.exit_code, e.message)),
        None => Ok(DebugFlags::default()),
    }
}

pub struct RunOpts {
    pub file_path: String,
    pub eval: Option<String>,
    pub verbose: bool,
    pub no_run: bool,
    pub debug: DebugFlags,
    pub trace: bool,
    pub strict: bool,
    pub capabilities: CapabilitySet,
}

impl Default for RunOpts {
    fn default() -> Self {
        Self {
            file_path: String::new(),
            eval: None,
            verbose: false,
            no_run: false,
            debug: DebugFlags::default(),
            trace: false,
            strict: false,
            capabilities: CapabilitySet::allow_all(),
        }
    }
}
