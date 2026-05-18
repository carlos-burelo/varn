use crate::binder::BindResult;
use crate::types::Type;

pub(super) fn is_async_member(obj_ty: &Type, prop_name: &str, bind: &BindResult) -> bool {
    if let Some(tn) = obj_ty.descriptor_key() {
        if let Some(method_map) = bind.extensions.methods.get(tn) {
            if let Some(mangled) = method_map.get(prop_name) {
                let scope = bind.scopes.get(bind.global_scope);
                if let Some(sid) = scope.resolve(mangled, &bind.scopes) {
                    return bind.arena.get(sid).is_async;
                }
            }
        }

        if bind
            .type_members
            .classes
            .get(tn)
            .and_then(|members| {
                members
                    .members
                    .iter()
                    .find(|m| m.name.as_ref() == prop_name)
            })
            .map(|m| m.is_async)
            .unwrap_or(false)
        {
            return true;
        }

        if bind
            .type_members
            .interfaces
            .get(tn)
            .and_then(|members| members.iter().find(|m| m.name.as_ref() == prop_name))
            .map(|m| m.is_async)
            .unwrap_or(false)
        {
            return true;
        }
    }

    false
}
