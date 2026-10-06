#[derive(Debug, Default, Clone)]
pub struct KindReport {
    pub nregs: usize,
    pub blocks: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Default, Clone)]
pub struct CodeBytes {
    pub bytes: Vec<u8>,
    pub raw_off: usize,
    pub raw_len: usize,
    pub entry_off: usize,
}

#[derive(Debug, Default)]
pub struct ClifDebugSink {
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
}

use varn_types::ssa::SsaProto;

use cranelift_codegen::ir::Function;

use crate::mem::JitBuffer;

pub(super) fn capture_ir(debug: &mut Option<&mut ClifDebugSink>, func: &Function) {
    if let Some(sink) = debug.as_deref_mut() {
        sink.clif_ir = Some(func.display().to_string());
    }
}

pub(super) fn capture_kinds_ssa(debug: &mut Option<&mut ClifDebugSink>, ssa: &SsaProto) {
    if let Some(sink) = debug.as_deref_mut() {
        let blocks = ssa
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(i, blk)| {
                let mut vals: Vec<String> = blk.params.iter().map(|v| val_class(ssa, *v)).collect();
                vals.extend(
                    blk.insts
                        .iter()
                        .filter_map(|inst| inst.dest.map(|v| val_class(ssa, v))),
                );
                (!vals.is_empty()).then_some((i, vals))
            })
            .collect();
        sink.kinds = Some(KindReport {
            nregs: ssa.register_count as usize,
            blocks,
        });
    }
}

fn val_class(ssa: &SsaProto, v: u32) -> String {
    format!("v{v}:{:?}", ssa.value_ty(v))
}

pub(super) fn capture_code(
    debug: &mut Option<&mut ClifDebugSink>,
    buf: &mut JitBuffer,
    raw_len: usize,
    wrapper_off: usize,
    total: usize,
) {
    if let Some(sink) = debug.as_deref_mut() {
        let slice = buf.as_mut_slice();
        let end = total.min(slice.len());
        sink.code = Some(CodeBytes {
            bytes: slice[..end].to_vec(),
            raw_off: 0,
            raw_len,
            entry_off: wrapper_off,
        });
    }
}

use super::lower::{try_compile, ClifLinker};
use crate::JitHelpers;
use cranelift_codegen::isa::OwnedTargetIsa;
use varn_types::{FunctionProto, VmValue};

pub struct ClifInspection {
    pub name: String,

    pub route: Result<(), String>,
    pub kinds: Option<KindReport>,
    pub clif_ir: Option<String>,
    pub code: Option<CodeBytes>,
    pub frame_aware: bool,

    pub framed: bool,

    pub fa_reasons: Vec<&'static str>,
}

pub fn inspect(
    proto: &FunctionProto,
    constants: &[VmValue],
    helpers: &JitHelpers,
    isa: &OwnedTargetIsa,
    linker: &dyn ClifLinker,
) -> ClifInspection {
    let mut sink = ClifDebugSink::default();
    let result = try_compile(
        proto,
        constants,
        helpers,
        isa,
        linker,
        None,
        Some(&mut sink),
    );
    let (route, frame_aware, framed) = match &result {
        Ok(art) => (
            Ok(()),
            !art.frameless,
            art.activation == super::abi::Activation::Framed,
        ),
        Err(e) => (Err(e.clone()), false, false),
    };
    ClifInspection {
        name: proto.name.as_deref().unwrap_or("<top-level>").to_string(),
        route,
        kinds: sink.kinds,
        clif_ir: sink.clif_ir,
        code: sink.code,
        frame_aware,
        framed,
        fa_reasons: super::lower::frame_aware_reasons(proto),
    }
}
