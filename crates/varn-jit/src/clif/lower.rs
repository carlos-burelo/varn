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
        if self.activation != Activation::Native || proto.has_rest {
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

pub struct NoLinker;
impl ClifLinker for NoLinker {
    fn current_epoch(&self) -> u64 {
        0
    }
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
    linker: &dyn ClifLinker,
    osr_ip: Option<usize>,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
    if crate::PAIR_MIGRATION_PENDING {
        return Err(crate::PAIR_MIGRATION_BAIL.to_owned());
    }

    super::emit::reset_disabled_helper_hit();

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

    if proto.is_generator || proto.is_async {
        return Err("clif: generator/async not JIT-able in fase B".into());
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
        linker,
        osr_ip,
        Activation::Native,
        debug.as_deref_mut(),
    ) {
        Ok(lowered) => Ok((lowered, Activation::Native)),
        Err(reason) if reason == super::from_ssa::NEEDS_ACTIVATION => {
            super::emit::reset_disabled_helper_hit();
            super::from_ssa::try_lower(
                proto,
                ssa,
                constants,
                helpers,
                isa,
                linker,
                osr_ip,
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
                eprintln!(
                    "clif: from_ssa {}{} ({activation:?}{})",
                    proto.name.as_deref().unwrap_or("<module>"),
                    osr_ip.map_or(String::new(), |ip| format!(" osr@{ip}")),
                    if lowered.frameless { ", frameless" } else { "" }
                );
            }
            let wrapper = build_wrapper(proto, helpers, isa, activation, osr_ip.is_some())?;
            finish_artifact(lowered, wrapper, activation, debug)
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

fn finish_artifact(
    lowered: super::from_ssa::Lowered,
    wrapper: super::piece::CompiledPiece,
    activation: Activation,
    mut debug: Option<&mut ClifDebugSink>,
) -> Result<ClifArtifact, String> {
    let raw = lowered.piece;
    let wrapper_off = (raw.code.len() + 15) & !15;
    let total = wrapper_off + wrapper.code.len();
    let mut buf = JitBuffer::new(total.max(16))?;
    {
        let slice = buf.as_mut_slice();
        slice[..raw.code.len()].copy_from_slice(&raw.code);
        slice[wrapper_off..wrapper_off + wrapper.code.len()].copy_from_slice(&wrapper.code);
        for r in &raw.call_reloc_offsets {
            patch_rel32(slice, *r, 0)?;
        }
        for r in &wrapper.call_reloc_offsets {
            patch_rel32(slice, wrapper_off + *r, 0)?;
        }
    }
    super::debug::capture_code(&mut debug, &mut buf, raw.code.len(), wrapper_off, total);
    buf.make_executable()?;
    let raw_ptr = buf.as_ptr();
    let mut safepoints = raw.safepoints;
    for mut site in wrapper.safepoints {
        site.return_offset += wrapper_off as u32;
        safepoints.push(site);
    }
    crate::stack_roots::register(raw_ptr as usize, total, safepoints);
    let entry = unsafe { buf.as_ptr().add(wrapper_off) };
    Ok(ClifArtifact {
        buffer: buf,
        entry,
        raw: raw_ptr,
        activation,
        frameless: lowered.frameless,
    })
}
