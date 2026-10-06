








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
        let helpers = super::helpers::build_jit_helpers();
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

    
    
    
    
    
    
    
    
    
    
    pub(crate) fn osr_jit_fn(&self, _osr_ip: usize) -> Option<varn_jit::JitFn> {
        if FRAME_LAYOUT_V2_JIT_BAIL {
            return None;
        }
        let osr_ip = _osr_ip;
        let proto = &self.proto;
        if proto.jit_osr_failed.get() {
            return None;
        }
        let epoch = crate::clif_link::current_epoch();
        if epoch == 0 {
            return None;
        }
        if proto.jit_osr_epoch.get() == epoch {
            if let Some(entry) = proto.jit_osr_entry.get() {
                if proto.jit_osr_ip.get() != osr_ip {
                    return None;
                }
                return Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry) });
            }
        }

        
        
        
        
        
        
        
        
        
        
        
        
        
        
        if proto.is_generator || proto.is_async {
            proto.jit_osr_failed.set(true);
            return None;
        }

        let helpers = super::helpers::build_jit_helpers();
        let linker = crate::clif_link::CtxLinker;
        match varn_jit::compile(proto, &self.constants, helpers, &linker, Some(osr_ip)) {
            Ok(compiled) => {
                let entry_usize: usize = compiled.entry as usize;
                proto.jit_osr_epoch.set(epoch);
                proto.jit_osr_ip.set(osr_ip);
                proto.jit_osr_entry.set(Some(entry_usize));
                *proto.jit_osr_code.borrow_mut() = Some(compiled.code);
                
                
                debug_assert_eq!(
                    compiled.native, 0,
                    "osr lowering must not publish a native entry"
                );
                
                
                crate::clif_link::register_compiled(proto, None);
                Some(unsafe { std::mem::transmute::<usize, varn_jit::JitFn>(entry_usize) })
            }
            Err(_) => {
                proto.jit_osr_failed.set(true);
                proto.jit_osr_entry.set(None);
                proto.jit_osr_epoch.set(0);
                None
            }
        }
    }
}
