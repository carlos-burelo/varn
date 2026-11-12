use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JitConfig {
    pub enabled: bool,
    pub trace: bool,
    pub print_ir: bool,
    pub verify: bool,
    pub opt_level: &'static str,
    pub alloc_slow: bool,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self::of(&vn_core::env::Settings::default())
    }
}

impl JitConfig {
    pub fn of(settings: &vn_core::env::Settings) -> Self {
        Self {
            enabled: !settings.no_clif,
            trace: settings.clif_trace,
            print_ir: settings.clif_print_ir,
            verify: settings.clif_verify,
            opt_level: settings.clif_opt.flag_value(),
            alloc_slow: settings.alloc_slow,
        }
    }

    pub fn codegen_fingerprint(&self) -> u64 {
        let text = format!("{};{};{}", self.opt_level, self.verify, self.alloc_slow);
        vn_core::fingerprint::fingerprint(text.as_bytes())
    }
}

static CONFIG: OnceLock<JitConfig> = OnceLock::new();

pub fn configure(config: JitConfig) -> bool {
    CONFIG.set(config).is_ok()
}

pub fn config() -> &'static JitConfig {
    CONFIG.get_or_init(JitConfig::default)
}
