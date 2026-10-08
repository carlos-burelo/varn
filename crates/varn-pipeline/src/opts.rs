pub use varn_core::debug_flags::DebugFlags;
pub use varn_types::capabilities::CapabilitySet;

pub struct RunOpts {
    pub file_path: String,
    pub eval: Option<String>,
    pub append: Option<String>,
    pub verbose: bool,
    pub no_run: bool,
    pub debug: DebugFlags,
    pub trace: bool,
    pub capabilities: CapabilitySet,
}

impl Default for RunOpts {
    fn default() -> Self {
        Self {
            file_path: String::new(),
            eval: None,
            append: None,
            verbose: false,
            no_run: false,
            debug: DebugFlags::default(),
            trace: false,
            capabilities: CapabilitySet::allow_all(),
        }
    }
}
