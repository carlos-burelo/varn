//! `varn_checker::types::Type` → `varn_tir::BackendTy`.
//!
//! The narrowing that `CgTy → HirType → SlotKind` did silently, done once and
//! explicitly. A type the TIR does not model precisely (a bare type
//! parameter, an imported type, a host-shaped intrinsic like `Regex` or
//! `Task`, an arbitrary function type) lowers to `Dynamic(Unannotated)` — the
//! value carries no more type information here, which is the honest state.

mod lower;
mod resolve;
mod union;

pub use lower::{lower_type, prime};
pub use resolve::{NameResolver, NoNames};
