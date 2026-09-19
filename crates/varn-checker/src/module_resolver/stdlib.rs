/// Where a stdlib module's definition comes from.
///
/// Precompiled interface blobs win when present: they skip parsing and binding
/// entirely. `core:types` is excluded because it is what the alias resolver
/// reads, and it must come from real source.
pub(super) enum Carrier {
    Blob,
    Embedded(&'static str),
    File(String),
}

pub(super) fn stdlib_carrier(specifier: &str) -> Option<Carrier> {
    let provider = varn_modules::provider::get()?;

    // Source FIRST. The precompiled interface blob carries no portable
    // encoding for a `CheckerTyId`, so loading it degrades every non-intrinsic
    // type to `Dynamic` (see `cache::deserialize_module_interface`) — a class
    // imported through a blob loses its member types, and member lookups then
    // fail or type against the wrong shape depending on whether the exporting
    // module was reached through a blob or through source. Binding from the
    // embedded/bundled source is exactly what tree mode does, so both modes
    // now produce the same checking environment; the blob remains the
    // bytecode carrier.
    if specifier != "core:types" {
        if let Some(source) = provider
            .embedded_source(specifier)
            .or_else(|| provider.bundled_source(specifier))
        {
            return Some(Carrier::Embedded(source));
        }
    }
    if provider.interface_blob(specifier).is_some() {
        return Some(Carrier::Blob);
    }
    provider
        .source_path(specifier)
        .map(|p| Carrier::File(p.to_string_lossy().into_owned()))
}
