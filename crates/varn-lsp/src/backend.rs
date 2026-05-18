use std::sync::Arc;

use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::features::code_action::build_code_action;
use crate::features::completion::build_completion_response;
use crate::features::definition::build_goto_definition;
use crate::features::diagnostics::convert_diagnostics;
use crate::features::document_highlight::build_document_highlights;
use crate::features::folding::build_folding_ranges;
use crate::features::hover::build_hover;
use crate::features::inlay_hints::build_inlay_hints;
use crate::features::references::build_references;
use crate::features::rename::{build_prepare_rename, build_rename};
use crate::features::semantic_tokens::{build_semantic_tokens, LEGEND};
use crate::features::signature_help::build_signature_help;
use crate::features::symbols::build_document_symbols;
use crate::features::workspace_symbols::build_workspace_symbols;
use crate::workspace::Workspace;

pub struct Backend {
    pub client: Client,
    pub workspace: Arc<Workspace>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            workspace: Arc::new(Workspace::new()),
        }
    }

    async fn analyze_and_publish(&self, uri: Url, source: String) {
        let uri_str = uri.to_string();
        self.workspace.update_file(uri_str.clone(), source);

        let analysis = match self.workspace.get(&uri_str) {
            Some(a) => a,
            None => return,
        };

        let diags = convert_diagnostics(&analysis);

        let file_name = uri_str
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&uri_str)
            .to_owned();

        let user_syms_count = analysis
            .symbols
            .iter()
            .filter(|s| s.line != u32::MAX)
            .count();
        let stdlib_syms_count = analysis.symbols.len() - user_syms_count;

        self.client
            .log_message(
                MessageType::LOG,
                format!(
                    "── {file_name}  ({} tokens | {} user symbols | {} stdlib)",
                    analysis.tokens.len(),
                    user_syms_count,
                    stdlib_syms_count,
                ),
            )
            .await;

        drop(analysis);
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
                references_provider: Some(OneOf::Left(true)),
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
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Varn Language Server initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.analyze_and_publish(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.pop() {
            self.analyze_and_publish(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.analyze_and_publish(params.text_document.uri, text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.workspace
            .remove_file(params.text_document.uri.as_str());
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
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
        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
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
        Ok(resp)
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
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
        Ok(result)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> LspResult<Option<GotoDefinitionResponse>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;
        let state = self.workspace.get(&uri);
        let index = self.workspace.index.read().ok();
        let result = state
            .as_deref()
            .and_then(|a| build_goto_definition(a, index.as_deref(), pos.line, pos.character));
        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let result = self
            .workspace
            .get(&uri)
            .and_then(|a| build_references(&a, &self.workspace, pos.line, pos.character));
        Ok(result)
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).and_then(|a| {
            build_prepare_rename(&a, params.position.line, params.position.character)
        });
        Ok(result)
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let index = self.workspace.index.read().ok();
        let result = self.workspace.get(&uri).and_then(|a| {
            build_rename(
                &a,
                &self.workspace,
                index.as_deref(),
                pos.line,
                pos.character,
                params.new_name,
            )
        });
        Ok(result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| build_document_symbols(&a));
        Ok(result)
    }

    async fn code_action(&self, params: CodeActionParams) -> LspResult<Option<CodeActionResponse>> {
        Ok(build_code_action(params))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokensResult>> {
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
        Ok(result.map(SemanticTokensResult::Tokens))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
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
        Ok(result)
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| build_folding_ranges(&a));
        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<Vec<SymbolInformation>>> {
        let results = self
            .workspace
            .index
            .read()
            .ok()
            .map(|idx| build_workspace_symbols(&idx, &params.query))
            .unwrap_or_default();
        Ok(if results.is_empty() {
            None
        } else {
            Some(results)
        })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.to_string();
        let result = self.workspace.get(&uri).map(|a| build_inlay_hints(&a));
        Ok(result)
    }
}
