use tower_lsp_f::jsonrpc::Result as LspResult;
use tower_lsp_f::lsp_types::*;
use tower_lsp_f::LanguageServer;

use super::Backend;
use super::{edit, insight, navigate};
use crate::backend::{capabilities, lifecycle, sync};

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
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

        if let Some(root_uri) = params
            .workspace_folders_initialize_params
            .workspace_folders
            .as_ref()
            .and_then(|folders| match folders {
                WorkspaceFolders::WorkspaceFolderList(list) => {
                    list.first().map(|folder| folder.uri.clone())
                }
                WorkspaceFolders::Null => None,
            })
            .or_else(|| {
                #[allow(deprecated)]
                params.root_uri.clone()
            })
        {
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
            .log_message(MessageType::Info, "Varn Language Server initialized")
            .await;
        self.pull_configuration().await;

        if let Some(reason) = self.std_error {
            let msg =
                format!("Varn stdlib unavailable — `std:` imports will not resolve: {reason}");
            self.client.log_message(MessageType::Error, &msg).await;
            self.client.show_message(MessageType::Error, msg).await;
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

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.settings.apply(&params.settings);
        self.pull_configuration().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        sync::did_change_watched_files(self, params).await;
    }

    async fn hover(&self, params: HoverParams) -> LspResult<Option<Hover>> {
        insight::hover(self, params).await
    }

    async fn completion(&self, params: CompletionParams) -> LspResult<Option<CompletionResponse>> {
        edit::completion(self, params).await
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> LspResult<Option<SignatureHelp>> {
        insight::signature_help(self, params).await
    }

    async fn goto_definition(
        &self,
        params: DefinitionParams,
    ) -> LspResult<Option<DefinitionResponse>> {
        navigate::goto_definition(self, params).await
    }

    async fn goto_declaration(
        &self,
        params: DeclarationParams,
    ) -> LspResult<Option<DeclarationResponse>> {
        navigate::goto_declaration(self, params).await
    }

    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> LspResult<Option<Vec<DocumentLink>>> {
        navigate::document_link(self, params).await
    }

    async fn references(&self, params: ReferenceParams) -> LspResult<Option<Vec<Location>>> {
        navigate::references(self, params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> LspResult<Option<PrepareRenameResult>> {
        edit::prepare_rename(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> LspResult<Option<WorkspaceEdit>> {
        edit::rename(self, params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> LspResult<Option<DocumentSymbolResponse>> {
        navigate::document_symbol(self, params).await
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> LspResult<Option<Vec<CodeActionResponse>>> {
        edit::code_action(self, params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> LspResult<Option<SemanticTokens>> {
        insight::semantic_tokens_full(self, params).await
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> LspResult<Option<SemanticTokens>> {
        insight::semantic_tokens_range(self, params).await
    }

    async fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> LspResult<Option<SemanticTokensDeltaResponse>> {
        insight::semantic_tokens_full_delta(self, params).await
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> LspResult<DocumentDiagnosticReport> {
        insight::diagnostic(self, params).await
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> LspResult<WorkspaceDiagnosticReport> {
        insight::workspace_diagnostic(self, params).await
    }

    async fn completion_resolve(&self, params: CompletionItem) -> LspResult<CompletionItem> {
        edit::completion_resolve(self, params).await
    }

    async fn code_lens_resolve(&self, params: CodeLens) -> LspResult<CodeLens> {
        edit::code_lens_resolve(self, params).await
    }

    async fn inlay_hint_resolve(&self, params: InlayHint) -> LspResult<InlayHint> {
        insight::inlay_hint_resolve(self, params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> LspResult<Option<Vec<DocumentHighlight>>> {
        navigate::document_highlight(self, params).await
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> LspResult<Option<Vec<FoldingRange>>> {
        insight::folding_range(self, params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> LspResult<Option<WorkspaceSymbolResponse>> {
        navigate::symbol(self, params).await
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> LspResult<Option<Vec<InlayHint>>> {
        insight::inlay_hint(self, params).await
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        edit::formatting(self, params).await
    }

    async fn code_lens(&self, params: CodeLensParams) -> LspResult<Option<Vec<CodeLens>>> {
        insight::code_lens(self, params).await
    }

    async fn goto_type_definition(
        &self,
        params: TypeDefinitionParams,
    ) -> LspResult<Option<TypeDefinitionResponse>> {
        navigate::goto_type_definition(self, params).await
    }

    async fn goto_implementation(
        &self,
        params: ImplementationParams,
    ) -> LspResult<Option<ImplementationResponse>> {
        navigate::goto_implementation(self, params).await
    }

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        navigate::prepare_type_hierarchy(self, params).await
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        navigate::supertypes(self, params).await
    }

    async fn subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
        navigate::subtypes(self, params).await
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> LspResult<Option<LinkedEditingRanges>> {
        navigate::linked_editing_range(self, params).await
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        edit::range_formatting(self, params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> LspResult<Option<Vec<CallHierarchyItem>>> {
        insight::prepare_call_hierarchy(self, params).await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
        insight::incoming_calls(self, params).await
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
        insight::outgoing_calls(self, params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> LspResult<Option<Vec<SelectionRange>>> {
        insight::selection_range(self, params).await
    }

    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> LspResult<Option<Vec<TextEdit>>> {
        edit::on_type_formatting(self, params).await
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> LspResult<Option<LspAny>> {
        edit::execute_command(self, params).await
    }

    async fn did_create_files(&self, params: CreateFilesParams) {
        sync::did_create_files(self, params).await;
    }

    async fn did_delete_files(&self, params: DeleteFilesParams) {
        sync::did_delete_files(self, params).await;
    }

    async fn did_rename_files(&self, params: RenameFilesParams) {
        sync::did_rename_files(self, params).await;
    }

    async fn will_rename_files(
        &self,
        params: RenameFilesParams,
    ) -> LspResult<Option<WorkspaceEdit>> {
        sync::will_rename_files(self, params).await
    }
}
