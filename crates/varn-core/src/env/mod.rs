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

static LOADED: OnceLock<(Settings, Vec<EnvError>)> = OnceLock::new();

fn loaded() -> &'static (Settings, Vec<EnvError>) {
    LOADED.get_or_init(|| Settings::read(&process_lookup))
}

pub fn install(settings: Settings) -> Result<(), Settings> {
    LOADED
        .set((settings, Vec::new()))
        .map_err(|(rejected, _)| rejected)
}

pub fn settings() -> &'static Settings {
    &loaded().0
}

pub fn process_errors() -> &'static [EnvError] {
    &loaded().1
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

pub fn report_errors_and_exit_if_any() {
    let errors = process_errors();
    if errors.is_empty() {
        return;
    }
    for error in errors {
        eprintln!("error: {error}");
    }
    std::process::exit(2);
}
