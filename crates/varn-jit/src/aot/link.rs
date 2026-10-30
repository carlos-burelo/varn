use std::fmt;
use std::path::Path;
#[cfg(all(windows, target_arch = "x86_64"))]
use std::path::PathBuf;
#[cfg(all(windows, target_arch = "x86_64"))]
use std::process::Command;

#[derive(Debug)]
pub enum LinkError {
    Unsupported(&'static str),
    LinkerMissing,
    Spawn(String),
    Failed(Option<i32>),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Unsupported(what) => write!(f, "{what}"),
            LinkError::LinkerMissing => write!(
                f,
                "no linker found: install the Visual Studio Build Tools or LLVM so that `lld-link` is on PATH"
            ),
            LinkError::Spawn(e) => write!(f, "failed to invoke the linker: {e}"),
            LinkError::Failed(code) => write!(f, "linker exited with error code {code:?}"),
        }
    }
}

impl std::error::Error for LinkError {}

#[cfg(not(all(windows, target_arch = "x86_64")))]
pub fn link_executable(_object: &Path, _out: &Path, _runtime: &Path) -> Result<(), LinkError> {
    Err(LinkError::Unsupported(
        "native linking is implemented for x86_64 Windows (MSVC) only",
    ))
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub fn link_executable(object: &Path, out: &Path, runtime: &Path) -> Result<(), LinkError> {
    let linker = find_linker().ok_or(LinkError::LinkerMissing)?;
    let mut cmd = Command::new(&linker.program);
    cmd.args(&linker.leading_args);
    cmd.arg(format!("/out:{}", out.display()));
    cmd.arg(object);
    cmd.arg(runtime);

    if std::env::var_os("LIB").is_none() {
        for dir in system_library_dirs() {
            cmd.arg(format!("/libpath:{}", dir.display()));
        }
    }

    for lib in [
        "kernel32.lib",
        "ntdll.lib",
        "libcmt.lib",
        "libucrt.lib",
        "libvcruntime.lib",
        "ws2_32.lib",
        "userenv.lib",
        "bcrypt.lib",
    ] {
        cmd.arg(lib);
    }
    cmd.arg("/nologo");
    cmd.arg("/subsystem:console");

    let status = cmd.status().map_err(|e| LinkError::Spawn(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(LinkError::Failed(status.code()))
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
struct Linker {
    program: PathBuf,
    leading_args: Vec<&'static str>,
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn find_linker() -> Option<Linker> {
    let on_path = Command::new("lld-link")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if on_path {
        return Some(Linker {
            program: PathBuf::from("lld-link"),
            leading_args: Vec::new(),
        });
    }

    let output = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let sysroot = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let bin = sysroot
        .join("lib")
        .join("rustlib")
        .join("x86_64-pc-windows-msvc")
        .join("bin");

    let wrapper = bin.join("gcc-ld").join("lld-link.exe");
    if wrapper.exists() {
        return Some(Linker {
            program: wrapper,
            leading_args: Vec::new(),
        });
    }
    let rust_lld = bin.join("rust-lld.exe");
    rust_lld.exists().then_some(Linker {
        program: rust_lld,
        leading_args: vec!["-flavor", "link"],
    })
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn system_library_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(kits) = std::env::var_os("ProgramFiles(x86)")
        .map(|p| PathBuf::from(p).join("Windows Kits").join("10").join("Lib"))
    {
        for version in subdirectories_newest_first(&kits) {
            for part in [["um", "x64"], ["ucrt", "x64"]] {
                let dir = version.join(part[0]).join(part[1]);
                if dir.exists() {
                    dirs.push(dir);
                }
            }
        }
    }

    if let Some(studio) =
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Microsoft Visual Studio"))
    {
        for release in subdirectories_newest_first(&studio) {
            for edition in subdirectories_newest_first(&release) {
                let msvc = edition.join("VC").join("Tools").join("MSVC");
                for version in subdirectories_newest_first(&msvc) {
                    let lib = version.join("lib").join("x64");
                    if lib.exists() {
                        dirs.push(lib);
                    }
                }
            }
        }
    }

    dirs
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn subdirectories_newest_first(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    found.sort();
    found.reverse();
    found
}
