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
    pub constants: Vec<(Arc<str>, TirExpr)>,
    pub inert: bool,
}

impl ModuleTemplates {
    pub fn constant(&self, export: &str) -> Option<&TirExpr> {
        self.constants
            .iter()
            .find(|(name, _)| name.as_ref() == export)
            .map(|(_, literal)| literal)
    }

    pub fn package(&self, export: &str) -> Option<&Arc<InlinePackage>> {
        self.exports
            .iter()
            .find(|(name, _)| name.as_ref() == export)
            .map(|(_, package)| package)
    }
}

fn is_literal(kind: &TirExprKind) -> bool {
    matches!(
        kind,
        TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
    )
}

fn literal_assignment(stmt: &TirStmt) -> Option<(u32, &TirExpr)> {
    let TirStmt::Expr(TirExpr {
        kind: TirExprKind::Assign { target, value },
        ..
    }) = stmt
    else {
        return None;
    };
    let Resolution::GlobalSlot(slot) = target.res else {
        return None;
    };
    is_literal(&value.kind).then_some((slot, value))
}

fn visit_stmts(stmts: &[TirStmt], f: &mut dyn FnMut(&TirExpr)) {
    for stmt in stmts {
        match stmt {
            TirStmt::Expr(e) | TirStmt::Throw(e) => visit_expr(e, f),
            TirStmt::Let { init, .. } => init.iter().for_each(|e| visit_expr(e, f)),
            TirStmt::Return(value) => value.iter().for_each(|e| visit_expr(e, f)),
            TirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                visit_expr(cond, f);
                visit_stmts(then_body, f);
                visit_stmts(else_body, f);
            }
            TirStmt::Loop { cond, body } => {
                visit_expr(cond, f);
                visit_stmts(body, f);
            }
            TirStmt::Try {
                body, catch_body, ..
            } => {
                visit_stmts(body, f);
                visit_stmts(catch_body, f);
            }
            TirStmt::Break | TirStmt::Continue | TirStmt::BuildClass(_) => {}
        }
    }
}

fn visit_expr(e: &TirExpr, f: &mut dyn FnMut(&TirExpr)) {
    f(e);
    if let TirExprKind::Seq { stmts, .. } = &e.kind {
        visit_stmts(stmts, f);
    }
    for child in crate::children::child_exprs(e) {
        visit_expr(child, f);
    }
}

fn module_constants(module: &TirModule) -> FxHashMap<u32, TirExpr> {
    let mut assignments: FxHashMap<u32, u32> = FxHashMap::default();
    let mut count = |e: &TirExpr| {
        if let TirExprKind::Assign { target, .. } = &e.kind
            && let Resolution::GlobalSlot(slot) = target.res
        {
            *assignments.entry(slot).or_insert(0) += 1;
        }
    };
    visit_stmts(&module.top_level.body, &mut count);
    for function in &module.functions {
        visit_stmts(&function.body, &mut count);
    }
    let mut constants = FxHashMap::default();
    for stmt in &module.top_level.body {
        if let Some((slot, literal)) = literal_assignment(stmt)
            && assignments.get(&slot) == Some(&1)
        {
            constants.insert(slot, literal.clone());
        }
    }
    constants
}

pub fn extract(module: &TirModule) -> ModuleTemplates {
    let constants = module_constants(module);
    let mut exports = Vec::new();
    let mut exported_constants = Vec::new();
    for export in &module.exports {
        if export.reexport_from.is_none()
            && !export.namespace
            && let Some(slot) = module.global_names.iter().position(|n| *n == export.local)
            && let Some(literal) = constants.get(&(slot as u32))
        {
            exported_constants.push((export.exported.clone(), literal.clone()));
        }
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
            constants: &constants,
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
        constants: exported_constants,
        inert: is_inert(module),
    }
}

fn is_inert(module: &TirModule) -> bool {
    module
        .top_level
        .body
        .iter()
        .all(|stmt| literal_assignment(stmt).is_some())
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
    constants: &'m FxHashMap<u32, TirExpr>,
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
        if let (TirExprKind::Var, Resolution::GlobalSlot(slot)) = (&expr.kind, &expr.res)
            && let Some(literal) = self.constants.get(slot)
        {
            if literal.ty.representation() != expr.ty.representation() {
                return false;
            }
            expr.kind = literal.kind.clone();
            expr.res = Resolution::None;
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
