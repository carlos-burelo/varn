use super::scalar::{array_element_type, literal_expr_admitted, plain_literal_matches};
use crate::types::numeric_literal::{const_int_value, int_literal_adopts};
use crate::types::{CheckerTyTable, ObjectTypeMember, Type};

pub fn expr_satisfies_target_type(
    target_ty: &Type,
    _init_ty: &Type,
    arena: &varn_core::ast::AstArena,
    expr: Option<varn_core::ast::ExprId>,
    table: &CheckerTyTable,
    interner: Option<&varn_core::AtomInterner>,
) -> bool {
    let Some(expr) = expr else {
        return false;
    };
    use varn_core::ast::{ArrayEl, ExprKind};
    use varn_core::TypeKind;
    let expr_kind = &arena.expr(expr).kind;
    if let ExprKind::Paren { expression } = expr_kind {
        return expr_satisfies_target_type(
            target_ty,
            _init_ty,
            arena,
            Some(*expression),
            table,
            interner,
        );
    }
    if let Some(value) = const_int_value(arena, expr) {
        if int_literal_adopts(target_ty, value, table) {
            return true;
        }
    }
    if literal_expr_admitted(target_ty, arena, expr, table, interner) {
        return true;
    }
    if let (Some(elem_ty), ExprKind::Array { elements }) =
        (array_element_type(target_ty, table, interner), expr_kind)
    {
        let literal_elem = matches!(
            table.get(elem_ty.0),
            TypeKind::Primitive(
                varn_core::LangPrimitive::Float
                    | varn_core::LangPrimitive::Decimal
                    | varn_core::LangPrimitive::BigInt
            )
        ) || array_element_type(&elem_ty, table, interner).is_some();
        if literal_elem && !elements.is_empty() {
            return elements.iter().all(|el| match el {
                ArrayEl::Expr(e) => {
                    expr_satisfies_target_type(&elem_ty, &elem_ty, arena, Some(*e), table, interner)
                }
                ArrayEl::Spread(_) | ArrayEl::Hole => false,
            });
        }
    }
    if let (TypeKind::Object(mid), ExprKind::Object { properties }) =
        (table.get(target_ty.0), expr_kind)
    {
        let members = table.get_object_members(mid);
        if properties.is_empty() {
            return false;
        }
        let mut present: Vec<&str> = Vec::with_capacity(properties.len());
        for prop in properties {
            let varn_core::ast::ObjectProp::Property { key, value, .. } = prop else {
                return false;
            };
            let key_str = match key {
                varn_core::ast::PropKey::Identifier(s) | varn_core::ast::PropKey::Str(s) => {
                    s.as_str()
                }
                varn_core::ast::PropKey::Int(_) | varn_core::ast::PropKey::Computed(_) => {
                    return false
                }
            };
            let matched = members.iter().any(|m| match m {
                ObjectTypeMember::Property { name, ty, .. } if name.as_ref() == key_str => {
                    let ty = Type::resolved(*ty);
                    expr_satisfies_target_type(&ty, &ty, arena, Some(*value), table, interner)
                        || plain_literal_matches(&ty, arena, *value, table)
                }
                ObjectTypeMember::Property { .. }
                | ObjectTypeMember::Method { .. }
                | ObjectTypeMember::Index { .. }
                | ObjectTypeMember::Callable { .. } => false,
            });
            if !matched {
                return false;
            }
            present.push(key_str);
        }
        let required_missing = members.iter().any(|m| match m {
            ObjectTypeMember::Property {
                name,
                optional: false,
                ..
            }
            | ObjectTypeMember::Method {
                name,
                optional: false,
                ..
            } => !present.contains(&name.as_ref()),
            ObjectTypeMember::Property { .. }
            | ObjectTypeMember::Method { .. }
            | ObjectTypeMember::Index { .. }
            | ObjectTypeMember::Callable { .. } => false,
        });
        return !required_missing;
    }
    false
}
