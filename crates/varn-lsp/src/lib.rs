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
// Single-thread runtime: Rc<str> in checker types never cross thread boundaries.
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = tower_lsp::LspService::new(backend::Backend::new);
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
