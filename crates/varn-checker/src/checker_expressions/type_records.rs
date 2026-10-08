use crate::checker::Checker;
use varn_sem::output::ExprInfo;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn record_type(&mut self, offset: u32, ty: Type) {
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: None,
                },
            );
        }
    }

    pub(crate) fn record_type_with_symbol(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        self.symbol_types.insert(symbol_id, ty);
        self.mark_infer_env_dirty();
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }

    pub(crate) fn record_member_type(&mut self, offset: u32, ty: Type, symbol_id: SymbolId) {
        if self.record_expr_types {
            self.expr_types.insert(
                offset,
                ExprInfo {
                    ty,
                    symbol_id: Some(symbol_id),
                },
            );
        }
    }
}
