use crate::closure::VmClosure;
use varn_types::FunctionProto;

pub(crate) const FRAME_LAYOUT_V2_JIT_BAIL: bool = false;

impl VmClosure {
    const JIT_TIER_THRESHOLD: u32 = 1;

    const JIT_TIER_THRESHOLD_STRAIGHT: u32 = 128;

    const JIT_OSR_BACKEDGES: u32 = 1000;

    #[inline(always)]
    pub(crate) fn osr_backedge_threshold() -> u32 {
        static T: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *T.get_or_init(|| {
            std::env::var("VARN_JIT_OSR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(Self::JIT_OSR_BACKEDGES)
        })
    }

    fn tier_threshold(proto: &FunctionProto) -> u32 {
        if proto.has_backedge() {
            static LT: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
            return *LT.get_or_init(|| {
                std::env::var("VARN_JIT_TIER")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(Self::JIT_TIER_THRESHOLD)
            });
        }
        static ST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        *ST.get_or_init(|| {
            std::env::var("VARN_JIT_TIER_STRAIGHT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(Self::JIT_TIER_THRESHOLD_STRAIGHT)
        })
    }

    #[inline(always)]
    pub(crate) fn jit_fn(&self) -> Option<varn_jit::JitFn> {
        if self.proto.jit_epoch.get() != crate::clif_link::current_epoch() {
            return None;
        }
        let e = self.proto.jit_entry.get();
        if e == 0 {
            return None;
        }
        Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(e) })
    }

    pub(crate) fn hot_jit_fn(&self) -> Option<varn_jit::JitFn> {
        if FRAME_LAYOUT_V2_JIT_BAIL {
            return None;
        }
        if let Some(f) = self.jit_fn() {
            varn_jit::JIT_STATS
                .jit_cached
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(f);
        }
        if self.proto.jit_failed.get() {
            return None;
        }
        let n = self.proto.jit_entry_count.get() + 1;
        self.proto.jit_entry_count.set(n);
        if n < Self::tier_threshold(&self.proto) {
            return None;
        }
        self.compile_jit();
        self.jit_fn()
    }

    pub(crate) fn compile_jit(&self) {
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
        let helpers = super::table_build::build_jit_helpers();
        let linker = crate::clif_link::CtxLinker;
        match varn_jit::compile(&self.proto, &self.constants, helpers, &linker, None) {
            Ok(compiled) => {
                let entry_usize: usize = compiled.entry as usize;
                self.proto.jit_epoch.set(epoch);
                self.proto.jit_entry.set(entry_usize);
                *self.proto.jit_code.borrow_mut() = Some(compiled.code);

                self.proto.jit_native_sig.set(compiled.native_sig);
                self.proto.jit_native.set(compiled.native);
                crate::clif_link::register_compiled(&self.proto, previous);
            }
            Err(_) => {
                self.proto.jit_failed.set(true);
                self.proto.jit_entry.set(0);
                self.proto.jit_native.set(0);
                self.proto.jit_native_sig.set(0);
                self.proto.jit_epoch.set(0);
                if let Some((old_epoch, code)) = previous {
                    crate::clif_link::retire_code(old_epoch, code);
                }
            }
        }
    }

    pub(crate) fn osr_jit_fn(&self, req: varn_types::ssa::OsrRequest) -> Option<varn_jit::JitFn> {
        if FRAME_LAYOUT_V2_JIT_BAIL {
            return None;
        }
        let proto = &self.proto;
        let epoch = crate::clif_link::current_epoch();
        if epoch == 0 {
            return None;
        }
        if let Some(hit) = proto.osr_lookup(req.ip, req.mode, epoch) {
            return hit
                .map(|entry| unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry) });
        }

        let allowed = match req.mode {
            varn_types::ssa::OsrMode::Resume => {
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

        let helpers = super::table_build::build_jit_helpers();
        let linker = crate::clif_link::CtxLinker;
        match varn_jit::compile(proto, &self.constants, helpers, &linker, Some(req)) {
            Ok(compiled) => {
                let entry_usize: usize = compiled.entry as usize;
                debug_assert_eq!(
                    compiled.native, 0,
                    "osr lowering must not publish a native entry"
                );
                proto.osr_remember(req.ip, req.mode, epoch, Some((entry_usize, compiled.code)));

                crate::clif_link::register_compiled(proto, None);
                Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry_usize) })
            }
            Err(_) => {
                proto.osr_remember(req.ip, req.mode, epoch, None);
                None
            }
        }
    }
}
