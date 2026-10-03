use crate::binder::BindResult;
use crate::checker::Checker;
use crate::symbol::SymbolId;
use crate::types::Type;
use varn_core::Atom;

impl<'r> Checker<'r> {
    pub(crate) fn narrow_identifier(
        &mut self,
        name: Atom,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let scope = bind.scopes.get(self.current_scope);
        if let Some(id) = scope.resolve(name, &bind.scopes) {
            let original_ty = self
                .symbol_types
                .get(&id)
                .cloned()
                .or_else(|| bind.arena.get(id).ty);
            if let Some(ty) = original_ty {
                if is_true_branch {
                    let narrowed =
                        ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    if !narrowed.is_dynamic() && narrowed != ty {
                        out.push((id, narrowed));
                    }
                } else if ty.is_nullable(&self.ty_table) {
                    out.push((id, Type::Null));
                }
            }
        }
    }
}
