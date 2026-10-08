use std::time::Instant;

use tower_lsp_f::jsonrpc::Result as LspResult;
use tower_lsp_f::lsp_types::*;

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
    static LAST: std::sync::Mutex<(String, u32)> = std::sync::Mutex::new((String::new(), 0));
    let (uri, pos) = at(params.text_document_position_params);
    let trigger_char = params
        .context
        .as_ref()
        .and_then(|c| c.trigger_character.clone());
    let trigger_kind = format!("{:?}", params.context.as_ref().map(|c| c.trigger_kind));

    let Some((resp, log)) = backend
        .query("completion", move |an| {
            let state = an.workspace.get_fresh(&uri)?;
            let index = an.workspace.index.read().ok();
            Some(build_completion_response(
                &state,
                an.workspace.resolver(),
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
        let (flush, emit) = {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if msg == last.0 {
                last.1 += 1;
                (None, None)
            } else {
                let flush = if last.1 > 0 {
                    Some(format!("{} (x{})", last.0, last.1 + 1))
                } else {
                    None
                };
                *last = (msg.clone(), 0);
                (flush, Some(msg))
            }
        };
        if let Some(f) = flush {
            backend.client.log_message(MessageType::Log, f).await;
        }
        if let Some(m) = emit {
            backend.client.log_message(MessageType::Log, m).await;
        }
    }
    Ok(resp)
}

pub(crate) async fn prepare_rename(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> LspResult<Option<PrepareRenameResult>> {
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
    let (uri, pos) = at(params.text_document_position_params);
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
) -> LspResult<Option<Vec<CodeActionResponse>>> {
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

pub(crate) async fn range_formatting(
    backend: &Backend,
    params: DocumentRangeFormattingParams,
) -> LspResult<Option<Vec<TextEdit>>> {
    let uri = params.text_document.uri.to_string();
    let range = params.range;
    Ok(backend
        .query("range_formatting", move |an| {
            let doc = an.workspace.get(&uri)?;
            crate::features::formatting::build_range_edits(
                &doc.source,
                params.options,
                range.start.line,
                range.end.line,
            )
        })
        .await)
}

pub(crate) async fn on_type_formatting(
    backend: &Backend,
    params: DocumentOnTypeFormattingParams,
) -> LspResult<Option<Vec<TextEdit>>> {
    let uri = params.text_document.uri.to_string();
    let pos = params.position;
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
) -> LspResult<Option<LspAny>> {
    let start = Instant::now();
    let result = backend
        .analysis
        .run(move |an| {
            run_command(
                &params.command,
                params.arguments.unwrap_or_default(),
                &an.workspace,
            )
        })
        .await
        .unwrap_or(Ok(None));
    backend.log_slow("execute_command", start.elapsed()).await;
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            backend
                .client
                .log_message(MessageType::Error, format!("execute_command error: {e}"))
                .await;
            Ok(None)
        }
    }
}

pub(crate) async fn completion_resolve(
    _backend: &Backend,
    item: CompletionItem,
) -> LspResult<CompletionItem> {
    Ok(item)
}

pub(crate) async fn code_lens_resolve(_backend: &Backend, lens: CodeLens) -> LspResult<CodeLens> {
    Ok(lens)
}
