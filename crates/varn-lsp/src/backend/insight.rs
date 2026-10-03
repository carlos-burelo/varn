use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;

use super::state::{at, Backend};
use crate::features::call_hierarchy::{
    incoming_calls as build_incoming_calls, outgoing_calls as build_outgoing_calls,
    prepare_call_hierarchy as build_prepare_call_hierarchy,
};
use crate::features::folding::build_folding_ranges;
use crate::features::hover::build_hover;
use crate::features::inlay_hints::build_inlay_hints;
use crate::features::selection_range::build_selection_ranges;
use crate::features::semantic_tokens::build_semantic_tokens;
use crate::features::signature_help::build_signature_help;

pub(crate) async fn hover(backend: &Backend, params: HoverParams) -> LspResult<Option<Hover>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("hover", move |an| {
            let doc = an.workspace.get(&uri)?;
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
            let doc = an.workspace.get(&uri)?;
            build_signature_help(&doc, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn semantic_tokens_full(
    backend: &Backend,
    params: SemanticTokensParams,
) -> LspResult<Option<SemanticTokensResult>> {
    let uri = params.text_document.uri.to_string();
    let tokens = backend
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
    Ok(backend
        .query("inlay_hint", move |an| {
            an.workspace.get(&uri).map(|d| build_inlay_hints(&d))
        })
        .await)
}

pub(crate) async fn code_lens(
    backend: &Backend,
    params: CodeLensParams,
) -> LspResult<Option<Vec<CodeLens>>> {
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
