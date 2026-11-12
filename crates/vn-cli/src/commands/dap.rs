use crate::error::CliError;

pub fn execute() -> Result<(), CliError> {
    vn_dap::adapter::run_stdio();
    Ok(())
}
