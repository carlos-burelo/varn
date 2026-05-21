use std::rc::Rc;

use varn_core::{ImportSpecifier, ModuleId};
use varn_types::FunctionProto;
use varn_vm::loader::{ModuleError, ModuleLoader};

pub struct StdlibLoader;

impl ModuleLoader for StdlibLoader {
    fn resolve(&self, specifier: &str, _from: &ModuleId) -> Result<ModuleId, ModuleError> {
        match ImportSpecifier::parse(specifier) {
            ImportSpecifier::Stdlib(s) => Ok(ModuleId::Stdlib(s)),
            _ => Err(ModuleError::new(format!(
                "StdlibLoader cannot resolve non-stdlib specifier: {specifier}"
            ))),
        }
    }

    fn native(&self, _id: &ModuleId) -> Option<varn_types::Value> {
        None
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        let spec = match id {
            ModuleId::Stdlib(s) => s.as_ref(),
            _ => return Ok(None),
        };

        let loader = varn_builtins::ModuleLoader::from_env();
        let source = loader
            .embedded_source(spec)
            .map(|s| s.to_owned())
            .or_else(|| {
                loader
                    .vn_source_path(spec)
                    .and_then(|p| std::fs::read_to_string(p).ok())
            })
            .ok_or_else(|| ModuleError::new(format!("stdlib source not found: {spec}")))?;

        let proto = compile_source(&source, spec)
            .map_err(|e| ModuleError::new(format!("stdlib compile error in {spec}: {e}")))?;

        Ok(Some(Rc::new(proto)))
    }
}

fn compile_source(source: &str, path: &str) -> Result<FunctionProto, String> {
    let (tokens, lexeme_buf, _) = varn_lexer::scan(source, path);
    let mut program = varn_parser::parse(tokens, lexeme_buf, path).map_err(|errs| errs[0].message.clone())?;
    varn_core::assign_ast_ids(&mut program);
    let check = varn_checker::Checker::check(&program);
    varn_compiler::compile_with_check_result(
        &program,
        &check.type_annotations,
        &check.extension_calls,
        &check.extension_members,
        &check.extension_set_members,
    )
    .map_err(|e| e.to_string())
}
