use super::context::{FnEmitter, ModuleCtx};
use crate::checker::TypeEntry;
use crate::emit::ty::lower_type;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, ExprId};
use varn_tir::{
    BackendTy, ClassId, ClassInfo, DynReason, EnumId, LocalId, Resolution, Signature, Span,
    TirExpr, TirExprKind, TirFunction, TirStmt, TyTable,
};

impl<'a> FnEmitter<'a> {
    pub fn new(
        ast_arena: &'a AstArena,
        expr_table: &'a FxHashMap<AstId, TypeEntry>,
        tt: &'a mut TyTable,
        m: ModuleCtx<'a>,
        signatures: &'a mut Vec<Signature>,
        out_closures: &'a mut Vec<TirFunction>,
        closure_base: u32,
        params: Vec<Arc<str>>,
    ) -> Self {
        FnEmitter {
            ast_arena,
            expr_table,
            tt,
            m,
            signatures,
            out_closures,
            closure_base,
            locals: Vec::new(),
            scopes: vec![FxHashMap::default()],
            params,
            this_class: None,
            this_enum: None,
            top_level: false,
            saw_await: false,
            outer_names: FxHashSet::default(),
            captures: Vec::new(),
            pending: Vec::new(),
            disposables: Vec::new(),
        }
    }

    pub(super) fn fresh_local(&mut self, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        id
    }

    pub(super) fn lower_expr(&mut self, e: ExprId) -> TirExpr {
        let outer = std::mem::take(&mut self.pending);
        let value = self.lower_expr_node(e);
        let stmts = std::mem::replace(&mut self.pending, outer);
        if stmts.is_empty() {
            return value;
        }
        TirExpr {
            ty: value.ty,
            span: value.span,
            res: Resolution::None,
            kind: TirExprKind::Seq {
                stmts,
                value: Box::new(value),
            },
        }
    }

    pub(super) fn pin(&mut self, e: TirExpr) -> TirExpr {
        match e.kind {
            TirExprKind::Var => e,
            TirExprKind::IntLit(_)
            | TirExprKind::FloatLit(_)
            | TirExprKind::BoolLit(_)
            | TirExprKind::StrLit(_)
            | TirExprKind::CharLit(_)
            | TirExprKind::NullLit
            | TirExprKind::Binary { .. }
            | TirExprKind::Unary { .. }
            | TirExprKind::Field { .. }
            | TirExprKind::Index { .. }
            | TirExprKind::Call { .. }
            | TirExprKind::MethodCall { .. }
            | TirExprKind::Assign { .. }
            | TirExprKind::ArrayLit(_)
            | TirExprKind::TupleLit(_)
            | TirExprKind::ObjectLit { .. }
            | TirExprKind::RecordLit { .. }
            | TirExprKind::Await { .. }
            | TirExprKind::Yield { .. }
            | TirExprKind::Discriminant { .. }
            | TirExprKind::VariantPayload { .. }
            | TirExprKind::TypeTest { .. }
            | TirExprKind::Cast { .. }
            | TirExprKind::Closure { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::Seq { .. }
            | TirExprKind::ObjectKeys { .. }
            | TirExprKind::IterInit { .. }
            | TirExprKind::SuperCall { .. }
            | TirExprKind::SuperMethodCall { .. }
            | TirExprKind::DecimalLit(_)
            | TirExprKind::BigIntLit(_)
            | TirExprKind::RangeLit { .. }
            | TirExprKind::ObjectRest { .. }
            | TirExprKind::ExtensionCall { .. } => self.hoist(e),
        }
    }

    pub(super) fn pin_place(&mut self, place: TirExpr) -> TirExpr {
        let TirExpr {
            kind,
            ty,
            res,
            span,
        } = place;
        let kind = match kind {
            TirExprKind::Field { object, name } => TirExprKind::Field {
                object: Box::new(self.pin(*object)),
                name,
            },
            TirExprKind::Index { object, index } => TirExprKind::Index {
                object: Box::new(self.pin(*object)),
                index: Box::new(self.pin(*index)),
            },
            other @ TirExprKind::IntLit(_)
            | other @ TirExprKind::FloatLit(_)
            | other @ TirExprKind::BoolLit(_)
            | other @ TirExprKind::StrLit(_)
            | other @ TirExprKind::CharLit(_)
            | other @ TirExprKind::NullLit
            | other @ TirExprKind::Var
            | other @ TirExprKind::Binary { .. }
            | other @ TirExprKind::Unary { .. }
            | other @ TirExprKind::Call { .. }
            | other @ TirExprKind::MethodCall { .. }
            | other @ TirExprKind::Assign { .. }
            | other @ TirExprKind::ArrayLit(_)
            | other @ TirExprKind::TupleLit(_)
            | other @ TirExprKind::ObjectLit { .. }
            | other @ TirExprKind::RecordLit { .. }
            | other @ TirExprKind::Await { .. }
            | other @ TirExprKind::Yield { .. }
            | other @ TirExprKind::Discriminant { .. }
            | other @ TirExprKind::VariantPayload { .. }
            | other @ TirExprKind::TypeTest { .. }
            | other @ TirExprKind::Cast { .. }
            | other @ TirExprKind::Closure { .. }
            | other @ TirExprKind::New { .. }
            | other @ TirExprKind::MakeVariant { .. }
            | other @ TirExprKind::Select { .. }
            | other @ TirExprKind::Seq { .. }
            | other @ TirExprKind::ObjectKeys { .. }
            | other @ TirExprKind::IterInit { .. }
            | other @ TirExprKind::SuperCall { .. }
            | other @ TirExprKind::SuperMethodCall { .. }
            | other @ TirExprKind::DecimalLit(_)
            | other @ TirExprKind::BigIntLit(_)
            | other @ TirExprKind::RangeLit { .. }
            | other @ TirExprKind::ObjectRest { .. }
            | other @ TirExprKind::ExtensionCall { .. } => other,
        };
        TirExpr {
            kind,
            ty,
            res,
            span,
        }
    }

    pub fn hoist(&mut self, e: TirExpr) -> TirExpr {
        let ty = e.ty;
        let span = e.span;
        let local = self.fresh_local(ty);
        self.pending.push(TirStmt::Let {
            local,
            ty,
            init: Some(e),
        });
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::Local(local),
            span,
        }
    }

    pub(super) fn this_var(&self, span: Span) -> TirExpr {
        let ty = self
            .this_enum
            .map(BackendTy::Enum)
            .or_else(|| self.this_class.map(BackendTy::Class))
            .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res: Resolution::None,
            span,
        }
    }

    pub fn with_this(mut self, class: ClassId) -> Self {
        self.this_class = Some(class);
        self
    }

    pub fn with_this_enum(mut self, enum_id: EnumId) -> Self {
        self.this_enum = Some(enum_id);
        self
    }

    pub fn into_top_level(mut self) -> Self {
        self.top_level = true;
        self
    }

    pub fn saw_await(&self) -> bool {
        self.saw_await
    }

    pub fn lower_outer_expr(&mut self, e: ExprId) -> (Vec<TirStmt>, TirExpr) {
        let x = self.lower_expr(e);
        (std::mem::take(&mut self.pending), x)
    }

    pub fn lower_expression(&mut self, e: ExprId) -> TirExpr {
        self.lower_expr(e)
    }

    pub fn take_pending(&mut self) -> Vec<TirStmt> {
        std::mem::take(&mut self.pending)
    }

    pub(super) fn class_of(&self, ty: BackendTy) -> Option<&'a ClassInfo> {
        match ty.non_nullable(self.tt) {
            BackendTy::Class(c) => self.m.classes.get(c.0 as usize),
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Str
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Array(_)
            | BackendTy::Map(..)
            | BackendTy::Set(_)
            | BackendTy::Tuple(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => None,
        }
    }

    pub(super) fn core_class_name(&self, ty: BackendTy) -> Option<&'static str> {
        use varn_core::RuntimeKind as T;
        let tag = match ty.non_nullable(self.tt) {
            BackendTy::Array(_) => T::Array,
            BackendTy::Str => T::Str,
            BackendTy::Map(..) => T::Map,
            BackendTy::Set(_) => T::Set,
            BackendTy::Int
            | BackendTy::Float
            | BackendTy::Bool
            | BackendTy::Char
            | BackendTy::Bytes
            | BackendTy::Decimal
            | BackendTy::BigInt
            | BackendTy::Tuple(_)
            | BackendTy::Class(_)
            | BackendTy::Enum(_)
            | BackendTy::Fn(_)
            | BackendTy::Nullable(_)
            | BackendTy::Void
            | BackendTy::Never
            | BackendTy::Dynamic(_) => return None,
        };
        varn_core::op_id::core_class_name(tag)
    }

    pub(super) fn expr_ty(&mut self, e: ExprId) -> BackendTy {
        let names = self.m.names;
        let table = self.m.checker_table;
        let interner = self.m.interner;
        match self.expr_table.get(&e.index()) {
            Some(entry) => lower_type(&entry.ty, table, interner, self.tt, names),
            None => BackendTy::Dynamic(DynReason::NotYetSupported),
        }
    }

    pub(super) fn resolve_name(&mut self, name: &str) -> Resolution {
        for scope in self.scopes.iter().rev() {
            if let Some(id) = scope.get(name) {
                return Resolution::Local(*id);
            }
        }
        if let Some(i) = self.params.iter().position(|p| p.as_ref() == name) {
            return Resolution::Param(i as u32);
        }
        if self.outer_names.contains(name) {
            let idx = match self.captures.iter().position(|c| c.as_ref() == name) {
                Some(i) => i,
                None => {
                    self.captures.push(Arc::from(name));
                    self.captures.len() - 1
                }
            };
            return Resolution::Upvalue(idx as u32);
        }
        if let Some(&slot) = self.m.globals.get(name) {
            return Resolution::GlobalSlot(slot);
        }
        if let Some(idx) = varn_abi::native_global_index(name) {
            return Resolution::NativeGlobal(idx);
        }
        Resolution::ByName {
            name: Arc::from(name),
            why: DynReason::NotYetSupported,
        }
    }

    pub(super) fn bind_local(&mut self, name: Arc<str>, ty: BackendTy) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(ty);
        self.scopes.last_mut().unwrap().insert(name, id);
        id
    }
}
