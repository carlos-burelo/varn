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
        vn_core::env::EnvVar::Home.name(),
        vn_core::paths::varn_home_dir().display()
    );
    println!(
        "  cache dir: {}",
        vn_core::paths::varn_cache_dir().display()
    );
    match &vn_core::env::process().std {
        Some(raw) => println!("  VARN_STD: {raw}"),
        None => println!("  VARN_STD: <not set>"),
    }

    match vn_modules::provider::get().and_then(|p| p.std_provenance()) {
        Some((desc, prov)) => {
            let origin = match prov {
                vn_modules::std_root::StdProvenance::ProjectOverride => "varn.toml override",
                vn_modules::std_root::StdProvenance::Env => "VARN_STD",
                vn_modules::std_root::StdProvenance::DevCheckout => {
                    "dev checkout (std/ next to binary)"
                }
                vn_modules::std_root::StdProvenance::Embedded => "embedded in this binary",
            };
            println!("  std: {desc} (via {origin})");
        }
        None => println!("  std: none resolved"),
    }

    println!("  environment:");
    for var in vn_core::env::EnvVar::ALL {
        let shown = match vn_core::env::raw(*var) {
            Some(value) => format!("= {value}"),
            None => "(unset)".to_owned(),
        };
        println!("    {:<24} {:<10} {}", var.name(), shown, var.doc());
    }
    for name in vn_core::env::unknown_vars() {
        println!("    {name}: unknown VARN_ variable (typo?)");
    }

    match vn_builtins::std_load_error() {
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
