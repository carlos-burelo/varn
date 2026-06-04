use varn_utilities::chalk::chalk;
use varn_utilities::terminal;
use varn_utilities::terminal::Section;
use varn_core::ast::{Decl, Program, StmtKind};

pub fn debug_modules(program: &Program) {
    Section::new("module linkage").subtitle(&program.filename).color(|c| c.cyan()).print();

    let mut imports = 0;
    let mut exports = 0;

    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            match &**decl {
                Decl::Import(i) => {
                    terminal::log(format!(
                        "  {} {} from {}",
                        chalk("import").bold(),
                        chalk(format!("{:?}", i.specifiers)).yellow(),
                        chalk(format!("{:?}", i.source)).blue()
                    ));
                    imports += 1;
                }
                Decl::Export(e) => {
                    terminal::log(format!("  {} {}", chalk("export").bold(), chalk(format!("{:?}", e)).cyan()));
                    exports += 1;
                }
                _ => {}
            }
        }
    }

    Section::new("module linkage")
        .subtitle(format!("{} import(s), {} export(s)", imports, exports))
        .close();
}
