use super::stmt::Stmt;
use super::AstMetadata;
use crate::source::SourceRange;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Program {
    pub filename: Rc<str>,
    pub body: Vec<Stmt>,
    pub range: SourceRange,
    pub metadata: AstMetadata,
}
