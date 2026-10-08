use crate::error::CliError;

pub fn execute() -> Result<(), CliError> {
    varn_dap::adapter::run_stdio();
    Ok(())
}
