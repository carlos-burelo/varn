use std::sync::OnceLock;

use vn_types::{FunctionProto, VmValue};

pub trait CodeStore: Send + Sync {
    fn load(&self, key: u64) -> Option<Vec<u8>>;
    fn store(&self, key: u64, bytes: &[u8]);
}

static STORE: OnceLock<&'static dyn CodeStore> = OnceLock::new();

pub fn register_code_store(store: &'static dyn CodeStore) {
    let _ = STORE.set(store);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    On,
    Verify,
    Off,
}

static MODE: OnceLock<Mode> = OnceLock::new();

pub fn configure(mode: vn_core::env::JitCacheMode) -> bool {
    MODE.set(match mode {
        vn_core::env::JitCacheMode::Off => Mode::Off,
        vn_core::env::JitCacheMode::Verify => Mode::Verify,
        vn_core::env::JitCacheMode::On => Mode::On,
    })
    .is_ok()
}

fn mode() -> Mode {
    *MODE.get_or_init(|| Mode::On)
}

fn key(
    proto: &FunctionProto,
    _helpers: &vn_jit::JitHelpers,
    osr: Option<vn_types::ssa::OsrRequest>,
) -> Option<u64> {
    let mut bytes = postcard::to_allocvec(proto).ok()?;
    bytes.extend_from_slice(&vn_jit::isa_identity().to_le_bytes());
    bytes.extend_from_slice(&vn_jit::JIT_HELPERS_VERSION.to_le_bytes());
    if let Some(req) = osr {
        bytes.extend_from_slice(&(req.ip as u64).to_le_bytes());
    }
    Some(vn_core::fingerprint::fingerprint(&bytes))
}

pub(crate) fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: vn_jit::JitHelpers,
    linker: &dyn vn_jit::clif::lower::ClifLinker,
    osr: Option<vn_types::ssa::OsrRequest>,
) -> Result<vn_jit::Compiled, String> {
    let _compiling = crate::cpu_profile::Activity::compiling();
    let store = match (STORE.get(), mode()) {
        (Some(store), Mode::On | Mode::Verify) => *store,
        (None, _) | (_, Mode::Off) => {
            return vn_jit::compile(proto, constants, helpers, linker, osr)
        }
    };
    let key = key(proto, &helpers, osr);
    let cached = key
        .and_then(|k| store.load(k))
        .and_then(|bytes| postcard::from_bytes::<vn_jit::CodeImage>(&bytes).ok());
    if let Some(image) = cached {
        if mode() == Mode::Verify {
            verify(proto, constants, helpers, osr, &image);
        }
        if let Ok(compiled) = vn_jit::install(&image, proto, constants, helpers, linker) {
            return Ok(compiled);
        }
    }
    let image = vn_jit::lower(proto, constants, helpers, osr)?;
    if let Some(k) = key
        && image.persistable()
        && let Ok(bytes) = postcard::to_allocvec(&image)
    {
        store.store(k, &bytes);
    }
    vn_jit::install(&image, proto, constants, helpers, linker)
}

fn verify(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: vn_jit::JitHelpers,
    osr: Option<vn_types::ssa::OsrRequest>,
    cached: &vn_jit::CodeImage,
) {
    let fresh = vn_jit::lower(proto, constants, helpers, osr)
        .ok()
        .and_then(|image| postcard::to_allocvec(&image).ok());
    let cached = postcard::to_allocvec(cached).ok();
    if fresh != cached {
        eprintln!(
            "VARN_JIT_CACHE verify: cached code differs from a fresh compile of {}",
            proto.name.as_deref().unwrap_or("<module>")
        );
        std::process::exit(70);
    }
}
