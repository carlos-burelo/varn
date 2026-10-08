use tower_lsp_f::jsonrpc::Result as LspResult;
use tower_lsp_f::lsp_types::*;

use super::state::{at, Backend};
use crate::features::call_hierarchy::{
    incoming_calls as build_incoming_calls, outgoing_calls as build_outgoing_calls,
    prepare_call_hierarchy as build_prepare_call_hierarchy,
};
use crate::features::folding::build_folding_ranges;
use crate::features::hover::build_hover;
use crate::features::inlay_hints::build_inlay_hints;
use crate::features::selection_range::build_selection_ranges;
use crate::features::signature_help::build_signature_help;

pub(crate) async fn hover(backend: &Backend, params: HoverParams) -> LspResult<Option<Hover>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("hover", move |an| {
            let doc = an.workspace.get_fresh(&uri)?;
            build_hover(&doc, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn signature_help(
    backend: &Backend,
    params: SignatureHelpParams,
) -> LspResult<Option<SignatureHelp>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("signature_help", move |an| {
            let doc = an.workspace.get_fresh(&uri)?;
            build_signature_help(&doc, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn semantic_tokens_full(
    backend: &Backend,
    params: SemanticTokensParams,
) -> LspResult<Option<SemanticTokens>> {
    let uri = params.text_document.uri.to_string();
    let tokens = backend
        .query("semantic_tokens_full", move |an| {
            let doc = an.workspace.get(&uri)?;
            Some(encode_tokens(
                &crate::features::semantic_tokens::build_semantic_tokens(&doc),
            ))
        })
        .await;
    Ok(tokens.map(|data| SemanticTokens {
        result_id: Some(data_id(&data)),
        data,
    }))
}

pub(crate) async fn semantic_tokens_range(
    backend: &Backend,
    params: SemanticTokensRangeParams,
) -> LspResult<Option<SemanticTokens>> {
    let uri = params.text_document.uri.to_string();
    let range = params.range;
    let tokens = backend
        .query("semantic_tokens_range", move |an| {
            let doc = an.workspace.get(&uri)?;
            Some(filter_tokens_to_range(
                &crate::features::semantic_tokens::build_semantic_tokens(&doc),
                range,
            ))
        })
        .await;
    Ok(tokens.map(|data| SemanticTokens {
        result_id: None,
        data,
    }))
}

pub(crate) async fn semantic_tokens_full_delta(
    backend: &Backend,
    params: SemanticTokensDeltaParams,
) -> LspResult<Option<SemanticTokensDeltaResponse>> {
    let uri = params.text_document.uri.to_string();
    let previous = params.previous_result_id;
    let tokens = backend
        .query("semantic_tokens_delta", move |an| {
            let doc = an.workspace.get(&uri)?;
            Some(encode_tokens(
                &crate::features::semantic_tokens::build_semantic_tokens(&doc),
            ))
        })
        .await;
    let Some(data) = tokens else {
        return Ok(None);
    };
    let id = data_id(&data);
    if previous == id {
        return Ok(Some(SemanticTokensDeltaResponse::SemanticTokensDelta(
            SemanticTokensDelta {
                result_id: Some(id),
                edits: Vec::new(),
            },
        )));
    }
    Ok(Some(SemanticTokensDeltaResponse::SemanticTokens(
        SemanticTokens {
            result_id: Some(id),
            data,
        },
    )))
}

fn diagnostic_result_id(items: &[tower_lsp_f::lsp_types::Diagnostic]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    h.write_usize(items.len());
    for d in items {
        d.range.start.line.hash(&mut h);
        d.range.start.character.hash(&mut h);
        d.range.end.line.hash(&mut h);
        d.message.hash(&mut h);
        format!("{:?}", d.severity).hash(&mut h);
    }
    format!("{:x}", h.finish())
}

pub(crate) async fn diagnostic(
    backend: &Backend,
    params: DocumentDiagnosticParams,
) -> LspResult<DocumentDiagnosticReport> {
    use crate::features::diagnostics::convert_diagnostics;
    let uri_str = params.text_document.uri.to_string();
    let previous = params.previous_result_id;
    let items = backend
        .query("diagnostic_pull", move |an| {
            an.workspace
                .get(&uri_str)
                .map(|doc| convert_diagnostics(&doc))
        })
        .await
        .unwrap_or_default();
    let result_id = diagnostic_result_id(&items);
    if previous.as_deref() == Some(result_id.as_str()) {
        return Ok(
            DocumentDiagnosticReport::RelatedUnchangedDocumentDiagnosticReport(
                RelatedUnchangedDocumentDiagnosticReport {
                    related_documents: None,
                    unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                        result_id,
                    },
                },
            ),
        );
    }
    Ok(
        DocumentDiagnosticReport::RelatedFullDocumentDiagnosticReport(
            RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: Some(result_id),
                    items,
                },
            },
        ),
    )
}

pub(crate) async fn workspace_diagnostic(
    backend: &Backend,
    params: WorkspaceDiagnosticParams,
) -> LspResult<WorkspaceDiagnosticReport> {
    use crate::features::diagnostics::convert_diagnostics;
    let previous: std::collections::HashMap<String, String> = params
        .previous_result_ids
        .into_iter()
        .map(|p| (p.uri.to_string(), p.value))
        .collect();
    let items = backend
        .query("workspace_diagnostic", move |an| {
            let mut out = Vec::new();
            for entry in an.workspace.iter() {
                let uri_str = entry.key().clone();
                let Ok(uri) = Uri::parse(&uri_str) else {
                    continue;
                };
                let diags = convert_diagnostics(entry.value());
                if let Some(prev) = previous.get(&uri_str) {
                    if prev == "empty" && diags.is_empty() {
                        out.push(
                            WorkspaceDocumentDiagnosticReport::WorkspaceUnchangedDocumentDiagnosticReport(
                                WorkspaceUnchangedDocumentDiagnosticReport {
                                    uri,
                                    version: None,
                                    unchanged_document_diagnostic_report:
                                        UnchangedDocumentDiagnosticReport {
                                            result_id: "empty".to_string(),
                                        },
                                },
                            ),
                        );
                        continue;
                    }
                }
                let result_id = if diags.is_empty() {
                    "empty".to_string()
                } else {
                    an.workspace.revision().to_string()
                };
                out.push(
                    WorkspaceDocumentDiagnosticReport::WorkspaceFullDocumentDiagnosticReport(
                        WorkspaceFullDocumentDiagnosticReport {
                            uri,
                            version: None,
                            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                                result_id: Some(result_id),
                                items: diags,
                            },
                        },
                    ),
                );
            }
            Some(out)
        })
        .await
        .unwrap_or_default();
    Ok(WorkspaceDiagnosticReport { items })
}

fn encode_tokens(raw: &[u32]) -> Vec<SemanticToken> {
    raw.chunks_exact(5)
        .map(|c| SemanticToken {
            delta_line: c[0],
            delta_start: c[1],
            length: c[2],
            token_type: c[3],
            token_modifiers_bitset: c[4],
        })
        .collect()
}

fn data_id(data: &[SemanticToken]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    h.write_usize(data.len());
    for t in data.iter().take(64) {
        t.delta_line.hash(&mut h);
        t.delta_start.hash(&mut h);
        t.length.hash(&mut h);
        t.token_type.hash(&mut h);
    }
    format!("{:x}", h.finish())
}

fn filter_tokens_to_range(raw: &[u32], range: Range) -> Vec<SemanticToken> {
    let mut absolute: Vec<(u32, u32, SemanticToken)> = Vec::new();
    let mut line = 0u32;
    let mut col = 0u32;
    for c in raw.chunks_exact(5) {
        line += c[0];
        col = if c[0] == 0 { col + c[1] } else { c[1] };
        if line < range.start.line || line > range.end.line {
            continue;
        }
        absolute.push((
            line,
            col,
            SemanticToken {
                delta_line: 0,
                delta_start: 0,
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            },
        ));
    }
    let mut result = Vec::with_capacity(absolute.len());
    let mut rel_line = 0u32;
    let mut rel_col = 0u32;
    for (l, co, mut t) in absolute {
        t.delta_line = l - rel_line;
        t.delta_start = if l == rel_line { co - rel_col } else { co };
        rel_line = l;
        rel_col = co;
        result.push(t);
    }
    result
}

pub(crate) async fn folding_range(
    backend: &Backend,
    params: FoldingRangeParams,
) -> LspResult<Option<Vec<FoldingRange>>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("folding_range", move |an| {
            an.workspace.get(&uri).map(|d| build_folding_ranges(&d))
        })
        .await)
}

pub(crate) async fn inlay_hint(
    backend: &Backend,
    params: InlayHintParams,
) -> LspResult<Option<Vec<InlayHint>>> {
    if !backend.settings.inlay_hints_enabled() {
        return Ok(None);
    }
    let uri = params.text_document.uri.to_string();
    let range = params.range;
    Ok(backend
        .query("inlay_hint", move |an| {
            an.workspace.get(&uri).map(|d| {
                build_inlay_hints(&d)
                    .into_iter()
                    .filter(|h| {
                        h.position.line < range.start.line
                            || h.position.line > range.end.line
                            || (h.position.line == range.start.line
                                && h.position.line == range.end.line
                                && h.position.character >= range.start.character
                                && h.position.character <= range.end.character)
                            || (h.position.line == range.start.line
                                && range.start.line != range.end.line
                                && h.position.character >= range.start.character)
                            || (h.position.line == range.end.line
                                && range.start.line != range.end.line
                                && h.position.character <= range.end.character)
                            || (h.position.line > range.start.line
                                && h.position.line < range.end.line)
                    })
                    .collect()
            })
        })
        .await)
}

pub(crate) async fn inlay_hint_resolve(backend: &Backend, hint: InlayHint) -> LspResult<InlayHint> {
    let _ = backend;
    Ok(hint)
}

pub(crate) async fn code_lens(
    backend: &Backend,
    params: CodeLensParams,
) -> LspResult<Option<Vec<CodeLens>>> {
    if !backend.settings.code_lens_enabled() {
        return Ok(Some(Vec::new()));
    }
    let uri = params.text_document.uri;
    let uri_str = uri.to_string();
    Ok(backend
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

pub(crate) async fn prepare_call_hierarchy(
    backend: &Backend,
    params: CallHierarchyPrepareParams,
) -> LspResult<Option<Vec<CallHierarchyItem>>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("prepare_call_hierarchy", move |an| {
            let doc = an.workspace.get(&uri)?;
            build_prepare_call_hierarchy(&doc, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn incoming_calls(
    backend: &Backend,
    params: CallHierarchyIncomingCallsParams,
) -> LspResult<Option<Vec<CallHierarchyIncomingCall>>> {
    Ok(backend
        .query("incoming_calls", move |an| {
            build_incoming_calls(params.item, &an.workspace)
        })
        .await)
}

pub(crate) async fn outgoing_calls(
    backend: &Backend,
    params: CallHierarchyOutgoingCallsParams,
) -> LspResult<Option<Vec<CallHierarchyOutgoingCall>>> {
    Ok(backend
        .query("outgoing_calls", move |an| {
            build_outgoing_calls(params.item, &an.workspace)
        })
        .await)
}

pub(crate) async fn selection_range(
    backend: &Backend,
    params: SelectionRangeParams,
) -> LspResult<Option<Vec<SelectionRange>>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("selection_range", move |an| {
            let doc = an.workspace.get(&uri)?;
            Some(build_selection_ranges(&doc, &params.positions))
        })
        .await)
}
