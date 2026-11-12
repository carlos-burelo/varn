use super::vars::EnvVar;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JitCacheMode {
    #[default]
    On,
    Off,
    Verify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClifOpt {
    None,
    #[default]
    Speed,
    SpeedAndSize,
}

impl ClifOpt {
    pub const fn flag_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Speed => "speed",
            Self::SpeedAndSize => "speed_and_size",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvError {
    pub var: EnvVar,
    pub value: String,
    pub expected: &'static str,
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}='{}' is not valid: expected {}",
            self.var.name(),
            self.value,
            self.expected
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub std: Option<String>,
    pub std_bundle: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub user_home: Option<PathBuf>,
    pub no_jit: bool,
    pub no_clif: bool,
    pub jit_cache: JitCacheMode,
    pub jit_tier: Option<u32>,
    pub jit_tier_straight: Option<u32>,
    pub jit_osr: Option<u32>,
    pub clif_trace: bool,
    pub clif_verify: bool,
    pub clif_print_ir: bool,
    pub clif_opt: ClifOpt,
    pub alloc_slow: bool,
    pub alloc_profile: u8,
    pub home_trace: bool,
    pub gc_trace: bool,
    pub gc_verify: bool,
    pub gc_stress: Option<usize>,
    pub lsp_debug: bool,
}

const FALSE_WORDS: [&str; 4] = ["0", "false", "no", "off"];

struct Reader<'a> {
    lookup: &'a dyn Fn(&str) -> Option<String>,
    errors: Vec<EnvError>,
}

impl Reader<'_> {
    fn text(&self, var: EnvVar) -> Option<String> {
        (self.lookup)(var.name()).filter(|value| !value.is_empty())
    }

    fn path(&self, var: EnvVar) -> Option<PathBuf> {
        self.text(var).map(PathBuf::from)
    }

    fn flag(&self, var: EnvVar) -> bool {
        self.text(var).is_some_and(|value| {
            let lower = value.to_ascii_lowercase();
            !FALSE_WORDS.contains(&lower.as_str())
        })
    }

    fn number<T: std::str::FromStr>(&mut self, var: EnvVar) -> Option<T> {
        let value = self.text(var)?;
        match value.trim().parse::<T>() {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                self.errors.push(EnvError {
                    var,
                    value,
                    expected: "a non-negative integer",
                });
                None
            }
        }
    }

    fn choice<T: Copy>(&mut self, var: EnvVar, words: &[(&str, T)], default: T) -> T {
        let Some(value) = self.text(var) else {
            return default;
        };
        match words.iter().find(|(word, _)| *word == value) {
            Some((_, chosen)) => *chosen,
            None => {
                self.errors.push(EnvError {
                    var,
                    value,
                    expected: "one of the documented words",
                });
                default
            }
        }
    }
}

fn first_non_empty(lookup: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .find_map(|name| lookup(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

fn windows_home(lookup: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let drive = lookup("HOMEDRIVE").filter(|value| !value.is_empty())?;
    let path = lookup("HOMEPATH").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(drive).join(path))
}

impl Settings {
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Settings, Vec<EnvError>> {
        let (settings, errors) = Self::read(lookup);
        if errors.is_empty() {
            Ok(settings)
        } else {
            Err(errors)
        }
    }

    pub fn read(lookup: &dyn Fn(&str) -> Option<String>) -> (Settings, Vec<EnvError>) {
        let mut r = Reader {
            lookup,
            errors: Vec::new(),
        };
        let settings = Settings {
            std: r.text(EnvVar::Std),
            std_bundle: r.path(EnvVar::StdBundle),
            home: r.path(EnvVar::Home),
            cache_dir: r.path(EnvVar::CacheDir),
            user_home: first_non_empty(lookup, &["HOME", "USERPROFILE"])
                .or_else(|| windows_home(lookup)),
            no_jit: r.flag(EnvVar::NoJit),
            no_clif: r.flag(EnvVar::NoClif),
            jit_cache: r.choice(
                EnvVar::JitCache,
                &[
                    ("on", JitCacheMode::On),
                    ("off", JitCacheMode::Off),
                    ("verify", JitCacheMode::Verify),
                ],
                JitCacheMode::On,
            ),
            jit_tier: r.number(EnvVar::JitTier),
            jit_tier_straight: r.number(EnvVar::JitTierStraight),
            jit_osr: r.number(EnvVar::JitOsr),
            clif_trace: r.flag(EnvVar::ClifTrace),
            clif_verify: r.flag(EnvVar::ClifVerify),
            clif_print_ir: r.flag(EnvVar::ClifPrintIr),
            clif_opt: r.choice(
                EnvVar::ClifOpt,
                &[
                    ("none", ClifOpt::None),
                    ("speed", ClifOpt::Speed),
                    ("speed_and_size", ClifOpt::SpeedAndSize),
                ],
                ClifOpt::Speed,
            ),
            alloc_slow: r.flag(EnvVar::AllocSlow),
            alloc_profile: r.number(EnvVar::AllocProfile).unwrap_or(0),
            home_trace: r.flag(EnvVar::HomeTrace),
            gc_trace: r.flag(EnvVar::GcTrace),
            gc_verify: r.flag(EnvVar::GcVerify),
            gc_stress: r
                .number::<usize>(EnvVar::GcStress)
                .filter(|cells| *cells > 0),
            lsp_debug: r.flag(EnvVar::LspDebug),
        };
        (settings, r.errors)
    }
}
