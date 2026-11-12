use crate::{cli::TestArgs, error::CliError, tester};

pub fn execute(
    session: std::sync::Arc<vn_pipeline::Session>,
    args: TestArgs,
) -> Result<(), CliError> {
    tester::run_tests(session, args)
}
