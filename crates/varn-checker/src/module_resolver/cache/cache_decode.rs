use super::cache_types::PortableModule;
use crate::binder::BindResult;
use crate::types::CheckerTyTable;
use std::sync::Arc;
use varn_core::AtomInterner;

impl PortableModule {
    pub(super) fn into_live(
        self,
        interner: &mut AtomInterner,
        mut table: Arc<CheckerTyTable>,
    ) -> (super::super::ExportMap, BindResult) {
        for name in &self.names {
            interner.intern(name);
        }
        Arc::make_mut(&mut table).absorb_slice(&self.types);
        let mut bind = self.bind;
        bind.interner = interner.clone();
        bind.ty_table = table;
        (self.exports, bind)
    }
}
