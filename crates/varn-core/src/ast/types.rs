use super::arena::ExprId;
use super::decl::InterfaceMember;
use super::expr::AstId;
use crate::kinds::TypeKind;
use crate::source::SourceRange;
use crate::Atom;

/// The `ExprId`s carried here (`TypeKind::Typeof`, `Decorator::expression`)
/// are handles into an `AstArena`, but `TypeNode` does not name *which*
/// arena — it is implicitly the arena of the module that parsed this
/// `TypeNode`. That is safe for a `TypeNode` a binder walks against its own
/// module's arena, but a `TypeNode` that traveled to another module (e.g.
/// `Symbol::alias_node` for a generic `core:types` alias, expanded while
/// checking an importer) carries an `ExprId` that is only resolvable against
/// its *origin* module's arena, never the importer's. Resolving it against
/// the wrong arena is not a bounds-checked error — `ExprId` is a plain index,
/// so it silently reads whatever unrelated expression happens to sit at that
/// slot. Any code that resolves one of these `ExprId`s must first establish
/// which module's arena it belongs to; never assume "the current arena".
pub type AstTypeKind = TypeKind<
    Box<TypeNode>,
    Atom,
    Vec<TypeNode>,
    (Vec<TypeParam>, Box<TypeNode>),
    Vec<InterfaceMember>,
    ExprId,
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
    pub name: Atom,
    pub constraint: Option<TypeNode>,
    pub default: Option<TypeNode>,
    pub range: SourceRange,
}

#[derive(Clone, Debug)]
pub struct Decorator {
    pub expression: ExprId,
    pub range: SourceRange,
}
