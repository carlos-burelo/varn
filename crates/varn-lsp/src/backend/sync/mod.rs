

pub mod edits;

use std::time::Instant;

use tower_lsp::lsp_types::*;

use crate::backend::Backend;
use crate::features::diagnostics::convert_diagnostics;



const DEBOUNCE_MS: u64 = 150;






pub async fn analyze_and_publish(backend: &Backend, uri: Url, source: String, is_eager: bool) {
    let uri_str = uri.to_string();

    let cancel_token = backend
        .analysis
        .run({
            let uri_str = uri_str.clone();
            let source = source.clone();
            move |a| a.workspace.update_source(&uri_str, &source).2
        })
        .await;
    let Some(cancel_token) = cancel_token else {
        return;
    };

    if !is_eager {
        tokio::time::sleep(std::time::Duration::from_millis(DEBOUNCE_MS)).await;
        if cancel_token.is_cancelled() {
            return;
        }
    }

    let start = Instant::now();
    
    
    let report = backend
        .analysis
        .run({
            let uri_str = uri_str.clone();
            move |a| {
                if cancel_token.is_cancelled() {
                    return None;
                }
                a.workspace.update_file(uri_str.clone(), source);
                let analysis = a.workspace.get(&uri_str)?;
                let user_syms = analysis.symbols().filter(|s| s.line() != u32::MAX).count();
                Some((
                    convert_diagnostics(&analysis),
                    analysis.tokens.len(),
                    user_syms,
                    analysis.symbols.len() - user_syms,
                ))
            }
        })
        .await
        .flatten();

    let Some((diags, tokens, user_syms, stdlib_syms)) = report else {
        return;
    };

    let file_name = uri_str
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&uri_str)
        .to_owned();
    backend
        .client
        .log_message(
            MessageType::LOG,
            format!(
                "── {file_name}  ({tokens} tokens | {user_syms} user symbols | {stdlib_syms} stdlib) [{}ms]",
                start.elapsed().as_millis(),
            ),
        )
        .await;
    backend.client.publish_diagnostics(uri, diags, None).await;
}

pub async fn did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    analyze_and_publish(
        backend,
        params.text_document.uri,
        params.text_document.text,
        true,
    )
    .await;
}









pub async fn did_change(backend: &Backend, params: DidChangeTextDocumentParams) {
    let uri = params.text_document.uri;
    let uri_str = uri.to_string();
    let changes = params.content_changes;

    let updated = backend
        .analysis
        .run(move |a| {
            let mut source = a.workspace.source_of(&uri_str)?;
            edits::apply_changes(&mut source, changes);
            Some(source)
        })
        .await
        .flatten();

    
    
    let Some(source) = updated else {
        return;
    };
    analyze_and_publish(backend, uri, source, false).await;
}




pub async fn did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    if let Some(text) = params.text {
        analyze_and_publish(backend, params.text_document.uri, text, true).await;
    }
}

pub async fn did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = params.text_document.uri.to_string();
    backend
        .analysis
        .submit(move |a| a.workspace.close_file(&uri));
}










pub async fn did_change_watched_files(backend: &Backend, params: DidChangeWatchedFilesParams) {
    for event in params.changes {
        let uri = event.uri.clone();
        let uri_str = uri.to_string();

        if event.typ == FileChangeType::DELETED {
            backend
                .analysis
                .submit(move |a| a.workspace.remove_file(&uri_str));
            continue;
        }

        
        let Ok(path) = uri.to_file_path() else {
            continue;
        };
        let Ok(Ok(source)) =
            tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await
        else {
            continue;
        };

        
        
        analyze_and_publish(backend, uri, source, true).await;
    }
}
