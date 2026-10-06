use std::time::Instant;

use tower_lsp::lsp_types::*;
use tower_lsp::Client;

use crate::analysis::{AnalysisHandle, Analyzer};

pub(crate) const SLOW_REQUEST_MS: u128 = 30;

pub struct Backend {
    pub client: Client,

    pub(crate) analysis: AnalysisHandle,

    pub(crate) std_error: Option<&'static str>,

    pub(crate) settings: super::settings::Settings,

    pub(crate) progress_supported: std::sync::atomic::AtomicBool,

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

pub(crate) fn at(params: TextDocumentPositionParams) -> (String, Position) {
    (params.text_document.uri.to_string(), params.position)
}
