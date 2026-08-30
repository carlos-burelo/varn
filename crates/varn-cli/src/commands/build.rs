use crate::cli::BuildArgs;
use crate::error::CliError;
use crate::pipeline;
use varn_core::term::terminal;

pub fn execute(args: BuildArgs) -> Result<(), CliError> {
    let source = std::fs::read_to_string(&args.file)
        .map_err(|e| CliError::fatal(format!("cannot read '{}': {e}", args.file)))?;

    let compiled =
        pipeline::compile_source_for_build(&source, &args.file, args.verbose, &Default::default())?;

    let is_native = args.target.eq_ignore_ascii_case("native");
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
        let current_exe = std::env::current_exe()
            .map_err(|e| CliError::fatal(format!("cannot locate runner executable: {e}")))?;
        let host_bytes = std::fs::read(&current_exe).map_err(|e| {
            CliError::fatal(format!(
                "cannot read runner executable '{}': {e}",
                current_exe.display()
            ))
        })?;

        let payload = postcard::to_allocvec(&compiled.graph_artifact)
            .map_err(|e| CliError::fatal(format!("cannot serialize artifact: {e}")))?;

        let wrc_envelope = varn_modules::artifact::write_envelope(
            varn_modules::artifact::MAGIC_WRC,
            pipeline::DISTRIBUTABLE_VERSION,
            &payload,
        );

        let mut standalone = Vec::with_capacity(host_bytes.len() + wrc_envelope.len() + 12);
        standalone.extend_from_slice(&host_bytes);
        standalone.extend_from_slice(&wrc_envelope);
        standalone.extend_from_slice(&(wrc_envelope.len() as u64).to_le_bytes());
        standalone.extend_from_slice(varn_modules::artifact::MAGIC_VEXE);

        std::fs::write(&out_path, &standalone).map_err(|e| {
            CliError::fatal(format!(
                "cannot write standalone executable '{out_path}': {e}"
            ))
        })?;
    } else {
        pipeline::wrc::write_wrc(&out_path, &compiled.graph_artifact)?;
    }

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);

    terminal::log(format!(
        "Built '{}' → '{}' ({} KB, target: {})",
        args.file,
        out_path,
        size / 1024,
        if is_native {
            "native standalone"
        } else {
            "bytecode"
        }
    ));
    Ok(())
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
