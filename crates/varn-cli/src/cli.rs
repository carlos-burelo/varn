use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "vn",
    version,
    about = "Lenguaje compilado con tipado estático",
    arg_required_else_help = true,
    disable_help_subcommand = false
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Run(RunArgs),

    Check(CheckArgs),

    Eval(EvalArgs),

    Repl(ReplArgs),

    Bench(BenchArgs),

    Debug(DebugArgs),

    Build(BuildArgs),

    #[command(subcommand)]
    Pkg(PkgCommands),

    Init(InitArgs),

    Doctor,

    #[command(subcommand)]
    Cache(CacheCommands),

    Lsp,

    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct RunArgs {
    pub file: String,

    #[arg(last = true, value_name = "ARGS")]
    pub script_args: Vec<String>,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub trace: bool,

    #[arg(long)]
    pub strict: bool,
}

#[derive(Args)]
pub struct DebugArgs {
    pub file: Option<String>,

    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

    #[arg(short, long, value_name = "PHASE", default_value = "all")]
    pub phase: String,
}

#[derive(Args)]
pub struct CheckArgs {
    pub file: String,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub strict: bool,
}

#[derive(Args)]
pub struct EvalArgs {
    pub code: String,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct ReplArgs {
    #[arg(long)]
    pub debug_bytecode: bool,
}

#[derive(Args)]
pub struct BenchArgs {
    pub file: String,

    #[arg(long, default_value = "10", value_name = "N")]
    pub runs: usize,

    #[arg(long)]
    pub show_output: bool,
}

#[derive(Args)]
pub struct InitArgs {
    pub dir: Option<String>,

    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct CompletionsArgs {
    pub shell: Shell,
}

#[derive(Args)]
pub struct BuildArgs {
    pub file: String,

    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct AddArgs {
    pub alias: String,

    pub origin: String,
}

#[derive(Args)]
pub struct RemoveArgs {
    pub alias: String,
}

#[derive(Subcommand)]
pub enum PkgCommands {
    Add(AddArgs),

    Remove(RemoveArgs),

    Install,

    Update,
}

#[derive(Subcommand)]
pub enum CacheCommands {
    Clean,
}

#[derive(ValueEnum, Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Elvish,
}
