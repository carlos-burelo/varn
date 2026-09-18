use super::arena::StmtId;
use super::AstMetadata;
use crate::source::SourceRange;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Program {
    pub filename: Rc<str>,
    pub body: Vec<StmtId>,
    pub range: SourceRange,
    pub metadata: AstMetadata,
}
