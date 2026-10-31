use crate::error::CliError;

type CliResult<T> = Result<T, CliError>;

pub fn run_doctor() -> CliResult<()> {
    println!("Varn Doctor");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));
    match std::env::current_exe() {
        Ok(exe_path) => println!("  exe: {}", exe_path.display()),
        Err(_) => println!("  exe: <unavailable>"),
    }

    println!(
        "  {}: {}",
        varn_core::env::EnvVar::Home.name(),
        varn_core::paths::varn_home_dir().display()
    );
    println!(
        "  cache dir: {}",
        varn_core::paths::varn_cache_dir().display()
    );
    match &varn_core::env::settings().std {
        Some(raw) => println!("  VARN_STD: {raw}"),
        None => println!("  VARN_STD: <not set>"),
    }

    match varn_modules::provider::get().and_then(|p| p.std_provenance()) {
        Some((desc, prov)) => {
            let origin = match prov {
                varn_modules::std_root::StdProvenance::ProjectOverride => "varn.toml override",
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

    println!("  environment:");
    for var in varn_core::env::EnvVar::ALL {
        let shown = match varn_core::env::raw(*var) {
            Some(value) => format!("= {value}"),
            None => "(unset)".to_owned(),
        };
        println!("    {:<24} {:<10} {}", var.name(), shown, var.doc());
    }
    for name in varn_core::env::unknown_vars() {
        println!("    {name}: unknown VARN_ variable (typo?)");
    }

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
