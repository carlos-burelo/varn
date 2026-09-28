//! `#[jit_slow]` — marca un slow-path JIT (§4 del spec).
//!
//! Un slow-path se anota una vez en la VM:
//! `#[jit_slow] pub(crate) extern "C" fn jit_throw(…)`. El macro valida la
//! forma (ABI `extern "C"`, primer param `*mut` contexto) y deja la función
//! intacta. Olvidar registrar = error de compilación en el call-site que lo
//! pida, no salto a 0: la tabla futura se genera desde estas anotaciones y
//! `call_helper` rechazará direcciones nulas.
//!
//! Límites honestos de este paso base: la tabla generada y los asserts de
//! aridad/tipo contra call-sites Cranelift llegan con la migración de
//! `helper_abi.rs` (una entrada por commit, Ley 9). Este macro es el contrato
//! único nuevo; la lista vieja queda como legado a borrar.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

pub fn expand(attr: TokenStream, input: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let msg = "jit_slow: no admite argumentos";
        return quote! { compile_error!(#msg); }.into();
    }
    let func = parse_macro_input!(input as ItemFn);
    let name = func.sig.ident.to_string();

    let abi_ok = func
        .sig
        .abi
        .as_ref()
        .is_some_and(|a| a.name.as_ref().is_some_and(|lit| lit.value() == "C"));
    if !abi_ok {
        let msg = format!("jit_slow: `{name}` debe ser `extern \"C\"`");
        return quote! { compile_error!(#msg); }.into();
    }
    let first_is_ptr = func.sig.inputs.first().is_some_and(|arg| match arg {
        syn::FnArg::Typed(t) => matches!(&*t.ty, syn::Type::Ptr(_)),
        syn::FnArg::Receiver(_) => false,
    });
    if !first_is_ptr {
        let msg = format!("jit_slow: `{name}` primer param debe ser `*mut` contexto");
        return quote! { compile_error!(#msg); }.into();
    }

    quote! { #func }.into()
}
