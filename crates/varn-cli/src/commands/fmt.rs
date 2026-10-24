use crate::{cli::FmtArgs, error::CliError, formatter, pipeline};

pub fn execute(session: &pipeline::Session, args: FmtArgs) -> Result<(), CliError> {
    formatter::run_fmt(session.project(), args)
}
