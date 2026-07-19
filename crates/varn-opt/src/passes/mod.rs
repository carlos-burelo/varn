pub mod cfg;
pub mod const_fold;
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
