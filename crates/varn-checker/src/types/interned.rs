





























mod hash;
mod ids;
mod interning;
mod merge;
mod names;
mod resolution;
mod slice;
mod table;

pub use ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
pub use slice::TySlice;
pub use table::CheckerTyTable;
