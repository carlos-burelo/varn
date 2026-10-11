use super::const_int::overflows_int_literal;
use super::Checker;
use crate::checker::recorder::Recorder;
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::operators::UnaryOp;
use varn_core::ast::{ExprId, ExprKind};
use varn_sem::bind::BindResult;
use varn_sem::types::TypeContext;

impl<'r> Checker<'r> {
    pub(super) fn check_unary(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        op: UnaryOp,
        operand: ExprId,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        self.check_expr(rec, operand, bind);
        if op == UnaryOp::Not {
            self.require_condition(rec, operand, bind);
        }
        self.check_unary_capability(rec, expr, op, operand, bind);
        if overflows_int_literal(expr, arena) && !overflows_int_literal(operand, arena) {
            self.report_int_overflow(expr);
        }
    }

    pub(super) fn check_binary(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        left: ExprId,
        right: ExprId,
        op: BinaryOp,
        range: varn_core::SourceRange,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        self.check_expr(rec, left, bind);
        self.check_expr(rec, right, bind);

        if overflows_int_literal(expr, arena)
            && !overflows_int_literal(left, arena)
            && !overflows_int_literal(right, arena)
        {
            self.report_int_overflow(expr);
        }

        if !self.check_binary_capability(rec, expr, op, left, right, bind) {
            self.check_binary_operands(rec, op, left, right, range, bind);
        }
    }

    pub(super) fn check_new(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        callee: ExprId,
        args: &[varn_core::ast::Arg],
        range: varn_core::SourceRange,
        arena: &varn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        if self.pure_scope.is_some() {
            self.forbid_pure(
                "allocate with 'new' (constructors cannot prove purity)",
                range,
            );
        }
        let cls_name = match &arena.expr(callee).kind {
            ExprKind::Identifier { name } => Some(bind.interner.resolve(*name)),
            ExprKind::Member {
                property,
                computed: false,
                ..
            } => match &arena.expr(*property).kind {
                ExprKind::Identifier { name } => Some(bind.interner.resolve(*name)),
                ExprKind::IntLiteral { .. }
                | ExprKind::FloatLiteral { .. }
                | ExprKind::BigIntLiteral { .. }
                | ExprKind::DecimalLiteral { .. }
                | ExprKind::StrLiteral { .. }
                | ExprKind::CharLiteral { .. }
                | ExprKind::BoolLiteral { .. }
                | ExprKind::NullLiteral
                | ExprKind::RegexLiteral { .. }
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
                | ExprKind::Record { .. }
                | ExprKind::Unary { .. }
                | ExprKind::Update { .. }
                | ExprKind::Binary { .. }
                | ExprKind::Logical { .. }
                | ExprKind::Assign { .. }
                | ExprKind::Conditional { .. }
                | ExprKind::Member { .. }
                | ExprKind::Call { .. }
                | ExprKind::New { .. }
                | ExprKind::Function { .. }
                | ExprKind::Arrow { .. }
                | ExprKind::Sequence { .. }
                | ExprKind::Paren { .. }
                | ExprKind::Await { .. }
                | ExprKind::Spawn { .. }
                | ExprKind::Yield { .. }
                | ExprKind::Spread { .. }
                | ExprKind::Pipeline { .. }
                | ExprKind::Range { .. }
                | ExprKind::NonNull { .. }
                | ExprKind::Try { .. }
                | ExprKind::As { .. }
                | ExprKind::Satisfies { .. }
                | ExprKind::ClassExpr { .. }
                | ExprKind::Match { .. }
                | ExprKind::Is { .. }
                | ExprKind::With { .. }
                | ExprKind::MetaAccess { .. } => None,
            },
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => None,
        };
        if let Some(name) = cls_name {
            if self.abstract_classes.contains(name) {
                self.emit(
                    varn_core::Diagnostic::error(
                        varn_core::ErrorCode::AbstractMethodNotImplemented,
                        format!("cannot instantiate abstract class '{name}'"),
                    )
                    .with_range(range),
                );
            }
        }
        self.check_expr(rec, callee, bind);
        let view = varn_sem::bind::BindView::new(bind, self.resolver);
        let declared = cls_name.and_then(|cn| {
            view.get_class_members(cn, None).and_then(|members| {
                members
                    .iter()
                    .find(|m| m.kind == varn_sem::types::ClassMemberKind::Constructor)
                    .map(|m| m.ty)
            })
        });
        let cls_name: Option<String> = cls_name.map(str::to_owned);
        let instantiated = declared.and_then(|ctor| self.instantiated_constructor(rec, expr, ctor, cls_name.as_deref()?, bind));
        let context_ty = instantiated.or(declared);
        let ctor_params = match context_ty.map(|ty| self.ty_table.get(ty.0)) {
            Some(varn_core::TypeKind::Fn(fid)) => self.ty_table.get_function(fid).params.clone(),
            Some(_) | None => Vec::new(),
        };
        self.check_call_args_with_context(rec, args, &ctor_params, bind);
        if instantiated.is_some() {
            self.validate_call_arguments(rec, args, &ctor_params, &range, expr.index(), bind);
        }
    }

    fn instantiated_constructor(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        ctor: varn_sem::types::Type,
        class: &str,
        bind: &BindResult,
    ) -> Option<varn_sem::types::Type> {
        use crate::checker_expressions::members::member_util::{class_type_params, generic_mapping};
        let instance = self.infer_type(rec, expr, bind);
        match self.ty_table.get(instance.0) {
            varn_core::TypeKind::Generic(_, args, origin) => {
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                let args: Vec<varn_sem::types::Type> = self
                    .ty_table
                    .get_list(args)
                    .iter()
                    .map(|id| varn_sem::types::Type::resolved(*id))
                    .collect();
                let mapping = generic_mapping(self.resolver, class, &args, origin.as_ref(), bind);
                Some(ctor.map_generics(&mapping, &mut *std::sync::Arc::make_mut(&mut self.ty_table)))
            }
            varn_core::TypeKind::Named(_, origin) => {
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                class_type_params(self.resolver, class, origin.as_ref(), bind)
                    .is_empty()
                    .then_some(ctor)
            }
            varn_core::TypeKind::Primitive(_)
            | varn_core::TypeKind::Builtin(_)
            | varn_core::TypeKind::Literal(_)
            | varn_core::TypeKind::This
            | varn_core::TypeKind::Array(_)
            | varn_core::TypeKind::Union(_)
            | varn_core::TypeKind::Intersection(_)
            | varn_core::TypeKind::Tuple(_)
            | varn_core::TypeKind::TemplateLiteral(_)
            | varn_core::TypeKind::Fn(_)
            | varn_core::TypeKind::Object(_)
            | varn_core::TypeKind::Typeof(_)
            | varn_core::TypeKind::KeyOf(_)
            | varn_core::TypeKind::IndexedAccess { .. }
            | varn_core::TypeKind::Mapped { .. }
            | varn_core::TypeKind::Conditional { .. }
            | varn_core::TypeKind::Infer(_)
            | varn_core::TypeKind::EnumVariant { .. }
            | varn_core::TypeKind::TypePredicate { .. } => None,
        }
    }
}
