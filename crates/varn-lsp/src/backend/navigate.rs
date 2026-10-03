use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;

use super::state::{at, Backend};
use crate::features::definition::build_goto_definition;
use crate::features::document_highlight::build_document_highlights;
use crate::features::implementation::build_goto_implementation;
use crate::features::references::build_references;
use crate::features::symbols::build_document_symbols;
use crate::features::type_definition::build_goto_type_definition;
use crate::features::workspace_symbols::build_workspace_symbols;

pub(crate) async fn goto_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> LspResult<Option<GotoDefinitionResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("goto_definition", move |an| {
            let state = an.workspace.get(&uri)?;
            let index = an.workspace.index.read().ok();
            build_goto_definition(&state, index.as_deref(), pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn references(
    backend: &Backend,
    params: ReferenceParams,
) -> LspResult<Option<Vec<Location>>> {
    let (uri, pos) = at(params.text_document_position);
    Ok(backend
        .query("references", move |an| {
            let state = an.workspace.get(&uri)?;
            build_references(&state, &an.workspace, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn document_symbol(
    backend: &Backend,
    params: DocumentSymbolParams,
) -> LspResult<Option<DocumentSymbolResponse>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("document_symbol", move |an| {
            an.workspace.get(&uri).map(|d| build_document_symbols(&d))
        })
        .await)
}

pub(crate) async fn document_highlight(
    backend: &Backend,
    params: DocumentHighlightParams,
) -> LspResult<Option<Vec<DocumentHighlight>>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("document_highlight", move |an| {
            let doc = an.workspace.get(&uri)?;
            Some(build_document_highlights(&doc, pos.line, pos.character))
        })
        .await)
}

pub(crate) async fn symbol(
    backend: &Backend,
    params: WorkspaceSymbolParams,
) -> LspResult<Option<Vec<SymbolInformation>>> {
    let results = backend
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

pub(crate) async fn goto_type_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> LspResult<Option<GotoDefinitionResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
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

pub(crate) async fn goto_implementation(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> LspResult<Option<GotoDefinitionResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("goto_implementation", move |an| {
            let state = an.workspace.get(&uri)?;
            build_goto_implementation(&state, &an.workspace, pos.line, pos.character)
        })
        .await)
}
