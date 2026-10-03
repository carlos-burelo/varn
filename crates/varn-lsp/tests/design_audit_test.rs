//! Tests que fundamentan la auditoría de diseño (memoria + indexado).
//!
//! Cada test mide un hecho estructural sobre el código actual, no el
//! comportamiento deseado: si el diseño cambia, estos tests deben cambiar
//! con él. Referencian `docs` de arquitectura por hipótesis.

#![allow(unused_crate_dependencies)]

use std::collections::HashSet;
use std::mem::size_of;

use varn_checker::module_resolver::ImportResolver;
use varn_lsp::pipeline::run_pipeline;
use varn_lsp::workspace::Workspace;

const ACCOUNT_SRC: &str = r#"
class Account {
    balance: int;

    get_balance(): int {
        return this.balance;
    }

    deposit(amount: int) {
        this.balance = this.balance + amount;
    }
}

const acc = new Account();
acc.balance = 100;
const b = acc.balance;
"#;

/// H1 — los tokens no retienen lexemas: el texto vive solo en `source`.
///
/// `TokenRecord` guarda offsets de byte (`offset`/`end`); el lexema se
/// resuelve on-demand (`token_lexeme` / `DocumentState::lexeme`). Si alguien
/// reintroduce `lexeme: String`, este test no compila (campo inexistente) y
/// el assert de tamaño lo delata.
#[test]
fn h1_tokens_hold_no_lexeme_strings() {
    let uri = "file:///test/h1.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri);
    assert!(!state.tokens.is_empty(), "la muestra debe producir tokens");

    // Cero bytes de texto por token: 6×u32 (kind repr(u32) + 5 offsets),
    // sin `String` (24B c/u antes).
    assert_eq!(
        size_of::<varn_lsp::document::TokenRecord>(),
        6 * size_of::<u32>(),
        "TokenRecord = 6×u32, sin heap propio"
    );

    // Resolución exacta contra la fuente para cada token.
    for t in &state.tokens {
        let lex = state.lexeme(t);
        assert_eq!(lex.len(), (t.end - t.offset) as usize);
        assert!(!lex.is_empty());
        assert!(state.source.contains(lex));
    }
    eprintln!("tokens={} lexemes_resolved_on_demand", state.tokens.len());
}

/// H2 — un documento retiene UNA sola copia de arenas.
///
/// El dueño canónico es `db.bind` (`bind.arena` + `bind.scopes`). El clon
/// separado (`db.arena`/`db.scopes`) se eliminó: si existiera, este test no
/// compilaría al acceder solo vía `bind`.
#[test]
fn h2_document_keeps_single_arena_copy() {
    let uri = "file:///test/h2.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri);

    assert!(
        !state.db.bind.arena.all().is_empty(),
        "la muestra debe declarar símbolos"
    );
    // Toda id declarada resuelve en el arena único: no hay segunda tabla.
    for id in &state.symbols {
        assert!(
            *id < state.db.bind.arena.len(),
            "toda id debe vivir en db.bind.arena"
        );
        let _ = state.symbol(*id);
    }
    assert!(
        !state.db.bind.scopes.is_empty(),
        "debe haber scopes en el arena único"
    );
}

/// H3 — un solo mapa `Id → Type` (Ley 6).
///
/// `DocumentState.resolved_types` se eliminó: `SemanticDB.symbol_types` es la
/// única tabla, con la regla recorded-else-declared. El mapa esparso que el
/// checker devolvía no se fusiona porque `Checker::check` ya plegó su
/// finalize en `bind.arena` (`sym.ty`), que la regla consume. Si
/// `resolved_types` volviera, este test no compila: garantía estructural.
#[test]
fn h3_single_id_to_type_map() {
    let uri = "file:///test/h3.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri);

    let stored: HashSet<usize> = state.db.symbol_types.keys().copied().collect();
    assert!(!stored.is_empty(), "debe haber tipos resueltos");
    // Cobertura total del arena: cada símbolo declarado tiene su tipo en la
    // única tabla (el mapa esparso del checker cubría 2 de 73).
    assert_eq!(
        stored.len(),
        state.db.bind.arena.len(),
        "una tabla, cobertura total"
    );
    // Y cada vista de símbolo resuelve por ella.
    for id in &state.symbols {
        let _ = state.symbol(*id);
    }
    eprintln!(
        "symbols={} in_single_map={}",
        state.symbols.len(),
        stored.len()
    );
}

/// H4 — un solo `Arc<str>` de uri por archivo, compartido por sus N entradas.
///
/// Antes: `uri.to_owned()` por símbolo (medido N×len(uri) bytes: 448×26B en
/// la muestra). Ahora un alloc por archivo y refcount por entrada.
#[test]
fn h4_index_shares_one_uri_per_file() {
    let uri = "file:///test/h4_account.vn".to_string();
    let workspace = Workspace::new();
    workspace.index_file(uri.clone(), ACCOUNT_SRC.to_string());

    let idx = workspace.index.read().unwrap();
    let entries = idx
        .module_exports
        .get(&uri)
        .expect("el archivo debe quedar indexado");
    assert!(entries.len() > 1, "la muestra debe indexar varios símbolos");

    let first = &entries[0];
    assert!(
        entries
            .iter()
            .all(|e| std::sync::Arc::ptr_eq(&e.uri, &first.uri)),
        "todas las entradas comparten el mismo Arc<str>"
    );
    eprintln!("entries={} uri_allocs=1", entries.len());
}

/// H5 — `db.sources` sobrevive a `close_file` y `remove_file`.
///
/// Las fuentes (`Arc<str>`) y los uris del interner nunca se evictan:
/// retención efectiva tras cerrar/borrar.
#[test]
fn h5_sources_survive_close_and_remove() {
    let uri = "file:///test/h5.vn".to_string();
    let workspace = Workspace::new();
    workspace.update_file(uri.clone(), ACCOUNT_SRC.to_string());
    let fid = workspace.db.intern(&uri);
    assert!(workspace.db.get_source(fid).is_some());

    workspace.close_file(&uri);
    assert!(
        workspace.db.get_source(fid).is_some(),
        "hipótesis rota: close_file ya libera la fuente (mejora bienvenida, actualizar test)"
    );

    workspace.remove_file(&uri);
    assert!(
        workspace.db.get_source(fid).is_some(),
        "hipótesis rota: remove_file ya libera la fuente (mejora bienvenida, actualizar test)"
    );
}

/// H10 — `evict_heavy` suelta artefactos, conserva exports y re-deriva.
///
/// El grafo memoiza binds+programs+arenas de cada dependencia (el grueso de
/// la RSS tras indexar el repo). Todo lector los reconstruye en miss
/// (`module_bind`/`module_exports` reparsean + re-bindean, con caché en disco
/// primero), así que evictar cambia pico de memoria, no respuestas.
#[test]
fn h10_evict_heavy_keeps_exports_drops_artifacts() {
    use varn_lsp::workspace::resolver::with_resolver;

    let workspace = Workspace::new();
    workspace.index_file("file:///test/h10.vn".to_string(), ACCOUNT_SRC.to_string());

    let (b, p, a, e) = with_resolver(|r| r.graph_stats());
    eprintln!("graph before: binds={b} programs={p} arenas={a} exports={e}");
    assert!(b + p + a > 0, "indexar debe memoizar artefactos pesados");
    assert!(e > 0, "indexar debe memoizar exports");

    let evicted = with_resolver(|r| r.evict_heavy());
    assert_eq!(evicted, (b, p, a), "evict_heavy reporta lo que suelta");
    let (b2, p2, a2, e2) = with_resolver(|r| r.graph_stats());
    assert_eq!((b2, p2, a2), (0, 0, 0), "artefactos evictados");
    assert_eq!(e2, e, "exports sobreviven a la evicción");

    // El índice del proyecto sigue respondiendo sin los artefactos…
    {
        let idx = workspace.index.read().unwrap();
        assert!(
            !idx.definitions_of("Account").is_empty(),
            "exports indexados deben seguir visibles tras evictar"
        );
    }

    // …y el estado post-evict es estable: re-analizar lo ya exportado no
    // re-infla las tablas pesadas (el caché de exports responde sin bindear).
    workspace.index_file("file:///test/h10b.vn".to_string(), ACCOUNT_SRC.to_string());
    let (b3, p3, a3, e3) = with_resolver(|r| r.graph_stats());
    assert_eq!(
        (b3, p3, a3),
        (0, 0, 0),
        "re-analizar exports conocidos no debe re-bindear"
    );
    assert!(e3 >= e2, "los exports siguen acumulándose");
}

/// H10b — en miss real, `module_bind` re-deriva tras evictar.
///
/// Prueba la otra mitad del contrato: sin exports cacheados, el resolver
/// reconstruye desde fuente (o artefacto en disco) y vuelve a memoizar.
#[test]
fn h10b_module_bind_rederives_after_evict() {
    use varn_checker::module_resolver::{DiskResolver, ImportResolver};

    let dir = std::env::temp_dir().join(format!("varn-h10b-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rederive_marker.vn");
    std::fs::write(&path, "function rederived_marker(): int { return 7; }\n").unwrap();
    let key = std::fs::canonicalize(&path)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let resolver = DiskResolver::new();
    let first = resolver.module_bind(&key);
    assert!(
        first.is_some(),
        "module_bind debe resolver un archivo real: {key}"
    );
    let (b, _, _, _) = resolver.graph_stats();
    assert!(b > 0, "el bind debe quedar memoizado");

    let evicted = resolver.evict_heavy();
    assert!(evicted.0 > 0, "debía haber binds que soltar");
    assert_eq!(resolver.graph_stats().0, 0);

    let second = resolver.module_bind(&key);
    assert!(
        second.is_some(),
        "tras evictar, module_bind re-deriva desde fuente/caché"
    );
    assert!(
        resolver.graph_stats().0 > 0,
        "lo re-derivado vuelve a memoizarse"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// H7 — los tipos pequeños son `Copy`; los caros son `String` por entrada.
///
/// Fija el presupuesto de memoria por elemento: identidad y tipos cuestan
/// bytes, cada `String` dueña cuesta 24B + heap.
#[test]
fn h7_small_types_are_copy_sized() {
    fn is_copy<T: Copy>() {}
    is_copy::<varn_core::Atom>();
    is_copy::<varn_checker::Type>();

    assert_eq!(size_of::<varn_core::Atom>(), 4, "Atom = un u32");
    assert!(
        size_of::<varn_checker::Type>() <= 32,
        "Type = ids + flags, no heap propio"
    );
    assert!(
        size_of::<varn_core::Token>() <= 64,
        "Token = kind + rango + offsets, lexema zero-copy"
    );
    // Contrapartida: cada ExportEntry carga 4+ Strings dueñas.
    assert!(
        size_of::<varn_lsp::index::ExportEntry>() >= 4 * size_of::<String>(),
        "ExportEntry retiene name/global_key/uri/type_str como Strings"
    );
    assert_eq!(
        size_of::<varn_lsp::document::TokenRecord>(),
        6 * size_of::<u32>(),
        "TokenRecord = 6×u32, lexema on-demand (ver H1)"
    );
}

/// H8 — re-indexar no acumula entradas (guarda del fast-path O(N²)).
///
/// `update_file` sobre un módulo ya indexado debe evictar antes de insertar;
/// sobre uno nuevo puede insertar directo. En ambos casos, una sola
/// definición visible.
#[test]
fn h8_reindex_does_not_accumulate_entries() {
    let src = "function compute_something(): int { return 42; }";
    let uri = "file:///test/h8.vn".to_string();
    let workspace = Workspace::new();

    workspace.index_file(uri.clone(), src.to_string());
    workspace.update_file(uri.clone(), src.to_string());
    workspace.update_file(uri.clone(), src.to_string());

    let idx = workspace.index.read().unwrap();
    let defs = idx.definitions_of("compute_something");
    assert_eq!(
        defs.len(),
        1,
        "re-indexar debe reemplazar, no acumular: got {}",
        defs.len()
    );
}

/// H9 — identidad de miembros estructural, sin claves formateadas.
///
/// `ExportEntry.parent` + `name` identifican; el `global_key: String`
/// (`m:{uri}#{kind}:{name}` / `member:{parent}:{member}`) se eliminó junto a
/// `stable_global_key`, `SymbolView::global_key`, `symbol_global_key_for_id`
/// y `token_global_key` (este último ya no tenía lectores). El payload
/// opaco de call hierarchy viaja en `None`: `incoming/outgoing` resuelven
/// por `(uri, name)` y nunca leyeron `data`.
#[test]
fn h9_member_identity_is_structural() {
    use varn_lsp::features::call_hierarchy::prepare_call_hierarchy;

    let uri = "file:///test/h9.vn".to_string();
    let workspace = Workspace::new();
    workspace.index_file(uri.clone(), ACCOUNT_SRC.to_string());

    {
        let idx = workspace.index.read().unwrap();
        let balance = idx.definitions_of("balance");
        assert!(!balance.is_empty(), "balance debe estar indexado");
        assert!(
            balance
                .iter()
                .any(|e| e.parent.as_deref() == Some("Account")),
            "el miembro lleva su parent estructural, no renderizado en clave"
        );
        assert!(
            balance.iter().all(|e| e.name == "balance"),
            "el nombre sigue siendo la clave de búsqueda textual"
        );
    }

    // Goto-definition de miembro por vía estructural, end-to-end.
    // `acc.balance = 100;` vive en la línea 14 (0-based) de la muestra.
    workspace.update_file(uri.clone(), ACCOUNT_SRC.to_string());
    let doc = workspace.get(&uri).unwrap();
    let usage = doc
        .tokens
        .iter()
        .find(|t| doc.lexeme(t) == "balance" && t.line == 14)
        .expect("uso de acc.balance en la muestra");
    {
        let idx = workspace.index.read().unwrap();
        let loc = varn_lsp::features::definition::build_goto_definition(
            &doc,
            Some(&idx),
            usage.line,
            usage.col,
        );
        assert!(
            loc.is_some(),
            "goto-definition de miembro debe resolver sin global_key"
        );
    }

    // Sin claves en el payload de call hierarchy.
    let func = doc
        .tokens
        .iter()
        .find(|t| doc.lexeme(t) == "get_balance")
        .expect("método get_balance en la muestra");
    let items = prepare_call_hierarchy(&doc, func.line, func.col).expect("hierarchy del método");
    assert!(!items.is_empty());
    assert!(
        items.iter().all(|i| i.data.is_none()),
        "data viaja en None: nadie lo leía"
    );
}
