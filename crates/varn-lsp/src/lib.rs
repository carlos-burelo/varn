pub mod backend;
pub mod constants;
pub mod document;
pub mod features;
pub mod index;
pub mod pipeline;
pub mod query;
pub mod util;
pub mod workspace;

#[tokio::main(flavor = "current_thread")]
pub async fn run_server() {
    varn_builtins::register_provider();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = tower_lsp::LspService::new(backend::Backend::new);
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
