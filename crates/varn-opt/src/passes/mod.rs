pub mod algebraic;
pub mod cfg;
pub mod const_fold;
pub mod cse;
pub mod dce;
pub mod fixed_fields;
pub mod licm;
pub mod tco;

use crate::ssa::ir::SsaFunc;

pub fn optimize(func: &mut SsaFunc) {
    let mut iterations = 0;
    loop {
        let mut changed = false;

        changed |= tco::run(func);

        changed |= const_fold::run(func);

        // Needs const_fold's literals in place to recognize `x * 1`, and
        // feeds it back: collapsing one operand often makes the next
        // instruction fully constant on the following round.
        changed |= algebraic::run(func);

        // After folding, so a computation that collapsed to a literal is
        // deduplicated against the other copies of that literal; before DCE,
        // which is what actually deletes the instructions CSE orphans.
        changed |= cse::run(func);

        changed |= fixed_fields::run(func);

        changed |= licm::run(func);

        changed |= dce::run(func);

        changed |= cfg::simplify_and_compact(func);

        if !changed || iterations >= 100 {
            break;
        }
        iterations += 1;
    }
}
