use super::stmts::annotate_stmt;
use super::AnnotateCtx;
use crate::binder::resolve_type_node;
use crate::types::{Type, TypeContext};
use varn_core::ast::operators::{BinaryOp, UnaryOp};
use varn_core::ast::{Arg, ArrayEl, Expr, ExprKind};
use varn_core::intrinsic_ops::intrinsic_lookup;
use varn_core::AnnKey;
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
                ann.record_numeric(AnnKey::expr(expr.id), k);
            }
        }
        ExprKind::Paren { expression } => annotate_expr(expression, ann, ctx),
        ExprKind::Unary { op, operand, .. } => {
            annotate_expr(operand, ann, ctx);
            let un_ty = get_expr_type(operand, ctx);
            use varn_core::TypeTag;
            match (&un_ty.non_nullified().0, op) {
                (
                    TypeKind::Intrinsic(TypeTag::Int),
                    UnaryOp::Minus | UnaryOp::Plus | UnaryOp::BitNot,
                ) => {
                    ann.record_numeric(AnnKey::expr(expr.id), NumericKind::Int);
                }
                (TypeKind::Intrinsic(TypeTag::Float), UnaryOp::Minus | UnaryOp::Plus) => {
                    ann.record_numeric(AnnKey::expr(expr.id), NumericKind::Float);
                }
                _ => {}
            }
        }
        ExprKind::MetaAccess { target, .. } => annotate_expr(target, ann, ctx),
        ExprKind::Logical { left, right, .. } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
        }
        ExprKind::Assign { value, target, .. } => {
            annotate_expr(target, ann, ctx);
            annotate_expr(value, ann, ctx);
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
        }
        ExprKind::Conditional {
            test,
            consequent,
            alternate,
        } => {
            annotate_expr(test, ann, ctx);
            annotate_expr(consequent, ann, ctx);
            annotate_expr(alternate, ann, ctx);
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
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
                    let prop_key = AnnKey::expr(property.id);
                    if let TypeKind::Named(_, Some(ref origin_path)) = &obj_ty_nn.0 {
                        let key = format!("{}/{}", origin_path, prop_name);
                        if let Some(wire_byte) = intrinsic_lookup(&key) {
                            ann.record_intrinsic(prop_key, wire_byte);
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
                                    ann.record_intrinsic(prop_key, wire_byte);
                                } else if core_has_method(ctx.bind, class, prop_name) {
                                    ann.record_native_op(
                                        prop_key,
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
                        ann.record_intrinsic(AnnKey::expr(callee.id), wire_byte);
                    }
                }
            }

            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
            if let ExprKind::Member {
                property,
                computed: false,
                ..
            } = &callee.kind
            {
                record_cg_ty_at(AnnKey::expr(property.id), &result_ty, ann, ctx);
            }
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
                    ann.record_array_index(AnnKey::expr(expr.id));
                }
                let elem_ty = get_expr_type(expr, ctx);
                record_cg_ty_at(AnnKey::expr(property.id), &elem_ty, ann, ctx);
                record_cg_ty_at(AnnKey::expr(expr.id), &elem_ty, ann, ctx);
            } else if let ExprKind::Identifier { name: prop_name } = &property.kind {
                let obj_ty = get_expr_type(object, ctx);
                let check_ty = obj_ty.non_nullified();
                let member_ty = get_expr_type(expr, ctx);
                record_cg_ty_at(AnnKey::expr(property.id), &member_ty, ann, ctx);

                let class_name = match &check_ty.0 {
                    TypeKind::Named(n, _origin) | TypeKind::Generic(n, _, _origin) => {
                        Some(n.as_ref())
                    }
                    _ => None,
                };
                if let Some(cn) = class_name {
                    if ctx.bind.is_user_class(cn) {
                        let mut hierarchy = Vec::new();
                        let mut current: Option<std::rc::Rc<str>> = Some(std::rc::Rc::from(cn));
                        while let Some(c) = current {
                            hierarchy.push(c.clone());
                            current = ctx.bind.class_parents.get(c.as_ref()).cloned();
                        }
                        hierarchy.reverse();

                        if !hierarchy.iter().all(|c| ctx.bind.is_user_class(c.as_ref())) {
                            return;
                        }

                        let mut slot = 0u16;
                        let mut cur_offset = 0u32;
                        let mut known_props: rustc_hash::FxHashSet<std::rc::Rc<str>> =
                            rustc_hash::FxHashSet::default();
                        let mut found = false;

                        for cls in &hierarchy {
                            if let Some(entry) = ctx.bind.get_class_entry(cls) {
                                for m in &entry.members {
                                    if !m.is_static
                                        && (m.kind == crate::types::ClassMemberKind::Property
                                            || m.kind == crate::types::ClassMemberKind::Variable)
                                        && known_props.insert(m.name.clone())
                                    {
                                        let tag = m.ty.to_type_tag();
                                        let (size, align) = match tag {
                                            varn_core::TypeTag::Bool => (1u32, 1u32),
                                            varn_core::TypeTag::Char => (4u32, 4u32),
                                            varn_core::TypeTag::Int | varn_core::TypeTag::Float => (8u32, 8u32),
                                            varn_core::TypeTag::Str
                                            | varn_core::TypeTag::Array
                                            | varn_core::TypeTag::Map
                                            | varn_core::TypeTag::Set
                                            | varn_core::TypeTag::Object
                                            | varn_core::TypeTag::Class
                                            | varn_core::TypeTag::Function
                                            | varn_core::TypeTag::Task
                                            | varn_core::TypeTag::Generator => (8u32, 8u32),
                                            _ => (16u32, 8u32),
                                        };
                                        let padding = (align - (cur_offset % align)) % align;
                                        cur_offset += padding;
                                        let offset = cur_offset;
                                        cur_offset += size;

                                        if m.name.as_ref() == prop_name.as_ref() {
                                            ann.record_fixed_field_layout(
                                                AnnKey::expr(property.id),
                                                slot,
                                                offset,
                                                tag,
                                            );
                                            found = true;
                                            break;
                                        }
                                        slot += 1;
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
        ExprKind::As { expression, .. } => {
            annotate_expr(expression, ann, ctx);
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
        }
        ExprKind::Satisfies { expression, .. } => {
            annotate_expr(expression, ann, ctx);
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
            // `new C(...)` is a `C`. This is the one expression whose type
            // needs no inference at all — the word `new` says it — and it went
            // unrecorded, so every constructed object reached the backend as
            // `Dynamic` and every field read on it had to re-prove the class
            // at run time.
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
        }
        ExprKind::Template { parts } => {
            for part in parts {
                if let varn_core::ast::TemplatePart::Interpolation(e) = part {
                    annotate_expr(e, ann, ctx);
                }
            }
        }
        ExprKind::Object { properties } | ExprKind::Record { properties } => {
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
        ExprKind::Function { params, body, .. } => {
            let mut local_ctx = ctx.clone();
            for p in params {
                let name_opt = match &p.pattern {
                    varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = name_opt {
                    let type_ann = p.type_ann.as_ref().or(match &p.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    let ty = if let Some(ann_node) = type_ann {
                        resolve_type_node(ann_node, Some(ctx))
                    } else {
                        Type::Dynamic
                    };
                    record_cg_ty_at(AnnKey::decl(p.range.start.offset), &ty, ann, ctx);
                    local_ctx.locals.insert(name, ty);
                }
            }
            annotate_stmt(body, ann, &mut local_ctx);
        }
        ExprKind::Arrow { params, body, .. } => {
            let mut local_ctx = ctx.clone();
            for p in params {
                let name_opt = match &p.pattern {
                    varn_core::ast::Pattern::Identifier { name, .. } => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = name_opt {
                    let type_ann = p.type_ann.as_ref().or(match &p.pattern {
                        varn_core::ast::Pattern::Identifier { type_ann, .. } => type_ann.as_ref(),
                        _ => None,
                    });
                    let ty = if let Some(ann_node) = type_ann {
                        resolve_type_node(ann_node, Some(ctx))
                    } else {
                        Type::Dynamic
                    };
                    record_cg_ty_at(AnnKey::decl(p.range.start.offset), &ty, ann, ctx);
                    local_ctx.locals.insert(name, ty);
                }
            }
            match body.as_ref() {
                varn_core::ast::ArrowBody::Expr(e) => annotate_expr(e, ann, &mut local_ctx),
                varn_core::ast::ArrowBody::Block(b) => annotate_stmt(b, ann, &mut local_ctx),
            }
        }
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
            let res_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &res_ty, ann, ctx);
        }
        ExprKind::Is { expression, .. } => {
            annotate_expr(expression, ann, ctx);
            let bool_ty = Type(TypeKind::Intrinsic(varn_core::TypeTag::Bool), false);
            record_cg_ty_at(AnnKey::expr(expr.id), &bool_ty, ann, ctx);
        }
        ExprKind::Update { operand, .. } => {
            annotate_expr(operand, ann, ctx);
            let op_ty = get_expr_type(operand, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &op_ty, ann, ctx);
            use varn_core::{NumericKind, TypeTag};
            let k = match &op_ty.0 {
                TypeKind::Intrinsic(TypeTag::Int) => Some(NumericKind::Int),
                TypeKind::Intrinsic(TypeTag::Float) => Some(NumericKind::Float),
                TypeKind::Intrinsic(TypeTag::Decimal) => Some(NumericKind::Decimal),
                _ => None,
            };
            if let Some(k) = k {
                ann.record_numeric(AnnKey::expr(expr.id), k);
            }
        }
        ExprKind::Await { argument } => {
            annotate_expr(argument, ann, ctx);
            // The checker already unwrapped the task: this is `T`, not
            // `Task<T>`. Recording it here is what replaces the projection
            // shortcut removed from `project_cg_ty`, and it puts the type on
            // the expression that actually produces the value.
            let result_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &result_ty, ann, ctx);
        }
        ExprKind::Spawn { argument } | ExprKind::Spread { argument } => {
            annotate_expr(argument, ann, ctx)
        }
        ExprKind::Yield {
            argument: Some(e), ..
        } => {
            annotate_expr(e, ann, ctx);
        }
        ExprKind::Yield { argument: None, .. } => {}
        ExprKind::NonNull { expression } => {
            annotate_expr(expression, ann, ctx);
            let res_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &res_ty, ann, ctx);
        }
        ExprKind::Try { expression } => annotate_expr(expression, ann, ctx),
        ExprKind::Pipeline { left, right } => {
            annotate_expr(left, ann, ctx);
            annotate_expr(right, ann, ctx);
            let res_ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &res_ty, ann, ctx);
        }
        ExprKind::Range { start, end, .. } => {
            annotate_expr(start, ann, ctx);
            annotate_expr(end, ann, ctx);
        }
        ExprKind::Sequence { expressions } => {
            for e in expressions {
                annotate_expr(e, ann, ctx);
            }
            if let Some(last) = expressions.last() {
                let last_ty = get_expr_type(last, ctx);
                record_cg_ty_at(AnnKey::expr(expr.id), &last_ty, ann, ctx);
            }
        }
        ExprKind::Identifier { .. } => {
            let ty = get_expr_type(expr, ctx);
            record_cg_ty_at(AnnKey::expr(expr.id), &ty, ann, ctx);
        }
        _ => {}
    }
}

/// The type CODEGEN should compile `expr` against: the checker's proof when
/// it has one, its checked type otherwise.
///
/// A lookup. It used to be a three-way choice between two inference engines:
///
/// ```text
/// if overlay-governed      -> binder::infer_expr_type   (second engine)
/// else if in the table     -> the checker's answer
/// else                     -> binder::infer_expr_type   (second engine again)
/// ```
///
/// The third arm was measured over all 74 corpus files and fired zero times.
/// The first is now `TypeEntry::refined`, proved once by `checker::refine`
/// instead of re-derived here over an overlay environment. Everything the
/// backend compiles against therefore comes from the pass that checked the
/// program.
///
/// A missing entry is a BUG IN THE CHECKER, and is treated as one.
/// `check_expr` records every expression it visits, so the annotations pass
/// asking about a node the table does not have means the two disagree about
/// which nodes exist. Answering `Dynamic` there would be a silent precision
/// loss, and precision is the product: `Dynamic` is exactly what stops the
/// backend emitting `AddInt`, an unboxed array read, a fixed-field slot. So
/// the miss trips a `debug_assert` — every `cargo test` and every debug build
/// fails on it — and only the release path degrades.
pub(crate) fn get_expr_type(expr: &Expr, ctx: &AnnotateCtx) -> Type {
    match ctx.expr_table.get(&expr.id) {
        Some(entry) => entry.refined.clone().unwrap_or_else(|| entry.ty.clone()),
        None => {
            debug_assert!(
                false,
                "checker did not type expression {} ({:?}) — the annotations pass                  walks a node `check_expr` never visited, so codegen would lose                  its type. Fix the checker's traversal, do not widen to dynamic.",
                expr.id, expr.range
            );
            Type::Dynamic
        }
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
        TypeKind::Generic(name, args, origin) => {
            if name.as_ref() == varn_core::IntrinsicType::Array.as_str() {
                if let [el] = args.as_slice() {
                    return CgTy::Array(Box::new(project_cg_ty(el, ctx)));
                }
            } else if name.as_ref() == varn_core::IntrinsicType::Map.as_str() {
                if let [k, v] = args.as_slice() {
                    return CgTy::Map(
                        Box::new(project_cg_ty(k, ctx)),
                        Box::new(project_cg_ty(v, ctx)),
                    );
                }
            } else if name.as_ref() == varn_core::IntrinsicType::Set.as_str() {
                if let [el] = args.as_slice() {
                    return CgTy::Set(Box::new(project_cg_ty(el, ctx)));
                }
            } else if ctx
                .get_class_members(name, origin.as_ref().map(|o| o.as_ref()))
                .is_some()
                || ctx.bind.get_enum_members_local(name).is_some()
                || ctx.bind.is_user_class(name.as_ref())
            {
                return CgTy::Class(name.clone());
            }
            CgTy::Dynamic
        }
        TypeKind::Named(name, origin) => match varn_core::IntrinsicType::from_str(name.as_ref()) {
            Some(varn_core::IntrinsicType::Map) => {
                CgTy::Map(Box::new(CgTy::Dynamic), Box::new(CgTy::Dynamic))
            }
            Some(varn_core::IntrinsicType::Set) => CgTy::Set(Box::new(CgTy::Dynamic)),
            _ => {
                if ctx
                    .get_class_members(name, origin.as_ref().map(|o| o.as_ref()))
                    .is_some()
                    || ctx.bind.get_enum_members_local(name).is_some()
                    || ctx.bind.is_user_class(name.as_ref())
                {
                    CgTy::Class(name.clone())
                } else if ctx.bind.get_class_entry(name).is_some()
                    || ctx.bind.get_namespace_members_local(name).is_some()
                    || ctx.bind.get_interface_members_local(name).is_some()
                {
                    CgTy::Fn
                } else {
                    CgTy::Dynamic
                }
            }
        },
        TypeKind::EnumVariant { enum_name, .. } => CgTy::Class(enum_name.clone()),
        TypeKind::Fn(_) | TypeKind::Typeof(_) | TypeKind::Object(_) => CgTy::Fn,
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
    key: AnnKey,
    ty: &Type,
    ann: &mut TypeAnnotations,
    ctx: &AnnotateCtx,
) {
    let cg = project_cg_ty(ty, ctx);
    if cg != varn_core::CgTy::Dynamic {
        ann.record_cg_ty(key, cg);
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
        .is_some_and(|info| {
            info.members.iter().any(|m| {
                m.name.as_ref() == method
                    && matches!(m.kind, crate::types::ClassMemberKind::Method)
                    && !m.is_static
                    && !m.is_async
                    && !m.is_generator
            })
        })
}
