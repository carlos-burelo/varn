use super::ids::{CheckerTyId, FunctionTypeId, InternedTypeKind, ObjectMembersId, TyListId};
use super::table::CheckerTyTable;
use crate::types::{FunctionParam, FunctionType, ObjectTypeMember};
use std::collections::BTreeMap;
use varn_core::{Atom, TypeKind, TypeLiteral};

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TySlice {
    entries: Vec<(CheckerTyId, InternedTypeKind)>,
    lists: Vec<(TyListId, Vec<CheckerTyId>)>,
    functions: Vec<(FunctionTypeId, FunctionType)>,
    object_members: Vec<(ObjectMembersId, Vec<ObjectTypeMember>)>,
}

#[derive(Default)]
struct Reach {
    entries: BTreeMap<CheckerTyId, InternedTypeKind>,
    lists: BTreeMap<TyListId, Vec<CheckerTyId>>,
    functions: BTreeMap<FunctionTypeId, FunctionType>,
    object_members: BTreeMap<ObjectMembersId, Vec<ObjectTypeMember>>,
    pending: Vec<CheckerTyId>,
}

impl Reach {
    fn push_params(&mut self, params: &[FunctionParam]) {
        self.pending.extend(params.iter().map(|p| p.ty));
    }

    fn visit_list(&mut self, table: &CheckerTyTable, id: TyListId) {
        if self.lists.contains_key(&id) {
            return;
        }
        let tys = table.get_list(id).to_vec();
        self.pending.extend(tys.iter().copied());
        self.lists.insert(id, tys);
    }

    fn visit_function(&mut self, table: &CheckerTyTable, id: FunctionTypeId) {
        if self.functions.contains_key(&id) {
            return;
        }
        let f = table.get_function(id).clone();
        self.push_params(&f.params);
        self.pending.push(f.return_type);
        self.functions.insert(id, f);
    }

    fn visit_object(&mut self, table: &CheckerTyTable, id: ObjectMembersId) {
        if self.object_members.contains_key(&id) {
            return;
        }
        let members = table.get_object_members(id).to_vec();
        for m in &members {
            match m {
                ObjectTypeMember::Property { ty, .. } => self.pending.push(*ty),
                ObjectTypeMember::Method {
                    params,
                    return_type,
                    ..
                }
                | ObjectTypeMember::Callable {
                    params,
                    return_type,
                    ..
                } => {
                    self.push_params(params);
                    self.pending.push(*return_type);
                }
                ObjectTypeMember::Index {
                    key_ty, value_ty, ..
                } => {
                    self.pending.push(*key_ty);
                    self.pending.push(*value_ty);
                }
            }
        }
        self.object_members.insert(id, members);
    }

    fn visit(&mut self, table: &CheckerTyTable, id: CheckerTyId) {
        if self.entries.contains_key(&id) || !table.contains(id) {
            return;
        }
        let kind = table.get(id);
        self.entries.insert(id, kind);
        match kind {
            TypeKind::Primitive(_)
            | TypeKind::Builtin(_)
            | TypeKind::Literal(_)
            | TypeKind::This
            | TypeKind::Named(..)
            | TypeKind::Typeof(())
            | TypeKind::Infer(_) => {}
            TypeKind::Array(t) | TypeKind::KeyOf(t) => self.pending.push(t),
            TypeKind::Union(c)
            | TypeKind::Intersection(c)
            | TypeKind::Tuple(c)
            | TypeKind::TemplateLiteral(c)
            | TypeKind::Generic(_, c, _) => self.visit_list(table, c),
            TypeKind::Fn(f) => self.visit_function(table, f),
            TypeKind::Object(o) => self.visit_object(table, o),
            TypeKind::IndexedAccess { object, index } => {
                self.pending.push(object);
                self.pending.push(index);
            }
            TypeKind::Mapped { source, value, .. } => {
                self.pending.push(source);
                self.pending.push(value);
            }
            TypeKind::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.pending.extend([check, extends, true_type, false_type]);
            }
            TypeKind::EnumVariant {
                type_args,
                payload_ty,
                ..
            } => {
                self.visit_list(table, type_args);
                self.pending.push(payload_ty);
            }
            TypeKind::TypePredicate { target_type, .. } => self.pending.push(target_type),
        }
    }
}

fn kind_atoms(kind: &InternedTypeKind, out: &mut Vec<Atom>) {
    match *kind {
        TypeKind::Literal(TypeLiteral::Str(n)) | TypeKind::Infer(n) => out.push(n),
        TypeKind::Named(n, origin) | TypeKind::Generic(n, _, origin) => {
            out.push(n);
            out.extend(origin);
        }
        TypeKind::Mapped { key_var, .. } => out.push(key_var),
        TypeKind::EnumVariant {
            enum_name,
            variant_name,
            ..
        } => {
            out.push(enum_name);
            out.push(variant_name);
        }
        TypeKind::TypePredicate { parameter_name, .. } => out.push(parameter_name),
        TypeKind::Literal(TypeLiteral::Int(_) | TypeLiteral::Bool(_) | TypeLiteral::Char(_))
        | TypeKind::Primitive(_)
        | TypeKind::Builtin(_)
        | TypeKind::This
        | TypeKind::Array(_)
        | TypeKind::Union(_)
        | TypeKind::Intersection(_)
        | TypeKind::Tuple(_)
        | TypeKind::TemplateLiteral(_)
        | TypeKind::Fn(_)
        | TypeKind::Object(_)
        | TypeKind::Typeof(())
        | TypeKind::KeyOf(_)
        | TypeKind::IndexedAccess { .. }
        | TypeKind::Conditional { .. } => {}
    }
}

impl CheckerTyTable {
    pub fn slice(&self, roots: impl IntoIterator<Item = CheckerTyId>) -> TySlice {
        let mut reach = Reach::default();
        reach.pending.extend(roots);
        while let Some(id) = reach.pending.pop() {
            reach.visit(self, id);
        }
        TySlice {
            entries: reach.entries.into_iter().collect(),
            lists: reach.lists.into_iter().collect(),
            functions: reach.functions.into_iter().collect(),
            object_members: reach.object_members.into_iter().collect(),
        }
    }

    pub fn absorb_slice(&mut self, slice: &TySlice) {
        for (id, kind) in &slice.entries {
            if !self.contains(*id) {
                self.delta_entries.insert(*id, *kind);
            }
        }
        for (id, tys) in &slice.lists {
            if self
                .base
                .lists
                .get(id)
                .or_else(|| self.delta_lists.get(id))
                .is_none()
            {
                self.delta_lists.insert(*id, tys.clone());
            }
        }
        for (id, f) in &slice.functions {
            if self
                .base
                .functions
                .get(id)
                .or_else(|| self.delta_functions.get(id))
                .is_none()
            {
                self.delta_functions.insert(*id, f.clone());
            }
        }
        for (id, members) in &slice.object_members {
            if !self.contains_object_members(*id) {
                self.delta_object_members.insert(*id, members.clone());
            }
        }
        self.maybe_freeze();
    }
}

impl TySlice {
    pub fn atoms(&self) -> Vec<Atom> {
        let mut out = Vec::new();
        for (_, kind) in &self.entries {
            kind_atoms(kind, &mut out);
        }
        out
    }
}
