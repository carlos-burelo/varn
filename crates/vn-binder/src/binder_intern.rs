use super::Binder;
use std::sync::Arc;
use vn_sem::types::CheckerTyTable;

impl<'r> Binder<'r> {
    pub(crate) fn intern_local(&mut self, text: &str) -> vn_core::Atom {
        self.interner.intern(text)
    }

    pub(crate) fn adopt(&mut self, table: &CheckerTyTable) {
        self.interner.absorb(table.names());
        Arc::make_mut(&mut self.ty_table).absorb(table);
    }

    pub(crate) fn name_text(&self, atom: vn_core::Atom) -> Arc<str> {
        self.interner
            .try_resolve(atom)
            .or_else(|| self.ty_table.name(atom))
            .map(Arc::from)
            .unwrap_or_default()
    }
}
