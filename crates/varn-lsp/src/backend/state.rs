use std::time::Instant;

use tower_lsp::lsp_types::*;
use tower_lsp::Client;

use crate::analysis::{AnalysisHandle, Analyzer};

pub(crate) const SLOW_REQUEST_MS: u128 = 30;

pub struct Backend {
    pub client: Client,
    /// The analysis thread. `Backend` holds no analysis state of its own —
    /// none of it is `Send`, so it cannot live next to the request handlers.
    pub(crate) analysis: AnalysisHandle,
    /// Why the active std is unusable, if it is. Reported once on
    /// `initialized`; until it is fixed, `std:` imports resolve to nothing.
    pub(crate) std_error: Option<&'static str>,
    /// Client settings the server honours, refreshed on
    /// `workspace/didChangeConfiguration`.
    pub(crate) settings: super::settings::Settings,
    /// Whether the client can render `$/progress`, learned at the handshake.
    pub(crate) progress_supported: std::sync::atomic::AtomicBool,
    /// Whether the client answers `workspace/configuration`, learned likewise.
    pub(crate) configuration_supported: std::sync::atomic::AtomicBool,
}

impl Backend {
    pub fn new(client: Client, std_error: Option<&'static str>) -> Self {
        Self {
            client,
            analysis: AnalysisHandle::spawn(),
            std_error,
            settings: super::settings::Settings::new(),
            progress_supported: std::sync::atomic::AtomicBool::new(false),
            configuration_supported: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Ask the client for the `Varn` configuration section.
    ///
    /// A push notification is not enough on its own: `vscode-languageclient`
    /// sends `didChangeConfiguration` with `settings: null` and expects the
    /// server to pull what it needs. Handling only the push means the setting
    /// changes in the editor and never reaches here — which is the failure this
    /// whole path exists to fix, one step further along.
    ///
    /// The reply for a named section is that section's contents, so
    /// `{ "inlayHints": { "enabled": false } }` is what arrives.
    pub(crate) async fn pull_configuration(&self) {
        if !self
            .configuration_supported
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let items = vec![ConfigurationItem {
            scope_uri: None,
            section: Some("Varn".to_owned()),
        }];
        if let Ok(values) = self.client.configuration(items).await {
            if let Some(value) = values.first() {
                self.settings.apply(value);
            }
        }
    }

    /// Run a query on the analysis thread and time it.
    ///
    /// Goes on `AnalysisHandle`'s foreground queue (`run`, not
    /// `run_background`) — every interactive request takes priority over the
    /// initial workspace index; see [`crate::analysis`]. The `Send` bound on
    /// `T` is what keeps state that must stay on the analysis thread from
    /// leaving it.
    pub(crate) async fn query<T, F>(&self, op: &str, f: F) -> Option<T>
    where
        F: FnOnce(&mut Analyzer) -> Option<T> + Send + 'static,
        T: Send + 'static,
    {
        let start = Instant::now();
        let result = self.analysis.run(f).await.flatten();
        self.log_slow(op, start.elapsed()).await;
        result
    }

    /// Surfaces slow LSP operations in the client's output channel as they
    /// happen, instead of only being visible via external profiling.
    pub(crate) async fn log_slow(&self, op: &str, elapsed: std::time::Duration) {
        if elapsed.as_millis() >= SLOW_REQUEST_MS {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!("[perf] {op} took {}ms", elapsed.as_millis()),
                )
                .await;
        }
    }
}

/// The URI and position of a request, in the form the analysis closures want:
/// an owned URI string and a plain position, neither borrowing the request.
pub(crate) fn at(params: TextDocumentPositionParams) -> (String, Position) {
    (params.text_document.uri.to_string(), params.position)
}
