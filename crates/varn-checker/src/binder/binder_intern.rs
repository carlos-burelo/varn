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

    pub(crate) fn publish_interner_tail(&mut self) {
        let live = self.resolver.interner_snapshot();
        let texts: Vec<String> = self
            .interner
            .texts()
            .filter(|t| live.get(t).is_none())
            .map(|s| s.to_owned())
            .collect();
        for text in texts {
            self.resolver.intern(&text);
        }
    }
}
