#![allow(unused_crate_dependencies)]

use std::fs::read_to_string;

fn main() {
    varn_builtins::register_provider();

    let filename = "tests/47-isolates-multithread.vn";
    let source = read_to_string(filename).expect("Cannot read test file");
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(&source, filename);
    println!("Lex errors: {:?}", lex_errs);

    let (program, interner, ast_arena) =
        varn_parser::parse(tokens, lexeme_buf, filename, varn_core::AtomInterner::new())
            .expect("Parse error");

    let resolver = varn_resolver::DiskResolver::new();
    let bind = varn_binder::Binder::bind(&program, &ast_arena, interner, &resolver);
    println!("Bind diagnostics count: {}", bind.diagnostics.len());
    for d in bind.diagnostics.iter() {
        println!("  - {:?}", d);
    }

    println!("\n=== BINDER SCOPES ===");
    print_scope_tree(0, &bind.scopes, &bind.arena, &bind.interner, 0);
}

fn print_scope_tree(
    id: usize,
    arena: &varn_sem::scope::ScopeArena,
    symbol_arena: &varn_sem::symbol::SymbolArena,
    interner: &varn_core::AtomInterner,
    indent: usize,
) {
    let scope = arena.get(id);
    let indent_str = "  ".repeat(indent);
    println!("{}[Scope {}] Kind: {:?}", indent_str, id, scope.kind);
    for (name, sym_id) in &scope.bindings {
        let sym = symbol_arena.get(*sym_id);
        println!(
            "{}  - Binding: {} (SymbolId: {}, Kind: {:?}, Type: {:?})",
            indent_str,
            interner.try_resolve(*name).unwrap_or("?"),
            sym_id,
            sym.kind,
            sym.ty
        );
    }
    for &child_id in &scope.children {
        print_scope_tree(child_id, arena, symbol_arena, interner, indent + 1);
    }
}
