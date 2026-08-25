//! Document synchronization: keeping the server's text equal to the editor's.

pub mod edits;

use std::time::Instant;

use tower_lsp::lsp_types::*;

use crate::backend::Backend;
use crate::features::diagnostics::convert_diagnostics;

/// How long an edit waits before it is analysed, so that a burst of keystrokes
/// costs one analysis rather than one per key.
const DEBOUNCE_MS: u64 = 150;

/// Re-analyse `uri` and publish its diagnostics.
///
/// The debounce stays here, on the async side. Waiting costs nothing before
/// submitting, whereas waiting *on* the analysis thread would park every other
/// request behind a keystroke.
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
    // Everything that touches `DocumentState` happens inside this closure; only
    // the report — plain LSP types and counts — comes back out.
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
                let user_syms = analysis
                    .symbols()
                    .filter(|s| s.line() != u32::MAX)
                    .count();
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

/// An edit in the editor.
///
/// Under incremental sync the notification carries ranges, not a document, so
/// the new text has to be built from the text the server already holds. That
/// happens *on the analysis thread*, against the database that owns it: mirroring
/// document text on the async side would be a second copy of the one thing the
/// whole server is a projection of, and the two copies would diverge exactly
/// when an edit races an analysis.
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

    // No stored text means no `didOpen` for this document: there is nothing the
    // ranges could be relative to, so there is nothing to salvage.
    let Some(source) = updated else {
        return;
    };
    analyze_and_publish(backend, uri, source, false).await;
}

/// A save. The server declares `includeText: false`, so the text is normally
/// absent and the buffer is already current from `didChange`; a client that
/// sends it anyway is honoured rather than ignored.
pub async fn did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    if let Some(text) = params.text {
        analyze_and_publish(backend, params.text_document.uri, text, true).await;
    }
}

pub async fn did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    let uri = params.text_document.uri.to_string();
    backend
        .analysis
        .submit(move |a| a.workspace.remove_file(&uri));
}

/// A `.vn` file changed outside the editor.
///
/// The client has always registered a `**/*.vn` watcher, but this handler did
/// not exist — so a `git checkout`, a rebase, or an edit from another tool left
/// the server answering from the version it had read at startup, with nothing
/// to say it was stale.
///
/// Deletions evict; creations and changes re-read from disk and re-analyse,
/// which also invalidates every module that imports the file.
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

        // Reading is I/O, so it stays off the analysis thread.
        let Ok(path) = uri.to_file_path() else {
            continue;
        };
        let Ok(Ok(source)) =
            tokio::task::spawn_blocking(move || std::fs::read_to_string(path)).await
        else {
            continue;
        };

        // Eager: the change already happened on disk, so there is no keystroke
        // to debounce against.
        analyze_and_publish(backend, uri, source, true).await;
    }
}
