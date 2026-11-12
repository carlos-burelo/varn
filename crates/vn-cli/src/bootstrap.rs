use crate::cli::Commands;
use crate::error::CliError;

pub fn init(project: &vn_project::Project) {
    vn_project::apply_env(project);
    vn_modules::std_root::pin_project(project);
    crate::std_source::register();
    crate::code_store::register();
    vn_builtins::register_provider();
}

pub fn init_standalone(std_image: Vec<u8>) {
    let bytes: &'static [u8] = Box::leak(std_image.into_boxed_slice());
    vn_builtins::register_std(vn_builtins::StdInput::Image {
        bytes,
        tree_root: None,
        provenance: vn_modules::std_root::StdProvenance::Embedded,
    });
    crate::code_store::register();
    vn_builtins::register_provider();
}

pub fn require_std(command: &Commands) -> Result<(), CliError> {
    if matches!(command, Commands::Lsp(_) | Commands::Doctor) {
        return Ok(());
    }
    match vn_builtins::std_load_error() {
        Some(reason) => Err(CliError::fatal(reason)),
        None => Ok(()),
    }
}
