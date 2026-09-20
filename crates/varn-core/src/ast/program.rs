use super::arena::StmtId;
use crate::source::SourceRange;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Program {
    pub filename: Arc<str>,
    pub body: Vec<StmtId>,
    pub range: SourceRange,
}
