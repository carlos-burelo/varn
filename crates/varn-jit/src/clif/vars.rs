//! The Variable file a lowering works against.
//!
//! One `cranelift-frontend` Variable per VM register — that is what rebuilds
//! SSA for free from flat bytecode — plus the payload caches a hoisted loop
//! region needs. Declaring them is the first thing any lowering does and has
//! nothing to do with which opcodes follow, so it lives here.

use cranelift_codegen::ir::types;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;
use varn_core::OpCode;
use varn_types::bytecode::decode;
use varn_types::chunk::PoolEntry;
use varn_types::register_meta::RegisterMeta;

use super::emit::{self, meta_is_float, state_meta_int};

/// Every register's Variable, plus the loop-region payload caches.
pub(super) struct VarFile {
    /// One per VM register, indexed by register number.
    pub vars: Vec<Variable>,
    /// One entry per (loop region header, array receiver register).
    pub cache_vars: HashMap<(usize, usize), emit::RegionCache>,
    /// One entry per (loop region header, string receiver register).
    pub str_caches: HashMap<(usize, usize), emit::StrRegionCache>,
    /// One entry per (loop region header, object receiver register).
    pub obj_caches: HashMap<(usize, usize), emit::ObjRegionCache>,
    /// Flat list of every cache Variable, for the back-edge safepoint — it
    /// invalidates all of them at once and has no way to tell which region it
    /// sits in.
    pub all_caches: Vec<Variable>,
}

/// Determine the expected `ArrayRepr` disc for a read-only receiver in a
/// loop region by scanning the region's body for `ArrayGetIndex` sites
/// that use it. If EVERY such site reads into a register of the same
/// non-Boxed repr (all Int or all Float), that repr's disc is returned.
/// Otherwise returns `None` (mixed or boxed — no specialization possible).
///
/// Disc values: 0 = Boxed, 1 = I64, 2 = F64 (matching `arrays::ElemRepr`).
fn expected_disc_for_receiver(
    code: &[u16],
    pool: &[PoolEntry],
    register_meta: &[RegisterMeta],
    header: usize,
    back_edge: usize,
    receiver: usize,
) -> Option<i64> {
    let mut agreed: Option<i64> = None;
    let mut ip = header;
    while ip < back_edge {
        let info = decode(code, ip, pool)?;
        let op = OpCode::from_u8((code[ip] & 0xFF) as u8);
        if matches!(op, Some(OpCode::ArrayGetIndex)) {
            let obj_r = (code[ip + 1] >> 8) as usize;
            if obj_r == receiver {
                let dest = (code[ip] >> 8) as usize;
                let disc = if meta_is_float(register_meta, dest) {
                    2 // F64
                } else if state_meta_int(register_meta, dest) {
                    1 // I64
                } else {
                    0 // Boxed — can't skip repr check
                };
                // Boxed destination → no specialization: the disc branch
                // would be needed for element unboxing anyway.
                if disc == 0 {
                    return None;
                }
                match agreed {
                    None => agreed = Some(disc),
                    Some(prev) if prev != disc => return None, // mixed reprs
                    _ => {}
                }
            }
        }
        ip += info.len;
    }
    agreed
}

/// Declare the whole file. `want_roots` marks the I64 Variables as needing
/// stack maps, which Cranelift requires BEFORE any use — it explicitly does
/// not retrofit pre-existing ones.
pub(super) fn declare(
    b: &mut FunctionBuilder,
    nregs: usize,
    register_meta: &[RegisterMeta],
    regions: &[emit::Region],
    code: &[u16],
    pool: &[PoolEntry],
    want_roots: bool,
) -> VarFile {
    // A float-typed register (`register_meta[r] == Float`) is an unboxed f64
    // in an `F64` Variable; every other register is a boxed/unboxed i64.
    let vars: Vec<Variable> = (0..nregs)
        .map(|r| {
            let ty = if meta_is_float(register_meta, r) {
                types::F64
            } else {
                types::I64
            };
            let v = b.declare_var(ty);
            // Floats are never GC references.
            if want_roots && ty == types::I64 {
                b.declare_var_needs_stack_map(v);
            }
            v
        })
        .collect();
    // One payload-cache variable per (loop region, receiver register).
    // Zero-defined at entry like every var, and 0 means "not resolved":
    // the frontend's all-paths-defined rule and the sentinel share a def.
    let cache_vars: HashMap<(usize, usize), emit::RegionCache> = regions
        .iter()
        .flat_map(|reg| {
            reg.arrays
                .iter()
                .map(move |r| ((reg.header, reg.back_edge, *r), reg.read_only.contains(r)))
        })
        .map(|((h, e, r), read_only)| {
            let payload = b.declare_var(types::I64);
            let view = read_only.then(|| {
                [
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                ]
            });
            // For read-only receivers, determine if all ArrayGetIndex
            // accesses agree on a single non-Boxed repr. If so, the
            // preheader can validate the disc once and the body skips
            // both repr branches per access.
            let repr_validated_disc = read_only
                .then(|| expected_disc_for_receiver(code, pool, register_meta, h, e, r))
                .flatten();
            // Bounds hoisting: the preheader will validate that the loop
            // bound fits within the array length. When it does, data != 0
            // guarantees BOTH repr match AND bounds safety, so the loop body
            // can skip per-iteration bounds checks entirely.
            let bounds_guaranteed = repr_validated_disc.is_some()
                && regions.iter().any(|reg| {
                    reg.header == h
                        && reg.induction_bound.is_some()
                        && reg.bounds_hoistable.iter().any(|bh| bh.array_reg == r)
                });
            (
                (h, r),
                emit::RegionCache {
                    payload,
                    view,
                    repr_validated_disc,
                    bounds_guaranteed,
                },
            )
        })
        .collect();
    // One pair of Variables per (region, string receiver): the byte pointer
    // and the byte length resolved once in the preheader.
    let str_caches: HashMap<(usize, usize), emit::StrRegionCache> = regions
        .iter()
        .flat_map(|reg| reg.strings.iter().map(move |r| (reg.header, *r)))
        .map(|key| {
            (
                key,
                emit::StrRegionCache {
                    bytes: b.declare_var(types::I64),
                    len: b.declare_var(types::I64),
                },
            )
        })
        .collect();
    // One Variable per (region, object receiver): the inline field data base pointer.
    let obj_caches: HashMap<(usize, usize), emit::ObjRegionCache> = regions
        .iter()
        .flat_map(|reg| reg.objects.iter().map(move |r| (reg.header, *r)))
        .map(|key| {
            (
                key,
                emit::ObjRegionCache {
                    data_base: b.declare_var(types::I64),
                },
            )
        })
        .collect();
    // The back-edge safepoint zeroes every cache after a collection. A string
    // cache holds a raw interior pointer, so leaving it out would be the one
    // way this survives a GC — it must be in the same list as the payloads.
    let all_caches: Vec<Variable> = cache_vars
        .values()
        .flat_map(|c| std::iter::once(c.payload).chain(c.view.into_iter().flatten()))
        .chain(str_caches.values().flat_map(|c| [c.bytes, c.len]))
        .chain(obj_caches.values().map(|c| c.data_base))
        .collect();
    VarFile {
        vars,
        cache_vars,
        str_caches,
        obj_caches,
        all_caches,
    }
}
