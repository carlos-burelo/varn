use crate::checker::{Checker, ExprInfo};
use crate::types::Type;
use crate::SymbolId;

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
