fn main() {
    let source = std::fs::read_to_string(
        "crates/varn-builtins/src/modules/std/collections/collections.vn"
    ).unwrap();
    let (tokens, lexeme_buf, _) = varn_lexer::scan(&source, "test");
    let program = varn_parser::parse(tokens, lexeme_buf, "test").unwrap();
    for stmt in &program.body {
        use varn_core::ast::{StmtKind, Decl, ExportDecl, ClassMember};
        if let StmtKind::Decl(decl) = &stmt.kind {
            if let Decl::Export(ExportDecl::Decl { declaration, .. }) = decl {
                if let Decl::Class(c) = declaration.as_ref() {
                    if c.id.as_deref() == Some("Record") {
                        println!("Record class body has {} members:", c.body.len());
                        for m in &c.body {
                            match m {
                                ClassMember::Method { key, .. } =>
                                    println!("  Method: {key}"),
                                ClassMember::Constructor { .. } =>
                                    println!("  Constructor"),
                                ClassMember::Getter { key, .. } =>
                                    println!("  Getter: {key}"),
                                ClassMember::Setter { key, .. } =>
                                    println!("  Setter: {key}"),
                                ClassMember::Property { key, .. } =>
                                    println!("  Property: {key}"),
                                ClassMember::StaticBlock { .. } =>
                                    println!("  StaticBlock"),
                                ClassMember::Destructor { .. } =>
                                    println!("  Destructor"),
                            }
                        }
                    }
                }
            }
        }
    }
}
