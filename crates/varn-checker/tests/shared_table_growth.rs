//! Integración: muchos módulos publicando `Atom`s/`CheckerTyId`s nuevos al
//! `DiskResolver` compartido deben seguir resolviendo correctamente incluso
//! cuando el número de símbolos nuevos cruza el umbral de congelación interno
//! de `AtomInterner`/`CheckerTyTable` (`docs/superpowers/plans/
//! 2026-09-25-shared-atom-type-tables-base-delta.md`, Tasks 1-2). Antes de ese
//! cambio no había umbral que cruzar; este test fija el comportamiento para
//! que una futura tabla que no lo tenga en cuenta no regresione en silencio.

use std::fs;
use std::path::PathBuf;
use varn_checker::module_resolver::{DiskResolver, ImportResolver};

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "varn-shared-table-growth-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Enough distinct modules, each with enough distinct symbol names, to push
/// the shared interner well past its internal freeze threshold (2048) more
/// than once. Every module's symbols must still resolve to their own text
/// through the resolver's live snapshot after all of them bind.
#[test]
fn many_modules_binding_cross_freeze_threshold_still_resolve_correctly() {
    let dir = scratch_dir("many-modules");
    let resolver = DiskResolver::new();

    const MODULE_COUNT: usize = 30;
    const SYMBOLS_PER_MODULE: usize = 200; // 30 * 200 = 6000 > 2 * FREEZE_THRESHOLD

    let mut module_paths = Vec::new();
    for m in 0..MODULE_COUNT {
        let mut src = String::new();
        for s in 0..SYMBOLS_PER_MODULE {
            src.push_str(&format!(
                "export let sym_{m}_{s}: int = {s}\n",
                m = m,
                s = s
            ));
        }
        let path = dir.join(format!("mod{m}.vn"));
        fs::write(&path, src).expect("write module source");
        module_paths.push(path.to_string_lossy().into_owned());
    }

    for path in &module_paths {
        let bind = resolver
            .module_bind(path)
            .unwrap_or_else(|| panic!("module {path} must bind"));
        assert!(
            !bind.diagnostics.has_errors(),
            "module {path} should bind cleanly: {:?}",
            bind.diagnostics.errors().collect::<Vec<_>>()
        );
    }

    // Every symbol from every module must still resolve correctly through
    // the resolver's live, shared interner snapshot — including symbols
    // whose `Atom` was minted before a freeze and symbols minted after one.
    let live_interner = resolver.interner_snapshot();
    for m in 0..MODULE_COUNT {
        for s in 0..SYMBOLS_PER_MODULE {
            let name = format!("sym_{m}_{s}");
            let atom = live_interner
                .get(&name)
                .unwrap_or_else(|| panic!("{name} must be in the live interner"));
            assert_eq!(live_interner.resolve(atom), name);
        }
    }

    let _ = fs::remove_dir_all(&dir);
}
