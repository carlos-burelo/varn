use crate::{cli::FmtArgs, error::CliError, formatter};

pub fn execute(args: FmtArgs) -> Result<(), CliError> {
    formatter::run_fmt(args)
}
