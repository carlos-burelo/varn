//! Questions about a document's types.
//!
//! A `Type` is a handle into the document's type table
//! ([`SemanticDB::types`]); these read the table so features never match on
//! a handle or print one without it.

use varn_checker::types::{CheckerTyId, FunctionType, InternedTypeKind, TyListId};
use varn_checker::Type;
use varn_core::{BuiltinType, LangPrimitive, TypeKind};

use super::SemanticDB;

impl SemanticDB {
    /// `ty` as source text.
    pub fn ty_text(&self, ty: &Type) -> String {
        ty.display(&self.types.borrow(), &self.bind.interner)
            .to_string()
    }

    /// A signature's type (a parameter's, a return type) as source text.
    pub fn id_text(&self, id: CheckerTyId) -> String {
        self.ty_text(&Type(id, false))
    }

    /// The kind of `ty`.
    pub fn ty_kind(&self, ty: &Type) -> InternedTypeKind {
        ty.kind(&self.types.borrow())
    }

    /// The kind of a signature's type.
    pub fn id_kind(&self, id: CheckerTyId) -> InternedTypeKind {
        self.ty_kind(&Type(id, false))
    }

    /// The members of a union, intersection or tuple.
    pub fn ty_list(&self, list: TyListId) -> Vec<Type> {
        let types = self.types.borrow();
        types
            .get_list(list)
            .iter()
            .map(|&id| Type(id, false))
            .collect()
    }

    /// The function shape of `ty`, if it is a function type.
    pub fn fn_shape(&self, ty: &Type) -> Option<FunctionType> {
        let types = self.types.borrow();
        match ty.kind(&types) {
            TypeKind::Fn(f) => Some(types.get_function(f).clone()),
            _ => None,
        }
    }

    /// The signature a call of a `ty` value takes: its own when it is a
    /// function type, the first function member's when it is a union with
    /// one (`((int) => str) | null`).
    pub fn callable_shape(&self, ty: &Type) -> Option<FunctionType> {
        match self.ty_kind(ty) {
            TypeKind::Fn(_) => self.fn_shape(ty),
            TypeKind::Union(list) => self.ty_list(list).iter().find_map(|t| self.fn_shape(t)),
            _ => None,
        }
    }

    /// The type named `name`, interned into this document's table.
    pub fn named_type(&self, name: &str) -> Type {
        crate::workspace::resolver::with_resolver(|r| {
            Type::named(name.to_owned(), r, &mut self.types.borrow_mut())
        })
    }

    /// The type of a primitive.
    pub fn primitive(&self, p: LangPrimitive) -> Type {
        Type::primitive(p, &mut self.types.borrow_mut())
    }

    /// `ty` without its `null`: what a `?.` reads members from.
    pub fn non_null(&self, ty: &Type) -> Type {
        let nullable = ty.is_nullable(&self.types.borrow());
        if nullable {
            ty.non_nullified(&mut self.types.borrow_mut())
        } else {
            *ty
        }
    }

    /// Whether `ty` is `dynamic`.
    pub fn is_dynamic(&self, ty: &Type) -> bool {
        matches!(
            self.ty_kind(ty),
            TypeKind::Primitive(LangPrimitive::Dynamic)
        )
    }

    /// The name a type is known by, when it has one.
    ///
    /// `None` for types that name no declaration — unions, tuples, function
    /// types, `dynamic` — because the callers want a *declaration* to look
    /// members up on, and there is none.
    pub fn decl_name(&self, ty: &Type) -> Option<String> {
        match self.ty_kind(ty) {
            TypeKind::Named(n, _) | TypeKind::Generic(n, _, _) => Some(self.name(n).to_owned()),
            TypeKind::Primitive(LangPrimitive::Dynamic) => None,
            TypeKind::Primitive(p) => Some(p.name().to_owned()),
            TypeKind::Builtin(b) => Some(b.name().to_owned()),
            TypeKind::Array(_) => Some(BuiltinType::Array.name().to_owned()),
            _ => None,
        }
    }
}
