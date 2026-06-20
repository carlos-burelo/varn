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
    /// Ejecutar un archivo Varn
    Run(RunArgs),
    /// Verificar tipos sin ejecutar
    Check(CheckArgs),
    /// Evaluar código directamente desde la línea de comandos
    Eval(EvalArgs),
    /// Iniciar REPL interactivo
    Repl(ReplArgs),
    /// Ejecutar benchmarks de rendimiento
    Bench(BenchArgs),
    /// Inspeccionar AST, tipos, bytecode y otras estructuras internas
    Debug(DebugArgs),
    /// Compilar proyecto
    Build(BuildArgs),
    /// Gestión de paquetes
    #[command(subcommand)]
    Pkg(PkgCommands),
    /// Inicializar nuevo proyecto Varn
    Init(InitArgs),
    /// Diagnóstico del sistema y configuración
    Doctor,
    /// Gestión del caché de compilación
    #[command(subcommand)]
    Cache(CacheCommands),
    /// Iniciar servidor LSP (comunica por stdio)
    Lsp,
    /// Generar scripts de autocompletado para el shell
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct RunArgs {
    /// Archivo Varn a ejecutar
    pub file: String,

    /// Argumentos para el script
    #[arg(last = true, value_name = "ARGS")]
    pub script_args: Vec<String>,

    /// Modo verbose
    #[arg(short, long)]
    pub verbose: bool,

    /// Tracing de ejecución
    #[arg(long)]
    pub trace: bool,

    /// Advertir sobre tipos Dynamic implícitos
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args)]
pub struct DebugArgs {
    /// Archivo Varn a inspeccionar
    pub file: Option<String>,

    /// Evaluar código directamente en lugar de archivo
    #[arg(short, long, value_name = "CODE")]
    pub eval: Option<String>,

    /// Fase a mostrar: tokens, ast, check, bytecode, hir, ssa, symbols, binds, types[:N],
    /// expr, modules, graph, caps, scope, errors, trace, info, lsp[:sub], all
    #[arg(short, long, value_name = "PHASE", default_value = "all")]
    pub phase: String,
}

#[derive(Args)]
pub struct CheckArgs {
    /// Archivo Varn a verificar
    pub file: String,

    /// Modo verbose
    #[arg(short, long)]
    pub verbose: bool,

    /// Advertir sobre tipos Dynamic implícitos
    #[arg(long)]
    pub strict: bool,
}

#[derive(Args)]
pub struct EvalArgs {
    /// Código Varn a evaluar
    pub code: String,

    /// Modo verbose
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct ReplArgs {
    /// Mostrar bytecode generado en cada evaluación
    #[arg(long)]
    pub debug_bytecode: bool,
}

#[derive(Args)]
pub struct BenchArgs {
    /// Archivo Varn a medir
    pub file: String,

    /// Número de runs (default: 10)
    #[arg(long, default_value = "10", value_name = "N")]
    pub runs: usize,

    /// Mostrar output del programa (normalmente silenciado)
    #[arg(long)]
    pub show_output: bool,
}

#[derive(Args)]
pub struct InitArgs {
    /// Directorio del proyecto (default: directorio actual)
    pub dir: Option<String>,

    /// Nombre del proyecto
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell para el que generar completions
    pub shell: Shell,
}

#[derive(Args)]
pub struct BuildArgs {
    /// Archivo Varn a compilar
    pub file: String,

    /// Ruta de salida del binario compilado
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<String>,

    /// Modo verbose
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args)]
pub struct AddArgs {
    /// Alias del paquete
    pub alias: String,
    /// Origen del paquete (URL o ruta)
    pub origin: String,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Alias del paquete a eliminar
    pub alias: String,
}

#[derive(Subcommand)]
pub enum PkgCommands {
    /// Agregar dependencia al proyecto
    Add(AddArgs),
    /// Eliminar dependencia del proyecto
    Remove(RemoveArgs),
    /// Instalar dependencias del proyecto
    Install,
    /// Actualizar dependencias del proyecto
    Update,
}

#[derive(Subcommand)]
pub enum CacheCommands {
    /// Eliminar archivos de caché del proyecto actual
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
