use std::time::Instant;

use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;

use super::state::{at, Backend};
use crate::features::code_action::build_code_action;
use crate::features::compiler_inspect::execute_command as run_command;
use crate::features::completion::build_completion_response;
use crate::features::formatting::build_formatting;
use crate::features::on_type_formatting::build_on_type_formatting;
use crate::features::rename::{build_prepare_rename, build_rename};

pub(crate) async fn completion(
    backend: &Backend,
    params: CompletionParams,
) -> LspResult<Option<CompletionResponse>> {
    let (uri, pos) = at(params.text_document_position);
    let trigger_char = params
        .context
        .as_ref()
        .and_then(|c| c.trigger_character.clone());
    let trigger_kind = format!("{:?}", params.context.as_ref().map(|c| c.trigger_kind));

    let Some((resp, log)) = backend
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
        backend.client.log_message(MessageType::LOG, msg).await;
    }
    Ok(resp)
}

pub(crate) async fn prepare_rename(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> LspResult<Option<PrepareRenameResponse>> {
    let (uri, pos) = at(params);
    Ok(backend
        .query("prepare_rename", move |an| {
            let doc = an.workspace.get(&uri)?;
            build_prepare_rename(&doc, pos.line, pos.character)
        })
        .await)
}

pub(crate) async fn rename(
    backend: &Backend,
    params: RenameParams,
) -> LspResult<Option<WorkspaceEdit>> {
    let (uri, pos) = at(params.text_document_position);
    let new_name = params.new_name;
    Ok(backend
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

pub(crate) async fn code_action(
    backend: &Backend,
    params: CodeActionParams,
) -> LspResult<Option<CodeActionResponse>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("code_action", move |an| {
            let state = an.workspace.get(&uri);
            let index = an.workspace.index.read().ok();
            build_code_action(params, state.as_deref(), index.as_deref())
        })
        .await)
}

pub(crate) async fn formatting(
    backend: &Backend,
    params: DocumentFormattingParams,
) -> LspResult<Option<Vec<TextEdit>>> {
    let uri = params.text_document.uri.to_string();
    Ok(backend
        .query("formatting", move |an| {
            let doc = an.workspace.get(&uri)?;
            build_formatting(&doc.source, params.options)
        })
        .await)
}

pub(crate) async fn on_type_formatting(
    backend: &Backend,
    params: DocumentOnTypeFormattingParams,
) -> LspResult<Option<Vec<TextEdit>>> {
    let uri = params.text_document_position.text_document.uri.to_string();
    let pos = params.text_document_position.position;
    Ok(backend
        .query("on_type_formatting", move |an| {
            let doc = an.workspace.get(&uri)?;
            build_on_type_formatting(&doc.source, pos, &params.ch, params.options)
        })
        .await)
}

pub(crate) async fn execute_command(
    backend: &Backend,
    params: ExecuteCommandParams,
) -> LspResult<Option<serde_json::Value>> {
    let start = Instant::now();
    let result = backend
        .analysis
        .run(move |an| run_command(&params.command, params.arguments, &an.workspace))
        .await
        .unwrap_or(Ok(None));
    backend.log_slow("execute_command", start.elapsed()).await;
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            backend
                .client
                .log_message(MessageType::ERROR, format!("execute_command error: {e}"))
                .await;
            Ok(None)
        }
    }
}
