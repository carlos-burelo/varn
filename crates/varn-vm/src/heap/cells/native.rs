use super::CellSpace;
use varn_types::HeapRef;

/// Where native code was when it entered: restored when it returns.
pub(crate) struct NativeScope {
    was_native: bool,
    roots_len: usize,
}

impl CellSpace {
    /// Native code may hold what it allocates only in Rust locals, across a
    /// callback that collects: from here until [`Self::exit_native`], every
    /// cell it takes is a root.
    pub(crate) fn enter_native(&mut self) -> NativeScope {
        let scope = NativeScope {
            was_native: self.native,
            roots_len: self.native_roots.len(),
        };
        self.native = true;
        scope
    }

    pub(crate) fn exit_native(&mut self, scope: NativeScope) {
        self.native = scope.was_native;
        self.native_roots.truncate(scope.roots_len);
    }

    /// Running VM code again, from inside native code or not: what it
    /// allocates is collectable as usual. Returns whether native code was
    /// running, for [`Self::resume`].
    pub(crate) fn suspend_native(&mut self) -> bool {
        std::mem::replace(&mut self.native, false)
    }

    /// Back from VM code. A value it handed to native code is a root for as
    /// long as that native code runs.
    pub(crate) fn resume(&mut self, was_native: bool, result: Option<HeapRef>) {
        self.native = was_native;
        if let (true, Some(r)) = (was_native, result) {
            self.native_roots.push(r);
        }
    }

    pub(crate) fn native_roots(&self) -> &[HeapRef] {
        &self.native_roots
    }
}
