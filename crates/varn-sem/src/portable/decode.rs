use super::model::PortableModule;
use crate::bind::BindResult;
use crate::types::CheckerTyTable;
use std::sync::Arc;
use varn_core::AtomInterner;

impl PortableModule {
    pub(crate) fn into_live(
        self,
        interner: &mut AtomInterner,
        mut table: Arc<CheckerTyTable>,
    ) -> (crate::exports::ExportMap, BindResult) {
        let tables = Arc::make_mut(&mut table);
        for name in &self.names {
            interner.intern(name);
            tables.intern_name(name);
        }
        tables.absorb_slice(&self.types);
        let mut bind = self.bind;
        bind.interner = interner.clone();
        bind.ty_table = table.clone();
        let mut exports = self.exports;
        exports.table = table;
        (exports, bind)
    }
}
