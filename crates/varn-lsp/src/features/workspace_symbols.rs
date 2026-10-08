use crate::index::ProjectIndex;
use crate::util::converters::to_lsp_symbol_kind;
use tower_lsp_f::lsp_types::{
    BaseSymbolInformation, Location, Position, Range, SymbolInformation, Uri,
};
pub fn build_workspace_symbols(index: &ProjectIndex, query: &str) -> Vec<SymbolInformation> {
    let q = query.to_lowercase();
    let mut results: Vec<SymbolInformation> = Vec::new();
    for (name, entries) in &index.name_index {
        if !q.is_empty() && !name.to_lowercase().contains(q.as_str()) {
            continue;
        }
        for entry in entries {
            let Ok(uri) = Uri::parse(&entry.uri) else {
                continue;
            };
            let pos = Position {
                line: entry.line,
                character: entry.col,
            };
            #[allow(deprecated)]
            let symbol = SymbolInformation {
                deprecated: None,
                location: Location::new(
                    uri,
                    Range {
                        start: pos,
                        end: pos,
                    },
                ),
                base_symbol_information: BaseSymbolInformation {
                    name: name.clone(),
                    kind: to_lsp_symbol_kind(entry.kind),
                    tags: None,
                    container_name: None,
                },
            };
            results.push(symbol);
        }
    }
    results.sort_by(|a, b| {
        a.base_symbol_information
            .name
            .cmp(&b.base_symbol_information.name)
            .then_with(|| a.location.uri.as_str().cmp(b.location.uri.as_str()))
    });
    results.truncate(200);
    results
}
