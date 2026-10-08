use cranelift_codegen::ir::{ExternalName, Function};
use cranelift_codegen::isa::OwnedTargetIsa;

pub(super) struct CompiledPiece {
    pub code: Vec<u8>,
    pub safepoints: Vec<crate::stack_roots::SafepointMap>,

    pub call_reloc_offsets: Vec<usize>,
}

pub(super) fn compile_piece(func: Function, isa: &OwnedTargetIsa) -> Result<CompiledPiece, String> {
    super::with_ctx(func, isa.as_ref(), |compiled| {
        let mut call_reloc_offsets = Vec::new();
        for reloc in compiled.buffer.relocs() {
            match &reloc.target {
                cranelift_codegen::FinalizedRelocTarget::ExternalName(ExternalName::User(_)) => {
                    if reloc.addend != -4 {
                        return Err(format!("clif: unexpected reloc addend {}", reloc.addend));
                    }
                    call_reloc_offsets.push(reloc.offset as usize);
                }
                other @ cranelift_codegen::FinalizedRelocTarget::ExternalName(_)
                | other @ cranelift_codegen::FinalizedRelocTarget::Func(_) => {
                    return Err(format!("clif: unsupported reloc target {other:?}"))
                }
            }
        }
        let mut safepoints = Vec::new();
        for (return_offset, _span, map) in compiled.buffer.user_stack_maps() {
            let mut slots = Vec::new();
            for (ty, offset) in map.entries() {
                if ty != cranelift_codegen::ir::types::I128 {
                    return Err(format!("clif: stack map slot of type {ty}"));
                }
                slots.push(offset);
            }
            safepoints.push(crate::stack_roots::SafepointMap {
                return_offset: *return_offset,
                slots: slots.into_boxed_slice(),
            });
        }
        Ok(CompiledPiece {
            code: compiled.code_buffer().to_vec(),
            safepoints,
            call_reloc_offsets,
        })
    })
}
