pub mod analysis;
pub mod backend;
pub mod constants;
pub mod db;
pub mod document;
pub mod features;
pub mod index;
pub mod pipeline;
pub mod query;
pub mod util;
pub mod workspace;

/// Provider first, then the source mirror it feeds: `materialize` reads the
/// active std through the provider, so registering has to come first.
fn init_std() -> Option<&'static str> {
    varn_builtins::register_provider();
    workspace::std_sources::materialize();
    varn_builtins::std_load_error()
}

#[tokio::main]
pub async fn run_server() {
    let std_error = init_std();

    eprintln!("Varn LSP server listening on stdio (JSON-RPC). Connect your editor or press Ctrl+C to exit.");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) =
        tower_lsp::LspService::new(|client| backend::Backend::new(client, std_error));
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}

#[tokio::main]
pub async fn run_server_tcp(addr: &str) {
    let std_error = init_std();

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind TCP listener on {addr}: {e}");
            return;
        }
    };

    eprintln!("Varn LSP server listening on TCP {addr}. Press Ctrl+C to exit.");

    while let Ok((stream, client_addr)) = listener.accept().await {
        eprintln!("LSP client connected from {client_addr}");
        let (read, write) = stream.into_split();
        let (service, socket) =
            tower_lsp::LspService::new(|client| backend::Backend::new(client, std_error));
        tower_lsp::Server::new(read, write, socket)
            .serve(service)
            .await;
        eprintln!("LSP client disconnected: {client_addr}");
    }
}
