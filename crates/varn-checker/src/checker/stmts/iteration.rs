use super::super::Checker;
use varn_core::{BuiltinType, CoreSum, LangPrimitive, MemberKey, TypeKind};
use varn_sem::bind::BindResult;
use varn_sem::output::ForOfPlan;
use varn_sem::types::Type;

impl Checker<'_> {
    pub(super) fn for_of_element(
        &mut self,
        source: Type,
        is_await: bool,
        bind: &BindResult,
    ) -> Option<(ForOfPlan, Type)> {
        match self.ty_table.get(source.0) {
            TypeKind::Array(inner) => {
                let elem = Type::resolved(inner);
                return Some((ForOfPlan::Collection { method: None, elem }, elem));
            }
            TypeKind::Primitive(LangPrimitive::Str) | TypeKind::TemplateLiteral(_) => {
                let elem = Type::Char;
                return Some((
                    ForOfPlan::Collection {
                        method: Some(MemberKey::Chars),
                        elem,
                    },
                    elem,
                ));
            }
            TypeKind::Tuple(list) => {
                let members = self
                    .ty_table
                    .get_list(list)
                    .iter()
                    .map(|&id| Type::resolved(id))
                    .collect();
                let elem = Type::union(members, std::sync::Arc::make_mut(&mut self.ty_table));
                return Some((ForOfPlan::Collection { method: None, elem }, elem));
            }
            TypeKind::Object(members) | TypeKind::Record(members) => {
                if let [varn_sem::types::ObjectTypeMember::Index {
                    key_ty, value_ty, ..
                }] = self.ty_table.get_object_members(members)
                {
                    let (key, value) = (*key_ty, *value_ty);
                    let table = std::sync::Arc::make_mut(&mut self.ty_table);
                    let pair = table.intern_list(&[key, value]);
                    let entry = Type::resolved(table.intern(TypeKind::Tuple(pair)));
                    let value = Type::resolved(value);
                    return Some((ForOfPlan::Dict { value }, entry));
                }
            }
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::Fn(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => {}
        }
        if let Some(builtin) = source.builtin_of(&self.ty_table) {
            match builtin {
                BuiltinType::Map => {
                    let key = source.builtin_arg(0, &self.ty_table);
                    let value = source.builtin_arg(1, &self.ty_table);
                    let table = std::sync::Arc::make_mut(&mut self.ty_table);
                    let pair = table.intern_list(&[key.0, value.0]);
                    let entry = Type::resolved(table.intern(TypeKind::Tuple(pair)));
                    return Some((
                        ForOfPlan::Collection {
                            method: Some(MemberKey::Entries),
                            elem: entry,
                        },
                        entry,
                    ));
                }
                BuiltinType::Set => {
                    let elem = source.builtin_arg(0, &self.ty_table);
                    return Some((
                        ForOfPlan::Collection {
                            method: Some(MemberKey::Values),
                            elem,
                        },
                        elem,
                    ));
                }
                BuiltinType::Range => {
                    let elem = source.builtin_arg(0, &self.ty_table);
                    if elem == Type::Int || elem.is_dynamic() {
                        return Some((ForOfPlan::IntRange, Type::Int));
                    }
                    return Some((
                        ForOfPlan::Collection {
                            method: Some(MemberKey::ToArray),
                            elem,
                        },
                        elem,
                    ));
                }
                BuiltinType::Array
                | BuiltinType::Bytes
                | BuiltinType::Generator
                | BuiltinType::AsyncGenerator
                | BuiltinType::Task
                | BuiltinType::TaskHandle => {}
            }
        }
        let iter_result = self.method_return(&source, MemberKey::Iter.as_str(), bind);
        let iterator = iter_result.unwrap_or(source);
        let next = self.method_return(&iterator, MemberKey::IterNext.as_str(), bind)?;
        let step = if is_await {
            varn_sem::types::awaited(&next, &self.ty_table)
        } else {
            next
        };
        let (sum, args) = step.core_sum(&self.ty_table)?;
        let elem = (sum == CoreSum::Option)
            .then(|| args.first().copied())
            .flatten()?;
        Some((
            ForOfPlan::Protocol {
                iterator: iter_result,
                next,
                step,
            },
            elem,
        ))
    }

    fn method_return(&mut self, receiver: &Type, name: &str, bind: &BindResult) -> Option<Type> {
        let (member, _) = self.find_member_info(receiver, name, bind)?;
        match self.ty_table.get(member.0) {
            TypeKind::Fn(fid) => Some(Type::resolved(self.ty_table.get_function(fid).return_type)),
            TypeKind::Primitive(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Array(_)
            | TypeKind::Union(_)
            | TypeKind::Intersection(_)
            | TypeKind::Tuple(_)
            | TypeKind::Named(..)
            | TypeKind::Generic(..)
            | TypeKind::TemplateLiteral(_)
            | TypeKind::Object(_)
            | TypeKind::Record(_)
            | TypeKind::Typeof(_)
            | TypeKind::KeyOf(_)
            | TypeKind::IndexedAccess { .. }
            | TypeKind::Mapped { .. }
            | TypeKind::Conditional { .. }
            | TypeKind::Infer(_)
            | TypeKind::EnumVariant { .. }
            | TypeKind::TypePredicate { .. } => None,
        }
    }
}
