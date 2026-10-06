use proc_macro2::TokenStream as TS2;
use quote::quote;
use syn::Ident;

use crate::contract_members::{Kind, Member};
use varn_core::ast::TypeNode;
use varn_core::kinds::TypeKind;
use varn_core::{AtomInterner, LangPrimitive};

#[derive(Clone)]
pub(crate) enum Mapped {
    Int,
    Float,
    Bool,
    Char,
    Str,
    
    
    
    
    
    
    
    StrRecv,
    Array,
    Dynamic,
    Void,
    Opt(Box<Mapped>),
}





pub(super) fn scalar_mapped(p: LangPrimitive) -> Mapped {
    match p {
        LangPrimitive::Int => Mapped::Int,
        LangPrimitive::Float => Mapped::Float,
        LangPrimitive::Bool => Mapped::Bool,
        LangPrimitive::Char => Mapped::Char,
        LangPrimitive::Str => Mapped::Str,
        LangPrimitive::Void => Mapped::Void,
        LangPrimitive::Null
        | LangPrimitive::BigInt
        | LangPrimitive::Decimal
        | LangPrimitive::Never
        | LangPrimitive::Dynamic => Mapped::Dynamic,
    }
}

pub(crate) fn classify(t: &TypeNode, interner: &AtomInterner) -> Mapped {
    match &t.kind {
        TypeKind::Named(n, _) => LangPrimitive::from_str(interner.resolve(*n))
            .map(scalar_mapped)
            .unwrap_or(Mapped::Dynamic),
        TypeKind::Primitive(varn_core::LangPrimitive::Void) => Mapped::Void,
        TypeKind::TypePredicate { .. } => Mapped::Bool,
        TypeKind::Array(_) => Mapped::Array,
        TypeKind::Union(members) if members.len() == 2 => {
            if matches!(
                members[1].kind,
                TypeKind::Primitive(varn_core::LangPrimitive::Null)
            ) {
                Mapped::Opt(Box::new(classify(&members[0], interner)))
            } else if matches!(
                members[0].kind,
                TypeKind::Primitive(varn_core::LangPrimitive::Null)
            ) {
                Mapped::Opt(Box::new(classify(&members[1], interner)))
            } else {
                Mapped::Dynamic
            }
        }
        _ => Mapped::Dynamic,
    }
}



pub(super) fn mapped_tag_path(m: &Mapped) -> TS2 {
    let name = match m {
        Mapped::Int => quote! { Int },
        Mapped::Float => quote! { Float },
        Mapped::Bool => quote! { Bool },
        Mapped::Char => quote! { Char },
        Mapped::Str | Mapped::StrRecv => quote! { Str },
        Mapped::Array => quote! { Array },
        Mapped::Dynamic | Mapped::Void | Mapped::Opt(_) => return quote! { None },
    };
    quote! { Some(::varn_core::RuntimeKind::#name) }
}

pub(super) fn receiver_mapped(class: &str) -> Mapped {
    if class == varn_core::BuiltinType::Array.name() {
        return Mapped::Array;
    }
    if LangPrimitive::from_str(class) == Some(LangPrimitive::Str) {
        return Mapped::StrRecv;
    }
    LangPrimitive::from_str(class)
        .map(scalar_mapped)
        .unwrap_or(Mapped::Dynamic)
}

pub(super) fn param_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(i64),
        Mapped::Float => quote!(f64),
        Mapped::Bool => quote!(bool),
        Mapped::Char => quote!(char),
        Mapped::Str => quote!(&str),
        
        Mapped::StrRecv => quote!(&::varn_types::VnStr),
        Mapped::Array => quote!(::varn_types::VnArray),
        Mapped::Dynamic => quote!(::varn_types::VmValue),
        Mapped::Void => quote!(()),
        Mapped::Opt(inner) => {
            let i = param_ty(inner);
            quote!(::core::option::Option<#i>)
        }
    }
}

pub(super) fn owned_ty(m: &Mapped) -> TS2 {
    match m {
        
        
        Mapped::Str | Mapped::StrRecv => quote!(::varn_types::VnStr),
        Mapped::Opt(inner) => {
            let i = owned_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        _ => param_ty(m),
    }
}

pub(super) fn ret_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(String),
        Mapped::Array => quote!(::std::vec::Vec<::varn_types::VmValue>),
        Mapped::Opt(inner) => {
            let i = ret_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        _ => param_ty(m),
    }
}

pub(super) fn call_expr(binding: &Ident, m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(#binding.as_str()),
        
        Mapped::StrRecv => quote!(&#binding),
        Mapped::Opt(inner) if matches!(**inner, Mapped::Str) => {
            quote!(#binding.as_ref().map(|s| s.as_str()))
        }
        _ => quote!(#binding),
    }
}

pub(super) fn map_to_arg_type_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(::varn_types::ArgType::Int),
        Mapped::Float => quote!(::varn_types::ArgType::Float),
        Mapped::Bool => quote!(::varn_types::ArgType::Bool),
        Mapped::Char => quote!(::varn_types::ArgType::Char),
        Mapped::Str | Mapped::StrRecv => quote!(::varn_types::ArgType::Str),
        Mapped::Array => quote!(::varn_types::ArgType::Generic),
        Mapped::Dynamic => quote!(::varn_types::ArgType::Generic),
        Mapped::Void => quote!(::varn_types::ArgType::Void),
        Mapped::Opt(_) => quote!(::varn_types::ArgType::Generic),
    }
}

pub(super) fn is_scalar(m: &Mapped) -> bool {
    matches!(
        m,
        Mapped::Int | Mapped::Float | Mapped::Bool | Mapped::Char | Mapped::Void
    )
}

pub(super) fn is_fast_eligible(m: &Member) -> bool {
    if m.fallible {
        return false;
    }
    if !matches!(m.kind, Kind::Function | Kind::StaticMethod) {
        return false;
    }
    if !is_scalar(&m.ret) {
        return false;
    }
    for p in &m.params {
        if p.is_rest || !is_scalar(&p.mapped) {
            return false;
        }
    }
    true
}

pub(super) fn signature_token(m: &Member) -> TS2 {
    let ret_token = map_to_arg_type_token(&m.ret);
    let mut param_tokens = Vec::new();

    if m.kind == Kind::Method {
        param_tokens.push(quote!(::varn_types::ArgType::Generic));
    }

    for p in m.params.iter().take(7) {
        param_tokens.push(map_to_arg_type_token(&p.mapped));
    }
    while param_tokens.len() < 7 {
        param_tokens.push(quote!(::varn_types::ArgType::Void));
    }
    let count = (m.params.len() + if m.kind == Kind::Method { 1 } else { 0 }) as u8;
    quote! {
        ::varn_types::SignatureDescriptor {
            return_type: #ret_token,
            param_count: #count,
            param_types: [ #(#param_tokens),* ],
        }
    }
}

pub(super) fn default_value_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Dynamic => quote!(::varn_types::VmValue::null()),
        _ => quote!(Default::default()),
    }
}
