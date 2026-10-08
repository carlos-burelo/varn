use varn_core::ast::{TypeNode, TypeParam};
use varn_sem::types::{CheckerTyTable, FunctionType, Type, TypeContext};

use super::names::resolve_atom_name;
use super::resolve_type_node;

pub(super) fn resolve_fn_type(
    params: &[TypeParam],
    ret: &TypeNode,
    ctx: Option<&dyn TypeContext>,
    interner: &varn_core::AtomInterner,
    table: &mut CheckerTyTable,
) -> Type {
    let resolved_params = params
        .iter()
        .map(|p| {
            let ty = p
                .constraint
                .as_ref()
                .map(|m| resolve_type_node(m, ctx, table))
                .unwrap_or(Type::Dynamic);
            varn_sem::types::FunctionParam {
                name: Some(resolve_atom_name(p.name, ctx, interner)),
                ty: ty.0,
                optional: false,
                is_rest: false,
            }
        })
        .collect();
    let ret_ty = resolve_type_node(ret, ctx, table);
    Type::fn_(
        FunctionType {
            params: resolved_params,
            return_type: ret_ty.0,
            is_arrow: false,
            type_params: vec![],
        },
        table,
    )
}
