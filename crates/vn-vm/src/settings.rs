#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecSettings {
    pub no_jit: bool,

    pub trace: bool,

    pub osr_backedges: u32,

    pub tier_loop: u32,

    pub tier_straight: u32,

    pub gc_stress: Option<usize>,

    pub gc_verify: bool,
}

impl ExecSettings {
    const OSR_BACKEDGES: u32 = 1000;
    const TIER_LOOP: u32 = 1;
    const TIER_STRAIGHT: u32 = 128;
    const NURSERY_BYTES: u64 = 16 << 20;
    const STRESS_CELL_BYTES: u64 = 24;

    pub fn nursery_bytes(&self) -> u64 {
        self.gc_stress
            .map_or(Self::NURSERY_BYTES, |cells| {
                (cells as u64).saturating_mul(Self::STRESS_CELL_BYTES)
            })
            .max(Self::STRESS_CELL_BYTES)
    }

    pub fn stress_cells(&self) -> Option<u64> {
        self.gc_stress.map(|cells| cells as u64)
    }

    pub fn of(settings: &vn_core::env::Settings, trace: bool) -> Self {
        Self {
            no_jit: settings.no_jit,
            trace,
            osr_backedges: settings.jit_osr.unwrap_or(Self::OSR_BACKEDGES),
            tier_loop: settings.jit_tier.unwrap_or(Self::TIER_LOOP),
            tier_straight: settings.jit_tier_straight.unwrap_or(Self::TIER_STRAIGHT),
            gc_stress: settings.gc_stress,
            gc_verify: settings.gc_verify,
        }
    }
}

impl Default for ExecSettings {
    fn default() -> Self {
        Self::of(&vn_core::env::Settings::default(), false)
    }
}

pub fn configure_process(settings: &vn_core::env::Settings) {
    vn_jit::configure(vn_jit::JitConfig::of(settings));
    crate::jit::code_cache::configure(settings.jit_cache);
    crate::gc_trace::configure(settings.gc_trace);
    crate::home_trace::configure(settings.home_trace);
    crate::alloc_profile::configure(settings.alloc_profile);
}
