use tower_lsp_f::lsp_types::*;
use tower_lsp_f::Client;

use crate::analysis::AnalysisHandle;
use crate::backend::progress::Progress;
use crate::backend::SLOW_REQUEST_MS;

pub fn supports_progress(caps: &ClientCapabilities) -> bool {
    caps.window
        .as_ref()
        .and_then(|w| w.work_done_progress)
        .unwrap_or(false)
}

pub fn supports_configuration(caps: &ClientCapabilities) -> bool {
    caps.workspace
        .as_ref()
        .and_then(|w| w.configuration)
        .unwrap_or(false)
}

const INDEX_SIZE_LIMIT_BYTES: u64 = 512 * 1024;

pub async fn index_workspace(client: Client, analysis: AnalysisHandle, progress_supported: bool) {
    let Ok(root) = std::env::current_dir() else {
        return;
    };
    client
        .log_message(
            MessageType::Info,
            format!("Indexing workspace: scanning {root:?}"),
        )
        .await;

    let start = std::time::Instant::now();
    let mut files = tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        walk_dir(&root, &mut files);
        files
    })
    .await
    .unwrap_or_default();

    files.sort();

    let mut read_set = tokio::task::JoinSet::new();
    for path in files {
        read_set.spawn_blocking(move || {
            let abs_path = std::fs::canonicalize(&path).ok()?;
            let size = std::fs::metadata(&abs_path).ok()?.len();
            if size > INDEX_SIZE_LIMIT_BYTES {
                return Some(Err((abs_path, size)));
            }
            let uri = Uri::from_file_path(&abs_path).ok()?;
            let source = std::fs::read_to_string(&abs_path).ok()?;
            Some(Ok((abs_path, uri, source)))
        });
    }
    let mut skipped: Vec<(std::path::PathBuf, u64)> = Vec::new();
    let mut ready: Vec<(std::path::PathBuf, Uri, String)> = Vec::new();
    while let Some(joined) = read_set.join_next().await {
        match joined.ok().flatten() {
            Some(Ok(row)) => ready.push(row),
            Some(Err((abs, size))) => skipped.push((abs, size)),
            None => {}
        }
    }
    for (abs_path, size) in &skipped {
        client
            .log_message(
                MessageType::Info,
                format!(
                    "[index] skipping {} ({} KB > {} KB startup-scan limit)",
                    abs_path.display(),
                    size / 1024,
                    INDEX_SIZE_LIMIT_BYTES / 1024
                ),
            )
            .await;
    }

    ready.sort_by(|a, b| a.0.cmp(&b.0));

    let total = ready.len() + skipped.len();
    let progress = Progress::begin(
        &client,
        progress_supported,
        "varn/index",
        "Indexing Varn workspace",
    )
    .await;

    for (idx, (abs_path, uri, source)) in ready.into_iter().enumerate() {
        let elapsed = analysis
            .run_background(move |a| {
                let file_start = std::time::Instant::now();
                a.workspace.index_file(uri.to_string(), source);
                file_start.elapsed()
            })
            .await;
        if let Some(elapsed) = elapsed {
            if super::state::verbose() && elapsed.as_millis() >= SLOW_REQUEST_MS {
                client
                    .log_message(
                        MessageType::Warning,
                        format!(
                            "[perf] slow index {} ({}ms)",
                            abs_path.display(),
                            elapsed.as_millis()
                        ),
                    )
                    .await;
            }
        }

        let done = skipped.len() + idx + 1;
        if done.is_multiple_of(25) || done == total {
            progress
                .report(
                    format!("{done}/{total} files"),
                    (done * 100 / total.max(1)) as u32,
                )
                .await;
        }
    }

    let (ev_b, ev_p, ev_a) = analysis
        .run_background(|a| {
            use varn_checker::module_resolver::ImportResolver;
            a.workspace.resolver().evict_heavy()
        })
        .await
        .unwrap_or((0, 0, 0));

    progress.end(format!("{total} files")).await;
    let mem_msg = crate::backend::mem::resident_kb()
        .map(|kb| format!(" (RSS: {} MB)", kb / 1024))
        .unwrap_or_default();
    client
        .log_message(
            MessageType::Info,
            format!(
                "Workspace indexed successfully in {:?}{mem_msg} \
                 (evicted binds:{ev_b} programs:{ev_p} arenas:{ev_a})",
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
