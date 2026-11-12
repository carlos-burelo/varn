use super::resolve::resolve_atom;
use crate::bind::BindView;
use crate::symbol::SymbolKind;
use crate::types::{
    CheckerTyTable, ClassMemberInfo, ClassMemberKind, ObjectTypeMember, TyListId, Type, TypeContext,
};
use rustc_hash::FxHashSet;
use vn_core::{Atom, LangConstraint, LangPrimitive, TypeKind};

pub(super) fn constraint_shortcut(
    declared: &Type,
    inferred: &Type,
    bind: Option<&BindView>,
    table: &CheckerTyTable,
) -> Option<bool> {
    match declared.lang_constraint(table)? {
        LangConstraint::Hashable => Some(is_hashable(inferred, bind, table)),
    }
}

pub fn is_hashable(ty: &Type, bind: Option<&BindView>, table: &CheckerTyTable) -> bool {
    hashable_in(ty, bind, &mut FxHashSet::default(), table)
}

const fn primitive_is_hashable(primitive: LangPrimitive) -> bool {
    match primitive {
        LangPrimitive::Null
        | LangPrimitive::Bool
        | LangPrimitive::Int
        | LangPrimitive::Float
        | LangPrimitive::BigInt
        | LangPrimitive::Decimal
        | LangPrimitive::Char
        | LangPrimitive::Str
        | LangPrimitive::Never
        | LangPrimitive::Error => true,
        LangPrimitive::Void | LangPrimitive::Dynamic => false,
    }
}

fn hashable_in(
    ty: &Type,
    bind: Option<&BindView>,
    visiting: &mut FxHashSet<(usize, Type)>,
    table: &CheckerTyTable,
) -> bool {
    let visit = (std::ptr::from_ref(table) as usize, *ty);
    if !visiting.insert(visit) {
        return true;
    }
    let result = hashable_kind(ty, bind, visiting, table);
    visiting.remove(&visit);
    result
}

fn all_hashable(
    list: TyListId,
    bind: Option<&BindView>,
    visiting: &mut FxHashSet<(usize, Type)>,
    table: &CheckerTyTable,
) -> bool {
    table
        .get_list(list)
        .iter()
        .all(|id| hashable_in(&Type::resolved(*id), bind, visiting, table))
}

fn hashable_kind(
    ty: &Type,
    bind: Option<&BindView>,
    visiting: &mut FxHashSet<(usize, Type)>,
    table: &CheckerTyTable,
) -> bool {
    match table.get(ty.0) {
        TypeKind::Primitive(primitive) => primitive_is_hashable(primitive),
        TypeKind::Literal(literal) => primitive_is_hashable(literal.base()),
        TypeKind::TemplateLiteral(_) => true,
        TypeKind::EnumVariant {
            type_args,
            payload_ty,
            ..
        } => {
            all_hashable(type_args, bind, visiting, table)
                && hashable_in(&Type::resolved(payload_ty), bind, visiting, table)
        }
        TypeKind::Tuple(list) | TypeKind::Union(list) | TypeKind::Intersection(list) => {
            all_hashable(list, bind, visiting, table)
        }
        TypeKind::Record(members) => {
            table
                .get_object_members(members)
                .iter()
                .all(|member| match member {
                    ObjectTypeMember::Property { ty, .. } => {
                        hashable_in(&Type::resolved(*ty), bind, visiting, table)
                    }
                    ObjectTypeMember::Method { .. } | ObjectTypeMember::Callable { .. } => false,
                })
        }
        TypeKind::Array(_)
        | TypeKind::Fn(_)
        | TypeKind::Interface(_)
        | TypeKind::This
        | TypeKind::TypePredicate { .. } => false,
        TypeKind::Conditional { .. }
        | TypeKind::Infer(_)
        | TypeKind::Typeof(_)
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Mapped { .. } => true,
        TypeKind::Named(name, origin) => {
            nominal_is_hashable(name, origin, &[], bind, visiting, table)
        }
        TypeKind::Generic(name, args, origin) => {
            let args: Vec<Type> = table
                .get_list(args)
                .iter()
                .map(|id| Type::resolved(*id))
                .collect();
            nominal_is_hashable(name, origin, &args, bind, visiting, table)
        }
    }
}

fn nominal_is_hashable(
    name: Atom,
    origin: Option<Atom>,
    args: &[Type],
    bind: Option<&BindView>,
    visiting: &mut FxHashSet<(usize, Type)>,
    table: &CheckerTyTable,
) -> bool {
    if vn_core::BuiltinType::identify(name, origin).is_some()
        || LangConstraint::identify(name, origin).is_some()
    {
        return false;
    }
    let Some(bind) = bind else {
        return true;
    };
    let Some(name_str) = resolve_atom(Some(bind), name) else {
        return true;
    };
    let origin_str = origin.and_then(|o| resolve_atom(Some(bind), o));
    let origin_str = origin_str.as_deref();

    if origin.is_none() && bind.symbol_kind(&name_str) == Some(SymbolKind::TypeParameter) {
        return bind.type_param_constraint(&name_str).is_some_and(|bound| {
            bound.lang_constraint(table) == Some(LangConstraint::Hashable)
                || hashable_in(&bound, Some(bind), visiting, table)
        });
    }
    let owner = bind.foreign(origin_str);
    let owner_view = owner
        .as_ref()
        .map(|owning_bind| BindView::new(owning_bind, bind.resolver));
    let member_view = owner_view.as_ref().unwrap_or(bind);
    let member_table: &CheckerTyTable = owner
        .as_ref()
        .map_or(table, |owning_bind| &*owning_bind.ty_table);

    let member_types: Vec<Type> =
        if let Some(variants) = member_view.get_enum_members(&name_str, origin_str) {
            variants
                .iter()
                .filter(|variant| variant.kind == ClassMemberKind::Property)
                .flat_map(|variant| variant_payload_types(variant, member_table))
                .collect()
        } else if let Some(members) = member_view.get_class_members(&name_str, origin_str) {
            let is_struct = member_view
                .class_entry(&name_str, origin_str)
                .is_some_and(|entry| entry.is_struct);
            if !is_struct {
                return true;
            }
            members
                .iter()
                .filter(|member| !member.is_static && member.kind == ClassMemberKind::Property)
                .map(|member| member.ty)
                .collect()
        } else {
            return member_view
                .get_interface_members(&name_str, origin_str)
                .is_none();
        };
    if !args
        .iter()
        .all(|arg| hashable_in(arg, Some(bind), visiting, table))
    {
        return false;
    }
    let params = member_view
        .type_param_names(&name_str, origin_str)
        .unwrap_or_default();
    let names_a_parameter = |member_ty: &Type| {
        let TypeKind::Named(member_name, None) = member_table.get(member_ty.0) else {
            return false;
        };
        resolve_atom(Some(member_view), member_name)
            .is_some_and(|text| params.iter().any(|param| &**param == text.as_str()))
    };
    member_types.iter().all(|member_ty| {
        names_a_parameter(member_ty)
            || hashable_in(member_ty, Some(member_view), visiting, member_table)
    })
}

fn variant_payload_types(variant: &ClassMemberInfo, table: &CheckerTyTable) -> Vec<Type> {
    let TypeKind::Fn(constructor) = table.get(variant.ty.0) else {
        return Vec::new();
    };
    table
        .get_function(constructor)
        .params
        .iter()
        .map(|param| Type::resolved(param.ty))
        .collect()
}
