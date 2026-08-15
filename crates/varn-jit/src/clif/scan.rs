//! Pre-lowering bytecode scans: where blocks begin, and which loop regions
//! are worth hoisting an array receiver out of.
//!
//! Both walk the bytecode before a single CLIF instruction exists, and both
//! answer questions about the CFG rather than about codegen — which is why
//! they live apart from the body that consumes their answers.

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;

use super::alloc;
use super::emit;

/// Block starts: jump targets, loop headers, and the fall-through after a
/// conditional jump. Walked through the shared decoder so an operand word can
/// never be mistaken for an opcode.
pub(super) fn block_starts(code: &[u16], pool: &[PoolEntry]) -> Result<Vec<usize>, String> {
    let mut starts: Vec<usize> = vec![0];
    let mut ip = 0usize;
    while ip < code.len() {
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        let op = OpCode::from_u8(code[ip] as u8).ok_or("clif: unknown opcode")?;
        let next = ip + info.len;
        match op {
            OpCode::Jump | OpCode::JumpIfFalse | OpCode::JumpIfTrue => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                starts.push(ip + 3 + off);
                if matches!(op, OpCode::JumpIfFalse | OpCode::JumpIfTrue) {
                    starts.push(next);
                }
            }
            OpCode::Loop => {
                let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
                starts.push((ip + 3).saturating_sub(off));
            }
            _ => {}
        }
        ip = next;
    }
    starts.sort_unstable();
    starts.dedup();
    Ok(starts)
}

/// The register a char-indexing `Intrinsic` at `ip` took its receiver from.
///
/// The call sequence stages the receiver into the intrinsic's own destination
/// register — `Move r10 = r1` then `Intrinsic r10 = intrinsic(…)` — so the
/// destination is rewritten every iteration and is never itself hoistable.
/// The register worth caching is the one the `Move` copied FROM, and this
/// walks the region forward to find the last `Move` that defined the staging
/// register before `ip`.
///
/// `None` when the intrinsic is not char-indexing, or when its receiver was
/// produced by something other than a plain `Move` (a property read, a call)
/// — in which case there is no loop-invariant register to hang a cache on.
fn str_receiver_source(
    code: &[u16],
    pool: &[PoolEntry],
    header: usize,
    ip: usize,
) -> Result<Option<usize>, String> {
    if !varn_core::intrinsic_ops::intrinsic_is_char_index((code[ip + 1] >> 8) as u8) {
        return Ok(None);
    }
    let staged = (code[ip] >> 8) as usize;
    let mut source = None;
    let mut j = header;
    while j < ip {
        let info = decode(code, j, pool).ok_or("clif: undecodable opcode")?;
        if OpCode::from_u8(code[j] as u8) == Some(OpCode::Move) && (code[j] >> 8) as usize == staged
        {
            source = Some((code[j + 1] >> 8) as usize);
        }
        j += info.len;
    }
    Ok(source)
}

/// Loop hoisting plan.
///
/// Post-linearization (loop-aware RPO in ssa/emit) loops are CONTIGUOUS:
/// a `Loop` op at L targeting T delimits the region [T, L). For each
/// region, array receivers that are never redefined inside it get their
/// payload pointer resolved ONCE in the fall-through preheader into a
/// cache variable (0 = invalid — matching the template's loop_hoist
/// sentinel; no live allocation sits at address 0). Accesses test the
/// cache and skip the whole tag/generation/slot walk on hit. Sound for
/// the routed subset: nothing under a routed frame can run a GC, and an
/// append only mutates the payload's inner words, never the payload
/// pointer itself.
///
/// An ALLOCATING function may cache too, but only over a loop whose own
/// body allocates nothing. The pointer a cache holds is the address of
/// the receiver's heap slot, and any allocation can push that slot's Vec
/// (nursery or old gen) past its capacity and move it — a collection is
/// not the only way to invalidate one. An allocation-free region cannot
/// do that, and the collection that CAN happen at its back edge resets
/// every cache from the safepoint's taken arm
/// (`alloc::emit_backedge_safepoint`). That pair is the whole soundness
/// argument; `readonly` stays false here regardless, since the mid-end
/// must not hoist a resolve across the safepoint on its own.
pub(super) fn loop_regions(
    proto: &varn_types::FunctionProto,
    code: &[u16],
    pool: &[PoolEntry],
    has_alloc: bool,
) -> Result<Vec<emit::Region>, String> {
    let mut regions: Vec<emit::Region> = Vec::new();
    let mut scan_ip = 0usize;
    while scan_ip < code.len() {
        let ip = scan_ip;
        let info = decode(code, ip, pool).ok_or("clif: undecodable opcode")?;
        if OpCode::from_u8(code[ip] as u8) == Some(OpCode::Loop) {
            let off = ((code[ip + 1] as u32) << 16 | code[ip + 2] as u32) as usize;
            let header = (ip + 3) - off;
            // The region gate reads `Intrinsic` by wire byte: a `charCodeAt`
            // loop allocates nothing, and treating it as an allocation kept
            // BOTH caches — the array payloads and the string bytes — out of
            // exactly the loops that need them most.
            let hoistable = header > 0
                && (!has_alloc
                    || !alloc::has_alloc_scan(
                        &code[header..ip],
                        pool,
                        alloc::IntrinsicScan::ByWireByte,
                    )?);
            if hoistable {
                let mut receivers: Vec<usize> = Vec::new();
                let mut string_sites: Vec<(usize, usize)> = Vec::new();
                let mut written: Vec<usize> = Vec::new();
                let mut redefined: Vec<usize> = Vec::new();
                let mut j = header;
                while j < ip {
                    let jinfo = decode(code, j, pool).ok_or("clif: undecodable opcode")?;
                    let jop = OpCode::from_u8(code[j] as u8).ok_or("clif: unknown opcode")?;
                    let dest = (code[j] >> 8) as usize;
                    match jop {
                        OpCode::ArrayGetIndex | OpCode::ArrayLength => {
                            receivers.push((code[j + 1] >> 8) as usize);
                            redefined.push(dest);
                        }
                        OpCode::ArraySetIndex => {
                            receivers.push(dest);
                            written.push(dest);
                        }
                        OpCode::CallSelf => redefined.push((code[j + 1] >> 8) as usize),
                        // An `Intrinsic` stages its receiver in `dest` and its
                        // arguments in the registers above it, so `dest` is
                        // both the receiver and the result slot: it IS
                        // redefined, and the fallthrough arm below records
                        // that. The register the receiver was COPIED FROM is
                        // what can be hoisted, and the bytecode does not name
                        // it here — so string caching keys off the copy source
                        // found by `str_receiver_source`.
                        OpCode::Intrinsic => {
                            if let Some(src) = str_receiver_source(code, pool, header, j)? {
                                string_sites.push((j, src));
                            }
                            redefined.push(dest);
                        }
                        _ => {
                            if jinfo.def.is_some() {
                                redefined.push(dest);
                            }
                        }
                    }
                    j += jinfo.len;
                }
                receivers.sort_unstable();
                receivers.dedup();
                receivers.retain(|r| !redefined.contains(r));
                // A receiver redefined inside the region is not loop-invariant,
                // so its sites go too — the cache would describe a string the
                // loop has already replaced.
                string_sites.retain(|(_, r)| !redefined.contains(r));
                let mut strings: Vec<usize> = string_sites.iter().map(|(_, r)| *r).collect();
                strings.sort_unstable();
                strings.dedup();
                // A receiver the region only READS gets the stronger cache:
                // with no allocation in the region either, its element
                // pointer, length and repr are all loop-invariant.
                let read_only: Vec<usize> = receivers
                    .iter()
                    .copied()
                    .filter(|r| !written.contains(r))
                    .collect();
                if !receivers.is_empty() || !strings.is_empty() {
                    if super::trace() {
                        eprintln!(
                            "CLIF REGION {:?}: [{header}..{ip}] recv={receivers:?} \
                             ro={read_only:?} str={strings:?}",
                            proto.name
                        );
                    }
                    regions.push(emit::Region {
                        header,
                        back_edge: ip,
                        arrays: receivers,
                        read_only,
                        string_sites,
                        strings,
                    });
                }
            }
        }
        scan_ip += info.len;
    }
    Ok(regions)
}
