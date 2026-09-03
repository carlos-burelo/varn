use crate::cli::BuildArgs;
use crate::error::CliError;
use crate::pipeline;
use varn_core::term::terminal;

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn execute(args: BuildArgs) -> Result<(), CliError> {
    let source = std::fs::read_to_string(&args.file)
        .map_err(|e| CliError::fatal(format!("cannot read '{}': {e}", args.file)))?;

    let compiled =
        pipeline::compile_source_for_build(&source, &args.file, args.verbose, &Default::default())?;

    let is_native = args.native || args.target.eq_ignore_ascii_case("native");
    let ext = if is_native {
        if cfg!(windows) {
            "exe"
        } else {
            ""
        }
    } else {
        "vnc"
    };

    let out_path = resolve_output_path(&args.file, args.output.as_deref(), ext);

    if is_native {
        if args.verbose {
            terminal::tagged("AOT", "compiling to machine code object file...");
        }

        let isa = varn_jit::clif::host_isa()
            .map_err(|e| CliError::fatal(format!("failed to configure target ISA: {e}")))?;

        let aot_output = varn_jit::aot::compile_to_object(&compiled.entry_proto, &isa)
            .map_err(|e| CliError::fatal(format!("AOT compilation failed: {e}")))?;

        let obj_path = Path::new(&out_path).with_extension("obj");
        std::fs::write(&obj_path, &aot_output.object_bytes)
            .map_err(|e| CliError::fatal(format!("cannot write object file '{}': {e}", obj_path.display())))?;

        if args.verbose {
            terminal::tagged("AOT", format!("linking object file to '{}'...", out_path));
        }

        link_native_executable(&obj_path, Path::new(&out_path))?;

        // Clean up intermediate .obj file
        let _ = std::fs::remove_file(&obj_path);
    } else {
        pipeline::portable::write_portable(&out_path, &compiled.graph_artifact)?;
    }

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);

    terminal::log(format!(
        "Built '{}' → '{}' ({} KB, target: {})",
        args.file,
        out_path,
        size / 1024,
        if is_native {
            "native machine code"
        } else {
            "bytecode"
        }
    ));
    Ok(())
}

fn link_native_executable(obj_path: &Path, out_exe: &Path) -> Result<(), CliError> {
    let lld_link = find_lld_link();

    let mut cmd = if let Some(lld) = lld_link {
        Command::new(lld)
    } else {
        Command::new("lld-link")
    };

    let rt_lib_path = find_varn_rt_lib()?;

    cmd.arg(format!("/out:{}", out_exe.display()));
    cmd.arg(obj_path.as_os_str());
    cmd.arg(rt_lib_path.as_os_str());

    add_system_lib_paths(&mut cmd);

    cmd.arg("kernel32.lib");
    cmd.arg("ntdll.lib");
    cmd.arg("libcmt.lib");
    cmd.arg("libucrt.lib");
    cmd.arg("libvcruntime.lib");
    cmd.arg("ws2_32.lib");
    cmd.arg("userenv.lib");
    cmd.arg("bcrypt.lib");
    cmd.arg("/nologo");
    cmd.arg("/subsystem:console");

    let status = cmd.status().map_err(|e| {
        CliError::fatal(format!(
            "failed to invoke linker (lld-link). Ensure Visual Studio Build Tools or LLVM is installed: {e}"
        ))
    })?;

    if !status.success() {
        return Err(CliError::fatal(format!(
            "linker exited with error code {:?}",
            status.code()
        )));
    }

    Ok(())
}

fn find_lld_link() -> Option<PathBuf> {
    if let Ok(output) = Command::new("lld-link").arg("--version").output() {
        if output.status.success() {
            return Some(PathBuf::from("lld-link"));
        }
    }

    if let Ok(output) = Command::new("rustc").arg("--print").arg("sysroot").output() {
        if output.status.success() {
            let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let p1 = Path::new(&sysroot)
                .join("lib")
                .join("rustlib")
                .join("x86_64-pc-windows-msvc")
                .join("bin")
                .join("gcc-ld")
                .join("lld-link.exe");
            if p1.exists() {
                return Some(p1);
            }
            let p2 = Path::new(&sysroot)
                .join("lib")
                .join("rustlib")
                .join("x86_64-pc-windows-msvc")
                .join("bin")
                .join("rust-lld.exe");
            if p2.exists() {
                return Some(p2);
            }
        }
    }

    None
}

fn find_varn_rt_lib() -> Result<PathBuf, CliError> {
    let candidates = [
        PathBuf::from("target/debug/varn_rt.lib"),
        PathBuf::from("target/release/varn_rt.lib"),
        PathBuf::from("../target/debug/varn_rt.lib"),
        PathBuf::from("../target/release/varn_rt.lib"),
    ];

    for c in &candidates {
        if c.exists() {
            return Ok(c.canonicalize().unwrap_or_else(|_| c.clone()));
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let lib_candidate = dir.join("varn_rt.lib");
            if lib_candidate.exists() {
                return Ok(lib_candidate);
            }
        }
    }

    Err(CliError::fatal(
        "cannot find 'varn_rt.lib'. Run 'cargo build -p varn-rt' first.",
    ))
}

fn add_system_lib_paths(cmd: &mut Command) {
    let sdk_roots = [
        r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0",
        r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.22621.0",
        r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.22000.0",
        r"C:\Program Files (x86)\Windows Kits\10\Lib\10.0.19041.0",
    ];

    for root in &sdk_roots {
        let um = Path::new(root).join("um").join("x64");
        let ucrt = Path::new(root).join("ucrt").join("x64");
        if um.exists() {
            cmd.arg(format!("/libpath:{}", um.display()));
        }
        if ucrt.exists() {
            cmd.arg(format!("/libpath:{}", ucrt.display()));
        }
    }

    let vs_base = Path::new(r"C:\Program Files\Microsoft Visual Studio");
    if vs_base.exists() {
        if let Ok(entries) = std::fs::read_dir(vs_base) {
            for entry in entries.flatten() {
                let p = entry.path();
                for edition in &["Insiders", "Community", "Professional", "Enterprise", "BuildTools"] {
                    let msvc_dir = p.join(edition).join("VC").join("Tools").join("MSVC");
                    if msvc_dir.exists() {
                        if let Ok(versions) = std::fs::read_dir(&msvc_dir) {
                            for ver in versions.flatten() {
                                let x64_lib = ver.path().join("lib").join("x64");
                                if x64_lib.exists() {
                                    cmd.arg(format!("/libpath:{}", x64_lib.display()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn resolve_output_path(source_file: &str, output: Option<&str>, default_ext: &str) -> String {
    use std::path::Path;

    let src = Path::new(source_file);
    let stem = src.file_stem().unwrap_or_default().to_string_lossy();

    match output {
        Some(out) => {
            let out_path = Path::new(out);
            if out_path.is_dir() || out.ends_with('/') || out.ends_with('\\') {
                let filename = if default_ext.is_empty() {
                    stem.to_string()
                } else {
                    format!("{stem}.{default_ext}")
                };
                out_path.join(filename).to_string_lossy().into_owned()
            } else {
                out.to_owned()
            }
        }
        None => {
            if default_ext.is_empty() {
                stem.to_string()
            } else {
                src.with_extension(default_ext)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }
}
