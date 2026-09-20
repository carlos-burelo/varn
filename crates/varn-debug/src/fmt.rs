//! Output format for debug phases (DEBUG_PLAN §3.4).

/// How a phase renders its [`crate::report::Report`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// Byte-for-byte the historical `vn debug` output: ANSI colors, box
    /// drawing, `eprintln!` layout. The default; frozen by the golden tests.
    #[default]
    Plain,
    /// Diffable, colorless, one record per line with `|`-separated fields,
    /// deterministic order, basename paths. Generalizes the `check:types`
    /// contract to every phase.
    Text,
}
