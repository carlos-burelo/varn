use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_core::Atom;
use varn_sem::bind::BindResult;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn narrowing_base(
        &self,
        rec: &Recorder,
        id: SymbolId,
        bind: &BindResult,
    ) -> Option<Type> {
        self.narrowed_types
            .get(&id)
            .and_then(|stack| stack.last().copied())
            .or_else(|| rec.symbol_types.get(&id).cloned())
            .or_else(|| bind.arena.get(id).ty)
    }

    pub(crate) fn narrow_identifier(
        &mut self,
        rec: &mut Recorder,
        name: Atom,
        bind: &BindResult,
        is_true_branch: bool,
        out: &mut Vec<(SymbolId, Type)>,
    ) {
        let scope = bind.scopes.get(self.current_scope);
        if let Some(id) = scope.resolve(name, &bind.scopes) {
            let original_ty = self.narrowing_base(rec, id, bind);
            if let Some(ty) = original_ty {
                if is_true_branch {
                    let narrowed =
                        ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    if !narrowed.is_dynamic() && narrowed != ty {
                        out.push((id, narrowed));
                    }
                } else if ty.is_nullable(&self.ty_table) {
                    let present =
                        ty.non_nullified(&mut *std::sync::Arc::make_mut(&mut self.ty_table));
                    if self.never_false(present, bind) {
                        out.push((id, Type::Null));
                    }
                }
            }
        }
    }
}
