use crate::document::DocumentState;
use tower_lsp_f::lsp_types::{DocumentLink, Position, Range, Uri};

pub fn build_document_links(state: &DocumentState) -> Vec<DocumentLink> {
    let mut out = Vec::new();
    for (idx, line) in state.source.lines().enumerate() {
        let t = line.trim_start();
        if !(t.starts_with("import") || t.starts_with("export") || line.contains(" from ")) {
            continue;
        }
        let bytes: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == '\'' || c == '"' {
                let start = i;
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != c {
                    j += 1;
                }
                if j < bytes.len() {
                    let spec: String = bytes[start + 1..j].iter().collect();
                    if let Some(target) = resolve_specifier(state, &spec) {
                        out.push(DocumentLink {
                            range: Range {
                                start: Position {
                                    line: idx as u32,
                                    character: (start + 1) as u32,
                                },
                                end: Position {
                                    line: idx as u32,
                                    character: j as u32,
                                },
                            },
                            target: Some(target),
                            tooltip: None,
                            data: None,
                        });
                    }
                    i = j + 1;
                    continue;
                }
                break;
            }
            i += 1;
        }
    }
    out
}

fn resolve_specifier(state: &DocumentState, spec: &str) -> Option<Uri> {
    if spec.starts_with("std:") {
        let path = crate::workspace::std_sources::resolve_module_file(spec)?;
        return Uri::from_file_path(path).ok();
    }
    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        let base = crate::document::uri_to_path(&state.uri);
        let base_path = std::path::Path::new(&base);
        let dir = base_path.parent()?;
        let joined = dir.join(spec);
        let joined_str = joined.to_string_lossy().into_owned();
        let normalized = varn_modules::resolver::normalize_display_path(&joined_str);
        let uri = varn_modules::resolver::path_to_uri(&normalized);
        return Uri::parse(&uri).ok();
    }
    if let Some(path) = crate::workspace::std_sources::resolve_module_file(spec) {
        return Uri::from_file_path(path).ok();
    }
    None
}
