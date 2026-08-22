use super::decl::InterfaceMember;
use super::expr::{AstId, Expr};
use crate::kinds::TypeKind;
use crate::source::SourceRange;

pub type AstTypeKind = TypeKind<
    Box<TypeNode>,
    String,
    Vec<TypeNode>,
    (Vec<TypeParam>, Box<TypeNode>),
    Vec<InterfaceMember>,
    Box<Expr>,
>;

#[derive(Clone, Debug)]
pub struct TypeNode {
    pub id: AstId,
    pub kind: AstTypeKind,
    pub range: SourceRange,
}

impl TypeNode {
    pub fn id(&self) -> AstId {
        self.id
    }

    pub fn range(&self) -> &SourceRange {
        &self.range
    }
}

#[derive(Clone, Debug)]
pub struct TypeParam {
    pub name: String,
    pub constraint: Option<TypeNode>,
    pub default: Option<TypeNode>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct Decorator {
    pub expression: Expr,
    pub range: SourceRange,
}
