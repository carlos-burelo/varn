use crate::cli::Commands;
use crate::error::CliError;

pub fn init() {
    varn_project::apply_from_cwd();
    crate::std_source::register();
    crate::code_store::register();
    varn_builtins::register_provider();
}

pub fn init_standalone(std_image: Vec<u8>) {
    let bytes: &'static [u8] = Box::leak(std_image.into_boxed_slice());
    varn_builtins::register_std(varn_builtins::StdInput::Image {
        bytes,
        tree_root: None,
        provenance: varn_modules::std_root::StdProvenance::Embedded,
    });
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
