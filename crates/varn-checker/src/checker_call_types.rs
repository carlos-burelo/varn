mod call_binary;
mod call_closure;
mod call_composite;
mod call_control;
mod call_invoke;
mod call_member;
mod call_new;

use call_binary::infer_binary;
use call_closure::infer_closure;
use call_composite::infer_match;
use call_control::{infer_await, infer_conditional};
use call_invoke::infer_invoke;
use call_member::infer_member;
use call_new::infer_new;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_binder::resolve_type_node;
use varn_core::ast::{AstArena, ExprId, ExprKind};
use varn_core::AtomInterner;
use varn_sem::types::{CheckerTyTable, Type, TypeContext};

pub(crate) struct CallTypeCtx<'a> {
    pub fn_map: &'a FxHashMap<Arc<str>, Type>,
    pub fn_type_params: &'a FxHashMap<Arc<str>, Vec<Arc<str>>>,
    pub class_methods: &'a FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    pub sym_map: &'a FxHashMap<Arc<str>, Type>,
    pub ast_arena: &'a AstArena,
    pub ctx: Option<&'a dyn TypeContext>,
    pub current_class: Option<&'a str>,
    pub interner: &'a AtomInterner,
    pub table: &'a mut CheckerTyTable,
}

impl<'a> CallTypeCtx<'a> {
    pub fn infer(&mut self, expr: ExprId) -> Option<Type> {
        let kind = self.ast_arena.expr(expr).kind.clone();
        match kind {
            ExprKind::IntLiteral { .. } => Some(Type::Int),
            ExprKind::FloatLiteral { .. } => Some(Type::Float),
            ExprKind::StrLiteral { .. } => Some(Type::Str),
            ExprKind::BoolLiteral { .. } => Some(Type::Bool),
            ExprKind::This => {
                let current = self.current_class;
                let origin = self.ctx.and_then(|c| c.source_file());
                current.map(|n| {
                    Type::named_with_origin(Arc::from(n), origin.map(Arc::from), self.table)
                })
            }
            ExprKind::Identifier { name } => {
                let key = self.interner.resolve(name);
                self.sym_map.get(key).cloned()
            }
            ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } => infer_member(self, object, property),
            ExprKind::Binary { left, right, op } => infer_binary(self, left, right, op),
            ExprKind::Call {
                callee, type_args, ..
            } => infer_invoke(self, callee, &type_args),
            ExprKind::New {
                callee, type_args, ..
            } => infer_new(self, callee, &type_args),
            ExprKind::Paren { expression } => self.infer(expression),
            ExprKind::As { type_ann, .. } => {
                let ctx = self.ctx;
                Some(resolve_type_node(&type_ann, ctx, self.table))
            }
            ExprKind::Await { argument } => infer_await(self, argument),
            ExprKind::Conditional {
                consequent,
                alternate,
                ..
            } => infer_conditional(self, consequent, alternate),
            ExprKind::Function {
                params,
                return_type,
                ..
            } => infer_closure(self, &params, &return_type, None),
            ExprKind::Arrow {
                params,
                return_type,
                body,
                ..
            } => infer_closure(self, &params, &return_type, Some(body.as_ref())),
            ExprKind::Match { cases, .. } => infer_match(self, &cases),
            ExprKind::Pipeline { right, .. } => self.infer(right),
            ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Member { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => Some(Type::Dynamic),
        }
    }
}

pub(crate) fn infer_call_type(
    fn_map: &FxHashMap<Arc<str>, Type>,
    fn_type_params: &FxHashMap<Arc<str>, Vec<Arc<str>>>,
    class_methods: &FxHashMap<Arc<str>, FxHashMap<Arc<str>, Type>>,
    sym_map: &FxHashMap<Arc<str>, Type>,
    expr: ExprId,
    ast_arena: &AstArena,
    ctx: Option<&dyn TypeContext>,
    current_class: Option<&str>,
    interner: &AtomInterner,
    table: &mut CheckerTyTable,
) -> Option<Type> {
    CallTypeCtx {
        fn_map,
        fn_type_params,
        class_methods,
        sym_map,
        ast_arena,
        ctx,
        current_class,
        interner,
        table,
    }
    .infer(expr)
}
