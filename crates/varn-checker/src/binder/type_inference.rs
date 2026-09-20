use std::sync::Arc;
use varn_core::ast::operators::{BinaryOp, LogicalOp, UnaryOp};
use varn_core::ast::{
    ArrayEl, ArrowBody, AstArena, ExprId, ExprKind, MatchBody, ObjectProp, PropKey,
};

use super::inference_utils::{build_fn_type, build_method_params, infer_object_member_type};
pub use super::inference_utils::{pattern_lead_name, pattern_to_string, widen_literal};
use super::type_resolution::resolve_type_node;
use crate::types::{CheckerTyTable, FunctionType, ObjectTypeMember, Type};
use varn_core::TypeKind;

/// Resolve `atom` to owned text without panicking when it was minted by a
/// sibling module after this context's interner snapshot: the context table
/// first, the resolver's live table as fallback, `None` when neither knows
/// it (callers degrade to `Dynamic`/no-match instead of crashing).
fn ctx_resolve_text(
    ctx: Option<&dyn crate::types::TypeContext>,
    atom: varn_core::Atom,
) -> Option<String> {
    let c = ctx?;
    if let Some(i) = c.interner() {
        if let Some(s) = i.try_resolve(atom) {
            return Some(s.to_string());
        }
    }
    c.resolver()?
        .interner_snapshot()
        .try_resolve(atom)
        .map(|s| s.to_string())
}

/// Decode a member type returned by `TypeContext::get_class_members` (and
/// friends) into `table` when it came from another module.
///
/// The member tables belong to the bind of the module that DECLARES the
/// class, so their `Type`s carry that module's `CheckerTyTable` ids. Reading
/// one against the current table answers with whatever shape sits at that
/// index here — observed as `Channel<T>.tx` typing as an unrelated function
/// and then `Sender<int>.send` "not existing". `CheckerTyTable::reintern`
/// walks and re-interns the foreign shape locally; portable ids and
/// unreachable/degenerate sources pass through unchanged.
fn reintern_member_type(
    ctx: Option<&dyn crate::types::TypeContext>,
    origin: Option<&str>,
    ty: Type,
    table: &mut CheckerTyTable,
) -> Type {
    let Some(ctx) = ctx else { return ty };
    if table.contains(ty.0) {
        return ty;
    }
    let Some(origin) = origin else { return ty };
    if ctx.source_file().is_some_and(|s| s == origin) {
        return ty;
    }
    let Some(resolver) = ctx.resolver() else {
        return ty;
    };
    let Some(b) = resolver
        .stdlib_bind(origin)
        .or_else(|| resolver.module_bind(origin))
    else {
        return ty;
    };
    if !b.ty_table.contains(ty.0) {
        return ty;
    }
    // Content-addressed ids need no translation: union the foreign shapes in
    // so `table` can resolve `ty`, then the id is already correct.
    table.absorb(&b.ty_table);
    if matches!(
        table.get(ty.0),
        varn_core::TypeKind::Named(_, None) | varn_core::TypeKind::Generic(_, _, None)
    ) {
        let origin = resolver.intern(b.source_file.as_ref());
        return ty.with_origin(origin, table);
    }
    ty
}

pub fn infer_expr_type(
    id: ExprId,
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    match &arena.expr(id).kind {
        ExprKind::IntLiteral { .. } => Type::Int,
        ExprKind::FloatLiteral { .. } => Type::Float,
        ExprKind::DecimalLiteral { .. } => Type::Decimal,
        ExprKind::BigIntLiteral { .. } => Type::BigInt,
        ExprKind::StrLiteral { .. } => Type::Str,
        ExprKind::CharLiteral { .. } => Type::Char,
        ExprKind::BoolLiteral { .. } => Type::Bool,
        ExprKind::NullLiteral => Type::Null,
        ExprKind::Template { .. } => Type::Str,
        ExprKind::Paren { expression } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::As { type_ann, .. } => resolve_type_node(type_ann, ctx, table),
        ExprKind::Is { .. } => Type::Bool,
        ExprKind::Satisfies { expression, .. } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::Await { argument } => {
            let inner = infer_expr_type(*argument, arena, ctx, table);
            match table.get(inner.0) {
                TypeKind::Generic(name, args, _)
                    if ctx.and_then(|c| c.interner()).is_some_and(|i| {
                        i.get(varn_core::IntrinsicType::Task.as_str()) == Some(name)
                    }) && table.get_list(args).len() == 1 =>
                {
                    Type(table.get_list(args)[0], false)
                }
                _ => inner,
            }
        }
        ExprKind::NonNull { expression } => infer_expr_type(*expression, arena, ctx, table),
        ExprKind::Try { expression } => {
            let inner = infer_expr_type(*expression, arena, ctx, table);
            let ok_first = match table.get(inner.0) {
                TypeKind::Generic(name, args, _)
                    if ctx_resolve_text(ctx, name)
                        .is_some_and(|n| n == "Result" || n == "Option") =>
                {
                    table.get_list(args).first().copied()
                }
                _ => None,
            };
            if let Some(id) = ok_first {
                Type(id, false)
            } else if inner.is_nullable(table) {
                inner.non_nullified(table)
            } else {
                inner
            }
        }
        ExprKind::Logical {
            op, left, right, ..
        } => match op {
            LogicalOp::Nullish => {
                let rhs = infer_expr_type(*right, arena, ctx, table);
                if !rhs.is_dynamic() {
                    return rhs;
                }
                infer_expr_type(*left, arena, ctx, table)
            }
            LogicalOp::And | LogicalOp::Or => {
                let l = infer_expr_type(*left, arena, ctx, table);
                let r = infer_expr_type(*right, arena, ctx, table);
                if l.0 == r.0 {
                    l
                } else {
                    Type::Dynamic
                }
            }
        },
        ExprKind::Member {
            object,
            property,
            computed,
            ..
        } => infer_member(*object, *property, *computed, arena, ctx, table),
        ExprKind::Unary { op, operand, .. } => {
            let inner = infer_expr_type(*operand, arena, ctx, table);
            match op {
                UnaryOp::Minus | UnaryOp::Plus => inner,
                UnaryOp::Not => Type::Bool,
                UnaryOp::BitNot => match table.get(inner.0) {
                    TypeKind::Intrinsic(varn_core::TypeTag::Int) => Type::Int,
                    _ => Type::Dynamic,
                },
                _ => Type::Dynamic,
            }
        }
        ExprKind::Binary {
            op, left, right, ..
        } => infer_binary(op, *left, *right, arena, ctx, table),
        ExprKind::Array { elements } => {
            for el in elements {
                if let ArrayEl::Expr(first) = el {
                    let elem_ty = infer_expr_type(*first, arena, ctx, table);
                    if !elem_ty.is_dynamic() {
                        let widened = widen_literal(elem_ty);
                        return Type::array(widened, table);
                    }
                }
            }
            Type::Dynamic
        }
        ExprKind::Call { callee, .. } => {
            let callee_ty = infer_expr_type(*callee, arena, ctx, table);
            if let TypeKind::Fn(fid) = table.get(callee_ty.0) {
                return Type(table.get_function(fid).return_type, false);
            }
            Type::Dynamic
        }
        ExprKind::New {
            callee, type_args, ..
        } => infer_new(*callee, type_args, arena, ctx, table),
        ExprKind::Identifier { name } => ctx
            .and_then(|c| c.interner().map(|i| (c, i)))
            .and_then(|(c, i)| c.resolve_symbol(i.resolve(*name)))
            .unwrap_or(Type::Dynamic),
        ExprKind::This => ctx
            .and_then(|c| c.resolve_symbol("this"))
            .unwrap_or(Type::This),
        ExprKind::Arrow {
            params,
            return_type,
            body,
            ..
        } => {
            let mut inferred_ret = Type::Dynamic;
            if let ArrowBody::Expr(e) = body.as_ref() {
                inferred_ret = infer_expr_type(*e, arena, ctx, table);
            }
            build_fn_type(params, return_type, true, ctx, table, inferred_ret)
        }
        ExprKind::Function {
            params,
            return_type,
            is_generator,
            ..
        } => {
            let ret = if *is_generator {
                crate::types::generator_of(
                    Type::Dynamic,
                    false,
                    table,
                    ctx.and_then(|c| c.resolver()),
                )
            } else {
                Type::Dynamic
            };
            build_fn_type(params, return_type, false, ctx, table, ret)
        }
        ExprKind::Match { cases, .. } => {
            let mut expr_arm_ty = None;
            for case in cases {
                match &case.body {
                    MatchBody::Expr(e) => {
                        let ty = infer_expr_type(*e, arena, ctx, table);
                        if !ty.is_dynamic() && !ty.is_never() {
                            expr_arm_ty = Some(ty);
                            break;
                        }
                    }
                    MatchBody::Block(_) => {}
                }
            }
            expr_arm_ty.unwrap_or(Type::Dynamic)
        }
        ExprKind::Object { properties } => infer_object(properties, arena, ctx, table),
        ExprKind::Range { .. } => Type::intrinsic(varn_core::TypeTag::Range, table),
        ExprKind::Pipeline { right, .. } => infer_expr_type(*right, arena, ctx, table),
        _ => Type::Dynamic,
    }
}

fn infer_member(
    object: ExprId,
    property: ExprId,
    computed: bool,
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let obj_ty = infer_expr_type(object, arena, ctx, table);
    if computed {
        return match table.get(obj_ty.0).clone() {
            TypeKind::Array(inner) => Type(inner, false),
            TypeKind::Intrinsic(varn_core::TypeTag::Str) => Type::Str,
            TypeKind::Named(name, _)
                if ctx.and_then(|c| c.interner()).is_some_and(|i| {
                    i.get(varn_core::IntrinsicType::Str.as_str()) == Some(name)
                }) =>
            {
                Type::Str
            }
            TypeKind::Generic(name, args, _)
                if ctx.and_then(|c| c.interner()).is_some_and(|i| {
                    i.get(varn_core::IntrinsicType::Map.as_str()) == Some(name)
                }) =>
            {
                let arg_ids = table.get_list(args).to_vec();
                if arg_ids.len() == 2 {
                    Type(arg_ids[1], false)
                } else if arg_ids.len() == 1 {
                    Type(arg_ids[0], false)
                } else {
                    Type::Dynamic
                }
            }
            TypeKind::Object(mid) => table
                .get_object_members(mid)
                .iter()
                .find_map(|m| match m {
                    crate::types::ObjectTypeMember::Index { value_ty, .. } => Some(*value_ty),
                    _ => None,
                })
                .map(|id| Type(id, false))
                .unwrap_or(Type::Dynamic),
            _ => Type::Dynamic,
        };
    }
    let prop_name_atom = match &arena.expr(property).kind {
        ExprKind::Identifier { name } => *name,
        _ => return Type::Dynamic,
    };

    if let Some(ctx) = ctx {
        let Some(interner) = ctx.interner() else {
            return Type::Dynamic;
        };
        let prop_name = interner.resolve(prop_name_atom);
        match table.get(obj_ty.0).clone() {
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => {
                let Some(name_str) = ctx_resolve_text(Some(ctx), name) else {
                    return Type::Dynamic;
                };
                let origin_str = origin.and_then(|o| ctx_resolve_text(Some(ctx), o));
                if let Some(variants) = ctx.get_enum_members(&name_str, origin_str.as_deref()) {
                    if prop_name == varn_core::MemberKey::RawValue.as_str()
                        || prop_name == varn_core::MemberKey::Tag.as_str()
                    {
                        return Type::Int;
                    }
                    if prop_name == varn_core::MemberKey::Name.as_str()
                        || prop_name == varn_core::MemberKey::VariantName.as_str()
                    {
                        return Type::Str;
                    }
                    let mut found_tys = Vec::new();
                    for v in &variants {
                        let vty =
                            reintern_member_type(Some(ctx), origin_str.as_deref(), v.ty, table);
                        if let TypeKind::Fn(fid) = table.get(vty.0) {
                            if let Some(p) = table.get_function(fid).params.iter().find(|p| {
                                p.name.as_ref().is_some_and(|pn| pn.as_ref() == prop_name)
                            }) {
                                found_tys.push(Type(p.ty, false));
                            }
                        }
                    }
                    if !found_tys.is_empty() {
                        return Type::union(found_tys, table);
                    }
                }
                if let Some(members) = ctx
                    .get_class_members(&name_str, origin_str.as_deref())
                    .or_else(|| ctx.get_interface_members(&name_str, origin_str.as_deref()))
                    .or_else(|| ctx.get_namespace_members(&name_str, origin_str.as_deref()))
                    .or_else(|| ctx.get_enum_members(&name_str, origin_str.as_deref()))
                {
                    if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                        return reintern_member_type(Some(ctx), origin_str.as_deref(), m.ty, table);
                    }
                }
            }
            TypeKind::Object(mid) => {
                for m in table.get_object_members(mid).to_vec() {
                    if let Some(ty) = infer_object_member_type(&m, prop_name, table) {
                        return ty;
                    }
                }
            }
            TypeKind::Array(inner) => {
                if prop_name == varn_core::MemberKey::Length.as_str() {
                    return Type::Int;
                }
                if prop_name == varn_core::MemberKey::Push.as_str() {
                    return Type::fn_(
                        FunctionType {
                            params: vec![crate::types::FunctionParam {
                                name: Some(Arc::from("item")),
                                ty: inner,
                                optional: false,
                                is_rest: false,
                            }],
                            return_type: Type::Int.0,
                            is_arrow: false,
                            type_params: vec![],
                        },
                        table,
                    );
                }
            }
            _ => {}
        }
    }
    Type::Dynamic
}

/// Numeric result type of a binary op, from the shared rules in
/// `varn_core::numeric`. `None` when the operands have no common numeric
/// class — the caller picks its own fallback.
pub(crate) fn numeric_binary_type(
    op: BinaryOp,
    l: &Type,
    r: &Type,
    table: &CheckerTyTable,
) -> Option<Type> {
    use varn_core::{binary_operand_kind, binary_result_kind, NumericOperand, TypeTag};
    let operand = |t: &Type| match table.get(t.0) {
        TypeKind::Intrinsic(
            TypeTag::Int
            | TypeTag::I8
            | TypeTag::I16
            | TypeTag::I32
            | TypeTag::U8
            | TypeTag::U16
            | TypeTag::U32
            | TypeTag::U64,
        ) => Some(NumericOperand::Int),
        TypeKind::Intrinsic(TypeTag::Float | TypeTag::F32) => Some(NumericOperand::Float),
        TypeKind::Intrinsic(TypeTag::Decimal) => Some(NumericOperand::Decimal),
        _ => None,
    };
    let kind = binary_operand_kind(operand(l), operand(r))?;
    Some(match binary_result_kind(op, kind) {
        NumericOperand::Int => Type::Int,
        NumericOperand::Float => Type::Float,
        NumericOperand::Decimal => Type::Decimal,
    })
}

fn infer_binary(
    op: &BinaryOp,
    left: ExprId,
    right: ExprId,
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    match op {
        BinaryOp::Add => {
            let l = infer_expr_type(left, arena, ctx, table);
            let r = infer_expr_type(right, arena, ctx, table);
            match (table.get(l.0), table.get(r.0)) {
                (TypeKind::Intrinsic(varn_core::TypeTag::Str), _)
                | (_, TypeKind::Intrinsic(varn_core::TypeTag::Str)) => Type::Str,
                _ => numeric_binary_type(*op, &l, &r, table).unwrap_or(Type::Dynamic),
            }
        }
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
            let l = infer_expr_type(left, arena, ctx, table);
            let r = infer_expr_type(right, arena, ctx, table);
            numeric_binary_type(*op, &l, &r, table).unwrap_or(Type::Dynamic)
        }
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::Lt
        | BinaryOp::Gt
        | BinaryOp::LtEq
        | BinaryOp::GtEq
        | BinaryOp::Instanceof
        | BinaryOp::In => Type::Bool,
        BinaryOp::BitAnd
        | BinaryOp::BitOr
        | BinaryOp::BitXor
        | BinaryOp::Shl
        | BinaryOp::Shr
        | BinaryOp::UShr => {
            let l = infer_expr_type(left, arena, ctx, table);
            let r = infer_expr_type(right, arena, ctx, table);
            match (table.get(l.0), table.get(r.0)) {
                (
                    TypeKind::Intrinsic(varn_core::TypeTag::Int),
                    TypeKind::Intrinsic(varn_core::TypeTag::Int),
                ) => Type::Int,
                _ => Type::Dynamic,
            }
        }
    }
}

fn infer_new(
    callee: ExprId,
    type_args: &[varn_core::ast::TypeNode],
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    if let ExprKind::Identifier { name } = &arena.expr(callee).kind {
        let name_str = ctx
            .and_then(|c| c.interner())
            .map(|i| i.resolve(*name).to_owned())
            .unwrap_or_default();
        let resolver = ctx.and_then(|c| c.resolver());
        let origin = ctx
            .and_then(|c| c.source_file())
            .and_then(|s| resolver.map(|r| r.intern(s)));
        if type_args.is_empty() {
            if name_str == varn_core::IntrinsicType::Map.as_str() {
                return Type::generic_atom(*name, vec![Type::Dynamic], origin, table);
            }
            return Type::named_with_origin_atom(*name, origin, table);
        }
        let args = type_args
            .iter()
            .map(|m| resolve_type_node(m, ctx, table))
            .collect();
        return Type::generic_atom(*name, args, origin, table);
    }
    if let ExprKind::Member { .. } = &arena.expr(callee).kind {
        let callee_ty = infer_expr_type(callee, arena, ctx, table);
        match table.get(callee_ty.0).clone() {
            TypeKind::Named(name, origin) => {
                return Type::named_with_origin_atom(name, origin, table);
            }
            TypeKind::Generic(name, args, origin) => {
                let arg_ids = table.get_list(args).to_vec();
                let arg_tys: Vec<Type> = arg_ids.into_iter().map(|id| Type(id, false)).collect();
                return Type::generic_atom(name, arg_tys, origin, table);
            }
            _ => {}
        }
    }
    Type::Dynamic
}

fn infer_object(
    properties: &[ObjectProp],
    arena: &AstArena,
    ctx: Option<&dyn crate::types::TypeContext>,
    table: &mut CheckerTyTable,
) -> Type {
    let mut members = Vec::new();
    for p in properties {
        match p {
            ObjectProp::Property { key, value, .. } => {
                let value = *value;
                if matches!(key, PropKey::Computed(_)) {
                    let val_ty = infer_expr_type(value, arena, ctx, table);
                    members.push(ObjectTypeMember::Index {
                        param_name: Arc::from("_key"),
                        key_ty: Type::Str.0,
                        value_ty: val_ty.0,
                    });
                    continue;
                }
                let name = match key {
                    PropKey::Identifier(n) | PropKey::Str(n) => Arc::from(n.as_str()),
                    _ => continue,
                };
                let ty = infer_expr_type(value, arena, ctx, table);
                if let TypeKind::Fn(fid) = table.get(ty.0) {
                    let ft = table.get_function(fid).clone();
                    members.push(ObjectTypeMember::Method {
                        name,
                        params: ft.params.clone(),
                        return_type: ft.return_type,
                        optional: false,
                        is_arrow: ft.is_arrow,
                    });
                } else {
                    members.push(ObjectTypeMember::Property {
                        name,
                        ty: ty.0,
                        optional: false,
                        readonly: false,
                    });
                }
            }
            ObjectProp::Method {
                key,
                params,
                return_type,
                ..
            } => {
                let name = match key {
                    PropKey::Identifier(n) | PropKey::Str(n) => Arc::from(n.as_str()),
                    _ => continue,
                };
                let ps = build_method_params(params, ctx, table);
                let ret = return_type
                    .as_ref()
                    .map(|m| resolve_type_node(m, ctx, table))
                    .unwrap_or(Type::Dynamic);
                members.push(ObjectTypeMember::Method {
                    name,
                    params: ps,
                    return_type: ret.0,
                    optional: false,
                    is_arrow: false,
                });
            }
            ObjectProp::Spread { argument, .. } => {
                let spread_ty = infer_expr_type(*argument, arena, ctx, table);
                if let TypeKind::Object(mid) = table.get(spread_ty.0) {
                    members.extend(table.get_object_members(mid).to_vec());
                }
            }
            _ => {}
        }
    }
    Type::object(members, table)
}
