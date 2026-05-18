use crate::error::CliError;

pub fn execute() -> Result<(), CliError> {
    crate::doctor_impl::run_doctor()
}
