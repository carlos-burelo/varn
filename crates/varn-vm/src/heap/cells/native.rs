use super::CellSpace;
use varn_types::HeapRef;

pub(crate) struct NativeScope {
    was_native: bool,
    roots_len: usize,
}

impl CellSpace {
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

    pub(crate) fn suspend_native(&mut self) -> bool {
        std::mem::replace(&mut self.native, false)
    }

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
