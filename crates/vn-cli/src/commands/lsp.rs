use crate::cli::LspArgs;
use crate::error::CliError;

pub fn execute(args: LspArgs) -> Result<(), CliError> {
    if let Some(port) = args.port {
        vn_lsp::run_server_tcp(&format!("127.0.0.1:{port}"));
    } else if let Some(tcp) = args.tcp {
        vn_lsp::run_server_tcp(&tcp);
    } else {
        vn_lsp::run_server();
    }
    Ok(())
}
