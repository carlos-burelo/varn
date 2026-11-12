use proc_macro2::TokenStream as TS2;
use quote::quote;
use syn::Ident;

use crate::contract_members::{Kind, Member};

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

fn primitive_mapped(name: &str) -> Option<Mapped> {
    use vn_core::LangPrimitive as P;
    match P::from_str(name)? {
        P::Int => Some(Mapped::Int),
        P::Float => Some(Mapped::Float),
        P::Bool => Some(Mapped::Bool),
        P::Char => Some(Mapped::Char),
        P::Str => Some(Mapped::Str),
        P::Void => Some(Mapped::Void),
        P::Null | P::BigInt | P::Decimal | P::Never | P::Dynamic | P::Error => None,
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
    quote! { Some(::vn_core::RuntimeKind::#name) }
}

pub(super) fn receiver_mapped(class: &str) -> Mapped {
    if class == vn_core::BuiltinType::Array.name() {
        return Mapped::Array;
    }
    match primitive_mapped(class) {
        Some(Mapped::Str) => Mapped::StrRecv,
        Some(m) => m,
        None => Mapped::Dynamic,
    }
}

pub(super) fn param_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(i64),
        Mapped::Float => quote!(f64),
        Mapped::Bool => quote!(bool),
        Mapped::Char => quote!(char),
        Mapped::Str => quote!(&str),

        Mapped::StrRecv => quote!(&::vn_types::VnStr),
        Mapped::Array => quote!(::vn_types::VnArray),
        Mapped::Dynamic => quote!(::vn_types::VmValue),
        Mapped::Void => quote!(()),
        Mapped::Opt(inner) => {
            let i = param_ty(inner);
            quote!(::core::option::Option<#i>)
        }
    }
}

pub(super) fn owned_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Str | Mapped::StrRecv => quote!(::vn_types::VnStr),
        Mapped::Opt(inner) => {
            let i = owned_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        Mapped::Int
        | Mapped::Float
        | Mapped::Bool
        | Mapped::Char
        | Mapped::Array
        | Mapped::Dynamic
        | Mapped::Void => param_ty(m),
    }
}

pub(super) fn ret_ty(m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(impl ::vn_types::marshal::IntoVm + use<Self>),
        Mapped::Array => quote!(::std::vec::Vec<::vn_types::VmValue>),
        Mapped::Opt(inner) => {
            let i = ret_ty(inner);
            quote!(::core::option::Option<#i>)
        }
        Mapped::Int
        | Mapped::Float
        | Mapped::Bool
        | Mapped::Char
        | Mapped::StrRecv
        | Mapped::Dynamic
        | Mapped::Void => param_ty(m),
    }
}

pub(super) fn call_expr(binding: &Ident, m: &Mapped) -> TS2 {
    match m {
        Mapped::Str => quote!(#binding.as_str()),

        Mapped::StrRecv => quote!(&#binding),
        Mapped::Opt(inner) if matches!(**inner, Mapped::Str) => {
            quote!(#binding.as_ref().map(|s| s.as_str()))
        }
        Mapped::Int
        | Mapped::Float
        | Mapped::Bool
        | Mapped::Char
        | Mapped::Array
        | Mapped::Dynamic
        | Mapped::Void
        | Mapped::Opt(_) => quote!(#binding),
    }
}

pub(super) fn map_to_arg_type_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Int => quote!(::vn_types::ArgType::Int),
        Mapped::Float => quote!(::vn_types::ArgType::Float),
        Mapped::Bool => quote!(::vn_types::ArgType::Bool),
        Mapped::Char => quote!(::vn_types::ArgType::Char),
        Mapped::Str | Mapped::StrRecv => quote!(::vn_types::ArgType::Str),
        Mapped::Array => quote!(::vn_types::ArgType::Generic),
        Mapped::Dynamic => quote!(::vn_types::ArgType::Generic),
        Mapped::Void => quote!(::vn_types::ArgType::Void),
        Mapped::Opt(_) => quote!(::vn_types::ArgType::Generic),
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
        param_tokens.push(quote!(::vn_types::ArgType::Generic));
    }

    for p in m.params.iter().take(7) {
        param_tokens.push(map_to_arg_type_token(&p.mapped));
    }
    while param_tokens.len() < 7 {
        param_tokens.push(quote!(::vn_types::ArgType::Void));
    }
    let count = (m.params.len() + if m.kind == Kind::Method { 1 } else { 0 }) as u8;
    quote! {
        ::vn_types::SignatureDescriptor {
            return_type: #ret_token,
            param_count: #count,
            param_types: [ #(#param_tokens),* ],
        }
    }
}

pub(super) fn default_value_token(m: &Mapped) -> TS2 {
    match m {
        Mapped::Dynamic => quote!(::vn_types::VmValue::null()),
        Mapped::Int
        | Mapped::Float
        | Mapped::Bool
        | Mapped::Char
        | Mapped::Str
        | Mapped::StrRecv
        | Mapped::Array
        | Mapped::Void
        | Mapped::Opt(_) => quote!(Default::default()),
    }
}
