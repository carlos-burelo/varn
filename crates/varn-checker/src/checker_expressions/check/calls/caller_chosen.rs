use crate::checker::Checker;
use crate::generic_substitution::map_generics_cached;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use varn_core::TypeKind;
use varn_sem::types::{FunctionType, Type};

impl<'r> Checker<'r> {
    pub(super) fn returns_caller_chosen_type(
        &mut self,
        ft: &FunctionType,
        type_params: &[Arc<str>],
        mapping: &FxHashMap<Arc<str>, Type>,
    ) -> bool {
        let returned = Type::resolved(ft.return_type);
        if matches!(self.ty_table.get(returned.0), TypeKind::Generic(..)) {
            return false;
        }
        type_params.iter().any(|name| {
            mapping.get(name).is_some_and(|ty| !ty.is_dynamic())
                && self.mentions_type_param(returned, name)
                && !ft
                    .params
                    .iter()
                    .any(|p| self.mentions_type_param(Type::resolved(p.ty), name))
        })
    }

    fn mentions_type_param(&mut self, ty: Type, name: &Arc<str>) -> bool {
        let probe = FxHashMap::from_iter([(name.clone(), Type::Dynamic)]);
        map_generics_cached(self, &ty, &probe) != ty
    }
}
