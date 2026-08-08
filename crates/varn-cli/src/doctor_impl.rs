use crate::error::CliError;

type CliResult<T> = Result<T, CliError>;

pub fn run_doctor() -> CliResult<()> {
    println!("Varn Doctor");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));
    match std::env::current_exe() {
        Ok(exe_path) => println!("  exe: {}", exe_path.display()),
        Err(_) => println!("  exe: <unavailable>"),
    }

    println!("  VARN_HOME: {}", varn_core::paths::varn_home_dir().display());
    println!("  cache dir: {}", varn_core::paths::varn_cache_dir().display());
    match std::env::var(varn_modules::std_root::ENV_VARN_STD) {
        Ok(raw) => println!("  VARN_STD: {raw}"),
        Err(_) => println!("  VARN_STD: <not set>"),
    }

    match varn_modules::provider::get().and_then(|p| p.std_provenance()) {
        Some((desc, prov)) => {
            let origin = match prov {
                varn_modules::std_root::StdProvenance::ProjectOverride => "varn.json override",
                varn_modules::std_root::StdProvenance::Env => "VARN_STD",
                varn_modules::std_root::StdProvenance::DevCheckout => {
                    "dev checkout (std/ next to binary)"
                }
                varn_modules::std_root::StdProvenance::Embedded => "embedded in this binary",
            };
            println!("  std: {desc} (via {origin})");
        }
        None => println!("  std: none resolved"),
    }

    // The only genuinely broken state: a std was found and is unusable. Every
    // other tier falls through to the bundle compiled into this binary, which
    // is fingerprint-matched by construction.
    match varn_builtins::std_load_error() {
        Some(reason) => {
            println!("  status: BROKEN");
            Err(CliError::fatal(reason))
        }
        None => {
            println!("  status: ok");
            Ok(())
        }
    }
}
