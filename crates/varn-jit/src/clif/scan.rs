//! Pre-lowering bytecode scans: where blocks begin, and which loop regions
//! are worth hoisting an array receiver out of.
//!
//! Both walk the bytecode before a single CLIF instruction exists, and both
//! answer questions about the CFG rather than about codegen — which is why
//! they live apart from the body that consumes their answers.

use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;

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
    // `charCodeAt` reaches the lowering either as a `Str`-domain `Intrinsic`
    // or as a `CallNativeOp` (the core-type method table dispatch), depending
    // on how the checker resolved the receiver. Both stage the receiver into
    // the op's own destination register with a `Move`, so both are hoistable.
    let is_char_site = match OpCode::from_u8(code[ip] as u8) {
        Some(OpCode::Intrinsic) => {
            varn_core::intrinsic_ops::intrinsic_is_char_index((code[ip + 1] >> 8) as u8)
        }
        Some(OpCode::CallNativeOp) => call_native_op_id(code, pool, ip)
            .is_some_and(varn_core::op_id::is_str_char_index_op_id),
        _ => false,
    };
    if !is_char_site {
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

/// The stable op-id a `CallNativeOp` at `ip` dispatches to (its pool constant
/// is an int literal). `None` for any other opcode or a non-int constant.
pub(super) fn call_native_op_id(code: &[u16], pool: &[PoolEntry], ip: usize) -> Option<u64> {
    if OpCode::from_u8(code[ip] as u8) != Some(OpCode::CallNativeOp) {
        return None;
    }
    let cidx = code[ip + 1] as usize;
    match pool.get(cidx) {
        Some(PoolEntry::Literal(varn_types::chunk::Literal::Int(i))) => Some(*i as u64),
        _ => None,
    }
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
    _has_alloc: bool,
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
            let hoistable = header > 0;
            if hoistable {
                let mut receivers: Vec<usize> = Vec::new();
                let mut string_sites: Vec<(usize, usize)> = Vec::new();
                let mut objects: Vec<usize> = Vec::new();
                let mut written: Vec<usize> = Vec::new();
                let mut redefined: Vec<usize> = Vec::new();
                let mut j = header;
                while j < ip {
                    let jinfo = decode(code, j, pool).ok_or("clif: undecodable opcode")?;
                    let jop = OpCode::from_u8(code[j] as u8).ok_or("clif: unknown opcode")?;
                    let dest = (code[j] >> 8) as usize;
                    if let Some(def_reg) = jinfo.def {
                        redefined.push(def_reg as usize);
                    }
                    match jop {
                        OpCode::ArrayGetIndex | OpCode::ArrayLength => {
                            receivers.push((code[j + 1] >> 8) as usize);
                        }
                        OpCode::ArraySetIndex => {
                            receivers.push(dest);
                            written.push(dest);
                        }
                        OpCode::SetIndex | OpCode::ArrayPush | OpCode::ArrayExtend => {
                            let r = (code[j] >> 8) as usize;
                            written.push(r);
                        }
                        OpCode::CallMethod | OpCode::InvokeVirtual => {
                            let this_reg = (code[j + 1] & 0xFF) as usize;
                            written.push(this_reg);
                        }
                        OpCode::GetFixedField => {
                            let obj_r = (code[j + 1] >> 8) as usize;
                            objects.push(obj_r);
                        }
                        OpCode::SetFixedField => {
                            let obj_r = dest;
                            objects.push(obj_r);
                        }
                        OpCode::Intrinsic | OpCode::CallNativeOp => {
                            if let Some(src) = str_receiver_source(code, pool, header, j)? {
                                string_sites.push((j, src));
                            }
                        }
                        _ => {}
                    }
                    j += jinfo.len;
                }
                receivers.sort_unstable();
                receivers.dedup();
                receivers.retain(|r| !redefined.contains(r));
                objects.sort_unstable();
                objects.dedup();
                objects.retain(|r| !redefined.contains(r));
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
                let mut induction_increments: Vec<usize> = Vec::new();
                let mut induction_var: Option<usize> = None;
                let mut induction_bound: Option<usize> = None;
                let mut bounds_hoistable = Vec::new();
                let mut bounds_safe_arith = Vec::new();
                if let Some(info) = find_induction_info(code, pool, header, ip) {
                    induction_increments.push(info.increment_ip);
                    induction_var = Some(info.cond_var);
                    induction_bound = Some(info.bound_var);
                    // Detect bounds-hoistable array accesses.
                    let (bh, sa) = find_bounds_hoistable(
                        code,
                        pool,
                        header,
                        ip,
                        info.cond_var,
                        &redefined,
                        &read_only,
                    );
                    bounds_hoistable = bh;
                    bounds_safe_arith = sa;
                }
                if !receivers.is_empty()
                    || !strings.is_empty()
                    || !objects.is_empty()
                    || !induction_increments.is_empty()
                {
                    if super::trace() {
                        eprintln!(
                            "CLIF REGION {:?}: [{header}..{ip}] recv={receivers:?} \
                             ro={read_only:?} str={strings:?} obj={objects:?} inc={induction_increments:?} \
                             bounds_hoist={bounds_hoistable:?} safe_arith={bounds_safe_arith:?}",
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
                        objects,
                        induction_increments,
                        induction_var,
                        induction_bound,
                        bounds_hoistable,
                        bounds_safe_arith,
                    });
                }
            }
        }
        scan_ip += info.len;
    }
    Ok(regions)
}

/// Full induction-variable information for a loop region.
pub(super) struct InductionInfo {
    /// Register of the induction variable (left operand of header comparison).
    pub cond_var: usize,
    /// Register of the loop bound (right operand of header comparison).
    pub bound_var: usize,
    /// Bytecode offset of the AddImm/SubImm that increments the induction var.
    pub increment_ip: usize,
}

/// Identifies the loop's induction variable, bound, and increment instruction.
fn find_induction_info(
    code: &[u16],
    pool: &[PoolEntry],
    header: usize,
    back_edge: usize,
) -> Option<InductionInfo> {
    // 1. Find the loop condition check at or near the header.
    let mut j = header;
    let mut cond_var: Option<usize> = None;
    let mut bound_var: Option<usize> = None;
    let scan_limit = (header + 64).min(back_edge);
    while j < scan_limit {
        let jinfo = decode(code, j, pool)?;
        let jop = OpCode::from_u8(code[j] as u8)?;
        match jop {
            OpCode::LtInt | OpCode::LteInt | OpCode::GtInt | OpCode::GteInt => {
                let w1 = code[j + 1];
                let op1 = (w1 >> 8) as usize;
                let op2 = (w1 & 0xFF) as usize;
                cond_var = Some(op1);
                bound_var = Some(op2);
                break;
            }
            _ => {}
        }
        j += jinfo.len;
    }

    let cond_var = cond_var?;
    let bound_var = bound_var?;

    // 2. Look for AddImm / SubImm that updates cond_var before back_edge.
    let mut k = header;
    let mut last_increment: Option<usize> = None;
    while k < back_edge {
        let kinfo = decode(code, k, pool)?;
        let kop = OpCode::from_u8(code[k] as u8)?;
        if matches!(kop, OpCode::AddImm | OpCode::SubImm) {
            let dest = (code[k] >> 8) as usize;
            let src = (code[k + 1] >> 8) as usize;
            if src == cond_var || dest == cond_var {
                last_increment = Some(k);
            } else {
                let next = k + kinfo.len;
                if next < back_edge {
                    if let Some(next_op) = OpCode::from_u8(code[next] as u8) {
                        if next_op == OpCode::Move {
                            let move_dest = (code[next] >> 8) as usize;
                            let move_src = (code[next + 1] >> 8) as usize;
                            if move_dest == cond_var && move_src == dest {
                                last_increment = Some(k);
                            }
                        }
                    }
                }
            }
        }
        k += kinfo.len;
    }

    Some(InductionInfo {
        cond_var,
        bound_var,
        increment_ip: last_increment?,
    })
}

/// Scans the loop body for `ArrayGetIndex` operations whose index is derived
/// from the induction variable, either directly (`a[k]`), via a
/// loop-invariant base offset (`a[base + k]`), or an affine pattern (`a[k * stride + base]`).
fn find_bounds_hoistable(
    code: &[u16],
    pool: &[PoolEntry],
    header: usize,
    back_edge: usize,
    cond_var: usize,
    redefined: &[usize],
    read_only: &[usize],
) -> (Vec<emit::BoundsHoistable>, Vec<usize>) {
    let mut hoistable = Vec::new();
    let mut safe_arith = Vec::new();
    let mut j = header;
    while j < back_edge {
        let jinfo = match decode(code, j, pool) {
            Some(i) => i,
            None => break,
        };
        let jop = match OpCode::from_u8(code[j] as u8) {
            Some(o) => o,
            None => {
                j += jinfo.len;
                continue;
            }
        };
        if jop == OpCode::ArrayGetIndex {
            let arr_reg = (code[j + 1] >> 8) as usize;
            let idx_reg = (code[j + 1] & 0xFF) as usize;
            // Only consider read-only arrays (they get view caches).
            if read_only.contains(&arr_reg) {
                if let Some(origin) =
                    find_index_origin(code, pool, header, j, idx_reg, cond_var, redefined)
                {
                    hoistable.push(emit::BoundsHoistable {
                        array_reg: arr_reg,
                        base_reg: origin.base_reg,
                        stride_reg: origin.stride_reg,
                    });
                    safe_arith.extend(origin.safe_ips);
                }
            }
        }
        j += jinfo.len;
    }
    // Deduplicate by array_reg (keep first occurrence).
    hoistable.sort_by_key(|h| h.array_reg);
    hoistable.dedup_by_key(|h| h.array_reg);
    safe_arith.sort_unstable();
    safe_arith.dedup();
    (hoistable, safe_arith)
}

struct IndexOrigin {
    base_reg: Option<usize>,
    stride_reg: Option<usize>,
    safe_ips: Vec<usize>,
}

/// Identifies whether `idx_reg` is derived from `cond_var`:
/// - `cond_var` directly
/// - `cond_var + base` or `base + cond_var`
/// - `cond_var * stride` or `stride * cond_var`
/// - `(cond_var * stride) + base` or `base + (cond_var * stride)`
fn find_index_origin(
    code: &[u16],
    pool: &[PoolEntry],
    header: usize,
    before_ip: usize,
    idx_reg: usize,
    cond_var: usize,
    redefined: &[usize],
) -> Option<IndexOrigin> {
    if idx_reg == cond_var {
        return Some(IndexOrigin {
            base_reg: None,
            stride_reg: None,
            safe_ips: Vec::new(),
        });
    }

    // Walk forward from header to before_ip to find the LAST definition of idx_reg.
    let mut def_op: Option<(OpCode, usize, usize, usize)> = None;
    let mut k = header;
    while k < before_ip {
        let kinfo = decode(code, k, pool)?;
        let kop = OpCode::from_u8(code[k] as u8)?;
        match kop {
            OpCode::AddInt | OpCode::MulInt => {
                let dest = (code[k] >> 8) as usize;
                if dest == idx_reg {
                    let w1 = code[k + 1];
                    let op1 = (w1 >> 8) as usize;
                    let op2 = (w1 & 0xFF) as usize;
                    def_op = Some((kop, op1, op2, k));
                }
            }
            _ => {
                let dest = (code[k] >> 8) as usize;
                if dest == idx_reg {
                    def_op = None;
                }
            }
        }
        k += kinfo.len;
    }

    let (op, op1, op2, def_ip) = def_op?;
    match op {
        OpCode::AddInt => {
            // Pattern 1: AddInt(cond_var, base) or AddInt(base, cond_var)
            if op1 == cond_var && !redefined.contains(&op2) {
                return Some(IndexOrigin {
                    base_reg: Some(op2),
                    stride_reg: None,
                    safe_ips: vec![def_ip],
                });
            }
            if op2 == cond_var && !redefined.contains(&op1) {
                return Some(IndexOrigin {
                    base_reg: Some(op1),
                    stride_reg: None,
                    safe_ips: vec![def_ip],
                });
            }

            // Pattern 2: AddInt(scaled, base) or AddInt(base, scaled)
            let (scaled_reg, base_r) = if !redefined.contains(&op2) {
                (op1, op2)
            } else if !redefined.contains(&op1) {
                (op2, op1)
            } else {
                return None;
            };

            // Scan backwards for the defining MulInt of scaled_reg
            let mut mul_def: Option<(usize, usize, usize)> = None;
            let mut m = header;
            while m < def_ip {
                let minfo = decode(code, m, pool)?;
                let mop = OpCode::from_u8(code[m] as u8)?;
                if mop == OpCode::MulInt {
                    let dest = (code[m] >> 8) as usize;
                    if dest == scaled_reg {
                        let w1 = code[m + 1];
                        let m_op1 = (w1 >> 8) as usize;
                        let m_op2 = (w1 & 0xFF) as usize;
                        mul_def = Some((m_op1, m_op2, m));
                    }
                } else if (code[m] >> 8) as usize == scaled_reg {
                    mul_def = None;
                }
                m += minfo.len;
            }

            let (m_op1, m_op2, mul_ip) = mul_def?;
            let stride_r = if m_op1 == cond_var && !redefined.contains(&m_op2) {
                m_op2
            } else if m_op2 == cond_var && !redefined.contains(&m_op1) {
                m_op1
            } else {
                return None;
            };

            Some(IndexOrigin {
                base_reg: Some(base_r),
                stride_reg: Some(stride_r),
                safe_ips: vec![def_ip, mul_ip],
            })
        }
        OpCode::MulInt => {
            // Pattern 3: MulInt(cond_var, stride) or MulInt(stride, cond_var)
            let stride_r = if op1 == cond_var && !redefined.contains(&op2) {
                op2
            } else if op2 == cond_var && !redefined.contains(&op1) {
                op1
            } else {
                return None;
            };

            Some(IndexOrigin {
                base_reg: None,
                stride_reg: Some(stride_r),
                safe_ips: vec![def_ip],
            })
        }
        _ => None,
    }
}
