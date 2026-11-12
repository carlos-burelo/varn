use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

fn is_extern_c(func: &ItemFn) -> bool {
    func.sig
        .abi
        .as_ref()
        .is_some_and(|a| a.name.as_ref().is_some_and(|lit| lit.value() == "C"))
}

fn first_is_ptr(func: &ItemFn) -> bool {
    func.sig.inputs.first().is_some_and(|arg| match arg {
        syn::FnArg::Typed(t) => matches!(&*t.ty, syn::Type::Ptr(_)),
        syn::FnArg::Receiver(_) => false,
    })
}

fn has_any_ptr(func: &ItemFn) -> bool {
    func.sig.inputs.iter().any(|arg| match arg {
        syn::FnArg::Typed(t) => matches!(&*t.ty, syn::Type::Ptr(_)),
        syn::FnArg::Receiver(_) => false,
    })
}

pub fn field_of(attr: TokenStream) -> Result<Option<String>, String> {
    if attr.is_empty() {
        return Ok(None);
    }
    let attr2: proc_macro2::TokenStream = attr.into();
    let nv: syn::MetaNameValue = syn::parse2(attr2)
        .map_err(|_| "jit_slow: se admite solo `field = \"nombre\"`".to_owned())?;
    if !nv.path.is_ident("field") {
        return Err("jit_slow: se admite solo `field = \"nombre\"`".to_owned());
    }
    if let syn::Expr::Lit(e) = nv.value {
        if let syn::Lit::Str(s) = e.lit {
            Ok(Some(s.value()))
        } else {
            Err("jit_slow: field debe ser string".to_owned())
        }
    } else {
        Err("jit_slow: field debe ser string".to_owned())
    }
}

pub fn expand(attr: TokenStream, input: TokenStream) -> TokenStream {
    let field = match field_of(attr) {
        Ok(f) => f,
        Err(msg) => return quote! { compile_error!(#msg); }.into(),
    };
    let _ = field;
    let func = parse_macro_input!(input as ItemFn);
    let name = func.sig.ident.to_string();

    if !is_extern_c(&func) {
        let msg = format!("jit_slow: `{name}` debe ser `extern \"C\"`");
        return quote! { compile_error!(#msg); }.into();
    }
    if !(first_is_ptr(&func) || !has_any_ptr(&func)) {
        let msg = format!("jit_slow: `{name}` primer param debe ser `*mut` contexto");
        return quote! { compile_error!(#msg); }.into();
    }

    quote! { #func }.into()
}
