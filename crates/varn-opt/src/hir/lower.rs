//! AST -> HIR lowering.
//!
//! Stage 1 grows this to cover the subset `tests/main` exercises. Until a
//! construct is handled, lowering returns `OptError::Unsupported`, and the
//! caller falls back to legacy codegen.

use crate::hir::HirModule;
use crate::{OptError, OptInput};

/// Lower a whole program to HIR. Stage 0: not yet implemented — declines so the
/// `VN_OPT` gate is a safe no-op.
pub fn lower_program(_input: &OptInput<'_>) -> Result<HirModule, OptError> {
    Err(OptError::Unsupported("AST->HIR lowering not yet implemented"))
}
