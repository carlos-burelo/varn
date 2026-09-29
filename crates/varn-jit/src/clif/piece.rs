//! One CLIF function → machine code bytes.
//!
//! The seam between "we built IR" and "Cranelift produced code": it runs the
//! backend, and then reads back the one thing the caller cannot reconstruct
//! afterwards — where the call displacements are. Both pieces of a
//! compilation (raw and wrapper) go through here, which is why it knows
//! about neither.

use cranelift_codegen::ir::{ExternalName, Function};
use cranelift_codegen::isa::OwnedTargetIsa;

pub(super) struct CompiledPiece {
    pub code: Vec<u8>,
    /// Offsets of rel32 call displacements that must resolve to raw@0.
    pub call_reloc_offsets: Vec<usize>,
}

pub(super) fn compile_piece(func: Function, isa: &OwnedTargetIsa) -> Result<CompiledPiece, String> {
    super::with_ctx(func, isa.as_ref(), |compiled| {
        let mut call_reloc_offsets = Vec::new();
        for reloc in compiled.buffer.relocs() {
            // The only symbol either piece may reference is user func 0 — the
            // raw function itself.
            match &reloc.target {
                cranelift_codegen::FinalizedRelocTarget::ExternalName(ExternalName::User(_)) => {
                    if reloc.addend != -4 {
                        return Err(format!("clif: unexpected reloc addend {}", reloc.addend));
                    }
                    call_reloc_offsets.push(reloc.offset as usize);
                }
                other => return Err(format!("clif: unsupported reloc target {other:?}")),
            }
        }
        Ok(CompiledPiece {
            code: compiled.code_buffer().to_vec(),
            call_reloc_offsets,
        })
    })
}
