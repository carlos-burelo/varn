//! One CLIF function → machine code bytes.
//!
//! The seam between "we built IR" and "Cranelift produced code": it runs the
//! backend, and then reads back the two things the caller cannot reconstruct
//! afterwards — where the call displacements are, and which bytecode ip each
//! stack map belongs to. Both pieces of a compilation (raw and wrapper) go
//! through here, which is why it knows about neither.

use cranelift_codegen::ir::{ExternalName, Function};
use cranelift_codegen::isa::OwnedTargetIsa;

pub(super) struct CompiledPiece {
    pub code: Vec<u8>,
    /// Offsets of rel32 call displacements that must resolve to raw@0.
    pub call_reloc_offsets: Vec<usize>,
    /// `(bytecode ip, roots declared)` per emitted stack map, joined through
    /// the srclocs stamped in the dispatch loop. Empty unless roots were asked
    /// for — with no marking Cranelift emits no maps at all.
    pub stack_maps: Vec<(usize, usize)>,
    /// Maps whose PC fell in no stamped srcloc range.
    pub maps_unmatched: usize,
}

pub(super) fn compile_piece(
    func: Function,
    isa: &OwnedTargetIsa,
) -> Result<CompiledPiece, String> {
    super::with_ctx(func, isa.as_ref(), |compiled| {
        let srclocs = compiled.buffer.get_srclocs_sorted();
        let mut stack_maps = Vec::new();
        let mut maps_unmatched = 0usize;
        for (offset, _, map) in compiled.buffer.user_stack_maps() {
            // The map's PC is the safepoint instruction. Attribute it to the
            // NEAREST PRECEDING stamped srcloc rather than to a range that
            // strictly contains it: regalloc interleaves spills and reloads
            // around a call, and those carry no srcloc of their own, so a
            // containment test drops the map even though the emitting opcode
            // is unambiguous.
            match srclocs
                .iter()
                .filter(|l| l.start <= *offset && !l.loc.is_default())
                .max_by_key(|l| l.start)
            {
                Some(l) => stack_maps.push((l.loc.bits() as usize, map.entries().count())),
                None => maps_unmatched += 1,
            }
        }
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
            stack_maps,
            maps_unmatched,
        })
    })
}
