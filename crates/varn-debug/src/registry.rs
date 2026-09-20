//! The static phase registry (DEBUG_PLAN §3.1).
//!
//! Single source of truth for accepted phase ids, aliases, titles, stages and
//! `-p all` membership. Adding a phase is one entry here (a real `Phase`
//! implementation once it moves into `phases/`); the parser and
//! `--list-phases` are derived, so they cannot drift.

use crate::phase::{PerModule, Phase, Stage};

/// A metadata-only phase. Phases that have not migrated to a real
/// implementation yet use this so the registry is complete and the parser can
/// be generated. `collect`/`render` are added to `Phase` in the next steps.
pub struct Info {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub title: &'static str,
    pub stage: Stage,
    pub per_module: PerModule,
    pub in_all: bool,
    pub groups: &'static [&'static str],
}

impl Phase for Info {
    fn id(&self) -> &'static str {
        self.id
    }
    fn aliases(&self) -> &'static [&'static str] {
        self.aliases
    }
    fn title(&self) -> &'static str {
        self.title
    }
    fn stage(&self) -> Stage {
        self.stage
    }
    fn per_module(&self) -> PerModule {
        self.per_module
    }
    fn in_all(&self) -> bool {
        self.in_all
    }
    fn groups(&self) -> &'static [&'static str] {
        self.groups
    }
}

static TOKENS: Info = Info {
    id: "tokens",
    aliases: &[],
    title: "flujo de tokens del lexer",
    stage: Stage::Lex,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static AST: Info = Info {
    id: "ast",
    aliases: &[],
    title: "árbol sintáctico",
    stage: Stage::Parse,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static MODULES: Info = Info {
    id: "modules",
    aliases: &[],
    title: "imports/exports del módulo",
    stage: Stage::Parse,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static SYMBOLS: Info = Info {
    id: "symbols",
    aliases: &[],
    title: "tabla de símbolos con tipos inferidos",
    stage: Stage::Check,
    per_module: PerModule::No,
    in_all: true,
    groups: &["check"],
};
static CHECK_TYPES: Info = Info {
    id: "check:types",
    aliases: &[],
    title: "volcado diffeable: tabla de tipos + anotaciones",
    stage: Stage::Check,
    per_module: PerModule::No,
    in_all: false,
    groups: &[],
};
static BYTECODE: Info = Info {
    id: "bytecode",
    aliases: &[],
    title: "bytecode por función",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static SCOPE: Info = Info {
    id: "scope",
    aliases: &[],
    title: "árbol de scopes",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static CAPS: Info = Info {
    id: "caps",
    aliases: &["cap-trace", "cap"],
    title: "traza de capabilities",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static GRAPH: Info = Info {
    id: "graph",
    aliases: &[],
    title: "grafo de dependencias de módulos",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static SUMMARY: Info = Info {
    id: "summary",
    aliases: &[],
    title: "tamaños, exports y top-10 de funciones",
    stage: Stage::Compile,
    per_module: PerModule::Graph,
    in_all: true,
    groups: &[],
};
static TYPELOSS: Info = Info {
    id: "typeloss",
    aliases: &[],
    title: "dónde deja de ser estático: opcodes genéricos con equivalente tipado",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: false,
    groups: &[],
};
static TIR: Info = Info {
    id: "tir",
    aliases: &[],
    title: "IR tipado que emite el checker",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: false,
    groups: &[],
};
static TIERS: Info = Info {
    id: "tiers",
    aliases: &[],
    title: "tier por función: clif / gate / bail",
    stage: Stage::Compile,
    per_module: PerModule::Graph,
    in_all: true,
    groups: &[],
};
static BAILS: Info = Info {
    id: "bails",
    aliases: &[],
    title: "solo lo que no rutea, agrupado por causa",
    stage: Stage::Compile,
    per_module: PerModule::Graph,
    in_all: true,
    groups: &[],
};
static ROOTS: Info = Info {
    id: "roots",
    aliases: &[],
    title: "conjunto de raíces GC por safepoint",
    stage: Stage::Compile,
    per_module: PerModule::Graph,
    in_all: false,
    groups: &[],
};
static CLIF: Info = Info {
    id: "clif",
    aliases: &[],
    title: "lowering Cranelift (route, kinds, ir, asm, check)",
    stage: Stage::Compile,
    per_module: PerModule::No,
    in_all: true,
    groups: &[],
};
static GC: Info = Info {
    id: "gc",
    aliases: &[],
    title: "nursery/old-gen/interners al terminar de correr",
    stage: Stage::Exec,
    per_module: PerModule::No,
    in_all: false,
    groups: &[],
};

/// Every registered phase, in `Stage` order.
pub static ALL: &[&dyn Phase] = &[
    &TOKENS,
    &AST,
    &MODULES,
    &SYMBOLS,
    &CHECK_TYPES,
    &BYTECODE,
    &SCOPE,
    &CAPS,
    &GRAPH,
    &SUMMARY,
    &TYPELOSS,
    &TIR,
    &TIERS,
    &BAILS,
    &ROOTS,
    &CLIF,
    &GC,
];

/// Find a phase by canonical id or alias.
pub fn lookup(name: &str) -> Option<&'static dyn Phase> {
    ALL.iter()
        .copied()
        .find(|p| p.id() == name || p.aliases().contains(&name))
}

/// Phases that are members of `-p all`.
pub fn in_all() -> impl Iterator<Item = &'static dyn Phase> {
    ALL.iter().copied().filter(|p| p.in_all())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for p in ALL {
            assert!(seen.insert(p.id()), "duplicate phase id {}", p.id());
        }
    }

    #[test]
    fn aliases_do_not_collide_with_ids_or_each_other() {
        let ids: HashSet<&str> = ALL.iter().map(|p| p.id()).collect();
        let mut seen: HashSet<&str> = HashSet::new();
        for p in ALL {
            for a in p.aliases() {
                assert!(!ids.contains(a), "alias '{a}' shadows a phase id");
                assert!(seen.insert(a), "alias '{a}' declared twice");
            }
        }
    }

    #[test]
    fn titles_are_non_empty() {
        for p in ALL {
            assert!(!p.title().is_empty(), "{} has no title", p.id());
        }
    }

    #[test]
    fn lookup_resolves_ids_and_aliases() {
        assert_eq!(lookup("tokens").unwrap().id(), "tokens");
        assert_eq!(lookup("cap").unwrap().id(), "caps");
        assert_eq!(lookup("cap-trace").unwrap().id(), "caps");
        assert!(lookup("definitely-not-a-phase").is_none());
    }

    #[test]
    fn all_membership_excludes_sweep_and_exec_phases() {
        let in_all: HashSet<&str> = in_all().map(|p| p.id()).collect();
        for excluded in ["tir", "check:types", "roots", "typeloss", "gc"] {
            assert!(!in_all.contains(excluded), "{excluded} must not be in all");
        }
        for included in ["tokens", "ast", "bytecode", "symbols", "summary", "clif"] {
            assert!(in_all.contains(included), "{included} must be in all");
        }
    }
}
