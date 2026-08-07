use super::stmts::annotate_stmt;
use super::AnnotateCtx;
use crate::binder::infer_expr_type;
use crate::types::{Type, TypeContext};
use varn_core::ast::operators::BinaryOp;
use varn_core::ast::{Arg, ArrayEl, Expr, ExprKind};
use varn_core::intrinsic_ops::intrinsic_lookup;
use varn_core::TypeKind;
use varn_core::{NumericKind, TypeAnnotations};

pub(crate) fn annotate_expr(expr: &Expr, ann: &mut TypeAnnotations, ctx: &mut AnnotateCtx) {
    match &expr.kind {
        ExprKind::Binary { op, left, right } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);

            let is_arithmetic = matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
            );
            let is_comparison = matches!(
                op,
                BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::Eq
                    | BinaryOp::NotEq
            );
            if !is_arithmetic && !is_comparison {
                return;
            }

            let l = get_expr_type(left, ctx);
            let r = get_expr_type(right, ctx);

            use varn_core::{binary_operand_kind, NumericOperand, TypeTag};
            let operand = |tk: &TypeKind<_, _, _, _, _, _>| match tk {
                TypeKind::Intrinsic(TypeTag::Int) => Some(NumericOperand::Int),
                TypeKind::Intrinsic(TypeTag::Float) => Some(NumericOperand::Float),
                TypeKind::Intrinsic(TypeTag::Decimal) => Some(NumericOperand::Decimal),
                _ => None,
            };

            let kind = match binary_operand_kind(operand(&l.0), operand(&r.0)) {
                Some(NumericOperand::Int) => Some(NumericKind::Int),
                Some(NumericOperand::Float) => Some(NumericKind::Float),
                Some(NumericOperand::Decimal) | None => None,
            };
            if let Some(k) = kind {
                ann.record_numeric(expr.range.start.offset, k);
            }
        }
        ExprKind::Paren { expression } => annotate_expr(expression, ann, ctx),
        ExprKind::Unary { operand, .. } => annotate_expr(operand, ann, ctx),
        ExprKind::Logical { left, right, .. } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);
        }
        ExprKind::Assign { value, target, .. } => {
            annotate_expr(target, ann, ctx);
            annotate_expr(value, ann, ctx);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            annotate_expr(test, ann, ctx);
            annotate_expr(consequent, ann, ctx);
            annotate_expr(alternate, ann, ctx);
        }
        ExprKind::Call { callee, args, .. } => {
            annotate_expr(callee, ann, ctx);
            for arg in args {
                let e = match arg {
                    Arg::Positional(e) => e,
                    Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                annotate_expr(e, ann, ctx);
            }

            if let ExprKind::Member {
                object,
                property,
                computed: false,
                ..
            } = &callee.kind
            {
                if let ExprKind::Identifier { name: prop_name } = &property.kind {
                    let obj_ty = get_expr_type(object, ctx);
                    let obj_ty_nn = obj_ty.non_nullified();
                    let mut recorded = false;
                    let key_offset = property.range.start.offset;
                    if let TypeKind::Named(_, Some(ref origin_path)) = &obj_ty_nn.0 {
                        let key = format!("{}/{}", origin_path, prop_name);
                        if let Some(wire_byte) = intrinsic_lookup(&key) {
                            ann.record_intrinsic(key_offset, wire_byte);
                            recorded = true;
                        }
                    }
                    if !recorded {
                        let has_spread = args.iter().any(|a| matches!(a, Arg::Spread(_)));
                        if !has_spread {
                            if let Some(class) = core_class_of_type(&obj_ty_nn) {
                                if let Some(wire_byte) =
                                    varn_core::intrinsic_ops::core_method_intrinsic(
                                        class, prop_name,
                                    )
                                {
                                    ann.record_intrinsic(key_offset, wire_byte);
                                } else if core_has_method(ctx.bind, class, prop_name) {
                                    ann.record_native_op(
                                        key_offset,
                                        varn_core::op_id::core_method_op_id(class, prop_name),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if let ExprKind::Identifier { name } = &callee.kind {
                if !ctx.locals.contains_key(name.as_ref()) {
                    if let Some(wire_byte) = ctx.bind.intrinsic_import_wire(name) {
                        ann.record_intrinsic(callee.range.start.offset, wire_byte);
                    }
                }
            }

            let result_ty = get_expr_type(expr, ctx);
            let result_key = if let ExprKind::Member {
                property,
                computed: false,
                ..
            } = &callee.kind
            {
                property.range.start.offset
            } else {
                expr.range.start.offset
            };
            record_cg_ty_at(result_key, &result_ty, ann, ctx);
        }
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => {
            annotate_expr(object, ann, ctx);
            if *computed {
                annotate_expr(property, ann, ctx);
                let obj_ty = get_expr_type(object, ctx);
                if matches!(obj_ty.non_nullified().0, TypeKind::Array(_)) {
                    ann.record_array_index(expr.range.start.offset);
                }
                let elem_ty = get_expr_type(expr, ctx);
                record_cg_ty_at(property.range.start.offset, &elem_ty, ann, ctx);
            } else if let ExprKind::Identifier { name: prop_name } = &property.kind {
                let obj_ty = get_expr_type(object, ctx);
                let check_ty = obj_ty.non_nullified();
                let member_ty = get_expr_type(expr, ctx);
                record_cg_ty_at(property.range.start.offset, &member_ty, ann, ctx);

                let class_name = match &check_ty.0 {
                    TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => {
                        Some(n.as_ref())
                    }
                    _ => None,
                };
                if let Some(cn) = class_name {
                    if ctx.bind.type_members.classes.contains_key(cn) {
                        let mut hierarchy = Vec::new();
                        let mut current: Option<std::rc::Rc<str>> = Some(std::rc::Rc::from(cn));
                        while let Some(c) = current {
                            hierarchy.push(c.clone());
                            current = ctx.bind.class_parents.get(c.as_ref()).cloned();
                        }
                        hierarchy.reverse();

                        if !hierarchy
                            .iter()
                            .all(|c| ctx.bind.type_members.classes.contains_key(c.as_ref()))
                        {
                            return;
                        }

                        let mut slot = 0u16;
                        let mut known_props: rustc_hash::FxHashSet<std::rc::Rc<str>> =
                            rustc_hash::FxHashSet::default();
                        let mut found = false;

                        for cls in &hierarchy {
                            if let Some(entry) = ctx.bind.get_class_entry(cls) {
                                for m in &entry.members {
                                    if !m.is_static
                                        && (m.kind == crate::types::ClassMemberKind::Property
                                            || m.kind == crate::types::ClassMemberKind::Variable)
                                    {
                                        if known_props.insert(m.name.clone()) {
                                            if m.name.as_ref() == prop_name.as_ref() {
                                                ann.record_fixed_field_slot(
                                                    property.range.start.offset,
                                                    slot,
                                                );
                                                found = true;
                                                break;
                                            }
                                            slot += 1;
                                        }
                                    }
                                }
                            }
                            if found {
                                break;
                            }
                        }
                    }
                }
            }
        }

        ExprKind::TaggedTemplate { tag, template } => {
            annotate_expr(tag, ann, ctx);
            annotate_expr(template, ann, ctx);
        }
        ExprKind::As { expression, .. } | ExprKind::Satisfies { expression, .. } => {
            annotate_expr(expression, ann, ctx)
        }
        ExprKind::Array { elements } => {
            for el in elements {
                match el {
                    ArrayEl::Expr(e) | ArrayEl::Spread(e) => annotate_expr(e, ann, ctx),
                    ArrayEl::Hole => {}
                }
            }
        }
        ExprKind::Tuple { elements } => {
            for e in elements {
                annotate_expr(e, ann, ctx);
            }
        }
        ExprKind::New { callee, args, .. } => {
            annotate_expr(callee, ann, ctx);
            for arg in args {
                let e = match arg {
                    Arg::Positional(e) | Arg::Spread(e) => e,
                    Arg::Named { value, .. } => value,
                };
                annotate_expr(e, ann, ctx);
            }
        }
        ExprKind::Template { parts } => {
            for part in parts {
                if let varn_core::ast::TemplatePart::Interpolation(e) = part {
                    annotate_expr(e, ann, ctx);
                }
            }
        }
        ExprKind::Object { properties }
        | ExprKind::Record { properties } => {
            for p in properties {
                match p {
                    varn_core::ast::ObjectProp::Property { key, value, .. } => {
                        if let varn_core::ast::PropKey::Computed(k_expr) = key {
                            annotate_expr(k_expr, ann, ctx);
                        }
                        annotate_expr(value, ann, ctx);
                    }
                    varn_core::ast::ObjectProp::Method { body, .. }
                    | varn_core::ast::ObjectProp::Getter { body, .. } => {
                        annotate_stmt(body, ann, ctx);
                    }
                    varn_core::ast::ObjectProp::Setter { body, .. } => {
                        annotate_stmt(body, ann, ctx);
                    }
                    varn_core::ast::ObjectProp::Spread { argument, .. } => {
                        annotate_expr(argument, ann, ctx);
                    }
                }
            }
        }
        ExprKind::Function { body, .. } => {
            annotate_stmt(body, ann, ctx);
        }
        ExprKind::Arrow { body, .. } => match body.as_ref() {
            varn_core::ast::ArrowBody::Expr(e) => annotate_expr(e, ann, ctx),
            varn_core::ast::ArrowBody::Block(b) => annotate_stmt(b, ann, ctx),
        },
        ExprKind::Match { subject, cases } => {
            annotate_expr(subject, ann, ctx);
            for c in cases {
                if let Some(g) = &c.guard {
                    annotate_expr(g, ann, ctx);
                }
                match &c.body {
                    varn_core::ast::MatchBody::Expr(e) => annotate_expr(e, ann, ctx),
                    varn_core::ast::MatchBody::Block(b) => annotate_stmt(b, ann, ctx),
                }
            }
        }
        ExprKind::Is { expression, .. } => {
            annotate_expr(expression, ann, ctx);
        }
        ExprKind::Update { operand, .. } => annotate_expr(operand, ann, ctx),
        ExprKind::Await { argument }
        | ExprKind::Spawn { argument }
        | ExprKind::Spread { argument } => annotate_expr(argument, ann, ctx),
        ExprKind::Yield { argument, .. } => {
            if let Some(e) = argument {
                annotate_expr(e, ann, ctx);
            }
        }
        ExprKind::NonNull { expression } | ExprKind::Try { expression } => {
            annotate_expr(expression, ann, ctx)
        }
        ExprKind::Pipeline { left, right } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);
        }
        ExprKind::Range { start, end, .. } => {
            annotate_expr(start, ann, ctx);
            annotate_expr(end, ann, ctx);
        }
        ExprKind::Sequence { expressions } => {
            for e in expressions {
                annotate_expr(e, ann, ctx);
            }
        }
        _ => {}
    }
}

pub(crate) fn get_expr_type(expr: &Expr, ctx: &AnnotateCtx) -> Type {
    if !ctx.evolved_locals.is_empty() && ctx.is_overlay_governed(expr) {
        return infer_expr_type(expr, Some(ctx));
    }
    if let Some(ty) = ctx.resolved_expr_types.get(&expr.id) {
        ty.clone()
    } else {
        infer_expr_type(expr, Some(ctx))
    }
}

fn project_cg_ty(ty: &Type, ctx: &AnnotateCtx) -> varn_core::CgTy {
    use varn_core::CgTy;
    use varn_core::TypeTag;
    match &ty.0 {
        TypeKind::Intrinsic(TypeTag::Int) => CgTy::Int,
        TypeKind::Intrinsic(TypeTag::Float) => CgTy::Float,
        TypeKind::Intrinsic(TypeTag::Bool) => CgTy::Bool,
        TypeKind::Intrinsic(TypeTag::Str) | TypeKind::TemplateLiteral(_) => CgTy::Str,
        TypeKind::Intrinsic(TypeTag::Char) => CgTy::Char,
        TypeKind::Intrinsic(TypeTag::Decimal) => CgTy::Decimal,
        TypeKind::Intrinsic(TypeTag::BigInt) => CgTy::BigInt,
        TypeKind::Array(el) => CgTy::Array(Box::new(project_cg_ty(el, ctx))),
        TypeKind::Generic(name, args, _) => match (name.as_ref(), args.as_slice()) {
            ("Task", [ret]) => project_cg_ty(ret, ctx),
            _ => CgTy::Dynamic,
        },
        TypeKind::Named(name, origin) => match name.as_ref() {
            "Map" => CgTy::Map(Box::new(CgTy::Dynamic), Box::new(CgTy::Dynamic)),
            "Set" => CgTy::Set(Box::new(CgTy::Dynamic)),
            _ => {
                if ctx
                    .get_class_members(name, origin.as_ref().map(|o| o.as_ref()))
                    .is_some()
                {
                    CgTy::Class(name.clone())
                } else {
                    CgTy::Dynamic
                }
            }
        },
        TypeKind::Fn(_) => CgTy::Fn,
        TypeKind::Union(members) => {
            let non_null: Vec<&Type> = members.iter().filter(|m| !m.is_nullable()).collect();
            let has_null = non_null.len() < members.len();
            match (has_null, non_null.as_slice()) {
                (true, [single]) => {
                    let inner = project_cg_ty(single, ctx);
                    if inner == CgTy::Dynamic {
                        CgTy::Dynamic
                    } else {
                        CgTy::Nullable(Box::new(inner))
                    }
                }
                _ => CgTy::Dynamic,
            }
        }
        _ => CgTy::Dynamic,
    }
}

pub(crate) fn record_cg_ty_at(
    offset: u32,
    ty: &Type,
    ann: &mut TypeAnnotations,
    ctx: &AnnotateCtx,
) {
    let cg = project_cg_ty(ty, ctx);
    if cg != varn_core::CgTy::Dynamic {
        ann.record_cg_ty(offset, cg);
    }
}

fn core_class_of_type(ty: &Type) -> Option<&'static str> {
    use varn_core::op_id::core_class_name;
    use varn_core::TypeTag;
    match &ty.0 {
        TypeKind::Array(_) => core_class_name(TypeTag::Array),
        TypeKind::Intrinsic(TypeTag::BigInt) => None,
        TypeKind::Intrinsic(tag) => core_class_name(*tag),
        TypeKind::TemplateLiteral(_) => core_class_name(TypeTag::Str),
        TypeKind::Named(name, _) => varn_core::op_id::core_class(name.as_ref()),
        _ => None,
    }
}

fn core_has_method(bind: &crate::BindResult, class: &str, method: &str) -> bool {
    bind.core
        .as_ref()
        .and_then(|c| c.class_members.get(class))
        .map_or(false, |info| {
            info.members.iter().any(|m| {
                m.name.as_ref() == method
                    && matches!(m.kind, crate::types::ClassMemberKind::Method)
                    && !m.is_static
                    && !m.is_async
                    && !m.is_generator
            })
        })
}
