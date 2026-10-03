use super::Binder;

impl<'r> Binder<'r> {
    pub(crate) fn intern_local(&mut self, text: &str) -> varn_core::Atom {
        self.resync_interner();
        let atom = self.interner.intern(text);
        self.resolver.intern(text);
        atom
    }

    pub(crate) fn resync_interner(&mut self) {
        let live_len = self.resolver.interner_len();
        if live_len != self.live_names_seen {
            self.interner.absorb(&self.resolver.interner_snapshot());
            self.live_names_seen = live_len;
        }
    }
}
