use std::sync::OnceLock;

use varn_types::{FunctionProto, VmValue};

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

fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("VARN_JIT_CACHE").as_deref() {
        Ok("off") => Mode::Off,
        Ok("verify") => Mode::Verify,
        _ => Mode::On,
    })
}

fn key(proto: &FunctionProto, osr: Option<varn_types::ssa::OsrRequest>) -> Option<u64> {
    let mut bytes = postcard::to_allocvec(proto).ok()?;
    bytes.extend_from_slice(&varn_jit::isa_identity().to_le_bytes());
    bytes.extend_from_slice(&varn_jit::JIT_HELPERS_VERSION.to_le_bytes());
    if let Some(req) = osr {
        bytes.extend_from_slice(&(req.ip as u64).to_le_bytes());
    }
    Some(varn_core::fingerprint::fingerprint(&bytes))
}

pub(crate) fn compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: varn_jit::JitHelpers,
    linker: &dyn varn_jit::clif::lower::ClifLinker,
    osr: Option<varn_types::ssa::OsrRequest>,
) -> Result<varn_jit::Compiled, String> {
    let store = match (STORE.get(), mode()) {
        (Some(store), Mode::On | Mode::Verify) => *store,
        (None, _) | (_, Mode::Off) => {
            return varn_jit::compile(proto, constants, helpers, linker, osr)
        }
    };
    let key = key(proto, osr);
    let cached = key
        .and_then(|k| store.load(k))
        .and_then(|bytes| postcard::from_bytes::<varn_jit::CodeImage>(&bytes).ok());
    if let Some(image) = cached {
        if mode() == Mode::Verify {
            verify(proto, constants, helpers, osr, &image);
        }
        if let Ok(compiled) = varn_jit::install(&image, proto, constants, helpers, linker) {
            return Ok(compiled);
        }
    }
    let image = varn_jit::lower(proto, constants, helpers, osr)?;
    if let Some(k) = key
        && image.persistable()
        && let Ok(bytes) = postcard::to_allocvec(&image)
    {
        store.store(k, &bytes);
    }
    varn_jit::install(&image, proto, constants, helpers, linker)
}

fn verify(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: varn_jit::JitHelpers,
    osr: Option<varn_types::ssa::OsrRequest>,
    cached: &varn_jit::CodeImage,
) {
    let fresh = varn_jit::lower(proto, constants, helpers, osr)
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
