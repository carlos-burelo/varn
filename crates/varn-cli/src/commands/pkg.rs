use crate::{cli::PkgCommands, error::CliError};

pub fn execute(cmd: PkgCommands) -> Result<(), CliError> {
    match cmd {
        PkgCommands::Add(args) => super::add::execute(args),
        PkgCommands::Remove(args) => super::remove::execute(args),
        PkgCommands::Install => super::install::execute(),
        PkgCommands::Update => super::update::execute(),
    }
}
