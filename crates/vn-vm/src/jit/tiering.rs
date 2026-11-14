use crate::closure::VmClosure;
use crate::settings::ExecSettings;
use vn_types::FunctionProto;

impl VmClosure {
    fn tier_threshold(proto: &FunctionProto, settings: &ExecSettings) -> u32 {
        if proto.has_backedge() {
            settings.tier_loop
        } else {
            settings.tier_straight
        }
    }

    #[inline(always)]
    pub(crate) fn jit_fn(&self) -> Option<vn_jit::JitFn> {
        if self.proto.jit_epoch.get() != crate::clif_link::current_epoch() {
            return None;
        }
        let e = self.proto.jit_entry.get();
        if e == 0 {
            return None;
        }
        Some(unsafe { std::mem::transmute::<usize, vn_jit::JitFn>(e) })
    }

    pub(crate) fn hot_jit_fn(&self, settings: &ExecSettings) -> Option<vn_jit::JitFn> {
        if let Some(f) = self.jit_fn() {
            vn_jit::JitStats::bump(&vn_jit::JIT_STATS.jit_cached);
            return Some(f);
        }
        if self.proto.jit_failed.get() {
            return None;
        }
        let n = self.proto.jit_entry_count.get() + 1;
        self.proto.jit_entry_count.set(n);
        if n < Self::tier_threshold(&self.proto, settings) {
            return None;
        }
        self.compile_jit(settings);
        self.jit_fn()
    }

    pub(crate) fn compile_jit(&self, settings: &ExecSettings) {
        let epoch = crate::clif_link::current_epoch();
        if self.proto.jit_failed.get() || epoch == 0 {
            return;
        }

        let previous = if self.proto.jit_entry.get() != 0 {
            if self.proto.jit_epoch.get() == epoch {
                return;
            }
            let old_epoch = self.proto.jit_epoch.get();
            self.proto
                .jit_code
                .borrow_mut()
                .take()
                .map(|code| (old_epoch, code))
        } else {
            None
        };
        let helpers = super::table_build::build_jit_helpers(settings);
        let linker = crate::clif_link::CtxLinker;
        match super::code_cache::compile(&self.proto, &self.constants, helpers, &linker, None) {
            Ok(compiled) => {
                let entry_usize: usize = compiled.entry as usize;
                self.proto.jit_epoch.set(epoch);
                self.proto.jit_entry.set(entry_usize);
                *self.proto.jit_code.borrow_mut() = Some(compiled.code);

                self.proto.jit_native_sig.set(compiled.native_sig);
                self.proto.jit_native.set(compiled.native);
                self.proto.jit_host.set(compiled.host);
                crate::clif_link::register_compiled(&self.proto, previous);
            }
            Err(_) => {
                self.proto.jit_failed.set(true);
                self.proto.jit_entry.set(0);
                self.proto.jit_native.set(0);
                self.proto.jit_native_sig.set(0);
                self.proto.jit_host.set(0);
                self.proto.jit_epoch.set(0);
                if let Some((old_epoch, code)) = previous {
                    crate::clif_link::retire_code(old_epoch, code);
                }
            }
        }
    }

    pub(crate) fn osr_jit_fn(
        &self,
        req: vn_types::ssa::OsrRequest,
        settings: &ExecSettings,
    ) -> Option<vn_jit::JitFn> {
        let proto = &self.proto;
        let epoch = crate::clif_link::current_epoch();
        if epoch == 0 {
            return None;
        }
        if let Some(hit) = proto.osr_lookup(req.ip, req.mode, epoch) {
            return hit.map(|entry| unsafe { std::mem::transmute::<usize, vn_jit::JitFn>(entry) });
        }

        let allowed = match req.mode {
            vn_types::ssa::OsrMode::Resume => {
                if proto.is_generator || proto.is_async {
                    proto
                        .ssa
                        .get()
                        .is_some_and(|ssa| ssa.is_resume_entry(req.ip))
                } else {
                    true
                }
            }
        };
        if !allowed {
            proto.osr_remember(req.ip, req.mode, epoch, None);
            return None;
        }

        let helpers = super::table_build::build_jit_helpers(settings);
        let linker = crate::clif_link::CtxLinker;
        match super::code_cache::compile(proto, &self.constants, helpers, &linker, Some(req)) {
            Ok(compiled) => {
                let entry_usize: usize = compiled.entry as usize;
                debug_assert_eq!(
                    compiled.native, 0,
                    "osr lowering must not publish a native entry"
                );
                proto.osr_remember(req.ip, req.mode, epoch, Some((entry_usize, compiled.code)));

                crate::clif_link::register_compiled(proto, None);
                Some(unsafe { std::mem::transmute::<usize, vn_jit::JitFn>(entry_usize) })
            }
            Err(_) => {
                proto.osr_remember(req.ip, req.mode, epoch, None);
                None
            }
        }
    }
}
