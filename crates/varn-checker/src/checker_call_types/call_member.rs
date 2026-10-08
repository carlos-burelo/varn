use super::CallTypeCtx;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::{Atom, TypeKind};
use varn_sem::types::Type;

fn text_of(c: &CallTypeCtx, atom: Atom) -> Option<String> {
    c.ctx
        .and_then(|x| x.atom_text(atom))
        .or_else(|| c.table.name(atom).map(str::to_owned))
}

pub(super) fn infer_member(c: &mut CallTypeCtx, object: ExprId, property: ExprId) -> Option<Type> {
    let arena = c.ast_arena;
    let interner = c.interner;
    let prop_name = match &arena.expr(property).kind {
        ExprKind::Identifier { name } => interner.resolve(*name),
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
        | ExprKind::MetaAccess { .. } => return None,
    };
    let obj_ty = c.infer(object)?;
    let obj_kind = c.table.get(obj_ty.0);
    let (class_name, origin): (Option<String>, Option<String>) = match obj_kind {
        TypeKind::Named(n, origin) => (text_of(c, n), origin.and_then(|o| text_of(c, o))),
        TypeKind::Generic(name, _, origin) => {
            (text_of(c, name), origin.and_then(|o| text_of(c, o)))
        }
        TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::Literal(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. }
        | TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::EnumVariant { .. }
        | TypeKind::TypePredicate { .. } => (Some(obj_ty.stdlib_key(c.table)?.to_string()), None),
    };
    let (Some(class_name), origin) = (class_name, origin) else {
        return None;
    };

    if let Some(ctx) = c.ctx {
        if let Some(members) = ctx.get_class_members(&class_name, origin.as_deref()) {
            if let Some(m) = members.iter().find(|m| m.name.as_ref() == prop_name) {
                return Some(m.ty);
            }
        }
        if let Some(ext_ty) = ctx.get_extension_method(&class_name, prop_name) {
            return Some(ext_ty);
        }
    }
    if let Some(methods) = c.class_methods.get(class_name.as_str()) {
        if let Some(ty) = methods.get(prop_name) {
            return Some(*ty);
        }
    }
    None
}
