use crate::error::CliError;

type CliResult<T> = Result<T, CliError>;

pub fn run_doctor() -> CliResult<()> {
    let exe = std::env::current_exe().ok();
    let home = varn_core::paths::varn_home_dir();
    let cache = varn_core::paths::varn_cache_dir();
    let loader = varn_builtins::ModuleLoader::from_env();
    let stdlib = loader.stdlib_root();

    println!("Varn Doctor");
    println!("  version: {}", env!("CARGO_PKG_VERSION"));
    if let Some(exe_path) = exe {
        println!("  exe: {}", exe_path.display());
    } else {
        println!("  exe: <unavailable>");
    }

    println!("  varn_HOME: {}", home.display());
    println!("  cache dir: {}", cache.display());

    if let Ok(raw) = std::env::var("varn_STDLIB") {
        println!("  varn_STDLIB env: {raw}");
    } else {
        println!("  varn_STDLIB env: <not set>");
    }

    if stdlib.exists() {
        println!("  stdlib: {} (ok)", stdlib.display());
        Ok(())
    } else {
        println!("  stdlib: {} (NOT FOUND)", stdlib.display());
        Err(CliError::fatal(
            "stdlib not found. Run installer script or set varn_STDLIB to a valid directory.",
        ))
    }
}
