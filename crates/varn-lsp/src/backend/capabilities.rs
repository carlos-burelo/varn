use tower_lsp_f::lsp_types::*;

use crate::features::semantic_tokens::LEGEND;

fn vn_file_filter() -> FileOperationRegistrationOptions {
    FileOperationRegistrationOptions {
        filters: vec![FileOperationFilter {
            scheme: Some("file".to_string()),
            pattern: FileOperationPattern {
                glob: "**/*.vn".to_string(),
                matches: Some(FileOperationPatternKind::File),
                options: None,
            },
        }],
    }
}

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSync::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::Incremental),

            save: Some(Save::SaveOptions(SaveOptions {
                include_text: Some(false),
            })),
            ..Default::default()
        })),
        hover_provider: Some(HoverProvider::Bool(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(true),
            trigger_characters: Some(vec![
                ".".to_string(),
                ":".to_string(),
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
        definition_provider: Some(DefinitionProvider::Bool(true)),
        declaration_provider: Some(DeclarationProvider::Bool(true)),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        type_definition_provider: Some(TypeDefinitionProvider::Bool(true)),
        implementation_provider: Some(ImplementationProvider::Bool(true)),
        type_hierarchy_provider: Some(TypeHierarchyProvider::Bool(true)),
        linked_editing_range_provider: Some(LinkedEditingRangeProvider::Bool(true)),
        document_range_formatting_provider: Some(DocumentRangeFormattingProvider::Bool(true)),
        references_provider: Some(ReferencesProvider::Bool(true)),
        call_hierarchy_provider: Some(CallHierarchyProvider::Bool(true)),
        selection_range_provider: Some(SelectionRangeProvider::Bool(true)),
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "}".to_string(),
            more_trigger_character: Some(vec![";".to_string(), "\n".to_string()]),
        }),

        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![
                "varn.showAst".to_string(),
                "varn.syntaxTree".to_string(),
                "varn.showBytecode".to_string(),
                "varn.showSSA".to_string(),
                "varn.getCFG".to_string(),
                "varn.memoryStats".to_string(),
                "varn.stdList".to_string(),
                "varn.stdRead".to_string(),
            ],
            work_done_progress_options: Default::default(),
        }),
        rename_provider: Some(RenameProvider::RenameOptions(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        diagnostic_provider: Some(DiagnosticProvider::DiagnosticOptions(DiagnosticOptions {
            identifier: Some("varn".to_string()),
            inter_file_dependencies: true,
            workspace_diagnostics: true,
            work_done_progress_options: Default::default(),
        })),
        document_symbol_provider: Some(DocumentSymbolProvider::Bool(true)),
        semantic_tokens_provider: Some(SemanticTokensProvider::SemanticTokensOptions(
            SemanticTokensOptions {
                range: Some(SemanticTokensOptionsRange::Bool(true)),
                full: Some(Full::SemanticTokensFullDelta(SemanticTokensFullDelta {
                    delta: Some(true),
                })),
                legend: LEGEND.clone(),
                ..Default::default()
            },
        )),
        document_highlight_provider: Some(DocumentHighlightProvider::Bool(true)),
        folding_range_provider: Some(FoldingRangeProvider::Bool(true)),
        workspace_symbol_provider: Some(WorkspaceSymbolProvider::Bool(true)),
        inlay_hint_provider: Some(InlayHintProvider::InlayHintOptions(InlayHintOptions {
            resolve_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        code_action_provider: Some(CodeActionProvider::Bool(true)),
        document_formatting_provider: Some(DocumentFormattingProvider::Bool(true)),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(true),
            work_done_progress_options: Default::default(),
        }),
        workspace: Some(WorkspaceOptions {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(ChangeNotifications::Bool(true)),
            }),
            file_operations: Some(FileOperationOptions {
                did_create: Some(vn_file_filter()),
                did_rename: Some(vn_file_filter()),
                did_delete: Some(vn_file_filter()),
                will_rename: Some(vn_file_filter()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
