mod launch;
#[cfg(windows)]
mod shadow;

use std::ffi::OsString;
use std::path::PathBuf;

const TARGET_ENV: &str = "VARN_SHADOW_TARGET";

fn main() {
    let target = match resolve_target() {
        Ok(path) => path,
        Err(reason) => {
            eprintln!("vn-shadow: {reason}");
            std::process::exit(2);
        }
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    launch::run(&target, &args)
}

fn resolve_target() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os(TARGET_ENV) {
        return Ok(PathBuf::from(explicit));
    }
    let own = std::env::current_exe().map_err(|e| format!("cannot locate own executable: {e}"))?;
    let sibling = own.with_file_name(format!("vn{}", std::env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(format!(
            "{} not found; set {TARGET_ENV} to the vn binary to launch",
            sibling.display()
        ))
    }
}
