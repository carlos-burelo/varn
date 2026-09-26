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

/// Files past this size are skipped by the *automatic* startup scan.
///
/// A handful of `tests/errors/*.vn` fixtures exist specifically to be
/// enormous (one is 1.5MB / 65,546 lines, generated to trip a constant-pool
/// overflow) — checking one has been observed pushing the server to 8GB+ RSS.
/// Nothing here fixes that cost; excluding these from the scan nobody asked
/// for is the scoped mitigation for the deployment context that matters (a
/// server sitting idle in an editor should not gamble a user's machine on a
/// stress-test fixture). Opening one by hand still analyses it normally —
/// this only skips the eager, unrequested pass.
const INDEX_SIZE_LIMIT_BYTES: u64 = 256 * 1024;

/// Index every `.vn` file under the workspace root.
///
/// Directory walk and file reads are I/O and stay off the analysis thread; only
/// the analysis of each file is submitted to it, one `run_background` call per
/// file so a live request queued mid-scan only ever waits behind whichever
/// single file is currently running, never the rest of the scan.
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
            let size = std::fs::metadata(&abs_path).ok()?.len();
            if size > INDEX_SIZE_LIMIT_BYTES {
                return Some(Err((abs_path, size)));
            }
            let uri = Url::from_file_path(&abs_path).ok()?;
            let source = std::fs::read_to_string(&abs_path).ok()?;
            Some(Ok((abs_path, uri, source)))
        })
        .await
        .ok()
        .flatten();

        if let Some(Err((abs_path, size))) = &read {
            client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "[index] skipping {} ({} KB > {} KB startup-scan limit)",
                        abs_path.display(),
                        size / 1024,
                        INDEX_SIZE_LIMIT_BYTES / 1024
                    ),
                )
                .await;
        }

        if let Some(Ok((abs_path, uri, source))) = read {
            let elapsed = analysis
                .run_background(move |a| {
                    let file_start = std::time::Instant::now();
                    a.workspace.index_file(uri.to_string(), source);
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
    let mem_msg = crate::backend::mem::resident_kb()
        .map(|kb| format!(" (RSS: {} MB)", kb / 1024))
        .unwrap_or_default();
    client
        .log_message(
            MessageType::INFO,
            format!(
                "Workspace indexed successfully in {:?}{mem_msg}",
                start.elapsed()
            ),
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
                            || n == ".gemini"
                            || n == "dist"
                            || n == "build"
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
