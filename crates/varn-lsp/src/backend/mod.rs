//! The LSP surface: one thin dispatch table over the analysis thread.
//!
//! Everything a handler does is the same three steps — turn the request into a
//! position, run a closure on the analysis thread, time it — so those live in
//! [`Backend::query`] and the handlers below are one call each. The work itself
//! belongs to `features/`, and the protocol chores that are not per-request
//! queries live next door: [`capabilities`], [`lifecycle`], [`settings`],
//! [`sync`].

pub mod capabilities;
pub mod lifecycle;
pub mod progress;
pub mod settings;
pub mod sync;

use std::time::Instant;

use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::{AnalysisHandle, Analyzer};
use crate::features::call_hierarchy::{incoming_calls, outgoing_calls, prepare_call_hierarchy};
use crate::features::code_action::build_code_action;
use crate::features::compiler_inspect::execute_command;
use crate::features::completion::build_completion_response;
use crate::features::definition::build_goto_definition;
use crate::features::document_highlight::build_document_highlights;
use crate::features::folding::build_folding_ranges;
use crate::features::formatting::build_formatting;
use crate::features::hover::build_hover;
use crate::features::implementation::build_goto_implementation;
use crate::features::inlay_hints::build_inlay_hints;
use crate::features::on_type_formatting::build_on_type_formatting;
use crate::features::references::build_references;
use crate::features::rename::{build_prepare_rename, build_rename};
use crate::features::selection_range::build_selection_ranges;
use crate::features::semantic_tokens::build_semantic_tokens;
use crate::features::signature_help::build_signature_help;
use crate::features::symbols::build_document_symbols;
use crate::features::type_definition::build_goto_type_definition;
use crate::features::workspace_symbols::build_workspace_symbols;

pub use settings::Settings;

pub(crate) const SLOW_REQUEST_MS: u128 = 30;

pub struct Backend {
    pub client: Client,
    /// The analysis thread. `Backend` holds no analysis state of its own —
    /// none of it is `Send`, so it cannot live next to the request handlers.
    pub(crate) analysis: AnalysisHandle,
    /// Why the active std is unusable, if it is. Reported once on
    /// `initialized`; until it is fixed, `std:` imports resolve to nothing.
    std_error: Option<&'static str>,
    /// Client settings the server honours, refreshed on
    /// `workspace/didChangeConfiguration`.
    settings: Settings,
    /// Whether the client can render `$/progress`, learned at the handshake.
    progress_supported: std::sync::atomic::AtomicBool,
    /// Whether the client answers `workspace/configuration`, learned likewise.
    configuration_supported: std::sync::atomic::AtomicBool,
}

impl Backend {
    pub fn new(client: Client, std_error: Option<&'static str>) -> Self {
        Self {
            client,
            analysis: AnalysisHandle::spawn(),
            std_error,
            settings: Settings::new(),
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
    async fn pull_configuration(&self) {
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
    /// The `Send` bound on `T` is what keeps `Rc`-backed analysis state from
    /// leaving the thread that owns it; see [`crate::analysis`].
    async fn query<T, F>(&self, op: &str, f: F) -> Option<T>
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
    async fn log_slow(&self, op: &str, elapsed: std::time::Duration) {
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
fn at(params: TextDocumentPositionParams) -> (String, Position) {
    (params.text_document.uri.to_string(), params.position)
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Honour settings from the handshake too, not only from a later change:
        // otherwise a client that starts with hints disabled still gets them
        // until it happens to send a configuration notification.
        if let Some(opts) = &params.initialization_options {
            self.settings.apply(opts);
        }
        self.progress_supported.store(
            lifecycle::supports_progress(&params.capabilities),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.configuration_supported.store(
            lifecycle::supports_configuration(&params.capabilities),
            std::sync::atomic::Ordering::Relaxed,
        );

        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                let _ = std::env::set_current_dir(path);
            }
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "varn-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: capabilities::server_capabilities(),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Varn Language Server initialized")
            .await;
        self.pull_configuration().await;

        if let Some(reason) = self.std_error {
            let msg =
                format!("Varn stdlib unavailable — `std:` imports will not resolve: {reason}");
            self.client.log_message(MessageType::ERROR, &msg).await;
            self.client.show_message(MessageType::ERROR, msg).await;
        }

        let analysis = self.analysis.clone();
        let client = self.client.clone();
        let progress = self
            .progress_supported
            .load(std::sync::atomic::Ordering::Relaxed);
        tokio::spawn(lifecycle::index_workspace(client, analysis, progress));
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        sync::did_open(self, params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        sync::did_change(self, params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        sync::did_save(self, params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        sync::did_close(self, params).await;
    }

    /// Settings changed. Take whatever the notification carries — some clients
    /// send the whole payload — and then pull, for the ones that send `null`
    /// and expect to be asked.
    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.settings.apply(&params.settings);
        self.pull_configuration().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        sync::did_change_watched_files(self, params).await;
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("hover", move |an| {
                let doc = an.workspace.get(&uri)?;
                build_hover(&doc, pos.line, pos.character)
            })
            .await)
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let (uri, pos) = at(params.text_document_position);
        let trigger_char = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.clone());
        let trigger_kind = format!("{:?}", params.context.as_ref().map(|c| c.trigger_kind));

        let Some((resp, log)) = self
            .query("completion", move |an| {
                let state = an.workspace.get(&uri)?;
                let index = an.workspace.index.read().ok();
                Some(build_completion_response(
                    &state,
                    pos.line,
                    pos.character,
                    trigger_char.as_deref(),
                    trigger_kind,
                    index.as_deref(),
                ))
            })
            .await
        else {
            return Ok(None);
        };
        if let Some(msg) = log {
            self.client.log_message(MessageType::LOG, msg).await;
        }
        Ok(resp)
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("signature_help", move |an| {
                let doc = an.workspace.get(&uri)?;
                build_signature_help(&doc, pos.line, pos.character)
            })
            .await)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("goto_definition", move |an| {
                let state = an.workspace.get(&uri)?;
                let index = an.workspace.index.read().ok();
                build_goto_definition(&state, index.as_deref(), pos.line, pos.character)
            })
            .await)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let (uri, pos) = at(params.text_document_position);
        Ok(self
            .query("references", move |an| {
                let state = an.workspace.get(&uri)?;
                build_references(&state, &an.workspace, pos.line, pos.character)
            })
            .await)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let (uri, pos) = at(params);
        Ok(self
            .query("prepare_rename", move |an| {
                let doc = an.workspace.get(&uri)?;
                build_prepare_rename(&doc, pos.line, pos.character)
            })
            .await)
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let (uri, pos) = at(params.text_document_position);
        let new_name = params.new_name;
        Ok(self
            .query("rename", move |an| {
                let state = an.workspace.get(&uri)?;
                let index = an.workspace.index.read().ok();
                build_rename(
                    &state,
                    &an.workspace,
                    index.as_deref(),
                    pos.line,
                    pos.character,
                    new_name,
                )
            })
            .await)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("document_symbol", move |an| {
                an.workspace.get(&uri).map(|d| build_document_symbols(&d))
            })
            .await)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("code_action", move |an| {
                let state = an.workspace.get(&uri);
                let index = an.workspace.index.read().ok();
                build_code_action(params, state.as_deref(), index.as_deref())
            })
            .await)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri.to_string();
        let tokens = self
            .query("semantic_tokens_full", move |an| {
                let doc = an.workspace.get(&uri)?;
                Some(SemanticTokens {
                    result_id: None,
                    data: build_semantic_tokens(&doc)
                        .chunks_exact(5)
                        .map(|c| SemanticToken {
                            delta_line: c[0],
                            delta_start: c[1],
                            length: c[2],
                            token_type: c[3],
                            token_modifiers_bitset: c[4],
                        })
                        .collect(),
                })
            })
            .await;
        Ok(tokens.map(SemanticTokensResult::Tokens))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("document_highlight", move |an| {
                let doc = an.workspace.get(&uri)?;
                Some(build_document_highlights(&doc, pos.line, pos.character))
            })
            .await)
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("folding_range", move |an| {
                an.workspace.get(&uri).map(|d| build_folding_ranges(&d))
            })
            .await)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        let results = self
            .query("symbol", move |an| {
                let index = an.workspace.index.read().ok()?;
                Some(build_workspace_symbols(&index, &params.query))
            })
            .await
            .unwrap_or_default();
        Ok(if results.is_empty() {
            None
        } else {
            Some(results)
        })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        if !self.settings.inlay_hints_enabled() {
            return Ok(None);
        }
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("inlay_hint", move |an| {
                an.workspace.get(&uri).map(|d| build_inlay_hints(&d))
            })
            .await)
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("formatting", move |an| {
                let doc = an.workspace.get(&uri)?;
                build_formatting(&doc.source, params.options)
            })
            .await)
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri;
        let uri_str = uri.to_string();
        Ok(self
            .query("code_lens", move |an| {
                let state = an.workspace.get(&uri_str)?;
                Some(crate::features::code_lens::build_code_lenses(
                    &uri,
                    &state,
                    Some(&an.workspace),
                ))
            })
            .await)
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("goto_type_definition", move |an| {
                let state = an.workspace.get(&uri)?;
                build_goto_type_definition(
                    &state,
                    an.workspace.index.read().ok().as_deref(),
                    pos.line,
                    pos.character,
                )
            })
            .await)
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("goto_implementation", move |an| {
                let state = an.workspace.get(&uri)?;
                build_goto_implementation(&state, &an.workspace, pos.line, pos.character)
            })
            .await)
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        let (uri, pos) = at(params.text_document_position_params);
        Ok(self
            .query("prepare_call_hierarchy", move |an| {
                let doc = an.workspace.get(&uri)?;
                prepare_call_hierarchy(&doc, pos.line, pos.character)
            })
            .await)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(self
            .query("incoming_calls", move |an| {
                incoming_calls(params.item, &an.workspace)
            })
            .await)
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(self
            .query("outgoing_calls", move |an| {
                outgoing_calls(params.item, &an.workspace)
            })
            .await)
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> LspResult<Option<Vec<SelectionRange>>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .query("selection_range", move |an| {
                let doc = an.workspace.get(&uri)?;
                Some(build_selection_ranges(&doc, &params.positions))
            })
            .await)
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        Ok(self
            .query("on_type_formatting", move |an| {
                let doc = an.workspace.get(&uri)?;
                build_on_type_formatting(&doc.source, pos, &params.ch, params.options)
            })
            .await)
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> LspResult<Option<serde_json::Value>> {
        let start = Instant::now();
        let result = self
            .analysis
            .run(move |an| execute_command(&params.command, params.arguments, &an.workspace))
            .await
            .unwrap_or(Ok(None));
        self.log_slow("execute_command", start.elapsed()).await;
        match result {
            Ok(v) => Ok(v),
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("execute_command error: {e}"))
                    .await;
                Ok(None)
            }
        }
    }
}
