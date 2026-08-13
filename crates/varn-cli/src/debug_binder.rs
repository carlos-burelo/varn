#![allow(unused_crate_dependencies)] // dev-tool bin: uses a slice of the crate deps

use std::fs::read_to_string;

fn main() {
    const STDLIB_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/std.vnb"));
    varn_builtins::register_embedded_stdlib(STDLIB_BYTES);
    varn_builtins::register_provider();

    let filename = "tests/47-isolates-multithread.vn";
    let source = read_to_string(filename).expect("Cannot read test file");
    let (tokens, lexeme_buf, lex_errs) = varn_lexer::scan(&source, filename);
    println!("Lex errors: {:?}", lex_errs);
    let program = varn_parser::parse(tokens, lexeme_buf, filename).expect("Parse error");

    let bind = varn_checker::Binder::bind(&program);
    println!("Bind diagnostics count: {}", bind.diagnostics.len());
    for d in bind.diagnostics.iter() {
        println!("  - {:?}", d);
    }

    println!("\n=== BINDER SCOPES ===");
    let arena = &bind.scopes;
    print_scope_tree(0, arena, &bind.arena, 0);
}

fn print_scope_tree(
    id: usize,
    arena: &varn_checker::scope::ScopeArena,
    symbol_arena: &varn_checker::symbol::SymbolArena,
    indent: usize,
) {
    let scope = arena.get(id);
    let indent_str = "  ".repeat(indent);
    println!("{}[Scope {}] Kind: {:?}", indent_str, id, scope.kind);
    for (name, sym_id) in &scope.bindings {
        let sym = symbol_arena.get(*sym_id);
        println!(
            "{}  - Binding: {} (SymbolId: {}, Kind: {:?}, Type: {:?})",
            indent_str, name, sym_id, sym.kind, sym.ty
        );
    }
    for &child_id in &scope.children {
        print_scope_tree(child_id, arena, symbol_arena, indent + 1);
    }
}
