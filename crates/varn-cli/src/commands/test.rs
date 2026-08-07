use crate::{cli::TestArgs, error::CliError, tester};

pub fn execute(args: TestArgs) -> Result<(), CliError> {
    tester::run_tests(args)
}
