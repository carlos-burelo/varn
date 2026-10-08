pub mod algebraic;
pub mod cfg;
pub mod cfg_dom;
pub mod const_fold;
pub mod cse;
pub mod dce;
pub mod escape;
pub mod fixed_fields;
pub mod licm;
pub mod monomorphize;
pub mod redundant_guards;
pub mod state_machine;

use crate::ssa::ir::SsaFunc;

pub fn optimize(func: &mut SsaFunc) {
    let mut iterations = 0;
    loop {
        let mut changed = false;

        changed |= const_fold::run(func);

        changed |= redundant_guards::run(func);

        changed |= monomorphize::run(func);

        changed |= algebraic::run(func);

        changed |= cse::run(func);

        changed |= fixed_fields::run(func);

        changed |= escape::run(func);

        changed |= licm::run(func);

        changed |= dce::run(func);

        changed |= cfg::simplify_and_compact(func);

        if !changed || iterations >= 100 {
            break;
        }
        iterations += 1;
    }

    crate::ssa::verify::recompute_preds(func);
    if cse::run_global(func) {
        dce::run(func);
    }
}
