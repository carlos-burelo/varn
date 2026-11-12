mod settings;
mod vars;

pub use settings::{ClifOpt, EnvError, JitCacheMode, Settings};
pub use vars::{EnvVar, Scope};

use std::sync::OnceLock;

const VARN_PREFIX: &str = "VARN_";

#[allow(clippy::disallowed_methods)]
fn process_lookup(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub fn raw(var: EnvVar) -> Option<String> {
    process_lookup(var.name())
}

static PROCESS: OnceLock<Settings> = OnceLock::new();

pub fn from_process() -> (Settings, Vec<EnvError>) {
    Settings::read(&process_lookup)
}

pub fn install(settings: Settings) -> &'static Settings {
    PROCESS.get_or_init(|| settings)
}

pub fn install_from_process_or_exit() -> &'static Settings {
    let (settings, errors) = from_process();
    if !errors.is_empty() {
        for error in &errors {
            eprintln!("error: {error}");
        }
        std::process::exit(2);
    }
    install(settings)
}

pub fn process() -> &'static Settings {
    PROCESS.get_or_init(|| from_process().0)
}

#[allow(clippy::disallowed_methods)]
pub fn unknown_vars() -> Vec<String> {
    let mut unknown: Vec<String> = std::env::vars()
        .map(|(name, _)| name)
        .filter(|name| name.starts_with(VARN_PREFIX) && EnvVar::from_name(name).is_none())
        .collect();
    unknown.sort();
    unknown
}
