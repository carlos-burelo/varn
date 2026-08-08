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

    Test(TestArgs),

    #[command(subcommand)]
    Pkg(PkgCommands),

    Init(InitArgs),

    Doctor,

    #[command(subcommand)]
    Cache(CacheCommands),

    Lsp(LspArgs),

    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct RunArgs {
    pub file: Option<String>,

    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

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

    /// Only dump functions whose name contains NAME.
    #[arg(long = "fn", value_name = "NAME")]
    pub fn_filter: Option<String>,

    /// List every phase `-p` accepts and exit.
    #[arg(long)]
    pub list_phases: bool,
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
    pub file: Option<String>,

    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

    #[arg(long, default_value = "10", value_name = "N")]
    pub runs: usize,

    #[arg(long)]
    pub show_output: bool,

    #[arg(short, long)]
    pub verbose: bool,

    /// Show phase and breakdown rows that measured zero or negligible time.
    #[arg(long)]
    pub all_rows: bool,
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

#[derive(Args, Default)]
pub struct LspArgs {
    #[arg(short, long, help = "Puerto TCP para escuchar en socket (ej: 9257)")]
    pub port: Option<u16>,

    #[arg(long, help = "Dirección host:puerto TCP para escuchar (ej: 127.0.0.1:9257)")]
    pub tcp: Option<String>,

    /// Accepted and ignored: stdio is already the default when no TCP option
    /// is given. LSP clients pass it unprompted — vscode-languageclient
    /// appends `--stdio` for any `TransportKind.stdio` executable, and editor
    /// configs write it by convention — so rejecting it kills the server at
    /// startup with nothing but an EPIPE on the client side.
    #[arg(long, help = "Servir sobre stdio (por defecto; aceptado por convención)")]
    pub stdio: bool,
}

#[derive(Args)]
pub struct BuildArgs {
    pub file: String,

    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,

    #[arg(short, long, default_value = "bytecode", value_name = "TARGET")]
    pub target: String,

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

    Tree,

    Doctor,

    Clean,
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

#[derive(Args)]
pub struct TestArgs {
    /// Target file, directory, or pattern (default: "./tests" if exists, or current directory)
    pub path: Option<String>,

    /// Filter test names or file names matching this pattern
    #[arg(short, long)]
    pub filter: Option<String>,

    /// Run tests in parallel across N isolates/worker threads
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    /// Stop execution on first test failure
    #[arg(long)]
    pub fail_fast: bool,

    /// Show detailed execution log and outputs for every test
    #[arg(short, long)]
    pub verbose: bool,
}
