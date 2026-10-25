use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

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

pub fn with_implicit_run(mut args: Vec<String>) -> Vec<String> {
    let known: Vec<String> = Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_owned())
        .chain(std::iter::once("help".to_owned()))
        .collect();
    if let Some(first) = args.get(1)
        && !known.contains(first)
        && !first.starts_with('-')
    {
        args.insert(1, "run".to_owned());
    }
    args
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

    Task(TaskArgs),

    Fmt(FmtArgs),

    #[command(subcommand)]
    Pkg(PkgCommands),

    Init(InitArgs),

    Doctor,

    #[command(subcommand)]
    Cache(CacheCommands),

    Lsp(LspArgs),

    Completions(CompletionsArgs),

    Dap,

    StdBundle(StdBundleArgs),
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

    #[command(flatten)]
    pub permissions: PermissionArgs,

    #[arg(long = "compare-tiers")]
    pub compare_tiers: bool,
}

#[derive(Args)]
pub struct PermissionArgs {
    #[arg(short = 'A', long = "allow-all")]
    pub allow_all: bool,

    #[arg(long = "allow-read", value_name = "PATHS", num_args = 0..=1, default_missing_value = "*", require_equals = true)]
    pub allow_read: Option<String>,

    #[arg(long = "allow-write", value_name = "PATHS", num_args = 0..=1, default_missing_value = "*", require_equals = true)]
    pub allow_write: Option<String>,

    #[arg(long = "allow-net", value_name = "HOSTS", num_args = 0..=1, default_missing_value = "*", require_equals = true)]
    pub allow_net: Option<String>,

    #[arg(long = "allow-env", value_name = "VARS", num_args = 0..=1, default_missing_value = "*", require_equals = true)]
    pub allow_env: Option<String>,

    #[arg(long = "allow-ffi")]
    pub allow_ffi: bool,

    #[arg(long = "allow-ui")]
    pub allow_ui: bool,

    #[arg(long = "sandbox")]
    pub sandbox: bool,
}

#[derive(Args)]
pub struct DebugArgs {
    #[command(flatten)]
    pub permissions: PermissionArgs,

    pub file: Option<String>,

    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

    #[arg(short, long, value_name = "PHASE", default_value = "all")]
    pub phase: String,

    #[arg(long = "fn", value_name = "NAME")]
    pub fn_filter: Option<String>,

    #[arg(long)]
    pub list_phases: bool,

    #[arg(long)]
    pub natives: bool,
}

#[derive(Args)]
pub struct CheckArgs {
    pub files: Vec<String>,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct EvalArgs {
    #[command(flatten)]
    pub permissions: PermissionArgs,

    pub code: String,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct ReplArgs {
    #[command(flatten)]
    pub permissions: PermissionArgs,

    #[arg(long)]
    pub debug_bytecode: bool,
}

#[derive(Args)]
pub struct BenchArgs {
    #[command(flatten)]
    pub permissions: PermissionArgs,

    pub file: Option<String>,

    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

    #[arg(long, default_value = "10", value_name = "N")]
    pub runs: usize,

    #[arg(long)]
    pub show_output: bool,

    #[arg(short, long)]
    pub verbose: bool,

    #[arg(long)]
    pub all_rows: bool,

    #[arg(long, value_name = "PCT")]
    pub min_clif_coverage: Option<f64>,
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

    #[arg(
        long,
        help = "Dirección host:puerto TCP para escuchar (ej: 127.0.0.1:9257)"
    )]
    pub tcp: Option<String>,

    #[arg(
        long,
        help = "Servir sobre stdio (por defecto; aceptado por convención)"
    )]
    pub stdio: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Bytecode,
    Exe,
    Native,
}

impl Target {
    pub fn extension(self) -> &'static str {
        match self {
            Target::Bytecode => "vnc",
            Target::Exe | Target::Native if cfg!(windows) => "exe",
            Target::Exe | Target::Native => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Bytecode => "bytecode",
            Target::Exe => "standalone executable",
            Target::Native => "native machine code",
        }
    }
}

#[derive(Args)]
pub struct BuildArgs {
    pub file: String,

    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,

    #[arg(short, long, value_enum, default_value_t = Target::Bytecode)]
    pub target: Target,

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
    #[command(flatten)]
    pub permissions: PermissionArgs,

    pub path: Option<String>,

    #[arg(short, long)]
    pub filter: Option<String>,

    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    #[arg(long)]
    pub fail_fast: bool,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct TaskArgs {
    pub name: Option<String>,

    #[arg(last = true, value_name = "ARGS")]
    pub args: Vec<String>,
}

#[derive(Args)]
pub struct FmtArgs {
    pub path: Option<String>,

    #[arg(long)]
    pub check: bool,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct StdBundleArgs {
    #[arg(long, default_value = "std")]
    pub std_dir: String,

    #[arg(long, default_value = "dist/std.vnb")]
    pub out: String,
}
