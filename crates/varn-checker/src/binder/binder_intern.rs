use super::Binder;

impl<'r> Binder<'r> {
    pub(crate) fn intern_local(&mut self, text: &str) -> varn_core::Atom {
        self.resync_interner();
        let atom = self.interner.intern(text);
        self.resolver.intern(text);
        atom
    }

    pub(crate) fn resync_interner(&mut self) {
        if self.resolver.interner_len() > self.interner.len() {
            self.interner = self.resolver.interner_snapshot();
        }
    }

    pub(crate) fn publish_interner_tail(&mut self) {
        let live_len = self.resolver.interner_len();
        if self.interner.len() <= live_len {
            return;
        }
        let texts: Vec<String> = self
            .interner
            .iter_strings()
            .skip(live_len)
            .map(|s| s.to_owned())
            .collect();
        for text in texts {
            self.resolver.intern(&text);
        }
    }
}
