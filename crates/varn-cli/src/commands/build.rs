use crate::cli::{BuildArgs, Target};
use crate::error::CliError;
use crate::pipeline;
use varn_core::term::terminal;

use std::path::{Path, PathBuf};

const RUNTIME_LIB: &str = "varn_rt.lib";

pub fn execute(session: &pipeline::Session, args: BuildArgs) -> Result<(), CliError> {
    let source = pipeline::Source::file(&args.file)?;

    let compiled = crate::exec::compile(
        session,
        source,
        pipeline::CachePolicy::Bypass,
        &Default::default(),
        args.verbose,
    )?;

    let out_path = resolve_output_path(&args.file, args.output.as_deref(), args.target.extension());

    match args.target {
        Target::Bytecode => pipeline::portable::write_portable(&out_path, &compiled.graph_artifact)?,
        Target::Native => build_native(&compiled, &out_path, args.verbose)?,
    }

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);

    terminal::log(format!(
        "Built '{}' → '{}' ({} KB, target: {})",
        args.file,
        out_path,
        size / 1024,
        args.target.label()
    ));
    Ok(())
}

fn build_native(
    compiled: &pipeline::Executable,
    out_path: &str,
    verbose: bool,
) -> Result<(), CliError> {
    if verbose {
        terminal::tagged("AOT", "compiling to machine code object file...");
    }

    let isa = varn_jit::clif::host_isa()
        .map_err(|e| CliError::fatal(format!("failed to configure target ISA: {e}")))?;

    let aot_output = varn_jit::aot::compile_to_object(&compiled.entry_proto, &isa)
        .map_err(|e| CliError::fatal(format!("AOT compilation failed: {e}")))?;

    let obj_path = Path::new(out_path).with_extension("obj");
    std::fs::write(&obj_path, &aot_output.object_bytes).map_err(|e| {
        CliError::fatal(format!(
            "cannot write object file '{}': {e}",
            obj_path.display()
        ))
    })?;

    if verbose {
        terminal::tagged("AOT", format!("linking object file to '{out_path}'..."));
    }

    let linked = varn_jit::aot::link::link_executable(
        &obj_path,
        Path::new(out_path),
        &runtime_lib()?,
    );
    let _ = std::fs::remove_file(&obj_path);
    linked.map_err(|e| CliError::fatal(e.to_string()))
}

fn runtime_lib() -> Result<PathBuf, CliError> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(RUNTIME_LIB)))
        .filter(|path| path.exists())
        .ok_or_else(|| {
            CliError::fatal(format!(
                "cannot find '{RUNTIME_LIB}' next to vn. Run 'cargo build -p varn-rt' first."
            ))
        })
}

fn resolve_output_path(source_file: &str, output: Option<&str>, default_ext: &str) -> String {
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
