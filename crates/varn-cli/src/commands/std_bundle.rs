use crate::cli::StdBundleArgs;
use crate::error::CliError;

pub fn execute(args: StdBundleArgs) -> Result<(), CliError> {
    let std_dir = std::path::PathBuf::from(&args.std_dir);
    let out = std::path::PathBuf::from(&args.out);
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CliError::fatal(format!("cannot create '{}': {e}", parent.display()))
            })?;
        }
    }
    let session = varn_pipeline::resolver::Session::new();
    let bytes = varn_pipeline::stdlib_loader::compile_stdlib_bundle(&std_dir, &session)
        .map_err(CliError::fatal)?;
    std::fs::write(&out, &bytes)
        .map_err(|e| CliError::fatal(format!("cannot write '{}': {e}", out.display())))?;
    eprintln!("wrote {} ({} bytes)", out.display(), bytes.len());
    Ok(())
}
