pub mod backend;
pub mod constants;
pub mod db;
pub mod document;
pub mod features;
pub mod index;
pub mod pipeline;
pub mod query;
pub mod queries;
pub mod util;
pub mod workspace;

fn prepare_stdlib() -> std::path::PathBuf {
    let temp_dir = std::env::temp_dir().join("varn-stdlib");
    let _ = std::fs::create_dir_all(&temp_dir);

    if let Some((src, _)) = varn_modules::std_root::resolve() {
        match src {
            varn_modules::std_root::StdSource::SourceTree(dir) => {
                let _ = copy_dir_all(&dir, &temp_dir);
                return temp_dir;
            }
            varn_modules::std_root::StdSource::Bundle(bundle_path) => {
                if let Ok(bytes) = std::fs::read(&bundle_path) {
                    if let Ok(bundle) = varn_modules::bundle::read_bundle(&bytes) {
                        for module in bundle.modules {
                            let rel = format!("{}.vn", module.id.replace(':', "/"));
                            let file_path = temp_dir.join(&rel);
                            if let Some(parent) = file_path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(&file_path, format!("// module {}\n", module.id));
                        }
                        return temp_dir;
                    }
                }
            }
        }
    }

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

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[tokio::main]
pub async fn run_server() {
    let stdlib_path = prepare_stdlib();
    std::env::set_var("VARN_STDLIB", &stdlib_path);

    varn_builtins::register_provider();

    let std_error = varn_builtins::std_load_error();

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
    let stdlib_path = prepare_stdlib();
    std::env::set_var("VARN_STDLIB", &stdlib_path);

    varn_builtins::register_provider();
    let std_error = varn_builtins::std_load_error();

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
