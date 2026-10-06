





use varn_checker::types::{CheckerTyId, FunctionType, InternedTypeKind, TyListId};
use varn_checker::Type;
use varn_core::{BuiltinType, LangPrimitive, TypeKind};

use super::SemanticDB;

impl SemanticDB {
    
    pub fn ty_text(&self, ty: &Type) -> String {
        ty.display(&self.types.borrow(), &self.bind.interner)
            .to_string()
    }

    
    pub fn id_text(&self, id: CheckerTyId) -> String {
        self.ty_text(&Type::resolved(id))
    }

    
    pub fn ty_kind(&self, ty: &Type) -> InternedTypeKind {
        ty.kind(&self.types.borrow())
    }

    
    pub fn id_kind(&self, id: CheckerTyId) -> InternedTypeKind {
        self.ty_kind(&Type::resolved(id))
    }

    
    pub fn ty_list(&self, list: TyListId) -> Vec<Type> {
        let types = self.types.borrow();
        types
            .get_list(list)
            .iter()
            .map(|&id| Type::resolved(id))
            .collect()
    }

    
    pub fn fn_shape(&self, ty: &Type) -> Option<FunctionType> {
        let types = self.types.borrow();
        match ty.kind(&types) {
            TypeKind::Fn(f) => Some(types.get_function(f).clone()),
            _ => None,
        }
    }

    
    
    
    pub fn callable_shape(&self, ty: &Type) -> Option<FunctionType> {
        match self.ty_kind(ty) {
            TypeKind::Fn(_) => self.fn_shape(ty),
            TypeKind::Union(list) => self.ty_list(list).iter().find_map(|t| self.fn_shape(t)),
            _ => None,
        }
    }

    
    pub fn named_type(&self, name: &str) -> Type {
        Type::named(
            name.to_owned(),
            std::sync::Arc::make_mut(&mut self.types.borrow_mut()),
        )
    }

    
    pub fn primitive(&self, p: LangPrimitive) -> Type {
        Type::primitive(p, std::sync::Arc::make_mut(&mut self.types.borrow_mut()))
    }

    
    pub fn non_null(&self, ty: &Type) -> Type {
        let nullable = ty.is_nullable(&self.types.borrow());
        if nullable {
            ty.non_nullified(std::sync::Arc::make_mut(&mut self.types.borrow_mut()))
        } else {
            *ty
        }
    }

    
    pub fn is_dynamic(&self, ty: &Type) -> bool {
        matches!(
            self.ty_kind(ty),
            TypeKind::Primitive(LangPrimitive::Dynamic)
        )
    }

    
    
    
    
    
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
