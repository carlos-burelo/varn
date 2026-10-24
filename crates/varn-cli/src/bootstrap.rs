use crate::cli::Commands;
use crate::error::CliError;

pub fn init() {
    varn_project::apply_from_cwd();
    crate::std_source::register();
    crate::code_store::register();
    varn_builtins::register_provider();
}

pub fn require_std(command: &Commands) -> Result<(), CliError> {
    if matches!(command, Commands::Lsp(_) | Commands::Doctor) {
        return Ok(());
    }
    match varn_builtins::std_load_error() {
        Some(reason) => Err(CliError::fatal(reason)),
        None => Ok(()),
    }
}
