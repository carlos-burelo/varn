use proc_macro2::TokenStream as TS2;
use quote::{format_ident, quote};
use syn::Ident;

use super::mapping::{
    call_expr, default_value_token, is_fast_eligible, owned_ty, param_ty, receiver_mapped, ret_ty,
    signature_token,
};
use super::member::Cx;
use crate::contract_members::{Kind, Member};

pub(super) fn fast_path(
    cx: &Cx,
    m: &Member,
    method_ident: &Ident,
    fast_wrap_ident: &Ident,
) -> (TS2, TS2, TS2) {
    let prefix = cx.prefix;
    let trait_ident = cx.trait_ident;
    let self_ty = cx.self_ty;
    // Fast path wrapper generation
    let is_fast = is_fast_eligible(m);
    if is_fast {
        let mut fast_sig_params = Vec::new();
        let mut fast_call_args = Vec::new();
        let mut fast_decode = Vec::new();
        let fallback = default_value_token(&m.ret);

        if m.kind == Kind::Method {
            fast_sig_params.push(quote!(this: ::varn_types::VmValue));
            let recv = receiver_mapped(&prefix);
            let oty = owned_ty(&recv);
            fast_decode.push(quote! {
                    let __this = match <#oty as ::varn_types::marshal::FromVm>::from_vm(&mut dummy_ctx, this) {
                        Ok(v) => v,
                        Err(_) => return #fallback,
                    };
                });
            fast_call_args.push(call_expr(&format_ident!("__this"), &recv));
        }

        for (i, p) in m.params.iter().enumerate() {
            let pname = format_ident!("__p{}", i);
            let pty = param_ty(&p.mapped);
            fast_sig_params.push(quote!(#pname: #pty));
            fast_call_args.push(call_expr(&pname, &p.mapped));
        }

        let fast_ret = ret_ty(&m.ret);
        let is_fn = m.kind == Kind::Function;

        let call = quote!(<__T>::#method_ident(&mut dummy_ctx, #(#fast_call_args),*));
        let fast_body = if is_fn {
            quote! {
                let mut dummy_ctx = ::varn_types::native::DummyCtx;
                #(#fast_decode)*
                match #call {
                    Ok(v) => v,
                    Err(_) => #fallback,
                }
            }
        } else {
            quote! {
                let mut dummy_ctx = ::varn_types::native::DummyCtx;
                #(#fast_decode)*
                #call
            }
        };

        let wrapper = quote! {
            #[allow(non_snake_case)]
            pub extern "C" fn #fast_wrap_ident<__T: #trait_ident>(
                #(#fast_sig_params),*
            ) -> #fast_ret {
                #fast_body
            }
        };

        let raw_ptr = quote!(#fast_wrap_ident::<#self_ty> as *const u8);
        let sig = signature_token(m);
        (wrapper, raw_ptr, sig)
    } else {
        (
            quote!(),
            quote!(::core::ptr::null()),
            quote!(::varn_types::SignatureDescriptor::empty()),
        )
    }
}
