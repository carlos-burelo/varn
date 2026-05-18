use super::{ExportEntry, ProjectIndex};

pub fn exports_for_module<'a>(index: &'a ProjectIndex, uri: &str) -> &'a [ExportEntry] {
    index.exports_for(uri)
}

pub fn definitions_named<'a>(index: &'a ProjectIndex, name: &str) -> &'a [(String, ExportEntry)] {
    index.definitions_of(name)
}

pub fn search_by_name<'a>(
    index: &'a ProjectIndex,
    query: &str,
) -> impl Iterator<Item = (&'a str, &'a ExportEntry)> {
    let query_lower = query.to_lowercase();
    index
        .name_index
        .iter()
        .filter(move |(name, _)| name.to_lowercase().contains(&query_lower))
        .flat_map(|(_, entries)| entries.iter().map(|(uri, e)| (uri.as_str(), e)))
}

pub fn all_exports(index: &ProjectIndex) -> impl Iterator<Item = (&str, &ExportEntry)> {
    index
        .module_exports
        .iter()
        .flat_map(|(uri, entries)| entries.iter().map(move |e| (uri.as_str(), e)))
}
