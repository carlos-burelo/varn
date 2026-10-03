use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
pub fn run(target: &Path, args: &[OsString]) -> ! {
    use std::os::unix::process::CommandExt;
    let err = Command::new(target).args(args).exec();
    eprintln!("vn-shadow: cannot exec {}: {err}", target.display());
    std::process::exit(127)
}

#[cfg(windows)]
pub fn run(target: &Path, args: &[OsString]) -> ! {
    let image = crate::shadow::materialize(target).unwrap_or_else(|e| {
        eprintln!(
            "vn-shadow: shadow copy failed ({e}); running {} in place",
            target.display()
        );
        target.to_path_buf()
    });
    let status = Command::new(&image).args(args).status();
    match status {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("vn-shadow: cannot run {}: {e}", image.display());
            std::process::exit(127)
        }
    }
}
