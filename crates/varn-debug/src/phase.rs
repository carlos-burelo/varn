//! Phase trait and the pipeline stages it runs in (DEBUG_PLAN §3.1).
//!
//! Each phase declares only metadata here; `collect`/`render` are added in the
//! migration steps as phases move into `phases/`. Metadata alone already kills
//! the "5 sites per new phase" boilerplate: `-p` parsing, `--list-phases` and
//! the group aliases are generated from [`crate::registry::ALL`].

/// The compiler stage a phase reads its input from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Lex,
    Parse,
    Check,
    Compile,
    Exec,
}

/// Whether a phase runs once, or once per module in the dependency graph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerModule {
    No,
    /// Iterate `graph`'s modules by path (sorted), one run per module.
    Graph,
}

/// One debug phase. Implementors are registered in
/// [`crate::registry::ALL`]; everything else (parser, listing, groups) is
/// derived from that table.
pub trait Phase: Sync {
    /// Canonical id, e.g. `"bytecode"` or `"check:types"`.
    fn id(&self) -> &'static str;

    /// Alternate spellings accepted by `-p`, e.g. `caps => ["cap-trace", "cap"]`.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// One-line description; single source for `--list-phases`.
    fn title(&self) -> &'static str;

    fn stage(&self) -> Stage;

    fn per_module(&self) -> PerModule {
        PerModule::No
    }

    /// `false` for sweep/exec-only phases that are members of views but not of
    /// `-p all` (tir, tir:check, check:types, roots, typeloss, gc, clif:check).
    fn in_all(&self) -> bool {
        true
    }

    /// Group memberships that `-p <group>` expands (e.g. `"check"`).
    fn groups(&self) -> &'static [&'static str] {
        &[]
    }
}
