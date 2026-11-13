use crate::package::{stmt_types, InlinePackage, ModuleTemplates};
use crate::walk::stmts_mut;
use crate::{
    BackendTy, FnId, Resolution, SigId, Signature, Span, TirExpr, TirExprKind, TirFunction,
    TirImportKind, TirModule, TyTable,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

pub trait TemplateSource {
    fn templates(&self, import_source: &str) -> Option<Arc<ModuleTemplates>>;
}

#[derive(Clone)]
struct Installed {
    id: FnId,
    slot: u32,
    sig: SigId,
}

struct Linker {
    base: u32,
    appended: Vec<TirFunction>,
    installed: FxHashMap<(Arc<str>, Arc<str>), Installed>,
}

impl Linker {
    fn install(
        &mut self,
        module: &mut TirModule,
        source: &Arc<str>,
        package: &InlinePackage,
        span: Span,
    ) -> Option<Installed> {
        let from = TyTable::from_portable(package.types.clone());
        let tag = vn_core::fingerprint::fingerprint(source.as_bytes()) & 0xff_ffff;
        let mut local: Vec<Installed> = Vec::with_capacity(package.functions.len());
        for original in &package.functions {
            let key = (source.clone(), original.name.clone());
            if let Some(done) = self.installed.get(&key) {
                local.push(done.clone());
                continue;
            }
            let mut function = original.clone();
            let mut ok = true;
            for ty in function.params.iter_mut().chain(function.locals.iter_mut()) {
                ok &= remap(&mut module.types, &from, ty);
            }
            ok &= remap(&mut module.types, &from, &mut function.return_ty);
            let mut body = std::mem::take(&mut function.body);
            stmt_types(&mut body, &mut |ty| {
                ok &= remap(&mut module.types, &from, ty)
            });
            stmts_mut(&mut body, &mut |expr| {
                expr.span = span;
                ok &= remap(&mut module.types, &from, &mut expr.ty);
                if let TirExprKind::Seq { stmts, .. } = &mut expr.kind {
                    stmt_types(stmts, &mut |ty| ok &= remap(&mut module.types, &from, ty));
                }
            });
            stmts_mut(&mut body, &mut |expr| {
                if let (TirExprKind::Call { callee, .. }, Resolution::DirectFn(target)) =
                    (&mut expr.kind, expr.res.clone())
                {
                    match local.get(target.0 as usize) {
                        Some(dep) => {
                            expr.res = Resolution::DirectFn(dep.id);
                            **callee = TirExpr {
                                kind: TirExprKind::Var,
                                ty: BackendTy::Fn(dep.sig),
                                res: Resolution::GlobalSlot(dep.slot),
                                span,
                            };
                        }
                        None => ok = false,
                    }
                }
            });
            function.body = body;
            if !ok {
                return None;
            }
            let name: Arc<str> = Arc::from(format!("__inl{tag:06x}::{}", original.name));
            module.signatures.push(Signature {
                params: function.params.clone(),
                return_ty: function.return_ty,
                has_rest: false,
            });
            let sig = SigId(module.signatures.len() as u32 - 1);
            function.sig = sig;
            function.name = name.clone();
            let slot = module.globals.len() as u32;
            module.globals.push(BackendTy::Fn(sig));
            module.global_names.push(name);
            let id = FnId(self.base + self.appended.len() as u32);
            self.appended.push(function);
            let installed = Installed { id, slot, sig };
            self.installed.insert(key, installed.clone());
            local.push(installed);
        }
        local.get(package.entry as usize).cloned()
    }
}

fn remap(to: &mut TyTable, from: &TyTable, ty: &mut BackendTy) -> bool {
    match to.import_ty(from, *ty) {
        Some(imported) => {
            *ty = imported;
            true
        }
        None => false,
    }
}

pub fn link(module: &mut TirModule, source: &dyn TemplateSource) {
    let mut linked: FxHashMap<u32, (Arc<str>, Arc<InlinePackage>)> = FxHashMap::default();
    let mut constants: FxHashMap<u32, TirExpr> = FxHashMap::default();
    let mut inert_imports: Vec<usize> = Vec::new();
    for (index, import) in module.imports.iter().enumerate() {
        if import.is_type_only {
            continue;
        }
        let Some(templates) = source.templates(&import.source) else {
            continue;
        };
        let mut all = !import.specs.is_empty();
        for spec in &import.specs {
            let (package, constant) = match &spec.kind {
                TirImportKind::Named(name) => (templates.package(name), templates.constant(name)),
                TirImportKind::Default | TirImportKind::Namespace => (None, None),
            };
            let slot = module.global_names.iter().position(|n| *n == spec.local);
            match (package, constant, slot) {
                (Some(package), _, Some(slot)) => {
                    linked.insert(slot as u32, (import.source.clone(), package.clone()));
                }
                (None, Some(literal), Some(slot)) => {
                    constants.insert(slot as u32, literal.clone());
                }
                _ => all = false,
            }
        }
        if all && templates.inert {
            inert_imports.push(index);
        }
    }
    if linked.is_empty() && constants.is_empty() {
        return;
    }

    let mut functions = std::mem::take(&mut module.functions);
    let mut top_level = std::mem::replace(&mut module.top_level, empty_function());
    let mut class_defs = std::mem::take(&mut module.class_defs);
    let mut linker = Linker {
        base: functions.len() as u32,
        appended: Vec::new(),
        installed: FxHashMap::default(),
    };
    {
        let mut rewrite = |expr: &mut TirExpr| {
            let span = expr.span;
            if let (TirExprKind::Var, Resolution::GlobalSlot(slot)) = (&expr.kind, &expr.res)
                && let Some(literal) = constants.get(slot)
                && literal.ty.representation() == expr.ty.representation()
            {
                expr.kind = literal.kind.clone();
                expr.res = Resolution::None;
                return;
            }
            let TirExprKind::Call { callee, .. } = &mut expr.kind else {
                return;
            };
            let Resolution::GlobalSlot(slot) = callee.res else {
                return;
            };
            if !matches!(expr.res, Resolution::None) {
                return;
            }
            let Some((import_source, package)) = linked.get(&slot) else {
                return;
            };
            if let Some(done) = linker.install(module, import_source, package, span) {
                expr.res = Resolution::DirectFn(done.id);
                **callee = TirExpr {
                    kind: TirExprKind::Var,
                    ty: BackendTy::Fn(done.sig),
                    res: Resolution::GlobalSlot(done.slot),
                    span,
                };
            }
        };
        for function in functions.iter_mut() {
            crate::walk::function_exprs_mut(function, &mut rewrite);
        }
        crate::walk::function_exprs_mut(&mut top_level, &mut rewrite);
        for def in class_defs.iter_mut() {
            crate::walk::class_def_exprs_mut(def, &mut rewrite);
        }
    }
    functions.append(&mut linker.appended);

    let mut referenced: FxHashSet<u32> = FxHashSet::default();
    {
        let mut note = |expr: &mut TirExpr| {
            if let Resolution::GlobalSlot(slot) = expr.res {
                referenced.insert(slot);
            }
        };
        for function in functions.iter_mut() {
            crate::walk::function_exprs_mut(function, &mut note);
        }
        crate::walk::function_exprs_mut(&mut top_level, &mut note);
        for def in class_defs.iter_mut() {
            crate::walk::class_def_exprs_mut(def, &mut note);
        }
    }
    module.functions = functions;
    module.top_level = top_level;
    module.class_defs = class_defs;

    let exported: FxHashSet<Arc<str>> = module.exports.iter().map(|e| e.local.clone()).collect();
    let removable: Vec<usize> = inert_imports
        .into_iter()
        .filter(|&index| {
            module.imports[index].specs.iter().all(|spec| {
                !exported.contains(&spec.local)
                    && module
                        .global_names
                        .iter()
                        .position(|n| *n == spec.local)
                        .is_some_and(|slot| !referenced.contains(&(slot as u32)))
            })
        })
        .collect();
    for index in removable.into_iter().rev() {
        module.imports.remove(index);
    }
}

fn empty_function() -> TirFunction {
    TirFunction {
        name: Arc::from(""),
        sig: SigId(0),
        params: Vec::new(),
        return_ty: BackendTy::Void,
        locals: Vec::new(),
        body: Vec::new(),
        has_this: false,
        this_class: None,
        is_async: false,
        is_generator: false,
        has_rest: false,
        force_inline: false,
    }
}
