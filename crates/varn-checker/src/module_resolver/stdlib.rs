/// Where a stdlib module's definition comes from.
///
/// Precompiled interface blobs win when present: they skip parsing and binding
/// entirely. `std:types` is excluded because it is what the alias resolver
/// reads, and it must come from real source.
pub(super) enum Carrier {
    Blob,
    Embedded(&'static str),
    File(String),
}

pub(super) fn stdlib_carrier(specifier: &str) -> Option<Carrier> {
    let provider = varn_modules::provider::get()?;

    if specifier != "std:types" && provider.interface_blob(specifier).is_some() {
        return Some(Carrier::Blob);
    }
    if let Some(source) = provider
        .embedded_source(specifier)
        .or_else(|| provider.bundled_source(specifier))
    {
        return Some(Carrier::Embedded(source));
    }
    provider
        .source_path(specifier)
        .map(|p| Carrier::File(p.to_string_lossy().into_owned()))
}
