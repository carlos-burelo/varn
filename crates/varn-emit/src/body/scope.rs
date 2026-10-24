use super::context::{FnEmitter, ModuleCtx};
use crate::ty::lower_type;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use varn_core::ast::{AstArena, AstId, ExprId};
use varn_sem::output::TypeEntry;
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
            lexical_this: false,
            home_class: None,
            namespace: None,
            not_yet_bound: FxHashSet::default(),
            top_level: false,
            saw_await: false,
            outer_names: FxHashSet::default(),
            captures: Vec::new(),
            captured_locals: FxHashSet::default(),
            pending: Vec::new(),
            return_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            yield_ty: BackendTy::Dynamic(DynReason::NotYetSupported),
            disposables: Vec::new(),
            loops: Vec::new(),
            pending_label: None,
            place_target: false,
        }
    }

    pub fn expect_results(&mut self, declared: BackendTy, is_async: bool, is_generator: bool) {
        if is_generator {
            self.yield_ty = match declared {
                BackendTy::Dynamic(DynReason::Yields(yielded)) => self.tt.get(yielded),
                other @ BackendTy::Int
                | other @ BackendTy::Float
                | other @ BackendTy::Bool
                | other @ BackendTy::Char
                | other @ BackendTy::Str
                | other @ BackendTy::Bytes
                | other @ BackendTy::Decimal
                | other @ BackendTy::BigInt
                | other @ BackendTy::Array(_)
                | other @ BackendTy::Map(..)
                | other @ BackendTy::Dict(_)
                | other @ BackendTy::Set(_)
                | other @ BackendTy::Tuple(_)
                | other @ BackendTy::Class(_)
                | other @ BackendTy::Enum(_)
                | other @ BackendTy::Fn(_)
                | other @ BackendTy::Nullable(_)
                | other @ BackendTy::Void
                | other @ BackendTy::Never
                | other @ BackendTy::Dynamic(_) => other,
            };
        } else if is_async {
            self.return_ty = match declared {
                BackendTy::Dynamic(DynReason::Awaitable(result)) => self.tt.get(result),
                other @ BackendTy::Int
                | other @ BackendTy::Float
                | other @ BackendTy::Bool
                | other @ BackendTy::Char
                | other @ BackendTy::Str
                | other @ BackendTy::Bytes
                | other @ BackendTy::Decimal
                | other @ BackendTy::BigInt
                | other @ BackendTy::Array(_)
                | other @ BackendTy::Map(..)
                | other @ BackendTy::Dict(_)
                | other @ BackendTy::Set(_)
                | other @ BackendTy::Tuple(_)
                | other @ BackendTy::Class(_)
                | other @ BackendTy::Enum(_)
                | other @ BackendTy::Fn(_)
                | other @ BackendTy::Nullable(_)
                | other @ BackendTy::Void
                | other @ BackendTy::Never
                | other @ BackendTy::Dynamic(_) => other,
            };
        } else {
            self.return_ty = declared;
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
            | TirExprKind::Checked { .. }
            | TirExprKind::Closure { .. }
            | TirExprKind::New { .. }
            | TirExprKind::MakeVariant { .. }
            | TirExprKind::Select { .. }
            | TirExprKind::Seq { .. }
            | TirExprKind::ObjectKeys { .. }
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
            | other @ TirExprKind::Checked { .. }
            | other @ TirExprKind::Closure { .. }
            | other @ TirExprKind::New { .. }
            | other @ TirExprKind::MakeVariant { .. }
            | other @ TirExprKind::Select { .. }
            | other @ TirExprKind::Seq { .. }
            | other @ TirExprKind::ObjectKeys { .. }
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

    pub(super) fn has_own_this(&self) -> bool {
        !self.lexical_this && (self.this_enum.is_some() || self.this_class.is_some())
    }

    pub(super) fn this_var(&mut self, span: Span) -> TirExpr {
        let ty = self
            .this_enum
            .map(BackendTy::Enum)
            .or_else(|| self.this_class.map(BackendTy::Class))
            .unwrap_or(BackendTy::Dynamic(DynReason::NotYetSupported));
        let res = if self.lexical_this {
            self.resolve_name(varn_core::TokenKind::This.as_str())
        } else {
            Resolution::None
        };
        TirExpr {
            kind: TirExprKind::Var,
            ty,
            res,
            span,
        }
    }

    pub fn with_this(mut self, class: ClassId) -> Self {
        self.this_class = Some(class);
        self.home_class = Some(class);
        self.namespace = crate::namespace_scope::enclosing(self.m.classes.get(class.0 as usize));
        self
    }

    pub fn with_this_enum(mut self, enum_id: EnumId) -> Self {
        self.this_enum = Some(enum_id);
        let owner = self.m.enums.get(enum_id.0 as usize).map(|info| &info.name);
        self.namespace = owner
            .and_then(|name| name.rsplit_once("::"))
            .map(|(outer, _)| Arc::from(outer));
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
            | BackendTy::Dict(_)
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
            BackendTy::Int => T::Int,
            BackendTy::Float => T::Float,
            BackendTy::Bool => T::Bool,
            BackendTy::Char => T::Char,
            BackendTy::Bytes => T::Bytes,
            BackendTy::Decimal => T::Decimal,
            BackendTy::BigInt => T::BigInt,
            BackendTy::Dynamic(DynReason::Yields(_)) => T::Generator,
            BackendTy::Dynamic(DynReason::Range(_)) => T::Range,
            BackendTy::Tuple(_)
            | BackendTy::Dict(_)
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
}
