use cranelift_codegen::isa::OwnedTargetIsa;
use varn_types::{FunctionProto, VmValue};

use super::abi::{build_wrapper, Activation};
use super::alloc;
use super::debug::ClifDebugSink;
use super::emit::patch_rel32;
use crate::mem::JitBuffer;
use crate::JitHelpers;

pub struct ClifArtifact {
    pub buffer: JitBuffer,
    pub entry: *const u8,
    pub raw: *const u8,

    pub activation: Activation,

    pub frameless: bool,
}

impl ClifArtifact {
    pub fn native_entry(&self, proto: &FunctionProto) -> (usize, u64) {
        if self.activation != Activation::Native
            || proto.has_rest
            || proto.is_async
            || proto.is_generator
        {
            return (0, 0);
        }
        let Some(id) = super::native_abi::NativeShape::of_proto(proto).id() else {
            return (0, 0);
        };
        let frameless = if self.frameless {
            super::native_abi::NATIVE_FRAMELESS
        } else {
            0
        };
        (self.raw as usize, id | frameless)
    }
}

impl Drop for ClifArtifact {
    fn drop(&mut self) {
        crate::stack_roots::unregister(self.raw as usize);
    }
}

pub trait ClifLinker {
    fn current_epoch(&self) -> u64;
}

fn boxed_slots(proto: &FunctionProto) -> bool {
    use varn_types::register_meta::SlotKind;
    let scalar = |k: &SlotKind| matches!(k, SlotKind::Int | SlotKind::Float | SlotKind::Bool);
    proto.param_kinds.iter().any(|k| !scalar(k)) || !scalar(&proto.return_kind)
}

pub fn frame_aware_reasons(proto: &FunctionProto) -> Vec<&'static str> {
    let code = &proto.chunk.code;
    let pool = &proto.chunk.constants;
    let mut r = Vec::new();
    if proto.has_this {
        r.push("this");
    }
    if alloc::has_alloc(code, pool).unwrap_or(true) {
        r.push("alloc");
    }
    if boxed_slots(proto) {
        r.push("boxed");
    }
    if proto.upvalue_count > 0 {
        r.push("upvalue");
    }
    if proto.is_generator {
        r.push("generator");
    }
    if proto.is_async {
        r.push("async");
    }
    if proto.resumes_in_interpreter() {
        r.push("resume");
    }
    r
}

pub fn gate_reason(proto: &FunctionProto) -> Option<String> {
    let words = proto.chunk.code.len();
    if words > crate::SIZE_GATE_WORDS && !leaf_safe(proto) {
        return Some(format!("too large (>{} words)", crate::SIZE_GATE_WORDS));
    }
    None
}

fn leaf_safe(proto: &FunctionProto) -> bool {
    if proto.is_generator || proto.is_async {
        return false;
    }
    !alloc::has_alloc(&proto.chunk.code, &proto.chunk.constants).unwrap_or(true)
}

pub fn try_compile(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    osr_req: Option<varn_types::ssa::OsrRequest>,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<CodeImage, String> {
    if crate::PAIR_MIGRATION_PENDING {
        return Err(crate::PAIR_MIGRATION_BAIL.to_owned());
    }

    super::emit::reset_disabled_helper_hit();
    super::reloc::reset_pinned_address();
    super::reloc::bind_helpers(helpers);

    if let Some(reason) = gate_reason(proto) {
        if super::trace() {
            eprintln!("CLIF GATE  {:?}: {reason}", proto.name);
        }
        crate::stats::JIT_STATS
            .gate_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::stats::record(|| crate::stats::CompileRecord {
            name: crate::fn_name(proto),
            words: proto.chunk.code.len(),
            outcome: crate::stats::CompileOutcome::Gated(reason.clone()),
            compile_ns: 0,
        });
        return Err("JIT Bailout: function too large".to_owned());
    }
    if proto.chunk.code.len() > crate::SIZE_GATE_WORDS && super::trace() {
        eprintln!(
            "CLIF GATE  {:?}: large but leaf-safe, admitted ({} words)",
            proto.name,
            proto.chunk.code.len()
        );
    }

    if let Some(why) = proto.ssa.unavailable() {
        if super::trace() {
            eprintln!(
                "clif: from_ssa unavailable {}: {why}",
                proto.name.as_deref().unwrap_or("<module>")
            );
        }
        return Err(format!("clif: from_ssa unavailable: {why}"));
    }
    let Some(ssa) = proto.ssa.get() else {
        return Err("clif: from_ssa unavailable".into());
    };
    let lowered = match super::from_ssa::try_lower(
        proto,
        ssa,
        constants,
        helpers,
        isa,
        osr_req,
        Activation::Native,
        debug.as_deref_mut(),
    ) {
        Ok(lowered) => Ok((lowered, Activation::Native)),
        Err(reason) if reason.starts_with(super::from_ssa::NEEDS_ACTIVATION) => {
            super::emit::reset_disabled_helper_hit();
            super::from_ssa::try_lower(
                proto,
                ssa,
                constants,
                helpers,
                isa,
                osr_req,
                Activation::Framed,
                debug.as_deref_mut(),
            )
            .map(|lowered| (lowered, Activation::Framed))
        }
        Err(reason) => Err(reason),
    };
    match lowered {
        Ok((lowered, activation)) if !super::emit::disabled_helper_hit() => {
            if super::trace() {
                let osr_tag = osr_req.map_or(String::new(), |req| format!(" osr@{}", req.ip));
                eprintln!(
                    "clif: from_ssa {}{} ({activation:?}{})",
                    proto.name.as_deref().unwrap_or("<module>"),
                    osr_tag,
                    if lowered.frameless { ", frameless" } else { "" }
                );
            }
            let wrapper = build_wrapper(proto, helpers, isa, activation, osr_req.is_some())?;
            let image = CodeImage::assemble(lowered, wrapper, activation);
            super::debug::capture_code(
                &mut debug,
                &image.code,
                image.raw_len(),
                image.wrapper_off as usize,
            );
            Ok(image)
        }
        Ok(_) => Err("clif: uses a helper disabled in fase B".into()),
        Err(reason) => {
            if super::trace() {
                eprintln!(
                    "clif: from_ssa bail {}: {reason}",
                    proto.name.as_deref().unwrap_or("<module>")
                );
            }
            Err(reason)
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CodeImage {
    code: Vec<u8>,
    wrapper_off: u32,
    call_relocs: Vec<u32>,
    abs_relocs: Vec<(u32, super::reloc::AbsSym)>,
    natives: Vec<u64>,
    safepoints: Vec<(u32, Box<[u32]>)>,
    activation: Activation,
    frameless: bool,
    #[serde(skip)]
    pinned: bool,
}

impl CodeImage {
    fn assemble(
        lowered: super::from_ssa::Lowered,
        wrapper: super::piece::CompiledPiece,
        activation: Activation,
    ) -> Self {
        let raw = lowered.piece;
        let wrapper_off = (raw.code.len() + 15) & !15;
        let mut code = vec![0u8; wrapper_off + wrapper.code.len()];
        code[..raw.code.len()].copy_from_slice(&raw.code);
        code[wrapper_off..].copy_from_slice(&wrapper.code);
        let shift = |off: usize| (wrapper_off + off) as u32;
        let call_relocs = raw
            .call_reloc_offsets
            .iter()
            .map(|&r| r as u32)
            .chain(wrapper.call_reloc_offsets.iter().map(|&r| shift(r)))
            .collect();
        let abs_relocs = raw
            .abs_relocs
            .iter()
            .copied()
            .chain(
                wrapper
                    .abs_relocs
                    .iter()
                    .map(|&(off, sym)| (shift(off as usize), sym)),
            )
            .collect();
        let safepoints = raw
            .safepoints
            .into_iter()
            .map(|s| (s.return_offset, s.slots))
            .chain(
                wrapper
                    .safepoints
                    .into_iter()
                    .map(|s| (shift(s.return_offset as usize), s.slots)),
            )
            .collect();
        CodeImage {
            code,
            wrapper_off: wrapper_off as u32,
            call_relocs,
            abs_relocs,
            natives: lowered.natives,
            safepoints,
            activation,
            frameless: lowered.frameless,
            pinned: super::reloc::pinned_address(),
        }
    }

    fn raw_len(&self) -> usize {
        self.wrapper_off as usize
    }

    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    pub fn persistable(&self) -> bool {
        !self.pinned
    }

    pub fn frameless(&self) -> bool {
        self.frameless
    }

    pub fn activation(&self) -> Activation {
        self.activation
    }

    pub fn install(&self, resolver: &super::reloc::Resolver<'_>) -> Result<ClifArtifact, String> {
        let natives = super::reloc::Resolver {
            natives: &self.natives,
            ..*resolver
        };
        let mut buf = JitBuffer::new(self.code.len().max(16))?;
        {
            let slice = buf.as_mut_slice();
            slice[..self.code.len()].copy_from_slice(&self.code);
            for &r in &self.call_relocs {
                patch_rel32(slice, r as usize, 0)?;
            }
            for &(off, sym) in &self.abs_relocs {
                let value = natives.resolve(sym)?;
                let at = off as usize;
                slice
                    .get_mut(at..at + 8)
                    .ok_or("clif: absolute reloc out of bounds")?
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        buf.make_executable()?;
        let raw_ptr = buf.as_ptr();
        let safepoints = self
            .safepoints
            .iter()
            .map(|(return_offset, slots)| crate::stack_roots::SafepointMap {
                return_offset: *return_offset,
                slots: slots.clone(),
            })
            .collect();
        crate::stack_roots::register(raw_ptr as usize, self.code.len(), safepoints);
        let entry = unsafe { buf.as_ptr().add(self.wrapper_off as usize) };
        Ok(ClifArtifact {
            buffer: buf,
            entry,
            raw: raw_ptr,
            activation: self.activation,
            frameless: self.frameless,
        })
    }
}
