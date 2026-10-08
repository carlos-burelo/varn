use crate::document::import::uri_to_path;
use crate::workspace::Workspace;
use tower_lsp_f::lsp_types::{Position, Range, TextEdit, Uri, WorkspaceEdit};

pub fn rename_edits(workspace: &Workspace, renames: &[(String, String)]) -> Option<WorkspaceEdit> {
    let mut docs: Vec<(Uri, Vec<TextEdit>)> = Vec::new();
    let index = workspace.index.read().ok()?;
    for (old_uri, new_uri) in renames {
        let dependents: Vec<String> = index.dependents_of(old_uri).map(str::to_owned).collect();
        let new_path = uri_to_path(new_uri);
        for dep_uri in dependents {
            let Some(state) = workspace.get(&dep_uri) else {
                continue;
            };
            let dep_path = uri_to_path(&dep_uri);
            let new_spec = varn_modules::resolver::relative_import_path(&dep_path, &new_path);
            let Ok(uri) = Uri::parse(&dep_uri) else {
                continue;
            };
            for (idx, line) in state.source.lines().enumerate() {
                for (start, end, spec) in quoted_specs(line) {
                    if resolves_to(&state.uri, &spec, old_uri) {
                        let edit = TextEdit {
                            range: Range {
                                start: Position {
                                    line: idx as u32,
                                    character: start,
                                },
                                end: Position {
                                    line: idx as u32,
                                    character: end,
                                },
                            },
                            new_text: new_spec.clone(),
                        };
                        match docs.iter_mut().find(|(u, _)| *u == uri) {
                            Some((_, edits)) => edits.push(edit),
                            None => docs.push((uri.clone(), vec![edit])),
                        }
                    }
                }
            }
        }
    }
    if docs.is_empty() {
        None
    } else {
        Some(crate::features::workspace_edit::doc_edits(docs))
    }
}

fn quoted_specs(line: &str) -> Vec<(u32, u32, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' || c == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                j += 1;
            }
            if j < chars.len() {
                out.push((i as u32 + 1, j as u32, chars[i + 1..j].iter().collect()));
                i = j + 1;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

fn resolves_to(doc_uri: &str, spec: &str, target_uri: &str) -> bool {
    if spec.starts_with("std:") || spec.starts_with("core:") || spec.starts_with("runtime:") {
        return false;
    }
    if !(spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/')) {
        return false;
    }
    let base = uri_to_path(doc_uri);
    let dir = match std::path::Path::new(&base).parent() {
        Some(d) => d,
        None => return false,
    };
    let joined = dir.join(spec);
    let joined_str = joined.to_string_lossy().into_owned();
    let normalized = varn_modules::resolver::normalize_display_path(&joined_str);
    let uri = varn_modules::resolver::path_to_uri(&normalized);
    uri == target_uri
}
