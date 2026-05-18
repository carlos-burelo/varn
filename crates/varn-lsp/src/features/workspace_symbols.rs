use tower_lsp::lsp_types::{Location, Position, Range, SymbolInformation, Url};

use crate::index::ProjectIndex;
use crate::util::converters::to_lsp_symbol_kind;

#[allow(deprecated)]
pub fn build_workspace_symbols(index: &ProjectIndex, query: &str) -> Vec<SymbolInformation> {
    let q = query.to_lowercase();

    let mut results: Vec<SymbolInformation> = Vec::new();

    for (name, entries) in &index.name_index {
        if !q.is_empty() && !name.to_lowercase().contains(q.as_str()) {
            continue;
        }
        for (uri, entry) in entries {
            let Ok(url) = Url::parse(uri) else { continue };
            let pos = Position {
                line: entry.line,
                character: entry.col,
            };
            results.push(SymbolInformation {
                name: name.clone(),
                kind: to_lsp_symbol_kind(entry.kind),
                tags: None,
                deprecated: None,
                location: Location::new(
                    url,
                    Range {
                        start: pos,
                        end: pos,
                    },
                ),
                container_name: None,
            });
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    results
}
