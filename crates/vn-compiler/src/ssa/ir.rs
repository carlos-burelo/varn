mod control;
mod func;
mod ids;
mod ops;

pub use control::Terminator;
pub use func::{Block, Inst, SsaFunc};
pub use ids::{BlockId, Value, ValueDef, VarId};
pub use ops::InstKind;
