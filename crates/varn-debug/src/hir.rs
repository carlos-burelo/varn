use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_core::ast::Program;
use varn_core::TypeAnnotations;

pub fn debug_hir(
    program: &Program,
    annotations: &TypeAnnotations,
    extension_calls: &FxHashMap<u32, Rc<str>>,
    extension_members: &FxHashMap<u32, Rc<str>>,
    extension_set_members: &FxHashMap<u32, Rc<str>>,
) {
    let input = varn_opt::OptInput {
        program,
        annotations,
        extension_calls,
        extension_members,
        extension_set_members,
        export_names: Vec::new(),
    };

    match varn_opt::lower_to_hir(input) {
        Ok(module) => {
            varn_opt::hir::dump::dump_module(&module, program.filename.as_ref());
        }
        Err(varn_opt::OptError::Unsupported(msg)) => {
            eprintln!("\n  \x1b[33mwarn\x1b[0m HIR lowering not fully supported for this file");
            eprintln!("  \x1b[2munsupported construct: {msg}\x1b[0m");
            eprintln!("  \x1b[2m(set VN_OPT=1 to see which module routes through varn-opt)\x1b[0m");
        }
    }
}
