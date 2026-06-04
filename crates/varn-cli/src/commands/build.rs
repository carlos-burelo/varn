use crate::cli::BuildArgs;
use crate::error::CliError;
use crate::pipeline;
use varn_utilities::terminal;

pub fn execute(args: BuildArgs) -> Result<(), CliError> {
    let source = std::fs::read_to_string(&args.file)
        .map_err(|e| CliError::fatal(format!("cannot read '{}': {e}", args.file)))?;

    let compiled =
        pipeline::compile_source_for_build(&source, &args.file, args.verbose, &Default::default())?;

    let out_path = resolve_output_path(&args.file, args.output.as_deref());

    pipeline::wrc::write_wrc(&out_path, &compiled.graph_artifact)?;

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);

    terminal::log(format!(
        "Built '{}' → '{}' ({} KB)",
        args.file,
        out_path,
        size / 1024
    ));
    Ok(())
}

fn resolve_output_path(source_file: &str, output: Option<&str>) -> String {
    use std::path::Path;

    let src = Path::new(source_file);
    let stem = src.file_stem().unwrap_or_default().to_string_lossy();

    match output {
        Some(out) => {
            let out_path = Path::new(out);
            if out_path.is_dir() || out.ends_with('/') || out.ends_with('\\') {
                out_path
                    .join(format!("{stem}.vnc"))
                    .to_string_lossy()
                    .into_owned()
            } else {
                out.to_owned()
            }
        }
        None => src.with_extension("vnc").to_string_lossy().into_owned(),
    }
}
