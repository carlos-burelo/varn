use crate::types::{Type, TypeContext};
use varn_core::{IntrinsicType, TypeKind};

use super::contexts::AliasSubstitutionContext;
use super::resolve_type_node;

pub(super) fn try_stdlib_generic_alias(
    name: &str,
    args: &[Type],
    ctx: Option<&dyn TypeContext>,
) -> Option<Type> {
    use crate::binder::BindResult;
    use std::cell::Cell;
    use std::cell::RefCell;
    use std::rc::Rc;

    thread_local! {
        static STD_TYPES: RefCell<Option<Rc<BindResult>>> = RefCell::new(None);
        static STD_TYPES_LOADING: Cell<bool> = Cell::new(false);
    }

    let needs_init = STD_TYPES.with(|c| c.borrow().is_none());
    if needs_init {
        let already_loading = STD_TYPES_LOADING.with(|flag| {
            if flag.get() {
                true
            } else {
                flag.set(true);
                false
            }
        });
        if already_loading {
            return None;
        }

        let resolved = crate::module_resolver::resolve_stdlib_module_bind_ref("std:types");
        STD_TYPES.with(|c| {
            *c.borrow_mut() = resolved;
        });
        STD_TYPES_LOADING.with(|flag| flag.set(false));
    }

    let bind_rc = STD_TYPES.with(|c| c.borrow().as_ref().map(Rc::clone))?;
    let bind = bind_rc.as_ref();

    let (params, alias_node) = bind.get_alias_node(name)?;
    if params.is_empty() || params.len() != args.len() {
        return None;
    }
    let alias_ctx = AliasSubstitutionContext {
        inner: ctx,
        params,
        args: args.to_vec(),
    };
    Some(resolve_type_node(&alias_node, Some(&alias_ctx)))
}

pub(super) fn is_primitive_type(ty: &Type) -> bool {
    matches!(
        &ty.0,
        TypeKind::Intrinsic(tag) if tag.is_primitive()
    )
}

pub fn resolve_primitive(name: &str, ctx: Option<&dyn TypeContext>) -> Type {
    use varn_core::TypeKind as K;
    if let Some(it) = IntrinsicType::from_str(name) {
        return Type(K::Intrinsic(it.0), false);
    }
    Type::named_with_origin(
        name.to_owned(),
        ctx.and_then(|c| c.source_file()).map(|s| s.to_owned()),
    )
}
