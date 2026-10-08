use crate::checker::recorder::Recorder;
use crate::checker::Checker;
use varn_sem::symbol::SymbolId;
use varn_sem::types::Type;

impl<'r> Checker<'r> {
    pub(crate) fn record_type(&self, rec: &mut Recorder, offset: u32, ty: Type) {
        rec.record_type(offset, ty);
    }

    pub(crate) fn record_type_with_symbol(
        &mut self,
        rec: &mut Recorder,
        offset: u32,
        ty: Type,
        symbol_id: SymbolId,
    ) {
        self.mark_infer_env_dirty();
        rec.record_type_with_symbol(offset, ty, symbol_id);
    }

    pub(crate) fn record_member_type(
        &self,
        rec: &mut Recorder,
        offset: u32,
        ty: Type,
        symbol_id: SymbolId,
    ) {
        rec.record_member_type(offset, ty, symbol_id);
    }
}
