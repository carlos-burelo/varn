use crate::cli::LspArgs;
use crate::error::CliError;

pub fn execute(args: LspArgs) -> Result<(), CliError> {
    if let Some(port) = args.port {
        varn_lsp::run_server_tcp(&format!("127.0.0.1:{port}"));
    } else if let Some(tcp) = args.tcp {
        varn_lsp::run_server_tcp(&tcp);
    } else {
        varn_lsp::run_server();
    }
    Ok(())
}


