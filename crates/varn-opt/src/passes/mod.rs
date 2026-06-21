//! SSA optimization passes.
//!
//! Stage 3 adds these one at a time, each independently toggleable and
//! validated against the suite + benchmarks: constant folding, copy
//! propagation, dead-code elimination, CFG simplification, global value
//! numbering, generalized inlining (multi-statement bodies), and escape
//! analysis. Static types drive specialization with no speculation/deopt.

pub mod cfg;
pub mod const_fold;
pub mod dce;

use crate::ssa::ir::SsaFunc;

/// Run all registered SSA optimization passes to a fixpoint (or safety limit).
pub fn optimize(func: &mut SsaFunc) {
    let mut iterations = 0;
    loop {
        let mut changed = false;
        
        // 1. Constant folding
        changed |= const_fold::run(func);
        
        // 2. Dead Code Elimination (DCE)
        changed |= dce::run(func);
        
        // 3. CFG Simplification and Compaction
        changed |= cfg::simplify_and_compact(func);
        
        if !changed || iterations >= 100 {
            break;
        }
        iterations += 1;
    }
}
