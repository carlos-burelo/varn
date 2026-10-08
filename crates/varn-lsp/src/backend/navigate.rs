use tower_lsp_f::jsonrpc::Result as LspResult;
use tower_lsp_f::lsp_types::*;

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
    params: DefinitionParams,
) -> LspResult<Option<DefinitionResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("goto_definition", move |an| {
            let state = an.workspace.get_fresh(&uri)?;
            let index = an.workspace.index.read().ok();
            build_goto_definition(&state, index.as_deref(), pos.line, pos.character)
                .map(DefinitionResponse::from)
        })
        .await)
}

pub(crate) async fn goto_declaration(
    backend: &Backend,
    params: DeclarationParams,
) -> LspResult<Option<DeclarationResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("goto_declaration", move |an| {
            let state = an.workspace.get_fresh(&uri)?;
            let index = an.workspace.index.read().ok();
            build_goto_definition(&state, index.as_deref(), pos.line, pos.character).map(|def| {
                match def {
                    Definition::Location(loc) => {
                        DeclarationResponse::Declaration(Declaration::Location(loc))
                    }
                    Definition::LocationList(locs) => {
                        DeclarationResponse::Declaration(Declaration::LocationList(locs))
                    }
                }
            })
        })
        .await)
}

pub(crate) async fn document_link(
    backend: &Backend,
    params: DocumentLinkParams,
) -> LspResult<Option<Vec<DocumentLink>>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("document_link", move |an| {
            let state = an.workspace.get(&uri)?;
            Some(crate::features::document_link::build_document_links(&state))
        })
        .await)
}

pub(crate) async fn references(
    backend: &Backend,
    params: ReferenceParams,
) -> LspResult<Option<Vec<Location>>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("references", move |an| {
            let state = an.workspace.get_fresh(&uri)?;
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
            let doc = an.workspace.get_fresh(&uri)?;
            Some(build_document_highlights(&doc, pos.line, pos.character))
        })
        .await)
}

pub(crate) async fn symbol(
    backend: &Backend,
    params: WorkspaceSymbolParams,
) -> LspResult<Option<WorkspaceSymbolResponse>> {
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
        Some(WorkspaceSymbolResponse::SymbolInformationList(results))
    })
}

pub(crate) async fn goto_type_definition(
    backend: &Backend,
    params: TypeDefinitionParams,
) -> LspResult<Option<TypeDefinitionResponse>> {
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
            .map(TypeDefinitionResponse::from)
        })
        .await)
}

pub(crate) async fn goto_implementation(
    backend: &Backend,
    params: ImplementationParams,
) -> LspResult<Option<ImplementationResponse>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("goto_implementation", move |an| {
            let state = an.workspace.get(&uri)?;
            build_goto_implementation(&state, &an.workspace, pos.line, pos.character)
                .map(ImplementationResponse::from)
        })
        .await)
}

pub(crate) async fn prepare_type_hierarchy(
    backend: &Backend,
    params: TypeHierarchyPrepareParams,
) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("prepare_type_hierarchy", move |an| {
            let state = an.workspace.get(&uri)?;
            let index = an.workspace.index.read().ok();
            crate::features::type_hierarchy::prepare(
                &state,
                index.as_deref(),
                pos.line,
                pos.character,
            )
        })
        .await)
}

pub(crate) async fn supertypes(
    backend: &Backend,
    params: TypeHierarchySupertypesParams,
) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
    let item = params.item;
    Ok(backend
        .query("supertypes", move |an| {
            let index = an.workspace.index.read().ok();
            crate::features::type_hierarchy::supertypes(item, index.as_deref())
        })
        .await)
}

pub(crate) async fn subtypes(
    backend: &Backend,
    params: TypeHierarchySubtypesParams,
) -> LspResult<Option<Vec<TypeHierarchyItem>>> {
    let item = params.item;
    Ok(backend
        .query("subtypes", move |an| {
            let index = an.workspace.index.read().ok();
            crate::features::type_hierarchy::subtypes(item, index.as_deref())
        })
        .await)
}

pub(crate) async fn linked_editing_range(
    backend: &Backend,
    params: LinkedEditingRangeParams,
) -> LspResult<Option<LinkedEditingRanges>> {
    let (uri, pos) = at(params.text_document_position_params);
    Ok(backend
        .query("linked_editing", move |an| {
            let doc = an.workspace.get(&uri)?;
            let token = doc.identifier_token_at(pos.line, pos.character)?;
            let word = doc.lexeme(token);
            let mut ranges = Vec::new();
            for t in doc.tokens.iter() {
                if doc.lexeme(t) == word
                    && doc.symbol_target_at_offset(t.offset)
                        == doc.symbol_target_at_offset(token.offset)
                {
                    ranges.push(crate::util::converters::range_on_line(
                        t.line,
                        t.col,
                        t.col + t.length,
                    ));
                }
            }
            if ranges.is_empty() {
                return None;
            }
            Some(LinkedEditingRanges {
                ranges,
                word_pattern: Some(word.to_owned()),
            })
        })
        .await)
}
