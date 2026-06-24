pub mod cfg;
pub mod const_fold;
pub mod dce;

use crate::ssa::ir::SsaFunc;

pub fn optimize(func: &mut SsaFunc) {
    let mut iterations = 0;
    loop {
        let mut changed = false;

        changed |= const_fold::run(func);

        changed |= dce::run(func);

        changed |= cfg::simplify_and_compact(func);

        if !changed || iterations >= 100 {
            break;
        }
        iterations += 1;
    }
}
