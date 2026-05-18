use crate::{
    cli::{Cli, CompletionsArgs, Shell},
    error::CliError,
};
use clap::CommandFactory;
use clap_complete::Shell as ClapShell;
use std::io;

pub fn execute(args: CompletionsArgs) -> Result<(), CliError> {
    let mut cmd = Cli::command();
    let shell = match args.shell {
        Shell::Bash => ClapShell::Bash,
        Shell::Zsh => ClapShell::Zsh,
        Shell::Fish => ClapShell::Fish,
        Shell::PowerShell => ClapShell::PowerShell,
        Shell::Elvish => ClapShell::Elvish,
    };
    clap_complete::generate(shell, &mut cmd, "Varn", &mut io::stdout());
    Ok(())
}
