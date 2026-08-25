use std::time::Instant;

use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::features::call_hierarchy::{
    incoming_calls, outgoing_calls, prepare_call_hierarchy,
};
use crate::features::code_action::build_code_action;
use crate::features::compiler_inspect::execute_command;
use crate::features::completion::build_completion_response;
use crate::features::definition::build_goto_definition;
use crate::features::diagnostics::convert_diagnostics;
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
use crate::features::semantic_tokens::{build_semantic_tokens, LEGEND};
use crate::features::signature_help::build_signature_help;
use crate::features::symbols::build_document_symbols;
use crate::features::type_definition::build_goto_type_definition;
use crate::features::workspace_symbols::build_workspace_symbols;
use crate::analysis::AnalysisHandle;

const SLOW_REQUEST_MS: u128 = 30;

pub struct Backend {
    pub client: Client,
    /// The analysis thread. `Backend` holds no analysis state of its own —
    /// none of it is `Send`, so it cannot live next to the request handlers.
    analysis: AnalysisHandle,
    /// Why the active std is unusable, if it is. Reported once on
    /// `initialized`; until it is fixed, `std:` imports resolve to nothing.
    std_error: Option<&'static str>,
}

impl Backend {
    pub fn new(client: Client, std_error: Option<&'static str>) -> Self {
        Self {
            client,
            analysis: AnalysisHandle::spawn(),
            std_error,
        }
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

    /// Re-analyse `uri` and publish its diagnostics.
    ///
    /// The debounce stays here, on the async side. Waiting costs nothing before
    /// submitting, whereas waiting *on* the analysis thread would park every
    /// other request behind a keystroke.
    async fn analyze_and_publish(&self, uri: Url, source: String, is_eager: bool) {
        let uri_str = uri.to_string();

        let cancel_token = self
            .analysis
            .run({
                let uri_str = uri_str.clone();
                let source = source.clone();
                move |a| a.workspace.update_source(&uri_str, &source).2
            })
            .await;
        let Some(cancel_token) = cancel_token else {
            return;
        };

        if !is_eager {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if cancel_token.is_cancelled() {
                return;
            }
        }

        let start = Instant::now();
        // Everything that touches `DocumentState` happens inside this closure;
        // only the report — plain LSP types and counts — comes back out.
        let report = self
            .analysis
            .run({
                let uri_str = uri_str.clone();
                move |a| {
                    if cancel_token.is_cancelled() {
                        return None;
                    }
                    a.workspace.update_file(uri_str.clone(), source);
                    let analysis = a.workspace.get(&uri_str)?;
                    let user_syms = analysis
                        .symbols
                        .iter()
                        .filter(|s| s.line != u32::MAX)
                        .count();
                    Some((
                        convert_diagnostics(&analysis),
                        analysis.tokens.len(),
                        user_syms,
                        analysis.symbols.len() - user_syms,
                    ))
                }
            })
            .await
            .flatten();

        let Some((diags, tokens, user_syms, stdlib_syms)) = report else {
            return;
        };

        let file_name = uri_str
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&uri_str)
            .to_owned();
        self.client
            .log_message(
                MessageType::LOG,
                format!(
                    "── {file_name}  ({tokens} tokens | {user_syms} user symbols | {stdlib_syms} stdlib) [{}ms]",
                    start.elapsed().as_millis(),
                ),
            )
            .await;
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
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
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "'".to_string(),
                        "\"".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "}".to_string(),
                    more_trigger_character: Some(vec![";".to_string(), "\n".to_string()]),
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "varn.showBytecode".to_string(),
                        "varn.showSSA".to_string(),
                        "varn.evalSelection".to_string(),
                    ],
                    work_done_progress_options: Default::default(),
                }),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                document_symbol_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            legend: LEGEND.clone(),
                            ..Default::default()
                        },
                    ),
                ),
                document_highlight_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    InlayHintOptions {
                        resolve_provider: Some(false),
                        work_done_progress_options: Default::default(),
                    },
                ))),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Varn Language Server initialized")
            .await;

        if let Some(reason) = self.std_error {
            let msg =
                format!("Varn stdlib unavailable — `std:` imports will not resolve: {reason}");
            self.client.log_message(MessageType::ERROR, &msg).await;
            self.client.show_message(MessageType::ERROR, msg).await;
        }

        // Directory walk and file reads are I/O and stay off the analysis
        // thread; only the analysis of each file is submitted to it. Doing the
        // walk there would park every request behind the initial scan.
        let analysis = self.analysis.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let Ok(current_dir) = std::env::current_dir() else {
                return;
            };
            client
                .log_message(
                    MessageType::INFO,
                    format!("Indexing workspace: scanning {:?}", current_dir),
                )
                .await;

            let start = std::time::Instant::now();
            let files = tokio::task::spawn_blocking(move || {
                let mut files = Vec::new();
                walk_dir(&current_dir, &mut files);
                files
            })
            .await
            .unwrap_or_default();

            let total = files.len();
            client
                .log_message(
                    MessageType::INFO,
                    format!("Indexing workspace: found {total} files to index"),
                )
                .await;

            for (idx, path) in files.into_iter().enumerate() {
                let read = tokio::task::spawn_blocking(move || {
                    let abs_path = std::fs::canonicalize(&path).ok()?;
                    let uri = Url::from_file_path(&abs_path).ok()?;
                    let source = std::fs::read_to_string(&abs_path).ok()?;
                    Some((abs_path, uri, source))
                })
                .await
                .ok()
                .flatten();

                if let Some((abs_path, uri, source)) = read {
                    let elapsed = analysis
                        .run(move |a| {
                            let file_start = std::time::Instant::now();
                            a.workspace.update_file(uri.to_string(), source);
                            file_start.elapsed()
                        })
                        .await;
                    if let Some(elapsed) = elapsed {
                        if elapsed.as_millis() >= SLOW_REQUEST_MS {
                            client
                                .log_message(
                                    MessageType::WARNING,
                                    format!(
                                        "[perf] slow index {} ({}ms)",
                                        abs_path.display(),
                                        elapsed.as_millis()
                                    ),
                                )
                                .await;
                        }
                    }
                }

                if (idx + 1) % 25 == 0 || idx + 1 == total {
                    client
                        .log_message(
                            MessageType::LOG,
                            format!("[{}/{}] Indexing...", idx + 1, total),
                        )
                        .await;
                }
            }

            client
                .log_message(
                    MessageType::INFO,
                    format!("Workspace indexed successfully in {:?}", start.elapsed()),
                )
                .await;
        });
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.analyze_and_publish(params.text_document.uri, params.text_document.text, true)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.pop() {
            self.analyze_and_publish(params.text_document.uri, change.text, false)
                .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.analyze_and_publish(params.text_document.uri, text, true)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.analysis.submit(move |a| a.workspace.remove_file(&uri));
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| build_hover(&a, pos.line, pos.character)))
            .await
            .flatten();
        self.log_slow("hover", start.elapsed()).await;
        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let trigger_char = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());
        let trigger_kind = format!("{:?}", params.context.as_ref().map(|c| c.trigger_kind));

        let trigger_char = trigger_char.map(str::to_owned);
        let Some((resp, log)) = self
            .analysis
            .run(move |an| {
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
            .flatten()
        else {
            return Ok(None);
        };
        if let Some(msg) = log {
            self.client.log_message(MessageType::LOG, msg).await;
        }
        self.log_slow("completion", start.elapsed()).await;
        Ok(resp)
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| build_signature_help(&a, pos.line, pos.character)))
            .await
            .flatten();
        self.log_slow("signature_help", start.elapsed()).await;
        Ok(result)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri);
                let index = an.workspace.index.read().ok();
                state
                    .as_deref()
                    .and_then(|a| build_goto_definition(a, index.as_deref(), pos.line, pos.character))
            })
            .await
            .flatten();
        self.log_slow("goto_definition", start.elapsed()).await;
        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let result = self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri)?;
                build_references(&state, &an.workspace, pos.line, pos.character)
            })
            .await
            .flatten();
        self.log_slow("references", start.elapsed()).await;
        Ok(result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| { build_prepare_rename(&a, params.position.line, params.position.character) }))
            .await
            .flatten();
        self.log_slow("prepare_rename", start.elapsed()).await;
        Ok(result)
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        let result = self
            .analysis
            .run(move |an| {
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
            .await
            .flatten();
        self.log_slow("rename", start.elapsed()).await;
        Ok(result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).map(|a| build_document_symbols(&a)))
            .await
            .flatten();
        self.log_slow("document_symbol", start.elapsed()).await;
        Ok(result)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.to_string();
        Ok(self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri);
                let index = an.workspace.index.read().ok();
                build_code_action(params, state.as_deref(), index.as_deref())
            })
            .await
            .flatten())
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| {
                an.workspace.get(&uri).map(|a| {
                    let raw = build_semantic_tokens(&a);
                    let tokens = raw
                        .chunks_exact(5)
                        .map(|c| SemanticToken {
                            delta_line: c[0],
                            delta_start: c[1],
                            length: c[2],
                            token_type: c[3],
                            token_modifiers_bitset: c[4],
                        })
                        .collect();
                    SemanticTokens {
                        result_id: None,
                        data: tokens,
                    }
                })
            })
            .await
            .flatten();
        self.log_slow("semantic_tokens_full", start.elapsed()).await;
        Ok(result.map(SemanticTokensResult::Tokens))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).map(|a| build_document_highlights(&a, pos.line, pos.character)))
            .await
            .flatten();
        self.log_slow("document_highlight", start.elapsed()).await;
        Ok(result)
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).map(|a| build_folding_ranges(&a)))
            .await
            .flatten();
        self.log_slow("folding_range", start.elapsed()).await;
        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        let start = Instant::now();
        let results = self
            .analysis
            .run(move |an| {
                an.workspace
                    .index
                    .read()
                    .ok()
                    .map(|idx| build_workspace_symbols(&idx, &params.query))
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
        self.log_slow("symbol", start.elapsed()).await;
        Ok(if results.is_empty() {
            None
        } else {
            Some(results)
        })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).map(|a| build_inlay_hints(&a)))
            .await
            .flatten();
        self.log_slow("inlay_hint", start.elapsed()).await;
        Ok(result)
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| build_formatting(&a.source, params.options)))
            .await
            .flatten();
        self.log_slow("formatting", start.elapsed()).await;
        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let start = Instant::now();
        let uri = params.text_document.uri;
        let uri_str = uri.to_string();
        let result = self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri_str)?;
                Some(crate::features::code_lens::build_code_lenses(
                    &uri,
                    &state,
                    Some(&an.workspace),
                ))
            })
            .await
            .flatten();
        self.log_slow("code_lens", start.elapsed()).await;
        Ok(result)
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri);
                let index = an.workspace.index.read().ok();
                state.as_deref().and_then(|a| {
                    build_goto_type_definition(a, index.as_deref(), pos.line, pos.character)
                })
            })
            .await
            .flatten();
        self.log_slow("goto_type_definition", start.elapsed()).await;
        Ok(result)
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| {
                let state = an.workspace.get(&uri)?;
                build_goto_implementation(&state, &an.workspace, pos.line, pos.character)
            })
            .await
            .flatten();
        self.log_slow("goto_implementation", start.elapsed()).await;
        Ok(result)
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        let start = Instant::now();
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| prepare_call_hierarchy(&a, pos.line, pos.character)))
            .await
            .flatten();
        self.log_slow("prepare_call_hierarchy", start.elapsed()).await;
        Ok(result)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        let start = Instant::now();
        let result = self
            .analysis
            .run(move |an| incoming_calls(params.item, &an.workspace))
            .await
            .flatten();
        self.log_slow("incoming_calls", start.elapsed()).await;
        Ok(result)
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        let start = Instant::now();
        let result = self
            .analysis
            .run(move |an| outgoing_calls(params.item, &an.workspace))
            .await
            .flatten();
        self.log_slow("outgoing_calls", start.elapsed()).await;
        Ok(result)
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> LspResult<Option<Vec<SelectionRange>>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).map(|a| build_selection_ranges(&a, &params.positions)))
            .await
            .flatten();
        self.log_slow("selection_range", start.elapsed()).await;
        Ok(result)
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let result = self
            .analysis
            .run(move |an| an.workspace.get(&uri).and_then(|a| { build_on_type_formatting(&a.source, pos, &params.ch, params.options) }))
            .await
            .flatten();
        self.log_slow("on_type_formatting", start.elapsed()).await;
        Ok(result)
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

fn walk_dir(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if ft.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str());
                    if let Some(n) = name {
                        if n == ".git"
                            || n == "target"
                            || n == ".vn"
                            || n == "node_modules"
                            || n == ".vscode"
                            || n == ".claude"
                        {
                            continue;
                        }
                    }
                    walk_dir(&path, files);
                } else if ft.is_file() {
                    if path.extension().and_then(|e| e.to_str()) == Some("vn") {
                        files.push(path);
                    }
                }
            }
        }
    }
}
