use std::rc::Rc;

use varn_core::{ImportSpecifier, ModuleId};
use varn_types::FunctionProto;
use varn_vm::loader::{ModuleError, ModuleLoader};



pub struct FileLoader;

impl ModuleLoader for FileLoader {
    fn resolve(&self, spec: &str, from: &ModuleId) -> Result<ModuleId, ModuleError> {
        match ImportSpecifier::parse(spec) {
            ImportSpecifier::Relative(_) | ImportSpecifier::Package(_) => {
                varn_modules::resolver::ModuleResolver::new()
                    .resolve(spec, from)
                    .map_err(ModuleError::new)
            }
            _ => Err(ModuleError::new(format!(
                "FileLoader cannot resolve non-local specifier: {spec}"
            ))),
        }
    }

    fn load(&self, id: &ModuleId) -> Result<Option<Rc<FunctionProto>>, ModuleError> {
        let path = match id {
            ModuleId::Local(p) => p.as_ref(),
            _ => return Ok(None),
        };
        let source = std::fs::read_to_string(path)
            .map_err(|e| ModuleError::new(format!("cannot read '{path}': {e}")))?;
        let proto = compile_source(&source, path)
            .map_err(|e| ModuleError::new(format!("compile error in '{path}': {e}")))?;
        Ok(Some(Rc::new(proto)))
    }

    fn native(&self, _id: &ModuleId) -> Option<varn_types::Value> {
        None
    }
}

pub struct StdlibLoader;

impl ModuleLoader for StdlibLoader {
    fn resolve(&self, specifier: &str, _from: &ModuleId) -> Result<ModuleId, ModuleError> {
        match ImportSpecifier::parse(specifier) {
            ImportSpecifier::Stdlib(s) => Ok(ModuleId::Std(s)),
            ImportSpecifier::Core(s) => Ok(ModuleId::Core(s)),
            ImportSpecifier::Runtime(s) => Ok(ModuleId::Runtime(s)),
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
            ModuleId::Std(s) | ModuleId::Core(s) => s.as_ref(),
            
            ModuleId::Runtime(_) => return Ok(None),
            _ => return Ok(None),
        };

        let loader = varn_builtins::CoreSourceLocator::from_env();
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
    let mut program =
        varn_parser::parse(tokens, lexeme_buf, path).map_err(|errs| errs[0].message.clone())?;
    varn_core::assign_ast_ids(&mut program);
    let check = varn_checker::Checker::check(&program);
    let exports = varn_checker::module_resolver::resolve_stdlib_module_exports_ref(path);
    let mut export_names: Vec<std::rc::Rc<str>> = exports
        .keys()
        .map(|k| std::rc::Rc::from(k.as_str()))
        .collect();
    export_names.sort();
    varn_opt::compile_module(
        &program,
        &check.type_annotations,
        &check.extension_calls,
        &check.extension_members,
        &check.extension_set_members,
        export_names,
    )
    .map_err(|e| e.to_string())
}
