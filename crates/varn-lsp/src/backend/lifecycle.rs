//! Server lifecycle: handshake and the initial workspace index.

use tower_lsp::lsp_types::*;
use tower_lsp::Client;

use crate::analysis::AnalysisHandle;
use crate::backend::progress::Progress;
use crate::backend::SLOW_REQUEST_MS;

/// Whether the client reports it can show `$/progress` for server-started work.
pub fn supports_progress(caps: &ClientCapabilities) -> bool {
    caps.window
        .as_ref()
        .and_then(|w| w.work_done_progress)
        .unwrap_or(false)
}

/// Whether the client answers `workspace/configuration`.
pub fn supports_configuration(caps: &ClientCapabilities) -> bool {
    caps.workspace
        .as_ref()
        .and_then(|w| w.configuration)
        .unwrap_or(false)
}

/// Index every `.vn` file under the workspace root.
///
/// Directory walk and file reads are I/O and stay off the analysis thread; only
/// the analysis of each file is submitted to it. Doing the walk there would
/// park every request behind the initial scan.
pub async fn index_workspace(client: Client, analysis: AnalysisHandle, progress_supported: bool) {
    let Ok(root) = std::env::current_dir() else {
        return;
    };
    client
        .log_message(
            MessageType::INFO,
            format!("Indexing workspace: scanning {root:?}"),
        )
        .await;

    let start = std::time::Instant::now();
    let files = tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        walk_dir(&root, &mut files);
        files
    })
    .await
    .unwrap_or_default();

    let total = files.len();
    let progress = Progress::begin(
        &client,
        progress_supported,
        "varn/index",
        "Indexing Varn workspace",
    )
    .await;

    for (idx, path) in files.into_iter().enumerate() {
        let read = tokio::task::spawn_blocking(move || {
            let abs_path = std::fs::canonicalize(&path).ok()?;
            let uri = Url::from_file_path(&abs_path).ok()?;
            let source = std::fs::read_to_string(&abs_path).ok()?;
            Some((abs_path, uri, source))
        })
        .await
        .ok()
        .flatten();

        if let Some((abs_path, uri, source)) = read {
            let elapsed = analysis
                .run(move |a| {
                    let file_start = std::time::Instant::now();
                    a.workspace.update_file(uri.to_string(), source);
                    file_start.elapsed()
                })
                .await;
            if let Some(elapsed) = elapsed {
                if elapsed.as_millis() >= SLOW_REQUEST_MS {
                    client
                        .log_message(
                            MessageType::WARNING,
                            format!(
                                "[perf] slow index {} ({}ms)",
                                abs_path.display(),
                                elapsed.as_millis()
                            ),
                        )
                        .await;
                }
            }
        }

        let done = idx + 1;
        if done % 25 == 0 || done == total {
            progress
                .report(
                    format!("{done}/{total} files"),
                    (done * 100 / total.max(1)) as u32,
                )
                .await;
        }
    }

    progress.end(format!("{total} files")).await;
    client
        .log_message(
            MessageType::INFO,
            format!("Workspace indexed successfully in {:?}", start.elapsed()),
        )
        .await;
}

fn walk_dir(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(ft) = entry.file_type() {
                if ft.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if ft.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str());
                    if let Some(n) = name {
                        if n == ".git"
                            || n == "target"
                            || n == ".vn"
                            || n == "node_modules"
                            || n == ".vscode"
                            || n == ".claude"
                        {
                            continue;
                        }
                    }
                    walk_dir(&path, files);
                } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("vn") {
                    files.push(path);
                }
            }
        }
    }
}
