mod assign;
mod bindings;
mod calls;
mod ops;
mod traverse;

use super::VerifyError;
use crate::TirModule;

pub(super) fn check(m: &TirModule, errors: &mut Vec<VerifyError>) {
    traverse::check(m, errors)
}
