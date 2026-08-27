pub mod algebraic;
pub mod cfg;
pub mod const_fold;
pub mod cse;
pub mod dce;
pub mod escape;
pub mod fixed_fields;
pub mod licm;
pub mod monomorphize;
pub mod redundant_guards;
pub mod state_machine;
pub mod tco;

use crate::hir::ctor_summary::CtorSummaries;
use crate::ssa::ir::SsaFunc;

/// Optimize without cross-function knowledge. `escape` needs constructor
/// summaries and is simply skipped; every other pass is intra-function.
pub fn optimize(func: &mut SsaFunc) {
    optimize_with(func, &CtorSummaries::default())
}

pub fn optimize_with(func: &mut SsaFunc, summaries: &CtorSummaries) {
    let mut iterations = 0;
    loop {
        let mut changed = false;

        changed |= tco::run(func);

        changed |= const_fold::run(func);

        changed |= redundant_guards::run(func);

        changed |= monomorphize::run(func);

        // Needs const_fold's literals in place to recognize `x * 1`, and
        // feeds it back: collapsing one operand often makes the next
        // instruction fully constant on the following round.
        changed |= algebraic::run(func);

        // After folding, so a computation that collapsed to a literal is
        // deduplicated against the other copies of that literal; before DCE,
        // which is what actually deletes the instructions CSE orphans.
        changed |= cse::run(func);

        changed |= fixed_fields::run(func);

        // After `fixed_fields` (same disqualification shape, and a literal
        // stored into a field is one fewer escaping use to reason about) and
        // before DCE, which deletes the `global` load the deleted call leaves
        // behind.
        changed |= escape::run(func, summaries);

        changed |= licm::run(func);

        changed |= dce::run(func);

        changed |= cfg::simplify_and_compact(func);

        if !changed || iterations >= 100 {
            break;
        }
        iterations += 1;
    }
}
