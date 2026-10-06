use super::ids::{BlockId, Value};

#[derive(Debug, Clone)]
pub enum Terminator {
    Return(Option<Value>),

    Throw(Value),
    Jump {
        target: BlockId,
        args: Vec<Value>,
    },
    Branch {
        cond: Value,
        then_blk: BlockId,
        then_args: Vec<Value>,
        else_blk: BlockId,
        else_args: Vec<Value>,
    },

    Unreachable,
}
