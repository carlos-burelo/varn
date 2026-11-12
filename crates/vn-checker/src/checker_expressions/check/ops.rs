use super::const_int::overflows_int_literal;
use super::Checker;
use crate::checker::recorder::Recorder;
use vn_core::ast::operators::BinaryOp;
use vn_core::ast::operators::UnaryOp;
use vn_core::ast::{ExprId, ExprKind};
use vn_sem::bind::BindResult;
use vn_sem::types::TypeContext;

impl<'r> Checker<'r> {
    pub(super) fn check_unary(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        op: UnaryOp,
        operand: ExprId,
        arena: &vn_core::ast::AstArena,
        bind: &BindResult,
    ) {
        self.check_expr(rec, operand, bind);
        if op == UnaryOp::Not {
            self.require_condition(rec, operand, bind);
        }
        let overloaded = self.check_unary_capability(rec, expr, op, operand, bind);
        match op {
            UnaryOp::Minus | UnaryOp::Plus if !overloaded => {
                self.require_number(rec, operand, "this operator", bind)
            }
            UnaryOp::BitNot if !overloaded => self.require_int(rec, operand, "this operator", bind),
            UnaryOp::Minus | UnaryOp::Plus | UnaryOp::BitNot | UnaryOp::Not => {}
        }
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
        range: vn_core::SourceRange,
        arena: &vn_core::ast::AstArena,
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

        if self.reject_reference_equality(rec, op, left, right, range, bind) {
            return;
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
        args: &[vn_core::ast::Arg],
        range: vn_core::SourceRange,
        arena: &vn_core::ast::AstArena,
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
                | ExprKind::Template { .. }
                | ExprKind::TaggedTemplate { .. }
                | ExprKind::Missing
                | ExprKind::This
                | ExprKind::ImportMeta(_)
                | ExprKind::Super
                | ExprKind::Array { .. }
                | ExprKind::Object { .. }
                | ExprKind::Tuple { .. }
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
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::ImportMeta(_)
            | ExprKind::Super
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
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
        if let Some(name) = cls_name
            && self.abstract_classes.contains(name)
        {
            self.emit(
                vn_core::Diagnostic::error(
                    vn_core::ErrorCode::AbstractMethodNotImplemented,
                    format!("cannot instantiate abstract class '{name}'"),
                )
                .with_range(range),
            );
        }
        self.check_expr(rec, callee, bind);
        let instance = self.infer_type(rec, expr, bind);
        let expected = self
            .expected_for(expr)
            .map(|ty| ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table)));
        if expected != Some(instance) {
            self.check_generic_type_use(
                &vn_sem::bind::GenericTypeUse {
                    range,
                    scope: self.current_scope,
                    ty: instance,
                },
                bind,
            );
        }
        let view = vn_sem::bind::BindView::new(bind, self.resolver);
        if let ExprKind::New { type_args, .. } = &arena.expr(expr).kind
            && !type_args.is_empty()
            && let Some(class) = cls_name
        {
            let expected = crate::checker_expressions::members::member_util::class_type_params(
                self.resolver,
                class,
                None,
                bind,
            );
            let known = !expected.is_empty()
                || view.get_class_members(class, None).is_some()
                || bind
                    .core
                    .as_ref()
                    .is_some_and(|b| b.class_type_params.contains_key(class));
            if known && expected.len() != type_args.len() {
                self.emit(
                    vn_core::Diagnostic::error(
                        vn_core::ErrorCode::WrongTypeArgCount,
                        format!(
                            "wrong number of type arguments: expected {} but got {}",
                            expected.len(),
                            type_args.len()
                        ),
                    )
                    .with_range(range),
                );
            }
        }
        let declared = cls_name.and_then(|cn| {
            view.get_class_members(cn, None).and_then(|members| {
                members
                    .iter()
                    .find(|m| m.kind == vn_sem::types::ClassMemberKind::Constructor)
                    .map(|m| m.ty)
            })
        });
        let cls_name: Option<String> = cls_name.map(str::to_owned);
        if let Some(class) = cls_name.as_deref() {
            self.require_instantiation(rec, expr, class, range, bind);
        }
        let instantiated = declared.and_then(|ctor| {
            self.instantiated_constructor(rec, expr, ctor, cls_name.as_deref()?, bind)
        });
        let context_ty = instantiated.or(declared);
        let ctor_params = match context_ty.map(|ty| self.ty_table.get(ty.0)) {
            Some(vn_core::TypeKind::Fn(fid)) => self.ty_table.get_function(fid).params.clone(),
            Some(_) | None => Vec::new(),
        };
        let synthesized = match declared {
            Some(_) => None,
            None => self.synthesized_ctor_params(rec, expr, bind),
        };
        let validate = instantiated.is_some() || synthesized.is_some();
        let params = synthesized.unwrap_or(ctor_params);
        self.check_call_args_with_context(rec, args, &params, bind);
        if validate {
            self.validate_call_arguments(rec, args, &params, &range, expr.index(), bind);
        }
        self.forget_closure_args(rec, args, bind);
        self.forget_all_members();
    }

    fn require_instantiation(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        class: &str,
        range: vn_core::SourceRange,
        bind: &BindResult,
    ) {
        use crate::checker_expressions::members::member_util::class_type_params;
        let instance = self.infer_type(rec, expr, bind);
        let vn_core::TypeKind::Named(_, origin) = self.ty_table.get(instance.0) else {
            return;
        };
        let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
        if class_type_params(self.resolver, class, origin.as_ref(), bind).is_empty() {
            return;
        }
        self.emit(
            vn_core::Diagnostic::error(
                vn_core::ErrorCode::TypeArgInferenceFailed,
                format!(
                    "cannot infer the type arguments of '{class}' from this call; write them: new {class}<...>(...)"
                ),
            )
            .with_range(range),
        );
    }

    fn instantiated_constructor(
        &mut self,
        rec: &mut Recorder,
        expr: ExprId,
        ctor: vn_sem::types::Type,
        class: &str,
        bind: &BindResult,
    ) -> Option<vn_sem::types::Type> {
        use crate::checker_expressions::members::member_util::{
            class_type_params, generic_mapping,
        };
        let instance = self.infer_type(rec, expr, bind);
        match self.ty_table.get(instance.0) {
            vn_core::TypeKind::Generic(_, args, origin) => {
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                let args: Vec<vn_sem::types::Type> = self
                    .ty_table
                    .get_list(args)
                    .iter()
                    .map(|id| vn_sem::types::Type::resolved(*id))
                    .collect();
                let mapping = generic_mapping(self.resolver, class, &args, origin.as_ref(), bind);
                Some(
                    ctor.map_generics(&mapping, &mut *std::sync::Arc::make_mut(&mut self.ty_table)),
                )
            }
            vn_core::TypeKind::Named(_, origin) => {
                let origin = origin.map(|o| self.resolve_bind_atom(bind, o));
                class_type_params(self.resolver, class, origin.as_ref(), bind)
                    .is_empty()
                    .then_some(ctor)
            }
            vn_core::TypeKind::Primitive(_)
            | vn_core::TypeKind::Literal(_)
            | vn_core::TypeKind::This
            | vn_core::TypeKind::Array(_)
            | vn_core::TypeKind::Union(_)
            | vn_core::TypeKind::Intersection(_)
            | vn_core::TypeKind::Tuple(_)
            | vn_core::TypeKind::TemplateLiteral(_)
            | vn_core::TypeKind::Fn(_)
            | vn_core::TypeKind::Record(_)
            | vn_core::TypeKind::Interface(_)
            | vn_core::TypeKind::Typeof(_)
            | vn_core::TypeKind::KeyOf(_)
            | vn_core::TypeKind::IndexedAccess { .. }
            | vn_core::TypeKind::Mapped { .. }
            | vn_core::TypeKind::Conditional { .. }
            | vn_core::TypeKind::Infer(_)
            | vn_core::TypeKind::EnumVariant { .. }
            | vn_core::TypeKind::TypePredicate { .. } => None,
        }
    }
}
