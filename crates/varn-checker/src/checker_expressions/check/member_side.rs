use crate::checker::Checker;
use varn_core::ast::{ExprId, ExprKind};
use varn_core::source::SourceRange;
use varn_core::{Diagnostic, ErrorCode, TypeKind};
use varn_sem::bind::{BindResult, TypeContext};
use varn_sem::symbol::SymbolKind;
use varn_sem::types::{ClassMemberKind, Type};

impl<'r> Checker<'r> {
    pub(super) fn check_member_side(
        &mut self,
        object: ExprId,
        owner: &Type,
        member: &str,
        range: &SourceRange,
        bind: &BindResult,
    ) {
        let (name, origin) = match self.ty_table.get(owner.0) {
            TypeKind::Named(name, origin) | TypeKind::Generic(name, _, origin) => (
                self.resolve_bind_atom(bind, name),
                origin.map(|o| self.resolve_bind_atom(bind, o)),
            ),
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Fn(_)
            | TypeKind::Object(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => return,
        };
        let scope = bind.scopes.get(self.current_scope);
        let kind_of = |atom| {
            scope
                .resolve(atom, &bind.scopes)
                .map(|sid| bind.arena.get(sid).kind)
        };
        if bind.interner.get(&name).and_then(kind_of) != Some(SymbolKind::Class) {
            return;
        }
        let through_class = match &self.ast_arena.expr(object).kind {
            ExprKind::Identifier { name } => kind_of(*name) == Some(SymbolKind::Class),
            ExprKind::Super => return,
            ExprKind::IntLiteral { .. }
            | ExprKind::FloatLiteral { .. }
            | ExprKind::BigIntLiteral { .. }
            | ExprKind::DecimalLiteral { .. }
            | ExprKind::StrLiteral { .. }
            | ExprKind::CharLiteral { .. }
            | ExprKind::BoolLiteral { .. }
            | ExprKind::NullLiteral
            | ExprKind::RegexLiteral { .. }
            | ExprKind::Template { .. }
            | ExprKind::TaggedTemplate { .. }
            | ExprKind::Missing
            | ExprKind::This
            | ExprKind::Array { .. }
            | ExprKind::Object { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Record { .. }
            | ExprKind::Unary { .. }
            | ExprKind::Update { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Logical { .. }
            | ExprKind::Assign { .. }
            | ExprKind::Conditional { .. }
            | ExprKind::Member { .. }
            | ExprKind::Call { .. }
            | ExprKind::New { .. }
            | ExprKind::Function { .. }
            | ExprKind::Arrow { .. }
            | ExprKind::Sequence { .. }
            | ExprKind::Paren { .. }
            | ExprKind::Await { .. }
            | ExprKind::Spawn { .. }
            | ExprKind::Yield { .. }
            | ExprKind::Spread { .. }
            | ExprKind::Pipeline { .. }
            | ExprKind::Range { .. }
            | ExprKind::NonNull { .. }
            | ExprKind::Try { .. }
            | ExprKind::As { .. }
            | ExprKind::Satisfies { .. }
            | ExprKind::ClassExpr { .. }
            | ExprKind::Match { .. }
            | ExprKind::Is { .. }
            | ExprKind::With { .. }
            | ExprKind::MetaAccess { .. } => false,
        };
        let Some(members) = varn_sem::bind::BindView::new(bind, self.resolver)
            .get_class_members(&name, origin.as_deref())
        else {
            return;
        };
        let mut sides = members
            .iter()
            .filter(|m| m.name.as_ref() == member && m.kind != ClassMemberKind::Constructor)
            .map(|m| m.is_static);
        let Some(is_static) = sides.next() else {
            return;
        };
        if sides.any(|other| other != is_static) || is_static == through_class {
            return;
        }
        let message = if is_static {
            format!("'{member}' is static: reach it through the class, as '{name}.{member}'")
        } else {
            format!("'{member}' belongs to an instance of '{name}', not to the class itself")
        };
        self.emit(
            Diagnostic::error(ErrorCode::StaticMemberOnInstance, message).with_range(*range),
        );
    }
}
