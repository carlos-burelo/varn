use crate::{cli::CheckArgs, error::CliError, pipeline};

pub fn execute(session: &pipeline::Session, args: CheckArgs) -> Result<(), CliError> {
    crate::exec::load(session, &args.file, &Default::default(), args.verbose).map(|_| ())
}
