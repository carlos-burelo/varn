mod dominance;
mod structural;
mod type_rules;

pub use structural::verify;
pub use structural::VerifyResult;
pub(crate) use structural::{inst_uses, out_edges, recompute_preds, term_value_uses};
pub(crate) use type_rules::convert_result_ty;
