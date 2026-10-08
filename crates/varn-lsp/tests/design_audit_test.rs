#![allow(unused_crate_dependencies)]

use std::collections::HashSet;
use std::mem::size_of;

use varn_lsp::pipeline::run_pipeline;
use varn_sem::resolver::ImportResolver;

fn test_resolver() -> std::sync::Arc<varn_resolver::DiskResolver> {
    std::sync::Arc::new(varn_resolver::DiskResolver::new())
}
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

#[test]
fn h1_tokens_hold_no_lexeme_strings() {
    let uri = "file:///test/h1.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri, test_resolver());
    assert!(!state.tokens.is_empty(), "la muestra debe producir tokens");

    assert_eq!(
        size_of::<varn_lsp::document::TokenRecord>(),
        6 * size_of::<u32>(),
        "TokenRecord = 6×u32, sin heap propio"
    );

    for t in &state.tokens {
        let lex = state.lexeme(t);
        assert_eq!(lex.len(), (t.end - t.offset) as usize);
        assert!(!lex.is_empty());
        assert!(state.source.contains(lex));
    }
    eprintln!("tokens={} lexemes_resolved_on_demand", state.tokens.len());
}

#[test]
fn h2_document_keeps_single_arena_copy() {
    let uri = "file:///test/h2.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri, test_resolver());

    assert!(
        !state.db.bind.arena.all().is_empty(),
        "la muestra debe declarar símbolos"
    );

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

#[test]
fn h3_single_id_to_type_map() {
    let uri = "file:///test/h3.vn".to_string();
    let state = run_pipeline(ACCOUNT_SRC.to_string(), uri, test_resolver());

    let stored: HashSet<usize> = state.db.symbol_types.keys().copied().collect();
    assert!(!stored.is_empty(), "debe haber tipos resueltos");

    assert_eq!(
        stored.len(),
        state.db.bind.arena.len(),
        "una tabla, cobertura total"
    );

    for id in &state.symbols {
        let _ = state.symbol(*id);
    }
    eprintln!(
        "symbols={} in_single_map={}",
        state.symbols.len(),
        stored.len()
    );
}

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

#[test]
fn h10_evict_heavy_keeps_exports_drops_artifacts() {
    let workspace = Workspace::new();
    workspace.index_file("file:///test/h10.vn".to_string(), ACCOUNT_SRC.to_string());

    let (b, p, a, e) = workspace.resolver().graph_stats();
    eprintln!("graph before: binds={b} programs={p} arenas={a} exports={e}");
    assert!(b + p + a > 0, "indexar debe memoizar artefactos pesados");
    assert!(e > 0, "indexar debe memoizar exports");

    let evicted = workspace.resolver().evict_heavy();
    assert_eq!(evicted, (b, p, a), "evict_heavy reporta lo que suelta");
    let (b2, p2, a2, e2) = workspace.resolver().graph_stats();
    assert_eq!((b2, p2, a2), (0, 0, 0), "artefactos evictados");
    assert_eq!(e2, e, "exports sobreviven a la evicción");

    {
        let idx = workspace.index.read().unwrap();
        assert!(
            !idx.definitions_of("Account").is_empty(),
            "exports indexados deben seguir visibles tras evictar"
        );
    }

    workspace.index_file("file:///test/h10b.vn".to_string(), ACCOUNT_SRC.to_string());
    let (b3, p3, a3, e3) = workspace.resolver().graph_stats();
    assert_eq!(
        (b3, p3, a3),
        (0, 0, 0),
        "re-analizar exports conocidos no debe re-bindear"
    );
    assert!(e3 >= e2, "los exports siguen acumulándose");
}

#[test]
fn h10b_module_bind_rederives_after_evict() {
    use varn_resolver::DiskResolver;
    use varn_sem::resolver::ImportResolver;

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

#[test]
fn h7_small_types_are_copy_sized() {
    fn is_copy<T: Copy>() {}
    is_copy::<varn_core::Atom>();
    is_copy::<varn_sem::types::Type>();

    assert_eq!(
        size_of::<varn_core::Atom>(),
        16,
        "Atom = hash XXH3-128 del texto"
    );
    assert!(
        size_of::<varn_sem::types::Type>() <= 32,
        "Type = ids + flags, no heap propio"
    );
    assert!(
        size_of::<varn_core::Token>() <= 64,
        "Token = kind + rango + offsets, lexema zero-copy"
    );

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
