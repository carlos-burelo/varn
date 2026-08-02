//! The Variable file a lowering works against.
//!
//! One `cranelift-frontend` Variable per VM register — that is what rebuilds
//! SSA for free from flat bytecode — plus the payload caches a hoisted loop
//! region needs. Declaring them is the first thing any lowering does and has
//! nothing to do with which opcodes follow, so it lives here.

use cranelift_codegen::ir::types;
use cranelift_frontend::{FunctionBuilder, Variable};
use std::collections::HashMap;
use varn_types::register_meta::RegisterMeta;

use super::emit::{self, meta_is_float};

/// Every register's Variable, plus the loop-region payload caches.
pub(super) struct VarFile {
    /// One per VM register, indexed by register number.
    pub vars: Vec<Variable>,
    /// One entry per (loop region header, receiver register).
    pub cache_vars: HashMap<(usize, usize), emit::RegionCache>,
    /// Flat list of every cache Variable, for the back-edge safepoint — it
    /// invalidates all of them at once and has no way to tell which region it
    /// sits in.
    pub all_caches: Vec<Variable>,
}

/// Declare the whole file. `want_roots` marks the I64 Variables as needing
/// stack maps, which Cranelift requires BEFORE any use — it explicitly does
/// not retrofit pre-existing ones.
pub(super) fn declare(
    b: &mut FunctionBuilder,
    nregs: usize,
    register_meta: &[RegisterMeta],
    regions: &[emit::Region],
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
        .flat_map(|(h, _, regs, ro)| regs.iter().map(move |r| ((*h, *r), ro.contains(r))))
        .map(|(k, read_only)| {
            let payload = b.declare_var(types::I64);
            let view = read_only.then(|| {
                [
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                    b.declare_var(types::I64),
                ]
            });
            (k, emit::RegionCache { payload, view })
        })
        .collect();
    let all_caches: Vec<Variable> = cache_vars
        .values()
        .flat_map(|c| std::iter::once(c.payload).chain(c.view.into_iter().flatten()))
        .collect();
    VarFile {
        vars,
        cache_vars,
        all_caches,
    }
}
