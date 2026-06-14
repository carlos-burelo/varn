pub mod backend;
pub mod constants;
pub mod document;
pub mod features;
pub mod index;
pub mod pipeline;
pub mod query;
pub mod util;
pub mod workspace;

fn prepare_stdlib() -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join("varn-stdlib");
    for spec in varn_builtins::MODULE_REGISTRY {
        if let Some(source) = spec.source() {
            let file_path = temp_dir.join(spec.vn_source);
            if let Some(parent) = file_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&file_path, source);
        }
    }
    temp_dir
}

#[tokio::main]
pub async fn run_server() {
    let stdlib_path = prepare_stdlib();
    std::env::set_var("VARN_STDLIB", &stdlib_path);

    varn_builtins::register_provider();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = tower_lsp::LspService::new(backend::Backend::new);
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
