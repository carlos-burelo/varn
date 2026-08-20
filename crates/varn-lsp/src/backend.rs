use std::sync::Arc;
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
use crate::workspace::Workspace;

const SLOW_REQUEST_MS: u128 = 30;

pub struct Backend {
    pub client: Client,
    pub workspace: Arc<Workspace>,
    /// Why the active std is unusable, if it is. Reported once on
    /// `initialized`; until it is fixed, `std:` imports resolve to nothing.
    std_error: Option<&'static str>,
}

impl Backend {
    pub fn new(client: Client, std_error: Option<&'static str>) -> Self {
        Self {
            client,
            workspace: Arc::new(Workspace::new()),
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

    async fn analyze_and_publish(&self, uri: Url, source: String, is_eager: bool) {
        let uri_str = uri.to_string();
        let (_file_id, _rev, cancel_token) = self.workspace.update_source(&uri_str, &source);

        if !is_eager {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if cancel_token.is_cancelled() {
                return;
            }
        }

        let workspace = Arc::clone(&self.workspace);
        let client = self.client.clone();
        let uri_clone = uri.clone();
        let uri_str_clone = uri_str.clone();

        tokio::task::spawn_blocking(move || {
            if cancel_token.is_cancelled() {
                return;
            }

            let start = Instant::now();
            workspace.update_file(uri_str_clone.clone(), source);

            if cancel_token.is_cancelled() {
                return;
            }

            let analysis = match workspace.get(&uri_str_clone) {
                Some(a) => a,
                None => return,
            };

            let diags = convert_diagnostics(&analysis);

            let file_name = uri_str_clone
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&uri_str_clone)
                .to_owned();

            let user_syms_count = analysis
                .symbols
                .iter()
                .filter(|s| s.line != u32::MAX)
                .count();
            let stdlib_syms_count = analysis.symbols.len() - user_syms_count;

            let rt = tokio::runtime::Handle::current();
            let _ = rt.block_on(client.log_message(
                MessageType::LOG,
                format!(
                    "── {file_name}  ({} tokens | {} user symbols | {} stdlib) [{}ms]",
                    analysis.tokens.len(),
                    user_syms_count,
                    stdlib_syms_count,
                    start.elapsed().as_millis(),
                ),
            ));

            drop(analysis);
            let _ = rt.block_on(client.publish_diagnostics(uri_clone, diags, None));
        });
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

        let workspace = Arc::clone(&self.workspace);
        let client = self.client.clone();
        tokio::task::spawn_blocking(move || {
            let current_dir = match std::env::current_dir() {
                Ok(d) => d,
                Err(_) => return,
            };
            let rt = tokio::runtime::Handle::current();
            let _ = rt.block_on(client.log_message(
                MessageType::INFO,
                format!("Indexing workspace: scanning {:?}", current_dir),
            ));
            let start = std::time::Instant::now();
            let mut files = Vec::new();
            walk_dir(&current_dir, &mut files);
            let _ = rt.block_on(client.log_message(
                MessageType::INFO,
                format!("Indexing workspace: found {} files to index", files.len()),
            ));
            let total = files.len();
            for (idx, path) in files.iter().enumerate() {
                if let Ok(abs_path) = std::fs::canonicalize(&path) {
                    if let Ok(uri) = Url::from_file_path(&abs_path) {
                        if let Ok(source) = std::fs::read_to_string(&abs_path) {
                            let file_start = std::time::Instant::now();
                            workspace.update_file(uri.to_string(), source);
                            let elapsed = file_start.elapsed();
                            if elapsed.as_millis() >= SLOW_REQUEST_MS {
                                let _ = rt.block_on(client.log_message(
                                    MessageType::WARNING,
                                    format!(
                                        "[perf] slow index {} ({}ms)",
                                        abs_path.display(),
                                        elapsed.as_millis()
                                    ),
                                ));
                            }
                        }
                    }
                }
                if (idx + 1) % 25 == 0 || idx + 1 == total {
                    let _ = rt.block_on(client.log_message(
                        MessageType::LOG,
                        format!("[{}/{}] Indexing...", idx + 1, total),
                    ));
                }
            }
            let _ = rt.block_on(client.log_message(
                MessageType::INFO,
                format!("Workspace indexed successfully in {:?}", start.elapsed()),
            ));
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
        self.workspace
            .remove_file(params.text_document.uri.as_str());
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
            .workspace
            .get(&uri)
            .and_then(|a| build_hover(&a, pos.line, pos.character));
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

        let (resp, log) = {
            let state = match self.workspace.get(&uri) {
                Some(a) => a,
                None => return Ok(None),
            };
            let index = self.workspace.index.read().ok();
            build_completion_response(
                &state,
                pos.line,
                pos.character,
                trigger_char,
                trigger_kind,
                index.as_deref(),
            )
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
            .workspace
            .get(&uri)
            .and_then(|a| build_signature_help(&a, pos.line, pos.character));
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
        let result = {
            let state = self.workspace.get(&uri);
            let index = self.workspace.index.read().ok();
            state
                .as_deref()
                .and_then(|a| build_goto_definition(a, index.as_deref(), pos.line, pos.character))
        };
        self.log_slow("goto_definition", start.elapsed()).await;
        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let result = self
            .workspace
            .get(&uri)
            .and_then(|a| build_references(&a, &self.workspace, pos.line, pos.character));
        self.log_slow("references", start.elapsed()).await;
        Ok(result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).and_then(|a| {
            build_prepare_rename(&a, params.position.line, params.position.character)
        });
        self.log_slow("prepare_rename", start.elapsed()).await;
        Ok(result)
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let result = {
            let index = self.workspace.index.read().ok();
            self.workspace.get(&uri).and_then(|a| {
                build_rename(
                    &a,
                    &self.workspace,
                    index.as_deref(),
                    pos.line,
                    pos.character,
                    params.new_name,
                )
            })
        };
        self.log_slow("rename", start.elapsed()).await;
        Ok(result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| build_document_symbols(&a));
        self.log_slow("document_symbol", start.elapsed()).await;
        Ok(result)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.to_string();
        let state = self.workspace.get(&uri);
        let index = self.workspace.index.read().ok();
        Ok(build_code_action(params, state.as_deref(), index.as_deref()))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| {
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
        });
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
            .workspace
            .get(&uri)
            .map(|a| build_document_highlights(&a, pos.line, pos.character));
        self.log_slow("document_highlight", start.elapsed()).await;
        Ok(result)
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let start = Instant::now();
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| build_folding_ranges(&a));
        self.log_slow("folding_range", start.elapsed()).await;
        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        let start = Instant::now();
        let results = self
            .workspace
            .index
            .read()
            .ok()
            .map(|idx| build_workspace_symbols(&idx, &params.query))
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
        let result = self.workspace.get(&uri).map(|a| build_inlay_hints(&a));
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
            .workspace
            .get(&uri)
            .and_then(|a| build_formatting(&a.source, params.options));
        self.log_slow("formatting", start.elapsed()).await;
        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        let start = Instant::now();
        let uri = params.text_document.uri;
        let uri_str = uri.to_string();
        let result = self
            .workspace
            .get(&uri_str)
            .map(|a| crate::features::code_lens::build_code_lenses(&uri, &a, Some(&self.workspace)));
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
        let result = {
            let state = self.workspace.get(&uri);
            let index = self.workspace.index.read().ok();
            state.as_deref().and_then(|a| {
                build_goto_type_definition(a, index.as_deref(), pos.line, pos.character)
            })
        };
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
        let result = self.workspace.get(&uri).and_then(|a| {
            build_goto_implementation(&a, &self.workspace, pos.line, pos.character)
        });
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
            .workspace
            .get(&uri)
            .and_then(|a| prepare_call_hierarchy(&a, pos.line, pos.character));
        self.log_slow("prepare_call_hierarchy", start.elapsed()).await;
        Ok(result)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        let start = Instant::now();
        let result = incoming_calls(params.item, &self.workspace);
        self.log_slow("incoming_calls", start.elapsed()).await;
        Ok(result)
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        let start = Instant::now();
        let result = outgoing_calls(params.item, &self.workspace);
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
            .workspace
            .get(&uri)
            .map(|a| build_selection_ranges(&a, &params.positions));
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
        let result = self.workspace.get(&uri).and_then(|a| {
            build_on_type_formatting(&a.source, pos, &params.ch, params.options)
        });
        self.log_slow("on_type_formatting", start.elapsed()).await;
        Ok(result)
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> LspResult<Option<serde_json::Value>> {
        let start = Instant::now();
        let result = execute_command(&params.command, params.arguments, &self.workspace);
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
