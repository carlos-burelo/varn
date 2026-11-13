use crate::inlinable::body_inlinable;
use crate::walk::stmts_mut;
use crate::{
    BackendTy, FnId, PortableTable, Resolution, Span, TirExpr, TirExprKind, TirFunction, TirModule,
    TirStmt, TyTable,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct InlinePackage {
    pub functions: Vec<TirFunction>,
    pub entry: u32,
    pub types: PortableTable,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModuleTemplates {
    pub exports: Vec<(Arc<str>, Arc<InlinePackage>)>,
    pub inert: bool,
}

impl ModuleTemplates {
    pub fn package(&self, export: &str) -> Option<&Arc<InlinePackage>> {
        self.exports
            .iter()
            .find(|(name, _)| name.as_ref() == export)
            .map(|(_, package)| package)
    }
}

pub fn extract(module: &TirModule) -> ModuleTemplates {
    let mut exports = Vec::new();
    for export in &module.exports {
        if export.reexport_from.is_some() || export.namespace {
            continue;
        }
        let Some(id) = free_function(module, &export.local) else {
            continue;
        };
        let mut extractor = Extractor {
            module,
            types: TyTable::default(),
            functions: Vec::new(),
            done: FxHashMap::default(),
            active: Vec::new(),
        };
        if let Some(entry) = extractor.include(id) {
            exports.push((
                export.exported.clone(),
                Arc::new(InlinePackage {
                    functions: extractor.functions,
                    entry,
                    types: extractor.types.to_portable(),
                }),
            ));
        }
    }
    ModuleTemplates {
        exports,
        inert: is_inert(module),
    }
}

fn is_inert(module: &TirModule) -> bool {
    module.top_level.body.is_empty()
        && !module.top_level.is_async
        && module.class_defs.is_empty()
        && module.exports.iter().all(|e| e.reexport_from.is_none())
        && module
            .imports
            .iter()
            .all(|import| import.is_type_only || !import.source.starts_with('.'))
}

fn free_function(module: &TirModule, name: &str) -> Option<FnId> {
    let mut found = module
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.name.as_ref() == name && !f.has_this);
    let (index, _) = found.next()?;
    found.next().is_none().then_some(FnId(index as u32))
}

struct Extractor<'m> {
    module: &'m TirModule,
    types: TyTable,
    functions: Vec<TirFunction>,
    done: FxHashMap<FnId, u32>,
    active: Vec<FnId>,
}

impl Extractor<'_> {
    fn include(&mut self, id: FnId) -> Option<u32> {
        if let Some(&index) = self.done.get(&id) {
            return Some(index);
        }
        if self.active.contains(&id) {
            return None;
        }
        let source = self.module.function(id)?;
        if source.has_this
            || source.this_class.is_some()
            || source.force_inline
            || !body_inlinable(source, false)
        {
            return None;
        }
        let mut function = source.clone();
        self.active.push(id);
        let portable = self.portable(&mut function);
        self.active.pop();
        if !portable {
            return None;
        }
        let index = self.functions.len() as u32;
        self.functions.push(function);
        self.done.insert(id, index);
        Some(index)
    }

    fn ty(&mut self, ty: &mut BackendTy) -> bool {
        match self.types.import_ty(&self.module.types, *ty) {
            Some(imported) => {
                *ty = imported;
                true
            }
            None => false,
        }
    }

    fn portable(&mut self, function: &mut TirFunction) -> bool {
        let mut ok = true;
        function.sig = crate::SigId(0);
        for ty in function.params.iter_mut().chain(function.locals.iter_mut()) {
            ok &= self.ty(ty);
        }
        ok &= self.ty(&mut function.return_ty);
        let mut body = std::mem::take(&mut function.body);
        stmt_types(&mut body, &mut |ty| ok &= self.ty(ty));
        stmts_mut(&mut body, &mut |expr| ok &= self.expr(expr));
        function.body = body;
        ok
    }

    fn expr(&mut self, expr: &mut TirExpr) -> bool {
        expr.span = Span::EMPTY;
        if !self.ty(&mut expr.ty) {
            return false;
        }
        match &mut expr.kind {
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::DecimalLit(_)
            | TirExprKind::BigIntLit(_)
            | TirExprKind::Var
            | TirExprKind::Binary { .. }
            | TirExprKind::Unary { .. }
            | TirExprKind::Index { .. }
            | TirExprKind::Assign { .. }
            | TirExprKind::ArrayLit(_)
            | TirExprKind::TupleLit(_)
            | TirExprKind::Cast { .. }
            | TirExprKind::Checked { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::Length { .. }
            | TirExprKind::RangeLit { .. }
            | TirExprKind::Field { .. }
            | TirExprKind::MethodCall { .. } => {}
            TirExprKind::Call { callee, args } => match expr.res {
                Resolution::DirectFn(target) => {
                    let Some(index) = self.include(target) else {
                        return false;
                    };
                    expr.res = Resolution::DirectFn(FnId(index));
                    **callee = placeholder(Resolution::None);
                }
                Resolution::NativeOp(_) | Resolution::NativeDecode { .. } => {
                    if args.is_empty() {
                        return false;
                    }
                    **callee = placeholder(Resolution::None);
                }
                Resolution::Intrinsic(_) => **callee = placeholder(Resolution::None),
                Resolution::None
                | Resolution::Local(_)
                | Resolution::Param(_)
                | Resolution::Upvalue(_)
                | Resolution::GlobalSlot(_)
                | Resolution::NativeGlobal(_)
                | Resolution::ModuleSlot { .. }
                | Resolution::FieldSlot(_)
                | Resolution::StaticField(_)
                | Resolution::VtableSlot(_)
                | Resolution::EnumVariant { .. }
                | Resolution::ByName { .. } => {}
            },
            TirExprKind::Seq { stmts, .. } => {
                let mut ok = true;
                stmt_types(stmts, &mut |ty| ok &= self.ty(ty));
                if !ok {
                    return false;
                }
            }
            TirExprKind::ObjectLit { .. }
            | TirExprKind::Await { .. }
            | TirExprKind::Yield { .. }
            | TirExprKind::Discriminant { .. }
            | TirExprKind::VariantPayload { .. }
            | TirExprKind::TypeTest { .. }
            | TirExprKind::Closure { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::ObjectKeys { .. }
            | TirExprKind::SuperCall { .. }
            | TirExprKind::SuperMethodCall { .. }
            | TirExprKind::ObjectRest { .. }
            | TirExprKind::ExtensionCall { .. } => return false,
        }
        match &expr.res {
            Resolution::None
            | Resolution::Local(_)
            | Resolution::Param(_)
            | Resolution::Intrinsic(_)
            | Resolution::NativeOp(_)
            | Resolution::NativeDecode { .. } => true,
            Resolution::DirectFn(_) => matches!(expr.kind, TirExprKind::Call { .. }),
            Resolution::Upvalue(_)
            | Resolution::GlobalSlot(_)
            | Resolution::NativeGlobal(_)
            | Resolution::ModuleSlot { .. }
            | Resolution::FieldSlot(_)
            | Resolution::StaticField(_)
            | Resolution::VtableSlot(_)
            | Resolution::EnumVariant { .. }
            | Resolution::ByName { .. } => false,
        }
    }
}

fn placeholder(res: Resolution) -> TirExpr {
    TirExpr {
        kind: TirExprKind::NullLit,
        ty: BackendTy::Dynamic(crate::DynReason::Runtime),
        res,
        span: Span::EMPTY,
    }
}

pub(crate) fn stmt_types(stmts: &mut [TirStmt], f: &mut impl FnMut(&mut BackendTy)) {
    for stmt in stmts {
        match stmt {
            TirStmt::Let { ty, .. } => f(ty),
            TirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                stmt_types(then_body, f);
                stmt_types(else_body, f);
            }
            TirStmt::Loop { body, .. } => stmt_types(body, f),
            TirStmt::Try {
                body, catch_body, ..
            } => {
                stmt_types(body, f);
                stmt_types(catch_body, f);
            }
            TirStmt::Expr(_)
            | TirStmt::Return(_)
            | TirStmt::Break
            | TirStmt::Continue
            | TirStmt::Throw(_)
            | TirStmt::BuildClass(_) => {}
        }
    }
}
