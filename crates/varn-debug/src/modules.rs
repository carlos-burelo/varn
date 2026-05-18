use crate::colors::{BLUE, BOLD, CYAN, C_MODULES, RESET, YELLOW};
use varn_core::ast::{Decl, Program, StmtKind};

pub fn debug_modules(program: &Program) {
    use crate::colors::{footer, header};
    header(C_MODULES, "module linkage", &program.filename);

    let mut imports = 0;
    let mut exports = 0;

    for stmt in &program.body {
        if let StmtKind::Decl(decl) = &stmt.kind {
            match &**decl {
                Decl::Import(i) => {
                    eprintln!(
                        "  {BOLD}import{RESET} {YELLOW}{:?}{RESET} from {BLUE}{:?}{RESET}",
                        i.specifiers, i.source
                    );
                    imports += 1;
                }
                Decl::Export(e) => {
                    eprintln!("  {BOLD}export{RESET} {CYAN}{:?}{RESET}", e);
                    exports += 1;
                }
                _ => {}
            }
        }
    }

    footer(
        C_MODULES,
        &format!("{} import(s), {} export(s)", imports, exports),
    );
}
