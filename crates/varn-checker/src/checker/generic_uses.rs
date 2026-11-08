use super::Checker;
use varn_core::{Diagnostic, ErrorCode, LangConstraint, LangPrimitive, TypeKind};
use varn_sem::bind::{BindResult, BindView, GenericTypeUse};
use varn_sem::types::Type;

impl Checker<'_> {
    pub(crate) fn check_generic_type_uses(&mut self, bind: &BindResult) {
        for usage in &bind.generic_type_uses {
            self.check_generic_type_use(usage, bind);
        }
    }

    fn check_generic_type_use(&mut self, usage: &GenericTypeUse, bind: &BindResult) {
        let TypeKind::Generic(name, args, _) = self.ty_table.get(usage.ty.0) else {
            return;
        };
        let name = self.resolve_bind_atom(bind, name);
        let Some(declaration) = bind
            .interner
            .get(name.as_ref())
            .and_then(|atom| bind.scopes.get(usage.scope).resolve(atom, &bind.scopes))
            .map(|id| bind.arena.get(id))
        else {
            return;
        };
        let given: Vec<Type> = self
            .ty_table
            .get_list(args)
            .iter()
            .map(|id| Type::resolved(*id))
            .collect();
        let view = BindView::new(bind, self.resolver).at(usage.scope);
        for (bound, arg) in declaration.type_param_constraints.iter().zip(&given) {
            let Some(bound) = bound else {
                continue;
            };
            if varn_sem::compat::types_compatible(bound, arg, Some(&view), &self.ty_table) {
                continue;
            }
            let shown = arg.display(&self.ty_table, &bind.interner);
            let diagnostic =
                if bound.lang_constraint(&self.ty_table) == Some(LangConstraint::Keyable) {
                    Diagnostic::error(
                        ErrorCode::NotKeyable,
                        format!(
                            "type '{shown}' cannot be used as a key of '{name}': {}",
                            self.keyable_alternative(arg)
                        ),
                    )
                } else {
                    Diagnostic::error(
                        ErrorCode::ConstraintViolation,
                        format!(
                            "type '{shown}' does not satisfy constraint '{}'",
                            bound.display(&self.ty_table, &bind.interner)
                        ),
                    )
                };
            self.emit(diagnostic.with_range(usage.range));
        }
    }

    fn keyable_alternative(&self, arg: &Type) -> &'static str {
        match self.ty_table.get(arg.0) {
            TypeKind::Primitive(LangPrimitive::Float) => {
                "a float is not equal to itself when it is NaN; use a decimal, or an int scaled from the float"
            }
            TypeKind::Array(_) => {
                "an array is mutable; use a tuple, or a str built from its elements"
            }
            TypeKind::Fn(_) => "a function has no equality; key by a name or an int instead",
            TypeKind::Record(_) => {
                "a record is compared by identity at run time; use a tuple, or a struct with the same fields"
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Interface(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => {
                "only int, str, bool, char, bigint, decimal, enums, classes and tuples or structs of those can be keys"
            }
        }
    }
}
